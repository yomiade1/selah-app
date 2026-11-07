//! Endpoint naming (ADR-004) and the private runtime directory (SEC-02).
//!
//! Windows: `\\.\pipe\selah-{namespace}-{service}`. Unix: `{runtime}/{namespace}-{service}.sock`
//! inside a directory that is owned by the current user and has mode `0700`.
use std::io;
use std::path::PathBuf;

/// The services Selah's processes expose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Service {
    /// UI and other clients talk to core here.
    Core,
    /// Core talks to the engine here.
    Engine,
    /// The emergency lane: the UI sends only Clear everything and Show logo (SAFE-03).
    Emergency,
    /// The watchdog's heartbeat channel.
    Watchdog,
}

impl Service {
    pub fn as_str(self) -> &'static str {
        match self {
            Service::Core => "core",
            Service::Engine => "engine",
            Service::Emergency => "emergency",
            Service::Watchdog => "watchdog",
        }
    }
}

/// Where a service listens. The namespace keeps users (and parallel test runs) apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    namespace: String,
    service: Service,
}

impl Endpoint {
    /// The default endpoint for the current user. `SELAH_IPC_NAMESPACE` overrides the
    /// namespace (tests, and side-by-side development copies).
    pub fn for_current_user(service: Service) -> Self {
        let namespace = std::env::var("SELAH_IPC_NAMESPACE")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(current_user_name);
        Self::new(&namespace, service)
    }

    /// Build an endpoint. The namespace is reduced to `[A-Za-z0-9_-]`, at most 40 characters,
    /// so it can't change the pipe or socket path.
    pub fn new(namespace: &str, service: Service) -> Self {
        let mut clean: String = namespace
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(40)
            .collect();
        if clean.is_empty() {
            clean.push_str("user");
        }
        Self {
            namespace: clean,
            service,
        }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn service(&self) -> Service {
        self.service
    }

    /// Another service in the same namespace.
    pub fn sibling(&self, service: Service) -> Self {
        Self {
            namespace: self.namespace.clone(),
            service,
        }
    }

    /// The Windows pipe name.
    pub fn pipe_name(&self) -> String {
        format!(
            r"\\.\pipe\selah-{}-{}",
            self.namespace,
            self.service.as_str()
        )
    }

    /// The Unix socket path. Creates and checks the private runtime directory.
    pub fn socket_path(&self) -> io::Result<PathBuf> {
        Ok(runtime_dir()?.join(format!("{}-{}.sock", self.namespace, self.service.as_str())))
    }
}

fn current_user_name() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".into())
}

/// The private directory for sockets and preview rings.
///
/// Unix: `$XDG_RUNTIME_DIR/selah` when set (already private), otherwise `/tmp/selah-<uid>`.
/// It's created with mode `0700`; an existing directory is accepted only if the current user
/// owns it and nobody else can enter it, so another user can't plant a socket there.
/// Windows: `%LOCALAPPDATA%\Selah\run`, which inherits the profile's per-user ACL.
pub fn runtime_dir() -> io::Result<PathBuf> {
    if let Ok(dir) = std::env::var("SELAH_RUNTIME_DIR")
        && !dir.is_empty()
    {
        return prepare_private_dir(PathBuf::from(dir));
    }
    #[cfg(unix)]
    {
        let base = match std::env::var("XDG_RUNTIME_DIR") {
            Ok(dir) if !dir.is_empty() => PathBuf::from(dir).join("selah"),
            _ => PathBuf::from(format!("/tmp/selah-{}", rustix::process::getuid().as_raw())),
        };
        prepare_private_dir(base)
    }
    #[cfg(windows)]
    {
        let local = std::env::var("LOCALAPPDATA")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
        prepare_private_dir(PathBuf::from(local).join("Selah").join("run"))
    }
}

#[cfg(unix)]
fn prepare_private_dir(dir: PathBuf) -> io::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    match std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
    {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = std::fs::symlink_metadata(&dir)?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the runtime path is not a directory",
        ));
    }
    if metadata.uid() != rustix::process::getuid().as_raw() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the runtime directory belongs to another user",
        ));
    }
    if metadata.mode() & 0o077 != 0 {
        // Ours but too open: tighten it rather than trusting it.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

#[cfg(windows)]
fn prepare_private_dir(dir: PathBuf) -> io::Result<PathBuf> {
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_cannot_inject_path_characters() {
        let endpoint = Endpoint::new(r"..\..\evil/../x y", Service::Core);
        assert_eq!(endpoint.namespace(), "evilxy");
        assert_eq!(endpoint.pipe_name(), r"\\.\pipe\selah-evilxy-core");
        assert_eq!(Endpoint::new("", Service::Engine).namespace(), "user");
        assert_eq!(
            Endpoint::new(&"a".repeat(100), Service::Engine)
                .namespace()
                .len(),
            40
        );
    }

    #[cfg(unix)]
    #[test]
    fn runtime_directory_is_private_0700() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("run");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let prepared = prepare_private_dir(dir.clone()).unwrap();
        let mode = std::fs::metadata(prepared).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o700,
            "sec: SEC-02 socket directory must be 0700"
        );
    }

    #[cfg(unix)]
    #[test]
    fn runtime_path_that_is_a_file_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        assert!(prepare_private_dir(file).is_err());
    }
}
