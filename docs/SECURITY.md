# Selah security and safety

Selah runs in front of a congregation, on church computers, often on shared Wi-Fi, operated by volunteers. A security bug can leak a church's data. A safety bug can put the wrong thing on a big screen, flash a strobe at a room full of people, or blast sound through a PA. This document sets out how Selah prevents both, **from the first milestone** (ADR-026).

Every requirement has an ID. Work packages, tests and PRs refer to these IDs. A feature that can put something on screen, make sound, open a network port, read files from outside, or store a secret isn't done until its SEC and SAFE items have passing tests.

To report a vulnerability, see [SECURITY.md](../SECURITY.md) in the repository root.

## 1. Principles

1. **Secure and safe by default.** Every risky feature (the API, web layers, speech cues, telemetry, network sources) starts **off**. Turning it on is a deliberate choice, and Selah explains what changes.
2. **Least privilege.** Selah never needs administrator rights to install or run. Each process and worker gets only what it needs.
3. **Everything from outside is untrusted.** Imported files, media, bundles, Content Packs, church articles, network messages and web pages are parsed defensively, with limits, in separate processes where possible.
4. **The screens are sacred.** Nothing reaches the audience screens or speakers without an authorised person choosing it. The emergency clear always works.
5. **Privacy by default.** Selah sends nothing over the network unless a person asks for it or turns on a feature that needs it. Audio from the microphone never leaves the computer.
6. **Fail safe.** When something breaks, Selah keeps the last good frame, stays quiet, and tells the operator in plain words what to do next.
7. **Proven, not promised.** Each requirement below has an automated test or a recorded human check.

## 2. What we protect

| Asset | Why it matters |
|---|---|
| What the room sees and hears | The audience screens, stage displays, streams and the PA. Mistakes are public and can harm people (flashing, loud sound). |
| The church's library | Songs, services, media, notes and settings: years of work. |
| Credentials | Stream keys, partner tokens (CCLI, Planning Center), API tokens, sign-in sessions. A leaked stream key lets anyone broadcast as the church. |
| Personal data | Team members' names and emails, accounts, audit logs, recordings, microphone audio. |
| The church's computer | Selah must never become a way into the machine or the church network. |
| The update channel | A tampered update would run on every church's computer. |
| The source code and CI | Especially while the repository is public (ADR-024). |

## 3. Threat model

