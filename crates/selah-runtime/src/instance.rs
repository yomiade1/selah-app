//! Single-instance locks (ARCHITECTURE §2): one watchdog and one core per user.
use std::fs::File;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// How often [`InstanceLock::acquire_waiting`] retries. Short enough that the wait adds
/// little to a restart, long enough not to spin.
const POLL: Duration = Duration::from_millis(10);

/// Held for the life of the process; the OS releases it if the process dies.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Take the named lock for the current IPC namespace in the private runtime directory, or
    /// fail with `WouldBlock` if another process holds it.
    pub fn acquire(name: &str) -> io::Result<Self> {
        let dir = selah_ipc::runtime_dir()?;
        let namespace = selah_ipc::Endpoint::for_current_user(selah_ipc::Service::Core)
            .namespace()
            .to_owned();
        Self::acquire_in(&dir, &format!("{namespace}-{name}"))
    }

    /// Take the named lock, waiting up to `timeout` for the holder to let go.
    ///
    /// A restarting engine reaches this while the engine it replaces is still shutting down:
    /// the operating system can take about half a second to tear down a graphics device, and
    /// the lock is only released once that process is really gone (ADR-028). Waiting here,
    /// instead of exiting as "already running", lets the replacement warm its own GPU during
    /// that time rather than after it.
    pub fn acquire_waiting(name: &str, timeout: Duration) -> io::Result<Self> {
        Self::waiting(timeout, || Self::acquire(name))
    }

    /// [`Self::acquire_waiting`] against an explicit directory, for tests.
    pub fn acquire_in_waiting(dir: &Path, name: &str, timeout: Duration) -> io::Result<Self> {
        Self::waiting(timeout, || Self::acquire_in(dir, name))
    }

    fn waiting(timeout: Duration, mut attempt: impl FnMut() -> io::Result<Self>) -> io::Result<Self> {
        let deadline = Instant::now() + timeout;
        loop {
            match attempt() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(error);
                    }
                    std::thread::sleep(POLL);
                }
                other => return other,
            }
        }
    }

    pub fn acquire_in(dir: &Path, name: &str) -> io::Result<Self> {
        let path = dir.join(format!("{name}.lock"));
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "another Selah process already holds this lock",
            )),
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_holder_is_refused_until_the_first_releases() {
        let dir = tempfile::tempdir().unwrap();
        let first = InstanceLock::acquire_in(dir.path(), "core").unwrap();
        let second = InstanceLock::acquire_in(dir.path(), "core").unwrap_err();
        assert_eq!(second.kind(), io::ErrorKind::WouldBlock);
        drop(first);
        InstanceLock::acquire_in(dir.path(), "core").unwrap();
    }

    #[test]
    fn waiting_takes_the_lock_once_the_holder_lets_go() {
        // The restart case (ADR-028): the replacement engine reaches the lock while the engine
        // it replaces is still shutting down, and takes it as soon as that finishes.
        let dir = tempfile::tempdir().unwrap();
        let held = InstanceLock::acquire_in(dir.path(), "engine").unwrap();
        let path = dir.path().to_owned();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            drop(held);
        });
        let started = Instant::now();
        let taken =
            InstanceLock::acquire_in_waiting(&path, "engine", Duration::from_secs(5)).unwrap();
        let waited = started.elapsed();
        releaser.join().unwrap();
        drop(taken);
        assert!(
            waited >= Duration::from_millis(100),
            "it must actually have waited, not raced through: {waited:?}"
        );
        assert!(
            waited < Duration::from_secs(2),
            "it must take the lock promptly once free: {waited:?}"
        );
    }

    #[test]
    fn waiting_gives_up_when_the_holder_keeps_it() {
        // A second Selah really is running: the engine must still exit rather than hang.
        let dir = tempfile::tempdir().unwrap();
        let _held = InstanceLock::acquire_in(dir.path(), "engine").unwrap();
        let started = Instant::now();
        let error =
            InstanceLock::acquire_in_waiting(dir.path(), "engine", Duration::from_millis(150))
                .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(started.elapsed() >= Duration::from_millis(150));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
