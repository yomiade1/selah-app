//! Engine state and the render thread.
//!
//! Commands only update the state and wake the render thread; the render thread renders the
//! newest scene across every configured output, presents it on a vsync-locked schedule, and
//! broadcasts `FrameShown`. The emergency lane takes the same lock, so it waits behind nothing
//! but a state update, and the render thread picks it up on the next frame (SAFE-03).
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use selah_ipc::FrameSender;
use selah_proto::v1::{
    Actor, ActorOrigin, EmergencyApplied, Envelope, Event, FrameShown, LayerKind, LayerSpec,
    OutputConfig, OutputFrameStatus, OutputRole, PreloadHint, RenderCommand, Reply, Resolution,
    Scene, Status, TestPattern, TransitionDirection, TransitionKind, TransitionSpec, envelope,
    event, layer_spec, render_command, reply,
};
use selah_render::Compositor;
use selah_render::png::decode as decode_png;
use selah_render::transitions::{LayerTransitionBlend, RenderTransitionPlan};
use selah_render::video::Nv12Frame;

use crate::media_playback::{MediaPlaybackManager, default_decode_mode, resolve_media_root};
use crate::outputs::{
    OutputMessage, OutputSurface, PREVIEW_HEIGHT, PREVIEW_WIDTH, PreviewWriter,
    apply_output_message,
};
use crate::scheduler::{FrameScheduler, SchedulerStats};

/// 60 Hz output clock.
pub const FRAME_PERIOD: Duration = Duration::from_nanos(16_666_667);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Offscreen targets only (tests, CI, the installer check).
    Headless,
    /// Output windows on displays (M1.5).
    Windowed,
}

/// One presented output frame on its way to the frames thread.
struct Shown {
    output_index: u32,
    live: bool,
    frame: selah_render::Frame,
    revision: u64,
    shown_ns: u64,
    received_ns: u64,
    /// The emergency lane caused this frame batch: (show logo, revision floor).
    emergency: Option<(bool, u64)>,
}

/// One offscreen staged preview frame on its way to the frames thread (isolated from live; SAFE-06).
struct ShownPreview {
    output_index: u32,
    frame: selah_render::Frame,
    revision: u64,
    shown_ns: u64,
    received_ns: u64,
}

enum FrameWork {
    Live(Vec<Shown>),
    Preview(ShownPreview),
}

struct State {
    scene: Scene,
    preview_scene: Option<Scene>,
    preview_dirty: bool,
    preview_received_ns: u64,
    output_configs: Vec<OutputConfig>,
    explicit_output_configs: bool,
    pending_preload: Vec<PreloadHint>,
    /// Window surfaces waiting for the render thread (windowed mode).
    outputs: Vec<OutputMessage>,
    /// After the emergency lane acts, scenes at or below this revision are stale.
    emergency_floor: Option<u64>,
    dirty: bool,
    received_ns: u64,
    pending_emergency: Option<bool>,
    media_root_override: Option<PathBuf>,
    media_root_dirty: bool,
    last_frames: BTreeMap<u32, FrameShown>,
    last_preview_frame: Option<FrameShown>,
    output_statuses: BTreeMap<u32, OutputFrameStatus>,
    scheduler_stats: SchedulerStats,
    cache_stats: (u64, u64),
    active_video_streams: usize,
    active_transitions: bool,
}

#[derive(Clone)]
struct ActiveOutputTransition {
    from_scene: Scene,
    kind: TransitionKind,
    direction: TransitionDirection,
    spec: TransitionSpec,
    started: Instant,
}

#[derive(Clone)]
struct ActiveLayerTransition {
    kind: LayerKind,
    from_layer: Option<LayerSpec>,
    transition: TransitionKind,
    direction: TransitionDirection,
    spec: TransitionSpec,
    started: Instant,
}

pub struct Engine {
    mode: Mode,
    state: Mutex<State>,
    wake: Condvar,
    clients: Mutex<Vec<FrameSender>>,
    epoch: Instant,
    actor: Actor,
    compositor_name: std::sync::OnceLock<String>,
    next_id: AtomicU64,
    emergency_pending: AtomicBool,
}

fn origin_actor() -> Actor {
    Actor {
        user_id: "selah".into(),
        device_id: "engine".into(),
        origin: ActorOrigin::Engine as i32,
    }
}

fn default_output_config() -> OutputConfig {
    OutputConfig {
        output_index: 0,
        name: "Main Output".into(),
        role: OutputRole::Main as i32,
        resolution: Some(Resolution {
            width: 1920,
            height: 1080,
        }),
        enabled: true,
        display_index: 0,
        fullscreen: false,
        refresh_rate_mhz: 60_000,
        template_id: None,
    }
}

