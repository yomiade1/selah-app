#![forbid(unsafe_code)]
//! Selah core: the only owner of data (ADR-002). M1 holds settings, the live-state snapshot,
//! the permission framework (SEC-14) and request handling. The library owns SQLite and media.
pub mod diagnostics;
pub mod library;
pub mod live;
pub mod permissions;
pub mod scripture;
pub mod service;
pub mod settings;
pub mod songs;
pub mod support;
pub mod thumbnails;
pub mod update;

pub use service::{CoreService, Outcome, Work};
