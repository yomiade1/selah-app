//! Engine state and the render thread.
//!
//! Commands only update the state and wake the render thread; the render thread renders the
//! newest scene across every configured output, presents it on a vsync-locked schedule, and
//! broadcasts `FrameShown`. The emergency lane takes the same lock, so it waits behind nothing
//! but a state update, and the render thread picks it up on the next frame (SAFE-03).
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use selah_ipc::FrameSender;
use selah_proto::v1::{
    Actor, ActorOrigin, EmergencyApplied, Envelope, Event, FrameShown, OutputConfig,
    OutputFrameStatus, OutputRole, PreloadHint, RenderCommand, Reply, Resolution, Scene, Status,
    TestPattern, envelope, event, render_command, reply,
};
use selah_render::Compositor;

use crate::outputs::{OutputMessage, OutputSurface, PreviewWriter, apply_output_message};
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

struct State {
    scene: Scene,
    preview_scene: Option<Scene>,
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
    last_frames: BTreeMap<u32, FrameShown>,
    output_statuses: BTreeMap<u32, OutputFrameStatus>,
    scheduler_stats: SchedulerStats,
    cache_stats: (u64, u64),
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
                output_configs: vec![default_output_config()],
                explicit_output_configs: false,
                pending_preload: Vec::new(),
                emergency_floor: None,
                dirty: true,
                received_ns: 0,
                pending_emergency: None,
                last_frames: BTreeMap::new(),
                output_statuses: BTreeMap::new(),
                scheduler_stats: SchedulerStats::default(),
                cache_stats: (0, 0),
            }),
            wake: Condvar::new(),
            clients: Mutex::new(Vec::new()),
            epoch: Instant::now(),
            actor: origin_actor(),
            compositor_name: std::sync::OnceLock::new(),
            next_id: AtomicU64::new(1),
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

    fn now_ns(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn add_client(&self, sender: FrameSender) {
        // A (re)connecting core learns what's on every output straight away.
        let last_frames: Vec<FrameShown> = self.lock().last_frames.values().cloned().collect();
        for last in last_frames {
            let _ = sender.try_send(self.event(event::Notification::FrameShown(last)));
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
                state.preview_scene = preview.scene.clone();
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
            }
            None => return,
        }
        self.wake.notify_all();
    }

    /// The emergency lane (SAFE-03): clear every output now, whatever core is doing.
    pub fn emergency(&self, show_logo: bool) {
        let received = self.now_ns();
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
        // Checksums, FrameShown and previews happen on their own thread, in order, so the render
        // thread is free for the next command as soon as a frame is presented.
        let (after, frames) = std::sync::mpsc::channel::<Vec<Shown>>();
        let engine = self.clone();
        let _ = std::thread::Builder::new()
            .name("selah-frames".into())
            .spawn(move || engine.after_frames(frames));
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
            ) = {
                let mut state = self.lock();
                let animating = selah_render::scene::scene_is_time_varying(&state.scene);
                let mut timed_out = false;
                while !state.dirty
                    && state.outputs.is_empty()
                    && state.pending_preload.is_empty()
                {
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
                        state = self.wake.wait(state).unwrap_or_else(|p| p.into_inner());
                    }
                }
                let dirty = std::mem::take(&mut state.dirty);
                (
                    state.scene.clone(),
                    state.output_configs.clone(),
                    state.explicit_output_configs,
                    state.received_ns,
                    state.pending_emergency.take(),
                    std::mem::take(&mut state.outputs),
                    dirty,
                    timed_out,
                    std::mem::take(&mut state.pending_preload),
                )
            };

            let resized = !messages.is_empty();
            if let Some(gpu) = compositor.as_gpu() {
                for message in messages {
                    apply_output_message(gpu, &mut surfaces, message);
                }
            }

            let should_render = dirty || tick_due;
            if !should_render {
                if resized {
                    self.present(&mut compositor, &mut surfaces);
                }
                if !preload.is_empty() {
                    if let Err(error) = compositor.prerasterise_hints(&preload) {
                        tracing::warn!(%error, "prerasterise failed");
                    }
                    self.lock().cache_stats = compositor.cache_stats();
                }
                continue;
            }

            let targets =
                selah_render::scene::resolve_targets(&scene, &output_configs, explicit_configs);
            let render_started = self.now_ns();
            let mut rendered = Vec::with_capacity(targets.len());
            let mut render_ok = true;
            for (output_index, (width, height)) in targets {
                match compositor.render_output(&scene, output_index, width, height) {
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
            if !render_ok {
                continue;
            }

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
            let _ = after.send(batch);

            // Warm caches for upcoming items after the live frame is handed off.
            if !preload.is_empty() {
                if let Err(error) = compositor.prerasterise_hints(&preload) {
                    tracing::warn!(%error, "post-frame prerasterise failed");
                }
                self.lock().cache_stats = compositor.cache_stats();
            }
        }
    }

    /// Runs on the frames thread: checksum, tell core and other clients, publish the preview.
    fn after_frames(self: Arc<Self>, frames: std::sync::mpsc::Receiver<Vec<Shown>>) {
        let mut preview = PreviewWriter::open();
        while let Ok(batch) = frames.recv() {
            if batch.is_empty() {
                continue;
            }
            let active_indices: Vec<u32> = batch.iter().map(|s| s.output_index).collect();
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
                };
                let status = OutputFrameStatus {
                    output_index: shown.output_index,
                    scene_revision: shown.revision,
                    frame_checksum: checksum,
                    resolution: Some(res),
                    live: shown.live,
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

            if let Some(applied) = emergency_applied {
                self.broadcast(event::Notification::EmergencyApplied(applied));
            }
            for message in messages {
                self.broadcast(event::Notification::FrameShown(message));
            }
            if let Some(primary) = batch.iter().find(|s| s.output_index == 0) {
                preview.publish(&primary.frame, Duration::from_nanos(primary.shown_ns));
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
            frame_checksum: primary.map(|f| f.frame_checksum.clone()).unwrap_or_default(),
            version: selah_runtime::VERSION.into(),
            live: selah_render::scene::scene_is_live(&state.scene),
            renderer: self.compositor_name().to_owned(),
            output_resolution: primary
                .and_then(|f| f.resolution)
                .or(state.scene.resolution),
            output_statuses: state.output_statuses.values().cloned().collect(),
            preview_scene_revision: state.preview_scene.as_ref().map_or(0, |s| s.revision),
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

#[cfg(test)]
mod tests {
    use super::*;
    use selah_proto::v1::{LayerKind, LayerSpec, OutputScene, SlideLayerSpec, layer_spec};

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
}
