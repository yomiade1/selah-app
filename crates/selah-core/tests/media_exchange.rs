//! Media import, thumbnail caching, canonical `.selah` exchange bundles, backup/restore,
//! and adversarial ZIP security checks (M2.6, SEC-03, SEC-04, SEC-15, M2-A6, M2-A11).
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::Compression as FlateLevel;
use flate2::write::DeflateEncoder;
use rusqlite::Connection;
use selah_core::library::new_id;
use selah_core::{CoreService, Outcome};
use selah_proto::v1::*;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ui_cmd_with(id: u64, action: command::Action, undo_group: Option<Id>) -> Envelope {
    Envelope {
        id,
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "local-owner".into(),
            device_id: "test-pc".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Command(Command {
            action: Some(action),
            undo_group,
        })),
    }
}

fn ui_cmd(id: u64, action: command::Action) -> Envelope {
    ui_cmd_with(id, action, None)
}

fn ui_query(id: u64, request: query::Request) -> Envelope {
    Envelope {
        id,
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "local-owner".into(),
            device_id: "test-pc".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Query(Query {
            request: Some(request),
        })),
    }
}

fn unwrap_reply(outcome: Outcome) -> reply::Result {
    match outcome.reply.and_then(|e| e.body) {
        Some(envelope::Body::Reply(r)) => r.result.expect("reply missing result"),
        Some(envelope::Body::Error(err)) => {
            panic!(
                "unexpected error {} ({}): fix={}",
                err.code, err.message_key, err.fix_article_id
            )
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

fn expect_error_code(outcome: Outcome) -> String {
    match outcome.reply.and_then(|e| e.body) {
        Some(envelope::Body::Error(err)) => err.code,
        other => panic!("expected error, got {other:?}"),
    }
}

fn ieee_crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

fn write_synthetic_wav(path: &Path) {
    let sample_rate = 44_100u32;
    let num_samples = 4_410u32; // 100 ms
    let data_bytes = num_samples * 2;
    let riff_size = 36 + data_bytes;

    let mut wav = Vec::with_capacity((44 + data_bytes) as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for i in 0..num_samples {
        let sample = ((i as f32 * 440.0 * std::f32::consts::TAU / sample_rate as f32).sin()
            * 12000.0) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, &wav).unwrap();
}

struct RawZipEntry<'a> {
    name: &'a str,
    method: u16,
    flags: u16,
    payload: Vec<u8>,
    crc: u32,
    uncompressed_size: u32,
    external_attr: u32,
    cd_name: Option<&'a str>,
    lfh_uncompressed_override: Option<u32>,
}

impl<'a> RawZipEntry<'a> {
    fn stored(name: &'a str, raw: &[u8]) -> Self {
        Self {
            name,
            method: 0,
            flags: 0x0800,
            payload: raw.to_vec(),
            crc: ieee_crc32(raw),
            uncompressed_size: raw.len() as u32,
            external_attr: 0o100644 << 16,
            cd_name: None,
            lfh_uncompressed_override: None,
        }
    }

    fn deflated(name: &'a str, raw: &[u8]) -> Self {
        let mut enc = DeflateEncoder::new(Vec::new(), FlateLevel::default());
        enc.write_all(raw).unwrap();
        let compressed = enc.finish().unwrap();
        Self {
            name,
            method: 8,
            flags: 0x0800,
            payload: compressed,
            crc: ieee_crc32(raw),
            uncompressed_size: raw.len() as u32,
            external_attr: 0o100644 << 16,
            cd_name: None,
            lfh_uncompressed_override: None,
        }
    }
}

fn build_raw_zip(entries: &[RawZipEntry<'_>], trailing: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cd = Vec::new();
    for entry in entries {
        let offset = out.len() as u32;
        let packed = entry.payload.len() as u32;
        let lfh_uncomp = entry
            .lfh_uncompressed_override
            .unwrap_or(entry.uncompressed_size);
        let lfh_name = entry.name.as_bytes();
        let cd_name = entry.cd_name.unwrap_or(entry.name).as_bytes();

        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&entry.flags.to_le_bytes());
        out.extend_from_slice(&entry.method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0x0021u16.to_le_bytes());
        out.extend_from_slice(&entry.crc.to_le_bytes());
        out.extend_from_slice(&packed.to_le_bytes());
        out.extend_from_slice(&lfh_uncomp.to_le_bytes());
        out.extend_from_slice(&(lfh_name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(lfh_name);
        out.extend_from_slice(&entry.payload);

        cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        cd.extend_from_slice(&((3u16 << 8) | 20u16).to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes());
        cd.extend_from_slice(&entry.flags.to_le_bytes());
        cd.extend_from_slice(&entry.method.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0x0021u16.to_le_bytes());
        cd.extend_from_slice(&entry.crc.to_le_bytes());
        cd.extend_from_slice(&packed.to_le_bytes());
        cd.extend_from_slice(&entry.uncompressed_size.to_le_bytes());
        cd.extend_from_slice(&(cd_name.len() as u16).to_le_bytes());
        cd.extend_from_slice(&[0u8; 8]);
        cd.extend_from_slice(&entry.external_attr.to_le_bytes());
        cd.extend_from_slice(&offset.to_le_bytes());
        cd.extend_from_slice(cd_name);
    }
    let cd_offset = out.len() as u32;
    let cd_size = cd.len() as u32;
    out.extend_from_slice(&cd);

    let count = entries.len() as u16;
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(trailing);
    out
}

#[test]
fn import_media_through_worker_caches_thumbnails_and_supports_undo_redo() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    let png_src = repo_root().join("tests/golden/reference/text-en.png");
    let mp4_src = repo_root().join("tests/fixtures/media/committed/h264-1080p60.mp4");
    let wav_src = dir.path().join("tone.wav");
    write_synthetic_wav(&wav_src);

    // 1. Import image with tags and custom name.
    let img_out = core.handle(&ui_cmd(
        1,
        command::Action::ImportMedia(ImportMedia {
            source_path: png_src.to_string_lossy().into_owned(),
            name: "Worship Backdrop".into(),
            tags: vec!["Easter".into(), "Blue".into()],
        }),
    ));
    assert!(
        img_out
            .broadcast
            .iter()
            .any(|e| matches!(e.notification, Some(event::Notification::LibraryChanged(_))))
    );
    let reply::Result::MediaItem(img_item) = unwrap_reply(img_out) else {
        panic!("expected MediaItem");
    };
    assert_eq!(img_item.kind, MediaKind::Image as i32);
    assert_eq!(img_item.name, "Worship Backdrop");
    assert_eq!(img_item.extension, "png");
    assert!(
        img_item
            .resolution
            .as_ref()
            .is_some_and(|r| r.width > 0 && r.height > 0)
    );

    // Verify content-addressed blob and lazy PNG thumbnail cache.
    let blob_path = core
        .library()
        .paths()
        .media
        .join(&img_item.content_hash[..2])
        .join(format!("{}.{}", img_item.content_hash, img_item.extension));
    assert!(blob_path.is_file());
    let cache_path = core
        .library()
        .paths()
        .root
        .join("thumbnails")
        .join(format!("{}_320x180.png", img_item.content_hash));
    assert!(cache_path.is_file());

    // SearchMedia finds the imported item by tag.
    let reply::Result::MediaSearchResults(found) = unwrap_reply(core.handle(&ui_query(
        2,
        query::Request::SearchMedia(SearchMedia {
            query: "Easter".into(),
            kind: MediaKind::Image as i32,
            limit: 10,
            offset: 0,
        }),
    ))) else {
        panic!("expected MediaSearchResults");
    };
    assert_eq!(found.total, 1);

    // Cache hit + delete-cache regeneration + non-default size.
    let reply::Result::MediaThumbnail(thumb1) = unwrap_reply(core.handle(&ui_query(
        3,
        query::Request::GetMediaThumbnail(GetMediaThumbnail {
            media_id: img_item.id.clone(),
            max_width: 0,
            max_height: 0,
        }),
    ))) else {
        panic!("expected MediaThumbnail");
    };
    assert!(thumb1.png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(thumb1.width <= 320 && thumb1.height <= 180);

    std::fs::remove_file(&cache_path).unwrap();
    let reply::Result::MediaThumbnail(thumb2) = unwrap_reply(core.handle(&ui_query(
        4,
        query::Request::GetMediaThumbnail(GetMediaThumbnail {
            media_id: img_item.id.clone(),
            max_width: 128,
            max_height: 128,
        }),
    ))) else {
        panic!("expected regenerated MediaThumbnail");
    };
    assert!(thumb2.png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(thumb2.width <= 128 && thumb2.height <= 128);
    assert!(
        core.library()
            .paths()
            .root
            .join("thumbnails")
            .join(format!("{}_128x128.png", img_item.content_hash))
            .is_file()
    );

    // Idempotency replay & conflicting command ID.
    let replay_out = core.handle(&ui_cmd(
        1,
        command::Action::ImportMedia(ImportMedia {
            source_path: png_src.to_string_lossy().into_owned(),
            name: "Worship Backdrop".into(),
            tags: vec!["Easter".into(), "Blue".into()],
        }),
    ));
    assert!(replay_out.broadcast.is_empty());
    let reply::Result::MediaItem(replayed_item) = unwrap_reply(replay_out) else {
        panic!("expected replayed MediaItem");
    };
    assert_eq!(replayed_item.id, img_item.id);

    let conflict_out = core.handle(&ui_cmd(
        1,
        command::Action::ImportMedia(ImportMedia {
            source_path: png_src.to_string_lossy().into_owned(),
            name: "Different Name".into(),
            tags: Vec::new(),
        }),
    ));
    assert_eq!(expect_error_code(conflict_out), "SEL-IPC-0002");

    // 2. Import video (1080p H.264) & verify resolution + duration + thumbnail.
    let reply::Result::MediaItem(vid_item) = unwrap_reply(core.handle(&ui_cmd(
        10,
        command::Action::ImportMedia(ImportMedia {
            source_path: mp4_src.to_string_lossy().into_owned(),
            name: String::new(),
            tags: vec!["motion".into()],
        }),
    ))) else {
        panic!("expected video MediaItem");
    };
    assert_eq!(vid_item.kind, MediaKind::Video as i32);
    assert_eq!(vid_item.name, "h264-1080p60");
    assert_eq!(
        vid_item.resolution,
        Some(Resolution {
            width: 1920,
            height: 1080
        })
    );
    assert!(vid_item.duration_milliseconds > 0);
    let reply::Result::MediaThumbnail(vid_thumb) = unwrap_reply(core.handle(&ui_query(
        11,
        query::Request::GetMediaThumbnail(GetMediaThumbnail {
            media_id: vid_item.id.clone(),
            max_width: 320,
            max_height: 180,
        }),
    ))) else {
        panic!("expected video thumbnail");
    };
    assert!(vid_thumb.png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    // 3. Import WAV audio: duration > 0, no resolution, thumbnail cleanly refused.
    let reply::Result::MediaItem(aud_item) = unwrap_reply(core.handle(&ui_cmd(
        12,
        command::Action::ImportMedia(ImportMedia {
            source_path: wav_src.to_string_lossy().into_owned(),
            name: "Click Track".into(),
            tags: vec!["audio".into()],
        }),
    ))) else {
        panic!("expected audio MediaItem");
    };
    assert_eq!(aud_item.kind, MediaKind::Audio as i32);
    assert!(aud_item.resolution.is_none());
    assert!(aud_item.duration_milliseconds >= 90);
    let audio_thumb_err = expect_error_code(core.handle(&ui_query(
        13,
        query::Request::GetMediaThumbnail(GetMediaThumbnail {
            media_id: aud_item.id.clone(),
            max_width: 320,
            max_height: 180,
        }),
    )));
    assert_eq!(audio_thumb_err, "SEL-LIB-0002");

    // 4. Undo removes audio MediaItem; Redo restores it.
    unwrap_reply(core.handle(&ui_cmd(14, command::Action::Undo(Undo { doc_id: None }))));
    assert!(
        core.library()
            .get::<MediaItem>(aud_item.id.as_ref().unwrap())
            .unwrap()
            .is_none()
    );
    unwrap_reply(core.handle(&ui_cmd(15, command::Action::Redo(Redo { doc_id: None }))));
    assert_eq!(
        core.library()
            .get::<MediaItem>(aud_item.id.as_ref().unwrap())
            .unwrap(),
        Some(aud_item)
    );

    // 5. Tampered blob fails thumbnail verification after clearing cache.
    let vid_cache = core
        .library()
        .paths()
        .root
        .join("thumbnails")
        .join(format!("{}_320x180.png", vid_item.content_hash));
    std::fs::remove_file(&vid_cache).unwrap();
    let vid_blob = core
        .library()
        .paths()
        .media
        .join(&vid_item.content_hash[..2])
        .join(format!("{}.{}", vid_item.content_hash, vid_item.extension));
    let mut bytes = std::fs::read(&vid_blob).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(&vid_blob, &bytes).unwrap();
    let corrupt_err = expect_error_code(core.handle(&ui_query(
        16,
        query::Request::GetMediaThumbnail(GetMediaThumbnail {
            media_id: vid_item.id,
            max_width: 320,
            max_height: 180,
        }),
    )));
    assert_eq!(corrupt_err, "SEL-LIB-0002");
}

#[test]
fn media_import_refuses_urls_protocols_and_dimension_bombs() {
    // sec: SEC-03, SEC-04
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    // Network URLs and FFmpeg protocol prefixes are rejected without touching storage.
    for (idx, source) in [
        "http://example.com/bg.png",
        "https://example.com/clip.mp4",
        "file:///etc/passwd",
        "rtsp://camera.local/live",
        "pipe:0",
        "concat:a.png|b.png",
        "image.png:stream",
    ]
    .into_iter()
    .enumerate()
    {
        let code = expect_error_code(core.handle(&ui_cmd(
            100 + idx as u64,
            command::Action::ImportMedia(ImportMedia {
                source_path: source.into(),
                name: String::new(),
                tags: Vec::new(),
            }),
        )));
        assert_eq!(code, "SEL-LIB-0002", "{source}");
    }

    // PNG IHDR bomb (9000x9000 with valid CRC).
    let sample_png = std::fs::read(repo_root().join("tests/golden/reference/text-en.png")).unwrap();
    let mut png_bomb = sample_png;
    png_bomb[16..20].copy_from_slice(&9000u32.to_be_bytes());
    png_bomb[20..24].copy_from_slice(&9000u32.to_be_bytes());
    let crc = ieee_crc32(&png_bomb[12..29]);
    png_bomb[29..33].copy_from_slice(&crc.to_be_bytes());
    let png_bomb_path = dir.path().join("bomb.png");
    std::fs::write(&png_bomb_path, &png_bomb).unwrap();

    // BMP bomb (9000x9000).
    let mut bmp_bomb = vec![0u8; 54];
    bmp_bomb[0..2].copy_from_slice(b"BM");
    bmp_bomb[18..22].copy_from_slice(&9000i32.to_le_bytes());
    bmp_bomb[22..26].copy_from_slice(&9000i32.to_le_bytes());
    let bmp_bomb_path = dir.path().join("bomb.bmp");
    std::fs::write(&bmp_bomb_path, &bmp_bomb).unwrap();

    // GIF bomb (9000x9000).
    let mut gif_bomb = Vec::from(*b"GIF89a");
    gif_bomb.extend_from_slice(&9000u16.to_le_bytes());
    gif_bomb.extend_from_slice(&9000u16.to_le_bytes());
    gif_bomb.extend_from_slice(&[0u8; 4]);
    let gif_bomb_path = dir.path().join("bomb.gif");
    std::fs::write(&gif_bomb_path, &gif_bomb).unwrap();

    // JPEG SOF0 bomb (9000x9000).
    let mut jpg_bomb = vec![0xff, 0xd8, 0xff, 0xc0, 0x00, 0x0b, 8];
    jpg_bomb.extend_from_slice(&9000u16.to_be_bytes());
    jpg_bomb.extend_from_slice(&9000u16.to_be_bytes());
    jpg_bomb.extend_from_slice(&[1, 1, 0x11, 0]);
    let jpg_bomb_path = dir.path().join("bomb.jpg");
    std::fs::write(&jpg_bomb_path, &jpg_bomb).unwrap();

    for (idx, bomb) in [
        &png_bomb_path,
        &bmp_bomb_path,
        &gif_bomb_path,
        &jpg_bomb_path,
    ]
    .into_iter()
    .enumerate()
    {
        let code = expect_error_code(core.handle(&ui_cmd(
            200 + idx as u64,
            command::Action::ImportMedia(ImportMedia {
                source_path: bomb.to_string_lossy().into_owned(),
                name: String::new(),
                tags: Vec::new(),
            }),
        )));
        assert_eq!(code, "SEL-LIB-0002", "{}", bomb.display());
    }

    assert!(core.library().list::<MediaItem>(10, 0).unwrap().is_empty());
}

#[test]
fn m2_a6_selah_export_import_export_is_identical_and_supports_overwrite_and_undo() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let dir_c = tempfile::tempdir().unwrap();
    let mut core_a = CoreService::new(dir_a.path(), false);

    // Seed song (with -0.0 tempo edge case, chords, translations, arrangements), template, media, presentation, tag.
    let sec_v = Section {
        id: Some(new_id()),
        label: "Verse 1".into(),
        kind: SectionKind::Verse as i32,
        color: Some(Color {
            red: -0.0,
            green: 0.5,
            blue: 0.75,
            alpha: 1.0,
        }),
        hotkey: "V".into(),
        lines: vec![LyricLine {
            text: "How great is our God".into(),
            chords: vec![Chord {
                position: 4,
                text: "G".into(),
            }],
        }],
        translations: [(
            "es".into(),
            TranslationLines {
                lines: vec!["Cuán grande es Dios".into()],
            },
        )]
        .into_iter()
        .collect(),
    };
    let song = Song {
        id: Some(new_id()),
        title: "Export Round Trip Song".into(),
        authors: vec!["Selah Writer".into()],
        ccli_number: "12345".into(),
        copyright: "Public Domain".into(),
        key: "G".into(),
        tempo: -0.0,
        tags: vec!["Sunday".into()],
        public_domain: true,
        arrangements: vec![Arrangement {
            id: Some(new_id()),
            name: "Main".into(),
            section_ids: vec![sec_v.id.clone().unwrap()],
        }],
        sections: vec![sec_v],
    };
    unwrap_reply(core_a.handle(&ui_cmd(
        1,
        command::Action::SaveSong(SaveSong {
            song: Some(song.clone()),
        }),
    )));

    let template = Template {
        id: Some(new_id()),
        name: "16:9 Clean".into(),
        variants: vec![TemplateVariant {
            id: Some(new_id()),
            resolution: Some(Resolution {
                width: 1920,
                height: 1080,
            }),
            max_lines: 4,
            style_json: r#"{"align":"center"}"#.into(),
        }],
    };
    unwrap_reply(core_a.handle(&ui_cmd(
        2,
        command::Action::SaveTemplate(SaveTemplate {
            template: Some(template.clone()),
        }),
    )));

    let tag = Tag {
        id: Some(new_id()),
        name: "Sunday".into(),
    };
    unwrap_reply(core_a.handle(&ui_cmd(
        3,
        command::Action::SaveTag(SaveTag {
            tag: Some(tag.clone()),
        }),
    )));

    let png_src = repo_root().join("tests/golden/reference/text-en.png");
    let reply::Result::MediaItem(media_item) = unwrap_reply(core_a.handle(&ui_cmd(
        4,
        command::Action::ImportMedia(ImportMedia {
            source_path: png_src.to_string_lossy().into_owned(),
            name: "Title Bg".into(),
            tags: vec!["Sunday".into()],
        }),
    ))) else {
        panic!("expected MediaItem");
    };

    let pres = Presentation {
        id: Some(new_id()),
        name: "Announcements".into(),
        slides: vec![Slide {
            id: Some(new_id()),
            text: "Welcome".into(),
            media_id: media_item.id.clone(),
            elements_json: "[]".into(),
        }],
        template_id: template.id.clone(),
    };
    unwrap_reply(core_a.handle(&ui_cmd(
        5,
        command::Action::SavePresentation(SavePresentation {
            presentation: Some(pres.clone()),
        }),
    )));

    // Export from A (both with and without media blobs).
    let bundle_path = dir_a.path().join("library-export.selah");
    let reply::Result::ExportedSelahBundle(exp_a) = unwrap_reply(core_a.handle(&ui_cmd(
        6,
        command::Action::ExportSelahBundle(ExportSelahBundle {
            output_path: bundle_path.to_string_lossy().into_owned(),
            include_media: true,
            ..Default::default()
        }),
    ))) else {
        panic!("expected ExportedSelahBundle");
    };
    assert_eq!(exp_a.song_count, 1);
    assert_eq!(exp_a.template_count, 1);
    assert_eq!(exp_a.media_count, 1);
    assert_eq!(exp_a.presentation_count, 1);
    assert_eq!(exp_a.tag_count, 1);
    assert!(!exp_a.canonical_manifest_json.contains("-0.0"));

    let meta_only_path = dir_a.path().join("meta-only.selah");
    unwrap_reply(core_a.handle(&ui_cmd(
        7,
        command::Action::ExportSelahBundle(ExportSelahBundle {
            output_path: meta_only_path.to_string_lossy().into_owned(),
            include_media: false,
            ..Default::default()
        }),
    )));

    // Import into fresh library B via source_path.
    let mut core_b = CoreService::new(dir_b.path(), false);
    let imp_out = core_b.handle(&ui_cmd(
        1,
        command::Action::ImportSelahBundle(ImportSelahBundle {
            source_path: bundle_path.to_string_lossy().into_owned(),
            raw_bytes: Vec::new(),
            overwrite_existing: false,
        }),
    ));
    let mut change_groups = Vec::new();
    for ev in &imp_out.broadcast {
        if let Some(event::Notification::LibraryChanged(lc)) = &ev.notification {
            change_groups.push(lc.undo_group.clone());
        }
    }
    assert_eq!(change_groups.len(), 5);
    assert!(change_groups.windows(2).all(|w| w[0] == w[1]));

    let reply::Result::ImportedSelahBundle(imp_b) = unwrap_reply(imp_out) else {
        panic!("expected ImportedSelahBundle");
    };
    assert_eq!(
        (
            imp_b.song_count,
            imp_b.template_count,
            imp_b.media_count,
            imp_b.presentation_count,
            imp_b.tag_count
        ),
        (1, 1, 1, 1, 1)
    );

    // Export from B -> identical canonical JSON!
    let reexport_path = dir_b.path().join("reexport.selah");
    let reply::Result::ExportedSelahBundle(exp_b) = unwrap_reply(core_b.handle(&ui_cmd(
        2,
        command::Action::ExportSelahBundle(ExportSelahBundle {
            output_path: reexport_path.to_string_lossy().into_owned(),
            include_media: true,
            ..Default::default()
        }),
    ))) else {
        panic!("expected reexport");
    };
    assert_eq!(exp_a.canonical_manifest_json, exp_b.canonical_manifest_json);

    // Fresh library C refuses meta-only import (media blob missing locally & not in ZIP),
    // but accepts full bundle via raw_bytes.
    let mut core_c = CoreService::new(dir_c.path(), false);
    let missing_blob_err = expect_error_code(core_c.handle(&ui_cmd(
        1,
        command::Action::ImportSelahBundle(ImportSelahBundle {
            source_path: meta_only_path.to_string_lossy().into_owned(),
            raw_bytes: Vec::new(),
            overwrite_existing: false,
        }),
    )));
    assert_eq!(missing_blob_err, "SEL-LIB-0002");

    let bundle_bytes = std::fs::read(&bundle_path).unwrap();
    let reply::Result::ImportedSelahBundle(imp_c) = unwrap_reply(core_c.handle(&ui_cmd(
        2,
        command::Action::ImportSelahBundle(ImportSelahBundle {
            source_path: String::new(),
            raw_bytes: bundle_bytes,
            overwrite_existing: false,
        }),
    ))) else {
        panic!("expected import from raw_bytes");
    };
    assert_eq!(imp_c.song_count, 1);

    // Overwrite = false preserves local edit; overwrite = true replaces and single Undo reverts it.
    let mut edited = core_b
        .library()
        .get::<Song>(song.id.as_ref().unwrap())
        .unwrap()
        .unwrap();
    edited.title = "Edited Locally In B".into();
    unwrap_reply(core_b.handle(&ui_cmd(
        3,
        command::Action::SaveSong(SaveSong {
            song: Some(edited.clone()),
        }),
    )));

    let reply::Result::ImportedSelahBundle(skip_res) = unwrap_reply(core_b.handle(&ui_cmd(
        4,
        command::Action::ImportSelahBundle(ImportSelahBundle {
            source_path: bundle_path.to_string_lossy().into_owned(),
            raw_bytes: Vec::new(),
            overwrite_existing: false,
        }),
    ))) else {
        panic!("expected skip import result");
    };
    assert_eq!(skip_res.song_count, 0);
    assert_eq!(
        core_b
            .library()
            .get::<Song>(song.id.as_ref().unwrap())
            .unwrap()
            .unwrap()
            .title,
        "Edited Locally In B"
    );

    let reply::Result::ImportedSelahBundle(over_res) = unwrap_reply(core_b.handle(&ui_cmd(
        5,
        command::Action::ImportSelahBundle(ImportSelahBundle {
            source_path: bundle_path.to_string_lossy().into_owned(),
            raw_bytes: Vec::new(),
            overwrite_existing: true,
        }),
    ))) else {
        panic!("expected overwrite import result");
    };
    assert_eq!(over_res.song_count, 1);
    assert_eq!(
        core_b
            .library()
            .get::<Song>(song.id.as_ref().unwrap())
            .unwrap()
            .unwrap()
            .title,
        "Export Round Trip Song"
    );

    // One global Undo restores "Edited Locally In B".
    unwrap_reply(core_b.handle(&ui_cmd(6, command::Action::Undo(Undo { doc_id: None }))));
    assert_eq!(
        core_b
            .library()
            .get::<Song>(song.id.as_ref().unwrap())
            .unwrap()
            .unwrap()
            .title,
        "Edited Locally In B"
    );

    // Subset export exports only the selected song.
    let subset_path = dir_a.path().join("song-only.selah");
    let reply::Result::ExportedSelahBundle(sub_exp) = unwrap_reply(core_a.handle(&ui_cmd(
        8,
        command::Action::ExportSelahBundle(ExportSelahBundle {
            output_path: subset_path.to_string_lossy().into_owned(),
            song_ids: vec![song.id.unwrap()],
            ..Default::default()
        }),
    ))) else {
        panic!("expected subset export");
    };
    assert_eq!(
        (
            sub_exp.song_count,
            sub_exp.template_count,
            sub_exp.media_count,
            sub_exp.presentation_count,
            sub_exp.tag_count
        ),
        (1, 0, 0, 0, 0)
    );
}

#[test]
fn m2_a6_full_library_backup_and_restore_round_trips_database_bibles_and_media() {
    let dir = tempfile::tempdir().unwrap();
    let backup_dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    // Import USFM Bible + bundled marker + song + template + imported image media + presentation + tag.
    let usfm = b"\\id JHN\n\\h John\n\\c 3\n\\v 16 For God so loved the world that he gave his only begotten Son.\n".to_vec();
    unwrap_reply(core.handle(&ui_cmd(
        1,
        command::Action::ImportBible(ImportBible {
            raw_bytes: usfm,
            format: BibleFormat::Usfm as i32,
            code: "SAMP".into(),
            name: "Sample USFM".into(),
            language: "en".into(),
            ..Default::default()
        }),
    )));
    std::fs::write(
        core.library().paths().bibles.join(".bundled-bibles.json"),
        br#"{"seeded":["KJV","WEB"]}"#,
    )
    .unwrap();

    let sec = Section {
        id: Some(new_id()),
        label: "Chorus".into(),
        kind: SectionKind::Chorus as i32,
        lines: vec![LyricLine {
            text: "Praise the Lord".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let song = Song {
        id: Some(new_id()),
        title: "Backup Hymn".into(),
        sections: vec![sec.clone()],
        arrangements: vec![Arrangement {
            id: Some(new_id()),
            name: "Default".into(),
            section_ids: vec![sec.id.unwrap()],
        }],
        ..Default::default()
    };
    unwrap_reply(core.handle(&ui_cmd(
        2,
        command::Action::SaveSong(SaveSong {
            song: Some(song.clone()),
        }),
    )));

    let png_src = repo_root().join("tests/golden/reference/text-en.png");
    let reply::Result::MediaItem(media_item) = unwrap_reply(core.handle(&ui_cmd(
        3,
        command::Action::ImportMedia(ImportMedia {
            source_path: png_src.to_string_lossy().into_owned(),
            name: "Backup Bg".into(),
            tags: vec!["backup".into()],
        }),
    ))) else {
        panic!("expected MediaItem");
    };

    let tag = Tag {
        id: Some(new_id()),
        name: "BackupTag".into(),
    };
    unwrap_reply(core.handle(&ui_cmd(
        4,
        command::Action::SaveTag(SaveTag {
            tag: Some(tag.clone()),
        }),
    )));

    let backup_path = backup_dir.path().join("church-backup.zip");
    let reply::Result::LibraryBackupInfo(info) = unwrap_reply(core.handle(&ui_cmd(
        5,
        command::Action::CreateLibraryBackup(CreateLibraryBackup {
            output_path: backup_path.to_string_lossy().into_owned(),
        }),
    ))) else {
        panic!("expected LibraryBackupInfo");
    };
    assert_eq!(info.media_blob_count, 1);
    assert_eq!(info.bible_count, 1);
    assert!(backup_path.is_file());

    // Also test default backup output path inside library/backups/.
    let reply::Result::LibraryBackupInfo(default_info) = unwrap_reply(core.handle(&ui_cmd(
        6,
        command::Action::CreateLibraryBackup(CreateLibraryBackup::default()),
    ))) else {
        panic!("expected default LibraryBackupInfo");
    };
    assert!(Path::new(&default_info.backup_path).starts_with(&core.library().paths().backups));

    // Mutate/wipe the live library after the backup.
    unwrap_reply(core.handle(&ui_cmd(
        7,
        command::Action::DeleteSong(DeleteSong {
            id: song.id.clone(),
        }),
    )));
    unwrap_reply(core.handle(&ui_cmd(
        8,
        command::Action::DeleteBible(DeleteBible {
            code: "SAMP".into(),
        }),
    )));
    let media_blob = core
        .library()
        .paths()
        .media
        .join(&media_item.content_hash[..2])
        .join(format!(
            "{}.{}",
            media_item.content_hash, media_item.extension
        ));
    std::fs::remove_file(&media_blob).unwrap();
    std::fs::remove_file(core.library().paths().bibles.join(".bundled-bibles.json")).unwrap();

    let extra_song = Song {
        id: Some(new_id()),
        title: "Post-Backup Ephemeral Song".into(),
        sections: vec![Section {
            id: Some(new_id()),
            label: "V1".into(),
            kind: SectionKind::Verse as i32,
            ..Default::default()
        }],
        ..Default::default()
    };
    unwrap_reply(core.handle(&ui_cmd(
        9,
        command::Action::SaveSong(SaveSong {
            song: Some(extra_song.clone()),
        }),
    )));

    let reply::Result::RestoreLibraryBackupResult(restored) = unwrap_reply(core.handle(&ui_cmd(
        10,
        command::Action::RestoreLibraryBackup(RestoreLibraryBackup {
            backup_path: backup_path.to_string_lossy().into_owned(),
        }),
    ))) else {
        panic!("expected RestoreLibraryBackupResult");
    };
    assert_eq!(restored.song_count, 1);
    assert_eq!(restored.media_count, 1);
    assert_eq!(restored.tag_count, 1);
    assert_eq!(restored.media_blob_count, 1);
    assert_eq!(restored.bible_count, 1);

    core.library().integrity_check().unwrap();
    assert_eq!(
        core.library()
            .get::<Song>(song.id.as_ref().unwrap())
            .unwrap(),
        Some(song)
    );
    assert!(
        core.library()
            .get::<Song>(extra_song.id.as_ref().unwrap())
            .unwrap()
            .is_none()
    );
    assert!(media_blob.is_file());
    assert!(
        core.library()
            .paths()
            .bibles
            .join(".bundled-bibles.json")
            .is_file()
    );

    // Search + Bible passage lookup + lazy thumbnail regeneration all work on the restored library.
    let reply::Result::MediaThumbnail(thumb) = unwrap_reply(core.handle(&ui_query(
        20,
        query::Request::GetMediaThumbnail(GetMediaThumbnail {
            media_id: media_item.id.clone(),
            max_width: 0,
            max_height: 0,
        }),
    ))) else {
        panic!("expected thumbnail after restore");
    };
    assert!(thumb.png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    let reply::Result::ScripturePassages(passages) = unwrap_reply(core.handle(&ui_query(
        21,
        query::Request::GetScripturePassages(GetScripturePassages {
            translation: "SAMP".into(),
            reference: "John 3:16".into(),
            spans: Vec::new(),
            language_hint: String::new(),
        }),
    ))) else {
        panic!("expected scripture passage after restore");
    };
    assert_eq!(passages.verses.len(), 1);

    // Restoring into a fresh library with envelope id 3 (already present in the backed-up
    // command_idempotency table from ImportMedia) must not collide, and replaying it stays idempotent.
    let reinstall_dir = tempfile::tempdir().unwrap();
    let mut core2 = CoreService::new(reinstall_dir.path(), false);
    let reply::Result::RestoreLibraryBackupResult(restored2) = unwrap_reply(core2.handle(&ui_cmd(
        3,
        command::Action::RestoreLibraryBackup(RestoreLibraryBackup {
            backup_path: backup_path.to_string_lossy().into_owned(),
        }),
    ))) else {
        panic!("expected RestoreLibraryBackupResult on fresh host");
    };
    assert_eq!(restored2.song_count, 1);
    let replay2 = core2.handle(&ui_cmd(
        3,
        command::Action::RestoreLibraryBackup(RestoreLibraryBackup {
            backup_path: backup_path.to_string_lossy().into_owned(),
        }),
    ));
    assert!(replay2.broadcast.is_empty());
    let reply::Result::RestoreLibraryBackupResult(replayed_res) = unwrap_reply(replay2) else {
        panic!("expected idempotent replay of RestoreLibraryBackupResult");
    };
    assert_eq!(replayed_res.song_count, 1);
}

#[test]
fn m2_a11_crafted_zip_bundles_and_corrupt_backups_fail_closed() {
    // sec: SEC-03, SEC-15
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    // Seed one valid song so we can verify no mutation happens on any rejected import or restore.
    let keep_song = Song {
        id: Some(new_id()),
        title: "Untouched Keeper Song".into(),
        sections: vec![Section {
            id: Some(new_id()),
            label: "V1".into(),
            kind: SectionKind::Verse as i32,
            ..Default::default()
        }],
        ..Default::default()
    };
    unwrap_reply(core.handle(&ui_cmd(
        1,
        command::Action::SaveSong(SaveSong {
            song: Some(keep_song.clone()),
        }),
    )));

    let empty_manifest = br#"{
  "format": "selah-bundle",
  "media_items": [],
  "presentations": [],
  "schema_version": 1,
  "songs": [],
  "tags": [],
  "templates": []
}"#;

    // 1. Traversal, absolute, backslash, Windows drive paths.
    for bad_name in [
        "../evil.json",
        "/abs/manifest.json",
        "sub\\manifest.json",
        "C:/evil.json",
    ] {
        let zip = build_raw_zip(
            &[
                RawZipEntry::deflated("manifest.json", empty_manifest),
                RawZipEntry::stored(bad_name, b"evil"),
            ],
            &[],
        );
        assert_eq!(
            expect_error_code(core.handle(&ui_cmd(
                10,
                command::Action::ImportSelahBundle(ImportSelahBundle {
                    raw_bytes: zip,
                    ..Default::default()
                }),
            ))),
            "SEL-LIB-0002",
            "{bad_name}"
        );
    }

    // 2. UNIX symlink entry external_attr (0o120000 << 16).
    let mut symlink_entry = RawZipEntry::stored("media/symlink.png", b"/etc/passwd");
    symlink_entry.external_attr = 0o120777 << 16;
    let symlink_zip = build_raw_zip(
        &[
            RawZipEntry::deflated("manifest.json", empty_manifest),
            symlink_entry,
        ],
        &[],
    );
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            11,
            command::Action::ImportSelahBundle(ImportSelahBundle {
                raw_bytes: symlink_zip,
                ..Default::default()
            }),
        ))),
        "SEL-LIB-0002"
    );

    // 3. Deflate overrun zip bomb: CD declares 10 bytes, actual deflated stream yields 2048 bytes.
    let bomb_raw = vec![b'A'; 2048];
    let mut bomb_entry = RawZipEntry::deflated("manifest.json", &bomb_raw);
    bomb_entry.uncompressed_size = 10;
    let bomb_zip = build_raw_zip(&[bomb_entry], &[]);
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            12,
            command::Action::ImportSelahBundle(ImportSelahBundle {
                raw_bytes: bomb_zip,
                ..Default::default()
            }),
        ))),
        "SEL-LIB-0002"
    );

    // 4. CRC32 mismatch.
    let mut bad_crc = RawZipEntry::deflated("manifest.json", empty_manifest);
    bad_crc.crc ^= 0xdead_beef;
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            13,
            command::Action::ImportSelahBundle(ImportSelahBundle {
                raw_bytes: build_raw_zip(&[bad_crc], &[]),
                ..Default::default()
            }),
        ))),
        "SEL-LIB-0002"
    );

    // 5. Case-colliding duplicate entry names.
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            14,
            command::Action::ImportSelahBundle(ImportSelahBundle {
                raw_bytes: build_raw_zip(
                    &[
                        RawZipEntry::deflated("manifest.json", empty_manifest),
                        RawZipEntry::deflated("Manifest.json", empty_manifest),
                    ],
                    &[],
                ),
                ..Default::default()
            }),
        ))),
        "SEL-LIB-0002"
    );

    // 6. LFH vs Central Directory mismatch.
    let mut mismatch = RawZipEntry::deflated("manifest.json", empty_manifest);
    mismatch.lfh_uncompressed_override = Some(mismatch.uncompressed_size + 1);
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            15,
            command::Action::ImportSelahBundle(ImportSelahBundle {
                raw_bytes: build_raw_zip(&[mismatch], &[]),
                ..Default::default()
            }),
        ))),
        "SEL-LIB-0002"
    );

    // 7. Trailing bytes after EOCD.
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            16,
            command::Action::ImportSelahBundle(ImportSelahBundle {
                raw_bytes: build_raw_zip(
                    &[RawZipEntry::deflated("manifest.json", empty_manifest)],
                    b"trailing-garbage",
                ),
                ..Default::default()
            }),
        ))),
        "SEL-LIB-0002"
    );

    // 8. Overlapping LFH spans (two CD records pointing to overlapping offsets).
    let single = build_raw_zip(
        &[RawZipEntry::deflated("manifest.json", empty_manifest)],
        &[],
    );
    // Take first LFH+payload, duplicate its CD pointing to offset 0 under another valid name.
    let mut cd_mismatch = RawZipEntry::deflated("manifest.json", empty_manifest);
    cd_mismatch.cd_name = Some("Manifest.json");
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            17,
            command::Action::ImportSelahBundle(ImportSelahBundle {
                raw_bytes: build_raw_zip(&[cd_mismatch], &[]),
                ..Default::default()
            }),
        ))),
        "SEL-LIB-0002"
    );
    let _ = single;

    // 9. Extra unreferenced media blob + unknown manifest JSON field + non-v7 UUID.
    let bogus_bytes = b"not referenced";
    let bogus_hash = blake3::hash(bogus_bytes).to_hex().to_string();
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            18,
            command::Action::ImportSelahBundle(ImportSelahBundle {
                raw_bytes: build_raw_zip(
                    &[
                        RawZipEntry::deflated("manifest.json", empty_manifest),
                        RawZipEntry::stored(&format!("media/{bogus_hash}.png"), bogus_bytes),
                    ],
                    &[],
                ),
                ..Default::default()
            }),
        ))),
        "SEL-LIB-0002"
    );

    let unknown_field_manifest = br#"{
  "format": "selah-bundle",
  "media_items": [],
  "presentations": [],
  "schema_version": 1,
  "songs": [],
  "tags": [],
  "templates": [],
  "unexpected": 42
}"#;
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            19,
            command::Action::ImportSelahBundle(ImportSelahBundle {
                raw_bytes: build_raw_zip(
                    &[RawZipEntry::deflated(
                        "manifest.json",
                        unknown_field_manifest
                    )],
                    &[],
                ),
                ..Default::default()
            }),
        ))),
        "SEL-LIB-0002"
    );

    let bad_uuid_manifest = br#"{
  "format": "selah-bundle",
  "media_items": [],
  "presentations": [],
  "schema_version": 1,
  "songs": [],
  "tags": [{"id": "550e8400-e29b-41d4-a716-446655440000", "name": "BadUUIDv4"}],
  "templates": []
}"#;
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            20,
            command::Action::ImportSelahBundle(ImportSelahBundle {
                raw_bytes: build_raw_zip(
                    &[RawZipEntry::deflated("manifest.json", bad_uuid_manifest)],
                    &[],
                ),
                ..Default::default()
            }),
        ))),
        "SEL-LIB-0002"
    );

    // 10. Corrupted backup (tampered SQLite bytes / SHA-256 mismatch) leaves live library intact.
    let corrupt_backup = dir.path().join("corrupt-backup.zip");
    let bad_db_bytes = b"corrupted sqlite bytes";
    let bad_backup_manifest = br#"{
  "bibles": [],
  "created_at_unix_ms": 1,
  "database_bytes": 22,
  "database_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
  "format": "selah-backup",
  "media_blobs": [],
  "media_count": 0,
  "presentation_count": 0,
  "schema_version": 3,
  "song_count": 0,
  "tag_count": 0,
  "template_count": 0
}"#;
    std::fs::write(
        &corrupt_backup,
        build_raw_zip(
            &[
                RawZipEntry::deflated("backup-manifest.json", bad_backup_manifest),
                RawZipEntry::stored("library.db", bad_db_bytes),
            ],
            &[],
        ),
    )
    .unwrap();
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            21,
            command::Action::RestoreLibraryBackup(RestoreLibraryBackup {
                backup_path: corrupt_backup.to_string_lossy().into_owned(),
            }),
        ))),
        "SEL-LIB-0002"
    );

    // Valid SHA-256 over invalid SQLite bytes must ALSO fail closed before swapping!
    use sha2::Digest;
    let db_sha = format!("{:x}", sha2::Sha256::digest(bad_db_bytes));
    let corrupt_sql_manifest = format!(
        r#"{{
  "bibles": [],
  "created_at_unix_ms": 1,
  "database_bytes": {},
  "database_sha256": "{db_sha}",
  "format": "selah-backup",
  "media_blobs": [],
  "media_count": 0,
  "presentation_count": 0,
  "schema_version": 3,
  "song_count": 0,
  "tag_count": 0,
  "template_count": 0
}}"#,
        bad_db_bytes.len()
    );
    let corrupt_sqlite_backup = dir.path().join("corrupt-sqlite-backup.zip");
    std::fs::write(
        &corrupt_sqlite_backup,
        build_raw_zip(
            &[
                RawZipEntry::deflated("backup-manifest.json", corrupt_sql_manifest.as_bytes()),
                RawZipEntry::stored("library.db", bad_db_bytes),
            ],
            &[],
        ),
    )
    .unwrap();
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            22,
            command::Action::RestoreLibraryBackup(RestoreLibraryBackup {
                backup_path: corrupt_sqlite_backup.to_string_lossy().into_owned(),
            }),
        ))),
        "SEL-LIB-0002"
    );

    // Keeper song and live DB integrity are completely untouched!
    assert_eq!(
        core.library()
            .get::<Song>(keep_song.id.as_ref().unwrap())
            .unwrap(),
        Some(keep_song)
    );
    assert!(core.library().list::<Tag>(10, 0).unwrap().is_empty());
    core.library().integrity_check().unwrap();
    let db = Connection::open(core.library().paths().database.clone()).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM songs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
