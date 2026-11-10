//! Stable-toolchain fuzz smoke suite for OSIS, USFM, Zefania, and reference parsers (SEC-03, M2-A11).
//!
//! Coverage-guided targets live in `fuzz/fuzz_targets/bible_{osis,usfm,zefania}.rs` (`cargo fuzz`,
//! nightly). These tests drive `selah_core::scripture::fuzz_{osis,usfm,zefania,reference}` under
//! random, mutated, and adversarial inputs for `SELAH_FUZZ_SECONDS`.
use std::time::{Duration, Instant};

use selah_core::scripture::{
    MAX_BIBLE_FILE_BYTES, fuzz_osis, fuzz_reference, fuzz_usfm, fuzz_zefania,
};

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

fn fuzz_seconds() -> u64 {
    std::env::var("SELAH_FUZZ_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2)
}

fn run_fuzz_loop(
    label: &str,
    seed_magic: u64,
    seeds: &[&str],
    tokens: &[&[u8]],
    fuzz_fn: fn(&[u8]) -> usize,
) {
    let seconds = fuzz_seconds();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut rng = XorShift(seed_magic ^ u64::from(std::process::id()));

    for seed in seeds {
        assert!(fuzz_fn(seed.as_bytes()) > 0, "{label} seed must parse");
    }

    // One explicit over-limit check without reallocating 32 MiB inside the hot loop.
    let oversized = vec![b'<'; MAX_BIBLE_FILE_BYTES + 1];
    assert_eq!(fuzz_fn(&oversized), 0);

    let mut runs = 0_u64;
    while Instant::now() < deadline {
        let input = match rng.below(4) {
            0 => (0..rng.below(256))
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
                let seed = seeds[rng.below(seeds.len())].as_bytes();
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
        let _ = fuzz_fn(&input);
        runs += 1;
    }
    eprintln!("{label} fuzz smoke: {runs} inputs in {seconds} s");
    assert!(runs > 0);
}

#[test]
fn osis_parser_survives_random_and_mutated_input() {
    // sec: SEC-03
    const SEEDS: &[&str] = &[
        r#"<osis><osisText osisIDWork="KJV"><header><work osisWork="KJV"><title>KJV</title><language>en</language></work></header><div type="book" osisID="John"><verse osisID="John.3.16">For God so loved &amp; gave.</verse></div></osisText></osis>"#,
        r#"<osis><osisText osisIDWork="WEB"><div type="book" osisID="Ps"><verse osisID="Ps.23.1" sID="Ps.23.1"/><l>The LORD is my shepherd;</l><verse eID="Ps.23.1"/></div></osisText></osis>"#,
    ];
    let tokens: &[&[u8]] = &[
        b"<osis>",
        b"</osis>",
        b"<verse osisID=\"Gen.1.1\">",
        b"<verse sID=\"x\" osisID=\"Gen.1.1\"/>",
        b"<verse eID=\"x\"/>",
        b"<note>",
        b"</note>",
        b"<!DOCTYPE osis [<!ENTITY a \"b\">]>",
        b"&amp;",
        b"&unknown;",
        b"hello",
        b"\0",
    ];
    run_fuzz_loop("bible_osis", 0x0515_cafe_u64, SEEDS, tokens, fuzz_osis);
}

#[test]
fn usfm_parser_survives_random_and_mutated_input() {
    // sec: SEC-03
    const SEEDS: &[&str] = &[
        "\\id JHN Sample\n\\c 3\n\\v 16 For \\w God|strong=\"G2316\"\\w* so loved\\f + \\fr 3:16 \\ft note\\f* the world.",
        "\\id PSA\n\\c 23\n\\v 1 \\q1 The \\nd LORD\\nd* is my shepherd;\n\\q2 I shall lack nothing.",
    ];
    let tokens: &[&[u8]] = &[
        b"\\id GEN\n",
        b"\\c 1\n",
        b"\\v 1 ",
        b"\\w word|s=1\\w*",
        b"\\f + note\\f*",
        b"\\f + unclosed",
        b"\\q1 ",
        b"\\p\n",
        b"|",
        b"\\",
        b"\r\n",
        b"\0",
    ];
    run_fuzz_loop("bible_usfm", 0x05f3_beef_u64, SEEDS, tokens, fuzz_usfm);
}

#[test]
fn zefania_parser_survives_random_and_mutated_input() {
    // sec: SEC-03
    const SEEDS: &[&str] = &[
        r#"<XMLBIBLE biblename="Sample"><INFORMATION><identifier>SMP</identifier><language>ENG</language></INFORMATION><BIBLEBOOK bnumber="43"><CHAPTER cnumber="3"><VERS vnumber="16">For God so loved the world.<NOTE>note</NOTE></VERS></CHAPTER></BIBLEBOOK></XMLBIBLE>"#,
    ];
    let tokens: &[&[u8]] = &[
        b"<XMLBIBLE>",
        b"</XMLBIBLE>",
        b"<BIBLEBOOK bnumber=\"1\">",
        b"<CHAPTER cnumber=\"1\">",
        b"<VERS vnumber=\"1\">",
        b"</VERS>",
        b"<NOTE>",
        b"</NOTE>",
        b"<BR/>",
        b"<!DOCTYPE XMLBIBLE>",
        b"text",
        b"\0",
    ];
    run_fuzz_loop(
        "bible_zefania",
        0x2efa_91a1_u64,
        SEEDS,
        tokens,
        fuzz_zefania,
    );
}

#[test]
fn scripture_reference_parser_survives_random_and_mutated_input() {
    // sec: SEC-03
    let seconds = fuzz_seconds().min(2);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut rng = XorShift(0x4ef5_1234_u64 ^ u64::from(std::process::id()));

    for seed in [
        "John 3:16",
        "Rom 8:28, 31-32, 38-39",
        "요한복음 3장 16절",
        "1 Àwọn Ọba 18:21",
        "1ª João 4:8; Jude 24",
    ] {
        assert!(fuzz_reference(seed.as_bytes()) > 0);
    }

    let mut runs = 0_u64;
    while Instant::now() < deadline {
        let len = rng.below(600);
        let input: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        let _ = fuzz_reference(&input);
        runs += 1;
    }
    assert!(runs > 0);
}