| Boundary | Who or what is on the other side | Main threats | Controls |
|---|---|---|---|
| Local IPC between Selah's processes | Other programs and other users on the same computer | Another process injects commands or reads live state | SEC-02, SEC-14 |
| Files from outside | Imports, media, `.selah` bundles, Bibles, Content Packs, fonts, church articles | Crafted files crash, hang or exploit a parser; zip bombs; path traversal | SEC-03, SEC-04, SEC-15 |
| The local network | Phones and tablets on church Wi-Fi (often a guest network), control surfaces, NDI sources, other Selah computers | Someone on the Wi-Fi takes control or pushes content to the screens; token theft; flooding | SEC-07, SEC-14, SAFE-01, SAFE-02, SAFE-08 |
| Web content | Web pages and online video shown as layers | Malicious pages, drive-by downloads, access to local files | SEC-08, SAFE-02 |
| The internet | Partner APIs, the sync server, the update feed | Man-in-the-middle, tampered updates, data leaks | SEC-12, SEC-13, SEC-05 |
| Custom shaders and Content Packs | Third-party creators | GPU hangs, unsafe code, misleading content | SEC-03, SEC-15, SAFE-04 |
| The microphone | Speech cues | Private conversations recorded or sent away | SEC-13, SAFE-09 |
| People | Volunteers with the wrong role, accidental clicks, mischief | Wrong content on screen, settings changed mid-service | SEC-14, SAFE-01, SAFE-03, SAFE-06 |
| Build and CI | Strangers' pull requests and forks while public, compromised dependencies or actions | Stolen tokens, poisoned builds | SEC-09, SEC-10 |
| Installer and updater (M1) | The update feed and package host the person configured; Velopack's `Update.exe` running as the user | A tampered or swapped package; an update that interrupts a service; an installer that asks for admin | SEC-01, SEC-12, SEC-13, SAFE-06 |
| Support files (M1) | The public problem-report form where the person attaches a support bundle or a Diagnostics report | Secrets, names, email addresses or church data leaking into a public issue | SEC-05, SEC-06 |
| Library storage (M2.1) | Existing database pages, media paths and bytes on the user's disk | Corrupt or future databases reset without warning; partial writes; path traversal; damaged media reused | SEC-03, SEC-15: integrity checks, immutable migration history, bounded validation, transactional documents, content-addressed verified media |
| Song, Bible and bundle parsers (M2.4–M2.6) | User-pasted lyrics and ChordPro text, imported OSIS/USFM/Zefania Bible files, and `.selah` or backup `.zip` archives | Parser hangs, zip or entity bombs, deep XML nesting, path traversal (`..`, symlinks, drive letters), oversized payloads | SEC-03, SEC-15: byte, line, depth, entry-count and deflate-ratio bounds, XML DOCTYPE/entity refusal, path normalisation, canonical manifest checks, 5 fuzz targets |
| Media probe worker (M2.6) | Imported local audio, image and video files inspected for metadata and thumbnails | Crash or memory exhaustion in media decoders bringing down `selah-core` | SEC-01, SEC-03, SEC-04: isolated one-shot `selah-media-worker` subprocess, file/pipe FFmpeg whitelist, image pixel-count limit, BLAKE3 verification |
| Slide effects and template/element JSON (M3.2, M3.5, M3.7) | User or imported `TemplateStyle` (`style_json`), `SlideElementsDocument` (`elements_json`), and effect chain specs (`EffectSpec`) | Custom shader/WGSL injection, unbounded JSON/layer/element payloads, NaN/Inf numeric parameters | SEC-03, SEC-04: closed 6-effect `EffectKind` enum only, recursive rejection of `FORBIDDEN_SHADER_KEYS` (`shader`, `wgsl`, `glsl`, `script`, etc.), 64 KiB/128 KiB JSON and 8-stage / 64-element caps, bundled-font canonicalisation |
| Audio playback and master limiter (M3.4) | Local video/audio playback tracks rendered on the host audio device | Network playlist SSRF/protocol abuse, sudden loud spikes or pops through a church PA | SEC-04, SAFE-05: file/pipe protocol + demuxer/decoder allowlists, `-1.94 dB` default safe level, 4× oversampled BS.1770 `-1 dBTP` brickwall limiter, smooth 25 ms fade-out on clear |
| WCAG flash analyser (M3.5) | Imported videos, rapid transitions, and live composed frames | Unwarned strobe or saturated-red flashing exceeding WCAG 2.3.1 thresholds reaching audience screens | SAFE-04: offline probe scan + `Flashing` tag, pre-flight transition/media warning on fit reports, live frame monitoring (`FrameShown.flash_warning`), UI confirmation before staging flagged slides |
| Offscreen thumbnail and scene element loader (M3.6–M3.7) | Scene payloads (`RenderSceneThumbnail`) and `<media_root>` files referenced by backgrounds/elements | Symlink or path escape out of media/thumbnail directories, disk exhaustion, image dimension bombs, corrupt PNG cache | SEC-03, SEC-04, SEC-15: strict address/resolution/opacity validation, `resolve_verified_media_path` + BLAKE3 hash check, bounded 512-file / 64 MiB LRU thumbnail cache |
| Live tools and stage layouts (M4.3, M4.6, M4.9) | Looks, props, messages, timers, macros, clear groups and stage layouts that people make in **Set up tiles** and the **Stage** tab, or that arrive in a library | Oversized or malformed payloads; a macro that runs forever; message tokens that smuggle control characters; layouts that draw off screen | SEC-03, SEC-14: the UI checks names, keys, words, minutes and steps, then core checks again (32 look rules, 32 macro steps, 32 tokens with bounded keys and values, no NUL, rects inside the screen); a macro may not run a macro; the `Macro` origin may only do live actions |
| Role lock, booth lock and booth messages (M4.4, M4.5) | A volunteer's Simple-mode UI, someone at an unlocked booth, notes typed by volunteers | A UI that claims Producer rights; guessing the PIN; a booth note reaching the audience | SEC-14, SAFE-01, SAFE-06: the lock lives in core (policy `live.role-lock`), never in a client-chosen user id; salted BLAKE3 PIN hash in `SecretStore`; 5 wrong tries wait 30 s; refused commands are logged; booth messages (≤ 200 characters) go only to the producer's UI and stage screens |
| Saved Producer layout (M4.4) | `producer-layout.json` under the app data folder | A crafted or damaged file hides the live controls or crashes the UI | SEC-03: Selah's own small JSON (not `Dock.Serializer`), size-capped and clamped on load, ignored if bad; panes can't close or float |
| Practice mode (M4.7) | An operator rehearsing during the week, or by mistake on Sunday | Practice changes reaching the room or being saved over the real service | SAFE-01: practice scenes go to the engine's preview ring only; nothing is saved while practising; leaving restores the checkpoint; emergency clear still clears the real screens |

