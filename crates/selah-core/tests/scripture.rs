//! Scripture integration tests (M2.5 / M2-A5 / M2-A11):
//! - >= 300 reference cases across all 6 languages (`en`, `es`, `pt`, `fr`, `ko`, `yo`)
//! - Bundled public-domain KJV & WEB Bibles (H-06)
//! - OSIS, USFM, and Zefania bounded importers + SEC-03 adversarial cases
//! - Rule-based verse-to-slide splitting
use std::collections::HashSet;

use selah_core::library::{CommandBusError, Library};
use selah_core::scripture::{
    BOOKS, BibleImportRequest, MAX_XML_DEPTH, delete_bible, ensure_bundled_bibles, get_passages,
    import_bible, list_bibles, parse_reference, resolve_book_alias, split_scripture_slides,
};
use selah_proto::v1::{BibleFormat, ReferencePlacement, SplitScriptureSlides};

#[test]
fn canonical_book_table_covers_66_books_and_six_languages_cleanly() {
    assert_eq!(BOOKS.len(), 66);
    let mut codes = HashSet::new();
    let mut osis = HashSet::new();
    let mut names = HashSet::new();
    let mut single_chapters = Vec::new();

    for (idx, book) in BOOKS.iter().enumerate() {
        assert_eq!(book.book_num, (idx + 1) as u32);
        assert!(codes.insert(book.code), "duplicate code {}", book.code);
        assert!(osis.insert(book.osis), "duplicate osis {}", book.osis);
        assert!(names.insert(book.name), "duplicate name {}", book.name);
        if book.is_single_chapter {
            single_chapters.push(book.code);
        }
        assert!(!book.en.is_empty(), "empty en aliases for {}", book.code);
        assert!(!book.es.is_empty(), "empty es aliases for {}", book.code);
        assert!(!book.pt.is_empty(), "empty pt aliases for {}", book.code);
        assert!(!book.fr.is_empty(), "empty fr aliases for {}", book.code);
        assert!(!book.ko.is_empty(), "empty ko aliases for {}", book.code);
        assert!(!book.yo.is_empty(), "empty yo aliases for {}", book.code);
    }

    // Verify no accidental same-language alias collisions across all 66 books.
    for book in &BOOKS {
        for (lang, aliases) in [
            ("en", book.en),
            ("es", book.es),
            ("pt", book.pt),
            ("fr", book.fr),
            ("ko", book.ko),
            ("yo", book.yo),
        ] {
            for &alias in aliases {
                let resolved = resolve_book_alias(alias, Some(lang))
                    .unwrap_or_else(|| panic!("alias '{alias}' ({lang}) did not resolve"));
                assert_eq!(
                    resolved.code, book.code,
                    "collision for {lang} alias '{alias}': expected {}, got {}",
                    book.code, resolved.code
                );
            }
        }
    }

    assert_eq!(single_chapters, ["OBA", "PHM", "2JN", "3JN", "JUD"]);
}

