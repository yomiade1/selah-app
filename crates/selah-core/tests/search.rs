use std::collections::HashMap;

use selah_core::library::{
    BibleVerseRecord, CommandBusError, CommandMeta, Library, MAX_SEARCH_LIMIT,
    MAX_SEARCH_QUERY_BYTES, new_id, write_bible_database,
};
use selah_proto::v1::*;

type TranslationSpec<'a> = (&'a str, Vec<&'a str>);
type SectionSpec<'a> = (&'a str, SectionKind, Vec<&'a str>, Vec<TranslationSpec<'a>>);

fn make_song(
    title: &str,
    authors: &[&str],
    ccli_number: &str,
    tags: &[&str],
    sections: Vec<SectionSpec<'_>>,
) -> Song {
    let built_sections = sections
        .into_iter()
        .map(|(label, kind, lines, translations)| {
            let mut trans_map = HashMap::new();
            for (lang, t_lines) in translations {
                trans_map.insert(
                    lang.to_owned(),
                    TranslationLines {
                        lines: t_lines.into_iter().map(str::to_owned).collect(),
                    },
                );
            }
            Section {
                id: Some(new_id()),
                label: label.to_owned(),
                kind: kind as i32,
                lines: lines
                    .into_iter()
                    .map(|text| LyricLine {
                        text: text.to_owned(),
                        chords: Vec::new(),
                    })
                    .collect(),
                translations: trans_map,
                ..Default::default()
            }
        })
        .collect();

    Song {
        id: Some(new_id()),
        title: title.to_owned(),
        authors: authors.iter().map(|s| (*s).to_owned()).collect(),
        ccli_number: ccli_number.to_owned(),
        tags: tags.iter().map(|s| (*s).to_owned()).collect(),
        public_domain: true,
        sections: built_sections,
        ..Default::default()
    }
}

fn save_media_fixture(
    library: &mut Library,
    name: &str,
    kind: MediaKind,
    tags: &[&str],
    ext: &str,
) -> MediaItem {
    let store = library.media_store().unwrap();
    let payload = format!("synthetic-media-{name}-{ext}");
    let blob = store.import_reader(payload.as_bytes(), ext, 1024).unwrap();
    let item = MediaItem {
        id: Some(new_id()),
        name: name.to_owned(),
        kind: kind as i32,
        content_hash: blob.content_hash,
        extension: blob.extension,
        size_bytes: blob.size_bytes,
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        ..Default::default()
    };
    library.save(&item).unwrap();
    item
}

