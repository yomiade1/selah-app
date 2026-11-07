#![deny(unsafe_code)]
//! Supervision policy (ARCHITECTURE §2, SAFE-07), kept free of process handling so it can be
//! tested directly.
pub mod keys;
pub mod policy;

/// Exit code a Selah process uses when another copy already holds its instance lock.
pub const EXIT_ALREADY_RUNNING: i32 = 3;

/// True when the installer called the launcher for one of its hooks (`--veloapp-install`,
/// `--veloapp-updated`, `--veloapp-obsolete`, `--veloapp-uninstall`). The launcher exits at once.
pub fn is_installer_hook(args: &[String]) -> bool {
    args.first()
        .is_some_and(|arg| arg.starts_with("--veloapp-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_hooks_never_start_selah() {
        let hook = |args: &[&str]| {
            is_installer_hook(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
        };
        assert!(hook(&["--veloapp-install", "0.0.1"]));
        assert!(hook(&["--veloapp-uninstall", "0.0.1"]));
        assert!(!hook(&[]));
        assert!(!hook(&["--headless"]));
        assert!(!hook(&["--safe-mode", "--veloapp-install"]));
    }
}
