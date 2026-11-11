#![forbid(unsafe_code)]
//! Create a synthetic historical schema fixture. Never overwrite an existing fixture.
use std::collections::HashMap;
use std::path::PathBuf;

use selah_core::library::{CommandMeta, Library, PolicyLevel};
use selah_proto::v1::{
    Actor, ActorOrigin, Arrangement, Chord, Color, Id, LyricLine, MediaItem, MediaKind,
    Presentation, Resolution, Section, SectionKind, Service, ServiceItem, ServiceItemKind,
    ServiceType, Slide, Song, Tag, Team, Template, TemplateVariant, TranslationLines,
};
use selah_render::slide_elements::{
    ShapeKind, SlideBackgroundSpec, SlideEffectEntry, SlideElement, SlideElementKind,
    SlideElementsDocument, SlideTransitionSpec,
};
use selah_render::template::{
    BackgroundBoxStyle, OutlineStyle, RoleSlots, SafeArea, ShadowStyle, SlotTextStyle,
    TemplateStyle, TextAlignH, TextAlignV,
};
use serde_json::json;

fn id(group: u8, last: u8) -> Id {
    Id {
        value: vec![1, 2, 3, 4, 5, group, 0x70, 0, 0x80, 0, 0, 0, 0, 0, 0, last],
    }
}

fn fixture_actor() -> Actor {
    Actor {
        user_id: "fixture-owner".into(),
        device_id: "fixture-device".into(),
        origin: ActorOrigin::Ui as i32,
    }
}

fn seed_v1(library: &mut Library) -> anyhow::Result<()> {
    let song = Song {
        id: Some(id(0, 1)),
        title: "Synthetic migration song".into(),
        public_domain: true,
        sections: vec![Section {
            id: Some(id(0, 2)),
            label: "Verse 1".into(),
            kind: SectionKind::Verse as i32,
            lines: vec![LyricLine {
                text: "These words exist to test a saved library".into(),
                chords: vec![],
            }],
            ..Default::default()
        }],
        arrangements: vec![Arrangement {
            id: Some(id(0, 3)),
            name: "Main".into(),
            section_ids: vec![id(0, 2), id(0, 2)],
        }],
        ..Default::default()
    };
    library.save(&song)?;
    Ok(())
}

