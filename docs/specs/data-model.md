# Library data model

M2.1 adds storage primitives in `selah-core::library`. Core owns the connection;
the UI must use the command bus. Storage methods are not IPC commands and do not
grant permission to a caller. M2.2 adds that boundary, operation logging and events.

## Contracts and identity

`library.proto` defines songs, sections, lyric lines with positioned chords,
translations, arrangements, templates with variants, media, presentations with
custom slides, and tags. `service.proto` defines service data, ordered items,
service types and teams. Live and planning commands remain in M4.

All document and child identities are RFC4122-variant UUIDv7 values, exactly 16
bytes in `Id.value`. `new_id()` uses the UUID crate's OS randomness and monotonic
v7 generator. Repositories reject missing and invalid IDs rather than inventing
IDs on receipt. Child IDs are unique in their table; arrangement entries may
repeat a section ID, and each entry must resolve inside its song.

Chords use Unicode character positions, including a position at the end of a
line. These are not UTF-8 byte positions or grapheme offsets. Translation lines
are keyed by language. Metric styles and custom slide elements arrive in M3;
their reserved JSON fields are opaque storage in M2.1.

## Folder and connection

`Library::open(root)` creates this layout:

```text
root/
  library.db
  media/<first two hash characters>/<hash>.<extension>
  bibles/
  packs/
  backups/
```

The host will choose the platform's default library folder when commands are
wired in M2.2. Passing a root is explicit in this storage API. Root paths are
canonicalised; existing database and child directories must stay within that
root. The library uses bundled SQLite (the same version as the KB), a 5-second
busy timeout, WAL, FULL synchronous writes, foreign keys and trusted-schema off.
No SQLite extensions are loaded. `integrity_check` and `foreign_key_check` run on
every open. Corrupt, unknown, incomplete and future schemas fail closed. The
original database is retained; there is no empty-library fallback.

## Schema and repositories

`library/migrations/001_library.sql` creates these STRICT tables:

| Tables | Purpose |
|---|---|
| `songs`, `song_sections`, `song_arrangements` | Song metadata, ordered sections and arrangements |
| `templates`, `template_variants` | Template name and ordered output variants |
| `media_items`, `media_tags` | Content hash, extension, metadata and tags |
| `presentations`, `slides` | Custom presentations and ordered slides |
| `services`, `service_items`, `service_types`, `teams` | Service data only |
| `tags` | Named reusable tags |
| `settings` | JSON values by key and policy level |
| `ops` | Sequence, UUID, timestamp, actor, origin, document, command, inverse, undo group and sync status |
| `editor_buffers`, `help_feedback` | Reserved for autosave and local help feedback |
| `schema_migrations` | Version and immutable migration checksum |

FTS5 `songs_fts` and `media_fts` are created with unicode61 and diacritic folding.
M2.3 supplies index maintenance, search and ranking. Future milestone tables
(looks, outputs, sync state and church articles) are added by future migrations.

Each document has its complete protobuf snapshot in `data`. Searchable metadata
has scalar columns. Ordered children also have protobuf snapshots with a parent
foreign key and unique parent/position. Repository saves update the aggregate
snapshot and all child rows in one transaction. A constraint error rolls back the
whole update, including the parent. Deletes cascade to child rows; media files
are retained for undo and deduplication. Repositories read aggregate snapshots,
validate them, and check their ID against the requested key.

The sealed `Document` trait permits typed save/get/list/delete for the eight
document types. Only a closed enum can supply a table name; values always use SQL
parameters. Pages have stable ID order and 1 to 200 entries. Encoded documents
are limited to 4 MiB; children, tags and arrangement entries to 4,096; languages
per section to 64; names to 512 characters. Tempo, section kinds, colours, chord
positions and template dimensions are validated before writes.

Cross-document content references are UUIDs in snapshots. Their missing-content
handling belongs to planning and live validation in M4. Storage validates their
identity format; it does not trigger or render them.

## Migrations

Migrations are append-only (ADR-027). `PRAGMA user_version` identifies the last
applied migration. On open, history must have a contiguous row for each applied
version and match the BLAKE3 hash of that version's checked-in SQL. SQL files use
LF on all platforms. Never edit a merged migration to change the schema.

A `BEGIN IMMEDIATE` transaction rechecks history, applies pending SQL, records
checksums and advances the version atomically. Two openers cannot apply the same
migration. Unversioned databases are accepted only if empty. Historical fixtures
must open and retain their original records in every later build.

`tests/fixtures/db/schema-v1.db` is the first storage-schema fixture, with invented
lyrics and fixed valid UUIDs. It is not the M2 completion fixture (`m2.db`), which
is saved at milestone completion. `cargo run -p selah-core --example
library_fixture -- <new-file.db>` creates a synthetic fixture and refuses to
overwrite one. Existing fixtures must never be regenerated to pass a test.

## Media storage (SEC-15)

The media store copies regular local files through a bounded stream, hashes each
byte with BLAKE3 and writes to a temporary file inside the store. The file is
synced and published without overwriting an existing object. Failed reads and
oversized inputs leave no partial object. Same-content imports reuse a verified
object; corrupt existing objects are reported rather than silently replaced.
File extensions are lower-case ASCII letters/digits of length 1 to 10. Hash
addresses are exactly 64 lower-case hex digits, so neither part can carry paths.

The storage hard limit is 8 GiB; a worker can impose a lower budget. Image pixel
limits, file type checks, probe metadata and thumbnails belong to the M2.6 import
worker. Raw stored bytes are not permission to decode media or show it.

Saving media metadata verifies that the blob exists, has the expected size and
matches its hash. `open_verified` checks an open handle, rewinds it and returns
that handle for reading, avoiding a second path lookup after verification.
On Unix, file and shard/root directories are synced for durable publication.
On Windows, files are synced before and after publication. A crash between
publishing bytes and metadata can leave an unreferenced object, which is safe to
retain. Garbage collection must account for undo and backups before deletion.

These checks protect against crafted paths and accidental corruption. They do
not isolate the library from malicious processes with this user's filesystem
permissions or stop another same-user writer changing an already open file.

## Settings policy

`settings` is separate from M1's `settings.json`; the existing appearance/update
settings continue to work. M2.2 will apply policy through authorised commands
before connecting it to settings UI. No secrets may enter this table.

Policy levels are user (0), machine (1) and organisation (2). With no lock, the
user value overrides the machine default, which overrides the organisation
default. A locked machine or organisation value overrides all unlocked values;
if both are locked the organisation wins. Reads return the value, source level
and lock state so the UI can explain the policy. User writes cannot create a
lock and are refused while policy locks a key. Removing a higher-level value
reveals the retained lower-level value. JSON values are limited to 64 KiB.

## Validation

`crates/selah-core/tests/library.rs` covers typed round trips, ordered children,
rollback on child conflicts, validation, pagination, cascading deletes, reopen,
historical migration, future/tampered/corrupt schema refusal, media corruption,
deduplication, size limits, path traversal, failed streams and policy precedence.
A Unix test additionally checks symlink escapes. No test uses real church data
or makes network connections. The full gate runs this integration suite.
