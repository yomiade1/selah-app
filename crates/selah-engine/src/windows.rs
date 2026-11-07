//! Output windows (winit). They run on the main thread, as macOS requires; rendering and
//! presenting happen on the render thread through the surfaces handed over here.
use std::sync::Arc;

use anyhow::Context;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Window, WindowId};

use crate::engine::Engine;
use crate::outputs::OutputMessage;

/// Where output 1 goes.
#[derive(Clone, Copy, Debug)]
pub enum Placement {
    /// A normal window (development).
    Windowed,
    /// Borderless full screen on display `n` (0-based, in the order the OS lists them).
    Fullscreen(usize),
}

struct OutputApp {
    engine: Arc<Engine>,
    instance: wgpu::Instance,
    placement: Placement,
    windows: Vec<(WindowId, u32, Arc<Window>)>,
}

impl ApplicationHandler for OutputApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }
        let mut attributes = Window::default_attributes()
            .with_title("Selah output 1")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        if let Placement::Fullscreen(index) = self.placement {
            let monitor = event_loop.available_monitors().nth(index);
            if monitor.is_none() {
                tracing::warn!(
                    index,
                    "that display isn't connected; using the main display"
                );
            }
            attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(monitor)));
        }
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                tracing::error!(%error, "couldn't open the output window");
                return;
            }
        };
        let size = window.inner_size();
        match self.instance.create_surface(window.clone()) {
            Ok(surface) => self.engine.output(OutputMessage::Add {
                id: 0,
                surface,
                width: size.width,
                height: size.height,
            }),
            Err(error) => tracing::error!(%error, "couldn't attach the GPU to the output window"),
        }
        self.windows.push((window.id(), 0, window));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some((_, output, _)) = self.windows.iter().find(|(window, _, _)| *window == id) else {
            return;
        };
        match event {
            WindowEvent::Resized(size) => self.engine.output(OutputMessage::Resize {
                id: *output,
                width: size.width,
                height: size.height,
            }),
            // Output windows aren't closed by accident: the screens stay up until Selah quits.
            // Development builds allow closing for convenience.
            WindowEvent::CloseRequested if cfg!(debug_assertions) => event_loop.exit(),
            _ => {}
        }
    }
}

pub fn run(
    engine: Arc<Engine>,
    instance: wgpu::Instance,
    placement: Placement,
) -> anyhow::Result<()> {
    let event_loop = EventLoop::new().context("no display available for output windows")?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = OutputApp {
        engine,
        instance,
        placement,
        windows: Vec::new(),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
