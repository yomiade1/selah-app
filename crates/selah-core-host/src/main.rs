#![forbid(unsafe_code)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! `selah-core`: owns the data (ADR-002), serves the UI on the core endpoint and drives the
//! engine. If it crashes, the watchdog restarts it; it reloads `live_state.bin` and re-sends
//! the scene, and the engine keeps the last frame meanwhile (ARCHITECTURE §2).
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Context;
use selah_core::update::{self, Install};
use selah_core::{CoreService, Work};
use selah_ipc::{Client, Connection, Endpoint, FrameSender, Listener, Service};
use selah_proto::v1::{Command, Envelope, Event, command, envelope};
use tokio::sync::Mutex;

struct Shared {
    service: Mutex<CoreService>,
    subscribers: Mutex<Vec<FrameSender>>,
    engine: Client,
    next_event_id: AtomicU64,
    /// The only way out to the network (SEC-13). Core sets its policy from the settings.
    net: Arc<selah_net::Net>,
    /// Velopack's Update.exe, when Selah was installed by Setup.
    installer: Option<Arc<dyn Install>>,
    /// Downloaded updates.
    updates_dir: std::path::PathBuf,
    paths: selah_runtime::paths::Paths,
}

/// Ask the engine for its own status (renderer, output size), if it answers.
async fn engine_status(shared: &Shared) -> Option<selah_proto::v1::Status> {
    let reply = shared
        .engine
        .request(
            envelope::Body::Query(selah_proto::v1::Query {
                request: Some(selah_proto::v1::query::Request::GetStatus(
                    selah_proto::v1::GetStatus {},
                )),
            }),
            Duration::from_secs(1),
        )
        .await
        .ok()?;
    match reply.body? {
        envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(selah_proto::v1::reply::Result::Status(status)),
        }) => Some(status),
        _ => None,
    }
}

/// Run slow work away from the request path, then send its reply.
async fn run_work(shared: Arc<Shared>, request: Envelope, work: Work, sender: FrameSender) {
    let reply = match work {
        Work::CheckForUpdate { feed_url } => {
            let net = shared.net.clone();
            let result = tokio::task::spawn_blocking(move || {
                update::check(&net, &feed_url, update::PACKAGE_ID, selah_runtime::VERSION)
            })
            .await
            .unwrap_or_else(|error| Err(update::UpdateError::Feed(error.to_string())));
            shared
                .service
                .lock()
                .await
                .finish_update_check(&request, result)
        }
        Work::InstallUpdate { feed_url, release } => {
            let net = shared.net.clone();
            let folder = shared.updates_dir.clone();
            let downloaded = tokio::task::spawn_blocking(move || {
                update::download(&net, &feed_url, &release, &folder)
            })
            .await
            .unwrap_or_else(|error| Err(update::UpdateError::Feed(error.to_string())));
            let installer = shared.installer.clone();
            shared.service.lock().await.finish_update_install(
                &request,
                downloaded,
                installer.as_deref(),
            )
        }
        Work::CreateSupportBundle => {
            let (settings, safe_mode) = {
                let service = shared.service.lock().await;
                (service.settings().clone(), service.safe_mode())
            };
            let paths = shared.paths.clone();
            let saved = tokio::task::spawn_blocking(move || {
                selah_core::support::create(&selah_core::support::BundleInput {
                    version: selah_runtime::VERSION,
                    safe_mode,
                    settings: &settings,
                    logs_dir: &paths.logs,
                    support_dir: &paths.support,
                })
            })
            .await
            .unwrap_or_else(|error| Err(std::io::Error::other(error.to_string())));
            shared
                .service
                .lock()
                .await
                .finish_support_bundle(&request, saved)
        }
        Work::GetDiagnostics => {
            let engine = engine_status(&shared).await;
            shared
                .service
                .lock()
                .await
                .finish_diagnostics(&request, engine.as_ref())
        }
        Work::RunDiagnostics { quick } => {
            let engine = engine_status(&shared).await;
            let context = {
                let service = shared.service.lock().await;
                serde_json::to_value(DiagnosticsContext::from(
                    service.diagnostics(engine.as_ref()),
                ))
                .unwrap_or_default()
            };
            let support = shared.paths.support.clone();
            let result = tokio::task::spawn_blocking(move || {
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
                    .unwrap_or_default();
                let plan = selah_core::diagnostics::plan(&exe_dir, quick)?;
                std::fs::create_dir_all(&support).map_err(|error| {
                    selah_core::diagnostics::DiagnosticsError::Failed(error.to_string())
                })?;
                selah_core::diagnostics::run(&plan, &support, context)
            })
            .await
            .unwrap_or_else(|error| {
                Err(selah_core::diagnostics::DiagnosticsError::Failed(
                    error.to_string(),
                ))
            });
            shared
                .service
                .lock()
                .await
                .finish_run_diagnostics(&request, result)
        }
    };
    let _ = sender.send(reply).await;
}

/// What a Diagnostics report records about this computer and Selah.
#[derive(serde::Serialize)]
struct DiagnosticsContext {
    safe_mode: bool,
    engine_connected: bool,
    renderer: String,
    output: String,
}

impl From<selah_proto::v1::Diagnostics> for DiagnosticsContext {
    fn from(d: selah_proto::v1::Diagnostics) -> Self {
        Self {
            safe_mode: d.safe_mode,
            engine_connected: d.engine_connected,
            renderer: d.renderer,
            output: d
                .output_resolution
                .map(|r| format!("{}x{}", r.width, r.height))
                .unwrap_or_default(),
        }
    }
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
        if let Some(work) = outcome.work {
            tokio::spawn(run_work(
                shared.clone(),
                request.clone(),
                work,
                sender.clone(),
            ));
        }
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

/// kb.db: `SELAH_KB_DB`, next to the executable (installed), or `target/kb/kb.db` (development,
/// built by `selah-kb build`, gate stage 9).
fn help_index() -> Option<(std::path::PathBuf, selah_kb::HelpIndex)> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("SELAH_KB_DB") {
        candidates.push(std::path::PathBuf::from(path));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    {
        candidates.push(dir.join("kb.db"));
        candidates.push(dir.join("../kb/kb.db"));
    }
    candidates
        .into_iter()
        .filter(|path| path.is_file())
        .find_map(|path| match selah_kb::HelpIndex::open(&path) {
            Ok(help) => Some((path, help)),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "kb.db couldn't be opened");
                None
            }
        })
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
    // SEC-13: one gate for every outgoing connection; offline until a setting allows a purpose.
    let net = Arc::new(selah_net::Net::https());
    let mut service = CoreService::with_platform(
        &paths.config,
        safe_mode,
        net.clone(),
        selah_runtime::power::platform(),
    );
    match help_index() {
        Some((path, help)) => {
            tracing::info!(path = %path.display(), "help index loaded");
            service.set_help(help);
        }
        None => tracing::warn!("kb.db not found; help search answers nothing"),
    }
    let actor = service.actor().clone();
    let shared = Arc::new(Shared {
        service: Mutex::new(service),
        subscribers: Mutex::new(Vec::new()),
        engine: Client::start(endpoint.sibling(Service::Engine), actor.clone()),
        next_event_id: AtomicU64::new(1),
        net,
        installer: update::VelopackInstaller::for_this_installation()
            .map(|installer| Arc::new(installer) as Arc<dyn Install>),
        updates_dir: paths.cache.join("updates"),
        paths: paths.clone(),
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
