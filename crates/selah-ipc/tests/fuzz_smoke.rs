//! Stable-toolchain fuzz smoke run for the frame decoder (SEC-03).
//!
//! The coverage-guided target lives in `fuzz/fuzz_targets/ipc_frame.rs` (cargo fuzz, nightly)
//! for long runs in cloud sessions. This test drives the same entry point with random and
//! mutated frames for `SELAH_FUZZ_SECONDS` (default 2; the CI gate uses 30).
use std::time::{Duration, Instant};

use prost::Message;
use selah_ipc::frame::fuzz_frames;
use selah_proto::v1::{Actor, Envelope, GetStatus, Query, envelope, query};

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn valid_frame() -> Vec<u8> {
    let message = Envelope {
        id: 1,
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "u".into(),
            device_id: "d".into(),
            origin: 1,
        }),
        body: Some(envelope::Body::Query(Query {
            request: Some(query::Request::GetStatus(GetStatus {})),
        })),
    };
    let body = message.encode_to_vec();
    let mut frame = (body.len() as u32).to_le_bytes().to_vec();
    frame.extend(body);
    frame
}

#[test]
fn frame_decoder_survives_random_and_mutated_input() {
    // sec: SEC-03
    let seconds: u64 = std::env::var("SELAH_FUZZ_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut rng = XorShift(0x5e1a_u64 ^ u64::from(std::process::id()));
    let seed = valid_frame();
    assert_eq!(fuzz_frames(&seed), 1);
    let mut runs = 0_u64;
    while Instant::now() < deadline {
        let mut input = if rng.below(4) == 0 {
            (0..rng.below(64))
                .map(|_| rng.next() as u8)
                .collect::<Vec<_>>()
        } else {
            let mut frames = seed.repeat(1 + rng.below(3));
            for _ in 0..1 + rng.below(6) {
                let at = rng.below(frames.len());
                match rng.below(4) {
                    0 => frames[at] = rng.next() as u8,
                    1 => frames[at] ^= 1 << rng.below(8),
                    2 => frames.truncate(at),
                    _ => frames.insert(at, rng.next() as u8),
                }
                if frames.is_empty() {
                    break;
                }
            }
            frames
        };
        // Occasionally claim a huge length with a short body.
        if rng.below(16) == 0 && input.len() >= 4 {
            input[..4].copy_from_slice(&(16_u32 * 1024 * 1024).to_le_bytes());
        }
        let _ = fuzz_frames(&input);
        runs += 1;
    }
    eprintln!("fuzz smoke: {runs} inputs in {seconds} s");
    assert!(runs > 0);
}
