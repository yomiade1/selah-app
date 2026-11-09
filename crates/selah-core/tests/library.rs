use std::io::Read;

use prost::Message;
use rusqlite::Connection;
use selah_core::library::{
    CURRENT_SCHEMA_VERSION, Library, MAX_DOCUMENT_BYTES, MediaStore, PolicyLevel, new_id,
    validate_id,
};
use selah_proto::v1::*;
use serde_json::json;

fn song() -> Song {
    let verse = Section {
        id: Some(new_id()),
        label: "Verse 1".into(),
        kind: SectionKind::Verse as i32,
        lines: vec![LyricLine {
            text: "A new song for this test".into(),
            chords: vec![Chord {
                position: 2,
                text: "C/G".into(),
            }],
        }],
        ..Default::default()
    };
    Song {
        id: Some(new_id()),
        title: "A test song".into(),
        authors: vec!["Test author".into()],
        tags: vec!["Test".into()],
        public_domain: true,
        arrangements: vec![Arrangement {
            id: Some(new_id()),
            name: "Main".into(),
            section_ids: vec![verse.id.clone().unwrap(); 2],
        }],
        sections: vec![verse],
        ..Default::default()
    }
}

#[test]
fn ids_are_unique_time_ordered_uuid_v7_and_invalid_ids_are_refused() {
    let ids: Vec<Id> = (0..1000).map(|_| new_id()).collect();
    for id in &ids {
        validate_id(id).unwrap();
    }
    for pair in ids.windows(2) {
        assert!(pair[0].value < pair[1].value);
    }
    for value in [vec![], vec![0; 16], vec![255; 16], vec![1; 17]] {
        assert!(validate_id(&Id { value }).is_err());
    }
}

