#![forbid(unsafe_code)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! `selah-core`: owns the data (ADR-002), serves the UI on the core endpoint and drives the
//! engine. If it crashes, the watchdog restarts it; it reloads `live_state.bin` and re-sends
//! the scene, and the engine keeps the last frame meanwhile (ARCHITECTURE §2).
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Context;
use selah_core::CoreService;
use selah_ipc::{Client, Connection, Endpoint, FrameSender, Listener, Service};
use selah_proto::v1::{Command, Envelope, Event, command, envelope};
use tokio::sync::Mutex;

struct Shared {
    service: Mutex<CoreService>,
    subscribers: Mutex<Vec<FrameSender>>,
    engine: Client,
    next_event_id: AtomicU64,
}

impl Shared {
    async fn broadcast(&self, events: Vec<Event>) {
        if events.is_empty() {
            return;
        }
        let actor = self.service.lock().await.actor().clone();
        let mut subscribers = self.subscribers.lock().await;
        subscribers.retain(|sender| !sender.is_closed());
        for event in events {
            let message = Envelope {
                id: self.next_event_id.fetch_add(1, Ordering::Relaxed),
                correlation_id: 0,
                actor: Some(actor.clone()),
                body: Some(envelope::Body::Event(event)),
            };
            for sender in subscribers.iter() {
                // A stalled UI must never hold up core.
                if sender.try_send(message.clone()).is_err() {
                    tracing::debug!("dropped an event for a slow subscriber");
                }
            }
        }
    }

    async fn send_to_engine(&self, command: selah_proto::v1::RenderCommand) {
        let body = envelope::Body::Command(Command {
            action: Some(command::Action::Render(command)),
        });
        if let Err(error) = self.engine.send(body).await {
            tracing::warn!(%error, "engine is not connected; it will be resynchronised");
        }
    }
}

async fn serve_connection(shared: Arc<Shared>, connection: Connection) {
    let Connection {
        mut reader, sender, ..
    } = connection;
    loop {
        let request = match reader.next().await {
            Ok(request) => request,
            Err(error) => {
                tracing::debug!(%error, "client connection closed");
                return;
            }
        };
        let outcome = shared.service.lock().await.handle(&request);
        if let Some(reply) = outcome.reply
            && sender.send(reply).await.is_err()
        {
            return;
        }
        if outcome.subscribe {
            shared.subscribers.lock().await.push(sender.clone());
        }
        if let Some(command) = outcome.to_engine {
            shared.send_to_engine(command).await;
        }
        shared.broadcast(outcome.broadcast).await;
    }
}

/// Keep the engine in step: resend the scene on every new connection, forward its events.
async fn follow_engine(shared: Arc<Shared>) {
    let mut state = shared.engine.state();
    // The engine may have connected before we subscribed: handle the current state first.
    state.mark_changed();
    let mut events = shared.engine.events();
    let mut seen_generation = 0;
    loop {
        tokio::select! {
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
                let current = *state.borrow_and_update();
                let updates = shared.service.lock().await.on_engine_connection(current.connected);
                if current.connected && current.generation != seen_generation {
                    seen_generation = current.generation;
                    let resync = shared.service.lock().await.resync_command();
                    shared.send_to_engine(resync).await;
                    tracing::info!(generation = current.generation, "engine connected; scene re-sent");
                }
                shared.broadcast(updates).await;
            }
            event = events.recv() => {
                let Ok(message) = event else { continue };
                if let Some(envelope::Body::Event(event)) = message.body {
                    let updates = shared.service.lock().await.on_engine_event(&event);
                    shared.broadcast(updates).await;
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let paths = selah_runtime::paths::Paths::from_env();
    let _logs = selah_runtime::logging::init("core", &paths.logs)?;
    let _lock = match selah_runtime::instance::InstanceLock::acquire("core") {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            tracing::info!("another selah-core is already running");
            std::process::exit(3);
        }
        Err(error) => return Err(error).context("could not take the core lock"),
    };
    let safe_mode = selah_runtime::safe_mode();
    if safe_mode {
        tracing::warn!("starting in safe mode (SAFE-07)");
    }
    let endpoint = Endpoint::for_current_user(Service::Core);
    let service = CoreService::new(&paths.config, safe_mode);
    let actor = service.actor().clone();
    let shared = Arc::new(Shared {
        service: Mutex::new(service),
        subscribers: Mutex::new(Vec::new()),
        engine: Client::start(endpoint.sibling(Service::Engine), actor.clone()),
        next_event_id: AtomicU64::new(1),
    });
    let _heartbeat = selah_runtime::heartbeat::spawn(actor);
    tokio::spawn(follow_engine(shared.clone()));

    let mut listener = Listener::bind(&endpoint)
        .await
        .context("could not open the core endpoint")?;
    tracing::info!(endpoint = endpoint.namespace(), "core is listening");
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    tokio::spawn(serve_connection(shared.clone(), Connection::new(stream, peer)));
                }
                Err(error) => {
                    tracing::warn!(%error, "accept failed");
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            },
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("core stopping");
                return Ok(());
            }
        }
    }
}
