//! Single-instance locks (ARCHITECTURE §2): one watchdog and one core per user.
use std::fs::File;
use std::io;
use std::path::Path;

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
}
