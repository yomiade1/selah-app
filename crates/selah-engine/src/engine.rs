//! Engine state and the render thread.
//!
//! Commands only update the state and wake the render thread; the render thread renders the
//! newest scene, presents it, and broadcasts `FrameShown`. The emergency lane takes the same
//! lock, so it waits behind nothing but a state update, and the render thread picks it up on
//! the next frame (SAFE-03).
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use selah_ipc::FrameSender;
use selah_proto::v1::{
    Actor, ActorOrigin, EmergencyApplied, Envelope, Event, FrameShown, RenderCommand, Reply,
    Resolution, Scene, Status, TestPattern, envelope, event, render_command, reply,
};
use selah_render::Compositor;

use crate::outputs::{OutputMessage, OutputSurface, PreviewWriter, apply_output_message};

/// 60 Hz output clock.
pub const FRAME_PERIOD: Duration = Duration::from_nanos(16_666_667);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Offscreen targets only (tests, CI, the installer check).
    Headless,
    /// Output windows on displays (M1.5).
    Windowed,
}

/// A presented frame on its way to the frames thread.
struct Shown {
    frame: selah_render::Frame,
    revision: u64,
    shown_ns: u64,
    received_ns: u64,
    /// The emergency lane caused this frame: (show logo, revision floor).
    emergency: Option<(bool, u64)>,
}