impl Engine {
    /// Start the render thread. Returns the GPU instance too, which windowed mode needs to
    /// create window surfaces on the main thread.
    pub fn start(mode: Mode) -> anyhow::Result<(Arc<Self>, Option<wgpu::Instance>)> {
        let engine = Arc::new(Self {
            mode,
            state: Mutex::new(State {
                outputs: Vec::new(),
                scene: Scene {
                    revision: 0,
                    test_pattern: TestPattern::Unspecified as i32,
                    text: String::new(),
                    cleared: true,
                    resolution: Some(Resolution {
                        width: 1920,
                        height: 1080,
                    }),
                    ..Default::default()
                },
                preview_scene: None,
                preview_dirty: false,
                preview_received_ns: 0,
                output_configs: vec![default_output_config()],
                explicit_output_configs: false,
                pending_preload: Vec::new(),
                emergency_floor: None,
                dirty: true,
                received_ns: 0,
                pending_emergency: None,
                media_root_override: None,
                media_root_dirty: true,
                last_frames: BTreeMap::new(),
                last_preview_frame: None,
                output_statuses: BTreeMap::new(),
                scheduler_stats: SchedulerStats::default(),
                cache_stats: (0, 0),
                active_video_streams: 0,
                active_transitions: false,
            }),
            wake: Condvar::new(),
            clients: Mutex::new(Vec::new()),
            epoch: Instant::now(),
            actor: origin_actor(),
            compositor_name: std::sync::OnceLock::new(),
            next_id: AtomicU64::new(1),
            emergency_pending: AtomicBool::new(false),
        });
        let render = engine.clone();
        // The compositor lives on the render thread (Skia's text objects can't move threads).
        let (ready, started) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("selah-render".into())
            .spawn(move || {
                let choice = selah_render::gpu::AdapterChoice::from_env();
                let (compositor, instance): (Box<dyn Compositor>, Option<wgpu::Instance>) =
                    match selah_render::gpu::GpuCompositor::new(choice) {
                        Ok(gpu) => {
                            let instance = (mode == Mode::Windowed).then(|| gpu.instance().clone());
                            (Box::new(gpu), instance)
                        }
                        Err(error) => {
                            tracing::error!(%error, "no usable GPU; using the CPU compositor (no windows)");
                            (Box::new(selah_render::cpu::CpuCompositor::default()), None)
                        }
                    };
                let _ = render.compositor_name.set(compositor.name().to_owned());
                let _ = ready.send(instance);
                render.render_loop(compositor);
            })?;
        // In headless mode, bind IPC immediately so a reconnecting core attaches during GPU/WARP
        // init and delivers the current scene without waiting behind reconnect backoff.
        let instance = if mode == Mode::Windowed {
            started
                .recv()
                .map_err(|_| anyhow::anyhow!("the render thread didn't start"))?
        } else {
            None
        };
        Ok((engine, instance))
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Hand a window surface (or a resize) to the render thread.
    pub fn output(&self, message: OutputMessage) {
        let mut state = self.lock();
        state.outputs.push(message);
        self.wake.notify_all();
    }

    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    pub fn compositor_name(&self) -> &str {
        self.compositor_name
            .get()
            .map_or("starting", String::as_str)
    }

    #[cfg(test)]
    pub fn scheduler_stats(&self) -> SchedulerStats {
        self.lock().scheduler_stats
    }

    #[cfg(test)]
    pub fn compositor_cache_stats(&self) -> (u64, u64) {
        self.lock().cache_stats
    }

    #[cfg(test)]
    pub fn set_media_root_for_test(&self, root: PathBuf) {
        let mut state = self.lock();
        state.media_root_override = Some(root);
        state.media_root_dirty = true;
        self.wake.notify_all();
    }

    #[cfg(test)]
    pub fn active_video_streams(&self) -> usize {
        self.lock().active_video_streams
    }

    #[cfg(test)]
    pub fn last_preview_frame(&self) -> Option<FrameShown> {
        self.lock().last_preview_frame.clone()
    }

    fn now_ns(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn add_client(&self, sender: FrameSender) {
        // A (re)connecting core learns what's on every output (and staged preview) straight away.
        let (last_frames, last_preview) = {
            let state = self.lock();
            (
                state.last_frames.values().cloned().collect::<Vec<_>>(),
                state.last_preview_frame.clone(),
            )
        };
        for last in last_frames {
            let _ = sender.try_send(self.event(event::Notification::FrameShown(last)));
        }
        if let Some(preview) = last_preview {
            let _ = sender.try_send(self.event(event::Notification::FrameShown(preview)));
        }
        let mut clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());
        clients.retain(|client| !client.is_closed());
        clients.push(sender);
    }

    fn event(&self, notification: event::Notification) -> Envelope {
        Envelope {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            correlation_id: 0,
            actor: Some(self.actor.clone()),
            body: Some(envelope::Body::Event(Event {
                notification: Some(notification),
            })),
        }
    }

    /// Apply a command from core.
    pub fn apply(&self, command: &RenderCommand) {
        let received = self.now_ns();
        let mut state = self.lock();
        match command.action.as_ref() {
            Some(render_command::Action::SetScene(scene)) => {
                if state
                    .emergency_floor
                    .is_some_and(|floor| scene.revision <= floor)
                {
                    tracing::info!(
                        revision = scene.revision,
                        "ignored a scene older than the emergency clear"
                    );
                    return;
                }
                if !scene.preload_items.is_empty() {
                    state
                        .pending_preload
                        .extend(scene.preload_items.iter().cloned());
                }
                let targets = selah_render::scene::resolve_targets(
                    scene,
                    &state.output_configs,
                    state.explicit_output_configs,
                );
                let all_cached = *scene == state.scene
                    && !targets.is_empty()
                    && targets.iter().all(|(idx, (w, h))| {
                        state
                            .last_frames
                            .get(idx)
                            .and_then(|f| f.resolution)
                            .is_some_and(|r| r.width == *w && r.height == *h)
                    });
                if all_cached {
                    let cached: Vec<FrameShown> = targets
                        .iter()
                        .filter_map(|(idx, _)| state.last_frames.get(idx).cloned())
                        .collect();
                    let need_preload = !state.pending_preload.is_empty();
                    if need_preload {
                        self.wake.notify_all();
                    }
                    drop(state);
                    for frame in cached {
                        self.broadcast(event::Notification::FrameShown(frame));
                    }
                    return;
                }
                state.scene = scene.clone();
                state.dirty = true;
                state.received_ns = received;
            }
            Some(render_command::Action::ClearEverything(_)) => {
                state.scene.cleared = true;
                state.scene.text.clear();
                state.scene.outputs.clear();
                state.dirty = true;
                state.received_ns = received;
            }
            Some(render_command::Action::ShowLogo(_)) => {
                // Logos arrive in M4; until then Show logo shows black, like Clear.
                state.scene.cleared = true;
                state.scene.text = "logo".into();
                state.scene.outputs.clear();
                state.dirty = true;
                state.received_ns = received;
            }
            Some(render_command::Action::SetPreviewScene(preview)) => {
                if let Some(scene) = preview.scene.as_ref()
                    && !scene.preload_items.is_empty()
                {
                    state
                        .pending_preload
                        .extend(scene.preload_items.iter().cloned());
                }
                if preview.scene == state.preview_scene {
                    let cached = state.last_preview_frame.clone();
                    let need_preload = !state.pending_preload.is_empty();
                    if need_preload {
                        self.wake.notify_all();
                    }
                    drop(state);
                    if let Some(frame) = cached {
                        self.broadcast(event::Notification::FrameShown(frame));
                    }
                    return;
                }
                state.preview_scene = preview.scene.clone();
                state.preview_dirty = true;
                state.preview_received_ns = received;
            }
            Some(render_command::Action::PreloadItems(preload)) => {
                if preload.items.is_empty() {
                    return;
                }
                state.pending_preload.extend(preload.items.iter().cloned());
                self.wake.notify_all();
                return;
            }
            Some(render_command::Action::SetOutputConfigs(cfg)) => {
                if cfg.outputs.is_empty() {
                    state.output_configs = vec![default_output_config()];
                    state.explicit_output_configs = false;
                } else {
                    state.output_configs = cfg.outputs.clone();
                    state.explicit_output_configs = true;
                }
                state.dirty = true;
                state.received_ns = received;
                if state.preview_scene.is_some() {
                    state.preview_dirty = true;
                    state.preview_received_ns = received;
                }
            }
            None => return,
        }
        self.wake.notify_all();
    }

    /// The emergency lane (SAFE-03): clear every output now, whatever core is doing.
    pub fn emergency(&self, show_logo: bool) {
        let received = self.now_ns();
        self.emergency_pending.store(true, Ordering::Release);
        let mut state = self.lock();
        let floor = state.emergency_floor.unwrap_or(0).max(state.scene.revision);
        state.emergency_floor = Some(floor);
        state.scene.cleared = true;
        state.scene.test_pattern = TestPattern::Unspecified as i32;
        state.scene.outputs.clear();
        state.scene.text = if show_logo {
            "logo".into()
        } else {
            String::new()
        };
        state.dirty = true;
        state.received_ns = received;
        state.pending_emergency = Some(show_logo);
        self.wake.notify_all();
    }

    fn render_loop(self: Arc<Self>, mut compositor: Box<dyn Compositor>) {
        let mut surfaces: Vec<OutputSurface> = Vec::new();
        let mut scheduler = FrameScheduler::with_default_period(self.epoch);
        let mut media_root = resolve_media_root();
        let decode_mode = default_decode_mode();
        let wake_engine = Arc::downgrade(&self);
        let mut playback = MediaPlaybackManager::new(Arc::new(move || {
            if let Some(eng) = wake_engine.upgrade() {
                let mut st = eng.lock();
                st.dirty = true;
                eng.wake.notify_all();
            }
        }));
        compositor.set_media_root(Some(media_root.clone()));

        // Isolated CPU compositor strictly for offscreen staged preview (SAFE-06).
        let mut preview_compositor = selah_render::cpu::CpuCompositor::default();
        preview_compositor.set_media_root(Some(media_root.clone()));

        // Checksums, FrameShown and previews happen on their own thread, in order, so the render
        // thread is free for the next command as soon as a frame is presented.
        let (after, frames) = std::sync::mpsc::channel::<FrameWork>();
        let engine = self.clone();
        let _ = std::thread::Builder::new()
            .name("selah-frames".into())
            .spawn(move || engine.after_frames(frames));

        let mut prev_scene: Option<Scene> = None;
        let mut active_output_transitions: BTreeMap<u32, ActiveOutputTransition> = BTreeMap::new();
        let mut active_layer_transitions: BTreeMap<(u32, LayerKind), ActiveLayerTransition> =
            BTreeMap::new();
        // Upcoming slides to draw ahead of time. They're drawn one per pass and only while no
        // new scene waits, so a Next never queues behind warming the caches (M4-A1).
        let mut warm_queue: VecDeque<PreloadHint> = VecDeque::new();

        loop {
            let (
                scene,
                output_configs,
                explicit_configs,
                received_ns,
                emergency,
                messages,
                dirty,
                tick_due,
                preload,
                media_root_update,
                preview_request,
            ) = {
                let mut state = self.lock();
                let animating = state.active_transitions
                    || selah_render::scene::scene_is_continuously_animated(&state.scene);
                let mut timed_out = false;
                while !state.dirty
                    && !state.preview_dirty
                    && !state.media_root_dirty
                    && state.outputs.is_empty()
                    && state.pending_preload.is_empty()
                    && warm_queue.is_empty()
                {
                    if Arc::strong_count(&self) <= 2 {
                        return;
                    }
                    if let Some(wait) = scheduler.wait_timeout(Instant::now(), animating) {
                        if wait.is_zero() {
                            timed_out = true;
                            break;
                        }
                        let (guard, result) = self
                            .wake
                            .wait_timeout(state, wait)
                            .unwrap_or_else(|p| p.into_inner());
                        state = guard;
                        if result.timed_out() {
                            timed_out = true;
                            break;
                        }
                    } else {
                        let (guard, _) = self
                            .wake
                            .wait_timeout(state, Duration::from_millis(200))
                            .unwrap_or_else(|p| p.into_inner());
                        state = guard;
                    }
                }
                if Arc::strong_count(&self) <= 2 {
                    return;
                }
                let dirty = std::mem::take(&mut state.dirty);
                let preview_dirty = std::mem::take(&mut state.preview_dirty);
                let preview_request =
                    preview_dirty.then(|| (state.preview_scene.clone(), state.preview_received_ns));
                let emergency = state.pending_emergency.take();
                if emergency.is_some() {
                    self.emergency_pending.store(false, Ordering::Release);
                }
                let root_update = if std::mem::take(&mut state.media_root_dirty) {
                    state.media_root_override.clone()
                } else {
                    None
                };
                (
                    state.scene.clone(),
                    state.output_configs.clone(),
                    state.explicit_output_configs,
                    state.received_ns,
                    emergency,
                    std::mem::take(&mut state.outputs),
                    dirty,
                    timed_out,
                    std::mem::take(&mut state.pending_preload),
                    root_update,
                    preview_request,
                )
            };

            queue_warm_hints(&mut warm_queue, preload);

            if let Some(new_root) = media_root_update {
                media_root = new_root;
                compositor.set_media_root(Some(media_root.clone()));
                preview_compositor.set_media_root(Some(media_root.clone()));
            }

            let resized = !messages.is_empty();
            if let Some(gpu) = compositor.as_gpu() {
                for message in messages {
                    apply_output_message(gpu, &mut surfaces, message);
                }
            }

            'live: {
                let should_render = dirty || tick_due;
                if !should_render {
                    if resized {
                        self.present(&mut compositor, &mut surfaces);
                    }
                    break 'live;
                }

                let emergency_active =
                    emergency.is_some() || self.emergency_pending.load(Ordering::Acquire);
                if emergency_active {
                    active_output_transitions.clear();
                    active_layer_transitions.clear();
                    if emergency.is_some() {
                        prev_scene = Some(scene.clone());
                    }
                }

                let targets =
                    selah_render::scene::resolve_targets(&scene, &output_configs, explicit_configs);

                if !emergency_active && prev_scene.as_ref() != Some(&scene) {
                    update_transitions_for_scene(
                        prev_scene.as_ref(),
                        &scene,
                        &targets,
                        &mut active_output_transitions,
                        &mut active_layer_transitions,
                    );
                }

                let retained_hashes = if emergency_active || scene.cleared {
                    HashSet::new()
                } else {
                    collect_transition_media_hashes(
                        &active_output_transitions,
                        &active_layer_transitions,
                    )
                };

                let media_updated = playback.sync_and_push_with_retained(
                    &scene,
                    &media_root,
                    decode_mode,
                    compositor.as_mut(),
                    &retained_hashes,
                );

                let has_active_transitions =
                    !active_output_transitions.is_empty() || !active_layer_transitions.is_empty();

                if !dirty
                    && emergency.is_none()
                    && !media_updated
                    && !has_active_transitions
                    && !scene_needs_non_media_tick(&scene)
                {
                    scheduler.advance_idle(Instant::now());
                    self.lock().active_transitions = false;
                    break 'live;
                }

                if emergency.is_none() && self.emergency_pending.load(Ordering::Acquire) {
                    active_output_transitions.clear();
                    active_layer_transitions.clear();
                    self.lock().active_transitions = false;
                    break 'live;
                }

                let render_started = self.now_ns();
                let render_now = Instant::now();
                let mut rendered = Vec::with_capacity(targets.len());
                let mut render_ok = true;
                for (output_index, (width, height)) in targets {
                    if emergency.is_none() && self.emergency_pending.load(Ordering::Acquire) {
                        render_ok = false;
                        break;
                    }
                    let mut plan = RenderTransitionPlan::default();
                    if let Some(tr) = active_output_transitions.get(&output_index) {
                        let elapsed_ms = render_now
                            .saturating_duration_since(tr.started)
                            .as_millis()
                            .min(u128::from(u32::MAX))
                            as u32;
                        let progress = selah_render::transitions::transition_progress(
                            Some(&tr.spec),
                            elapsed_ms,
                        );
                        if progress < 1.0 {
                            plan.output_transition =
                                Some((tr.from_scene.clone(), tr.kind, tr.direction, progress));
                        }
                    } else {
                        for ((out_idx, _), tr) in &active_layer_transitions {
                            if *out_idx != output_index {
                                continue;
                            }
                            let elapsed_ms = render_now
                                .saturating_duration_since(tr.started)
                                .as_millis()
                                .min(u128::from(u32::MAX))
                                as u32;
                            let progress = selah_render::transitions::transition_progress(
                                Some(&tr.spec),
                                elapsed_ms,
                            );
                            if progress < 1.0 {
                                plan.layer_transitions.push(LayerTransitionBlend {
                                    kind: tr.kind,
                                    from_layer: tr.from_layer.clone(),
                                    transition: tr.transition,
                                    direction: tr.direction,
                                    progress,
                                });
                            }
                        }
                    }
                    match compositor.render_output_with_plan(
                        &scene,
                        output_index,
                        width,
                        height,
                        &plan,
                    ) {
                        Ok(frame) => {
                            let norm = selah_render::scene::normalize_output_scene(
                                &scene,
                                output_index,
                                width,
                                height,
                            );
                            rendered.push((output_index, norm.live, frame));
                        }
                        Err(error) => {
                            tracing::error!(output_index, %error, "render failed; keeping the last frame");
                            render_ok = false;
                            break;
                        }
                    }
                }
                if !render_ok
                    || (emergency.is_none() && self.emergency_pending.load(Ordering::Acquire))
                {
                    break 'live;
                }
                prev_scene = Some(scene.clone());

                let finish_now = Instant::now();
                active_output_transitions.retain(|_, tr| {
                    finish_now.saturating_duration_since(tr.started).as_millis()
                        < u128::from(tr.spec.duration_ms)
                });
                active_layer_transitions.retain(|_, tr| {
                    finish_now.saturating_duration_since(tr.started).as_millis()
                        < u128::from(tr.spec.duration_ms)
                });
                let remaining_transitions =
                    !active_output_transitions.is_empty() || !active_layer_transitions.is_empty();

                tracing::debug!(
                    revision = scene.revision,
                    outputs = rendered.len(),
                    waited_ms = render_started.saturating_sub(received_ns) as f64 / 1e6,
                    render_ms = (self.now_ns() - render_started) as f64 / 1e6,
                    "frame rendered"
                );
                let presented = self.present(&mut compositor, &mut surfaces);
                // A frame is "shown" when it's handed over for display: the swap-chain present with
                // windows, the GPU queue submit when headless (the same moment in both modes).
                let shown_at = presented
                    .or_else(|| compositor.last_submitted())
                    .or_else(|| compositor.last_rendered())
                    .unwrap_or_else(Instant::now);
                scheduler.record_present(shown_at);
                let sched_stats = scheduler.stats();

                let (floor, cache_stats) = {
                    let mut state = self.lock();
                    state.scheduler_stats = sched_stats;
                    state.cache_stats = compositor.cache_stats();
                    state.active_video_streams = playback.active_stream_count();
                    state.active_transitions = remaining_transitions;
                    (state.emergency_floor.unwrap_or(0), state.cache_stats)
                };
                let _ = cache_stats;
                let shown_ns = shown_at.saturating_duration_since(self.epoch).as_nanos() as u64;
                let mut batch = Vec::with_capacity(rendered.len());
                for (idx, (output_index, live, frame)) in rendered.into_iter().enumerate() {
                    batch.push(Shown {
                        output_index,
                        live,
                        frame,
                        revision: scene.revision,
                        shown_ns,
                        received_ns,
                        emergency: if idx == 0 {
                            emergency.map(|show_logo| (show_logo, floor))
                        } else {
                            None
                        },
                    });
                }
                let _ = after.send(FrameWork::Live(batch));
            }

            if let Some((maybe_preview_scene, preview_received_ns)) = preview_request
                && !self.emergency_pending.load(Ordering::Acquire)
            {
                let staged = render_staged_preview(
                    &mut preview_compositor,
                    &media_root,
                    maybe_preview_scene,
                    self.now_ns(),
                    preview_received_ns,
                );
                if let Some(staged) = staged {
                    let _ = after.send(FrameWork::Preview(staged));
                }
            }

            // Warm one upcoming slide, unless a new scene or a clear is already waiting.
            let busy = {
                let state = self.lock();
                state.dirty || state.preview_dirty || self.emergency_pending.load(Ordering::Acquire)
            };
            if !busy && let Some(hint) = warm_queue.pop_front() {
                if let Err(error) = compositor.prerasterise_hints(std::slice::from_ref(&hint)) {
                    tracing::warn!(%error, "prerasterise failed");
                }
                self.lock().cache_stats = compositor.cache_stats();
            }
        }
    }

