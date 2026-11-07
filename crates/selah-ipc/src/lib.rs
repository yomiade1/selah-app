//! Selah's local IPC (ADR-004, SEC-02).
//!
//! - [`frame`]: length-prefixed `Envelope` frames with a 16 MiB limit and strict validation.
//! - [`transport`]: named pipes (Windows) and Unix sockets, private to the current user.
//! - [`conn`] and [`client`]: async connections, request/reply with correlation, events and
//!   reconnect with backoff.
//! - [`shm`]: shared-memory rings for preview frames.
//!
//! Unsafe code is denied crate-wide and allowed only in the modules listed in
//! `tools/unsafe-allowlist.txt` (SEC-11).
#![deny(unsafe_code)]

pub mod client;
pub mod conn;
pub mod endpoint;
pub mod frame;
pub mod shm;
pub mod transport;
#[cfg(windows)]
pub mod windows;

pub use client::{Client, ConnectionState};
pub use conn::{Connection, FrameReader, FrameSender};
pub use endpoint::{Endpoint, Service, runtime_dir};
pub use frame::{MAX_FRAME_BYTES, decode_body, encode_frame, read_frame, write_frame};
pub use transport::{Listener, Peer};

use selah_proto::v1::{Envelope, Reply, envelope};

/// Build the reply to `request`, correlated to its id.
pub fn reply_to(request: &Envelope, actor: selah_proto::v1::Actor, result: Reply) -> Envelope {
    Envelope {
        id: request.id,
        correlation_id: request.id,
        actor: Some(actor),
        body: Some(envelope::Body::Reply(result)),
    }
}

/// Build an error reply to `request` using a catalogue code (for example `SEL-IPC-0001`).
pub fn error_to(
    request: &Envelope,
    actor: selah_proto::v1::Actor,
    code: &str,
    message_key: &str,
) -> Envelope {
    Envelope {
        id: request.id,
        correlation_id: request.id,
        actor: Some(actor),
        body: Some(envelope::Body::Error(selah_proto::v1::Error {
            code: code.into(),
            message_key: message_key.into(),
            ..Default::default()
        })),
    }
}