#[test]
fn song_search_covers_every_field_translations_and_short_words() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();

    let amazing = make_song(
        "Amazing Grace",
        &["John Newton"],
        "22025",
        &["Hymn", "Closing"],
        vec![(
            "Verse 1",
            SectionKind::Verse,
            vec![
                "Amazing grace how sweet the sound",
                "That saved a wretch like me",
            ],
            vec![("es", vec!["Sublime gracia del Señor"])],
        )],
    );
    let short_words = make_song(
        "I Know Who Holds Tomorrow",
        &["Ira Stanphill"],
        "54321",
        &["Assurance"],
        vec![(
            "Verse 1",
            SectionKind::Verse,
            vec![
                "How great Is my Redeemer",
                "I will trust Him for my tomorrow",
            ],
            vec![],
        )],
    );
    library.save(&amazing).unwrap();
    library.save(&short_words).unwrap();

    // Title
    let r = library.search_songs("Amazing Grace", 10, 0).unwrap();
    assert_eq!(r.hits[0].song.as_ref().unwrap().title, "Amazing Grace");
    assert!(r.hits[0].matched_fields.contains(&"title".to_owned()));

    // Author
    let r = library.search_songs("John Newton", 10, 0).unwrap();
    assert_eq!(r.hits[0].song.as_ref().unwrap().title, "Amazing Grace");
    assert!(r.hits[0].matched_fields.contains(&"authors".to_owned()));

    // CCLI number
    let r = library.search_songs("22025", 10, 0).unwrap();
    assert_eq!(r.hits[0].song.as_ref().unwrap().title, "Amazing Grace");
    assert!(r.hits[0].matched_fields.contains(&"ccli_number".to_owned()));

    // Tags
    let r = library.search_songs("Closing", 10, 0).unwrap();
    assert_eq!(r.hits[0].song.as_ref().unwrap().title, "Amazing Grace");
    assert!(r.hits[0].matched_fields.contains(&"tags".to_owned()));

    // Lyrics & translated lines
    let r = library.search_songs("sweet the sound", 10, 0).unwrap();
    assert_eq!(r.hits[0].song.as_ref().unwrap().title, "Amazing Grace");
    assert!(r.hits[0].matched_fields.contains(&"lyrics".to_owned()));
    assert!(r.hits[0].snippet.contains("sweet the sound"));

    let r = library.search_songs("Sublime gracia", 10, 0).unwrap();
    assert_eq!(r.hits[0].song.as_ref().unwrap().title, "Amazing Grace");

    // Short worship tokens are never stop-word filtered
    for short in ["I", "How", "Is", "My"] {
        let r = library.search_songs(short, 10, 0).unwrap();
        assert!(
            r.hits
                .iter()
                .any(|h| h.song.as_ref().unwrap().title == "I Know Who Holds Tomorrow"),
            "short query '{short}' should match"
        );
    }
}

#[test]
fn song_ranking_prefers_title_then_ccli_tags_authors_and_lyrics() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();

    // Put the same target token in a different single field of each song.
    let in_lyrics = make_song(
        "Evening Prayer",
        &["Anon"],
        "10001",
        &["Vesper"],
        vec![(
            "Verse 1",
            SectionKind::Verse,
            vec!["Hold onto beaconlight in the night"],
            vec![],
        )],
    );
    let in_authors = make_song(
        "Morning Song",
        &["Beaconlight Writers"],
        "10002",
        &["Opening"],
        vec![("Verse 1", SectionKind::Verse, vec!["Wake and sing"], vec![])],
    );
    let in_tags = make_song(
        "Midday Hymn",
        &["Other"],
        "10003",
        &["Beaconlight"],
        vec![("Verse 1", SectionKind::Verse, vec!["Praise God"], vec![])],
    );
    let in_ccli = make_song(
        "Special Number",
        &["Other"],
        "Beaconlight",
        &["Solo"],
        vec![("Verse 1", SectionKind::Verse, vec!["Sing aloud"], vec![])],
    );
    let in_title_long = make_song(
        "The Shining Beaconlight Over Every Nation",
        &["Other"],
        "10005",
        &["Hymn"],
        vec![("Verse 1", SectionKind::Verse, vec!["Let all see"], vec![])],
    );
    let in_title_exact = make_song(
        "Beaconlight",
        &["Other"],
        "10006",
        &["Hymn"],
        vec![("Verse 1", SectionKind::Verse, vec!["Let all see"], vec![])],
    );

    for s in [
        in_lyrics,
        in_authors,
        in_tags,
        in_ccli,
        in_title_long,
        in_title_exact,
    ] {
        library.save(&s).unwrap();
    }

    let res = library.search_songs("beaconlight", 10, 0).unwrap();
    let titles: Vec<&str> = res
        .hits
        .iter()
        .map(|h| h.song.as_ref().unwrap().title.as_str())
        .collect();
    assert_eq!(
        titles,
        vec![
            "Beaconlight",
            "The Shining Beaconlight Over Every Nation",
            "Special Number",
            "Midday Hymn",
            "Morning Song",
            "Evening Prayer",
        ]
    );
}