    /// Runs on the frames thread: checksum, tell core and other clients, publish the preview.
    fn after_frames(self: Arc<Self>, frames: std::sync::mpsc::Receiver<FrameWork>) {
        let mut preview = PreviewWriter::open();
        let mut stage_preview = PreviewWriter::open_stage();
        let mut flash_analyzers: BTreeMap<u32, selah_render::flash::FlashAnalyzer> =
            BTreeMap::new();
        while let Ok(work) = frames.recv() {
            match work {
                FrameWork::Live(batch) => {
                    if batch.is_empty() {
                        continue;
                    }
                    let active_indices: Vec<u32> = batch.iter().map(|s| s.output_index).collect();
                    flash_analyzers.retain(|idx, _| active_indices.contains(idx));
                    let batch_emergency = batch.iter().any(|s| s.emergency.is_some());
                    let mut messages = Vec::with_capacity(batch.len());
                    let mut new_statuses = Vec::with_capacity(batch.len());
                    let mut emergency_applied = None;

                    for shown in &batch {
                        if let Some((show_logo, floor)) = shown.emergency {
                            emergency_applied = Some(EmergencyApplied {
                                show_logo,
                                scene_revision: floor,
                                monotonic_nanoseconds: shown.shown_ns,
                            });
                        }
                        let flash_warning = if batch_emergency || !shown.live {
                            if let Some(analyzer) = flash_analyzers.get_mut(&shown.output_index) {
                                analyzer.reset();
                            }
                            false
                        } else {
                            let analyzer = flash_analyzers.entry(shown.output_index).or_default();
                            analyzer
                                .push_frame(&shown.frame, shown.shown_ns)
                                .flash_warning
                        };
                        let checksum = shown.frame.checksum().to_vec();
                        let res = Resolution {
                            width: shown.frame.width,
                            height: shown.frame.height,
                        };
                        let message = FrameShown {
                            scene_revision: shown.revision,
                            monotonic_nanoseconds: shown.shown_ns,
                            frame_checksum: checksum.clone(),
                            command_received_nanoseconds: shown.received_ns,
                            output_index: shown.output_index,
                            frame_period_nanoseconds: FRAME_PERIOD.as_nanos() as u64,
                            resolution: Some(res),
                            is_preview: false,
                            flash_warning,
                        };
                        let status = OutputFrameStatus {
                            output_index: shown.output_index,
                            scene_revision: shown.revision,
                            frame_checksum: checksum,
                            resolution: Some(res),
                            live: shown.live,
                            flash_warning,
                        };
                        messages.push(message);
                        new_statuses.push(status);
                    }

                    {
                        let mut state = self.lock();
                        state
                            .last_frames
                            .retain(|idx, _| active_indices.contains(idx));
                        state
                            .output_statuses
                            .retain(|idx, _| active_indices.contains(idx));
                        for message in &messages {
                            state
                                .last_frames
                                .insert(message.output_index, message.clone());
                        }
                        for status in new_statuses {
                            state.output_statuses.insert(status.output_index, status);
                        }
                    }

                    if let Some(primary) = batch.iter().find(|s| s.output_index == 0) {
                        preview.publish(&primary.frame, Duration::from_nanos(primary.shown_ns));
                    }
                    if let Some(applied) = emergency_applied {
                        self.broadcast(event::Notification::EmergencyApplied(applied));
                    }
                    for message in messages {
                        self.broadcast(event::Notification::FrameShown(message));
                    }
                }
                FrameWork::Preview(staged) => {
                    stage_preview.publish(&staged.frame, Duration::from_nanos(staged.shown_ns));
                    let checksum = staged.frame.checksum().to_vec();
                    let res = Resolution {
                        width: staged.frame.width,
                        height: staged.frame.height,
                    };
                    let message = FrameShown {
                        scene_revision: staged.revision,
                        monotonic_nanoseconds: staged.shown_ns,
                        frame_checksum: checksum,
                        command_received_nanoseconds: staged.received_ns,
                        output_index: staged.output_index,
                        frame_period_nanoseconds: FRAME_PERIOD.as_nanos() as u64,
                        resolution: Some(res),
                        is_preview: true,
                        flash_warning: false,
                    };
                    self.lock().last_preview_frame = Some(message.clone());
                    self.broadcast(event::Notification::FrameShown(message));
                }
            }
        }
    }

