# Selah progress ledger

This file is the single source of truth for **what is done and what comes next**. Every coding session reads it first and updates it as work completes (see the protocol in [CLAUDE.md](CLAUDE.md)). Tick a box only when its evidence is real: name the test, command or document that proves it.

Status values: `Not started` · `In progress` · `Blocked` (say on what) · `Done` · `Done, H-checks pending`.

## Current status

| Milestone | Status | Branch / tag | Started | Finished | Notes |
|---|---|---|---|---|---|
| M1. Foundations and risk spikes | In progress | `codex/m1-foundations` | 2026-11-07 | — | M1.1–M1.5 done (local Windows session, H-01). |
| M2. Library, songs, scripture and the help system | Not started | — | — | — | |
| M3. Render engine, templates and the slide editor | Not started | — | — | — | |
| M4. Live operation, services and stage | Not started | — | — | — | |
| M5. Screens, video and audio I/O | Not started | — | — | — | |
| M6. Importers, migration and the alpha release | Not started | — | — | — | |
| M7. Remote control, API and show control | Not started | — | — | — | |
| M8. Broadcast, creative and intelligence | Not started | — | — | — | |
| M9. Cloud, teams and enterprise | Not started | — | — | — | |
| M10. Launch readiness and v1.0 | Not started | — | — | — | |

**Next action:** continue M1 at M1.6 (text spike). See the resume notes.

## Resume notes

_When a session stops partway through a milestone, write here: the milestone, the last finished WP, anything half-done (files, branch state) and the exact next step. Clear it when the milestone is done._

2026-11-07 (local Windows session, H-01 approved): M1.1–M1.4 are committed on
`codex/m1-foundations`, and M1.5 too. No PR is open yet; the branch is pushed.