fn seed_m2(library: &mut Library) -> anyhow::Result<()> {
    let actor = fixture_actor();
    let mut seq = 1u64;
    let mut next_meta = |tag: u8| {
        let envelope_id = seq;
        seq += 1;
        let mut command_hash = [0u8; 32];
        command_hash[0] = tag;
        command_hash[31] = (envelope_id & 0xFF) as u8;
        CommandMeta {
            actor: &actor,
            envelope_id,
            command_hash,
            undo_group: None,
        }
    };

    let tag_hymn = Tag {
        id: Some(id(1, 1)),
        name: "Hymn".into(),
    };
    let tag_bg = Tag {
        id: Some(id(1, 2)),
        name: "Background".into(),
    };
    library.save_document_op(&tag_hymn, &next_meta(1))?;
    library.save_document_op(&tag_bg, &next_meta(2))?;

    let song1_id = id(2, 1);
    let song1_v1_id = id(2, 2);
    let song1_ch_id = id(2, 3);
    let song1_arr_id = id(2, 4);
    let mut es_lines = HashMap::new();
    es_lines.insert(
        "es".to_owned(),
        TranslationLines {
            lines: vec!["Luz de prueba para este himno".to_owned()],
        },
    );
    let mut song1 = Song {
        id: Some(song1_id.clone()),
        title: "Synthetic Grace Hymn".into(),
        authors: vec!["Synthetic Composer".into()],
        ccli_number: String::new(),
        copyright: String::new(),
        key: "G".into(),
        tempo: 88.0,
        tags: vec!["Hymn".into(), "Opening".into()],
        public_domain: true,
        sections: vec![
            Section {
                id: Some(song1_v1_id.clone()),
                label: "Verse 1".into(),
                kind: SectionKind::Verse as i32,
                color: Some(Color {
                    red: 0.25,
                    green: 0.55,
                    blue: 0.85,
                    alpha: 1.0,
                }),
                hotkey: "1".into(),
                lines: vec![LyricLine {
                    text: "Morning light is synthetic fixture text".into(),
                    chords: vec![
                        Chord {
                            position: 0,
                            text: "G".into(),
                        },
                        Chord {
                            position: 8,
                            text: "C/G".into(),
                        },
                    ],
                }],
                translations: es_lines,
            },
            Section {
                id: Some(song1_ch_id.clone()),
                label: "Chorus".into(),
                kind: SectionKind::Chorus as i32,
                color: Some(Color {
                    red: 0.85,
                    green: 0.45,
                    blue: 0.20,
                    alpha: 1.0,
                }),
                hotkey: "c".into(),
                lines: vec![LyricLine {
                    text: "Sing praise on every screen".into(),
                    chords: vec![Chord {
                        position: 0,
                        text: "D".into(),
                    }],
                }],
                ..Default::default()
            },
        ],
        arrangements: vec![Arrangement {
            id: Some(song1_arr_id),
            name: "Sunday Main".into(),
            section_ids: vec![song1_v1_id.clone(), song1_ch_id.clone(), song1_v1_id],
        }],
    };
    library.save_document_op(&song1, &next_meta(3))?;
    song1.tempo = 92.0;
    library.save_document_op(&song1, &next_meta(4))?;

    let song2_v1_id = id(3, 2);
    let song2 = Song {
        id: Some(id(3, 1)),
        title: "Ọlọ́run Praise Fixture".into(),
        authors: vec!["Àmínà Synthetic".into()],
        ccli_number: "9990001".into(),
        copyright: "Synthetic Public Domain Fixture".into(),
        key: "D".into(),
        tempo: 76.0,
        tags: vec!["Closing".into(), "Multilingual".into()],
        public_domain: true,
        sections: vec![Section {
            id: Some(song2_v1_id.clone()),
            label: "Verse 1".into(),
            kind: SectionKind::Verse as i32,
            hotkey: "1".into(),
            lines: vec![
                LyricLine {
                    text: "Ọlọ́run ṣeun fún oore".into(),
                    chords: vec![],
                },
                LyricLine {
                    text: "찬양 테스트 한 줄".into(),
                    chords: vec![],
                },
            ],
            ..Default::default()
        }],
        arrangements: vec![Arrangement {
            id: Some(id(3, 3)),
            name: "Default".into(),
            section_ids: vec![song2_v1_id],
        }],
    };
    library.save_document_op(&song2, &next_meta(5))?;

    // Save + update + undo on song3 so both undo and redo stacks are populated.
    let song3_v1_id = id(4, 2);
    let mut song3 = Song {
        id: Some(id(4, 1)),
        title: "Synthetic Morning Anthem".into(),
        authors: vec!["Fixture Team".into()],
        key: "C".into(),
        tempo: 112.0,
        tags: vec!["Hymn".into()],
        public_domain: true,
        sections: vec![Section {
            id: Some(song3_v1_id.clone()),
            label: "Chorus".into(),
            kind: SectionKind::Chorus as i32,
            lines: vec![LyricLine {
                text: "Joyful fixture song words".into(),
                chords: vec![],
            }],
            ..Default::default()
        }],
        arrangements: vec![Arrangement {
            id: Some(id(4, 3)),
            name: "Main".into(),
            section_ids: vec![song3_v1_id],
        }],
        ..Default::default()
    };
    library.save_document_op(&song3, &next_meta(6))?;
    song3.title = "Synthetic Morning Anthem (Edited)".into();
    library.save_document_op(&song3, &next_meta(7))?;
    library.undo_op(song3.id.as_ref(), &next_meta(8))?;

    let template_id = id(5, 1);
    let template = Template {
        id: Some(template_id.clone()),
        name: "Synthetic 16:9 & LED Template".into(),
        variants: vec![
            TemplateVariant {
                id: Some(id(5, 2)),
                resolution: Some(Resolution {
                    width: 1920,
                    height: 1080,
                }),
                max_lines: 4,
                style_json: r#"{"font":"Manrope","align":"center"}"#.into(),
            },
            TemplateVariant {
                id: Some(id(5, 3)),
                resolution: Some(Resolution {
                    width: 3840,
                    height: 1080,
                }),
                max_lines: 2,
                style_json: r#"{"font":"Manrope","align":"center"}"#.into(),
            },
        ],
    };
    library.save_document_op(&template, &next_meta(9))?;

    // Valid tiny synthetic PNG bytes matching BLAKE3 hash checks during fixture generation.
    let png_stub: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15c4\x00\x00\x00\rIDATx\x9cc```\xf8\x0f\x00\x01\x04\x01\x00\x18\xdd\x8d\xb1\x00\x00\x00\x00IEND\xaeB`\x82";
    let blob = library
        .media_store()?
        .import_reader(png_stub, "png", 4096)?;
    let media_id = id(6, 1);
    let media = MediaItem {
        id: Some(media_id.clone()),
        name: "Synthetic Still Backdrop".into(),
        kind: MediaKind::Image as i32,
        content_hash: blob.content_hash,
        extension: blob.extension,
        size_bytes: blob.size_bytes,
        resolution: Some(Resolution {
            width: 1,
            height: 1,
        }),
        duration_milliseconds: 0,
        tags: vec!["Background".into(), "Motionless".into()],
        ..Default::default()
    };
    library.save_document_op(&media, &next_meta(10))?;

    let pres_id = id(7, 1);
    let presentation = Presentation {
        id: Some(pres_id.clone()),
        name: "Synthetic Announcements".into(),
        template_id: Some(template_id),
        slides: vec![
            Slide {
                id: Some(id(7, 2)),
                text: "Welcome to this synthetic fixture service".into(),
                media_id: Some(media_id),
                elements_json: "[]".into(),
            },
            Slide {
                id: Some(id(7, 3)),
                text: "Fixture notices slide".into(),
                media_id: None,
                elements_json: "[]".into(),
            },
        ],
    };
    library.save_document_op(&presentation, &next_meta(11))?;

    let service_type_id = id(8, 1);
    let service_type = ServiceType {
        id: Some(service_type_id.clone()),
        name: "Synthetic Sunday Service Type".into(),
        ..Default::default()
    };
    library.save(&service_type)?;

    let team_id = id(8, 2);
    let team = Team {
        id: Some(team_id.clone()),
        name: "Synthetic Tech Team".into(),
        ..Default::default()
    };
    library.save(&team)?;

    let service = Service {
        id: Some(id(8, 3)),
        name: "Synthetic Sunday Morning".into(),
        starts_at: None,
        service_type_id: Some(service_type_id),
        team_ids: vec![team_id],
        items: vec![
            ServiceItem {
                id: Some(id(8, 4)),
                kind: ServiceItemKind::Song as i32,
                content_id: Some(song1_id.clone()),
                arrangement_id: None,
                title: "Synthetic Grace Hymn".into(),
                notes: "Open with verse 1".into(),
                ..Default::default()
            },
            ServiceItem {
                id: Some(id(8, 5)),
                kind: ServiceItemKind::Presentation as i32,
                content_id: Some(pres_id),
                arrangement_id: None,
                title: "Synthetic Announcements".into(),
                ..Default::default()
            },
            ServiceItem {
                id: Some(id(8, 6)),
                kind: ServiceItemKind::Scripture as i32,
                title: "John 3:16".into(),
                scripture_reference: "John 3:16".into(),
                bible_translation: "KJV".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    library.save(&service)?;

    library.set_setting(
        "appearance.theme",
        &json!("light"),
        PolicyLevel::User,
        false,
    )?;
    library.set_setting(
        "appearance.text-size",
        &json!(115),
        PolicyLevel::Machine,
        false,
    )?;
    library.set_setting(
        "updates.allow-check",
        &json!(false),
        PolicyLevel::Organisation,
        true,
    )?;

    library.save_editor_buffer_op(
        &song1_id,
        selah_proto::v1::DocumentType::Song as i32,
        "[G]Morning light unsaved draft line",
        &next_meta(12),
    )?;

    library.record_feedback_op("songs.add-a-song-by-pasting", true, &next_meta(13))?;
    library.record_feedback_op("getting-started.get-help-in-selah", false, &next_meta(14))?;
    library.add_synonym("benevolence", "compassion")?;

    Ok(())
}

fn seed_m3(library: &mut Library) -> anyhow::Result<()> {
    seed_m2(library)?;

    let actor = fixture_actor();
    let mut seq = 100u64;
    let mut next_meta = |tag: u8| {
        let envelope_id = seq;
        seq += 1;
        let mut command_hash = [0u8; 32];
        command_hash[0] = tag;
        command_hash[31] = (envelope_id & 0xFF) as u8;
        CommandMeta {
            actor: &actor,
            envelope_id,
            command_hash,
            undo_group: None,
        }
    };

    // Rich M3 Template with outline, shadow, background_box, safe_area, and 3 aspect variants.
    let style_16_9 = TemplateStyle {
        schema_version: 1,
        name: "Synthetic Sanctuary Style".into(),
        reference_height: 1080,
        safe_area: SafeArea::default(),
        roles: RoleSlots {
            body: SlotTextStyle {
                font_family: "Manrope".into(),
                size: 72.0,
                min_size: 32.0,
                weight: 700,
                color: [255, 255, 255, 255],
                outline: Some(OutlineStyle {
                    color: [0, 0, 0, 230],
                    width: 3.0,
                }),
                shadow: Some(ShadowStyle {
                    color: [0, 0, 0, 190],
                    offset_x: 0.0,
                    offset_y: 4.0,
                    blur: 8.0,
                }),
                background_box: Some(BackgroundBoxStyle {
                    color: [16, 36, 68, 210],
                    padding_x: 24.0,
                    padding_y: 14.0,
                    radius: 12.0,
                    full_width: false,
                    accent_bar_color: Some([255, 190, 46, 255]),
                    accent_bar_width: 8.0,
                }),
                line_height: 1.25,
                letter_spacing: 0.5,
                align_h: TextAlignH::Center,
                align_v: TextAlignV::Middle,
                max_lines: Some(4),
                visible: true,
            },
            ..RoleSlots::default()
        },
        variants: Vec::new(),
    };
    let mut style_led = style_16_9.clone();
    style_led.safe_area = SafeArea::led_wide();
    style_led.roles.body.max_lines = Some(2);

    let m3_template_id = id(9, 1);
    let m3_template = Template {
        id: Some(m3_template_id.clone()),
        name: "Synthetic Sanctuary Style".into(),
        variants: vec![
            TemplateVariant {
                id: Some(id(9, 2)),
                resolution: Some(Resolution {
                    width: 1920,
                    height: 1080,
                }),
                max_lines: 4,
                style_json: style_16_9.to_json()?,
            },
            TemplateVariant {
                id: Some(id(9, 3)),
                resolution: Some(Resolution {
                    width: 3840,
                    height: 1080,
                }),
                max_lines: 2,
                style_json: style_led.to_json()?,
            },
            TemplateVariant {
                id: Some(id(9, 4)),
                resolution: Some(Resolution {
                    width: 1440,
                    height: 1080,
                }),
                max_lines: 4,
                style_json: style_16_9.to_json()?,
            },
        ],
    };
    library.save_document_op(&m3_template, &next_meta(20))?;

    let m3_tmpl_hex: String = m3_template_id
        .value
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    library.set_setting(
        "templates.default.song",
        &json!(m3_tmpl_hex),
        PolicyLevel::User,
        false,
    )?;
    library.set_setting(
        "templates.default.scripture",
        &json!(m3_tmpl_hex),
        PolicyLevel::Organisation,
        false,
    )?;

    // Synthetic M3 video MediaItem flagged under SAFE-04 (`flash_warning = true`, `Flashing` tag).
    let mp4_stub: &[u8] =
        b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isomsynthetic-m3-flash-fixture";
    let video_blob = library
        .media_store()?
        .import_reader(mp4_stub, "mp4", 4096)?;
    let video_id = id(10, 1);
    let video_media = MediaItem {
        id: Some(video_id),
        name: "Synthetic Flashing Motion Loop".into(),
        kind: MediaKind::Video as i32,
        content_hash: video_blob.content_hash.clone(),
        extension: video_blob.extension.clone(),
        size_bytes: video_blob.size_bytes,
        resolution: Some(Resolution {
            width: 1920,
            height: 1080,
        }),
        duration_milliseconds: 4_000,
        tags: vec!["Motion".into(), "Flashing".into()],
        flash_warning: true,
    };
    library.save_document_op(&video_media, &next_meta(21))?;

    // Valid PNG hash matching seed_m2's still image for structured custom element reference.
    let png_stub: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15c4\x00\x00\x00\rIDATx\x9cc```\xf8\x0f\x00\x01\x04\x01\x00\x18\xdd\x8d\xb1\x00\x00\x00\x00IEND\xaeB`\x82";
    let png_hash = blake3::hash(png_stub).to_hex().to_string();

    // Custom SlideElementsDocument v1 slides (Text + {clock}/{timer}, Shape, Image, Video, Transition, Effect).
    let slide1_doc = SlideElementsDocument {
        schema_version: 1,
        reference_height: 1080,
        elements: vec![
            SlideElement {
                id: "el-clock-1".into(),
                name: "Welcome Clock".into(),
                kind: SlideElementKind::Text,
                group_id: Some("grp-header".into()),
                x: 0.1,
                y: 0.1,
                width: 0.8,
                height: 0.2,
                text: "Service starts in {timer} ({clock})".into(),
                font_family: "Manrope".into(),
                font_size: 48.0,
                weight: 700,
                timer_duration_secs: Some(300),
                ..SlideElement::default()
            },
            SlideElement {
                id: "el-shape-1".into(),
                name: "Accent Card".into(),
                kind: SlideElementKind::Shape,
                group_id: Some("grp-header".into()),
                x: 0.1,
                y: 0.35,
                width: 0.5,
                height: 0.25,
                shape_kind: ShapeKind::RoundedRect,
                corner_radius: 16.0,
                ..SlideElement::default()
            },
            SlideElement {
                id: "el-image-1".into(),
                name: "Still Backdrop Card".into(),
                kind: SlideElementKind::Image,
                x: 0.65,
                y: 0.35,
                width: 0.25,
                height: 0.25,
                content_hash: png_hash,
                media_extension: "png".into(),
                fit: "fit".into(),
                ..SlideElement::default()
            },
        ],
        background: Some(SlideBackgroundSpec {
            mode: "solid".into(),
            solid_color: Some([12, 24, 48, 255]),
            ..SlideBackgroundSpec::default()
        }),
        transition: Some(SlideTransitionSpec {
            kind: "fade".into(),
            direction: "left".into(),
            easing: "ease_in_out".into(),
            duration_ms: 600,
        }),
        effects: vec![SlideEffectEntry {
            kind: "vignette".into(),
            enabled: true,
            intensity: 0.45,
            params_json: r#"{"inner_radius":0.45,"softness":0.7}"#.into(),
        }],
    };
    let slide1_json = slide1_doc.to_json()?;
    SlideElementsDocument::from_json_strict(&slide1_json)?;

    let slide2_doc = SlideElementsDocument {
        schema_version: 1,
        reference_height: 1080,
        elements: vec![SlideElement {
            id: "el-video-1".into(),
            name: "Motion Element".into(),
            kind: SlideElementKind::Video,
            x: 0.1,
            y: 0.15,
            width: 0.8,
            height: 0.7,
            content_hash: video_blob.content_hash,
            media_extension: video_blob.extension,
            fit: "fill".into(),
            ..SlideElement::default()
        }],
        ..SlideElementsDocument::default()
    };
    let slide2_json = slide2_doc.to_json()?;
    SlideElementsDocument::from_json_strict(&slide2_json)?;

    let mut m3_pres = Presentation {
        id: Some(id(11, 1)),
        name: "Synthetic M3 Custom Deck".into(),
        template_id: Some(m3_template_id),
        slides: vec![
            Slide {
                id: Some(id(11, 2)),
                text: "Service starts soon".into(),
                media_id: Some(id(6, 1)),
                elements_json: slide1_json,
            },
            Slide {
                id: Some(id(11, 3)),
                text: String::new(),
                media_id: Some(id(10, 1)),
                elements_json: slide2_json,
            },
        ],
    };
    library.save_document_op(&m3_pres, &next_meta(22))?;
    m3_pres.name = "Synthetic M3 Custom Deck (Edited)".into();
    library.save_document_op(&m3_pres, &next_meta(23))?;
    library.undo_op(m3_pres.id.as_ref(), &next_meta(24))?;

    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FixtureMode {
    V1,
    M2,
    M3,
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, output_arg) = match args.as_slice() {
        [path] => (FixtureMode::V1, path.as_str()),
        [flag, path] if flag == "--m2" => (FixtureMode::M2, path.as_str()),
        [flag, path] if flag == "--m3" => (FixtureMode::M3, path.as_str()),
        _ => anyhow::bail!("usage: library_fixture [--m2 | --m3] <new-fixture.db>"),
    };
    let output = PathBuf::from(output_arg);
    anyhow::ensure!(
        !output.exists(),
        "historical fixtures cannot be overwritten"
    );
    let temporary = tempfile::tempdir()?;
    let mut library = Library::open(temporary.path())?;
    match mode {
        FixtureMode::V1 => seed_v1(&mut library)?,
        FixtureMode::M2 => seed_m2(&mut library)?,
        FixtureMode::M3 => seed_m3(&mut library)?,
    }
    library.integrity_check()?;
    let source = library.paths().database.clone();
    // Closing the last WAL connection checkpoints the committed pages into library.db.
    drop(library);
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut source = std::fs::File::open(source)?;
    let mut target = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    std::io::copy(&mut source, &mut target)?;
    target.sync_all()?;
    Ok(())
}