#[test]
fn fresh_library_has_the_expected_layout_and_a_versioned_database() {
    let root = tempfile::tempdir().unwrap();
    let library = Library::open(root.path()).unwrap();
    for directory in ["media", "bibles", "packs", "backups"] {
        assert!(root.path().join(directory).is_dir());
    }
    assert_eq!(library.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    library.integrity_check().unwrap();
    let db = Connection::open(library.paths().database.clone()).unwrap();
    assert_eq!(
        db.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
}

#[test]
fn typed_documents_survive_reopen_update_pagination_and_delete() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    let mut original = song();
    let id = original.id.clone().unwrap();
    library.save(&original).unwrap();
    original.title = "Changed title".into();
    original.sections[0].lines[0].text = "Changed words".into();
    library.save(&original).unwrap();
    let other = song();
    library.save(&other).unwrap();
    drop(library);
    let mut library = Library::open(root.path()).unwrap();
    assert_eq!(library.get::<Song>(&id).unwrap(), Some(original));
    assert_eq!(library.list::<Song>(1, 1).unwrap(), vec![other]);
    assert!(library.list::<Song>(0, 0).is_err());
    assert!(library.list::<Song>(201, 0).is_err());
    assert!(library.delete::<Song>(&id).unwrap());
    assert!(!library.delete::<Song>(&id).unwrap());
    assert!(library.get::<Song>(&id).unwrap().is_none());
    library.integrity_check().unwrap();
    let db = Connection::open(library.paths().database.clone()).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM song_sections WHERE song_id=?1",
            [&id.value],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn every_m2_repository_round_trips_ordered_children() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    let template = Template {
        id: Some(new_id()),
        name: "Test template".into(),
        variants: vec![TemplateVariant {
            id: Some(new_id()),
            resolution: Some(Resolution {
                width: 1920,
                height: 1080,
            }),
            max_lines: 4,
            ..Default::default()
        }],
    };
    let presentation = Presentation {
        id: Some(new_id()),
        name: "Test presentation".into(),
        slides: vec![
            Slide {
                id: Some(new_id()),
                text: "First".into(),
                ..Default::default()
            },
            Slide {
                id: Some(new_id()),
                text: "Second".into(),
                ..Default::default()
            },
        ],
        template_id: template.id.clone(),
    };
    let service_type = ServiceType {
        id: Some(new_id()),
        name: "Test type".into(),
    };
    let team = Team {
        id: Some(new_id()),
        name: "Test team".into(),
    };
    let tag = Tag {
        id: Some(new_id()),
        name: "Test tag".into(),
    };
    let service = Service {
        id: Some(new_id()),
        name: "Test service".into(),
        service_type_id: service_type.id.clone(),
        team_ids: vec![team.id.clone().unwrap()],
        items: vec![
            ServiceItem {
                id: Some(new_id()),
                kind: ServiceItemKind::Presentation as i32,
                content_id: presentation.id.clone(),
                ..Default::default()
            },
            ServiceItem {
                id: Some(new_id()),
                kind: ServiceItemKind::Note as i32,
                notes: "Test note".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    macro_rules! round_trip {
        ($value:expr, $type:ty) => {
            library.save(&$value).unwrap();
            assert_eq!(
                library
                    .get::<$type>($value.id.as_ref().unwrap())
                    .unwrap()
                    .unwrap(),
                $value
            );
        };
    }
    round_trip!(template, Template);
    round_trip!(presentation, Presentation);
    round_trip!(service_type, ServiceType);
    round_trip!(team, Team);
    round_trip!(tag, Tag);
    round_trip!(service, Service);
    library.integrity_check().unwrap();
}

#[test]
fn child_constraint_failure_rolls_back_the_whole_document_update() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    let first = song();
    let second = song();
    library.save(&first).unwrap();
    library.save(&second).unwrap();
    let mut bad = second.clone();
    bad.title = "Must not persist".into();
    // Valid internally, but its section ID already belongs to another document.
    bad.sections = first.sections.clone();
    bad.arrangements = first.arrangements.clone();
    assert!(library.save(&bad).is_err());
    assert_eq!(
        library.get::<Song>(second.id.as_ref().unwrap()).unwrap(),
        Some(second)
    );
    assert_eq!(
        library.get::<Song>(first.id.as_ref().unwrap()).unwrap(),
        Some(first)
    );
    library.integrity_check().unwrap();
}

#[test]
fn validation_refuses_missing_ids_bad_chords_and_dangling_arrangements() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    let original = song();
    let mut cases = Vec::new();
    let mut bad = original.clone();
    bad.id = None;
    cases.push(bad);
    let mut bad = original.clone();
    bad.title = " ".into();
    cases.push(bad);
    let mut bad = original.clone();
    bad.tempo = f64::NAN;
    cases.push(bad);
    let mut bad = original.clone();
    bad.sections[0].kind = 99;
    cases.push(bad);
    let mut bad = original.clone();
    bad.sections[0].lines[0].chords[0].position = 999;
    cases.push(bad);
    let mut bad = original.clone();
    bad.arrangements[0].section_ids.push(new_id());
    cases.push(bad);
    let mut bad = original.clone();
    bad.sections.push(bad.sections[0].clone());
    cases.push(bad);
    for bad in cases {
        assert!(library.save(&bad).is_err());
    }
    assert!(library.list::<Song>(200, 0).unwrap().is_empty());
    // Character offsets work for multi-byte lyrics (not byte offsets).
    let mut unicode = original;
    unicode.sections[0].lines[0].text = "Ọlọ́run".into();
    unicode.sections[0].lines[0].chords[0].position = 7;
    library.save(&unicode).unwrap();
}

#[test]
fn oversized_documents_are_refused_before_storage() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    let mut bad = song();
    bad.copyright = "x".repeat(MAX_DOCUMENT_BYTES);
    assert!(library.save(&bad).is_err());
    assert!(library.list::<Song>(1, 0).unwrap().is_empty());
}

#[test]
fn migration_reopen_is_idempotent_and_preserves_records() {
    let root = tempfile::tempdir().unwrap();
    let original = song();
    {
        let mut library = Library::open(root.path()).unwrap();
        library.save(&original).unwrap();
    }
    for _ in 0..5 {
        let library = Library::open(root.path()).unwrap();
        assert_eq!(library.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
        assert_eq!(
            library
                .get::<Song>(original.id.as_ref().unwrap())
                .unwrap()
                .unwrap(),
            original
        );
    }
}

#[test]
fn historical_schema_v1_fixture_opens_without_losing_content() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("library.db"),
        include_bytes!("../../../tests/fixtures/db/schema-v1.db"),
    )
    .unwrap();
    let library = Library::open(root.path()).unwrap();
    let songs = library.list::<Song>(10, 0).unwrap();
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].title, "Synthetic migration song");
    assert_eq!(
        songs[0].sections[0].lines[0].text,
        "These words exist to test a saved library"
    );
    assert_eq!(songs[0].arrangements[0].section_ids.len(), 2);
    assert_eq!(
        songs[0].arrangements[0].section_ids[0],
        songs[0].sections[0].id.clone().unwrap()
    );
    library.integrity_check().unwrap();
}