#[test]
fn m2_a5_at_least_300_scripture_references_parse_across_six_languages() {
    // Part 1: All 66 canonical books × primary + abbreviated forms across all 6 languages
    // (66 × 6 = 396 book-language test cases >= 300 required by M2-A5)
    let mut executed_cases = 0usize;

    for book in &BOOKS {
        let langs: [(&str, &[&str]); 6] = [
            ("en", book.en),
            ("es", book.es),
            ("pt", book.pt),
            ("fr", book.fr),
            ("ko", book.ko),
            ("yo", book.yo),
        ];

        for (lang, aliases) in langs {
            let alias = aliases[0];
            let ref_str = if book.is_single_chapter {
                format!("{alias} 1:2")
            } else {
                format!("{alias} 1:2-4")
            };
            let parsed = parse_reference(&ref_str, Some(lang))
                .unwrap_or_else(|e| panic!("failed {lang} case '{ref_str}' ({}): {e}", book.code));
            assert_eq!(parsed.spans.len(), 1);
            assert_eq!(parsed.spans[0].book_num, book.book_num, "{ref_str}");
            assert_eq!(parsed.spans[0].book_code, book.code, "{ref_str}");
            assert_eq!(parsed.spans[0].start_chapter, 1, "{ref_str}");
            assert_eq!(parsed.spans[0].start_verse, 2, "{ref_str}");
            assert_eq!(
                parsed.spans[0].end_verse,
                if book.is_single_chapter { 2 } else { 4 },
                "{ref_str}"
            );
            executed_cases += 1;

            // Also verify the shortest/last abbreviation in each language when distinct
            if let Some(&short_alias) = aliases.last()
                && short_alias != alias
            {
                let short_ref = format!("{short_alias} 1:1");
                let p2 = parse_reference(&short_ref, Some(lang)).unwrap_or_else(|e| {
                    panic!(
                        "failed short {lang} case '{short_ref}' ({}): {e}",
                        book.code
                    )
                });
                assert_eq!(p2.spans[0].book_code, book.code, "{short_ref}");
                executed_cases += 1;
            }
        }
    }

    // Part 2: Diverse syntax, punctuation, diacritic, and edge-case variations
    type ExpectedSpan = (&'static str, u32, u32, u32, u32);
    type SyntaxCase = (&'static str, Option<&'static str>, &'static [ExpectedSpan]);
    let syntax_cases: &[SyntaxCase] = &[
        // English standard, spaces, dots, Roman numerals, ordinals, lists, semicolons
        ("John 3:16", Some("en"), &[("JHN", 3, 16, 3, 16)]),
        ("jn 3 16-18", Some("en"), &[("JHN", 3, 16, 3, 18)]),
        ("Jn3:16", Some("en"), &[("JHN", 3, 16, 3, 16)]),
        ("Gen. 1.1", Some("en"), &[("GEN", 1, 1, 1, 1)]),
        ("Genesis 1:1.", Some("en"), &[("GEN", 1, 1, 1, 1)]),
        (
            "Rom 8:28, 31-32, 38-39",
            Some("en"),
            &[
                ("ROM", 8, 28, 8, 28),
                ("ROM", 8, 31, 8, 32),
                ("ROM", 8, 38, 8, 39),
            ],
        ),
        ("Ps 23", Some("en"), &[("PSA", 23, 0, 23, 0)]),
        ("Psalm 23:1-6", Some("en"), &[("PSA", 23, 1, 23, 6)]),
        ("Gen 1-2", Some("en"), &[("GEN", 1, 0, 2, 0)]),
        ("Gen 1:26-2:3", Some("en"), &[("GEN", 1, 26, 2, 3)]),
        ("I Cor 13:4-7", Some("en"), &[("1CO", 13, 4, 13, 7)]),
        ("1st John 4:8", Some("en"), &[("1JN", 4, 8, 4, 8)]),
        ("Second Timothy 1:7", None, &[("2TI", 1, 7, 1, 7)]),
        ("II Kings 2:11", Some("en"), &[("2KI", 2, 11, 2, 11)]),
        ("III John 2", Some("en"), &[("3JN", 1, 2, 1, 2)]),
        ("Jude 24-25", Some("en"), &[("JUD", 1, 24, 1, 25)]),
        ("Phlm 6", Some("en"), &[("PHM", 1, 6, 1, 6)]),
        ("Oba 1-4", Some("en"), &[("OBA", 1, 1, 1, 4)]),
        ("Jude 1", Some("en"), &[("JUD", 1, 0, 1, 0)]),
        (
            "John 3:16; 4:24",
            Some("en"),
            &[("JHN", 3, 16, 3, 16), ("JHN", 4, 24, 4, 24)],
        ),
        (
            "John 3:16, 4:24",
            Some("en"),
            &[("JHN", 3, 16, 3, 16), ("JHN", 4, 24, 4, 24)],
        ),
        // Spanish
        ("Juan 3:16", Some("es"), &[("JHN", 3, 16, 3, 16)]),
        ("Génesis 1:1-5", Some("es"), &[("GEN", 1, 1, 1, 5)]),
        ("Genesis 1:1", Some("es"), &[("GEN", 1, 1, 1, 1)]),
        ("1 Corintios 13:4-8", Some("es"), &[("1CO", 13, 4, 13, 8)]),
        ("Mateo 5, 3-12", Some("es"), &[("MAT", 5, 3, 5, 12)]),
        ("Hechos 2:38", Some("es"), &[("ACT", 2, 38, 2, 38)]),
        ("Apocalipsis 21:4", Some("es"), &[("REV", 21, 4, 21, 4)]),
        // Portuguese (including Jó vs Jo disambiguation & ordinals)
        ("João 3:16", Some("pt"), &[("JHN", 3, 16, 3, 16)]),
        ("Jo 3:16", Some("pt"), &[("JHN", 3, 16, 3, 16)]),
        ("Jó 1:21", Some("pt"), &[("JOB", 1, 21, 1, 21)]),
        ("1ª João 4:8", Some("pt"), &[("1JN", 4, 8, 4, 8)]),
        ("Salmos 91:1-2", Some("pt"), &[("PSA", 91, 1, 91, 2)]),
        ("Provérbios 3:5-6", Some("pt"), &[("PRO", 3, 5, 3, 6)]),
        ("Jonas 2:2", Some("pt"), &[("JON", 2, 2, 2, 2)]),
        // French (European commas, compact lists, ordinals, ligatures/accents)
        ("Jean 3,16", Some("fr"), &[("JHN", 3, 16, 3, 16)]),
        ("Jean 3, 16-18", Some("fr"), &[("JHN", 3, 16, 3, 18)]),
        (
            "Jn 3,16, 18-19",
            Some("fr"),
            &[("JHN", 3, 16, 3, 16), ("JHN", 3, 18, 3, 19)],
        ),
        (
            "1re Corinthiens 13:13",
            Some("fr"),
            &[("1CO", 13, 13, 13, 13)],
        ),
        ("Ésaïe 40:31", Some("fr"), &[("ISA", 40, 31, 40, 31)]),
        ("Psaumes 23:1-4", Some("fr"), &[("PSA", 23, 1, 23, 4)]),
        ("Genèse 1:1", None, &[("GEN", 1, 1, 1, 1)]),
        // Korean (with 장 / 편 / 절 markers, with/without spaces, no hint)
        ("창세기 1:1", Some("ko"), &[("GEN", 1, 1, 1, 1)]),
        ("창 1장 1-3절", Some("ko"), &[("GEN", 1, 1, 1, 3)]),
        ("시편 23편", Some("ko"), &[("PSA", 23, 0, 23, 0)]),
        ("시 23편 1절", Some("ko"), &[("PSA", 23, 1, 23, 1)]),
        ("요한복음 3장 16절", None, &[("JHN", 3, 16, 3, 16)]),
        ("요한1서 4:8", Some("ko"), &[("1JN", 4, 8, 4, 8)]),
        (
            "고린도전서 13장 4-7절",
            Some("ko"),
            &[("1CO", 13, 4, 13, 7)],
        ),
        ("요한계시록 21:4", Some("ko"), &[("REV", 21, 4, 21, 4)]),
        // Yoruba (precomposed & decomposed diacritics, undotted fallbacks)
        ("Jòhánù 3:16", Some("yo"), &[("JHN", 3, 16, 3, 16)]),
        ("Orin Dáfídì 23:1-6", Some("yo"), &[("PSA", 23, 1, 23, 6)]),
        ("Orin Dafidi 121:1-2", None, &[("PSA", 121, 1, 121, 2)]),
        (
            "Ìṣe Àwọn Àpọ́sítélì 2:38",
            Some("yo"),
            &[("ACT", 2, 38, 2, 38)],
        ),
        (
            "O\u{0323}badáya\u{0300} 1:3",
            Some("yo"),
            &[("OBA", 1, 3, 1, 3)],
        ),
        ("1 Àwọn Ọba 18:21", Some("yo"), &[("1KI", 18, 21, 18, 21)]),
        // Typography / fullwidth
        ("Ps 23:1\u{2013}6", Some("en"), &[("PSA", 23, 1, 23, 6)]),
        (
            "\u{FF11} Cor \u{FF11}\u{FF13}\u{FF1A}\u{FF14}\u{FF0D}\u{FF17}",
            Some("en"),
            &[("1CO", 13, 4, 13, 7)],
        ),
    ];

    for (raw, hint, expected) in syntax_cases {
        let parsed = parse_reference(raw, *hint)
            .unwrap_or_else(|e| panic!("syntax case '{raw}' failed: {e}"));
        let actual: Vec<(&str, u32, u32, u32, u32)> = parsed
            .spans
            .iter()
            .map(|s| {
                (
                    s.book_code.as_str(),
                    s.start_chapter,
                    s.start_verse,
                    s.end_chapter,
                    s.end_verse,
                )
            })
            .collect();
        assert_eq!(actual, *expected, "mismatch for '{raw}'");
        executed_cases += 1;
    }

    assert!(
        executed_cases >= 750,
        "expected well over 300 reference cases, ran {executed_cases}"
    );

    // Part 3: Invalid references must fail cleanly
    for bad in [
        "",
        "   ",
        "NoSuchBook 3:16",
        "John",
        "Jn 0:16",
        "Jn 3:0",
        "Jn 3:18-16",
        "Gen 4-2",
        "Jn 3:16 foo",
        "Jn 3:16;",
        ";Jn 3:16",
        "Jn 3:16,",
        "Jn 3:16\0",
        &format!("John {}", "3:16; ".repeat(40)),
        &"a".repeat(600),
    ] {
        assert!(parse_reference(bad, None).is_err(), "'{bad}' should fail");
    }
}

#[test]
fn bundled_kjv_and_web_bibles_materialize_query_search_and_delete_without_resurrection() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::open(dir.path()).unwrap();

    // First run materializes KJV (31,102 verses) and WEB (31,098 verses)
    assert!(ensure_bundled_bibles(&library.paths().bibles).unwrap());
    // Second run is an immediate no-op
    assert!(!ensure_bundled_bibles(&library.paths().bibles).unwrap());

    let listed = list_bibles(&library, true).unwrap();
    assert_eq!(listed.bibles.len(), 2);

    let kjv = listed.bibles.iter().find(|b| b.code == "KJV").unwrap();
    assert_eq!(kjv.name, "King James Version");
    assert_eq!(kjv.language, "en");
    assert!(kjv.is_bundled);
    assert_eq!(kjv.book_count, 66);
    assert_eq!(kjv.verse_count, 31_102);
    assert_eq!(kjv.books.len(), 66);
    assert_eq!(kjv.books[0].chapter_count, 50); // Genesis
    assert_eq!(kjv.books[18].chapter_count, 150); // Psalms
    assert_eq!(kjv.books[18].verse_counts[118], 176); // Psalm 119
    assert_eq!(kjv.books[30].verse_counts, vec![21]); // Obadiah

    let web = listed.bibles.iter().find(|b| b.code == "WEB").unwrap();
    assert_eq!(web.name, "World English Bible");
    assert_eq!(web.language, "en");
    assert!(web.is_bundled);
    assert_eq!(web.book_count, 66);
    assert_eq!(web.verse_count, 31_098);

    // Exact passage retrieval across single verse, list, chapter, and cross-lingual reference
    let jn316 = get_passages(&library, "KJV", "Jn 3:16", &[], "").unwrap();
    assert_eq!(jn316.normalized_reference, "John 3:16");
    assert_eq!(jn316.verses.len(), 1);
    assert!(jn316.verses[0].text.contains("For God so loved the world"));

    let rom = get_passages(&library, "WEB", "Rom 8:28, 31-32, 38-39", &[], "").unwrap();
    assert_eq!(rom.verses.len(), 5);
    assert_eq!(
        rom.verses.iter().map(|v| v.verse).collect::<Vec<_>>(),
        vec![28, 31, 32, 38, 39]
    );

    let cor13 = get_passages(&library, "KJV", "1 Cor 13", &[], "").unwrap();
    assert_eq!(cor13.verses.len(), 13);

    // Spanish reference against bundled KJV falls back across languages cleanly
    let juan = get_passages(&library, "KJV", "Juan 3:16", &[], "").unwrap();
    assert_eq!(juan.verses[0].verse, 16);
    assert_eq!(juan.verses[0].book_code, "JHN");

    // WEB omits 4 verses present in KJV (e.g. Acts 8:37) — range skips cleanly without error
    let web_acts8 = get_passages(&library, "WEB", "Acts 8:35-38", &[], "").unwrap();
    assert_eq!(web_acts8.verses.len(), 3);

    // Out-of-range / nonexistent passages or translations
    assert!(matches!(
        get_passages(&library, "KJV", "John 99:1", &[], ""),
        Err(CommandBusError::NotFound(_))
    ));
    assert!(matches!(
        get_passages(&library, "NOPE", "John 3:16", &[], ""),
        Err(CommandBusError::NotFound(_))
    ));
    assert!(matches!(
        get_passages(&library, "KJV", "Psalms 1-150", &[], ""),
        Err(CommandBusError::Invalid(_))
    ));

    // Full-text & typo search inside bundled KJV
    let hits = library
        .search_scripture("God so loved world", "KJV", 5, 0)
        .unwrap();
    assert_eq!(hits.hits[0].book, "JHN");
    assert_eq!(hits.hits[0].chapter, 3);
    assert_eq!(hits.hits[0].verse, 16);

    // Deleting a bundled translation works and is NOT resurrected on next startup
    assert!(delete_bible(&library, "KJV").unwrap());
    let after_delete = list_bibles(&library, false).unwrap();
    assert_eq!(after_delete.bibles.len(), 1);
    assert_eq!(after_delete.bibles[0].code, "WEB");

    assert!(!ensure_bundled_bibles(&library.paths().bibles).unwrap());
    let after_restart = list_bibles(&library, false).unwrap();
    assert_eq!(after_restart.bibles.len(), 1);
}

#[test]
fn m2_a5_osis_usfm_and_zefania_importers_parse_and_enforce_security_limits() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::open(dir.path()).unwrap();

    // 1. OSIS sample (both milestone and container verses, namespace prefix, notes stripped)
    let osis_sample = r#"<?xml version="1.0" encoding="UTF-8"?>
<osis xmlns="http://www.bibletechnologies.net/2003/OSIS/namespace">
  <osisText osisIDWork="OSIS-1" xml:lang="en">
    <header>
      <work osisWork="OSIS-1">
        <title>OSIS Sample Bible</title>
        <identifier type="OSIS">OSIS-1</identifier>
        <language>eng</language>
      </work>
    </header>
    <div type="book" osisID="Ps">
      <title>Psalm 23</title>
      <verse osisID="Ps.23.1" sID="Ps.23.1"/>
        <lg><l>The <divineName>Lord</divineName> is my shepherd;<note type="study">Hebrew YHWH</note></l>
        <l>I shall not want.</l></lg>
      <verse eID="Ps.23.1"/>
    </div>
    <div type="book" osisID="John">
      <verse osisID="John.3.16">For God so &ldquo;loved&rdquo; the world&#x2014;that he gave.</verse>
    </div>
  </osisText>
</osis>"#;

    let osis_res = import_bible(
        &library,
        &BibleImportRequest {
            source_path: "",
            raw_bytes: osis_sample.as_bytes(),
            format: BibleFormat::Unspecified,
            code: "",
            name: "",
            language: "",
            replace_existing: false,
        },
    )
    .unwrap();
    assert_eq!(osis_res.code, "OSIS-1");
    assert_eq!(osis_res.name, "OSIS Sample Bible");
    assert_eq!(osis_res.language, "en");
    assert_eq!(osis_res.book_count, 2);
    assert_eq!(osis_res.verse_count, 2);
    assert!(!osis_res.is_bundled);

    let osis_ps = get_passages(&library, "OSIS-1", "Ps 23:1", &[], "").unwrap();
    assert!(!osis_ps.verses[0].text.contains("Hebrew YHWH"));
    assert!(
        osis_ps.verses[0]
            .text
            .contains("shepherd; I shall not want.")
    );
    let osis_jn = get_passages(&library, "OSIS-1", "Jn 3:16", &[], "").unwrap();
    assert!(osis_jn.verses[0].text.contains("\u{201C}loved\u{201D}"));
    assert!(osis_jn.verses[0].text.contains("\u{2014}"));

    // 2. USFM sample (file path import, paratext skipped, footnotes/Strong's stripped, continuation lines)
    let usfm_path = dir.path().join("sample.usfm");
    let usfm_sample = "\
\\id FRT Frontmatter\n\
\\ip Ignore this intro paragraph.\n\
\\id GEN USFM Sample\n\
\\h Genesis\n\
\\c 1\n\
\\s1 Creation\n\
\\p\n\
\\v 1 In the \\w beginning|strong=\"H7225\"\\w* \\w God|strong=\"H430\"\\w* created\\f + \\fr 1:1 \\ft Heb. elohim\\f* the heaven\n\
\\q1 and the earth.\n\
\\id PSA\n\
\\c 23\n\
\\v 1-2 The \\nd LORD\\nd* is my shepherd;\n\
\\q2 he makes me lie down in green pastures.\n";
    std::fs::write(&usfm_path, usfm_sample).unwrap();

    let usfm_res = import_bible(
        &library,
        &BibleImportRequest {
            source_path: usfm_path.to_str().unwrap(),
            raw_bytes: &[],
            format: BibleFormat::Unspecified,
            code: "USFM-1",
            name: "USFM Sample",
            language: "en",
            replace_existing: false,
        },
    )
    .unwrap();
    assert_eq!(usfm_res.book_count, 2);
    assert_eq!(usfm_res.verse_count, 2);
    let gen1 = get_passages(&library, "USFM-1", "Gen 1:1", &[], "").unwrap();
    assert_eq!(
        gen1.verses[0].text,
        "In the beginning God created the heaven and the earth."
    );
    let ps23 = get_passages(&library, "USFM-1", "Ps 23:1", &[], "").unwrap();
    assert_eq!(
        ps23.verses[0].text,
        "The LORD is my shepherd; he makes me lie down in green pastures."
    );

    // 3. Zefania XML sample (with <NOTE> ignored, <BR/> spaced, French metadata)
    let zefania_sample = r#"<?xml version="1.0" encoding="utf-8"?>
<XMLBIBLE biblename="Zefania Louis">
  <INFORMATION>
    <title>Louis Segond Sample</title>
    <identifier>LSG_S</identifier>
    <language>FRA</language>
  </INFORMATION>
  <BIBLEBOOK bnumber="43" bname="Jean">
    <CHAPTER cnumber="3">
      <VERS vnumber="16">Car Dieu a tant aimé le monde<NOTE>Note Jean 3:16</NOTE><BR/>qu'il a donné son Fils unique.</VERS>
    </CHAPTER>
  </BIBLEBOOK>
</XMLBIBLE>"#;
    let zef_res = import_bible(
        &library,
        &BibleImportRequest {
            source_path: "",
            raw_bytes: zefania_sample.as_bytes(),
            format: BibleFormat::Unspecified,
            code: "",
            name: "",
            language: "",
            replace_existing: false,
        },
    )
    .unwrap();
    assert_eq!(zef_res.code, "LSG_S");
    assert_eq!(zef_res.language, "fr");
    assert_eq!(zef_res.verse_count, 1);

    let french_lookup = get_passages(&library, "LSG_S", "Jean 3, 16", &[], "").unwrap();
    assert!(!french_lookup.verses[0].text.contains("Note Jean 3:16"));
    assert!(
        french_lookup.verses[0]
            .text
            .contains("tant aimé le monde qu'il")
    );

    // Duplicate refusal unless replace_existing = true
    assert!(matches!(
        import_bible(
            &library,
            &BibleImportRequest {
                source_path: "",
                raw_bytes: zefania_sample.as_bytes(),
                format: BibleFormat::Zefania,
                code: "LSG_S",
                name: "Duplicate",
                language: "fr",
                replace_existing: false,
            }
        ),
        Err(CommandBusError::Invalid(_))
    ));
    let replaced = import_bible(
        &library,
        &BibleImportRequest {
            source_path: "",
            raw_bytes: zefania_sample.as_bytes(),
            format: BibleFormat::Zefania,
            code: "LSG_S",
            name: "Louis Segond Replaced",
            language: "fr",
            replace_existing: true,
        },
    )
    .unwrap();
    assert_eq!(replaced.name, "Louis Segond Replaced");

    // SEC-03 adversarial refusals: DTD/XXE, deep nesting > MAX_XML_DEPTH, NUL bytes, path traversal code
    let dtd_attack = b"<!DOCTYPE osis [<!ENTITY lol \"lol\">]><osis><verse osisID=\"Gen.1.1\">&lol;</verse></osis>";
    assert!(matches!(
        import_bible(
            &library,
            &BibleImportRequest {
                source_path: "",
                raw_bytes: dtd_attack,
                format: BibleFormat::Osis,
                code: "EVIL",
                name: "Evil",
                language: "en",
                replace_existing: false,
            }
        ),
        Err(CommandBusError::Invalid(_))
    ));

    let deep_xml = format!(
        "{}<verse osisID=\"Gen.1.1\">hi</verse>{}",
        "<div>".repeat(MAX_XML_DEPTH + 4),
        "</div>".repeat(MAX_XML_DEPTH + 4)
    );
    assert!(matches!(
        import_bible(
            &library,
            &BibleImportRequest {
                source_path: "",
                raw_bytes: deep_xml.as_bytes(),
                format: BibleFormat::Osis,
                code: "DEEP",
                name: "Deep",
                language: "en",
                replace_existing: false,
            }
        ),
        Err(CommandBusError::Invalid(_))
    ));

    assert!(matches!(
        import_bible(
            &library,
            &BibleImportRequest {
                source_path: "",
                raw_bytes: zefania_sample.as_bytes(),
                format: BibleFormat::Zefania,
                code: "../escape",
                name: "BadCode",
                language: "en",
                replace_existing: false,
            }
        ),
        Err(CommandBusError::Invalid(_))
    ));

    let nul_usfm = b"\\id GEN\n\\c 1\n\\v 1 Bad\0verse";
    assert!(matches!(
        import_bible(
            &library,
            &BibleImportRequest {
                source_path: "",
                raw_bytes: nul_usfm,
                format: BibleFormat::Usfm,
                code: "NUL1",
                name: "Nul",
                language: "en",
                replace_existing: false,
            }
        ),
        Err(CommandBusError::Invalid(_))
    ));
}