#[test]
fn song_search_handles_typos_short_transpositions_and_avoids_false_positives() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();

    let amazing = make_song(
        "Amazing Grace",
        &["John Newton"],
        "22025",
        &["Hymn"],
        vec![(
            "Verse 1",
            SectionKind::Verse,
            vec![
                "Amazing grace how sweet the sound",
                "That saved a wretch like me",
            ],
            vec![],
        )],
    );
    let false_positive_song = make_song(
        "Jacob's Struggle",
        &["Trad"],
        "22026",
        &["Hymn"],
        vec![(
            "Verse 1",
            SectionKind::Verse,
            vec!["He wrestled through the night until the break of day"],
            vec![],
        )],
    );
    let distractor_ccli = make_song(
        "Eternal River #2025",
        &["Anon"],
        "102025",
        &["Hymn"],
        vec![("Verse 1", SectionKind::Verse, vec!["Flow on"], vec![])],
    );

    library.save(&false_positive_song).unwrap();
    library.save(&distractor_ccli).unwrap();
    library.save(&amazing).unwrap();

    // Multi-word typo
    let res = library.search_songs("amzing grace", 10, 0).unwrap();
    assert_eq!(res.hits[0].song.as_ref().unwrap().title, "Amazing Grace");

    // Short transposition sharing zero trigrams ("grace" <-> "grcae")
    // Only triggers last-resort metadata fallback when trigram produces 0 verified hits.
    let res = library.search_songs("grcae", 10, 0).unwrap();
    assert_eq!(res.hits[0].song.as_ref().unwrap().title, "Amazing Grace");

    // Lyric typo finds "wretch" and rejects trigram neighbor "wrestled"
    let res = library.search_songs("wrecth", 10, 0).unwrap();
    let titles: Vec<&str> = res
        .hits
        .iter()
        .map(|h| h.song.as_ref().unwrap().title.as_str())
        .collect();
    assert_eq!(titles, vec!["Amazing Grace"]);

    // Exact CCLI must rank ahead of fuzzy title number "#2025"
    let res = library.search_songs("22025", 10, 0).unwrap();
    assert_eq!(res.hits[0].song.as_ref().unwrap().title, "Amazing Grace");
}

#[test]
fn song_search_pagination_and_synonyms_work() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();

    let xmas = make_song(
        "Christmas Joy",
        &["Carol"],
        "100",
        &["Christmas", "Winter"],
        vec![(
            "Verse 1",
            SectionKind::Verse,
            vec!["Christmas morning fills the air"],
            vec![],
        )],
    );
    let cross = make_song(
        "Beneath the Cross",
        &["Isaac Watts"],
        "101",
        &["Hymn"],
        vec![(
            "Verse 1",
            SectionKind::Verse,
            vec!["Beside the cross we stand"],
            vec![],
        )],
    );
    let hallelujah = make_song(
        "Sing Praise",
        &["Trad"],
        "102",
        &["Worship"],
        vec![(
            "Chorus",
            SectionKind::Chorus,
            vec!["Hallelujah to the Lamb"],
            vec![],
        )],
    );
    let peace = make_song(
        "Peace Like a River",
        &["Trad"],
        "103",
        &["Shalom"],
        vec![(
            "Verse 1",
            SectionKind::Verse,
            vec!["Christmas gift of peace"],
            vec![],
        )],
    );
    for s in [&xmas, &cross, &hallelujah, &peace] {
        library.save(s).unwrap();
    }

    assert!(
        library
            .search_songs("xmas", 10, 0)
            .unwrap()
            .hits
            .iter()
            .any(|h| h.song.as_ref().unwrap().title == "Christmas Joy")
    );
    assert_eq!(
        library.search_songs("calvary", 10, 0).unwrap().hits[0]
            .song
            .as_ref()
            .unwrap()
            .title,
        "Beneath the Cross"
    );
    assert_eq!(
        library.search_songs("alleluia", 10, 0).unwrap().hits[0]
            .song
            .as_ref()
            .unwrap()
            .title,
        "Sing Praise"
    );

    // Runtime custom bidirectional synonym
    library.add_synonym("eirene", "shalom").unwrap();
    assert_eq!(library.synonyms_for("EIRENE").unwrap(), vec!["shalom"]);
    assert_eq!(library.synonyms_for("Shalom").unwrap(), vec!["eirene"]);
    assert_eq!(
        library.search_songs("eirene", 10, 0).unwrap().hits[0]
            .song
            .as_ref()
            .unwrap()
            .title,
        "Peace Like a River"
    );

    // Invalid synonyms are refused
    assert!(library.add_synonym("same", "same").is_err());
    assert!(library.add_synonym("two words", "single").is_err());
    assert!(library.add_synonym("", "valid").is_err());
    assert!(library.add_synonym("bad\0term", "valid").is_err());
    assert!(library.add_synonym(&"x".repeat(200), "valid").is_err());

    // Pagination over shared query
    let p1 = library.search_songs("christmas", 1, 0).unwrap();
    let p2 = library.search_songs("christmas", 1, 1).unwrap();
    assert_eq!(p1.total, 2);
    assert_eq!(p2.total, 2);
    assert_eq!(p1.hits.len(), 1);
    assert_eq!(p2.hits.len(), 1);
    assert_ne!(
        p1.hits[0].song.as_ref().unwrap().id,
        p2.hits[0].song.as_ref().unwrap().id
    );
}

