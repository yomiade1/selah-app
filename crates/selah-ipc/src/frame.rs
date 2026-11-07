//! Transport-independent framing (ADR-004): a 4-byte little-endian length, then an `Envelope`.
//! OS transports authenticate their peer before any envelope is accepted; an `Actor` on the
//! wire is attribution, not authentication.
use std::io::{self, Read, Write};

use prost::Message;
use selah_proto::v1::{ActorOrigin, Envelope, envelope};

pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
/// Bodies start this big and grow as bytes arrive.
pub const INITIAL_READ_CAPACITY: usize = 64 * 1024;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Validate length before allocating or reading a body, including fuzz input.
pub fn frame_length(header: [u8; 4]) -> io::Result<usize> {
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(invalid("frame length is outside the permitted range"));
    }
    Ok(length)
}

pub fn decode_body(bytes: &[u8]) -> io::Result<Envelope> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
        return Err(invalid("frame body is outside the permitted range"));
    }
    let message = Envelope::decode(bytes).map_err(|_| invalid("malformed protobuf envelope"))?;
    // Unknown protobuf fields are valid, but an unknown oneof must not become
    // an accepted command with a default value.
    validate(&message)?;
    Ok(message)
}

fn validate(message: &Envelope) -> io::Result<()> {
    let actor = message
        .actor
        .as_ref()
        .ok_or_else(|| invalid("missing actor"))?;
    if actor.user_id.is_empty()
        || actor.device_id.is_empty()
        || !matches!(ActorOrigin::try_from(actor.origin), Ok(origin) if origin != ActorOrigin::Unspecified)
    {
        return Err(invalid("invalid actor attribution"));
    }
    let known = match message.body.as_ref() {
        Some(envelope::Body::Command(command)) => command.action.is_some(),
        Some(envelope::Body::Query(query)) => query.request.is_some(),
        Some(envelope::Body::Event(event)) => event.notification.is_some(),
        Some(envelope::Body::Reply(reply)) => reply.result.is_some(),
        Some(envelope::Body::Error(error)) => !error.code.is_empty(),
        None => false,
    };
    if !known {
        return Err(invalid("missing or unknown message type"));
    }
    Ok(())
}

pub fn read_frame(reader: &mut impl Read) -> io::Result<Envelope> {
    let mut header = [0_u8; 4];
    reader.read_exact(&mut header)?;
    let length = frame_length(header)?;
    // Grow with the data actually received, so a header alone can't reserve 16 MiB.
    let mut body = Vec::with_capacity(length.min(INITIAL_READ_CAPACITY));
    reader.take(length as u64).read_to_end(&mut body)?;
    if body.len() != length {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "frame body is truncated",
        ));
    }
    decode_body(&body)
}

/// Encode a validated envelope as one frame (header and body in one buffer).
pub fn encode_frame(message: &Envelope) -> io::Result<Vec<u8>> {
    let length = message.encoded_len();
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(invalid("frame body is outside the permitted range"));
    }
    validate(message)?;
    let mut bytes = Vec::with_capacity(4 + length);
    bytes.extend_from_slice(&(length as u32).to_le_bytes());
    message
        .encode(&mut bytes)
        .map_err(|_| invalid("envelope could not be encoded"))?;
    Ok(bytes)
}

pub fn write_frame(writer: &mut impl Write, message: &Envelope) -> io::Result<()> {
    writer.write_all(&encode_frame(message)?)?;
    writer.flush()
}

/// Fuzz entry point (SEC-03): decode arbitrary bytes as a frame stream. Must never panic,
/// hang or allocate more than one frame limit. Used by `fuzz/` and the stable smoke test.
pub fn fuzz_frames(data: &[u8]) -> usize {
    let mut reader = data;
    let mut accepted = 0;
    while !reader.is_empty() {
        match read_frame(&mut reader) {
            Ok(message) => {
                // Anything accepted must re-encode within the limit.
                let _ = encode_frame(&message).expect("accepted frames re-encode");
                accepted += 1;
            }
            Err(_) => break,
        }
    }
    accepted
}

/// Saturating reconnect delay: 25, 50, 100, 200, 400, then 500 milliseconds.
pub fn reconnect_delay(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis(25_u64.saturating_mul(1_u64 << attempt.min(5)).min(500))
}

#[cfg(test)]
mod tests {
    use super::*;
    use selah_proto::v1::{Actor, GetStatus, Query, query};

    fn message(id: u64) -> Envelope {
        Envelope {
            id,
            correlation_id: id.saturating_sub(1),
            actor: Some(Actor {
                user_id: "local-owner".into(),
                device_id: "test-device".into(),
                origin: 1,
            }),
            body: Some(envelope::Body::Query(Query {
                request: Some(query::Request::GetStatus(GetStatus {})),
            })),
        }
    }

    #[test]
    fn ten_thousand_framing_round_trips_preserve_correlation() {
        for id in 1..=10_000 {
            let original = message(id);
            let mut buffer = Vec::new();
            write_frame(&mut buffer, &original).unwrap();
            assert_eq!(read_frame(&mut buffer.as_slice()).unwrap(), original);
        }
    }

    #[test]
    fn oversized_length_is_rejected_without_reading_body() {
        let header = ((MAX_FRAME_BYTES + 1) as u32).to_le_bytes();
        let error = read_frame(&mut header.as_slice()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn boundary_lengths_are_checked() {
        assert!(frame_length(0_u32.to_le_bytes()).is_err());
        assert_eq!(
            frame_length((MAX_FRAME_BYTES as u32).to_le_bytes()).unwrap(),
            MAX_FRAME_BYTES
        );
        assert!(frame_length(u32::MAX.to_le_bytes()).is_err());
    }

    #[test]
    fn truncated_header_and_body_are_rejected() {
        assert_eq!(
            read_frame(&mut [1, 0].as_slice()).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert_eq!(
            read_frame(&mut [4, 0, 0, 0, 1].as_slice())
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn malformed_unknown_and_actorless_messages_are_rejected() {
        assert!(decode_body(&[0x1a, 0x7f, 0x01]).is_err());
        let mut unknown = message(1);
        unknown.body = None;
        assert!(decode_body(&unknown.encode_to_vec()).is_err());
        let mut actorless = message(1);
        actorless.actor = None;
        assert!(decode_body(&actorless.encode_to_vec()).is_err());
    }

    #[test]
    fn concatenated_frames_do_not_consume_the_next_frame() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &message(1)).unwrap();
        write_frame(&mut bytes, &message(2)).unwrap();
        let mut reader = bytes.as_slice();
        assert_eq!(read_frame(&mut reader).unwrap().id, 1);
        assert_eq!(read_frame(&mut reader).unwrap().id, 2);
        assert!(reader.is_empty());
    }

    #[test]
    fn unknown_nested_message_and_origin_are_rejected() {
        let mut empty_query = message(1);
        empty_query.body = Some(envelope::Body::Query(Query { request: None }));
        assert!(decode_body(&empty_query.encode_to_vec()).is_err());
        for origin in [0, 9999] {
            let mut unknown_origin = message(1);
            unknown_origin.actor.as_mut().unwrap().origin = origin;
            assert!(decode_body(&unknown_origin.encode_to_vec()).is_err());
        }
    }

    #[test]
    fn reconnect_backoff_is_bounded_even_after_many_failures() {
        let delays: Vec<_> = (0..7).map(|n| reconnect_delay(n).as_millis()).collect();
        assert_eq!(delays, [25, 50, 100, 200, 400, 500, 500]);
        assert_eq!(reconnect_delay(u32::MAX).as_millis(), 500);
    }
}
