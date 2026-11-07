#![forbid(unsafe_code)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! `selah-watchdog`: starts the engine, core and UI, watches their 250 ms heartbeats, and
//! restarts any that exit or hang, within 2 seconds (ADR-001). Three crashes in 60 seconds
//! start safe mode (SAFE-07).
//!
//! ```text
//! selah-watchdog [--headless] [--no-ui] [--safe-mode]
//! ```
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context;
use selah_ipc::{Connection, Endpoint, Listener, Service};
use selah_proto::v1::{
    ActorOrigin, ProcessInfo, Reply, WatchdogStatus, envelope, event, query, reply,
};
use selah_watchdog::EXIT_ALREADY_RUNNING;
use selah_watchdog::policy::{self, CrashLog};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Role {
    Engine,
    Core,
    Ui,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Role::Engine => "engine",
            Role::Core => "core",
            Role::Ui => "ui",
        }
    }

    fn from_origin(origin: i32) -> Option<Self> {
        match ActorOrigin::try_from(origin).ok()? {
            ActorOrigin::Engine => Some(Role::Engine),
            ActorOrigin::Core => Some(Role::Core),
            ActorOrigin::Ui => Some(Role::Ui),
            _ => None,
        }
    }
}

struct Supervised {
    role: Role,
    program: PathBuf,
    args: Vec<String>,
    child: Option<tokio::process::Child>,
    pid: u32,
    started: Instant,
    restarts: u32,
    recent_crashes: CrashLog,
    not_before: Option<Instant>,
    /// Another copy holds the lock (it outlived a previous watchdog); watch its heartbeats.
    adopted: bool,
}

#[derive(Default)]
struct Shared {
    heartbeats: HashMap<Role, Instant>,
    status: WatchdogStatus,
}

type SharedState = Arc<Mutex<Shared>>;

fn lock(shared: &SharedState) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|p| p.into_inner())
}

