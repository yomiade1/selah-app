//! M2-A9 / Stage 10 migration checks: every historical fixture opens, migrates cleanly,
//! passes SQLite integrity checks, and preserves seeded documents, search indexes, and ops.
use std::path::{Path, PathBuf};

use selah_core::library::{CURRENT_SCHEMA_VERSION, CommandMeta, Library, PolicyLevel};
use selah_proto::v1::*;
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
