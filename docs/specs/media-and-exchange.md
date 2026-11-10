# Media bin, `.selah` bundles, and library backups (M2.6)

Selah imports local images, video, and audio through an isolated probe worker
(`selah-media-worker`), caches downscaled PNG thumbnails lazily inside the library
folder, exchanges songs/templates/media/presentations/tags in canonical `.selah` ZIP
bundles (`formats/selah-bundle.schema.json`), and creates/restores verified full-library
backups (`SEC-03`, `SEC-04`, `SEC-14`, `SEC-15`, `M2-A6`, `M2-A11`).

## Contracts and IPC operations

`proto/selah/v1/library.proto` and `envelope.proto` expose media and library exchange
over the command bus (`SEC-14`: UI/Tour actors only):

| Kind | Message | Reply / Event | Purpose |
|---|---|---|---|
| Command | `ImportMedia { source_path, name, tags }` | `MediaItem` + `LibraryChanged(MEDIA)` + `UndoStateChanged` | Probe local media via `selah-media-worker`, hash/store bytes in `media/`, cache thumbnail in `thumbnails/`, save undoable `MediaItem` |
| Query | `GetMediaThumbnail { media_id, max_width, max_height }` | `MediaThumbnail` | Return cached PNG thumbnail (or verify BLAKE3 blob and regenerate via worker) |
| Command | `ExportSelahBundle { output_path, song_ids, template_ids, media_ids, presentation_ids, tag_ids, include_media }` | `ExportedSelahBundle` | Export selected entities (or whole library) into a canonical deterministic `.selah` archive |
| Command | `ImportSelahBundle { source_path, raw_bytes, overwrite_existing }` | `ImportedSelahBundle` + grouped `LibraryChanged` + `UndoStateChanged` | Validate ZIP/JSON schema/blobs, then import all tags, media, templates, songs, and presentations in one transaction + undo group |
| Command | `CreateLibraryBackup { output_path }` | `LibraryBackupInfo` | Snapshot `library.db` online via SQLite backup API and bundle `library.db`, verified `media/` blobs, installed `bibles/*.db`, `.bundled-bibles.json`, and `backup-manifest.json` |
| Command | `RestoreLibraryBackup { backup_path }` | `RestoreLibraryBackupResult` | Verify ZIP, manifest SHA-256/BLAKE3 hashes, and SQLite integrity in staging, then swap with rollback on failure |

All M2.6 commands participate in command idempotency (`command_idempotency`), and
`ImportMedia` / `ImportSelahBundle` record reversible entries in `ops` and
`undo_entries`.

## Out-of-process media probing and thumbnails (`SEC-04`)

`selah-media-worker` (`crates/selah-media/src/bin/selah_media_worker.rs`) isolates
image and FFmpeg decoding from `selah-core`:

- **Pre-checks before worker launch:** Core rejects URLs (`http://`, `https://`,
  `rtsp://`, `file://`), FFmpeg protocol prefixes (`pipe:`, `concat:`, NTFS `:stream`),
  NUL bytes, symlinks, files exceeding `128 MiB` (for images) or `8 GiB` (`MediaStore`),
  and forged image headers (PNG `IHDR` CRC, BMP, GIF, JPEG `SOF`) exceeding
  `MAX_DIMENSION = 8192` or `MAX_PIXELS = 67,108,864`.
- **Worker invocation:** Spawns `selah-media-worker probe --input <canon-path>` with
  a `30 s` wall-clock timeout and `256 KiB` stdout/stderr bounds.
- **FFmpeg allowlists:** Opens files with `protocol_whitelist=file,pipe`,
  `format_whitelist` limited to `DEMUXERS`, and `codec_whitelist` limited to
  `VIDEO_DECODERS` and `AUDIO_DECODERS` (including decoded audio formats on all
  platforms via `tools/build-ffmpeg-macos.sh` on macOS). Audio files report
  duration/codec/container and yield no thumbnail (`SEL-LIB-0002` on
  `GetMediaThumbnail`).
- **Lazy thumbnail cache:** Thumbnails are written as PNGs to
  `library/thumbnails/<blake3_hex>_<W>x<H>.png` (`DEFAULT_THUMB_W = 320`,
  `DEFAULT_THUMB_H = 180`, clamped to `16..=1024`). `library/thumbnails/` is created
  only on demand, never during `Library::open`, and missing cache files regenerate
  transparently from BLAKE3-verified blobs on `GetMediaThumbnail`.

## Bounded ZIP reader and deterministic writer (`SEC-03`)

`selah_core::library::archive` enforces strict archive safety before exposing bytes or
persisting files:

| Profile | Max archive | Max entries | Max entry | Max total uncompressed |
|---|---|---|---|---|
| `.selah` bundle | 256 MiB | 8,300 | 128 MiB | 512 MiB |
| Library backup | 2 GiB | 65,000 | 1 GiB | 3 GiB |

Both profiles enforce:

- EOCD discovery with no trailing garbage after the EOCD comment.
- Stored (`0`) and Deflate (`8`) compression only; disallowed flags (encryption, data
  descriptors, patched/masked data) rejected.
- Exact Central Directory to Local File Header match (name, flags, method, CRC32,
  compressed size, uncompressed size), LFH span overlap detection, and streaming
  byte-count bounds (`deflate` overrun/zip-bomb protection) + CRC32 verification.
- Unix mode symlink rejection (`S_IFLNK`) and POSIX-safe path normalisation (no empty
  segments, `.`, `..`, leading `/`, `\`, `:`, control/NUL bytes, or case-insensitive
  duplicate names).
- `DeterministicZipWriter` emits canonical entry order with fixed DOS timestamps
  (`1980-01-01 00:00:00`, UTF-8 flag) so identical content yields byte-for-byte
  reproducible archives.

## Canonical `.selah` exchange bundles (`M2-A6`)

Schema: [formats/selah-bundle.schema.json](../../formats/selah-bundle.schema.json)
(Draft 2020-12, `additionalProperties: false` on every object, canonical UUIDv7 regex,
zero timestamp fields).

- **Deterministic ordering & canonical JSON:** Arrays of `songs`, `templates`,
  `media_items`, `presentations`, and `tags` are sorted by canonical lowercase
  hyphenated UUIDv7; `translations` maps use sorted `BTreeMap` keys; IEEE `-0.0` in
  `tempo` and `color` channels is normalised to `0.0`. Media blobs referenced by
  `include_media = true` are written sorted as `media/<blake3_hex>.<ext>` using
  `Stored` compression inside the ZIP.
- **Strict import validation:** Accepts either `source_path` or `raw_bytes`, rejects
  unreferenced archive entries (`manifest.json` + exact referenced `media/<hash>.<ext>`
  only), validates every UUIDv7, structural limit, colour range (`0.0..=1.0`), and
  entity cross-reference (`MediaItem` blobs must exist in archive or local
  `MediaStore` and match BLAKE3 + byte length; `Slide.media_id` and
  `Presentation.template_id` must exist locally or in the bundle).
- **Transaction & undo:** Entire import applies inside one SQLite `BEGIN IMMEDIATE`
  transaction and shared `undo_group` (`overwrite_existing = false` skips existing IDs
  without overwriting; `overwrite_existing = true` replaces existing records so one
  global `Undo` restores all prior state). Fuzz target `fuzz/fuzz_targets/selah_bundle.rs`
  and smoke suite `crates/selah-core/tests/exchange_fuzz_smoke.rs` exercise
  `fuzz_selah_bundle`.

## Full library backup and restore (`SEC-15`)

- **Backup:** Uses SQLite's online `rusqlite::backup::Backup` into a staged temp DB,
  closes and checks SHA-256 + `PRAGMA integrity_check` and `foreign_key_check`, hashes
  every verified `media/` blob, installed `bibles/*.db`, and optional
  `bibles/.bundled-bibles.json`, writes `backup-manifest.json` plus all verified
  entries into a ZIP archive (`library/backups/selah-backup-<unix_ms>.zip` when
  `output_path` is empty).
- **Restore:** Extracts and verifies `backup-manifest.json`, checks SHA-256/byte length
  and SQLite integrity/schema version (`<= CURRENT_SCHEMA_VERSION`) for `library.db` and
  all `bibles/*.db` databases plus BLAKE3 hashes for all `media/` blobs in a staging
  directory inside the library root. Once staging verifies cleanly, live `library.db`
  (plus WAL/SHM sidecars), `media/`, `bibles/`, and `thumbnails/` are backed up to a
  rollback sibling folder, replaced atomically, re-opened with `Library::open`, checked
  with `integrity_check`, and cleaned of any colliding `command_idempotency` primary
  key before recording the restore's own idempotency entry. Failures restore the prior
  live files untouched.

## Validation

- `crates/selah-render/src/png.rs`: IHDR dimension/CRC bomb rejection and thumbnail
  aspect-ratio tests.
- `crates/selah-media`: 9 unit tests (URL/protocol refusal, PNG bomb rejection, real
  image/video/WAV probing and thumbnail output).
- `crates/selah-core/tests/media_exchange.rs`: end-to-end worker import, thumbnail cache
  hit/regeneration/tamper checks, undo/redo, URL/protocol & PNG/BMP/GIF/JPEG bomb
  rejection, `.selah` export → import → re-export canonical JSON identity,
  overwrite + single-group undo, backup & restore round-trip, and 10 crafted ZIP /
  corrupt backup attack cases (`M2-A6`, `M2-A11`).
- `crates/selah-core/tests/exchange_fuzz_smoke.rs`: stable fuzz smoke run over seeded
  and mutated `.selah` bundles.
- `tests/tools/test_bundle_schema.py`: schema invariants and absence of timestamp fields.