    /// Show the composited textures on every window. Returns when the last present happened.
    fn present(
        &self,
        compositor: &mut Box<dyn Compositor>,
        surfaces: &mut [OutputSurface],
    ) -> Option<Instant> {
        let gpu = compositor.as_gpu()?;
        let mut presented = None;
        for surface in surfaces.iter_mut() {
            if surface.present(gpu) {
                presented = Some(Instant::now());
            }
        }
        presented
    }

    fn broadcast(&self, notification: event::Notification) {
        let message = self.event(notification);
        let clients: Vec<FrameSender> = {
            let mut clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());
            clients.retain(|client| !client.is_closed());
            clients.clone()
        };
        for client in clients {
            if client.try_send(message.clone()).is_err() {
                tracing::debug!("a client isn't keeping up with frame events");
            }
        }
    }

    pub fn status(&self) -> Status {
        let state = self.lock();
        let primary = state.last_frames.get(&0);
        Status {
            connected: true,
            safe_mode: selah_runtime::safe_mode(),
            scene_revision: state.scene.revision,
            engine_connected: true,
            engine_scene_revision: primary.map_or(0, |f| f.scene_revision),
            frame_checksum: primary
                .map(|f| f.frame_checksum.clone())
                .unwrap_or_default(),
            version: selah_runtime::VERSION.into(),
            live: selah_render::scene::scene_is_live(&state.scene),
            renderer: self.compositor_name().to_owned(),
            output_resolution: primary
                .and_then(|f| f.resolution)
                .or(state.scene.resolution),
            output_statuses: state.output_statuses.values().cloned().collect(),
            preview_scene_revision: state.preview_scene.as_ref().map_or(0, |s| s.revision),
            flash_warning: state.output_statuses.values().any(|s| s.flash_warning),
        }
    }

    fn reply(&self, request: &Envelope, result: reply::Result) -> Envelope {
        selah_ipc::reply_to(
            request,
            self.actor.clone(),
            Reply {
                result: Some(result),
            },
        )
    }

    pub fn status_reply(&self, request: &Envelope) -> Envelope {
        self.reply(request, reply::Result::Status(self.status()))
    }

    pub fn ack(&self, request: &Envelope) -> Envelope {
        self.reply(request, reply::Result::Acknowledged(true))
    }

    pub fn refusal(&self, request: &Envelope) -> Envelope {
        selah_ipc::error_to(
            request,
            self.actor.clone(),
            "SEL-SEC-0001",
            "error-not-permitted",
        )
    }

    pub fn unsupported(&self, request: &Envelope) -> Envelope {
        selah_ipc::error_to(
            request,
            self.actor.clone(),
            "SEL-IPC-0001",
            "error-unsupported-request",
        )
    }
}