The threat model is reviewed at the end of every milestone (TESTING-AND-QUALITY §4.2). New boundaries get a row before their feature is merged. The full review with a penetration test is M10.6 (H-21).

**M1 review (2026-11-07).** M1 added two boundaries (installer/updater and support files). Residual risks: until signing (H-15), the update package is only as trustworthy as the feed it came from. HTTPS and the feed's SHA-256 stop tampering on the way, but not a compromised feed host, so feeds stay opt-in and set by the person. Support bundles are redacted by pattern and by registered secrets; free text that a person typed into a song or note could still name people, so the problem-report form tells them to check the file before attaching it.

**M2 review (2026-11-09).** M2 added library storage (`selah-core::library`), four song paste parsers, three Bible importers (`OSIS`, `USFM`, `Zefania`), bounded ZIP archive reading for `.selah` bundles and full library backups, an isolated `selah-media-worker` subprocess, 34-command `SEC-14` permission checks, and UI contextual help with `F12` live-safety bypass. Residual risks: `selah-media-worker` isolates decoder faults out of `selah-core` as a same-user subprocess without Windows job-object / macOS sandbox hardening yet (scheduled in M6); `.selah` bundles check BLAKE3 media hashes and strict schema rules rather than creator signatures (signed Content Packs arrive in M8); bundled/imported Bibles are operator-curated rather than cryptographically attested (`H-06`); and multi-user permissions remain local-owner only until roles arrive in M4/M7/M9.

**M3 review (2026-11-10).** M3 added multi-output rendering (`SetOutputConfigs`, `PresentScene`, `StagePreviewScene`, `PreloadItems`), role-slotted templates (`TemplateStyle`), Skia/HarfBuzz metric fit, background media & audio-synced looping playback with BS.1770 `-1 dBTP` limiting, 8 transitions, 6 built-in GPU/CPU effects, WCAG 2.3.1 flash monitoring (`AnalyzeFlashRisk`), bounded offscreen scene thumbnails (`RenderSceneThumbnail`), dual Program/Staged preview rings, custom slide elements (`SlideElementsDocument` v1), and extended `RunDiagnostics` (`selah-bench m3`). Residual risks: custom WGSL shader loading remains strictly disabled until the M8 sandbox and signed Content Packs arrive (`FORBIDDEN_SHADER_KEYS`); `MediaPlayer` in-process audio/video decode relies on the `SEC-04` protocol/demuxer/decoder allowlists rather than an out-of-process sandbox until worker isolation hardens in M6; WCAG flash analysis warns and blocks unacknowledged staging in the Slide Editor but does not censor operator-approved media; and hardware `8.0 ms` p99 frame-time compliance across reference GPUs awaits `H-23` sign-off in M10 (`selah-bench m3` tracks `hardware_pass = (p99 <= 8.0)` alongside a software-adapter CI ceiling).

