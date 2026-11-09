use std::collections::HashSet;
use std::path::{Path, PathBuf};

use selah_core::library::{Document, Library, new_id};
use selah_core::songs::parse::{
    MAX_BLOCK_DEPTH, MAX_CHORDS_PER_LINE, MAX_LINE_BYTES, MAX_LINES_PER_SECTION, MAX_PASTE_BYTES,
    MAX_PASTE_LINES, MAX_SECTIONS,
};
use selah_core::songs::split::{format_copyright_line, generate_slides};
use selah_core::songs::{
    SplitOptions, color_to_hex, detect_format, parse_chord, parse_key, parse_song,
    section_kind_key, transpose_chord, transpose_key, transpose_song,
};
use selah_proto::v1::{
    Arrangement, CopyrightPlacement, SectionKind, Song, SongPasteFormat, TranslationLines,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ExpectedHymn {
    title: String,
    author: String,
    key: String,
    public_domain: bool,
    arrangement: Vec<String>,
    sections: Vec<ExpectedSection>,
}

#[derive(Debug, Deserialize)]
struct ExpectedSection {
    label: String,
    kind: String,
    color_hex: String,
    hotkey: String,
    lines: Vec<ExpectedLine>,
}

#[derive(Debug, Deserialize)]
struct ExpectedLine {
    text: String,
    chords: Vec<ExpectedChord>,
}

#[derive(Debug, Deserialize)]
struct ExpectedChord {
    position: u32,
    text: String,
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/songs")
}

fn arrangement_labels(song: &Song) -> Vec<String> {
    assert_eq!(song.arrangements.len(), 1);
    song.arrangements[0]
        .section_ids
        .iter()
        .map(|id| {
            song.sections
                .iter()
                .find(|s| s.id.as_ref() == Some(id))
                .expect("arrangement section id exists")
                .label
                .clone()
        })
        .collect()
}

fn assert_matches_expected(
    slug: &str,
    format_name: &str,
    song: &Song,
    expected: &ExpectedHymn,
    expect_chords: bool,
    expect_chordpro_metadata: bool,
) {
    song.validate()
        .unwrap_or_else(|e| panic!("{slug} [{format_name}] invalid song: {e}"));
    assert_eq!(song.title, expected.title, "{slug} [{format_name}] title");
    assert_eq!(
        arrangement_labels(song),
        expected.arrangement,
        "{slug} [{format_name}] arrangement"
    );
    assert_eq!(
        song.sections.len(),
        expected.sections.len(),
        "{slug} [{format_name}] section count"
    );

    if expect_chordpro_metadata {
        assert_eq!(
            song.authors,
            vec![expected.author.clone()],
            "{slug} [{format_name}] author"
        );
        assert_eq!(song.key, expected.key, "{slug} [{format_name}] key");
        assert_eq!(
            song.public_domain, expected.public_domain,
            "{slug} [{format_name}] public_domain"
        );
    }

    let hotkeys: HashSet<&str> = song.sections.iter().map(|s| s.hotkey.as_str()).collect();
    assert_eq!(
        hotkeys.len(),
        song.sections.len(),
        "{slug} [{format_name}] unique hotkeys"
    );

    for (sec, exp_sec) in song.sections.iter().zip(expected.sections.iter()) {
        let kind = SectionKind::try_from(sec.kind).expect("valid SectionKind");
        assert_eq!(sec.label, exp_sec.label, "{slug} [{format_name}] label");
        assert_eq!(
            section_kind_key(kind),
            exp_sec.kind,
            "{slug} [{format_name}] kind"
        );
        let color = sec.color.as_ref().expect("section color assigned");
        assert_eq!(
            color_to_hex(color),
            exp_sec.color_hex,
            "{slug} [{format_name}] color"
        );
        assert_eq!(sec.hotkey, exp_sec.hotkey, "{slug} [{format_name}] hotkey");
        assert_eq!(
            sec.lines.len(),
            exp_sec.lines.len(),
            "{slug} [{format_name}] section {} line count",
            exp_sec.label
        );
        for (line_idx, (line, exp_line)) in sec.lines.iter().zip(exp_sec.lines.iter()).enumerate() {
            assert_eq!(
                line.text, exp_line.text,
                "{slug} [{format_name}] section {} line {line_idx} text",
                exp_sec.label
            );
            if expect_chords {
                let got_chords: Vec<(u32, &str)> = line
                    .chords
                    .iter()
                    .map(|c| (c.position, c.text.as_str()))
                    .collect();
                let want_chords: Vec<(u32, &str)> = exp_line
                    .chords
                    .iter()
                    .map(|c| (c.position, c.text.as_str()))
                    .collect();
                assert_eq!(
                    got_chords, want_chords,
                    "{slug} [{format_name}] section {} line {line_idx} chords",
                    exp_sec.label
                );
            } else {
                assert!(
                    line.chords.is_empty(),
                    "{slug} [{format_name}] expected no chords in text-only format"
                );
            }
        }
    }
}

#[test]
fn curated_25_hymns_match_expected_across_all_four_paste_formats() {
    // M2-A1: >= 20 songs across all 4 formats -> 25 hymns x 4 formats = 100 checks at 100%.
    let root = fixture_root();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("fixtures/songs exists")
        .filter_map(|e| e.ok().map(|ent| ent.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    assert_eq!(
        dirs.len(),
        25,
        "expected 25 curated hymn fixture directories"
    );

    let db_dir = tempfile::tempdir().unwrap();
    let mut library = Library::open(db_dir.path()).unwrap();

    let mut total_checks = 0usize;
    for dir in dirs {
        let slug = dir.file_name().unwrap().to_string_lossy().into_owned();
        let expected: ExpectedHymn = serde_json::from_str(
            &std::fs::read_to_string(dir.join("expected.json")).expect("expected.json"),
        )
        .unwrap_or_else(|e| panic!("{slug} expected.json parse error: {e}"));

        let formats = [
            ("plain.txt", SongPasteFormat::Plain, false, false),
            ("markers.txt", SongPasteFormat::Markers, false, false),
            ("chords.txt", SongPasteFormat::ChordsOverLyrics, true, false),
            ("song.cho", SongPasteFormat::ChordPro, true, true),
        ];

        for (file, expected_fmt, expect_chords, expect_meta) in formats {
            let raw = std::fs::read_to_string(dir.join(file))
                .unwrap_or_else(|e| panic!("{slug}/{file}: {e}"));
            let detected = detect_format(&raw).unwrap();
            assert_eq!(detected, expected_fmt, "{slug}/{file} auto-detection");

            let out = parse_song(&raw, SongPasteFormat::Unspecified, &expected.title)
                .unwrap_or_else(|e| panic!("{slug}/{file} parse failed: {e}"));
            assert_eq!(out.detected_format, expected_fmt);
            assert_matches_expected(
                &slug,
                file,
                &out.song,
                &expected,
                expect_chords,
                expect_meta,
            );

            // Every parsed ChordPro fixture must also survive Library save/load round-trip cleanly.
            if expect_meta {
                library.save(&out.song).unwrap();
                let loaded = library
                    .get::<Song>(out.song.id.as_ref().unwrap())
                    .unwrap()
                    .expect("saved curated hymn exists");
                assert_eq!(loaded, out.song);
            }
            total_checks += 1;
        }
    }
    assert_eq!(total_checks, 100);
}

#[test]
fn parses_all_section_kinds_and_assigns_colors_and_unique_hotkeys() {
    let input = "\
[Intro]
Instrumental opening

[Verse 1]
First verse line

[Pre-Chorus]
Rising lift before chorus

[Chorus]
Main refrain line

[Bridge]
Bridge proclamation

[Interlude]
Soft keys pad

[Tag]
Repeat the promise

[Chorus]

[Outro]
Amen and amen";

    let out = parse_song(input, SongPasteFormat::Unspecified, "Complete Song").unwrap();
    assert_eq!(out.detected_format, SongPasteFormat::Markers);
    assert!(out.song.validate().is_ok());

    let secs = &out.song.sections;
    let labels: Vec<&str> = secs.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(
        labels,
        vec![
            "Intro",
            "Verse 1",
            "Pre-Chorus",
            "Chorus",
            "Bridge",
            "Interlude",
            "Tag",
            "Outro"
        ]
    );

    assert_eq!(secs[0].kind, SectionKind::Intro as i32);
    assert_eq!(secs[1].kind, SectionKind::Verse as i32);
    assert_eq!(secs[2].kind, SectionKind::Other as i32);
    assert_eq!(secs[3].kind, SectionKind::Chorus as i32);
    assert_eq!(secs[4].kind, SectionKind::Bridge as i32);
    assert_eq!(secs[5].kind, SectionKind::Other as i32);
    assert_eq!(secs[6].kind, SectionKind::Tag as i32);
    assert_eq!(secs[7].kind, SectionKind::Outro as i32);

    assert_eq!(color_to_hex(secs[1].color.as_ref().unwrap()), "#2378C3");
    assert_eq!(color_to_hex(secs[3].color.as_ref().unwrap()), "#C84281");
    assert_eq!(color_to_hex(secs[4].color.as_ref().unwrap()), "#E66F0E");
    assert_eq!(color_to_hex(secs[6].color.as_ref().unwrap()), "#009EC1");
    assert_eq!(color_to_hex(secs[0].color.as_ref().unwrap()), "#71767A");
    assert_eq!(color_to_hex(secs[2].color.as_ref().unwrap()), "#71767A");

    let hotkeys: Vec<&str> = secs.iter().map(|s| s.hotkey.as_str()).collect();
    assert_eq!(hotkeys, vec!["I", "1", "P", "C", "B", "N", "T", "O"]);
    // Empty `[Chorus]` recall reused Chorus section without duplicating it
    assert_eq!(
        arrangement_labels(&out.song),
        vec![
            "Intro",
            "Verse 1",
            "Pre-Chorus",
            "Chorus",
            "Bridge",
            "Interlude",
            "Tag",
            "Chorus",
            "Outro"
        ]
    );
}

#[test]
fn multilingual_unicode_lyrics_compute_exact_char_offsets() {
    // Yoruba, Spanish, French, Korean diacritics & multibyte UTF-8 glyphs before chords.
    let chordpro = "\
{title: Multilingual Worship}
{tags: yoruba, spanish, french, korean}
{sov: Verse 1}
Ọlọ́run [G]wa, olù[D]gbàlà
Cuán [C]grande es [G]Él
À [Em]Dieu soit [D]la gloire
나 같은 [Am]죄인 [D]살리신
{eov}";

    let out = parse_song(chordpro, SongPasteFormat::Unspecified, "").unwrap();
    assert_eq!(out.detected_format, SongPasteFormat::ChordPro);
    assert!(out.song.validate().is_ok());
    assert_eq!(out.song.tags, vec!["yoruba", "spanish", "french", "korean"]);

    let lines = &out.song.sections[0].lines;
    assert_eq!(lines[0].text, "Ọlọ́run wa, olùgbàlà");
    // `"Ọlọ́run "` -> Ọ(1) l(2) ọ(3) combinig-acute(4) r(5) u(6) n(7) ' '(8) = 8 Unicode scalars
    assert_eq!(
        lines[0].chords[0].position as usize,
        "Ọlọ́run ".chars().count()
    );
    assert_eq!(
        lines[0].chords[1].position as usize,
        "Ọlọ́run wa, olù".chars().count()
    );
    assert_eq!(
        lines[1].chords[0].position as usize,
        "Cuán ".chars().count()
    );
    assert_eq!(lines[2].chords[0].position as usize, "À ".chars().count());
    assert_eq!(
        lines[3].chords[0].position as usize,
        "나 같은 ".chars().count()
    );

    // Chords-over-lyrics with tabs expanded and multibyte line underneath
    let col = "\
[Verse 1]
\t   G         D
Ọlọ́run wa, olùgbàlà";
    let col_out = parse_song(col, SongPasteFormat::Unspecified, "Yoruba").unwrap();
    assert_eq!(col_out.detected_format, SongPasteFormat::ChordsOverLyrics);
    assert!(col_out.song.validate().is_ok());
    assert_eq!(col_out.song.sections[0].lines[0].chords[0].position, 7);
    assert_eq!(col_out.song.sections[0].lines[0].chords[1].position, 17);
}

#[test]
fn chords_over_lyrics_and_chordpro_edge_cases() {
    // Barlines, repeat markers, orphan instrumental chord line, plus literal brackets
    let col = "\
[Intro]
| G  C | D  G | (x2)

[Verse 1]
G     D/F#
Stand up and sing
A
bright new morning awaits";
    let out = parse_song(col, SongPasteFormat::Unspecified, "Edge Case").unwrap();
    assert_eq!(out.detected_format, SongPasteFormat::ChordsOverLyrics);
    assert!(out.song.validate().is_ok());
    assert_eq!(out.song.sections[0].lines[0].text, "");
    let intro_chords: Vec<&str> = out.song.sections[0].lines[0]
        .chords
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(intro_chords, vec!["G", "C", "D", "G"]);
    assert_eq!(out.song.sections[1].lines[1].chords[0].text, "A");

    // ChordPro comment blocks, tabs stripped, # comments ignored, unclosed literal '[' preserved
    let cho = "\
# leading comment
{title: Mixed Directives}
{tempo: 76.5}
{ccli: 99999}
{comment: Bridge}
[G]Line with [unclosed bracket
{sot}
E|---0---|
{eot}
[D]After tab line";
    let cho_out = parse_song(cho, SongPasteFormat::Unspecified, "").unwrap();
    assert_eq!(cho_out.song.tempo, 76.5);
    assert_eq!(cho_out.song.ccli_number, "99999");
    assert_eq!(cho_out.song.sections[0].kind, SectionKind::Bridge as i32);
    assert_eq!(
        cho_out.song.sections[0].lines[0].text,
        "Line with [unclosed bracket"
    );
    assert_eq!(cho_out.song.sections[0].lines[1].text, "After tab line");
}

#[test]
fn transposition_cycles_cleanly_and_handles_all_12_semitones_and_spellings() {
    let raw = "\
{title: Cycle Test}
{key: D}
{sov}
[D]Holy, [Bm7]holy, [G/B]holy [A7sus4]Lord [D/F#]God
{eov}";
    let original = parse_song(raw, SongPasteFormat::ChordPro, "").unwrap().song;

    let mut cycled = original.clone();
    for _ in 0..12 {
        transpose_song(&mut cycled, 1, None).unwrap();
    }
    assert_eq!(cycled.key, "D");
    let cycled_chords: Vec<&str> = cycled.sections[0].lines[0]
        .chords
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(cycled_chords, vec!["D", "Bm7", "G/B", "A7sus4", "D/F#"]);

    // Auto flat key: G + 3 -> Bb major
    let mut to_bb = original.clone();
    to_bb.key = "G".into();
    transpose_song(&mut to_bb, 3, None).unwrap();
    assert_eq!(to_bb.key, "Bb");
    assert_eq!(transpose_key("Em", 1, None).unwrap(), "Fm");
    assert_eq!(parse_key("F#m"), Some((6, true, false)));
    assert_eq!(transpose_chord("C#m7/G#", -1, true).unwrap(), "Cm7/G");
    assert!(parse_chord("Dmaj9/F#").is_some());
    assert!(transpose_song(&mut to_bb, 200, None).is_err());
}

#[test]
fn rule_based_slide_splitting_and_copyright_formatting() {
    let mut song = parse_song(
        "\
{title: Blessed Assurance}
{artist: Fanny J. Crosby}
{ccli: 22329}
{copyright: Public Domain}
{sov: Verse 1}
Line 1
Line 2
Line 3
Line 4
Line 5
{eov}
{soc}
Chorus A
Chorus B
{eoc}",
        SongPasteFormat::ChordPro,
        "",
    )
    .unwrap()
    .song;

    song.sections[0].translations.insert(
        "es".into(),
        TranslationLines {
            lines: vec![
                "Linea 1".into(),
                "Linea 2".into(),
                "Linea 3".into(),
                "Linea 4".into(),
                "Linea 5".into(),
            ],
        },
    );
    let v1_id = song.sections[0].id.clone().unwrap();
    let ch_id = song.sections[1].id.clone().unwrap();
    song.arrangements.push(Arrangement {
        id: Some(new_id()),
        name: "Custom".into(),
        section_ids: vec![v1_id, ch_id.clone(), ch_id],
    });
    let custom_arr_id = song.arrangements[1].id.clone();

    assert_eq!(
        format_copyright_line(&song),
        "“Blessed Assurance” · Fanny J. Crosby · Public Domain · CCLI #22329"
    );

    let generated = generate_slides(
        &song,
        &SplitOptions {
            arrangement_id: custom_arr_id,
            max_lines: 2,
            copyright_placement: CopyrightPlacement::All,
            title_slide: true,
            blank_slide_at_end: true,
            translation_language: Some("es".into()),
        },
    )
    .unwrap();

    // Title(1) + Verse 1 (3 chunks: 2,2,1) + Chorus (1 chunk) + Chorus (1 chunk) + Blank(1) = 7
    assert_eq!(generated.slides.len(), 7);
    assert!(generated.slides[0].is_title);
    assert_eq!(generated.slides[1].part_index, 1);
    assert_eq!(generated.slides[1].part_count, 3);
    assert_eq!(
        generated.slides[1].translation_lines,
        vec!["Linea 1", "Linea 2"]
    );
    assert_eq!(generated.slides[3].part_index, 3);
    assert_eq!(generated.slides[3].translation_lines, vec!["Linea 5"]);
    assert_eq!(generated.slides[4].section_label, "Chorus");
    assert_eq!(generated.slides[5].section_label, "Chorus");
    // CopyrightPlacement::All places footer on every non-blank slide, never on trailing Blank slide
    for s in &generated.slides[..6] {
        assert_eq!(s.footer_copyright, generated.copyright_line);
    }
    assert!(generated.slides[6].is_blank);
    assert!(generated.slides[6].footer_copyright.is_empty());

    // `max_lines > 100` fails validation
    assert!(
        generate_slides(
            &song,
            &SplitOptions {
                max_lines: 101,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn sec03_parser_enforces_all_bounds_cleanly() {
    // 1. NUL bytes & empty input
    assert!(parse_song("", SongPasteFormat::Unspecified, "x").is_err());
    assert!(parse_song("   \n\t ", SongPasteFormat::Plain, "x").is_err());
    assert!(parse_song("abc\0def", SongPasteFormat::Plain, "x").is_err());

    // 2. Byte / line count / line length bounds
    assert!(
        parse_song(
            &"x".repeat(MAX_PASTE_BYTES + 1),
            SongPasteFormat::Plain,
            "x"
        )
        .is_err()
    );
    let too_many_lines = "word\n".repeat(MAX_PASTE_LINES + 1);
    assert!(parse_song(&too_many_lines, SongPasteFormat::Plain, "x").is_err());
    let long_line = "x".repeat(MAX_LINE_BYTES + 1);
    assert!(parse_song(&long_line, SongPasteFormat::Plain, "x").is_err());

    // 3. Section & lines-per-section & nesting bounds
    let many_sections: String = (1..=MAX_SECTIONS + 2)
        .map(|i| format!("[Part {i}]\nline\n"))
        .collect();
    assert!(parse_song(&many_sections, SongPasteFormat::Markers, "x").is_err());

    let many_lines_one_sec = format!("[Verse 1]\n{}", "line\n".repeat(MAX_LINES_PER_SECTION + 1));
    assert!(parse_song(&many_lines_one_sec, SongPasteFormat::Markers, "x").is_err());

    let nested = format!("{}\nline", "{sov}\n".repeat(MAX_BLOCK_DEPTH + 1));
    assert!(parse_song(&nested, SongPasteFormat::ChordPro, "x").is_err());

    // 4. Excess chords on a single line emits warning instead of allocating unbounded chords
    let excessive_chords = format!(
        "{{sov}}\n{}\n{{eov}}",
        "[G]la ".repeat(MAX_CHORDS_PER_LINE + 10)
    );
    let out = parse_song(&excessive_chords, SongPasteFormat::ChordPro, "Dense").unwrap();
    assert_eq!(
        out.song.sections[0].lines[0].chords.len(),
        MAX_CHORDS_PER_LINE
    );
    assert!(!out.warnings.is_empty());
}
