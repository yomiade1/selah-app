#![forbid(unsafe_code)]
//! Process plumbing shared by core, engine and watchdog.
pub mod heartbeat;
pub mod instance;
pub mod logging;
pub mod paths;
pub mod redact;

/// The product version, shown in the UI, logs and support bundles.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// True when the watchdog started this process in safe mode (SAFE-07).
pub fn safe_mode() -> bool {
    std::env::var("SELAH_SAFE_MODE").as_deref() == Ok("1")
}
