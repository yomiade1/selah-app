//! M2-A9 / M3-A7 / Stage 10 migration checks: every historical fixture opens, migrates
//! cleanly, passes SQLite integrity checks, and preserves seeded documents, search indexes, and ops.
use std::path::{Path, PathBuf};

use selah_core::library::{CURRENT_SCHEMA_VERSION, CommandMeta, Library, PolicyLevel};
use selah_proto::v1::*;
use selah_render::slide_elements::{ShapeKind, SlideElementKind, SlideElementsDocument};
use selah_render::template::TemplateStyle;
use serde_json::json;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/db")
        .canonicalize()
        .expect("tests/fixtures/db exists")
}

#[test]
fn every_historical_database_fixture_opens_migrates_and_passes_integrity() {
    let dir = fixtures_dir();
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "db"))
        .collect();
    fixtures.sort();

    let names: Vec<String> = fixtures
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"schema-v1.db".to_owned()), "{names:?}");
    assert!(names.contains(&"m2.db".to_owned()), "{names:?}");
    assert!(names.contains(&"m3.db".to_owned()), "{names:?}");

    for fixture in fixtures {
        let root = tempfile::tempdir().unwrap();
        std::fs::copy(&fixture, root.path().join("library.db")).unwrap();

        let library = Library::open(root.path())
            .unwrap_or_else(|e| panic!("opening {}: {e:#}", fixture.display()));
        assert_eq!(
            library.schema_version().unwrap(),
            CURRENT_SCHEMA_VERSION,
            "{}",
            fixture.display()
        );
        library.integrity_check().unwrap();

        let songs = library.list::<Song>(200, 0).unwrap();
        assert!(!songs.is_empty(), "{} has no songs", fixture.display());
        for song in &songs {
            let query = song.title.split_whitespace().next().unwrap_or(&song.title);
            let search = library.search_songs(query, 20, 0).unwrap();
            assert!(
                search
                    .hits
                    .iter()
                    .any(|h| h.song.as_ref().map(|s| &s.id) == Some(&song.id)),
                "{}: song '{}' missing from search index",
                fixture.display(),
                song.title
            );
        }
        drop(library);

        // Re-opening must be idempotent.
        let reopened = Library::open(root.path()).unwrap();
        assert_eq!(reopened.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
        reopened.integrity_check().unwrap();
        assert_eq!(reopened.list::<Song>(200, 0).unwrap(), songs);
    }
}