**M4 review (2026-11-12).** M4 added services and print export, the live state machine with crash-recovery snapshots, looks, props, messages, timers, macros and clear groups (and the **Set up tiles** editor), Producer mode on Dock.Avalonia (MIT, pinned, ADR-002), Simple mode with the role lock and **Message the booth**, the stage display, Ready for Sunday, practice mode, first-run setup, tours and **Settings ▸ Church**. Residual risks: if the role lock is on but its PIN record is missing from the OS keychain, unlocking succeeds and is logged (fail-open, chosen so a church is never trapped in Simple mode; only someone who can already act as that OS user can remove the record). Booth lock keeps **Show logo** working with Next, Previous and Clear, because SAFE-03 groups it with the emergency actions; this is flagged for review. Print export writes only to a checked path (SEC-04), but the PDF writer is Selah's own and isn't fuzzed yet. Stage layouts and live tools in imported bundles get the same core checks as ones made in the app. Continuity (ADR-001): the M4.8 chaos run shows UI and core kills never change the screens, but an engine restart takes 1.7–2.1 s from kill to restored picture on the software adapter, at the edge of the 2 s budget; a faster restart needs a design change (see PROGRESS.md, M4-A5).

## 4. Security requirements

**M2.1 storage review (2026-11-09).** Database failures retain the original file.
Documents and ordered child rows commit together; media bytes are synced and
published before metadata can reference them. Hashes, extensions and canonical
paths are checked, and streams are bounded. A saved schema fixture tests future
migration compatibility. No new IPC commands or outgoing connections are added.
Residual risks: another process with the same user's filesystem permissions can
change an open file; hash checks detect corruption but do not provide isolation
from that process. Media decoding, bundle verification, backups and restore are
still M2.6 work; storage does not decode or display imported bytes.

