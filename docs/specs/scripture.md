# Scripture, bundled Bibles, importers and reference parsing (M2.5)

Selah bundles two verified public-domain English Bibles (KJV and WEB, constrained by
`H-06`), parses multilingual scripture references across six languages, imports
user-supplied OSIS, USFM, and Zefania XML translations inside bounded parsers
(`SEC-03`), and splits verses into slides (`M2-A5`).

## Contracts and IPC operations

`proto/selah/v1/library.proto` and `envelope.proto` expose scripture operations over
the IPC bus:

| Kind | Message | Reply / Event | Purpose |
|---|---|---|---|
| Command | `ImportBible` | `BibleTranslation` + `LibraryChanged(BIBLE)` | Parse OSIS/USFM/Zefania XML from file or bytes and write `library/bibles/<CODE>.db` |
| Command | `DeleteBible` | `acknowledged = true` + `LibraryChanged(BIBLE)` | Remove a Bible SQLite database (and sidecars) from `library/bibles/` |
| Query | `ListBibles` | `BibleList` | Enumerate installed translations and optional 66-book chapter/verse tables |
| Query | `ParseScriptureReference` | `ParseScriptureReferenceReply` | Parse multilingual references into canonical `ScriptureSpan` values |
| Query | `GetScripturePassages` | `ScripturePassages` | Resolve reference text or spans against a translation (`MAX_PASSAGE_VERSES = 500`) |
| Query | `SplitScriptureSlides` | `GeneratedScriptureSlides` | Split verses across slides with word wrapping, numbering, header/footer references |

`ImportBible` and `DeleteBible` require `ActorOrigin::Ui | ActorOrigin::Tour`
(`selah-core::permissions`, `SEC-14`).

## Bundled public-domain Bibles (H-06)

- [content/bibles/kjv.tsv.gz](../../content/bibles/kjv.tsv.gz) — King James Version
  (`31,102` verses across 66 books), verified at rest by SHA-256.
- [content/bibles/web.tsv.gz](../../content/bibles/web.tsv.gz) — World English Bible
  (`31,098` verses across 66 books), verified at rest by SHA-256.
- Provenance details live in [content/bibles/PROVENANCE.md](../../content/bibles/PROVENANCE.md).

`selah-core-host` spawns background materialisation via `CoreService::ensure_bundled_bibles()`
on startup so IPC startup latency (`M1-A3` `< 2 s`) stays unaffected. Commands and
queries touching `library/bibles/` (`ListBibles`, `GetScripturePassages`,
`ImportBible`, `DeleteBible`, `SearchScripture`, `SearchLibrary`) wait on that
one-shot signal if still running. Seeded translations are recorded in
`library/bibles/.bundled-bibles.json` so user-deleted bundled Bibles are never
overwritten or resurrected on restart.

Commercial licensed translations (NIV, ESV, NLT, CSB, NKJV) remain gated on `H-06`
(`M2-A10`).

## Six-language reference parser

`selah_core::scripture::parse_reference(raw, language_hint)` resolves canonical books
and book/chapter/verse ranges across **English (`en`)**, **Spanish (`es`)**,
**Portuguese (`pt`)**, **French (`fr`)**, **Korean (`ko`)**, and **Yoruba (`yo`)**:

- Two-tier book index (`BOOKS`, 66 Protestant canonical books): exact lowercase+ordinal
  lookup first (e.g. preserving Portuguese `Jó` vs `Jo`), then diacritic-folded
  fallback via `selah_core::library::fold_text`.
- Ordinal and Roman prefix normalisation (`1st`, `1ª`, `1º`, `1re`, `I`/`II`/`III`).
- Space-separated verses (`jn 3 16-18`), European commas (`Jn 3,16`, `Jn 3, 16-18`,
  `Jn 3,16, 18-19`), discontiguous lists (`Rom 8:28, 31-32, 38-39`), whole chapters
  (`Ps 23`, `시편 23편`), chapter ranges (`Gen 1-2`), cross-chapter ranges (`Gen 1:26-2:3`),
  single-chapter books (`OBA`, `PHM`, `2JN`, `3JN`, `JUD`), Korean `장`/`편`/`절`
  markers, semicolon-separated multi-book clauses, and fullwidth/en-dash normalisation.
- Bounded by `MAX_REFERENCE_BYTES = 512`, `MAX_REFERENCE_SPANS = 32`, and NUL refusal.

## Bounded importers (`SEC-03`, `SEC-15`)

`selah_core::scripture::import` auto-detects or explicitly selects **OSIS**, **USFM**,
or **Zefania XML**, refusing symlinks when reading files and writing atomically via
`temp-file -> fsync -> persist` into `library/bibles/<CODE>.db` (with both `verses_fts`
and `verses_trigram` FTS5 tables):

- `MAX_BIBLE_FILE_BYTES = 32 MiB`
- `MAX_XML_DEPTH = 32` (enforced via `quick-xml` 0.41 without DTD/entity expansion;
  `Event::DocType` is immediately rejected)
- `MAX_XML_EVENTS = 2,000,000`
- `MAX_VERSES_PER_BIBLE = 50,000`
- `MAX_VERSE_TEXT_BYTES = 16 KiB`
- NUL bytes rejected; paratext/notes (`<note>`, `<NOTE>`, `\f...\f*`, `\x...\x*`,
  Strong's/morphology metadata after `|` in `\w...|...\w*`) stripped from verse text.

Coverage-guided fuzz targets `bible_osis`, `bible_usfm`, and `bible_zefania` (plus
`bible_reference` smoke coverage in `crates/selah-core/tests/scripture_fuzz_smoke.rs`)
exercise untrusted inputs.

## Verse-to-slide splitting

`selah_core::scripture::split_scripture_slides` wraps along whitespace (and Unicode
scalar boundaries for overlong tokens), respects `one_verse_per_slide`,
`verses_per_slide`, `max_lines` (`1..=100`, default `4`), `max_chars_per_line`
(`16..=200`, default `48`), `show_verse_numbers`, optional `title_slide` and trailing
blank slide, and `ReferencePlacement` (`None`, `SlideHeader`, `SlideFooter`) showing
only the verses on each slide.
