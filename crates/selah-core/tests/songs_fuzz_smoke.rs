//! Stable-toolchain fuzz smoke run for the ChordPro parser (SEC-03, M2-A11).
//!
//! The coverage-guided target lives in `fuzz/fuzz_targets/chordpro.rs` (cargo fuzz, nightly)
//! for long runs in cloud sessions. This test drives `selah_core::songs::fuzz_chordpro` with
//! random and mutated inputs for `SELAH_FUZZ_SECONDS` (default 2; the CI gate uses 30).
use std::time::{Duration, Instant};

use selah_core::songs::fuzz_chordpro;

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

const SEEDS: &[&str] = &[
    "{title: Amazing Grace}\n{artist: John Newton}\n{key: G}\n{sov}\n[G]Amazing grace! How [C]sweet the [G]sound\nThat saved a wretch like [D]me!\n{eov}\n{soc}\n[Em]Praise [D]God!\n{eoc}",
    "{sov: Verse 1}\n[D]’Twas grace that [G]taught my [D/F#]heart to fear\n{sob}\n[Bm7]When we’ve been [A7sus4]there\n{eob}\n{sot}\ne|--0--|\n{eot}",
    "{tags: hymn, classic}\n{tempo: 72}\n{ccli: 22025}\n{copyright: Public Domain}\n[Bb]Holy, [Eb]Holy, [F7]Holy!",
];

#[test]
fn chordpro_parser_survives_random_and_mutated_input() {
    // sec: SEC-03
    let seconds: u64 = std::env::var("SELAH_FUZZ_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut rng = XorShift(0x5e1a_4c6f_u64 ^ u64::from(std::process::id()));

    for seed in SEEDS {
        assert!(fuzz_chordpro(seed.as_bytes()) > 0);
    }

    let tokens = [
        b"{sov}".as_slice(),
        b"{eov}".as_slice(),
        b"{soc}".as_slice(),
        b"{eoc}".as_slice(),
        b"{sot}".as_slice(),
        b"{eot}".as_slice(),
        b"{title: x}".as_slice(),
        b"[G]".as_slice(),
        b"[C#m7/F#]".as_slice(),
        b"[not a chord".as_slice(),
        b"{{{".as_slice(),
        b"]]]".as_slice(),
        b"\n".as_slice(),
        b"\0".as_slice(),
    ];

    let mut runs = 0_u64;
    while Instant::now() < deadline {
        let mut input = match rng.below(4) {
            0 => (0..rng.below(128))
                .map(|_| rng.next() as u8)
                .collect::<Vec<_>>(),
            1 => {
                let mut buf = Vec::new();
                for _ in 0..1 + rng.below(24) {
                    buf.extend_from_slice(tokens[rng.below(tokens.len())]);
                }
                buf
            }
            _ => {
                let seed = SEEDS[rng.below(SEEDS.len())].as_bytes();
                let mut mutated = seed.to_vec();
                for _ in 0..1 + rng.below(8) {
                    if mutated.is_empty() {
                        break;
                    }
                    let at = rng.below(mutated.len());
                    match rng.below(4) {
                        0 => mutated[at] = rng.next() as u8,
                        1 => mutated[at] ^= 1 << rng.below(8),
                        2 => mutated.truncate(at),
                        _ => mutated.insert(at, rng.next() as u8),
                    }
                }
                mutated
            }
        };
        if rng.below(64) == 0 {
            input.resize(selah_core::songs::MAX_PASTE_BYTES + 128, b'a');
        }
        let _ = fuzz_chordpro(&input);
        runs += 1;
    }
    eprintln!("chordpro fuzz smoke: {runs} inputs in {seconds} s");
    assert!(runs > 0);
}