const MAX_STAGED_POSTER_BYTES: u64 = 16 * 1024 * 1024;

fn is_valid_hex_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn read_validated_staged_poster(thumbs_root: &Path, path: &Path) -> Option<Nv12Frame> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_STAGED_POSTER_BYTES {
        return None;
    }
    let canon = path.canonicalize().ok()?;
    if !canon.starts_with(thumbs_root) {
        return None;
    }
    let file = std::fs::File::open(&canon).ok()?;
    let mut bytes = Vec::with_capacity(meta.len().min(MAX_STAGED_POSTER_BYTES) as usize);
    file.take(MAX_STAGED_POSTER_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_STAGED_POSTER_BYTES {
        return None;
    }
    let frame = decode_png(&bytes).ok()?;
    Nv12Frame::from_rgba(&frame).ok()
}

fn load_staged_video_posters(
    compositor: &mut selah_render::cpu::CpuCompositor,
    media_root: &Path,
    scene: &Scene,
    output_index: u32,
) -> Vec<String> {
    if scene.cleared {
        return Vec::new();
    }
    let Some(out) = scene
        .outputs
        .iter()
        .find(|o| o.output_index == output_index)
    else {
        return Vec::new();
    };
    if out.cleared || out.show_logo {
        return Vec::new();
    }
    let mut hashes = HashSet::new();
    for layer in &out.layers {
        if !layer.enabled {
            continue;
        }
        if let Some(layer_spec::Content::Media(m)) = layer.content.as_ref()
            && m.media_kind == "video"
        {
            let hash = m.content_hash.trim();
            if is_valid_hex_hash(hash) {
                hashes.insert(hash.to_owned());
            }
        }
    }
    if hashes.is_empty() {
        return Vec::new();
    }

    let Some(thumbs_candidate) = media_root.parent().map(|p| p.join("thumbnails")) else {
        return Vec::new();
    };
    let Ok(thumbs_meta) = std::fs::symlink_metadata(&thumbs_candidate) else {
        return Vec::new();
    };
    if thumbs_meta.file_type().is_symlink() || !thumbs_meta.is_dir() {
        return Vec::new();
    }
    let Ok(thumbs_root) = thumbs_candidate.canonicalize() else {
        return Vec::new();
    };

    let mut loaded = Vec::new();
    for hash in hashes {
        let default_path = thumbs_root.join(format!("{hash}_320x180.png"));
        let mut poster = read_validated_staged_poster(&thumbs_root, &default_path);
        if poster.is_none()
            && let Ok(entries) = std::fs::read_dir(&thumbs_root)
        {
            let prefix = format!("{hash}_");
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy();
                if name.starts_with(&prefix) && name.ends_with(".png") {
                    poster = read_validated_staged_poster(&thumbs_root, &entry.path());
                    if poster.is_some() {
                        break;
                    }
                }
            }
        }
        if let Some(nv12) = poster
            && compositor.set_video_for_hash(&hash, Some(nv12)).is_ok()
        {
            loaded.push(hash);
        }
    }
    loaded
}

/// Most upcoming slides the engine keeps waiting to be drawn ahead of time.
const WARM_QUEUE_LIMIT: usize = 64;

/// Adds new upcoming-slide hints to the warm-up queue: a hint already waiting isn't added twice,
/// and the oldest hints go first when the queue is full (newer hints are closer to the operator).
fn queue_warm_hints(queue: &mut VecDeque<PreloadHint>, hints: Vec<PreloadHint>) {
    for hint in hints {
        if queue.contains(&hint) {
            continue;
        }
        if queue.len() >= WARM_QUEUE_LIMIT {
            queue.pop_front();
        }
        queue.push_back(hint);
    }
}

fn render_staged_preview(
    compositor: &mut selah_render::cpu::CpuCompositor,
    media_root: &Path,
    maybe_scene: Option<Scene>,
    shown_ns: u64,
    received_ns: u64,
) -> Option<ShownPreview> {
    let scene = maybe_scene.unwrap_or_else(|| Scene {
        revision: 0,
        cleared: true,
        ..Default::default()
    });
    let output_index = if scene.outputs.iter().any(|o| o.output_index == 0) {
        0
    } else {
        scene.outputs.first().map_or(0, |o| o.output_index)
    };
    let loaded_posters = load_staged_video_posters(compositor, media_root, &scene, output_index);
    let result = compositor.render_output(&scene, output_index, PREVIEW_WIDTH, PREVIEW_HEIGHT);
    for hash in loaded_posters {
        let _ = compositor.set_video_for_hash(&hash, None);
    }
    match result {
        Ok(frame) => Some(ShownPreview {
            output_index,
            frame,
            revision: scene.revision,
            shown_ns,
            received_ns,
        }),
        Err(error) => {
            tracing::warn!(%error, revision = scene.revision, "staged preview render failed");
            None
        }
    }
}

fn find_layer_for_kind(
    scene: Option<&Scene>,
    output_index: u32,
    kind: LayerKind,
) -> Option<&LayerSpec> {
    let scene = scene?;
    if scene.cleared {
        return None;
    }
    let out = scene
        .outputs
        .iter()
        .find(|o| o.output_index == output_index)?;
    if out.cleared || out.show_logo {
        return None;
    }
    out.layers
        .iter()
        .find(|l| selah_render::scene::layer_kind_of(l) == kind)
}

fn layers_functionally_equal(a: Option<&LayerSpec>, b: Option<&LayerSpec>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            x.kind == y.kind
                && x.enabled == y.enabled
                && x.use_opacity == y.use_opacity
                && x.opacity.to_bits() == y.opacity.to_bits()
                && x.effects == y.effects
                && x.content == y.content
        }
        _ => false,
    }
}