- `tools/verify.sh` / `verify.ps1` don't exist yet (M1.10), so WP evidence lists the gate
  stages run by hand: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`,
  `cargo test --workspace`, `cargo check` for x86_64-unknown-linux-gnu and aarch64-apple-darwin,
  `dotnet test app/Selah.sln`, `pnpm -r build/test`, `buf lint`, `buf format`, `cargo deny check`,
  `tools/check-unsafe.py`, `tools/check-secrets.py`. `tests/ledger.json` starts in M1.10.
- The Linux gate hasn't run yet: run it in an `ubuntu:24.04` container (Docker Desktop is
  installed here) or rely on the PR's `linux` job.
- LLVM/libclang (needed by FFmpeg bindings, M1.7) is not installed: a `winget install LLVM.LLVM`
  from the earlier session was still waiting, most likely for an administrator (UAC) prompt.
- The repository-local git email is now the GitHub no-reply address.
- Next: M1.6 (Skia text spike). `design/tokens/tokens.json`, `design/tokens/build.mjs` and the
  DESIGN-SYSTEM §3 focus/primary-fill change are prepared for M1.8 but not committed yet.

## Milestone checklists

### M1. Foundations and risk spikes

**Work packages**

- [x] **M1.1** Repository, toolchain and conventions — evidence: commit `M1.1`; `tools/setup.ps1 -CheckOnly` passes (Rust 1.91.0, .NET 10, Node 22, pnpm 10.20; LLVM pending); `python tools/third-party.py --extract` verifies every pinned file; `tools/generate-media.py` made and decoded 4 clips; `python -m unittest discover -s tests/tools` (20 tests); `cargo deny check` ok; lockfiles committed
- [x] **M1.2** Contracts and code generation — evidence: commit `M1.2`; `buf lint` and `buf format --exit-code` clean; `tools/codegen.ps1` regenerates TS/Rust/C#; shared wire fixture decoded by `selah-proto` tests, `ContractTests` (C#) and `web/contracts` tests
- [x] **M1.3** IPC layer — evidence: commit `M1.3`; `selah-ipc` tests (`ten_thousand_round_trips_keep_correlation`, `malformed_and_oversized_frames_close_only_that_connection`, `client_reconnects_after_the_server_restarts`, `pipe_dacl_allows_only_the_current_user`, `a_second_server_cannot_take_over_a_live_endpoint`, ring seqlock tests), `fuzz_smoke`, C# `IpcClientTests`/`PreviewRingReaderTests`; `selah-bench ipc --check`: named pipe p99 0.096 ms
- [x] **M1.4** Processes and watchdog — evidence: commit `M1.4`; `selah-testkit` continuity tests (engine, core and UI kills recover in < 2 s; 3 crashes → safe mode; single instance) pass 3 runs in a row; redaction tests (SEC-06); permission matrix (SEC-14); settings/live-state tests
- [x] **M1.5** Engine windows, compositor and test patterns — evidence: commit `M1.5`; `selah-render` `gpu_test_patterns_match_the_cpu_reference_exactly` (WARP; exact match at 1920×1080, 1366×768, 17×9); `selah-testkit` `show_test_pattern_reaches_the_output_within_one_frame` (FrameShown ≤ 1 frame after the command, bars checksum, preview ring); windowed run on an NVIDIA MX350 (DX12) opened the output window (Bgra8Unorm)
- [ ] **M1.6** Text spike — evidence: 
- [ ] **M1.7** Video spike — evidence: 
- [ ] **M1.8** UI shell, design tokens and text size — evidence: 
- [ ] **M1.9** Knowledge base pipeline — evidence: 
- [ ] **M1.10** Quality gate and CI — evidence: 
- [ ] **M1.11** Windows installer and updates — evidence: 
- [ ] **M1.12** Security and safety baseline — evidence: 
- [ ] **M1.13** Problem reports, diagnostics and spike report — evidence: 

**Acceptance criteria**

- [ ] **M1-A1** (auto) `tools/verify.sh --cloud` exits 0 in a cloud session. — evidence: 
- [ ] **M1-A2** (auto) Starting `selah` launches core, engine and UI. — evidence: 
- [ ] **M1-A3** (auto) If the UI is killed, the output frame checksum doesn't change, and the UI is relaunched and reconnected within 2 s. — evidence: 
- [ ] **M1-A4** (auto) If the engine is killed, it's restarted within 2 s and the previous scene is restored. — evidence: 
- [ ] **M1-A5** (auto) If core is killed, the engine keeps the last frame; core restarts and resynchronises within 2 s. — evidence: 
- [ ] **M1-A6** (auto) Golden text frames for en, yo, ko and ar pass on lavapipe. — evidence: 
- [ ] **M1-A7** (auto) The 1080p H.264 clip decodes and composites under text with frames in order and no drops on a 60 fps virtual clock. — evidence: 
- [ ] **M1-A8** (auto) (ci-win) The IPC p99 round trip is ≤ 2 ms on Unix sockets and on named pipes (bench results recorded). — evidence: 
- [ ] **M1-A9** (auto) UI tests pass at 100/150/200%, Light and Booth, `en` and `en-XA`, with no clipped critical labels. — evidence: 
- [ ] **M1-A10** (auto) The 3 exemplar articles pass `selah-kb-lint`; each bad fixture fails with its expected rule ID; `kb.db` builds; an article renders in the UI. — evidence: 
- [ ] **M1-A11** The spike report is written with measurements and a go/no-go. — evidence: 
- [ ] **M1-A12** (ci-win) The `windows` job builds `Selah-win-x64-Setup.exe`, and `tools/test-installer.ps1` installs it silently, sees "Connected" and the test pattern, and un… — evidence: 
- [ ] **M1-A13** (auto) Security baseline tests pass: IPC access and limits (SEC-02), the IPC fuzz smoke run (SEC-03), FFmpeg refusing network URLs in file items (SEC-04), seed… — evidence: 
- [ ] **M1-A14** (auto) Safety baseline: with core frozen, **Clear everything** clears every output within 1 frame (SAFE-03); 3 crashes in 60 seconds start safe mode with its b… — evidence: 
- [ ] **M1-A15** (auto) **Report a problem** produces a bundle that has logs and versions but no seeded secrets and no user name. — evidence: 
- [ ] **M1-A16** (auto) The session opened its PR with `tools/open-pr.sh`, the description follows the template, and `tools/ci.sh status` reads the PR's CI results. — evidence: 

### M2. Library, songs, scripture and the help system

**Work packages**

- [ ] **M2.1** Data model, store and migrations — evidence: 
- [ ] **M2.2** Command bus, op log, undo/redo and autosave — evidence: 
- [ ] **M2.3** Search — evidence: 
- [ ] **M2.4** Songs: paste-to-sections, chords and slide splitting — evidence: 
- [ ] **M2.5** Scripture — evidence: 
- [ ] **M2.6** Media bin and library exchange — evidence: 
- [ ] **M2.7** Library and song editor UI — evidence: 
- [ ] **M2.8** Contextual help components — evidence: 
- [ ] **M2.9** Knowledge base content for M2 — evidence: 

**Acceptance criteria**

- [ ] **M2-A1** (auto) The 25 curated hymns, pasted in 4 formats (plain, markers, chords over lyrics, ChordPro), produce exactly the expected section JSON (100%). — evidence: 
- [ ] **M2-A2** (auto) Undo/redo property test: 10,000 random op sequences. — evidence: 
- [ ] **M2-A3** (auto) Killing core during 1,000 writes leaves `PRAGMA integrity_check` = ok, loses no committed op, and the unsaved editor buffer is restored. — evidence: 
- [ ] **M2-A4** (auto) Song search p95 ≤ 50 ms on 10,000 songs. "amzing grace" finds "Amazing Grace"; synonym tests pass. — evidence: 
- [ ] **M2-A5** (auto) At least 300 scripture-reference test cases pass across 6 languages; OSIS, USFM and Zefania samples import correctly. — evidence: 
- [ ] **M2-A6** (auto) `.selah` export → import → export produces identical canonical JSON; backup and restore round trip. — evidence: 
- [ ] **M2-A7** (auto) Headless UI flows pass at 100% and 200% in both appearances: paste a song; edit a section; add scripture; import media; Ctrl+K finds a song, a command a… — evidence: 
- [ ] **M2-A8** (auto) Help coverage tests pass. — evidence: 
- [ ] **M2-A9** (auto) `tests/fixtures/db/m2.db` is saved, and every M1 test still passes (ledger). — evidence: 
- [ ] **M2-A10** (H) H-06 Bible rights are confirmed before more translations are bundled. — evidence: 
- [ ] **M2-A11** (auto) Every M2 parser passes its fuzz smoke run; a crafted bundle with `..` paths or oversized entries is refused; every new command is in the permission matr… — evidence: 

### M3. Render engine, templates and the slide editor

**Work packages**

- [ ] **M3.1** Scene graph, layers and frame scheduler — evidence: 
- [ ] **M3.2** Templates and text styling — evidence: 
- [ ] **M3.3** Metric fit: words that fit every screen — evidence: 
- [ ] **M3.4** Backgrounds and media playback — evidence: 
- [ ] **M3.5** Transitions and effects foundation — evidence: 
- [ ] **M3.6** Thumbnails and preview frames — evidence: 
- [ ] **M3.7** Template editor and slide editor UI — evidence: 
- [ ] **M3.8** Golden-frame suite and knowledge base — evidence: 

**Acceptance criteria**

- [ ] **M3-A1** (auto) Golden frames pass for every template style feature, transition step and effect. — evidence: 
- [ ] **M3-A2** (auto) Fit property test: 5,000 random lyric sets × 3 outputs (16:9, 32:9 LED, 4:3). — evidence: 
- [ ] **M3-A3** (auto) Thumbnails match the downscaled real output (pixel difference under the threshold). — evidence: 
- [ ] **M3-A4** (auto) A 1080p loop has no frame gap at the loop point, and A/V sync is within ±1 frame (flash/beep test clip). — evidence: 
- [ ] **M3-A5** (auto) Slide editor UI flow: create a custom slide with text, an image and a shape, align them, undo and redo. — evidence: 
- [ ] **M3-A6** (auto) (ci-win) Bench recorded in the cloud and the Windows job: 2 × 1080p outputs with a 4K video background, p99 frame time. — evidence: 
- [ ] **M3-A7** (auto) KB coverage passes; all earlier tests pass; `m3.db` is saved. — evidence: 
- [ ] **M3-A8** (auto) The flash analyser flags the known flashing test clip, and transition settings that would flash more than 3 times a second, and passes safe ones. — evidence: 

### M4. Live operation, services and stage

**Work packages**

- [ ] **M4.1** Services and the planner — evidence: 
- [ ] **M4.2** Live state machine — evidence: 
- [ ] **M4.3** Looks, props, messages, timers, macros and clear groups — evidence: 
- [ ] **M4.4** Producer mode — evidence: 
- [ ] **M4.5** Simple mode — evidence: 
- [ ] **M4.6** Stage display — evidence: 
- [ ] **M4.7** Ready for Sunday, practice mode, setup and tours — evidence: 
- [ ] **M4.8** End-to-end and chaos tests — evidence: 
- [ ] **M4.9** Knowledge base content for M4 — evidence: 

**Acceptance criteria**

- [ ] **M4-A1** (auto) A scripted 12-item service (songs, scripture, video, countdown) produces every expected frame, and every slide change lands within 1 frame of the comman… — evidence: 
- [ ] **M4-A2** (auto) Ctrl+Z in Live restores the previous on-screen state exactly (frame checksum). — evidence: 
- [ ] **M4-A3** (auto) Each screen shows the right layers for each look (golden frames). — evidence: 
- [ ] **M4-A4** (auto) Timer drift is ≤ 1 frame over 60 minutes (virtual clock); overrun behaves as set. — evidence: 
- [ ] **M4-A5** (auto) 50 random kills of UI, core or engine during an e2e run all keep the continuity rules: no black screen when UI or core is killed; an engine restart with… — evidence: 
- [ ] **M4-A6** (auto) A keyboard-only Simple-mode flow runs a song. — evidence: 
- [ ] **M4-A7** (auto) Stage display golden frames pass, including chord alignment. — evidence: 
- [ ] **M4-A8** (auto) Every Ready for Sunday check has unit tests and links to an existing fix article. — evidence: 
- [ ] **M4-A9** (auto) Tours refuse to start while live content is on outputs, unless practice mode is on. — evidence: 
- [ ] **M4-A10** (auto) KB coverage passes; the volunteer handbook collection exists; the ledger has grown; `m4.db` is saved. — evidence: 
- [ ] **M4-A11** (H) 3 people new to Selah run a practice service in Simple mode after the 5-minute tour, without help. — evidence: 
- [ ] **M4-A12** (auto) Booth lock leaves only Next, Previous and Clear working. — evidence: 

### M5. Screens, video and audio I/O

**Work packages**

- [ ] **M5.1** Display profiles and screen management — evidence: 
- [ ] **M5.2** Edge blending, corner pin and masks — evidence: 
- [ ] **M5.3** Colour calibration, LED walls and test patterns — evidence: 
- [ ] **M5.4** NDI in and out — evidence: 
- [ ] **M5.5** Texture sharing and virtual camera — evidence: 
- [ ] **M5.6** Cameras and capture inputs — evidence: 
- [ ] **M5.7** Audio routing, playlists and meters — evidence: 
- [ ] **M5.8** Web pages and web video — evidence: 
- [ ] **M5.9** Outputs UI, health and knowledge base — evidence: 

**Acceptance criteria**

- [ ] **M5-A1** (auto) The blend ramp matches its analytic curve within tolerance; corner pin and mask golden frames pass. — evidence: 
- [ ] **M5-A2** (auto) Colour calibration and test pattern golden frames pass; LED spanning layouts render correctly. — evidence: 
- [ ] **M5-A3** (auto) NDI loopback keeps alpha intact (fake backend always; the real SDK when present). — evidence: 
- [ ] **M5-A4** (auto) Spout or Syphon loopback works on the current platform. — evidence: 
- [ ] **M5-A5** (auto) Audio routing with a null sink sends signal to the right channels; measured per-output delay is within ±1 ms of the setting. — evidence: 
- [ ] **M5-A6** (auto) Simulated hot-plug switches profiles; outputs recover with no engine restart. — evidence: 
- [ ] **M5-A7** (auto) A local test web page renders as a layer (golden frame). — evidence: 
- [ ] **M5-A8** (H) A 3-projector blend has no visible seam. — evidence: 
- [ ] **M5-A9** (auto) KB coverage passes; earlier tests pass; `m5.db` is saved. — evidence: 
- [ ] **M5-A10** (auto) CEF policy tests pass: no file access, downloads, pop-ups or navigation to other sites. — evidence: 

### M6. Importers, migration and the alpha release

**Work packages**

- [ ] **M6.1** Import framework and worker — evidence: 
- [ ] **M6.2** MediaShow importer — evidence: 
- [ ] **M6.3** ProPresenter 6 and 7 importers — evidence: 
- [ ] **M6.4** Song-app importers — evidence: 
- [ ] **M6.5** Presentation importers and PowerPoint live mode — evidence: 
- [ ] **M6.6** Migration assistant and import report — evidence: 
- [ ] **M6.7** Alpha release pipeline — evidence: 
- [ ] **M6.8** Import quality gate, fuzzing and knowledge base — evidence: 

**Acceptance criteria**

- [ ] **M6-A1** (auto) Golden import tests pass for every format. — evidence: 
- [ ] **M6-A2** (auto) On the import corpus, at least 98% of songs need no manual fix (measured against expected outputs). — evidence: 
- [ ] **M6-A3** (auto) Each parser has been fuzzed for 10 minutes in a cloud session (corpus committed) and passes a 30-second smoke run in CI, with no crash or hang. — evidence: 
- [ ] **M6-A4** (auto) The support bundle includes importer logs and still contains no secrets (test with seeded fake secrets). — evidence: 
- [ ] **M6-A5** (H) PowerPoint import and live mode work on a PC with PowerPoint. — evidence: 
- [ ] **M6-A6** (H) The installer installs, updates and uninstalls cleanly on clean Windows and macOS machines. — evidence: 
- [ ] **M6-A7** (auto) KB coverage passes; earlier tests pass; `m6.db` is saved. — evidence: 
- [ ] **M6-A8** (H) The closed alpha starts with 10 churches (H-22). — evidence: 
- [ ] **M6-A9** (auto) Each worker runs with reduced rights (token and job-object checks on Windows, the sandbox profile on macOS). — evidence: 

### M7. Remote control, API and show control

**Work packages**

- [ ] **M7.1** Gateway, pairing and security — evidence: 
- [ ] **M7.2** Public API v1 and OpenAPI — evidence: 
- [ ] **M7.3** Web apps — evidence: 
- [ ] **M7.4** SDKs and Companion module — evidence: 
- [ ] **M7.5** MIDI — evidence: 
- [ ] **M7.6** OSC, DMX/sACN and network triggers — evidence: 
- [ ] **M7.7** Timelines and timecode — evidence: 
- [ ] **M7.8** Multi-machine sync: backup and campus feeds — evidence: 
- [ ] **M7.9** Knowledge base content for M7 — evidence: 

**Acceptance criteria**

- [ ] **M7-A1** (auto) The OpenAPI document validates; contract tests reach every command and query; every response validates against its schema. — evidence: 
- [ ] **M7-A2** (auto) Security: no access without a token; scopes are enforced (viewer can't trigger); revoked tokens fail at once; the API is off by default. — evidence: 
- [ ] **M7-A3** (auto) Playwright tests pass for the remote, pastor view and stage apps against a headless core. — evidence: 
- [ ] **M7-A4** (auto) MIDI loopback on virtual ports triggers the mapped commands; learn mode captures a note. — evidence: 
- [ ] **M7-A5** (auto) Timecode: LTC generated in the test chases with ±1 frame accuracy; timeline cues fire on time. — evidence: 
- [ ] **M7-A6** (auto) Failover: 2 engine/core pairs on one machine. — evidence: 
- [ ] **M7-A7** (auto) The Companion module builds and passes its tests against a mock API. — evidence: 
- [ ] **M7-A8** (H) A real phone pairs by QR and runs a service; Companion works with a Stream Deck. — evidence: 
- [ ] **M7-A9** (auto) KB coverage passes; earlier tests pass; `m7.db` is saved. — evidence: 
- [ ] **M7-A10** (auto) The API fuzz smoke run finds no crash. — evidence: 

### M8. Broadcast, creative and intelligence

**Work packages**

- [ ] **M8.1** Blackmagic DeckLink — evidence: 
- [ ] **M8.2** AJA — evidence: 
- [ ] **M8.3** Recording — evidence: 
- [ ] **M8.4** Streaming — evidence: 
- [ ] **M8.5** Multiview and stream graphics — evidence: 
- [ ] **M8.6** Full transition and effect library, and the shader SDK — evidence: 
- [ ] **M8.7** Speech cues — evidence: 
- [ ] **M8.8** Translation lines and parallel Bibles — evidence: 
- [ ] **M8.9** Content Packs — evidence: 
- [ ] **M8.10** Knowledge base content for M8 — evidence: 

**Acceptance criteria**

- [ ] **M8-A1** (auto) DeckLink and AJA simulators receive correct fill and key frames and timing; an input source works. — evidence: 
- [ ] **M8-A2** (auto) Recordings pass ffprobe validation; A/V sync is within ±1 frame; the disk guard stops recording safely. — evidence: 
- [ ] **M8-A3** (auto) A 30-minute stream to a local MediaMTX server (RTMP and SRT), run in a cloud session (CI runs a 1-minute version), has no reconnect; a forced disconnect… — evidence: 
- [ ] **M8-A4** (auto) Golden frames pass for every new transition and effect, and the SDK sample loads and hot-reloads. — evidence: 
- [ ] **M8-A5** (auto) On the test audio set, explicit scripture references are detected with at least 90% precision and 80% recall; nothing triggers live automatically. — evidence: 
- [ ] **M8-A6** (auto) Golden frames pass for translation lines and parallel Bibles; a Content Pack installs, searches and builds slides. — evidence: 
- [ ] **M8-A7** (H) DeckLink and AJA fill and key are verified on a real switcher; streaming is verified to a real platform. — evidence: 
- [ ] **M8-A8** (auto) KB coverage passes; earlier tests pass; `m8.db` is saved. — evidence: 
- [ ] **M8-A9** (auto) With speech cues on, the egress test still shows no outgoing connections. — evidence: 

### M9. Cloud, teams and enterprise

**Work packages**

- [ ] **M9.1** Sync server and organisation model — evidence: 
- [ ] **M9.2** Client sync engine — evidence: 
- [ ] **M9.3** Accounts, roles, SSO and SCIM — evidence: 
- [ ] **M9.4** Admin console, policies, audit log and privacy — evidence: 
- [ ] **M9.5** CCLI SongSelect and reporting — evidence: 
- [ ] **M9.6** Planning Center, MultiTracks and Loop — evidence: 
- [ ] **M9.7** Licensing and entitlements — evidence: 
- [ ] **M9.8** Enterprise packaging and update rings — evidence: 
- [ ] **M9.9** Telemetry, crash reporting and fleet dashboard — evidence: 
- [ ] **M9.10** Church articles, handbook export and knowledge base — evidence: 

**Acceptance criteria**

- [ ] **M9-A1** (auto) Sync property test: 5 simulated clients with random offline periods and edits all converge with no lost data; concurrent lyric edits produce conflict co… — evidence: 
- [ ] **M9-A2** (auto) SSO end to end against Keycloak in Docker; the SCIM conformance suite passes; the permission matrix is tested for every role × command. — evidence: 
- [ ] **M9-A3** (auto) The audit log records every admin and API action; GDPR export and delete work end to end. — evidence: 
- [ ] **M9-A4** (auto) CCLI, Planning Center, MultiTracks and Loop contract tests pass on recorded fixtures. — evidence: 
- [ ] **M9-A5** (auto) An upgrade from every earlier milestone's DB fixture and installer build to the current one migrates without data loss. — evidence: 
- [ ] **M9-A6** (auto) Telemetry and crash upload send nothing when consent is off (network-capture test) and the expected payloads when it's on. — evidence: 
- [ ] **M9-A7** (H) Silent MSI install with ADMX policies works on a domain or Intune test machine; the notarised macOS pkg installs with a configuration profile; real partner… — evidence: 
- [ ] **M9-A8** (auto) KB coverage passes; earlier tests pass; `m9.db` is saved. — evidence: 
- [ ] **M9-A9** (H) The closed beta starts with 50 churches, including 5 multi-campus and 5 broadcast-heavy (H-22). — evidence: 
- [ ] **M9-A10** (auto) The sync server refuses unauthenticated and cross-organisation requests (a test per endpoint). — evidence: 

### M10. Launch readiness and v1.0

**Work packages**

- [ ] **M10.1** Parity and v1.0-bar checklist — evidence: 
- [ ] **M10.2** Knowledge base completion and docs website — evidence: 
- [ ] **M10.3** Localisation — evidence: 
- [ ] **M10.4** Accessibility — evidence: 
- [ ] **M10.5** Performance, soak and chaos — evidence: 
- [ ] **M10.6** Security — evidence: 
- [ ] **M10.7** Release operations and v1.0 sign-off — evidence: 

**Acceptance criteria**

- [ ] **M10-A1** Every item in `docs/v1-parity-checklist.md` has passing evidence. — evidence: 
- [ ] **M10-A2** (auto) (ci-win) `tools/verify.sh --release` and `tools/verify.ps1 -Release` exit 0: KB coverage 100%, no drafts, every article reviewed, no stale translations. — evidence: 
- [ ] **M10-A3** (auto + H) The UI and knowledge base are complete in 6 languages and reviewed by native speakers. — evidence: 
- [ ] **M10-A4** (auto + H) Automated accessibility checks pass, and there are no open AA findings from the manual audit. — evidence: 
- [ ] **M10-A5** (H) Performance budgets (ARCHITECTURE §12) pass on reference hardware; the 72-hour soak shows no leaks or drift. — evidence: 
- [ ] **M10-A6** (auto + H) The chaos suite passes; there are no open P0 or P1 bugs or security findings. — evidence: 
- [ ] **M10-A7** (H) Every product plan §7.1 row is green: 99.9% glitch-free services across ≥ 50 churches over 12 weekends; SUS ≥ 80; volunteer findability ≥ 90%; migration ≥… — evidence: 
- [ ] **M10-A8** `v1.0.0` is tagged and signed, release notes are published, and the update feed is live. — evidence: 
- [ ] **M10-A9** (auto + H) Every SEC and SAFE item in docs/SECURITY.md has passing evidence in `docs/v1-parity-checklist.md`. — evidence: 

## Human tasks

Owner: you. Coding sessions keep this table up to date, mark which feature flags are waiting on each task, and never block other work on them (ADR-017).

| ID | Task | Needed by | Status | Notes |
|---|---|---|---|---|
| H-01 | Approve installing toolchains (Rust, VS Build Tools, LLVM, .NET 10 SDK, pnpm, buf). Only needed if you also code locally; cloud sessions install their own. | M1 (optional) | Not needed for cloud sessions | |
| H-02 | Confirm the product name and choose the code licence | M1 (placeholder allowed) | Open | |
| H-03 | GitHub remote (done). Before CI can run: clean the history, apply the public-period settings and make the repository public (docs/CLOUD-DEVELOPMENT.md §2.2 and §3) | M1 | Partly done | Repository recreated with a clean history and made public (2026-11-01). Still to do: the settings in docs/CLOUD-DEVELOPMENT.md §2.2. |
| H-04 | macOS checks run in the `macos` CI job. Provide a Mac only for hands-on macOS testing. | M1 onward (optional) | Open | |
| H-05 | Legal review: use of the analysis, interoperability importers (product plan §1) | Before M6 | Open | |
| H-06 | Confirm rights for bundled Bible translations | M2 | Open | |
| H-07 | Accept the NDI SDK licence and install the NDI runtime | M5 | Open | |
| H-08 | Hardware lab: projectors or displays, an LED processor, a capture card, a camera, a MIDI controller, a Stream Deck, a DMX node | M5, M7 | Open | |
| H-09 | Legitimate sample libraries from churches, with written permission | M6 | Open | |
| H-10 | A Google Cloud OAuth client for Google Slides | M6 | Open | |
| H-11 | A Blackmagic DeckLink card and SDK download; an AJA card | M8 | Open | |
| H-12 | A CCLI partner agreement and API credentials (start talks in M1) | M9 | Open | |
| H-13 | A Planning Center OAuth application | M9 | Open | |
| H-14 | MultiTracks and Loop Community partnerships | M9 | Open | |
| H-15 | Code-signing: Windows (Azure Trusted Signing or EV) and an Apple Developer ID with notarisation | M6 (alpha), M9 | Open | |
| H-16 | Cloud hosting, domains and transactional email for the sync service and docs site | M9 | Open | |
| H-17 | A payments account (for example Stripe) and pricing | M9 | Open | |
| H-18 | A content designer for 2i reviews of knowledge base articles | M2 onward | Open | |
| H-19 | Translators and native reviewers for es, pt, fr, ko, yo | M10 | Open | |
| H-20 | A manual accessibility audit with screen-reader users | M10 | Open | |
| H-21 | A third-party penetration test | M10 | Open | |
| H-22 | Pilot churches: 3 testers (M4), 10 alpha (M6), 50 beta (M9); usability studies | M4–M10 | Open | |
| H-23 | Reference hardware (ARCHITECTURE §12, TESTING §6) | M10 (useful from M1) | Open | |
| H-24 | Supply the private MediaShow format notes when M6.2 starts (they're kept outside the repository) | M6 | Open | |
| H-25 | Set up the Claude Code cloud environment: the setup script and network domains (docs/CLOUD-DEVELOPMENT.md §2.1) | M1 | Open | |
| H-26 | Before making the repository private again, choose how test builds and updates are delivered (docs/CLOUD-DEVELOPMENT.md §6) | When going private | Open | |

## Hardware and account checklists (H)

_Each (H) acceptance criterion gets a step-by-step checklist here when its milestone is built, for a person with the equipment or account to run._

(none yet)

## Test ledger history

Copied from `tests/ledger.json` at the end of each milestone. Counts may only go up (ADR-016).

| Milestone | rust | dotnet | web | golden | ui | e2e | kb | total |
|---|---|---|---|---|---|---|---|---|
| (baseline) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Deviations and decisions

_Anything done differently from BUILD-PLAN.md or the docs, with the reason and the ADR number if one was added._

- 2026-10-31, before M1: the plan moved to cloud-first development (ADR-023), lean CI (ADR-024), a pull-request workflow (ADR-025, replacing ADR-019) and secure and safe by default (ADR-026). M1 grew from 10 to 13 WPs (installer, security and safety baseline, problem reports and diagnostics).

- 2026-11-07, M1 (local session): this session runs on the user's Windows PC (H-01) rather than in a cloud session, so the gate was run stage by stage on Windows plus `cargo check` for the Linux and macOS targets; the Linux gate runs in the PR's `linux` job.
- 2026-11-07, M1.1: no LGPL macOS FFmpeg binary is published, so the lockfile pins the FFmpeg 7.1.2 source and `tools/setup.sh --fetch` builds a decode-only shared FFmpeg on macOS. Without encoders the Mac can't generate test clips, so the video correctness test (M1.7) uses a small committed clip.
- 2026-11-07, M1.1: `cargo-deny` is pinned as prebuilt release binaries in `tools/third_party.lock.json` instead of being compiled in every CI run.
- 2026-11-07, M1.3: the cargo-fuzz target needs nightly, so the CI smoke run is a stable test over the same entry point (`SELAH_FUZZ_SECONDS`); coverage-guided runs use `fuzz/` in cloud sessions.
- 2026-11-07, M1.4: the UI continuity check (M1-A3) uses `selah-fake-ui`, a stand-in that heartbeats, connects and subscribes like the UI, until the Avalonia shell (M1.8) exists. The crate `selah-runtime` (paths, locks, logs, redaction, heartbeats) is new and not yet in ARCHITECTURE §1; it's added with the M1 docs update.

## Quarantined tests

_Tests temporarily isolated because they're flaky, each with the reason, an owner and the milestone that must fix it. Quarantine blocks milestone completion._

(none)