#[test]
fn multilingual_diacritics_fold_for_search_and_typos() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();

    // Yoruba (precomposed + NFD combining), Spanish, Portuguese, French
    let yoruba_pre = make_song(
        "Ọlọ́run Alààyè",
        &["Trad"],
        "201",
        &["Yoruba"],
        vec![("Verse 1", SectionKind::Verse, vec!["Ìwọ ni Ọba"], vec![])],
    );
    let yoruba_nfd = make_song(
        "O\u{0323}lo\u{0323}\u{0301}run E\u{0323}le\u{0323}\u{0301}da\u{0300}a\u{0301}",
        &["Trad"],
        "202",
        &["Yoruba"],
        vec![("Verse 1", SectionKind::Verse, vec!["Oba rere"], vec![])],
    );
    let spanish = make_song(
        "Señor de Mi Corazón",
        &["Español"],
        "203",
        &["Adoración"],
        vec![(
            "Verse 1",
            SectionKind::Verse,
            vec!["Te damos gracias"],
            vec![],
        )],
    );
    let portuguese = make_song(
        "Bênção e Louvor",
        &["Português"],
        "204",
        &["Hino"],
        vec![("Verse 1", SectionKind::Verse, vec!["Coração fiel"], vec![])],
    );
    let french = make_song(
        "Grâce et Cœur Ouvert",
        &["Français"],
        "205",
        &["Louange"],
        vec![(
            "Verse 1",
            SectionKind::Verse,
            vec!["L’œuvre de Ta main"],
            vec![],
        )],
    );

    for s in [yoruba_pre, yoruba_nfd, spanish, portuguese, french] {
        library.save(&s).unwrap();
    }

    assert_eq!(library.search_songs("olorun", 10, 0).unwrap().total, 2);
    assert_eq!(
        library.search_songs("senor corazon", 10, 0).unwrap().hits[0]
            .song
            .as_ref()
            .unwrap()
            .title,
        "Señor de Mi Corazón"
    );
    assert_eq!(
        library.search_songs("bencao", 10, 0).unwrap().hits[0]
            .song
            .as_ref()
            .unwrap()
            .title,
        "Bênção e Louvor"
    );
    assert_eq!(
        library.search_songs("oeuvre", 10, 0).unwrap().hits[0]
            .song
            .as_ref()
            .unwrap()
            .title,
        "Grâce et Cœur Ouvert"
    );
}