#[test]
fn m2_fixture_preserves_all_seeded_documents_search_buffers_feedback_and_undo() {
    let m2_path = fixtures_dir().join("m2.db");
    let root = tempfile::tempdir().unwrap();
    std::fs::copy(&m2_path, root.path().join("library.db")).unwrap();

    let mut library = Library::open(root.path()).unwrap();
    assert_eq!(library.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    library.integrity_check().unwrap();

    let songs = library.list::<Song>(50, 0).unwrap();
    assert_eq!(songs.len(), 3);
    let grace = songs
        .iter()
        .find(|s| s.title == "Synthetic Grace Hymn")
        .expect("grace hymn present");
    assert_eq!(grace.tempo, 92.0);
    assert_eq!(grace.sections.len(), 2);
    assert_eq!(grace.sections[0].lines[0].chords.len(), 2);
    assert_eq!(
        grace.sections[0].translations["es"].lines,
        vec!["Luz de prueba para este himno".to_owned()]
    );
    assert_eq!(grace.arrangements[0].section_ids.len(), 3);

    let yoruba_korean = songs
        .iter()
        .find(|s| s.title == "Ọlọ́run Praise Fixture")
        .expect("multilingual song present");
    assert_eq!(yoruba_korean.ccli_number, "9990001");
    assert_eq!(
        yoruba_korean.sections[0].lines[0].text,
        "Ọlọ́run ṣeun fún oore"
    );
    assert_eq!(yoruba_korean.sections[0].lines[1].text, "찬양 테스트 한 줄");

    let templates = library.list::<Template>(50, 0).unwrap();
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].variants.len(), 2);

    let media = library.list::<MediaItem>(50, 0).unwrap();
    assert_eq!(media.len(), 1);
    assert_eq!(media[0].name, "Synthetic Still Backdrop");
    assert_eq!(media[0].extension, "png");

    let presentations = library.list::<Presentation>(50, 0).unwrap();
    assert_eq!(presentations.len(), 1);
    assert_eq!(presentations[0].slides.len(), 2);

    let service_types = library.list::<ServiceType>(50, 0).unwrap();
    let teams = library.list::<Team>(50, 0).unwrap();
    let services = library.list::<Service>(50, 0).unwrap();
    let tags = library.list::<Tag>(50, 0).unwrap();
    assert_eq!(service_types.len(), 1);
    assert_eq!(teams.len(), 1);
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].items.len(), 3);
    assert_eq!(tags.len(), 2);

    // Search indexes and custom synonym survive cleanly.
    assert!(
        library
            .synonyms_for("benevolence")
            .unwrap()
            .contains(&"compassion".to_owned())
    );
    let diacritic = library.search_songs("olorun", 10, 0).unwrap();
    assert_eq!(
        diacritic.hits[0].song.as_ref().unwrap().title,
        "Ọlọ́run Praise Fixture"
    );
    let media_hits = library
        .search_media("backdrop", MediaKind::Image, 10, 0)
        .unwrap();
    assert_eq!(media_hits.hits.len(), 1);

    // Policy settings across all 3 levels survive.
    let theme = library.setting("appearance.theme").unwrap().unwrap();
    assert_eq!(theme.value, json!("light"));
    assert_eq!(theme.level, PolicyLevel::User);
    let size = library.setting("appearance.text-size").unwrap().unwrap();
    assert_eq!(size.value, json!(115));
    assert_eq!(size.level, PolicyLevel::Machine);
    let locked = library.setting("updates.allow-check").unwrap().unwrap();
    assert_eq!(locked.value, json!(false));
    assert_eq!(locked.level, PolicyLevel::Organisation);
    assert!(locked.locked);

    // Unsaved editor buffer and help feedback survive.
    let buffers = library.list_editor_buffers(20, 0).unwrap();
    assert_eq!(buffers.buffers.len(), 1);
    assert_eq!(buffers.buffers[0].doc_type, DocumentType::Song as i32);
    assert_eq!(
        buffers.buffers[0].content,
        "[G]Morning light unsaved draft line"
    );
    assert_eq!(
        library
            .help_feedback_count("songs.add-a-song-by-pasting", Some(true))
            .unwrap(),
        1
    );
    assert_eq!(
        library
            .help_feedback_count("getting-started.get-help-in-selah", Some(false))
            .unwrap(),
        1
    );

    // Op log + undo/redo stack remain live and functional.
    let ops = library.list_ops(0, 100, None).unwrap();
    assert!(ops.ops.len() >= 10);
    let state = library.undo_state(None).unwrap();
    assert!(state.can_undo && state.can_redo);

    let actor = Actor {
        user_id: "fixture-owner".into(),
        device_id: "fixture-device".into(),
        origin: ActorOrigin::Ui as i32,
    };
    let meta = CommandMeta {
        actor: &actor,
        envelope_id: 9001,
        command_hash: [7u8; 32],
        undo_group: None,
    };
    library.redo_op(None, &meta).unwrap();
    let updated_songs = library.list::<Song>(50, 0).unwrap();
    assert!(
        updated_songs
            .iter()
            .any(|s| s.title == "Synthetic Morning Anthem (Edited)")
    );
    library.integrity_check().unwrap();
}

