# Library and scripture search (M2.3)

Selah searches songs, media items and installed Bible translations inside core
(`selah-core::library::search`) and exposes bounded paginated queries over IPC.

## Contracts and IPC queries

`proto/selah/v1/library.proto` and `envelope.proto` define four queries:

| Query | Reply | Notes |
|---|---|---|
| `SearchSongs { query, limit, offset }` | `SongSearchResults { hits, total }` | Searches title, lyrics (and translations), authors, CCLI number and tags |
| `SearchMedia { query, kind, limit, offset }` | `MediaSearchResults { hits, total }` | Searches name and tags, with optional `MediaKind` filter |
| `SearchScripture { query, translation, limit, offset }` | `ScriptureSearchResults { hits, total }` | Searches one translation (`translation`) or all validated Bibles in `library/bibles/` |
| `SearchLibrary { query, scope, translation, limit }` | `LibrarySearchResults { songs, media, scripture }` | Combined fan-out scoped to `ALL`, `SONGS`, `MEDIA` or `SCRIPTURE` |

All search queries require `1 <= limit <= 200`, queries of at most 512 UTF-8 bytes and no NUL bytes.

## Schema, indexes and backfill

Migration `003_search.sql` (schema version `3`) adds:

- `songs_trigram` (`title`, `authors`, `ccli_number`, `tags`, `lyrics`, `tokenize = 'trigram'`), storing folded lowercase ASCII-equivalent text keyed by `songs.rowid`
- `media_trigram` (`name`, `tags`, `tokenize = 'trigram'`), keyed by `media_items.rowid`
- `search_synonyms (term, synonym)` (STRICT, bidirectional lookup across `(term, synonym)`)
- `search_meta (key, value)` (STRICT, tracks `index_version`)

Existing `songs_fts` and `media_fts` (`unicode61 remove_diacritics 2`) use `rowid` equality joins against `songs` and `media_items`. Writes (`Song`, `MediaItem`), deletes, `save_songs_batch`, and undo/redo compensating snapshots keep all FTS5 and trigram tables synchronised within the same SQLite transaction.

On `Library::open`, `ensure_search_ready` checks `search_meta['index_version']`. When upgrading older databases such as `tests/fixtures/db/schema-v1.db`, it idempotently backfills `songs_fts`, `songs_trigram`, `media_fts`, `media_trigram` and seeds default bidirectional domain synonyms (`xmas <-> christmas`, `resurrection <-> easter`, `calvary <-> cross`, `hallelujah <-> alleluia`, `charity <-> love`, `ghost <-> spirit`, `backdrop/wallpaper <-> background`, `photo/picture <-> image`, `clip/movie <-> video`, etc.).

## Normalisation, retrieval and ranking

1. **Folding and tokenisation:** `fold_text` folds Unicode case, typographic quotes/dashes and combining marks (`U+0300..=U+036F`, etc.) as well as precomposed Latin/Spanish/Portuguese/French/Yoruba diacritics (`Ọlọ́run` -> `olorun`). Tokenisation never removes short English words (`I`, `How`, `Is`, `My`, `Was`, `Be`), so short worship lyric and title queries match faithfully.
2. **FTS5 injection safety:** Every token is double-quoted (`"tok"*` for length >= 3, `"tok"` for length 1..=2) with internal `"` stripped, so raw `AND`, `OR`, `NOT`, `:`, `*` or unbalanced quotes are searched safely as literals.
3. **Multi-stage candidate retrieval:**
   - Stage 1: strict FTS5 `AND` query across token+synonym groups.
   - Stage 2 (if strict hits are fewer than `max(offset + limit, 10)`): partial FTS5 `OR` plus `songs_trigram` on metadata `{title authors ccli_number tags}` (and folded lyric trigrams only when metadata/FTS does not already satisfy all tokens).
   - Stage 3 (last-resort short-transpose fallback): if verified candidates remain empty and the query has a 3..=8-character token whose transposition shares zero trigrams (e.g. `grcae`), scan metadata rows using restricted Damerau-Levenshtein distance.
4. **Scoring and lazy protobuf decoding:** Candidates are filtered and ranked using field weights (`title` `10.0`, `ccli_number` `8.0`, `tags` `5.0`, `authors` `4.0`, `lyrics` `3.0`), token coverage, exact/prefix/substring phrase bonuses, title conciseness and FTS5 BM25 tie-breaking. `Song::decode` runs only for the paginated `offset..offset+limit` winners, extracting `matched_fields` and a matching lyric/metadata snippet.

## Bible full-text indexes and security

Each per-translation database in `library/bibles/<code>.db` (prepared via `write_bible_database` in M2.3 and populated with KJV/WEB + importers in M2.5) stores `bible_meta`, `books`, `verses`, `verses_fts` (`unicode61 remove_diacritics 2`) and `verses_trigram` (`trigram`). Translation codes are restricted to `[A-Za-z0-9_-]{1,32}`. Opening Bible files enforces regular non-symlink files canonicalised inside `library/bibles/`, opens read-only with `PRAGMA trusted_schema = OFF`, and verifies `schema_version = 1`.

## Performance validation

`selah-bench search --check` populates a 10,000-song library via `save_songs_batch`, verifies `"amzing grace"` returns `"Amazing Grace"` first alongside synonym, CCLI, author, lyric-typo and Yoruba diacritic queries, and enforces **p95 <= 50 ms** (`ARCHITECTURE §12`, `M2-A4`). Stage 6 of `tools/verify` runs this benchmark on every verification pass.