| ID | Requirement | Verified by | Milestones |
|---|---|---|---|
| **SEC-01** | **Least privilege.** Selah installs per user and runs without administrator rights. Nothing runs as a service or with elevation. Workers (imports, speech) run with reduced rights: a Windows job object with limits and a low-integrity token, and a macOS sandbox profile from M6. | Installer test checks for no elevation; worker token tests | M1, M6 |
| **SEC-02** | **Local IPC is private to the user.** Named pipes have a DACL for the current user only and `PIPE_REJECT_REMOTE_CLIENTS`. The server checks the peer process. Unix sockets live in a `0700` directory. Frames over 16 MiB, unknown message types and bad lengths are rejected without crashing. | `selah-ipc` tests; IPC fuzz target | M1 |
| **SEC-03** | **Outside input is untrusted.** Parsers run in workers (ADR-020) with limits on size, depth, item count and time. Archive paths are normalised (no `..`, no absolute paths). Every parser and the IPC decoder has a fuzz target: a 30-second smoke run in CI, longer runs in cloud sessions. | Fuzz targets; limit tests | M1 (IPC), M2 (bundles, Bibles, ChordPro), M6 (importers), M8 (packs) |
| **SEC-04** | **Media decoding is limited.** FFmpeg opens local files with `protocol_whitelist=file,pipe`. Network protocols are allowed only for stream items the user added. Demuxers and decoders come from an allowlist. Images are decoded with a pixel-count limit. | Tests: an `http://` path in a file item is refused; an oversized image is refused | M1, M2, M3 |
| **SEC-05** | **Secrets live only in the OS keychain**, through one `SecretStore` interface (Windows Credential Manager with DPAPI, macOS Keychain, an in-memory fake for tests and Linux). Secrets never appear in SQLite, settings files, logs, the op log, support bundles, crash dumps or exports. | Seeded-secret tests scan every output | M1, M6, M8, M9 |
| **SEC-06** | **Logs and support bundles are redacted.** A `tracing` layer removes secrets, tokens, email addresses and the user's name in paths. Support bundles are built from redacted data only. Crash dumps stay on the computer unless the user chooses to send them. | Redaction tests with seeded data | M1, M6, M9 |
| **SEC-07** | **The gateway is safe on a shared network.** Off by default and LAN-only. TLS with a self-signed certificate pinned by the QR code. Pairing by QR. Scoped, expiring, revocable tokens. Rate limits, origin and CORS checks, request size limits. No default passwords. Every API action is recorded in the op log. | M7-A2; API fuzzing (M10.6) | M7, M10 |
| **SEC-08** | **Web content is sandboxed.** CEF runs with its sandbox on and its own profile. No file access, downloads, pop-ups or plugins. Navigation stays on the item's site. There's no script bridge into Selah. | CEF policy tests | M5 |
| **SEC-09** | **Supply chain.** Lockfiles are committed and enforced (`cargo --locked`, NuGet locked mode, `pnpm --frozen-lockfile`). `cargo deny` checks advisories, licences (no GPL) and sources. `dotnet list package --vulnerable` and `pnpm audit --prod` run in the gate. Third-party downloads are pinned by SHA-256 (`tools/third_party.lock.json`). Each release has a CycloneDX SBOM. Every new dependency is named and justified in the PR. | Gate stage 14 | M1, M9 (SBOM) |
| **SEC-10** | **CI is hardened** (ADR-024). Top-level `permissions: {}` and the least permission per job. Only GitHub-owned actions, pinned to full commit SHAs. `persist-credentials: false`. No `pull_request_target`, `workflow_run` or scheduled triggers. No secrets while the repository is public. Event text reaches scripts only through `env:`. | `tools/check-workflows.py` in the gate | M1 |
| **SEC-11** | **Memory safety.** Every Rust crate has `#![forbid(unsafe_code)]`, except the FFI crates and modules named in `tools/unsafe-allowlist.txt`, where each `unsafe` block has a `// SAFETY:` comment. C# projects don't allow unsafe blocks. | `tools/check-unsafe` in the gate | M1 |
| **SEC-12** | **Updates are authentic and never interrupt a service.** The updater downloads over HTTPS and checks each package's hash from the feed. Release builds are signed (H-15) and the updater checks the signature. No token or key is built into the app. Updates never install while anything is on the screens. The previous version can be restored. | Updater tests | M1 (hash, live deferral), M6 and M9 (signing) |
| **SEC-13** | **Privacy by default.** All outgoing network traffic goes through one gate (`selah-net`), which enforces policy and records each connection. With default settings, Selah makes **no** outgoing connections. Telemetry and crash upload are off until the user opts in. Speech audio never leaves the computer. People can export and delete their data. | Egress test (zero connections at default settings); M9-A6 | M1, M8, M9 |
| **SEC-14** | **Every command is authorised.** Each `Envelope` carries an `Actor`. Core checks a permission for every command before applying it. M1 has the framework (the local owner may do everything). Roles arrive in M4 (role lock), API scopes in M7 and organisation roles in M9. | Permission-matrix tests for every role × command | M1, M4, M7, M9 |
| **SEC-15** | **Data integrity.** Atomic writes, SQLite integrity checks, media verified by BLAKE3 hash, `.selah` bundles verified on import, Content Packs signed, and backups proven by restoring them. | Store and bundle tests | M2, M8 |

## 5. Safety requirements