fn outputs_functionally_equal(
    prev_scene: Option<&Scene>,
    output_index: u32,
    new_out: &selah_proto::v1::OutputScene,
) -> bool {
    let Some(scene) = prev_scene else {
        return false;
    };
    if scene.cleared {
        return false;
    }
    let Some(old_out) = scene
        .outputs
        .iter()
        .find(|o| o.output_index == output_index)
    else {
        return false;
    };
    if old_out.cleared != new_out.cleared
        || old_out.show_logo != new_out.show_logo
        || old_out.test_pattern != new_out.test_pattern
        || old_out.effects != new_out.effects
        || old_out.layers.len() != new_out.layers.len()
    {
        return false;
    }
    old_out
        .layers
        .iter()
        .zip(&new_out.layers)
        .all(|(a, b)| layers_functionally_equal(Some(a), Some(b)))
}

fn update_transitions_for_scene(
    prev_scene: Option<&Scene>,
    scene: &Scene,
    targets: &[(u32, (u32, u32))],
    active_outputs: &mut BTreeMap<u32, ActiveOutputTransition>,
    active_layers: &mut BTreeMap<(u32, LayerKind), ActiveLayerTransition>,
) {
    if scene.cleared {
        active_outputs.clear();
        active_layers.clear();
        return;
    }
    let from_scene_snapshot = prev_scene.cloned().unwrap_or_else(|| Scene {
        cleared: true,
        ..Default::default()
    });
    let active_output_indices: HashSet<u32> = targets.iter().map(|(i, _)| *i).collect();
    active_outputs.retain(|idx, _| active_output_indices.contains(idx));
    active_layers.retain(|(idx, _), _| active_output_indices.contains(idx));

    for (output_index, _) in targets {
        let Some(out) = scene
            .outputs
            .iter()
            .find(|o| o.output_index == *output_index)
        else {
            active_outputs.remove(output_index);
            active_layers.retain(|(idx, _), _| idx != output_index);
            continue;
        };

        let out_changed = !outputs_functionally_equal(prev_scene, *output_index, out);
        if selah_render::transitions::transition_is_animated(out.transition.as_ref()) {
            if out_changed {
                let spec = out.transition.expect("checked");
                let (kind, direction, _, _) =
                    selah_render::transitions::resolve_transition_spec(Some(&spec));
                active_outputs.insert(
                    *output_index,
                    ActiveOutputTransition {
                        from_scene: from_scene_snapshot.clone(),
                        kind,
                        direction,
                        spec,
                        started: Instant::now(),
                    },
                );
                active_layers.retain(|(idx, _), _| idx != output_index);
            }
            continue;
        }

        if out_changed {
            active_outputs.remove(output_index);
        }
        if out.cleared || out.show_logo {
            active_layers.retain(|(idx, _), _| idx != output_index);
            continue;
        }

        let mut seen_kinds = HashSet::new();
        for layer in &out.layers {
            let lk = selah_render::scene::layer_kind_of(layer);
            if lk == LayerKind::Unspecified {
                continue;
            }
            seen_kinds.insert(lk);
            let prev_layer = find_layer_for_kind(prev_scene, *output_index, lk);
            if layers_functionally_equal(prev_layer, Some(layer)) {
                continue;
            }
            if selah_render::transitions::transition_is_animated(layer.transition.as_ref()) {
                let spec = layer.transition.expect("checked");
                let (transition, direction, _, _) =
                    selah_render::transitions::resolve_transition_spec(Some(&spec));
                let from_layer = prev_layer.and_then(|pl| {
                    (pl.enabled && selah_render::scene::layer_has_content(pl)).then(|| pl.clone())
                });
                active_layers.insert(
                    (*output_index, lk),
                    ActiveLayerTransition {
                        kind: lk,
                        from_layer,
                        transition,
                        direction,
                        spec,
                        started: Instant::now(),
                    },
                );
            } else {
                active_layers.remove(&(*output_index, lk));
            }
        }
        active_layers.retain(|(idx, lk), _| idx != output_index || seen_kinds.contains(lk));
    }
}

fn collect_hashes_from_layer(spec: &LayerSpec, out: &mut HashSet<String>) {
    if !spec.enabled {
        return;
    }
    if let Some(layer_spec::Content::Media(media)) = spec.content.as_ref()
        && media.media_kind == "video"
    {
        let hash = media.content_hash.trim().to_ascii_lowercase();
        if !hash.is_empty() {
            out.insert(hash);
        }
    }
}

fn collect_transition_media_hashes(
    active_outputs: &BTreeMap<u32, ActiveOutputTransition>,
    active_layers: &BTreeMap<(u32, LayerKind), ActiveLayerTransition>,
) -> HashSet<String> {
    let mut hashes = HashSet::new();
    for tr in active_outputs.values() {
        for out in &tr.from_scene.outputs {
            if out.cleared || out.show_logo {
                continue;
            }
            for layer in &out.layers {
                collect_hashes_from_layer(layer, &mut hashes);
            }
        }
    }
    for tr in active_layers.values() {
        if let Some(layer) = tr.from_layer.as_ref() {
            collect_hashes_from_layer(layer, &mut hashes);
        }
    }
    hashes
}

