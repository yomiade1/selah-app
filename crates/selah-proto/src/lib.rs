#![forbid(unsafe_code)]

/// Generated wire contracts. Actor data must be bound to an authenticated peer.
pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/selah.v1.rs"));
}

#[cfg(test)]
mod tests {
    use super::v1::{Actor, ActorOrigin, Envelope, GetStatus, Query, envelope, query};
    use prost::Message;

    #[test]
    fn envelope_round_trip_keeps_actor_and_correlation() {
        let original = Envelope {
            id: 42,
            correlation_id: 7,
            actor: Some(Actor {
                user_id: "local-owner".into(),
                device_id: "test-device".into(),
                origin: ActorOrigin::Ui as i32,
            }),
            body: Some(envelope::Body::Query(Query {
                request: Some(query::Request::GetStatus(GetStatus {})),
            })),
        };
        let bytes = original.encode_to_vec();
        assert_eq!(Envelope::decode(bytes.as_slice()).unwrap(), original);
    }

    #[test]
    fn truncated_envelope_is_rejected() {
        assert!(Envelope::decode([0x1a, 0x7f, 0x01].as_slice()).is_err());
    }

    #[test]
    fn wire_fixture_is_shared_with_csharp_and_typescript() {
        let hex = include_str!("../../../tests/fixtures/proto/status-query.hex").trim();
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
            .collect();
        let decoded = Envelope::decode(bytes.as_slice()).unwrap();
        assert_eq!(decoded.id, 42);
        assert_eq!(decoded.correlation_id, 7);
        assert_eq!(
            decoded.actor.as_ref().unwrap().origin,
            ActorOrigin::Ui as i32
        );
        assert_eq!(decoded.encode_to_vec(), bytes);
    }
}
