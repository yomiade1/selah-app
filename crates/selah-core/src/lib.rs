#![forbid(unsafe_code)]
//! Selah core: the only owner of data (ADR-002). M1 holds settings, the live-state snapshot,
//! the permission framework (SEC-14) and request handling; the library arrives in M2.
pub mod live;
pub mod permissions;
pub mod service;
pub mod settings;

pub use service::{CoreService, Outcome};
