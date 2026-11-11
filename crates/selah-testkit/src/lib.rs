#![deny(unsafe_code)]
//! Run Selah's real processes in isolation for end-to-end and chaos tests. Only `freeze` (OS
//! thread suspension on Windows) may use unsafe code (SEC-11).
//!
//! Each [`Stack`] gets its own IPC namespace and its own `SELAH_HOME`, so stacks in parallel
//! tests never see each other. Binaries come from the same `target/<profile>` folder as the
//! test, so build the workspace first (`cargo test --workspace` does).
pub mod freeze;
pub mod golden;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use selah_ipc::{Client, Endpoint, Service};
use selah_proto::v1::{
    Actor, ActorOrigin, Command as WireCommand, Envelope, GetStatus, GetWatchdogStatus, Query,
    Reply, ShowTestPattern, Status, TestPattern, WatchdogStatus, command, envelope, query, reply,
};

/// The folder holding the workspace binaries for this test build.
pub fn bin_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("test executable path");
    let mut dir = exe.parent().expect("deps folder").to_path_buf();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir
}

pub fn bin(name: &str) -> PathBuf {
    let path = bin_dir().join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "{} is missing: build the workspace first (cargo build --workspace)",
        path.display()
    );
    path
}

/// The real Avalonia UI (`app/Selah.UI`, built by `dotnet build app/Selah.sln -c Release`, gate
/// stage 5). `SELAH_UI_BINARY` overrides it (the installer check points at the installed app).
pub fn ui_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("SELAH_UI_BINARY") {
        return PathBuf::from(path);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root
        .join("app/Selah.UI/bin/Release/net10.0")
        .join(format!("Selah{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "{} is missing: build the UI first (dotnet build app/Selah.sln -c Release)",
        path.display()
    );
    path
}

/// Kill a process by id (the test didn't spawn it, so it has no handle).
pub fn kill_pid(pid: u32) {
    if pid == 0 {
        return;
    }
    let status = if cfg!(windows) {
        Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    } else {
        Command::new("kill")
            .args(["-9", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };
    let _ = status;
}

pub fn test_actor() -> Actor {
    Actor {
        user_id: "local-owner".into(),
        device_id: "testkit".into(),
        origin: ActorOrigin::Ui as i32,
    }
}

#[derive(Clone, Debug, Default)]
pub struct StackOptions {
    /// Start the real UI, headless. It writes `$SELAH_HOME/ui.txt` (`<pid> <generation>`) each
    /// time it connects to core and subscribes.
    pub ui: bool,
    pub safe_mode: bool,
}

pub struct Stack {
    pub namespace: String,
    home: PathBuf,
    _home_guard: Option<tempfile::TempDir>,
    watchdog: Child,
    known_pids: Mutex<Vec<u32>>,
    core: Client,
    engine: Client,
    watchdog_client: Client,
}

fn unique_namespace() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!(
        "tk{}x{}x{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        nanos % 10_000
    )
}

impl Stack {
    /// Start watchdog → engine (headless) → core (→ UI, headless). Must run inside a tokio runtime.
    pub fn start(options: StackOptions) -> Self {
        let namespace = unique_namespace();
        let guard = tempfile::tempdir().expect("temporary SELAH_HOME");
        // SELAH_TEST_KEEP_HOME=1 keeps the folder (and its logs) for debugging.
        let (home, home_guard) = if std::env::var("SELAH_TEST_KEEP_HOME").as_deref() == Ok("1") {
            let kept = guard.keep();
            eprintln!("keeping test home {}", kept.display());
            (kept, None)
        } else {
            (guard.path().to_path_buf(), Some(guard))
        };
        let mut command = Command::new(bin("selah-watchdog"));
        command
            .arg("--headless")
            .env("SELAH_IPC_NAMESPACE", &namespace)
            .env("SELAH_HOME", &home)
            .env("SELAH_HEADLESS", "1")
            .env("SELAH_EXIT_WITH_WATCHDOG", "1")
            // Software rendering is deterministic and works on machines without a GPU.
            .env("SELAH_GPU", "software")
            .env(
                "SELAH_AUDIO",
                std::env::var("SELAH_AUDIO").unwrap_or_else(|_| "null".into()),
            )
            .env(
                "SELAH_LOG",
                std::env::var("SELAH_LOG").unwrap_or_else(|_| "info".into()),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if options.safe_mode {
            command.arg("--safe-mode");
        }
        if options.ui {
            command
                .env("SELAH_UI_PATH", ui_binary())
                .env("AVALONIA_TELEMETRY_OPTOUT", "1");
        } else {
            command.arg("--no-ui");
        }
        let watchdog = command.spawn().expect("start selah-watchdog");
        let endpoint = |service| Endpoint::new(&namespace, service);
        Self {
            core: Client::start(endpoint(Service::Core), test_actor()),
            engine: Client::start(endpoint(Service::Engine), test_actor()),
            watchdog_client: Client::start(endpoint(Service::Watchdog), test_actor()),
            namespace,
            home,
            _home_guard: home_guard,
            watchdog,
            known_pids: Mutex::new(Vec::new()),
        }
    }

    pub fn endpoint(&self, service: Service) -> Endpoint {
        Endpoint::new(&self.namespace, service)
    }

    pub fn core(&self) -> &Client {
        &self.core
    }

    /// The test's client on the engine endpoint.
    pub fn engine(&self) -> &Client {
        &self.engine
    }

    /// The test's client on the watchdog endpoint (its actor is the UI).
    pub fn watchdog(&self) -> &Client {
        &self.watchdog_client
    }

    pub fn watchdog_pid(&self) -> u32 {
        self.watchdog.id()
    }

    async fn ask(client: &Client, request: query::Request) -> Option<Envelope> {
        if !client.is_connected() {
            return None;
        }
        client
            .request(
                envelope::Body::Query(Query {
                    request: Some(request),
                }),
                Duration::from_millis(500),
            )
            .await
            .ok()
    }

    fn status_of(reply: Option<Envelope>) -> Option<Status> {
        match reply?.body? {
            envelope::Body::Reply(Reply {
                result: Some(reply::Result::Status(status)),
            }) => Some(status),
            _ => None,
        }
    }

    pub async fn core_status(&self) -> Option<Status> {
        Self::status_of(Self::ask(&self.core, query::Request::GetStatus(GetStatus {})).await)
    }

    pub async fn engine_status(&self) -> Option<Status> {
        Self::status_of(Self::ask(&self.engine, query::Request::GetStatus(GetStatus {})).await)
    }

    pub async fn watchdog_status(&self) -> Option<WatchdogStatus> {
        let reply = Self::ask(
            &self.watchdog_client,
            query::Request::GetWatchdogStatus(GetWatchdogStatus {}),
        )
        .await?;
        match reply.body? {
            envelope::Body::Reply(Reply {
                result: Some(reply::Result::WatchdogStatus(status)),
            }) => {
                let mut known = self.known_pids.lock().unwrap_or_else(|p| p.into_inner());
                for process in &status.processes {
                    if process.pid != 0 && !known.contains(&process.pid) {
                        known.push(process.pid);
                    }
                }
                Some(status)
            }
            _ => None,
        }
    }

    pub async fn pid(&self, role: &str) -> Option<u32> {
        self.watchdog_status()
            .await?
            .processes
            .into_iter()
            .find(|p| p.role == role && p.running && p.pid != 0)
            .map(|p| p.pid)
    }

    /// Ask core to show a test pattern.
    pub async fn show_test_pattern(&self, pattern: TestPattern) -> Envelope {
        self.core
            .request(
                envelope::Body::Command(WireCommand {
                    action: Some(command::Action::ShowTestPattern(ShowTestPattern {
                        pattern: pattern as i32,
                    })),
                    ..Default::default()
                }),
                Duration::from_secs(5),
            )
            .await
            .expect("core answers")
    }

    /// Wait until everything is up, the engine shows core's scene, and return core's status.
    pub async fn wait_ready(&self, timeout: Duration) -> Status {
        let deadline = Instant::now() + timeout;
        loop {
            // The test's own clients must be connected too, or a later "is it still up?" check
            // can catch one of them still in its reconnect backoff.
            if let Some(status) = self.core_status().await
                && status.engine_connected
                && status.engine_scene_revision == status.scene_revision
                && !status.frame_checksum.is_empty()
                && self.engine_status().await.is_some()
                && self.watchdog_status().await.is_some()
            {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "the stack didn't become ready in time"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Stop every process while keeping `home()` on disk for post-shutdown verification.
    pub async fn stop_processes(&mut self) {
        let _ = self.watchdog_status().await;
        self.kill_all();
    }

    /// Stop everything: the watchdog first so nothing is restarted, then every known child.
    pub async fn shutdown(mut self) {
        self.stop_processes().await;
    }

    fn kill_all(&mut self) {
        let _ = self.watchdog.kill();
        let _ = self.watchdog.wait();
        let mut pids = self
            .known_pids
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Ok(text) = std::fs::read_to_string(self.home.join("pids.txt")) {
            for line in text.lines() {
                if let Ok(pid) = line.trim().parse::<u32>()
                    && pid != 0
                    && !pids.contains(&pid)
                {
                    pids.push(pid);
                }
            }
        }
        if let Ok(text) = std::fs::read_to_string(self.home.join("ui.txt"))
            && let Some(first) = text.split_whitespace().next()
            && let Ok(pid) = first.parse::<u32>()
            && pid != 0
            && !pids.contains(&pid)
        {
            pids.push(pid);
        }
        for pid in pids {
            kill_pid(pid);
        }
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        self.kill_all();
    }
}

/// Poll `check` every 10 ms until it returns `Some`, or panic after `timeout`.
pub async fn eventually<T, F, Fut>(timeout: Duration, what: &str, mut check: F) -> (T, Duration)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let started = Instant::now();
    loop {
        if let Some(value) = check().await {
            return (value, started.elapsed());
        }
        assert!(started.elapsed() < timeout, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