#[test]
fn m3_fixture_preserves_templates_policies_custom_elements_and_flash_tags() {
    let m3_path = fixtures_dir().join("m3.db");
    let root = tempfile::tempdir().unwrap();
    std::fs::copy(&m3_path, root.path().join("library.db")).unwrap();

    let mut library = Library::open(root.path()).unwrap();
    assert_eq!(library.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    library.integrity_check().unwrap();

    // Rich M3 Template preserves 3 aspect variants, outline, shadow, background_box, safe_area.
    let templates = library.list::<Template>(50, 0).unwrap();
    assert_eq!(templates.len(), 2);
    let m3_tmpl = templates
        .iter()
        .find(|t| t.name == "Synthetic Sanctuary Style")
        .expect("M3 sanctuary style template present");
    assert_eq!(m3_tmpl.variants.len(), 3);
    let parsed_style = TemplateStyle::from_proto_template(m3_tmpl).unwrap();
    assert_eq!(parsed_style.name, "Synthetic Sanctuary Style");
    assert!(parsed_style.roles.body.outline.is_some());
    assert!(parsed_style.roles.body.shadow.is_some());
    let bg_box = parsed_style
        .roles
        .body
        .background_box
        .as_ref()
        .expect("background_box preserved");
    assert!(bg_box.accent_bar_color.is_some());
    assert_eq!(parsed_style.variants.len(), 3);
    let led_variant = parsed_style
        .variants
        .iter()
        .find(|v| v.reference_width == 3840 && v.reference_height == 1080)
        .expect("32:9 LED variant present");
    assert_eq!(led_variant.max_lines, Some(2));
    assert!((led_variant.safe_area.unwrap().left - 0.04).abs() < 1e-4);

    // Template policy settings store valid 32-char lowercase hex matching m3_tmpl.id.
    let expected_hex: String = m3_tmpl
        .id
        .as_ref()
        .unwrap()
        .value
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(expected_hex.len(), 32);

    let song_policy = library.setting("templates.default.song").unwrap().unwrap();
    assert_eq!(song_policy.value, json!(expected_hex));
    assert_eq!(song_policy.level, PolicyLevel::User);

    let scripture_policy = library
        .setting("templates.default.scripture")
        .unwrap()
        .unwrap();
    assert_eq!(scripture_policy.value, json!(expected_hex));
    assert_eq!(scripture_policy.level, PolicyLevel::Organisation);

    // Flash-flagged M3 video media survives migration and search indexing.
    let media = library.list::<MediaItem>(50, 0).unwrap();
    assert_eq!(media.len(), 2);
    let flash_video = media
        .iter()
        .find(|m| m.name == "Synthetic Flashing Motion Loop")
        .expect("M3 flashing motion video present");
    assert_eq!(flash_video.kind, MediaKind::Video as i32);
    assert!(flash_video.flash_warning);
    assert!(flash_video.tags.contains(&"Motion".to_owned()));
    assert!(flash_video.tags.contains(&"Flashing".to_owned()));

    let flash_hits = library
        .search_media("Flashing", MediaKind::Video, 10, 0)
        .unwrap();
    assert_eq!(flash_hits.hits.len(), 1);
    assert_eq!(
        flash_hits.hits[0].media_item.as_ref().unwrap().name,
        "Synthetic Flashing Motion Loop"
    );

    // Custom SlideElementsDocument v1 presentation preserves dynamic tokens, shapes, transitions, effects.
    let presentations = library.list::<Presentation>(50, 0).unwrap();
    assert_eq!(presentations.len(), 2);
    let m3_pres = presentations
        .iter()
        .find(|p| p.name == "Synthetic M3 Custom Deck")
        .expect("M3 custom presentation present");
    assert_eq!(m3_pres.slides.len(), 2);
    assert_eq!(m3_pres.template_id, m3_tmpl.id);

    let slide1_doc =
        SlideElementsDocument::from_json_strict(&m3_pres.slides[0].elements_json).unwrap();
    assert_eq!(slide1_doc.elements.len(), 3);
    assert_eq!(slide1_doc.elements[0].kind, SlideElementKind::Text);
    assert!(slide1_doc.elements[0].text.contains("{clock}"));
    assert!(slide1_doc.elements[0].text.contains("{timer}"));
    assert_eq!(slide1_doc.elements[0].timer_duration_secs, Some(300));
    assert_eq!(slide1_doc.elements[1].kind, SlideElementKind::Shape);
    assert_eq!(slide1_doc.elements[1].shape_kind, ShapeKind::RoundedRect);
    assert_eq!(slide1_doc.elements[2].kind, SlideElementKind::Image);
    assert_eq!(
        slide1_doc
            .transition
            .as_ref()
            .map(|t| (t.kind.as_str(), t.duration_ms)),
        Some(("fade", 600))
    );
    assert_eq!(slide1_doc.effects.len(), 1);
    assert_eq!(slide1_doc.effects[0].kind, "vignette");

    let slide2_doc =
        SlideElementsDocument::from_json_strict(&m3_pres.slides[1].elements_json).unwrap();
    assert_eq!(slide2_doc.elements.len(), 1);
    assert_eq!(slide2_doc.elements[0].kind, SlideElementKind::Video);

    // Undo / redo history on the M3 presentation remains live.
    let state = library.undo_state(None).unwrap();
    assert!(state.can_undo && state.can_redo);
    let actor = Actor {
        user_id: "fixture-owner".into(),
        device_id: "fixture-device".into(),
        origin: ActorOrigin::Ui as i32,
    };
    let meta = CommandMeta {
        actor: &actor,
        envelope_id: 9002,
        command_hash: [8u8; 32],
        undo_group: None,
    };
    library.redo_op(None, &meta).unwrap();
    let updated_pres = library.list::<Presentation>(50, 0).unwrap();
    assert!(
        updated_pres
            .iter()
            .any(|p| p.name == "Synthetic M3 Custom Deck (Edited)")
    );
    library.integrity_check().unwrap();
}