fn scene_needs_non_media_tick(scene: &Scene) -> bool {
    if scene.cleared {
        return false;
    }
    scene.outputs.iter().any(|out| {
        if out.cleared || out.show_logo {
            return false;
        }
        out.layers.iter().any(|layer| {
            if !layer.enabled {
                return false;
            }
            match layer.content.as_ref() {
                Some(selah_proto::v1::layer_spec::Content::Slide(s)) => {
                    s.text.contains("{clock}")
                        || s.text.contains("{timer}")
                        || s.elements_json.contains("{clock}")
                        || s.elements_json.contains("{timer}")
                }
                Some(selah_proto::v1::layer_spec::Content::Props(p)) => {
                    p.label.contains("{clock}")
                        || p.label.contains("{timer}")
                        || p.elements_json.contains("{clock}")
                        || p.elements_json.contains("{timer}")
                }
                Some(selah_proto::v1::layer_spec::Content::Messages(m)) => {
                    m.ticker || m.text.contains("{clock}") || m.text.contains("{timer}")
                }
                _ => false,
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use selah_proto::v1::{LayerKind, LayerSpec, OutputScene, SlideLayerSpec, layer_spec};

    #[test]
    fn warm_queue_skips_repeats_and_keeps_the_newest_hints() {
        let hint = |n: usize| PreloadHint {
            key: format!("slide-{n}"),
            text: format!("Line {n}"),
            ..Default::default()
        };
        let mut queue = VecDeque::new();
        queue_warm_hints(&mut queue, vec![hint(0), hint(1), hint(0)]);
        assert_eq!(queue.len(), 2, "a hint already waiting isn't queued twice");
        queue_warm_hints(&mut queue, (0..WARM_QUEUE_LIMIT + 10).map(hint).collect());
        assert_eq!(queue.len(), WARM_QUEUE_LIMIT);
        assert_eq!(queue.back(), Some(&hint(WARM_QUEUE_LIMIT + 9)));
        assert_eq!(
            queue.front(),
            Some(&hint(10)),
            "the oldest hints were dropped"
        );
    }

    #[tokio::test]
    async fn engine_renders_multiple_outputs_and_clears_all_on_emergency() {
        let (engine, _) = Engine::start(Mode::Headless).expect("engine starts headless");
        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetOutputConfigs(
                selah_proto::v1::SetOutputConfigs {
                    outputs: vec![
                        OutputConfig {
                            output_index: 0,
                            name: "Main".into(),
                            role: OutputRole::Main as i32,
                            resolution: Some(Resolution {
                                width: 320,
                                height: 180,
                            }),
                            enabled: true,
                            ..Default::default()
                        },
                        OutputConfig {
                            output_index: 1,
                            name: "LED".into(),
                            role: OutputRole::Main as i32,
                            resolution: Some(Resolution {
                                width: 640,
                                height: 180,
                            }),
                            enabled: true,
                            ..Default::default()
                        },
                    ],
                },
            )),
        });

        let scene = Scene {
            revision: 3,
            outputs: vec![
                OutputScene {
                    output_index: 0,
                    layers: vec![LayerSpec {
                        kind: LayerKind::Slide as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                            text: "Main Hall".into(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                OutputScene {
                    output_index: 1,
                    layers: vec![LayerSpec {
                        kind: LayerKind::Slide as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                            text: "Wide LED".into(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            preload_items: vec![PreloadHint {
                key: "preload-next".into(),
                output_index: 0,
                resolution: Some(Resolution {
                    width: 320,
                    height: 180,
                }),
                text: "Next Chorus".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetScene(scene)),
        });

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = engine.status();
            if status.output_statuses.len() == 2
                && status
                    .output_statuses
                    .iter()
                    .all(|s| s.scene_revision == 3 && s.live)
            {
                assert_eq!(
                    status.output_statuses[0].resolution,
                    Some(Resolution {
                        width: 320,
                        height: 180
                    })
                );
                assert_eq!(
                    status.output_statuses[1].resolution,
                    Some(Resolution {
                        width: 640,
                        height: 180
                    })
                );
                assert_ne!(
                    status.output_statuses[0].frame_checksum,
                    status.output_statuses[1].frame_checksum
                );
                break;
            }
            assert!(Instant::now() < deadline, "timed out waiting for 2 outputs");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // Wait for post-frame pre-rasterisation and verify cache hit on preloaded "Next Chorus".
        let preload_scene = Scene {
            revision: 4,
            outputs: vec![OutputScene {
                output_index: 0,
                layers: vec![LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        text: "Next Chorus".into(),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetScene(preload_scene)),
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let status = engine.status();
            let (hits, misses) = engine.compositor_cache_stats();
            if status.output_statuses.len() == 2
                && status.output_statuses[0].scene_revision == 4
                && hits >= 1
                && misses >= 1
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for preloaded slide cache hit"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // SAFE-03: emergency clear clears EVERY enabled output immediately.
        engine.emergency(false);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let status = engine.status();
            if status.output_statuses.len() == 2
                && status.output_statuses.iter().all(|s| !s.live)
                && !status.live
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for emergency clear across all outputs"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(engine.scheduler_stats().scheduled_frames >= 2);
    }

    #[tokio::test]
    async fn engine_shares_one_video_player_across_outputs_and_clears_smoothly() {
        let store_dir = tempfile::tempdir().expect("tempdir");
        let clip_src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/media/committed/flash-beep-360p60.mp4");
        let clip_bytes = std::fs::read(&clip_src).expect("read flash-beep clip");
        let hash = blake3::hash(&clip_bytes).to_hex().to_string();
        let shard = store_dir.path().join(&hash[..2]);
        std::fs::create_dir_all(&shard).expect("create shard dir");
        std::fs::write(shard.join(format!("{hash}.mp4")), clip_bytes).expect("write stored clip");

        let (engine, _) = Engine::start(Mode::Headless).expect("engine starts headless");
        engine.set_media_root_for_test(store_dir.path().to_path_buf());
        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetOutputConfigs(
                selah_proto::v1::SetOutputConfigs {
                    outputs: vec![
                        OutputConfig {
                            output_index: 0,
                            name: "Main".into(),
                            role: OutputRole::Main as i32,
                            resolution: Some(Resolution {
                                width: 320,
                                height: 180,
                            }),
                            enabled: true,
                            ..Default::default()
                        },
                        OutputConfig {
                            output_index: 1,
                            name: "Stream".into(),
                            role: OutputRole::Stream as i32,
                            resolution: Some(Resolution {
                                width: 640,
                                height: 180,
                            }),
                            enabled: true,
                            ..Default::default()
                        },
                    ],
                },
            )),
        });

        let make_video_layer = || LayerSpec {
            kind: LayerKind::Media as i32,
            enabled: true,
            content: Some(layer_spec::Content::Media(
                selah_proto::v1::MediaLayerSpec {
                    content_hash: hash.clone(),
                    media_kind: "video".into(),
                    media_extension: "mp4".into(),
                    fit: selah_proto::v1::MediaFitMode::Fill as i32,
                    loop_media: true,
                    muted: true,
                    ..Default::default()
                },
            )),
            ..Default::default()
        };

        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetScene(Scene {
                revision: 10,
                outputs: vec![
                    OutputScene {
                        output_index: 0,
                        layers: vec![make_video_layer()],
                        ..Default::default()
                    },
                    OutputScene {
                        output_index: 1,
                        layers: vec![make_video_layer()],
                        ..Default::default()
                    },
                ],
                ..Default::default()
            })),
        });

        // Two outputs sharing one content hash must spawn at most one MediaPlayer and advance frames.
        let mut distinct_checksums = std::collections::HashSet::new();
        let deadline = Instant::now() + Duration::from_secs(15);
        while distinct_checksums.len() < 3 {
            let status = engine.status();
            if status.output_statuses.len() == 2
                && status
                    .output_statuses
                    .iter()
                    .all(|s| s.scene_revision == 10 && s.live)
            {
                assert!(engine.active_video_streams() <= 1);
                distinct_checksums.insert(status.output_statuses[0].frame_checksum.clone());
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for video background frame progression"
            );
            tokio::time::sleep(Duration::from_millis(16)).await;
        }
        assert_eq!(engine.active_video_streams(), 1);

        // Emergency clear stops visual output immediately (<= 1 frame) and drops active streams.
        engine.emergency(false);
        let clear_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let status = engine.status();
            if status.output_statuses.len() == 2
                && status.output_statuses.iter().all(|s| !s.live)
                && engine.active_video_streams() == 0
            {
                break;
            }
            assert!(
                Instant::now() < clear_deadline,
                "timed out waiting for emergency clear of background video"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn timed_transitions_animate_across_frames_and_emergency_cancels_immediately() {
        // safe: SAFE-03
        use selah_proto::v1::{
            Color, MediaLayerSpec, TransitionDirection, TransitionEasing, TransitionKind,
            TransitionSpec,
        };

        let (engine, _) = Engine::start(Mode::Headless).expect("engine starts headless");
        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetOutputConfigs(
                selah_proto::v1::SetOutputConfigs {
                    outputs: vec![OutputConfig {
                        output_index: 0,
                        name: "Main".into(),
                        role: OutputRole::Main as i32,
                        resolution: Some(Resolution {
                            width: 160,
                            height: 90,
                        }),
                        enabled: true,
                        ..Default::default()
                    }],
                },
            )),
        });

        let blue_scene = Scene {
            revision: 1,
            outputs: vec![OutputScene {
                output_index: 0,
                layers: vec![LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        solid_color: Some(Color {
                            red: 0.0,
                            green: 0.0,
                            blue: 1.0,
                            alpha: 1.0,
                        }),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetScene(blue_scene)),
        });

        let deadline = Instant::now() + Duration::from_secs(15);
        let blue_checksum = loop {
            let st = engine.status();
            if st.output_statuses.len() == 1
                && st.output_statuses[0].scene_revision == 1
                && st.output_statuses[0].live
            {
                break st.output_statuses[0].frame_checksum.clone();
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for blue frame"
            );
            tokio::time::sleep(Duration::from_millis(8)).await;
        };

        // Start a 260 ms output Fade to green and collect intermediate distinct checksums.
        let green_scene = Scene {
            revision: 2,
            outputs: vec![OutputScene {
                output_index: 0,
                transition: Some(TransitionSpec {
                    kind: TransitionKind::Fade as i32,
                    direction: TransitionDirection::Left as i32,
                    easing: TransitionEasing::Linear as i32,
                    duration_ms: 260,
                }),
                layers: vec![LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        solid_color: Some(Color {
                            red: 0.0,
                            green: 1.0,
                            blue: 0.0,
                            alpha: 1.0,
                        }),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetScene(green_scene)),
        });

        let mut seen_checksums = std::collections::HashSet::new();
        let wait_until = Instant::now() + Duration::from_millis(450);
        while Instant::now() < wait_until {
            let st = engine.status();
            if st.engine_scene_revision == 2 && !st.frame_checksum.is_empty() {
                seen_checksums.insert(st.frame_checksum);
            }
            tokio::time::sleep(Duration::from_millis(8)).await;
        }
        assert!(
            seen_checksums.len() >= 3,
            "expected >= 3 intermediate transition checksums, got {}",
            seen_checksums.len()
        );
        assert_ne!(
            engine.status().frame_checksum,
            blue_checksum,
            "completed fade must settle on destination green frame"
        );

        // Once completed, the scheduler must return to idle (no runaway polling).
        let frames_after_settle = engine.scheduler_stats().scheduled_frames;
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(
            engine.scheduler_stats().scheduled_frames,
            frames_after_settle,
            "engine must stop waking at 60 Hz after finite transition finishes"
        );

        // SAFE-03: emergency clear during a long 3000 ms transition cancels immediately.
        let slow_red = Scene {
            revision: 3,
            outputs: vec![OutputScene {
                output_index: 0,
                transition: Some(TransitionSpec {
                    kind: TransitionKind::Wipe as i32,
                    direction: TransitionDirection::Right as i32,
                    easing: TransitionEasing::EaseInOut as i32,
                    duration_ms: 3000,
                }),
                layers: vec![LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        solid_color: Some(Color {
                            red: 1.0,
                            green: 0.0,
                            blue: 0.0,
                            alpha: 1.0,
                        }),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetScene(slow_red)),
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        engine.emergency(false);

        let clear_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let st = engine.status();
            if !st.live && st.output_statuses.iter().all(|s| !s.live) {
                break;
            }
            assert!(
                Instant::now() < clear_deadline,
                "timed out waiting for emergency cancellation of slow transition"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // Must remain cleared and idle even though 3000 ms transition had 2950 ms remaining.
        let frames_after_clear = engine.scheduler_stats().scheduled_frames;
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(!engine.status().live);
        assert_eq!(
            engine.scheduler_stats().scheduled_frames,
            frames_after_clear
        );
    }

    #[tokio::test]
    async fn engine_stages_preview_ring_without_affecting_live_outputs() {
        // safe: SAFE-06
        let lib_dir = tempfile::tempdir().expect("tempdir");
        let media_dir = lib_dir.path().join("media");
        let thumbs_dir = lib_dir.path().join("thumbnails");
        std::fs::create_dir_all(&media_dir).expect("create media dir");
        std::fs::create_dir_all(&thumbs_dir).expect("create thumbnails dir");

        // Pre-seed a video poster PNG for a dummy clip hash so staged preview exercises poster lookup.
        let poster_hash = "a".repeat(64);
        let mut poster_frame = selah_render::Frame::new(320, 180);
        for px in poster_frame.pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&[20, 140, 210, 255]);
        }
        let poster_png = selah_render::png::encode(&poster_frame).expect("encode poster");
        std::fs::write(
            thumbs_dir.join(format!("{poster_hash}_320x180.png")),
            poster_png,
        )
        .expect("write poster");

        let (engine, _) = Engine::start(Mode::Headless).expect("engine starts headless");
        engine.set_media_root_for_test(media_dir);
        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetOutputConfigs(
                selah_proto::v1::SetOutputConfigs {
                    outputs: vec![OutputConfig {
                        output_index: 0,
                        name: "Main".into(),
                        role: OutputRole::Main as i32,
                        resolution: Some(Resolution {
                            width: PREVIEW_WIDTH,
                            height: PREVIEW_HEIGHT,
                        }),
                        enabled: true,
                        ..Default::default()
                    }],
                },
            )),
        });

        // Wait for initial cleared live frame (output 0, revision 0).
        let deadline = Instant::now() + Duration::from_secs(15);
        let cleared_live_checksum = loop {
            let st = engine.status();
            if !st.frame_checksum.is_empty() && !st.live {
                break st.frame_checksum;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for initial engine status"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };

        // Stage a preview scene with video poster + slide text while live remains cleared.
        let preview_scene = Scene {
            revision: 7,
            is_preview: true,
            outputs: vec![OutputScene {
                output_index: 0,
                layers: vec![
                    LayerSpec {
                        kind: LayerKind::Media as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Media(
                            selah_proto::v1::MediaLayerSpec {
                                content_hash: poster_hash,
                                media_kind: "video".into(),
                                media_extension: "mp4".into(),
                                ..Default::default()
                            },
                        )),
                        ..Default::default()
                    },
                    LayerSpec {
                        kind: LayerKind::Slide as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                            text: "Next Hymn".into(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetPreviewScene(
                selah_proto::v1::SetPreviewScene {
                    scene: Some(preview_scene),
                },
            )),
        });

        let preview_deadline = Instant::now() + Duration::from_secs(5);
        let preview_event = loop {
            if let Some(frame) = engine.last_preview_frame()
                && frame.is_preview
                && frame.scene_revision == 7
            {
                break frame;
            }
            assert!(
                Instant::now() < preview_deadline,
                "timed out waiting for FrameShown {{ is_preview: true }}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };

        assert_eq!(preview_event.output_index, 0);
        assert!(!preview_event.flash_warning);
        assert_eq!(
            preview_event.resolution,
            Some(Resolution {
                width: PREVIEW_WIDTH,
                height: PREVIEW_HEIGHT,
            })
        );
        assert_ne!(preview_event.frame_checksum, cleared_live_checksum);

        // Live state and checksum must remain untouched (SAFE-06).
        let st = engine.status();
        assert!(!st.live);
        assert_eq!(st.scene_revision, 0);
        assert_eq!(st.preview_scene_revision, 7);
        assert_eq!(st.frame_checksum, cleared_live_checksum);

        // Now go live with a different scene, then stage a second preview scene:
        // live checksum must stay pinned to the live scene while staged checksum updates.
        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetScene(Scene {
                revision: 2,
                outputs: vec![OutputScene {
                    output_index: 0,
                    layers: vec![LayerSpec {
                        kind: LayerKind::Slide as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                            text: "Live Verse".into(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            })),
        });
        let live_deadline = Instant::now() + Duration::from_secs(5);
        let live_checksum = loop {
            let s = engine.status();
            if s.live && s.engine_scene_revision == 2 {
                break s.frame_checksum;
            }
            assert!(
                Instant::now() < live_deadline,
                "timed out waiting for live revision 2"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };

        engine.apply(&RenderCommand {
            action: Some(render_command::Action::SetPreviewScene(
                selah_proto::v1::SetPreviewScene {
                    scene: Some(Scene {
                        revision: 8,
                        is_preview: true,
                        outputs: vec![OutputScene {
                            output_index: 0,
                            layers: vec![LayerSpec {
                                kind: LayerKind::Slide as i32,
                                enabled: true,
                                content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                                    text: "Second Preview".into(),
                                    ..Default::default()
                                })),
                                ..Default::default()
                            }],
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                },
            )),
        });

        let preview2_deadline = Instant::now() + Duration::from_secs(5);
        let preview2 = loop {
            if let Some(frame) = engine.last_preview_frame()
                && frame.is_preview
                && frame.scene_revision == 8
            {
                break frame;
            }
            assert!(
                Instant::now() < preview2_deadline,
                "timed out waiting for second staged preview FrameShown"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };

        assert_ne!(preview2.frame_checksum, preview_event.frame_checksum);
        assert_ne!(preview2.frame_checksum, live_checksum);
        let st_after = engine.status();
        assert!(st_after.live);
        assert_eq!(st_after.engine_scene_revision, 2);
        assert_eq!(st_after.frame_checksum, live_checksum);
        assert_eq!(st_after.preview_scene_revision, 8);
    }
}
