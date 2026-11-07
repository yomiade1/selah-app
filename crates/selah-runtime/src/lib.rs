#![deny(unsafe_code)]
//! Process plumbing shared by core, engine and watchdog. Only `power` (an OS call with no safe
//! API) may use unsafe code (SEC-11, tools/unsafe-allowlist.txt).
pub mod heartbeat;
pub mod instance;
pub mod logging;
pub mod paths;
pub mod power;
pub mod redact;

/// The product version, shown in the UI, logs and support bundles, and compared by the updater.
/// Installer builds set `SELAH_BUILD_VERSION` (tools/package.ps1: `0.0.<run>` for PRs,
/// `0.<N>.0` for milestones); other builds use the crate version.
pub const VERSION: &str = match option_env!("SELAH_BUILD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// True when the watchdog started this process in safe mode (SAFE-07).
pub fn safe_mode() -> bool {
    std::env::var("SELAH_SAFE_MODE").as_deref() == Ok("1")
}
