//! Stable-toolchain fuzz smoke run for `.selah` bundle parsing (SEC-03, M2-A11).
//!
//! Coverage-guided fuzzing lives in `fuzz/fuzz_targets/selah_bundle.rs` (`cargo fuzz`, nightly).
//! This test drives `selah_core::library::fuzz_selah_bundle` under random, ZIP-token-spliced, and
//! bit-mutated bundle bytes for `SELAH_FUZZ_SECONDS` (default 2; CI uses 30).
use std::time::{Duration, Instant};

use selah_core::library::{fuzz_selah_bundle, new_id};
use selah_core::{CoreService, Outcome};
use selah_proto::v1::*;

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

fn ui_cmd(id: u64, action: command::Action) -> Envelope {
    Envelope {
        id,
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "fuzz".into(),
            device_id: "seed".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Command(Command {
            action: Some(action),
            ..Default::default()
        })),
    }
}

fn assert_ok(out: Outcome) -> reply::Result {
    match out.reply.and_then(|e| e.body) {
        Some(envelope::Body::Reply(r)) => r.result.expect("missing reply variant"),
        other => panic!("unexpected outcome: {other:?}"),
    }
}

fn build_seed_bundles() -> Vec<Vec<u8>> {
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    let verse = Section {
        id: Some(new_id()),
        label: "Verse 1".into(),
        kind: SectionKind::Verse as i32,
        hotkey: "V".into(),
        lines: vec![LyricLine {
            text: "Grace will lead me home".into(),
            chords: vec![Chord {
                position: 0,
                text: "G".into(),
            }],
        }],
        ..Default::default()
    };
    let song = Song {
        id: Some(new_id()),
        title: "Fuzz Seed Hymn".into(),
        authors: vec!["Anonymous".into()],
        ccli_number: "0".into(),
        copyright: "Public Domain".into(),
        key: "G".into(),
        tempo: 72.0,
        tags: vec!["hymn".into()],
        public_domain: true,
        arrangements: vec![Arrangement {
            id: Some(new_id()),
            name: "Default".into(),
            section_ids: vec![verse.id.clone().unwrap()],
        }],
        sections: vec![verse],
    };
    assert_ok(core.handle(&ui_cmd(
        1,
        command::Action::SaveSong(SaveSong { song: Some(song) }),
    )));

    let tag = Tag {
        id: Some(new_id()),
        name: "hymn".into(),
    };
    assert_ok(core.handle(&ui_cmd(
        2,
        command::Action::SaveTag(SaveTag { tag: Some(tag) }),
    )));

    let raw_media = b"fuzz media blob bytes";
    let blob = core
        .library()
        .media_store()
        .unwrap()
        .import_reader(raw_media.as_slice(), "png", 1024)
        .unwrap();
    let item = MediaItem {
        id: Some(new_id()),
        name: "Fuzz Image".into(),
        kind: MediaKind::Image as i32,
        content_hash: blob.content_hash,
        extension: blob.extension,
        size_bytes: blob.size_bytes,
        resolution: Some(Resolution {
            width: 32,
            height: 32,
        }),
        duration_milliseconds: 0,
        tags: vec!["backdrop".into()],
        ..Default::default()
    };
    assert_ok(core.handle(&ui_cmd(
        3,
        command::Action::SaveMediaItem(SaveMediaItem {
            media_item: Some(item),
        }),
    )));

    let bundle_no_media = dir.path().join("seed-meta.selah");
    assert_ok(core.handle(&ui_cmd(
        4,
        command::Action::ExportSelahBundle(ExportSelahBundle {
            output_path: bundle_no_media.to_string_lossy().into_owned(),
            include_media: false,
            ..Default::default()
        }),
    )));

    let bundle_with_media = dir.path().join("seed-full.selah");
    assert_ok(core.handle(&ui_cmd(
        5,
        command::Action::ExportSelahBundle(ExportSelahBundle {
            output_path: bundle_with_media.to_string_lossy().into_owned(),
            include_media: true,
            ..Default::default()
        }),
    )));

    vec![
        std::fs::read(&bundle_no_media).unwrap(),
        std::fs::read(&bundle_with_media).unwrap(),
    ]
}

#[test]
fn selah_bundle_parser_survives_random_and_mutated_input() {
    // sec: SEC-03
    let seconds: u64 = std::env::var("SELAH_FUZZ_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let seeds = build_seed_bundles();
    for seed in &seeds {
        assert!(fuzz_selah_bundle(seed) >= 3);
    }

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut rng = XorShift(0x5e1a_b0ad_u64 ^ u64::from(std::process::id()));
    let fragments: &[&[u8]] = &[
        b"PK\x03\x04",
        b"PK\x01\x02",
        b"PK\x05\x06",
        b"manifest.json",
        b"../evil",
        b"media/",
        br#"{"format":"selah-bundle","schema_version":1,"songs":[],"templates":[],"media_items":[],"presentations":[],"tags":[]}"#,
        b"\x00\xff\xff\xff\xff",
    ];

    let mut runs = 0_u64;
    while Instant::now() < deadline {
        let mut input = match rng.below(5) {
            0 => (0..rng.below(512))
                .map(|_| rng.next() as u8)
                .collect::<Vec<_>>(),
            1 => {
                let mut buf = Vec::new();
                for _ in 0..1 + rng.below(16) {
                    buf.extend_from_slice(fragments[rng.below(fragments.len())]);
                }
                buf
            }
            _ => {
                let seed = &seeds[rng.below(seeds.len())];
                let mut mutated = seed.clone();
                for _ in 0..1 + rng.below(12) {
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
            input.resize(8 * 1024 * 1024 + 64, 0x50);
        }
        let _ = fuzz_selah_bundle(&input);
        runs += 1;
    }
    eprintln!("selah_bundle fuzz smoke: {runs} inputs in {seconds} s");
    assert!(runs > 0);
}