#[test]
fn newer_tampered_unversioned_and_corrupt_databases_fail_closed() {
    // sec: SEC-15 (never silently reset the library)
    for sql in [
        "PRAGMA user_version=999",
        "UPDATE schema_migrations SET checksum='wrong'",
        "DELETE FROM schema_migrations",
        "PRAGMA user_version=0",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut library = Library::open(root.path()).unwrap();
        let original = song();
        library.save(&original).unwrap();
        let path = library.paths().database.clone();
        drop(library);
        let db = Connection::open(&path).unwrap();
        db.execute_batch(sql).unwrap();
        drop(db);
        let before = std::fs::read(&path).unwrap();
        assert!(Library::open(root.path()).is_err(), "{sql}");
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    std::fs::write(&path, b"not a SQLite database").unwrap();
    assert!(Library::open(root.path()).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"not a SQLite database");
}

#[test]
fn stored_document_corruption_is_reported_not_returned_as_content() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    let original = song();
    library.save(&original).unwrap();
    let db = Connection::open(&library.paths().database).unwrap();
    let replacement = song();
    db.execute("UPDATE songs SET data=?1", [replacement.encode_to_vec()])
        .unwrap();
    assert!(library.get::<Song>(original.id.as_ref().unwrap()).is_err());
}

#[test]
fn media_is_deduplicated_verified_and_metadata_survives_a_restart() {
    // sec: SEC-15 (content hashes are verified before metadata is accepted)
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    let store = library.media_store().unwrap();
    let bytes = b"synthetic storage test bytes; not decoded as an image";
    let blob = store.import_reader(bytes.as_slice(), "PNG", 1024).unwrap();
    assert_eq!(
        blob,
        store.import_reader(bytes.as_slice(), "png", 1024).unwrap()
    );
    let item = MediaItem {
        id: Some(new_id()),
        name: "Test media".into(),
        kind: MediaKind::Image as i32,
        content_hash: blob.content_hash.clone(),
        extension: blob.extension.clone(),
        size_bytes: blob.size_bytes,
        tags: vec!["Test".into(), "Test".into()],
        ..Default::default()
    };
    library.save(&item).unwrap();
    drop(library);
    let library = Library::open(root.path()).unwrap();
    assert_eq!(
        library
            .get::<MediaItem>(item.id.as_ref().unwrap())
            .unwrap()
            .unwrap(),
        item
    );
    let mut read = Vec::new();
    store
        .open_verified(&blob.content_hash, &blob.extension, blob.size_bytes)
        .unwrap()
        .read_to_end(&mut read)
        .unwrap();
    assert_eq!(read, bytes);
}

#[test]
fn media_path_traversal_oversize_and_stream_failure_leave_no_objects() {
    // sec: SEC-03, SEC-15
    let root = tempfile::tempdir().unwrap();
    let store = MediaStore::open(root.path()).unwrap();
    for extension in ["../png", "/png", "C:\\png", "a.b", "", "png:stream", "☃"] {
        assert!(
            store
                .import_reader(b"test".as_slice(), extension, 100)
                .is_err()
        );
    }
    assert!(
        store
            .import_reader(b"too large".as_slice(), "png", 1)
            .is_err()
    );
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("test read error"))
        }
    }
    assert!(store.import_reader(Broken, "png", 100).is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    assert!(store.verify(&"0".repeat(64), "../../outside", 0).is_err());
    assert!(store.verify(&"x".repeat(64), "png", 0).is_err());
}