| ID | Requirement | Verified by | Milestones |
|---|---|---|---|
| **SAFE-01** | **Only authorised people change the screens.** Every live change comes from an authorised actor (SEC-14). Remote devices need pairing and the right scope. Things sent from elsewhere (notes from the pastor view, **Message the booth**) go to the operator, never straight to the audience screens. | Permission and routing tests | M1, M4, M7 |
| **SAFE-02** | **Preview first.** Content from outside sources (web pages, NDI and camera inputs, speech suggestions, remote submissions, new imports) appears in preview first. It never goes to the screens automatically. | Tests for each source type | M5, M7, M8 |
| **SAFE-03** | **The emergency clear always works.** **Clear everything** and **Show logo** go from the UI straight to the engine through a priority lane, so they work even if core is frozen (ADR-026). F12 or holding Esc for 1 second triggers it. The engine acts on it within 1 frame. | Chaos test with core frozen | M1 (lane), M4 (UI) |
| **SAFE-04** | **Flash guard.** The engine checks output for general and red flashes above the WCAG 2.3.1 threshold (more than 3 flashes in any 1 second). Transitions, effects and videos that would exceed it are flagged in preview and in Ready for Sunday. Strobe-like effects ask for confirmation. | Analyser tests with known clips | M3, M8 |
| **SAFE-05** | **Safe sound.** A master limiter is on by default (−1 dBTP). Clearing audio fades instead of cutting. Volume changes are smoothed. A newly connected output starts at a safe level. | Audio tests with a null sink | M3, M5 |
| **SAFE-06** | **No surprises while live.** Quitting, signing out, changing screen settings or installing an update while something is on the screens asks first. The computer doesn't go to sleep while live. **Booth lock** (Ctrl/Cmd+L) leaves only Next, Previous and Clear working. | Tests | M1 (quit and update guards, sleep), M4 (Booth lock) |
| **SAFE-07** | **Safe mode.** After 3 crashes in 60 seconds, or if the user holds Shift while starting Selah, it starts with web layers, custom shaders, the gateway, network sources and hardware output drivers turned off. A banner says so and links to the fix article. | Watchdog tests | M1 |
| **SAFE-08** | **Approve new sources and devices.** A new NDI source, camera, MIDI device, remote device or network trigger must be approved once before it can appear on screens or control Selah. | Tests | M5, M7 |
| **SAFE-09** | **Capture is always visible.** A clear indicator shows while the camera, microphone (speech cues), recording or streaming is on. It uses an icon and a word, not colour alone. | UI tests | M5, M8 |

## 6. Rules for coding sessions

- **No secrets anywhere in the repository**, in tests, fixtures or CI. Use fakes and seeded fake values (for example `TEST-SECRET-1234`) that the redaction tests look for. `tools/check-secrets.py` runs in the gate.
- **No network in tests** (TESTING-AND-QUALITY §3, rule 9). The egress gate makes accidental calls fail the test.
- **Crypto:** use vetted libraries only (`rustls` with `aws-lc-rs` or `ring`, the OS keychain, BLAKE3 for content hashes). No home-made crypto. Random values come from the OS.
- **Errors** shown to users use catalogue codes and plain messages, never stack traces or internal paths.
- **Unsafe code** only in allowlisted FFI modules (SEC-11).
- **New dependency?** Check its licence and maintenance, run `cargo deny`, and say why it's needed in the PR description.
- **Limits everywhere** input is parsed: size, depth, count and time. Write the limit test with the parser.
- **Security and safety bugs** are P0 or P1. They block merging and get a failing test first (TESTING §3, rule 6).
- **PR descriptions** list the SEC and SAFE items the change touches and how they're tested (`.github/pull_request_template.md`).

## 7. While the repository is public

The repository is public for a limited time so that GitHub Actions minutes are free (ADR-024, [CLOUD-DEVELOPMENT.md](CLOUD-DEVELOPMENT.md)). During that time:

- Treat everything committed as **permanently public**. Forks and clones made while it's public stay public after it goes private.
- Never commit private notes, personal data, real church data, partner credentials or unlicensed material. Detailed competitive research stays outside the repository.
- Workflows use no secrets. Signing (H-15) and any secret-using job wait until the repository is private, or use OIDC with a protected environment.
- Pull requests from anyone other than the owner need approval before their workflows run.

## 8. Vulnerability response