#[test]
fn scripture_slide_splitter_wraps_packs_and_places_references() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::open(dir.path()).unwrap();
    ensure_bundled_bibles(&library.paths().bibles).unwrap();

    // Long single verse (Esther 8:9 in KJV) splits cleanly across multiple slides in one-verse mode
    let esther = get_passages(&library, "KJV", "Esth 8:9", &[], "").unwrap();
    let esther_slides = split_scripture_slides(&SplitScriptureSlides {
        translation: "KJV".into(),
        translation_name: "King James Version".into(),
        reference: esther.normalized_reference.clone(),
        verses: esther.verses.clone(),
        max_lines: 3,
        max_chars_per_line: 42,
        verses_per_slide: 0,
        one_verse_per_slide: true,
        show_verse_numbers: true,
        reference_placement: ReferencePlacement::SlideFooter as i32,
        title_slide: true,
        blank_slide_at_end: true,
        ..Default::default()
    })
    .unwrap();

    assert_eq!(esther_slides.reference_label, "Esther 8:9 (KJV)");
    assert!(esther_slides.slides.first().unwrap().is_title);
    assert!(esther_slides.slides.last().unwrap().is_blank);
    let content: Vec<_> = esther_slides
        .slides
        .iter()
        .filter(|s| !s.is_title && !s.is_blank)
        .collect();
    assert!(content.len() >= 3);
    for (idx, s) in content.iter().enumerate() {
        assert_eq!(s.part_index, (idx + 1) as u32);
        assert_eq!(s.part_count, content.len() as u32);
        assert!(s.lines.len() <= 3);
        assert!(s.lines.iter().all(|l| l.chars().count() <= 42));
        assert_eq!(s.footer_reference, "Esther 8:9 (KJV)");
        assert!(s.header_reference.is_empty());
    }

    // Multi-verse passage packing with header placement and no verse numbers
    let rom = get_passages(&library, "WEB", "Rom 8:28-30", &[], "").unwrap();
    let rom_slides = split_scripture_slides(&SplitScriptureSlides {
        translation: "WEB".into(),
        translation_name: "World English Bible".into(),
        reference: String::new(),
        verses: rom.verses,
        max_lines: 4,
        max_chars_per_line: 48,
        verses_per_slide: 2,
        one_verse_per_slide: false,
        show_verse_numbers: false,
        reference_placement: ReferencePlacement::SlideHeader as i32,
        title_slide: false,
        blank_slide_at_end: false,
        ..Default::default()
    })
    .unwrap();
    assert!(!rom_slides.slides.is_empty());
    assert!(
        rom_slides
            .slides
            .iter()
            .all(|s| !s.header_reference.is_empty() && s.footer_reference.is_empty())
    );
    assert!(
        rom_slides
            .slides
            .iter()
            .all(|s| !s.lines[0].starts_with("28 "))
    );

    // Invalid bounds fail cleanly
    assert!(
        split_scripture_slides(&SplitScriptureSlides {
            max_lines: 101,
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        split_scripture_slides(&SplitScriptureSlides {
            max_chars_per_line: 8,
            ..Default::default()
        })
        .is_err()
    );

    // Multi-byte UTF-8 overlong token splits safely along Unicode scalar boundaries
    let korean_long = split_scripture_slides(&SplitScriptureSlides {
        translation: "KJV".into(),
        translation_name: "Korean Sample".into(),
        reference: "요한복음 3:16".into(),
        verses: vec![selah_proto::v1::ScriptureVerse {
            translation: "KJV".into(),
            book_num: 43,
            book_code: "JHN".into(),
            book_name: "John".into(),
            chapter: 3,
            verse: 16,
            text: "하나님이세상을이처럼사랑하사독생자를주셨으니이는그를믿는자마다멸망하지않고"
                .into(),
        }],
        max_lines: 2,
        max_chars_per_line: 16,
        verses_per_slide: 1,
        one_verse_per_slide: true,
        show_verse_numbers: false,
        reference_placement: ReferencePlacement::SlideFooter as i32,
        title_slide: false,
        blank_slide_at_end: false,
        ..Default::default()
    })
    .unwrap();
    assert!(korean_long.slides.len() >= 2);
    for slide in &korean_long.slides {
        for line in &slide.lines {
            assert!(line.chars().count() <= 16);
        }
    }
}

#[cfg(unix)]
#[test]
fn importing_bible_from_symlink_source_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::open(dir.path()).unwrap();
    let real = dir.path().join("real.usfm");
    std::fs::write(&real, "\\id GEN\n\\c 1\n\\v 1 In the beginning").unwrap();
    let link = dir.path().join("link.usfm");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let err = import_bible(
        &library,
        &BibleImportRequest {
            source_path: link.to_str().unwrap(),
            raw_bytes: &[],
            format: BibleFormat::Usfm,
            code: "SYM1",
            name: "Symlink",
            language: "en",
            replace_existing: false,
        },
    )
    .unwrap_err();
    assert!(matches!(err, CommandBusError::Invalid(_)));
}