#[test]
fn search_indexes_track_updates_deletes_undo_redo_and_v1_migration() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    let actor = Actor {
        user_id: "owner".into(),
        device_id: "ui".into(),
        origin: ActorOrigin::Ui as i32,
    };

    let mut song = make_song(
        "First Lighthouse",
        &["Alpha"],
        "301",
        &["Classic"],
        vec![("Verse 1", SectionKind::Verse, vec!["Old beacon"], vec![])],
    );
    let song_id = song.id.clone().unwrap();
    let meta = |envelope_id: u64| CommandMeta {
        actor: &actor,
        envelope_id,
        command_hash: [envelope_id as u8; 32],
        undo_group: None,
    };

    // Save via command bus
    library.save_document_op(&song, &meta(1)).unwrap();
    assert_eq!(library.search_songs("beacon", 10, 0).unwrap().total, 1);

    // Update removes old indexed text and indexes new text
    song.title = "Second Harbor".into();
    song.sections[0].lines[0].text = "Anchor of the soul".into();
    library.save_document_op(&song, &meta(2)).unwrap();
    assert_eq!(library.search_songs("beacon", 10, 0).unwrap().total, 0);
    assert_eq!(library.search_songs("lighthouse", 10, 0).unwrap().total, 0);
    assert_eq!(library.search_songs("harbor", 10, 0).unwrap().total, 1);

    // Undo restores "First Lighthouse"
    library.undo_op(Some(&song_id), &meta(3)).unwrap();
    assert_eq!(library.search_songs("harbor", 10, 0).unwrap().total, 0);
    assert_eq!(library.search_songs("lighthouse", 10, 0).unwrap().total, 1);

    // Undo again deletes the song from index
    library.undo_op(Some(&song_id), &meta(4)).unwrap();
    assert_eq!(library.search_songs("lighthouse", 10, 0).unwrap().total, 0);

    // Redo twice brings "Second Harbor" back
    library.redo_op(Some(&song_id), &meta(5)).unwrap();
    library.redo_op(Some(&song_id), &meta(6)).unwrap();
    assert_eq!(library.search_songs("harbor", 10, 0).unwrap().total, 1);

    // Opening historical schema-v1.db fixture backfills search indexes
    let v1_root = tempfile::tempdir().unwrap();
    std::fs::write(
        v1_root.path().join("library.db"),
        include_bytes!("../../../tests/fixtures/db/schema-v1.db"),
    )
    .unwrap();
    let v1_lib = Library::open(v1_root.path()).unwrap();
    let migrated = v1_lib.search_songs("migration song", 10, 0).unwrap();
    assert_eq!(migrated.total, 1);
    assert_eq!(
        migrated.hits[0].song.as_ref().unwrap().title,
        "Synthetic migration song"
    );
    assert_eq!(v1_lib.search_songs("migrtion", 10, 0).unwrap().total, 1);
}

#[test]
fn media_search_filters_by_kind_matches_synonyms_typos_and_paginates() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();

    save_media_fixture(
        &mut library,
        "Sunset Mountain Background",
        MediaKind::Image,
        &["Nature", "Warm"],
        "png",
    );
    save_media_fixture(
        &mut library,
        "Forest Dawn Video",
        MediaKind::Video,
        &["Motion", "Nature"],
        "mp4",
    );
    save_media_fixture(
        &mut library,
        "Acoustic Prelude Track",
        MediaKind::Audio,
        &["Prelude"],
        "wav",
    );

    // Synonym ("backdrop" -> "background") + Image kind filter
    let res = library
        .search_media("backdrop", MediaKind::Image, 10, 0)
        .unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(
        res.hits[0].media_item.as_ref().unwrap().name,
        "Sunset Mountain Background"
    );

    // Kind filtering excludes other kinds
    let res = library
        .search_media("nature", MediaKind::Video, 10, 0)
        .unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(
        res.hits[0].media_item.as_ref().unwrap().name,
        "Forest Dawn Video"
    );

    // Typo tolerance & synonym ("clip" -> "video", typo "muntain")
    assert_eq!(
        library
            .search_media("clip", MediaKind::Unspecified, 10, 0)
            .unwrap()
            .hits[0]
            .media_item
            .as_ref()
            .unwrap()
            .name,
        "Forest Dawn Video"
    );
    assert_eq!(
        library
            .search_media("muntain", MediaKind::Unspecified, 10, 0)
            .unwrap()
            .hits[0]
            .media_item
            .as_ref()
            .unwrap()
            .name,
        "Sunset Mountain Background"
    );

    // Pagination on shared tag
    let p1 = library
        .search_media("nature", MediaKind::Unspecified, 1, 0)
        .unwrap();
    let p2 = library
        .search_media("nature", MediaKind::Unspecified, 1, 1)
        .unwrap();
    assert_eq!(p1.total, 2);
    assert_eq!(p1.hits.len(), 1);
    assert_eq!(p2.hits.len(), 1);
    assert_ne!(
        p1.hits[0].media_item.as_ref().unwrap().id,
        p2.hits[0].media_item.as_ref().unwrap().id
    );
}

