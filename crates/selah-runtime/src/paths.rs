//! Where Selah keeps its files (ARCHITECTURE §4.1).
//!
//! `SELAH_HOME` puts everything under one folder (tests, portable development copies).
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    /// Settings and the live-state snapshot.
    pub config: PathBuf,
    /// Thumbnails and previews.
    pub cache: PathBuf,
    /// Rotating JSON logs.
    pub logs: PathBuf,
    /// The library folder (M2).
    pub library: PathBuf,
}

impl Paths {
    pub fn from_env() -> Self {
        if let Ok(home) = std::env::var("SELAH_HOME")
            && !home.is_empty()
        {
            return Self::under(PathBuf::from(home));
        }
        Self::platform_default()
    }

    /// Everything under one root.
    pub fn under(root: PathBuf) -> Self {
        Self {
            config: root.join("config"),
            cache: root.join("cache"),
            logs: root.join("logs"),
            library: root.join("library"),
        }
    }

    fn platform_default() -> Self {
        let env = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let home = env("HOME")
            .or_else(|| env("USERPROFILE"))
            .unwrap_or_else(|| PathBuf::from("."));
        if cfg!(windows) {
            let roaming = env("APPDATA").unwrap_or_else(|| home.join("AppData/Roaming"));
            let local = env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData/Local"));
            Self {
                config: roaming.join("Selah"),
                cache: local.join("Selah").join("cache"),
                logs: local.join("Selah").join("logs"),
                library: home.join("Documents").join("Selah Library"),
            }
        } else if cfg!(target_os = "macos") {
            Self {
                config: home.join("Library/Application Support/Selah"),
                cache: home.join("Library/Caches/Selah"),
                logs: home.join("Library/Logs/Selah"),
                library: home.join("Documents/Selah Library"),
            }
        } else {
            // Linux is a development platform only (ADR-023): follow XDG.
            let config = env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
            let cache = env("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"));
            let state = env("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state"));
            Self {
                config: config.join("selah"),
                cache: cache.join("selah"),
                logs: state.join("selah/logs"),
                library: home.join("Documents/Selah Library"),
            }
        }
    }
}

/// Write a file atomically: write a sibling temporary file, flush it, then rename it over the
/// target, so a crash never leaves a half-written file (SEC-15).
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("path has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let mut name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("path has no file name"))?
        .to_os_string();
    name.push(".tmp");
    let temporary = dir.join(name);
    {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selah_home_puts_everything_under_one_root() {
        let paths = Paths::under(PathBuf::from("/x"));
        assert_eq!(paths.config, PathBuf::from("/x/config"));
        assert_eq!(paths.logs, PathBuf::from("/x/logs"));
    }

    #[test]
    fn atomic_write_replaces_the_whole_file_and_leaves_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/settings.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }
}
