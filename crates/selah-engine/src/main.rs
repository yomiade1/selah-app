#![forbid(unsafe_code)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! `selah-engine`: renders scenes to the outputs and holds the last frame whatever happens to
//! core or the UI (ADR-001). Two endpoints:
//! - **engine**: render commands from core, status queries;
//! - **emergency** (SAFE-03, ADR-026): only Clear everything and Show logo, from the UI,
//!   handled ahead of everything else so it works even if core is frozen.
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use selah_ipc::{Connection, Endpoint, Listener, Service};
use selah_proto::v1::{ActorOrigin, command, envelope, query, render_command};

mod engine;
mod outputs;
mod windows;

use engine::{Engine, Mode};

async fn serve_engine(engine: Arc<Engine>, connection: Connection) {
    let Connection {
        mut reader, sender, ..
    } = connection;
    engine.add_client(sender.clone());
    while let Ok(request) = reader.next().await {
        let origin = request
            .actor
            .as_ref()
            .and_then(|actor| ActorOrigin::try_from(actor.origin).ok())
            .unwrap_or(ActorOrigin::Unspecified);
        match request.body.as_ref() {
            Some(envelope::Body::Command(command)) => match command.action.as_ref() {
                // Only core drives the engine (ADR-001, SEC-14).
                Some(command::Action::Render(render)) if origin == ActorOrigin::Core => {
                    engine.apply(render);
                }
                _ => {
                    let _ = sender.send(engine.refusal(&request)).await;
                }
            },
            Some(envelope::Body::Query(query)) => {
                if let Some(query::Request::GetStatus(_)) = query.request {
                    let _ = sender.send(engine.status_reply(&request)).await;
                } else {
                    let _ = sender.send(engine.unsupported(&request)).await;
                }
            }
            _ => {}
        }
    }
}

/// Accepts exactly two commands from the UI. Anything else closes the connection.
async fn serve_emergency(engine: Arc<Engine>, connection: Connection) {
    let Connection {
        mut reader, sender, ..
    } = connection;
    while let Ok(request) = reader.next().await {
        let origin = request
            .actor
            .as_ref()
            .and_then(|actor| ActorOrigin::try_from(actor.origin).ok())
            .unwrap_or(ActorOrigin::Unspecified);
        let action = match request.body.as_ref() {
            Some(envelope::Body::Command(command)) => match command.action.as_ref() {
                Some(command::Action::Render(render)) => render.action.as_ref(),
                _ => None,
            },
            _ => None,
        };
        let show_logo = match action {
            Some(render_command::Action::ClearEverything(_)) => false,
            Some(render_command::Action::ShowLogo(_)) => true,
            _ => {
                tracing::warn!("emergency lane refused a message that isn't Clear or Show logo");
                return;
            }
        };
        if origin != ActorOrigin::Ui {
            tracing::warn!(?origin, "emergency lane refused a non-UI sender");
            return;
        }
        engine.emergency(show_logo);
        let _ = sender.send(engine.ack(&request)).await;
    }
}

async fn listen<F, Fut>(endpoint: Endpoint, engine: Arc<Engine>, serve: F) -> anyhow::Result<()>
where
    F: Fn(Arc<Engine>, Connection) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let mut listener = Listener::bind(&endpoint)
        .await
        .with_context(|| format!("could not open the {:?} endpoint", endpoint.service()))?;
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                tokio::spawn(serve(engine.clone(), Connection::new(stream, peer)));
            }
            Err(error) => {
                tracing::warn!(%error, "accept failed");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let paths = selah_runtime::paths::Paths::from_env();
    let _logs = selah_runtime::logging::init("engine", &paths.logs)?;
    let _lock = match selah_runtime::instance::InstanceLock::acquire("engine") {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            tracing::info!("another selah-engine is already running");
            std::process::exit(3);
        }
        Err(error) => return Err(error).context("could not take the engine lock"),
    };
    let headless = std::env::args().any(|arg| arg == "--headless")
        || std::env::var("SELAH_HEADLESS").as_deref() == Ok("1");
    let mode = if headless {
        Mode::Headless
    } else {
        Mode::Windowed
    };
    if selah_runtime::safe_mode() {
        tracing::warn!("starting in safe mode: hardware output drivers stay off (SAFE-07)");
    }
    let placement = std::env::args()
        .skip_while(|arg| arg != "--fullscreen")
        .nth(1)
        .and_then(|index| index.parse().ok())
        .map_or(windows::Placement::Windowed, windows::Placement::Fullscreen);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;
    let (engine, instance) = Engine::start(mode)?;
    let servers = {
        let engine = engine.clone();
        runtime.spawn(async move {
            let endpoint = Endpoint::for_current_user(Service::Engine);
            let _heartbeat = selah_runtime::heartbeat::spawn(engine.actor().clone());
            let emergency = tokio::spawn(listen(
                endpoint.sibling(Service::Emergency),
                engine.clone(),
                serve_emergency,
            ));
            let normal = tokio::spawn(listen(endpoint, engine.clone(), serve_engine));
            tracing::info!(
                mode = ?engine.mode(),
                compositor = engine.compositor_name(),
                "engine is running"
            );
            tokio::select! {
                result = emergency => result??,
                result = normal => result??,
                _ = tokio::signal::ctrl_c() => tracing::info!("engine stopping"),
            }
            anyhow::Ok(())
        })
    };
    match (mode, instance) {
        (Mode::Windowed, Some(instance)) => {
            // The window loop owns the main thread until the windows close.
            windows::run(engine, instance, placement)?;
            Ok(())
        }
        (Mode::Windowed, None) => {
            tracing::error!("no GPU: output windows can't open, running offscreen");
            runtime.block_on(servers)?
        }
        (Mode::Headless, _) => runtime.block_on(servers)?,
    }
}