fn exe(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

fn spawn(process: &mut Supervised, safe_mode: bool, headless: bool) {
    let mut command = tokio::process::Command::new(&process.program);
    command
        .args(&process.args)
        .env("SELAH_SAFE_MODE", if safe_mode { "1" } else { "0" })
        .stdin(std::process::Stdio::null());
    if headless {
        command.env("SELAH_HEADLESS", "1");
    }
    #[cfg(windows)]
    {
        // No console windows for the helper processes.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    match command.spawn() {
        Ok(child) => {
            process.pid = child.id().unwrap_or(0);
            process.child = Some(child);
            process.started = Instant::now();
            process.adopted = false;
            tracing::info!(role = process.role.name(), pid = process.pid, "started");
        }
        Err(error) => {
            tracing::error!(role = process.role.name(), %error, "could not start");
            process.child = None;
            process.not_before = Some(Instant::now() + Duration::from_secs(1));
        }
    }
}

async fn serve(shared: SharedState, connection: Connection) {
    let Connection {
        mut reader, sender, ..
    } = connection;
    while let Ok(message) = reader.next().await {
        match message.body.as_ref() {
            Some(envelope::Body::Event(event)) => {
                if let Some(event::Notification::Heartbeat(_)) = event.notification {
                    let role = message
                        .actor
                        .as_ref()
                        .and_then(|actor| Role::from_origin(actor.origin));
                    if let Some(role) = role {
                        lock(&shared).heartbeats.insert(role, Instant::now());
                    }
                }
            }
            Some(envelope::Body::Query(query))
                if matches!(query.request, Some(query::Request::GetWatchdogStatus(_))) =>
            {
                let status = lock(&shared).status.clone();
                let reply = selah_ipc::reply_to(
                    &message,
                    watchdog_actor(),
                    Reply {
                        result: Some(reply::Result::WatchdogStatus(status)),
                    },
                );
                if sender.send(reply).await.is_err() {
                    return;
                }
            }
            _ => {}
        }
    }
}

fn watchdog_actor() -> selah_proto::v1::Actor {
    selah_proto::v1::Actor {
        user_id: "selah".into(),
        device_id: "watchdog".into(),
        origin: ActorOrigin::Watchdog as i32,
    }
}

struct Options {
    headless: bool,
    ui: Option<PathBuf>,
    safe_mode: bool,
}

fn options() -> Options {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    let dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    // Installed layout (packaging/velopack/README.md): the UI lives in `ui/`, because its
    // `Selah.exe` would clash with the `selah.exe` launcher on case-insensitive file systems.
    let ui = if has("--no-ui") {
        None
    } else {
        std::env::var_os("SELAH_UI_PATH")
            .map(PathBuf::from)
            .or_else(|| Some(exe(&dir.join("ui"), "Selah")))
            .filter(|path| path.is_file())
    };
    Options {
        headless: has("--headless") || std::env::var("SELAH_HEADLESS").as_deref() == Ok("1"),
        ui,
        safe_mode: has("--safe-mode") || selah_runtime::safe_mode(),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let paths = selah_runtime::paths::Paths::from_env();
    let _logs = selah_runtime::logging::init("watchdog", &paths.logs)?;
    let _lock = match selah_runtime::instance::InstanceLock::acquire("watchdog") {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            tracing::info!("Selah is already running");
            std::process::exit(EXIT_ALREADY_RUNNING);
        }
        Err(error) => return Err(error).context("could not take the watchdog lock"),
    };
    let options = options();
    let dir = std::env::current_exe()?
        .parent()
        .context("watchdog has no folder")?
        .to_path_buf();
    // Children find each other through the same namespace and runtime folder.
    let runtime_dir = selah_ipc::runtime_dir()?;
    let endpoint = Endpoint::for_current_user(Service::Watchdog);
    let mut safe_mode = options.safe_mode;
    let mut safe_reason = if safe_mode { "requested" } else { "" }.to_string();
    if safe_mode {
        tracing::warn!("starting in safe mode (SAFE-07)");
    }

    let shared: SharedState = Arc::new(Mutex::new(Shared::default()));
    let mut listener = Listener::bind(&endpoint)
        .await
        .context("could not open the watchdog endpoint")?;
    let accept_state = shared.clone();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    tokio::spawn(serve(accept_state.clone(), Connection::new(stream, peer)));
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    });

    let engine_args = if options.headless {
        vec!["--headless".to_string()]
    } else {
        Vec::new()
    };
    let mut processes = vec![
        Supervised::new(Role::Engine, exe(&dir, "selah-engine"), engine_args),
        Supervised::new(Role::Core, exe(&dir, "selah-core"), Vec::new()),
    ];
    if let Some(ui) = options.ui.clone() {
        processes.push(Supervised::new(Role::Ui, ui, Vec::new()));
    } else {
        tracing::info!("no UI: running core and engine only");
    }
    for process in processes.iter_mut() {
        spawn(process, safe_mode, options.headless);
    }
    tracing::info!(runtime = %runtime_dir.display(), "watchdog is supervising");

    let mut crashes = CrashLog::default();
    let mut ticker = tokio::time::interval(Duration::from_millis(25));
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = tokio::signal::ctrl_c() => break,
        }
        let now = Instant::now();
        let heartbeats = lock(&shared).heartbeats.clone();
        let mut enter_safe_mode = false;
        let mut quit = false;
        for process in processes.iter_mut() {
            let last_beat = heartbeats
                .get(&process.role)
                .copied()
                .filter(|beat| *beat >= process.started);
            let exited = match process.child.as_mut() {
                Some(child) => child.try_wait().ok().flatten(),
                None => None,
            };
            if let Some(status) = exited {
                process.child = None;
                if status.code() == Some(EXIT_ALREADY_RUNNING) {
                    tracing::info!(role = process.role.name(), "already running; adopting it");
                    process.adopted = true;
                    process.started = now;
                    continue;
                }
                if process.role == Role::Ui && status.success() {
                    // The person quit Selah (the UI asked first if something was live, SAFE-06).
                    quit = true;
                    continue;
                }
                tracing::warn!(role = process.role.name(), ?status, "exited unexpectedly");
                process.crashed(now, &mut crashes, &mut enter_safe_mode);
            } else if process.adopted {
                if policy::is_hung(process.started, heartbeats.get(&process.role).copied(), now) {
                    process.adopted = false;
                    process.not_before = None;
                }
            } else if process.child.is_some() && policy::is_hung(process.started, last_beat, now) {
                tracing::warn!(role = process.role.name(), "stopped responding; restarting");
                if let Some(child) = process.child.as_mut() {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                }
                process.child = None;
                process.crashed(now, &mut crashes, &mut enter_safe_mode);
            }
        }
        if quit {
            break;
        }
        if enter_safe_mode && !safe_mode {
            safe_mode = true;
            safe_reason = "crashes".into();
            tracing::warn!("3 crashes in 60 seconds: restarting core and the UI in safe mode");
            // The engine keeps the screens as they are; it picks up safe mode on its next start.
            for process in processes.iter_mut().filter(|p| p.role != Role::Engine) {
                if let Some(child) = process.child.as_mut() {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                }
                process.child = None;
                process.not_before = None;
            }
        }
        for process in processes.iter_mut() {
            if process.child.is_none()
                && !process.adopted
                && process.not_before.is_none_or(|at| now >= at)
            {
                process.not_before = None;
                if process.restarts > 0 || process.pid != 0 {
                    process.restarts += 1;
                }
                spawn(process, safe_mode, options.headless);
            }
        }
        let status = WatchdogStatus {
            processes: processes
                .iter()
                .map(|p| ProcessInfo {
                    role: p.role.name().into(),
                    pid: p.pid,
                    restarts: p.restarts,
                    running: p.child.is_some() || p.adopted,
                })
                .collect(),
            safe_mode,
            safe_mode_reason: safe_reason.clone(),
        };
        lock(&shared).status = status;
    }

    tracing::info!("stopping Selah");
    for role in [Role::Ui, Role::Core, Role::Engine] {
        for process in processes.iter_mut().filter(|p| p.role == role) {
            if let Some(child) = process.child.as_mut() {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
        }
    }
    Ok(())
}

impl Supervised {
    fn new(role: Role, program: PathBuf, args: Vec<String>) -> Self {
        Self {
            role,
            program,
            args,
            child: None,
            pid: 0,
            started: Instant::now(),
            restarts: 0,
            recent_crashes: CrashLog::default(),
            not_before: None,
            adopted: false,
        }
    }

    fn crashed(&mut self, now: Instant, all: &mut CrashLog, enter_safe_mode: &mut bool) {
        if all.record(now) {
            *enter_safe_mode = true;
        }
        self.recent_crashes.record(now);
        let delay = policy::restart_delay(self.recent_crashes.recent());
        self.not_before = (!delay.is_zero()).then(|| now + delay);
        if !delay.is_zero() {
            tracing::warn!(role = self.role.name(), ?delay, "crash loop: backing off");
        }
    }
}
