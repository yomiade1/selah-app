# Songs, chord transposition and slide generation (M2.4)

Selah implements clean-room song parsing, chord analysis/transposition, section
defaults, and rule-based slide splitting inside `selah-core::songs`, exposed to
the UI and external tools over IPC queries.

## Contracts and IPC queries

`proto/selah/v1/library.proto` and `envelope.proto` define three read-only queries:

| Query | Reply | Purpose |
|---|---|---|
| `ParsePastedSong { raw_text, format, default_title }` | `ParsePastedSongReply { song, detected_format, warnings }` | Auto-detect or explicitly parse pasted lyrics/chords into a validated `Song` |
| `TransposeSong { song, semitones, accidental_spelling }` | `Song` | Transpose `key` and every chord in `-120..=120` semitones using `Auto` (key-aware), `Sharps`, or `Flats` |
| `SplitSongSlides { song, arrangement_id, max_lines, copyright_placement, title_slide, blank_slide_at_end, translation_language }` | `GeneratedSongSlides { slides, copyright_line }` | Break sections by `max_lines` (`1..=100`), apply copyright footer rules, and align translations |

None of these queries mutate `library.db` directly; callers persist edits via
`SaveSong` commands through the undoable command bus.

## Four paste formats

1. **Plain (`SONG_PASTE_FORMAT_PLAIN`):** Paragraphs separated by blank lines become
   numbered verses (`Verse 1`, `Verse 2`, ...). Stanzas appearing two or more times
   with identical normalised lyrics are collapsed into `Chorus` (`Chorus`, `Chorus 2`...)
   and referenced in their original sequence inside the default `Arrangement`.
2. **Markers (`SONG_PASTE_FORMAT_MARKERS`):** Recognises standalone bracketed and
   colon-suffixed headers (`[Verse 1]`, `Chorus:`, `Pre-Chorus`, `Bridge`, `Tag`,
   `Coda`, `Intro`, `Outro`, `Ending`, `Interlude`, `Instrumental`) as well as bare
   standard labels (`Verse 1`, `Chorus`, `Bridge`, `Tag`, `Intro`, `Outro`). Empty or
   verbatim-repeated `Chorus` sections append the existing chorus section ID to the
   arrangement rather than duplicating sections.
3. **Chords-over-lyrics (`SONG_PASTE_FORMAT_CHORDS_OVER_LYRICS`):** Identifies chord
   lines above lyrics (or standalone instrumental chord lines separated by barlines
   like `| G  C | D  G |`). Column offsets are recorded as **Unicode scalar indices**
   (matching `LyricLine.text.chars().count()`), expanding leading/inline `\t` tabs to
   four spaces. Single isolated `"A"` or `"Am"` lines remain prose in Plain mode
   unless at least one unambiguous strong chord line exists in the paste.
4. **ChordPro (`SONG_PASTE_FORMAT_CHORD_PRO`):** Supports `{title}`/`{t}`,
   `{artist}`/`{author}`/`{composer}`/`{lyricist}`, `{key}`, `{tempo}`, `{ccli}`,
   `{copyright}`, `{tags}`, section blocks (`{sov}`..`{eov}`, `{soc}`..`{eoc}`,
   `{sob}`..`{eob}`, `{sop}`..`{eop}`), `{comment: <section>}`, tab skip blocks
   (`{sot}`..`{eot}`), `#` comment lines, and inline `[Chord]` tags placed directly
   before Unicode lyric scalars. Literal `[` characters that do not form valid chords
   are preserved in the lyric text.

## Security limits (SEC-03)

All paste parsers validate bounds before constructing protobuf structures:

- `MAX_PASTE_BYTES = 524,288` (512 KiB)
- `MAX_PASTE_LINES = 4,096`
- `MAX_LINE_BYTES = 4,096`
- `MAX_SECTIONS = 256`
- `MAX_LINES_PER_SECTION = 512`
- `MAX_CHORDS_PER_LINE = 64` (excess chords emit a bounded warning instead of allocating)
- `MAX_DIRECTIVE_LEN = 1,024`
- `MAX_BLOCK_DEPTH = 4`
- NUL bytes (`\0`) are rejected before line normalisation.

Coverage-guided fuzz target `fuzz/fuzz_targets/chordpro.rs` and stable smoke suite
`crates/selah-core/tests/songs_fuzz_smoke.rs` (`SELAH_FUZZ_SECONDS`) exercise
`selah_core::songs::fuzz_chordpro`.

## Chords, transposition, section colours and hotkeys

- **Chord grammar (`songs::chords`):** Accepts roots `A..=G` with `#`/`b`/`♯`/`♭`,
  qualities (`m`, `min`, `maj`, `dim`, `aug`, `sus`, `add`, `°`, `ø`, `Δ`),
  extensions (`2`, `4`, `5`, `6`, `7`, `9`, `11`, `13`, `6/9`), up to four alterations
  or parenthesised alterations (`7(#9)`, `m7b5`, `7alt`), and optional `/Bass` note.
  Ambiguous English words (`Do`, `Amazing`, `Grace`, `M`, `God`) are rejected.
- **Transposition:** Shifts root and slash bass modulo 12 in `-120..=120` semitones.
  When `AccidentalSpelling::Unspecified`, spelling follows the target key signature
  (flats for `F`, `Bb`, `Eb`, `Ab`, `Db`, `Gb` major and `Dm`, `Gm`, `Cm`, `Fm`,
  `Bbm`, `Ebm`, `Abm` minor; sharps otherwise).
- **Section colours (`DESIGN-SYSTEM §3`):** Verse `#2378C3`, Chorus `#C84281`,
  Bridge `#E66F0E`, Tag `#009EC1`, Intro/Outro/Pre-Chorus/Other `#71767A`.
- **Default hotkeys:** Verse `1`–`9`, Chorus `C`, Bridge `B`, Tag `T`, Pre-Chorus `P`,
  Intro `I`, Outro `O`, Ending `E`, Interlude `N`, falling back to unused alphanumerics
  and reserving `F9`–`F12` exclusively for live emergency actions.

## Rule-based slide generation

`songs::split::generate_slides` resolves the requested `Arrangement` (or default
sections in order), optionally prepends a `title_slide`, breaks **only at section
boundaries** (never merging distinct sections), splits each section into chunks of at
most `max_lines` (`1..=100`, default `4`), tracks `part_index` / `part_count`, copies
aligned per-line chords and matching `translation_lines[idx]`, applies
`CopyrightPlacement` (`None`, `First`, `Last`, `FirstAndLast`, `All`) across
non-blank slides using `"Title" · Authors · Copyright · CCLI #...`, and optionally
appends a blank trailing slide. Metric font fitting and overflow reports join in M3.

## Validation

- `tests/fixtures/songs/` holds 25 curated worldwide public-domain hymns (see
  `PROVENANCE.md`), each represented in `plain.txt`, `markers.txt`, `chords.txt`,
  `song.cho`, and `expected.json`.
- `crates/selah-core/tests/songs.rs` verifies 100% exact format detection, section
  extraction, colours, hotkeys, arrangements, lyrics, aligned Unicode chord
  positions, metadata, and SQLite persistence across all 100 fixtures (`M2-A1`), plus
  multilingual UTF-8 offsets, transposition cycles, slide splitting, and SEC-03 limits.