#[test]
fn scripture_search_and_library_scope_fanout_respect_security_and_pagination() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();

    // Seed synthetic KJV and WEB test Bible databases
    write_bible_database(
        &library.paths().bibles.join("KJV.db"),
        "KJV",
        "King James Version",
        "en",
        &[
            BibleVerseRecord {
                book_num: 46,
                book_code: "1CO",
                book_name: "1 Corinthians",
                chapter: 13,
                verse: 4,
                text: "Charity suffereth long, and is kind; charity envieth not.",
            },
            BibleVerseRecord {
                book_num: 49,
                book_code: "EPH",
                book_name: "Ephesians",
                chapter: 2,
                verse: 8,
                text: "For by grace are ye saved through faith.",
            },
        ],
    )
    .unwrap();
    write_bible_database(
        &library.paths().bibles.join("WEB.db"),
        "WEB",
        "World English Bible",
        "en",
        &[
            BibleVerseRecord {
                book_num: 46,
                book_code: "1CO",
                book_name: "1 Corinthians",
                chapter: 13,
                verse: 4,
                text: "Love is patient and is kind. Love doesn't envy.",
            },
            BibleVerseRecord {
                book_num: 49,
                book_code: "EPH",
                book_name: "Ephesians",
                chapter: 2,
                verse: 8,
                text: "For by grace you have been saved through faith.",
            },
        ],
    )
    .unwrap();

    // Seed a song & media item too so SearchLibrary scopes can be contrasted
    let amazing = make_song(
        "Amazing Grace",
        &["John Newton"],
        "22025",
        &[],
        vec![(
            "Verse 1",
            SectionKind::Verse,
            vec!["Amazing grace how sweet the sound"],
            vec![],
        )],
    );
    library.save(&amazing).unwrap();
    save_media_fixture(
        &mut library,
        "Grace Abstract Overlay",
        MediaKind::Image,
        &[],
        "png",
    );

    // Synonyms across KJV/WEB ("love" matches KJV "charity" too)
    let love_all = library.search_scripture("love", "", 10, 0).unwrap();
    assert_eq!(love_all.total, 2);
    assert!(love_all.hits.iter().any(|h| h.translation == "KJV"));
    assert!(love_all.hits.iter().any(|h| h.translation == "WEB"));

    // Specific translation
    let web_only = library.search_scripture("grace", "web", 10, 0).unwrap();
    assert_eq!(web_only.total, 1);
    assert_eq!(web_only.hits[0].translation, "WEB");
    assert_eq!(web_only.hits[0].book, "EPH");

    // Typo tolerance inside scripture
    let typo_bible = library.search_scripture("suffreth", "KJV", 10, 0).unwrap();
    assert_eq!(typo_bible.total, 1);
    assert_eq!(typo_bible.hits[0].book, "1CO");

    // Security checks: unknown translation -> SEL-LIB-0001; traversal code -> SEL-LIB-0002
    assert!(matches!(
        library.search_scripture("grace", "MISSING", 10, 0),
        Err(CommandBusError::NotFound(_))
    ));
    for bad_code in ["../evil", "KJV/../../x", "kjv.db", "a b"] {
        assert!(
            matches!(
                library.search_scripture("grace", bad_code, 10, 0),
                Err(CommandBusError::Invalid(_))
            ),
            "code '{bad_code}' must be refused"
        );
    }

    // Combined SearchLibrary scopes
    let all = library
        .search_library("grace", SearchScope::All, "", 10)
        .unwrap();
    assert_eq!(all.songs.len(), 1);
    assert_eq!(all.media.len(), 1);
    assert_eq!(all.scripture.len(), 2);

    let songs_only = library
        .search_library("grace", SearchScope::Songs, "", 10)
        .unwrap();
    assert_eq!(songs_only.songs.len(), 1);
    assert!(songs_only.media.is_empty());
    assert!(songs_only.scripture.is_empty());

    let media_only = library
        .search_library("grace", SearchScope::Media, "", 10)
        .unwrap();
    assert!(media_only.songs.is_empty());
    assert_eq!(media_only.media.len(), 1);
    assert!(media_only.scripture.is_empty());

    let bible_only = library
        .search_library("grace", SearchScope::Scripture, "KJV", 10)
        .unwrap();
    assert!(bible_only.songs.is_empty());
    assert!(bible_only.media.is_empty());
    assert_eq!(bible_only.scripture.len(), 1);
    assert_eq!(bible_only.scripture[0].translation, "KJV");
}

