#![deny(unsafe_code)]
//! Supervision policy (ARCHITECTURE §2, SAFE-07), kept free of process handling so it can be
//! tested directly.
pub mod keys;
pub mod policy;

/// Exit code a Selah process uses when another copy already holds its instance lock.
pub const EXIT_ALREADY_RUNNING: i32 = 3;