struct State {
    scene: Scene,
    /// Window surfaces waiting for the render thread (windowed mode).
    outputs: Vec<OutputMessage>,
    /// After the emergency lane acts, scenes at or below this revision are stale.
    emergency_floor: Option<u64>,
    dirty: bool,
    received_ns: u64,
    pending_emergency: Option<bool>,
    last: Option<FrameShown>,
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

fn is_live(scene: &Scene) -> bool {
    !scene.cleared
        && (scene.test_pattern != TestPattern::Unspecified as i32 || !scene.text.is_empty())
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
                },
                emergency_floor: None,
                dirty: true,
                received_ns: 0,
                pending_emergency: None,
                last: None,
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
                            let instance = gpu.instance().clone();
                            (Box::new(gpu), Some(instance))
                        }
                        Err(error) => {
                            tracing::error!(%error, "no usable GPU; using the CPU compositor (no windows)");
                            (Box::new(selah_render::cpu::CpuCompositor::default()), None)
                        }
                    };
                let _ = ready.send((compositor.name().to_owned(), instance));
                render.render_loop(compositor);
            })?;
        let (name, instance) = started
            .recv()
            .map_err(|_| anyhow::anyhow!("the render thread didn't start"))?;
        let _ = engine.compositor_name.set(name);
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

    fn now_ns(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn add_client(&self, sender: FrameSender) {
        // A (re)connecting core learns what's on the screens straight away.
        if let Some(last) = self.lock().last.clone() {
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
                if *scene == state.scene
                    && let Some(last) = state.last.clone()
                {
                    // Already showing it (a resync): confirm instead of re-rendering.
                    drop(state);
                    self.broadcast(event::Notification::FrameShown(last));
                    return;
                }
                state.scene = scene.clone();
            }
            Some(render_command::Action::ClearEverything(_)) => {
                state.scene.cleared = true;
                state.scene.text.clear();
            }
            Some(render_command::Action::ShowLogo(_)) => {
                // Logos arrive in M4; until then Show logo shows black, like Clear.
                state.scene.cleared = true;
                state.scene.text = "logo".into();
            }
            None => return,
        }
        state.dirty = true;
        state.received_ns = received;
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
        // Checksums, FrameShown and previews happen on their own thread, in order, so the render
        // thread is free for the next command as soon as a frame is presented.
        let (after, frames) = std::sync::mpsc::channel::<Shown>();
        let engine = self.clone();
        let _ = std::thread::Builder::new()
            .name("selah-frames".into())
            .spawn(move || engine.after_frames(frames));
        loop {
            let (scene, received_ns, emergency, messages, dirty) = {
                let mut state = self.lock();
                while !state.dirty && state.outputs.is_empty() {
                    state = self
                        .wake
                        .wait_timeout(state, FRAME_PERIOD)
                        .map(|(guard, _)| guard)
                        .unwrap_or_else(|p| p.into_inner().0);
                }
                let dirty = std::mem::take(&mut state.dirty);
                (
                    state.scene.clone(),
                    state.received_ns,
                    state.pending_emergency.take(),
                    std::mem::take(&mut state.outputs),
                    dirty,
                )
            };
            let resized = !messages.is_empty();
            if let Some(gpu) = compositor.as_gpu() {
                for message in messages {
                    apply_output_message(gpu, &mut surfaces, message);
                }
            }
            if !dirty {
                if resized {
                    self.present(&mut compositor, &mut surfaces);
                }
                continue;
            }
            let (width, height) = scene
                .resolution
                .map(|r| (r.width.clamp(16, 7680), r.height.clamp(16, 4320)))
                .unwrap_or((1920, 1080));
            let render_started = self.now_ns();
            let frame = match compositor.render(&scene, width, height) {
                Ok(frame) => frame,
                Err(error) => {
                    tracing::error!(%error, "render failed; keeping the last frame");
                    continue;
                }
            };
            tracing::debug!(
                revision = scene.revision,
                waited_ms = render_started.saturating_sub(received_ns) as f64 / 1e6,
                render_ms = (self.now_ns() - render_started) as f64 / 1e6,
                "frame rendered"
            );
            let presented = self.present(&mut compositor, &mut surfaces);
            // Headless, a frame is "shown" when the GPU has finished it; with windows, when
            // it has been handed to the swap chain.
            let shown_at = presented
                .or_else(|| compositor.last_rendered())
                .unwrap_or_else(Instant::now);
            let floor = self.lock().emergency_floor.unwrap_or(0);
            let _ = after.send(Shown {
                frame,
                revision: scene.revision,
                shown_ns: shown_at.saturating_duration_since(self.epoch).as_nanos() as u64,
                received_ns,
                emergency: emergency.map(|show_logo| (show_logo, floor)),
            });
        }
    }

    /// Runs on the frames thread: checksum, tell core and other clients, publish the preview.
    fn after_frames(self: Arc<Self>, frames: std::sync::mpsc::Receiver<Shown>) {
        let mut preview = PreviewWriter::open();
        while let Ok(shown) = frames.recv() {
            let message = FrameShown {
                scene_revision: shown.revision,
                monotonic_nanoseconds: shown.shown_ns,
                frame_checksum: shown.frame.checksum().to_vec(),
                command_received_nanoseconds: shown.received_ns,
                output_index: 0,
                frame_period_nanoseconds: FRAME_PERIOD.as_nanos() as u64,
            };
            self.lock().last = Some(message.clone());
            if let Some((show_logo, floor)) = shown.emergency {
                self.broadcast(event::Notification::EmergencyApplied(EmergencyApplied {
                    show_logo,
                    scene_revision: floor,
                    monotonic_nanoseconds: shown.shown_ns,
                }));
            }
            self.broadcast(event::Notification::FrameShown(message));
            preview.publish(&shown.frame, Duration::from_nanos(shown.shown_ns));
        }
    }

    /// Show the composited texture on every window. Returns when the last present happened.
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
        Status {
            connected: true,
            safe_mode: selah_runtime::safe_mode(),
            scene_revision: state.scene.revision,
            engine_connected: true,
            engine_scene_revision: state.last.as_ref().map_or(0, |f| f.scene_revision),
            frame_checksum: state
                .last
                .as_ref()
                .map(|f| f.frame_checksum.clone())
                .unwrap_or_default(),
            version: selah_runtime::VERSION.into(),
            live: is_live(&state.scene),
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