1. Reports arrive privately through GitHub's private vulnerability reporting ([SECURITY.md](../SECURITY.md)).
2. Acknowledge within 7 days. Reproduce it with a failing test.
3. Fix in a PR marked as a security fix. Before v1.0, the fix ships in the next milestone build. After v1.0, a patch release goes to every update ring, and the release notes say what was fixed in plain words once users have had time to update.
4. If a release could harm churches (for example, a broken updater), pull it from the feed first, then fix it.

## 9. Where the M1–M3 controls live

| ID | Implementation | Tests |
|---|---|---|
| SEC-01 | Velopack per-user install (`tools/package.ps1`, `packaging/velopack/`); one-shot `selah-media-worker` subprocess isolates media probing from `selah-core` | `tools/test-installer.ps1`: no elevation in any manifest, no service, no machine-wide entry |
| SEC-02 | `selah-ipc` (pipe DACL, remote clients rejected, peer check, `0700` socket folder, 16 MiB limit) | `selah-ipc` tests |
| SEC-03 | Frame decoder, ChordPro, OSIS/USFM/Zefania Bible, and `.selah` bundle fuzz targets; update feeds, support bundles, song paste, Bible XML/USFM, search queries, ZIP archive readers, `TemplateStyle` (`64 KiB` / `32` variants), `SlideElementsDocument` (`128 KiB` / `64` elements), and `EffectSpec` chains (`8` stages / `16 KiB` params) enforce size, depth, count and finite-value bounds | `fuzz_smoke`, `songs_fuzz_smoke`, `scripture_fuzz_smoke`, `exchange_fuzz_smoke`, `sec03_parser_enforces_all_bounds_cleanly`, `m2_a5_osis_usfm_and_zefania_importers_parse_and_enforce_security_limits`, `m2_a11_crafted_zip_bundles_and_corrupt_backups_fail_closed`, `template::tests`, `effects::tests`, `slide_elements::tests`, `scene_thumbnail_rejects_malformed_and_unsafe_inputs` |
| SEC-04 | `selah-media` protocol whitelist (`file,pipe`), `DEMUXERS`/`VIDEO_DECODERS`/`AUDIO_DECODERS` allowlists, `MAX_DIMENSION` (8K), `MAX_IMAGE_FILE_BYTES` (128 MiB), symlink/URL refusal across `selah-media-worker`, `MediaPlayer`, and `media_bg::load_verified_image`; closed built-in 6-effect shader chain rejecting `FORBIDDEN_SHADER_KEYS` | `opening_an_http_url_is_refused`, `a_local_playlist_cant_reach_the_network`, `media_import_refuses_urls_protocols_and_dimension_bombs`, `background_image_loader_refuses_corrupt_hash_and_oversize_bomb`, `backgrounds_image_video_and_emergency_clear_end_to_end`, `selah-core/tests/flash_and_shaders.rs` |
| SEC-05 | `selah-secrets` (`SecretStore`: Credential Manager, Keychain, memory) | `values_round_trip_and_are_redacted_from_logs`, `the_platform_keychain_round_trips`, the support-bundle seeded-secret test |
| SEC-06 | `selah_runtime::redact` on every log line and every support-bundle / Diagnostics file | `seeded_secrets_and_identifiers_are_removed`, `redacting_writer_removes_seeded_secrets_before_writing`, `a_bundle_has_logs_and_versions_but_no_secrets_or_user_name`, `diagnostics_shows_the_engine_and_writes_a_benchmark_report` |
| SEC-09 | Lockfiles, `deny.toml`, pinned third-party files | Gate stage 14 |
| SEC-10 | `ci.yml`, `release.yml`, `bless-golden.yml` | `tools/check-workflows.py` |
| SEC-11 | `#![forbid(unsafe_code)]`, `tools/unsafe-allowlist.txt` | `tools/check-unsafe.py` |
| SEC-12 | `selah-core::update` (https feeds, size and SHA-256 checks, live deferral) | `a_package_with_the_wrong_hash_or_size_is_never_saved`, `updates_never_install_while_something_is_on_the_screens` |
| SEC-13 | `selah-net` (purposes, default-off policy, connection log); core sets the policy from settings | `the_default_policy_connects_to_nothing`, `only_selah_net_reaches_the_network`, `update_checks_are_off_at_default_settings`, `default_settings_make_no_outgoing_connections` (real processes) |
| SEC-14 | `selah-core::permissions` (all 38 M1–M3 commands, exhaustive) | `permission_matrix_covers_every_origin_and_command`, `unauthorized_library_commands_are_refused_before_touching_database` |
| SEC-15 | `selah-core::library` (SQLite STRICT/WAL/FULL, append-only checksummed migrations, transactional ops, BLAKE3 media blobs & verified background/thumbnail reads, canonical `.selah` bundles, full backup/restore) and `tests/fixtures/db/` | `selah-core/tests/library.rs`, `selah-core/tests/migrations.rs` (Stage 10), `selah-testkit::library_crash` (`M2-A3`), `selah-core/tests/media_exchange.rs` (`M2-A6`, `M2-A11`), `selah-core/tests/thumbnails.rs` |
| SAFE-01 | Same as SEC-14; only the local owner changes live screens; help overlays, Ctrl+K palette, offscreen thumbnails, and `StagePreviewScene` / `stage-0.ring` previews remain isolated from live outputs | `refused_commands_change_nothing`, `ContextualHelpTests::Whats_this_inspects_without_firing_live_actions_and_esc_dismisses`, `multi_output_layer_scenes_present_independently_and_clear_all_on_emergency` |
| SAFE-03 | Emergency lane: UI → engine pipe, preempts transition & video/audio playback ahead of rendering (`emergency_pending`); `F12` bypasses help search and What's this overlays | Emergency-lane chaos test (core frozen), `ContextualHelpTests::Live_safety_f12_bypasses_help_search_and_help_never_covers_live_controls`, `backgrounds_image_video_and_emergency_clear_end_to_end`, `transitions_effects_flash_guard_and_emergency_cancel_end_to_end` |
| SAFE-04 | `selah-render::flash::FlashAnalyzer` + `analyze_transition_risk` across `selah-media-worker` (`ImportMedia` tagging), `AnalyzeFlashRisk` / split reports, live engine `FrameShown.flash_warning`, and Slide Editor acknowledgement gate | `wcag_luminance_rate_and_area_thresholds_hold`, `transition_risk_catches_unsafe_strobe_and_passes_smooth_fade`, `selah-core/tests/flash_and_shaders.rs`, `selah-testkit/tests/transitions_effects_flash.rs`, `Slide_editor_flash_warning_guard_blocks_unconfirmed_stage` |
| SAFE-05 | `selah-media::audio::MasterAudioProcessor` (`DEFAULT_SAFE_VOLUME = 0.80`, ITU-R BS.1770 4× oversampled `-1 dBTP` brickwall limiter, smoothed gain changes, `25 ms` fade-out on clear/stop) | `selah-media::audio::tests` (`master_limiter_keeps_plus_6dbfs_sine_under_minus_1_dbtp`, `clear_fade_ramps_smoothly_to_silence`), `selah-media::player::tests::clear_fade_out_silences_and_finishes_player_cleanly`, `backgrounds_image_video_and_emergency_clear_end_to_end` |
| SAFE-06 | Quit, Diagnostics, and update guards in UI and core (`RunDiagnostics` and `InstallUpdate` refused while live); `selah_runtime::power` keeps the computer awake while live; staging isolation (`stage-0.ring`) | `LiveGuardTests`, `UpdateTests`, `the_computer_stays_awake_only_while_something_is_on_the_screens`, `multi_output_layer_scenes_present_independently_and_clear_all_on_emergency` |
| SAFE-07 | Watchdog safe mode (3 crashes in 60 s or Shift), banner, **Restart normally** | `three_crashes_in_a_minute_start_safe_mode`, `restart_normally_leaves_safe_mode_and_keeps_the_screens` |