#[cfg(unix)]
#[test]
fn symlinked_bible_outside_library_folder_is_refused() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let library = Library::open(root.path()).unwrap();
    let outside_db = outside.path().join("EVIL.db");
    write_bible_database(
        &outside_db,
        "EVIL",
        "Outside Bible",
        "en",
        &[BibleVerseRecord {
            book_num: 1,
            book_code: "GEN",
            book_name: "Genesis",
            chapter: 1,
            verse: 1,
            text: "In the beginning.",
        }],
    )
    .unwrap();
    symlink(&outside_db, library.paths().bibles.join("EVIL.db")).unwrap();
    assert!(library.search_scripture("beginning", "", 10, 0).is_err());
}

#[test]
fn fts5_special_characters_and_invalid_bounds_never_error_or_panic() {
    let root = tempfile::tempdir().unwrap();
    let mut library = Library::open(root.path()).unwrap();
    let song = make_song(
        "OR AND NOT Title:Grace",
        &["Writer"],
        "999",
        &[],
        vec![("Verse 1", SectionKind::Verse, vec!["Pure text"], vec![])],
    );
    library.save(&song).unwrap();

    // FTS5 operator syntax & unbalanced punctuation must safely search as literals or return empty
    for adversarial in [
        "",
        "   \t\n ",
        "OR",
        "AND",
        "NOT",
        "\"unclosed",
        "title:grace",
        "(a OR b)",
        "*^*:*",
        "--- ''' ...",
    ] {
        let r = library.search_songs(adversarial, 10, 0).unwrap();
        assert!(r.hits.len() <= 10);
    }

    // Invalid bounds & NUL / oversized queries fail cleanly with CommandBusError::Invalid
    assert!(matches!(
        library.search_songs("grace", 0, 0),
        Err(CommandBusError::Invalid(_))
    ));
    assert!(matches!(
        library.search_songs("grace", MAX_SEARCH_LIMIT + 1, 0),
        Err(CommandBusError::Invalid(_))
    ));
    assert!(matches!(
        library.search_songs("grace\0null", 10, 0),
        Err(CommandBusError::Invalid(_))
    ));
    assert!(matches!(
        library.search_songs(&"a".repeat(MAX_SEARCH_QUERY_BYTES + 1), 10, 0),
        Err(CommandBusError::Invalid(_))
    ));
}