#[test]
fn corrupt_existing_media_is_never_replaced_or_accepted() {
    // sec: SEC-15
    let root = tempfile::tempdir().unwrap();
    let store = MediaStore::open(root.path()).unwrap();
    let blob = store
        .import_reader(b"original".as_slice(), "png", 100)
        .unwrap();
    let path = root
        .path()
        .join(&blob.content_hash[..2])
        .join(format!("{}.png", blob.content_hash));
    std::fs::write(&path, b"tampered").unwrap();
    assert!(store.verify(&blob.content_hash, "png", 8).is_err());
    assert!(
        store
            .import_reader(b"original".as_slice(), "png", 100)
            .is_err()
    );
    assert_eq!(std::fs::read(path).unwrap(), b"tampered");
    assert!(store.verify(&blob.content_hash, "png", 999).is_err());
}

#[test]
fn missing_media_blob_prevents_metadata_write() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    let item = MediaItem {
        id: Some(new_id()),
        name: "Missing media".into(),
        kind: MediaKind::Image as i32,
        content_hash: "0".repeat(64),
        extension: "png".into(),
        size_bytes: 0,
        ..Default::default()
    };
    assert!(library.save(&item).is_err());
    assert!(library.list::<MediaItem>(1, 0).unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn symlinked_media_shards_and_library_directories_cannot_escape_the_root() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store = MediaStore::open(root.path()).unwrap();
    let hash = blake3::hash(b"test").to_hex().to_string();
    symlink(outside.path(), root.path().join(&hash[..2])).unwrap();
    assert!(store.import_reader(b"test".as_slice(), "png", 100).is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    let root = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.path().join("media")).unwrap();
    assert!(Library::open(root.path()).is_err());
    let root = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("must-not-be-created.db");
    symlink(&outside_file, root.path().join("library.db")).unwrap();
    assert!(Library::open(root.path()).is_err());
    assert!(!outside_file.exists());
}

#[test]
fn policy_defaults_user_values_and_locks_have_explicit_precedence() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    let key = "appearance.text-size";
    assert!(library.setting(key).unwrap().is_none());
    library
        .set_setting(key, &json!(150), PolicyLevel::Organisation, false)
        .unwrap();
    assert_eq!(library.setting(key).unwrap().unwrap().value, json!(150));
    library
        .set_setting(key, &json!(130), PolicyLevel::Machine, false)
        .unwrap();
    library
        .set_setting(key, &json!(100), PolicyLevel::User, false)
        .unwrap();
    assert_eq!(library.setting(key).unwrap().unwrap().value, json!(100));
    library
        .set_setting(key, &json!(175), PolicyLevel::Machine, true)
        .unwrap();
    assert!(
        library
            .set_setting(key, &json!(115), PolicyLevel::User, false)
            .is_err()
    );
    assert_eq!(library.setting(key).unwrap().unwrap().value, json!(175));
    library
        .set_setting(key, &json!(200), PolicyLevel::Organisation, true)
        .unwrap();
    let effective = library.setting(key).unwrap().unwrap();
    assert_eq!(effective.value, json!(200));
    assert_eq!(effective.level, PolicyLevel::Organisation);
    assert!(effective.locked);
    library
        .remove_setting(key, PolicyLevel::Organisation)
        .unwrap();
    assert_eq!(library.setting(key).unwrap().unwrap().value, json!(175));
    library.remove_setting(key, PolicyLevel::Machine).unwrap();
    assert_eq!(library.setting(key).unwrap().unwrap().value, json!(100));
    drop(library);
    assert_eq!(
        Library::open(root.path())
            .unwrap()
            .setting(key)
            .unwrap()
            .unwrap()
            .value,
        json!(100)
    );
}

#[test]
fn policy_storage_refuses_invalid_keys_user_locks_and_large_values() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    for key in ["", "a b", "../path", "x'; DROP TABLE songs;--"] {
        assert!(
            library
                .set_setting(key, &json!(1), PolicyLevel::User, false)
                .is_err()
        );
    }
    assert!(
        library
            .set_setting("test", &json!(1), PolicyLevel::User, true)
            .is_err()
    );
    assert!(
        library
            .set_setting("test", &json!("x".repeat(65536)), PolicyLevel::User, false)
            .is_err()
    );
    assert!(library.setting("test").unwrap().is_none());
}
