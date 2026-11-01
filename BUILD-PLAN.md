# Selah build plan: from idea to v1.0 in 10 milestones

This plan turns the [product plan](Church-Presentation-Platform-Plan.md) into buildable milestones. It is written so that a fresh coding session can be told **"Complete milestone N"** and carry it out completely and without regressions, following the protocol in [CLAUDE.md](CLAUDE.md).

**How to read each milestone**
- **Goal**: one sentence.
- **Depends on**: milestones that must be Done first.
- **Read first**: the only documents the session needs beyond this one.
- **Human inputs**: things only a person can provide. If they're missing, the work continues with fakes (ADR-017) and the gap is recorded.
- **Work packages (WPs)**: ordered; each is sized for about one focused session and ends in one commit.
- **Acceptance criteria**: all must pass.
  - (auto) is checked by the gate: `tools/verify.sh --cloud` in the session, and the `linux` CI job.
  - (ci-win) and (ci-mac) are checked by the `windows` and `macos` CI jobs.
  - (H) needs a person, hardware or an account. Most (H) checks are done by installing the test build on your PC ([docs/CLOUD-DEVELOPMENT.md §4](docs/CLOUD-DEVELOPMENT.md#4-each-milestone)).
- **Knowledge base**: the help content the milestone must ship (see [KNOWLEDGE-BASE.md §9](docs/KNOWLEDGE-BASE.md#9-article-inventory-by-milestone)).
- **Security and safety**: the SEC and SAFE items from [docs/SECURITY.md](docs/SECURITY.md) the milestone delivers, each with tests (ADR-026).

The rules in [TESTING-AND-QUALITY.md §4](docs/TESTING-AND-QUALITY.md#4-definition-of-done) (definition of done) apply to every WP and every milestone, and aren't repeated below.

Work happens in Claude Code cloud sessions (ADR-023). Each session works on its own branch and ends by opening a pull request, which the user reviews and merges (ADR-025). CI is small on purpose (ADR-024).

---

## Overview

| # | Milestone | Main outcome | Depends on | WPs |
|---|---|---|---|---|
| M1 | Foundations and risk spikes | All processes run and talk; text, video and crash survival proven; the gate, CI, a Windows installer and the security and safety baseline exist | — | 13 |
| M2 | Library, songs, scripture and the help system | Songs, Bibles and media in a real database; paste-to-slides; undo; search; contextual help framework | M1 | 9 |
| M3 | Render engine, templates and the slide editor | Templates, words that fit every screen, backgrounds, video, transitions, thumbnails, slide editor | M2 | 8 |
| M4 | Live operation, services and stage | Plan and run a full service in Producer or Simple mode; stage display; Ready for Sunday; tours | M3 | 9 |
| M5 | Screens, video and audio I/O | Edge blend, corner pin, masks, colour, LED walls, NDI, Spout/Syphon, cameras, audio routing, web layers | M4 | 9 |
| M6 | Importers, migration and the alpha release | Every importer, the migration assistant and the first installable alpha | M3 (M4 for services) | 8 |
| M7 | Remote control, API and show control | Gateway, phone remote, pastor view, API, Companion, MIDI, OSC, DMX, timelines, backup computer | M4 | 9 |
| M8 | Broadcast, creative and intelligence | SDI fill and key, recording, streaming, multiview, full effects library, speech cues, translations, Content Packs | M5 | 10 |
| M9 | Cloud, teams and enterprise | Sync server, accounts, SSO, roles, audit, CCLI, Planning Center, licensing, enterprise packaging, telemetry | M2, M7 | 10 |
| M10 | Launch readiness and v1.0 | Every v1.0-bar row green: docs, languages, accessibility, performance, security, release | M1–M9 | 7 |

**Release stages:** after M6, a closed alpha (10 churches, H-22). After M9, a closed beta (50 churches). After M10, the public v1.0 (product plan §7).

**Where each v1.0-bar row (product plan §7.1) is delivered:**

| Row | Delivered in |
|---|---|
| Power | M2–M9 |
| Beyond the incumbents | M3 fit-every-screen, M4 modes and Ready for Sunday, M7 phone remote, M8 speech cues, M1 crash survival |
| Reliability | M1, M4, M10 |
| Security and safety | M1 baseline, then every milestone (docs/SECURITY.md), signed off in M10 |
| Ease of use | M4, M10 |
| Migration | M6 |
| Help and documentation | every milestone, finished in M10 |

---

## M1. Foundations and risk spikes

| | |
|---|---|
| **Goal** | Create the toolchains, contracts, all four processes, the quality gate, CI, a Windows installer, the security and safety baseline and the knowledge base pipeline, and prove the risky parts: text, video, IPC speed and surviving crashes. |
| **Depends on** | — |
| **Read first** | docs/ARCHITECTURE.md, docs/DECISIONS.md, docs/TESTING-AND-QUALITY.md, docs/SECURITY.md, docs/CLOUD-DEVELOPMENT.md §1 and §5, docs/DESIGN-SYSTEM.md §3–5 and §10, docs/KNOWLEDGE-BASE.md §6–7 |
| **Human inputs** | H-25 the cloud environment (setup script and network domains). H-03 the repository made public with the CI settings; until then CI can't run, but the session still opens its PR. H-02 licence and name confirmation (placeholder until then). H-04 macOS checks come from the `macos` CI job. H-01 applies only if you also code locally. |

### Work packages
- **M1.1 Repository, toolchain and conventions**
  - These already exist (added with ADR-023 to ADR-026): the repository, `.gitattributes`, `.claude/settings.json`, `ci.yml` and `release.yml`, the PR and issue templates, `tools/cloud-setup.sh`, `tools/open-pr.sh`, `tools/ci.sh`, `tools/check-workflows.py` and `tools/check-secrets.py`. Work on the session's own branch (ADR-025) and extend the starter `.gitignore`.
  - `tools/setup.sh` (Linux and macOS) and `tools/setup.ps1` (Windows) check the toolchain. Cloud sessions get it from `tools/cloud-setup.sh`; CI runners use their images plus `actions/setup-dotnet`. For anyone coding locally (H-01), `setup.ps1 -Install` installs rustup with the MSVC toolchain, Visual Studio 2022 Build Tools (C++ workload), LLVM/libclang, the .NET 10 SDK, and Node 22 LTS with pnpm through corepack. `@bufbuild/buf` is a pinned dev dependency in `package.json`.
  - Setup also downloads pinned third-party files (FFmpeg 7 LGPL shared builds for Linux, Windows and macOS; Inter, Manrope, JetBrains Mono, Noto Sans/Serif, Noto Sans KR and Noto Naskh Arabic fonts) into `third_party/`, verified against `tools/third_party.lock.json`. It also generates synthetic test videos (H.264 and HEVC, 1080p and 4K, with a flash and beep for A/V sync tests) into `tests/fixtures/media/generated/`.
  - Root `Cargo.toml` workspace and `rust-toolchain.toml`; `app/Selah.sln`, `global.json`, `Directory.Build.props` (nullable, warnings as errors, no unsafe blocks, NuGet locked mode); `pnpm-workspace.yaml`. Lockfiles are committed (SEC-09).
  - Every crate starts with `#![forbid(unsafe_code)]` unless it's listed in `tools/unsafe-allowlist.txt` (SEC-11). `deny.toml` for `cargo deny`: advisories, licences (no GPL) and sources (SEC-09).
  - `.editorconfig`, `LICENSE` (placeholder: all rights reserved until H-02), `CHANGELOG.md`, `README.md` updated.
- **M1.2 Contracts and code generation**
  - `proto/selah/v1/` with `envelope`, `common`, `render`, `settings`, `help` and `errors` at minimum, plus `buf.yaml` and `buf.gen.yaml`.
  - `selah-proto` crate (prost + `protoc-bin-vendored`); C# generation in `Selah.Client` (Grpc.Tools); `protobuf-es` for the web.
  - `tools/codegen.ps1` regenerates everything. `buf lint` is clean.
- **M1.3 IPC layer**
  - `selah-ipc`: named pipes / UDS, length-prefixed `Envelope` frames, request/reply with correlation, event subscriptions, reconnect with backoff.
  - A shared-memory ring buffer for preview frames.
  - C# client in `Selah.Client`.
  - Access and limits (SEC-02): a pipe DACL for the current user only, `PIPE_REJECT_REMOTE_CLIENTS`, a peer-process check, a `0700` socket directory, and a 16 MiB frame limit.
  - Tests: 10,000 round trips; malformed and oversized frames are rejected safely; reconnect after a server restart. A `cargo fuzz` target for the frame decoder (SEC-03).
  - Bench: p99 round trip.
- **M1.4 Processes and watchdog**
  - `selah-core` host skeleton: status, settings store, a live-state snapshot file.
  - `selah-engine` skeleton.
  - `selah-watchdog`: spawns processes, sends a 250 ms heartbeat, restarts within 2 s, backs off after a crash loop.
  - Single-instance lock; `tracing` JSON logs with rotation and the redaction layer (SEC-06); launcher `selah`.
- **M1.5 Engine windows, compositor and test patterns**
  - winit output windows (fullscreen on a chosen display, or windowed for development); a wgpu compositor with a layer stack stub. It runs on lavapipe (Linux, cloud), WARP (Windows CI) and Metal (macOS).
  - Headless offscreen mode.
  - WGSL test patterns: SMPTE bars, grid, gradient.
  - `FrameShown` events with timestamps; preview frames written to the shared ring.
- **M1.6 Text spike**
  - Skia (`skia-safe`) CPU raster with HarfBuzz shaping, using bundled slide fonts; texture cache keyed by content, style and resolution.
  - Golden frames for English, Yorùbá (with tone marks), Korean and Arabic (right to left). The reference set is rendered on lavapipe in the cloud; the Windows (WARP) and macOS jobs compare against it within tolerance (ADR-023).
- **M1.7 Video spike**
  - FFmpeg hardware decode (d3d11va / videotoolbox, software fallback) → NV12 upload → shader conversion → composited under text.
  - FFmpeg opens files with a protocol whitelist and a demuxer and decoder allowlist (SEC-04).
  - `selah-bench` reports decode and present timing and dropped frames. The cloud and CI use software decode; hardware decode is measured on your PC with **Help ▸ Diagnostics ▸ Run benchmark** (M1.13).
  - Headless correctness test on the 1080p clip.
- **M1.8 UI shell, design tokens and text size**
  - `design/tokens/tokens.json` (from DESIGN-SYSTEM §3–5), generated into Avalonia resources and CSS variables (Style Dictionary).
  - `Selah.UI` shell matching the prototype's toolbar, sidebar and inspector skeleton, with Light and Booth appearances.
  - Bundled Inter, Manrope and JetBrains Mono.
  - **Settings ▸ Appearance ▸ Text size** with 7 steps, "Use my computer's text size" and a live preview (prototype screen 8). Ctrl/Cmd `+` `−` `0` shortcuts.
  - Fluent strings for `en` and the pseudo-locale `en-XA`.
  - Connects to core, shows status, and has a **Show test pattern** command.
  - Token contrast test.
- **M1.9 Knowledge base pipeline**
  - `selah-kb` (parse, validate, build `kb.db` with FTS5) and `selah-kb-lint` (rules R1–R18 where automatable, readability per KNOWLEDGE-BASE §7.1).
  - `content/terms.yaml`; `content/help/anchors.yaml`; `content/errors/errors.yaml` (with `SEL-OUT-0001` "a screen is not connected" and `SEL-OUT-0002` "a screen was unplugged"); article templates in `content/kb/_templates/`.
  - `content/kb/planned.yaml` (KNOWLEDGE-BASE §7.1) listing everything the 3 exemplars reference that later milestones build:
    - articles such as `live.clear-the-screens` and `screens.set-up-edge-blending`;
    - anchors such as `simple.slide-strip` and `screens.blend-settings`;
    - settings such as `outputs.blend.overlap`;
    each tagged with the milestone that delivers it.
  - `help.proto` queries served by core; a basic article view in the UI that renders the AST with native controls.
  - Lint unit tests use deliberately bad fixtures.
- **M1.10 Quality gate and CI**
  - `tools/verify.sh` (`--cloud`, `--ci`, `--fast`, `--release`) and `tools/verify.ps1` (`-CI`, `-Fast`, `-Release`): stages 1–9, 13 and 14 from TESTING §1. In the cloud it also runs `cargo check --target x86_64-pc-windows-gnu`. Failures print GitHub annotations (`::error file=…,line=…::`) and a summary in `target/verify-report/`.
  - `tests/ledger.json` and `tools/test-ledger`; `tools/bless-golden.sh --reason`; `.github/workflows/bless-golden.yml` (manual only: adds a reviewed platform-specific reference image to the PR branch, with its reason in `tests/golden/CHANGES.md`).
  - `tools/check-unsafe` and `tools/unsafe-allowlist.txt` (SEC-11). `tools/check-workflows.py` and `tools/check-secrets.py` run in the gate as well as in `guard`.
  - `ci.yml` and `release.yml` already exist and start running the gate once `tools/verify.sh` exists. Keep them passing `tools/check-workflows.py`, and confirm the first PR run is green.
- **M1.11 Windows installer and updates**
  - `packaging/velopack/` and `tools/package.ps1`: a Velopack `Setup.exe` for Windows x64 that installs per user without administrator rights (SEC-01). It adds Start menu shortcuts, has a `dev` channel, and its uninstaller leaves the library and settings. CI sets the version: `0.0.<run>` for PRs, `0.<N>.0` for milestones.
  - `tools/test-installer.ps1`: on the Windows runner, install silently, start Selah, wait for "Connected" and the test pattern (headless check), uninstall, and check that nothing is left except user data.
  - The updater checks only when the user asks (in dev builds), verifies package hashes, and never installs while something is on the screens (SEC-12, SAFE-06). The feed URL comes from settings, never a built-in token. Signing waits for H-15.
  - CI uploads `Selah-win-x64-Setup` on every PR run that changes code; `release.yml` publishes it for milestone tags.
- **M1.12 Security and safety baseline**
  - `selah-net`, the single gate for outgoing connections, with a policy and a connection log. The egress test checks that default settings make no outgoing connections (SEC-13).
  - `SecretStore` (Credential Manager with DPAPI, Keychain, and an in-memory fake), with seeded-secret tests over logs, settings, the op log and support bundles (SEC-05, SEC-06).
  - An `Actor` on every `Envelope`, and a permission check on every command in core, with a permission-matrix test (SEC-14, SAFE-01).
  - The emergency lane (ADR-026): the UI sends **Clear everything** and **Show logo** straight to the engine on a dedicated pipe, and the engine applies them within 1 frame even if core is frozen (SAFE-03).
  - Safe mode in the watchdog: 3 crashes in 60 seconds, or Shift held at start, starts Selah with the risky subsystems off and a banner (SAFE-07). Error code `SEL-APP-0001`, with the fix article `troubleshooting.selah-started-in-safe-mode`.
  - Guards while something is on the screens: quitting and updating ask first, and the computer doesn't sleep (SAFE-06).
  - Update docs/SECURITY.md with anything M1 changed.
- **M1.13 Problem reports, diagnostics and spike report**
  - **Help ▸ Report a problem** saves a redacted support bundle (logs, versions, and settings without secrets or the user's name) and opens the GitHub problem-report form with the version filled in (SEC-06). Article `getting-started.report-a-problem`.
  - **Help ▸ Diagnostics** shows system, GPU and output information, and **Run benchmark** (4K60 HEVC decode and present, IPC round trip) saves a report to attach to a PR or issue. This is how hardware results reach cloud sessions (ADR-023).
  - `docs/specs/command-bus.md` and `docs/specs/spike-report.md` (measurements against budgets, plus a go/no-go recommendation on Rust + Avalonia); ADRs updated if anything changed.
  - The PR that completes M1 has the `ci:macos` label.

### Acceptance criteria
- **M1-A1** (auto) `tools/verify.sh --cloud` exits 0 in a cloud session. (ci-win) The PR's `guard`, `linux` and `windows` jobs pass. (ci-mac) The `macos` job passes on the PR that completes the milestone.
- **M1-A2** (auto) Starting `selah` launches core, engine and UI. The UI shows "Connected". **Show test pattern** puts SMPTE bars on the output, and `FrameShown` arrives no more than 1 frame after the engine receives the command (headless test).
- **M1-A3** (auto) If the UI is killed, the output frame checksum doesn't change, and the UI is relaunched and reconnected within 2 s.
- **M1-A4** (auto) If the engine is killed, it's restarted within 2 s and the previous scene is restored.
- **M1-A5** (auto) If core is killed, the engine keeps the last frame; core restarts and resynchronises within 2 s.
- **M1-A6** (auto) Golden text frames for en, yo, ko and ar pass on lavapipe. (ci-win) (ci-mac) The same reference images pass on Windows (WARP) and macOS within tolerance.
- **M1-A7** (auto) The 1080p H.264 clip decodes and composites under text with frames in order and no drops on a 60 fps virtual clock. (H) You run **Help ▸ Diagnostics ▸ Run benchmark** on your PC and attach the report; the 4K60 HEVC result goes in the spike report. Reference hardware comes in M10.
- **M1-A8** (auto) (ci-win) The IPC p99 round trip is ≤ 2 ms on Unix sockets and on named pipes (bench results recorded).
- **M1-A9** (auto) UI tests pass at 100/150/200%, Light and Booth, `en` and `en-XA`, with no clipped critical labels. The token contrast test passes.
- **M1-A10** (auto) The 3 exemplar articles pass `selah-kb-lint`; each bad fixture fails with its expected rule ID; `kb.db` builds; an article renders in the UI.
- **M1-A11** The spike report is written with measurements and a go/no-go.
- **M1-A12** (ci-win) The `windows` job builds `Selah-win-x64-Setup.exe`, and `tools/test-installer.ps1` installs it silently, sees "Connected" and the test pattern, and uninstalls it cleanly. (H) You install it from the PR on your PC and see the test pattern (checklist in PROGRESS.md).
- **M1-A13** (auto) Security baseline tests pass: IPC access and limits (SEC-02), the IPC fuzz smoke run (SEC-03), FFmpeg refusing network URLs in file items (SEC-04), seeded secrets appearing nowhere (SEC-05, SEC-06), no outgoing connections at default settings (SEC-13), a permission check on every command (SEC-14), `check-unsafe` (SEC-11), `cargo deny` and the other audits (SEC-09), and `check-workflows` (SEC-10).
- **M1-A14** (auto) Safety baseline: with core frozen, **Clear everything** clears every output within 1 frame (SAFE-03); 3 crashes in 60 seconds start safe mode with its banner (SAFE-07); quitting or updating while something is on screen asks first (SAFE-06).
- **M1-A15** (auto) **Report a problem** produces a bundle that has logs and versions but no seeded secrets and no user name. **Diagnostics** writes a benchmark report.
- **M1-A16** (auto) The session opened its PR with `tools/open-pr.sh`, the description follows the template, and `tools/ci.sh status` reads the PR's CI results.

### Knowledge base
The 3 exemplars (already written) pass lint and are built into `kb.db`. Article templates exist. Two new articles: `troubleshooting.selah-started-in-safe-mode` and `getting-started.report-a-problem`.

### Security and safety
The baseline: SEC-01, SEC-02, SEC-03 (IPC), SEC-04, SEC-05, SEC-06, SEC-09, SEC-10, SEC-11, SEC-12 (hashes and live deferral), SEC-13 and SEC-14 (framework); SAFE-01 (framework), SAFE-03 (lane), SAFE-06 (quit, update and sleep guards) and SAFE-07.

### Out of scope
The real data model, live UI and every integration.

---

## M2. Library, songs, scripture and the help system

| | |
|---|---|
| **Goal** | Store songs, Bibles and media in a real database with undo, autosave and search; turn pasted lyrics into structured songs and slides; ship the contextual help framework. |
| **Depends on** | M1 |
| **Read first** | docs/ARCHITECTURE.md §3–4, §7, §9; docs/KNOWLEDGE-BASE.md (all); docs/DESIGN-SYSTEM.md §6–8; prototype screen 03 |
| **Human inputs** | H-06 confirm rights for bundled Bible translations (until then, only KJV and WEB ship). H-18 a content designer for 2i reviews (articles stay `status: review` until then). |

### Work packages
- **M2.1 Data model, store and migrations**
  - `library.proto` and `service.proto` (data parts); SQLite schema per ARCHITECTURE §4.2 with append-only migrations.
  - Repositories in `selah-core`; BLAKE3 media store; library folder layout; settings store with policy levels.
  - `docs/specs/data-model.md`.
- **M2.2 Command bus, op log, undo/redo and autosave**
  - Validation; ops with inverses; undo groups; per-document and global undo; WAL; crash recovery of unsaved edit buffers; events to subscribers.
  - The error catalogue grows, and every new code has a fix article.
- **M2.3 Search**
  - FTS5 for songs (title, lyrics, authors, CCLI number, tags), media and Bible text; synonyms; typo tolerance (trigrams); ranking.
  - Bench on a 10,000-song generated corpus.
- **M2.4 Songs: paste-to-sections, chords and slide splitting**
  - Detect sections from plain text, `[Verse]`-style markers, chords-over-lyrics and ChordPro. Repeated blocks become a chorus.
  - Labels, colours and per-label hotkeys; arrangements; chord parsing and transposition.
  - Rule-based splitting by template `max_lines`; copyright-line rules; title and blank slides; translation lines.
  - Curated fixtures: 25 public-domain hymns, with provenance.
- **M2.5 Scripture**
  - Bible store (one SQLite per translation); bundled KJV and WEB.
  - Reference parser (`jn 3 16-18`, `Rom 8:28–39`, `1 Cor 13`, book names and abbreviations in en, es, pt, fr, ko and yo).
  - OSIS, USFM and Zefania importers; verse-to-slide splitting.
- **M2.6 Media bin and library exchange**
  - Import images and videos (probe, thumbnails via a temporary CPU path until M3), tags.
  - `.selah` bundle export and import with JSON Schema in `formats/`; backup and restore; canonical JSON for round trips.
- **M2.7 Library and song editor UI**
  - Library (songs, media, Bibles), matching prototype screen 03: the paste flow with a step indicator, the sections editor, chords, the arrangement builder, and an inspector with labels and hints.
  - Bible browser and reference entry; media bin; command palette (Ctrl/Cmd+K) across content, commands and help.
- **M2.8 Contextual help components**
  - `Help.Anchor` attached property and constants generated from `anchors.yaml`.
  - `HintText`, `InfoButton` with popover, `HelpPanel` (F1, "For this screen", search, article view, feedback), `WhatsThisOverlay` (Shift+F1), `EmptyState`, `ErrorAlert` with "How to fix".
  - Local feedback store; coverage tests 1–6 (KNOWLEDGE-BASE §7.2) switched on in verify.
- **M2.9 Knowledge base content for M2**
  - About 35 articles per KNOWLEDGE-BASE §9, plus the `getting-started` area. All as `status: review`, all lint-clean.

### Acceptance criteria
- **M2-A1** (auto) The 25 curated hymns, pasted in 4 formats (plain, markers, chords over lyrics, ChordPro), produce exactly the expected section JSON (100%).
- **M2-A2** (auto) Undo/redo property test: 10,000 random op sequences. Undoing everything restores the original state, and redoing everything restores the final state.
- **M2-A3** (auto) Killing core during 1,000 writes leaves `PRAGMA integrity_check` = ok, loses no committed op, and the unsaved editor buffer is restored.
- **M2-A4** (auto) Song search p95 ≤ 50 ms on 10,000 songs. "amzing grace" finds "Amazing Grace"; synonym tests pass.
- **M2-A5** (auto) At least 300 scripture-reference test cases pass across 6 languages; OSIS, USFM and Zefania samples import correctly.
- **M2-A6** (auto) `.selah` export → import → export produces identical canonical JSON; backup and restore round trip.
- **M2-A7** (auto) Headless UI flows pass at 100% and 200% in both appearances: paste a song; edit a section; add scripture; import media; Ctrl+K finds a song, a command and a help article.
- **M2-A8** (auto) Help coverage tests pass. The help panel shows the right "For this screen" articles on 3 screens. What's this shows a popover for a selected control.
- **M2-A9** (auto) `tests/fixtures/db/m2.db` is saved, and every M1 test still passes (ledger).
- **M2-A10** (H) H-06 Bible rights are confirmed before more translations are bundled.
- **M2-A11** (auto) Every M2 parser passes its fuzz smoke run; a crafted bundle with `..` paths or oversized entries is refused; every new command is in the permission matrix.

### Knowledge base
About 35 articles (§9 list). Help anchors for every M2 screen.

### Security and safety
SEC-03: `.selah` bundles, Bible files (OSIS, USFM, Zefania) and ChordPro are parsed with limits, and each has a fuzz target. SEC-04: imported images are decoded with a pixel limit. SEC-14: every new command has a permission entry. SEC-15: atomic writes, an integrity check when the library opens, media verified by BLAKE3 hash, and backups proven by restoring them.

### Out of scope
Metric text fit and real thumbnails (M3), services (M4) and importers from other apps (M6).

---

## M3. Render engine, templates and the slide editor

| | |
|---|---|
| **Goal** | Render real slides from templates on every output, make words fit every screen, and add backgrounds, video, transitions, effects, thumbnails and a slide editor. |
| **Depends on** | M2 |
| **Read first** | docs/ARCHITECTURE.md §4.3, §5, §6; ADR-005, ADR-006; prototype screens 01 and 03 |
| **Human inputs** | H-23 reference hardware (optional now; required in M10) |

### Work packages
- **M3.1 Scene graph, layers and frame scheduler**
  - A layer stack per output (video input, media, slide, props, messages); vsync-locked scheduling; multiple outputs at different resolutions; pre-rasterising the current and next items.
- **M3.2 Templates and text styling**
  - Role slots (body, reference, title, footer/copyright) and variants per aspect ratio.
  - Text styles: font, size, weight, colour, outline, shadow, background box, line height, letter spacing, alignment, safe area, minimum size.
- **M3.3 Metric fit: words that fit every screen**
  - Skia measurement in slide generation: shrink to the minimum size, then re-split if needed.
  - A per-output "fits" report used by the UI and by Ready for Sunday (M4).
- **M3.4 Backgrounds and media playback**
  - Images (fit, fill, stretch, blur, opacity), colours and gradients.
  - Video: loop, in and out points, speed. Audio from video on the default device, with audio as the master clock for A/V sync.
- **M3.5 Transitions and effects foundation**
  - 8 transitions: cut, fade, dissolve, fade through black, push, wipe, zoom, slide.
  - 6 effects: blur, colour adjust, vignette, background blur, colour key, mask.
  - Per-item overrides; the WGSL shader chain interface used later by the shader SDK (M8).
- **M3.6 Thumbnails and preview frames**
  - An offscreen thumbnail service with a disk cache; the UI slide grid uses real thumbnails; program and preview rings drive UI previews.
- **M3.7 Template editor and slide editor UI**
  - Custom slides: text, shapes, images, video, and clock or timer text tokens. Alignment guides, grouping, z-order and an inspector.
  - A template editor; assigning templates to songs, scripture and services.
- **M3.8 Golden-frame suite and knowledge base**
  - Golden frames for every style feature, transitions at t = 0, 0.25, 0.5, 0.75 and 1, and every effect. About 20 articles.

### Acceptance criteria
- **M3-A1** (auto) Golden frames pass for every template style feature, transition step and effect.
- **M3-A2** (auto) Fit property test: 5,000 random lyric sets × 3 outputs (16:9, 32:9 LED, 4:3). No text leaves the safe area; text is never below the template's minimum size without an extra split; the fit report is correct.
- **M3-A3** (auto) Thumbnails match the downscaled real output (pixel difference under the threshold).
- **M3-A4** (auto) A 1080p loop has no frame gap at the loop point, and A/V sync is within ±1 frame (flash/beep test clip).
- **M3-A5** (auto) Slide editor UI flow: create a custom slide with text, an image and a shape, align them, undo and redo.
- **M3-A6** (auto) (ci-win) Bench recorded in the cloud and the Windows job: 2 × 1080p outputs with a 4K video background, p99 frame time. (H) The same benchmark from **Help ▸ Diagnostics** on your PC; reference hardware in M10.
- **M3-A7** (auto) KB coverage passes; all earlier tests pass; `m3.db` is saved.
- **M3-A8** (auto) The flash analyser flags the known flashing test clip, and transition settings that would flash more than 3 times a second, and passes safe ones. The limiter keeps a +6 dBFS test tone under −1 dBTP. Clearing audio fades.

### Knowledge base
About 20 articles (§9 list).

### Security and safety
SAFE-04: the flash guard runs on output frames, and its results show in preview and in the fit report. SAFE-05: the master limiter and fades for audio from video. SEC-04: the decoder allowlist covers backgrounds. Until the M8 sandbox exists, the shader chain accepts only built-in shaders.

### Out of scope
Edge blend and other output processing (M5), live control (M4) and the full effects library (M8).

---

## M4. Live operation, services and stage

| | |
|---|---|
| **Goal** | Plan a service and run it live in Producer or Simple mode, with looks, props, messages, timers, macros, a stage display, Ready for Sunday checks, practice mode and guided tours. |
| **Depends on** | M3 |
| **Read first** | Product plan §5; docs/ARCHITECTURE.md §5, §7; docs/DESIGN-SYSTEM.md (all); docs/KNOWLEDGE-BASE.md §5; prototype screens 01, 02, 04, 06 |
| **Human inputs** | H-22 3 volunteers for an informal Simple-mode check (M4-A11) |

### Work packages
- **M4.1 Services and the planner**
  - Services, service types, templates, "inherit from last week", items of every type (song, scripture, media, presentation, countdown, header, audio; web and camera arrive in M5).
  - Run-time projection, owners and team (local), notes; print and PDF (service order, chord charts). The Plan view matches prototype screen 04.
- **M4.2 Live state machine**
  - Current and next slide and item; triggering; layers; F9–F12; undo in Live (re-trigger the previous state); per-label hotkeys; basic audio-layer playback.
  - Live-state snapshots for crash recovery.
- **M4.3 Looks, props, messages, timers, macros and clear groups**
  - Look rules (screen × layer); props; messages with tokens and durations; timers (countdown, count up, count to a time, overrun) and clocks; macros and slide actions; clear groups.
- **M4.4 Producer mode**
  - A docking workspace (Dock.Avalonia) with saved layouts, matching prototype screen 01: service order, slide grid, program and preview, layers, tiles for looks, timers and macros, the health bar, and keyboard-first operation.
- **M4.5 Simple mode**
  - Matches prototype screen 02: a service step indicator, Now and Next, big buttons and plain clear buttons.
  - Role lock; **Message the booth**, which sends a message to the producer's UI.
- **M4.6 Stage display**
  - A stage layout editor (current and next text, chords, key, clock, timers, messages, notes, arrangement position).
  - Stage outputs rendered by the engine; matches prototype screen 06.
- **M4.7 Ready for Sunday, practice mode, setup and tours**
  - Pre-flight checks: media present and hash OK, fonts present, text fits on all outputs, licence and copyright lines, outputs connected, backup sync (a stub until M7). Each failed check links to its fix.
  - Practice mode: outputs go to a preview window only, with a banner.
  - First-run setup (welcome, screens, Bible, CCLI number, a sample service of public-domain songs).
  - The tour engine and tours: Simple mode in 5 minutes, and a Producer overview.
- **M4.8 End-to-end and chaos tests**
  - `tests/e2e` scripted 12-item services on headless processes, with frame and latency asserts and random process kills.
- **M4.9 Knowledge base content for M4**
  - About 45 articles, Quick tips for Simple mode, and the `volunteer-handbook` collection.

### Acceptance criteria
- **M4-A1** (auto) A scripted 12-item service (songs, scripture, video, countdown) produces every expected frame, and every slide change lands within 1 frame of the command.
- **M4-A2** (auto) Ctrl+Z in Live restores the previous on-screen state exactly (frame checksum).
- **M4-A3** (auto) Each screen shows the right layers for each look (golden frames).
- **M4-A4** (auto) Timer drift is ≤ 1 frame over 60 minutes (virtual clock); overrun behaves as set.
- **M4-A5** (auto) 50 random kills of UI, core or engine during an e2e run all keep the continuity rules: no black screen when UI or core is killed; an engine restart within 2 s restores the scene.
- **M4-A6** (auto) A keyboard-only Simple-mode flow runs a song. A role-locked volunteer can't open Producer mode.
- **M4-A7** (auto) Stage display golden frames pass, including chord alignment.
- **M4-A8** (auto) Every Ready for Sunday check has unit tests and links to an existing fix article.
- **M4-A9** (auto) Tours refuse to start while live content is on outputs, unless practice mode is on.
- **M4-A10** (auto) KB coverage passes; the volunteer handbook collection exists; the ledger has grown; `m4.db` is saved.
- **M4-A11** (H) 3 people new to Selah run a practice service in Simple mode after the 5-minute tour, without help.
- **M4-A12** (auto) Booth lock leaves only Next, Previous and Clear working. F12 and holding Esc clear the screens from every Live view. When a role-locked volunteer tries a Producer command, core refuses it and logs it.

### Knowledge base
About 45 articles, Quick tips and the volunteer handbook.

### Security and safety
SEC-14 and SAFE-01: the role lock and Simple-mode permissions are enforced in core; **Message the booth** reaches only the operator. SAFE-03: **Clear everything** is always visible in Live, on F12 and on a held Esc. SAFE-06: **Booth lock** (Ctrl/Cmd+L). Ready for Sunday includes the flash and limiter checks.

### Out of scope
Output geometry and video inputs (M5), the API and remote (M7).

---

## M5. Screens, video and audio I/O

| | |
|---|---|
| **Goal** | Support real rooms: display profiles, edge blending, corner pin, masks, colour, LED walls, NDI, texture sharing, cameras, audio routing and web layers, with health and diagnostics. |
| **Depends on** | M4 |
| **Read first** | docs/ARCHITECTURE.md §6, §10; ADR-017; prototype screen 05 |
| **Human inputs** | H-07 NDI SDK licence and runtime. H-08 hardware lab (2–3 projectors or displays, an LED processor or simulated wall, a capture card, a camera). |

### Work packages
- **M5.1 Display profiles and screen management**
  - Detect displays; assign roles (audience, stage, lobby); switch profiles automatically by connected display set; handle hot-plug; **Identify screens**.
- **M5.2 Edge blending, corner pin and masks**
  - A multi-projector canvas with overlap, gamma and black-level lift; 4-point and mesh corner pin; masks.
- **M5.3 Colour calibration, LED walls and test patterns**
  - Brightness, contrast, gamma and RGB gain per output; non-standard and spanned LED layouts; test patterns (SMPTE, PLUGE, grid, gradient, alignment).
- **M5.4 NDI in and out**
  - Send program or any output with alpha; receive with discovery as a video input. The SDK is loaded dynamically, and a fake backend is used for tests.
- **M5.5 Texture sharing and virtual camera**
  - Spout2 (Windows) and Syphon (macOS), in and out. A virtual camera behind a feature flag (it needs signing; H-15).
- **M5.6 Cameras and capture inputs**
  - Media Foundation and AVFoundation sources as layers, with crop and scale; a fake source for tests.
- **M5.7 Audio routing, playlists and meters**
  - Device enumeration; a routing matrix; per-output delay; playlists with shuffle and crossfade; meters; audio input (used by M8).
- **M5.8 Web pages and web video**
  - CEF offscreen rendering as a layer or background (web pages, YouTube and Vimeo URLs), with page-load handling.
- **M5.9 Outputs UI, health and knowledge base**
  - The outputs screen matching prototype screen 05; per-output health (frame time, drops); a diagnostics panel; output health in Live. About 30 articles.

### Acceptance criteria
- **M5-A1** (auto) The blend ramp matches its analytic curve within tolerance; corner pin and mask golden frames pass.
- **M5-A2** (auto) Colour calibration and test pattern golden frames pass; LED spanning layouts render correctly.
- **M5-A3** (auto) NDI loopback keeps alpha intact (fake backend always; the real SDK when present).
- **M5-A4** (auto) Spout or Syphon loopback works on the current platform.
- **M5-A5** (auto) Audio routing with a null sink sends signal to the right channels; measured per-output delay is within ±1 ms of the setting.
- **M5-A6** (auto) Simulated hot-plug switches profiles; outputs recover with no engine restart.
- **M5-A7** (auto) A local test web page renders as a layer (golden frame).
- **M5-A8** (H) A 3-projector blend has no visible seam. An LED processor shows correct mapping. An NDI output is received in OBS or vMix on the network. A real camera works as a layer.
- **M5-A9** (auto) KB coverage passes; earlier tests pass; `m5.db` is saved.
- **M5-A10** (auto) CEF policy tests pass: no file access, downloads, pop-ups or navigation to other sites. A new NDI source or camera can't reach the screens until it's approved. Outside sources open in preview only. The camera indicator shows whenever a camera layer is active.

### Knowledge base
About 30 articles.

### Security and safety
SEC-08: the CEF sandbox policy. SAFE-02: web pages, NDI and cameras start in preview. SAFE-08: new NDI sources and cameras are approved once. SAFE-09: the camera indicator. SAFE-05: new audio outputs start at a safe level. SEC-13: NDI discovery runs only when NDI is turned on.

### Out of scope
SDI, recording and streaming (M8).

---

## M6. Importers, migration and the alpha release

| | |
|---|---|
| **Goal** | Bring in every common library and presentation format cleanly, guide people through moving from their old app, and ship an installable, updatable alpha. |
| **Depends on** | M3; M4 for services and playlists |
| **Read first** | Product plan §4.5; ADR-018, ADR-020, ADR-015; docs/TESTING-AND-QUALITY.md §2 (fuzz) |
| **Human inputs** | H-05 legal review of importers. H-09 legitimate sample libraries with permission. H-10 a Google OAuth client. H-15 signing certificates (unsigned alpha builds are allowed for internal testing). H-22 alpha churches. |

### Work packages
- **M6.1 Import framework and worker**
  - `selah-import-worker` process; job progress and cancel; per-item results, warnings and fixes; duplicate song detection; media relinking.
- **M6.2 MediaShow importer**
  - Libraries, themes, bundles, and its sermon and Bible databases. The format notes aren't in this repository; ask the user for them when this WP starts (H-24).
- **M6.3 ProPresenter 6 and 7 importers**
  - `.pro6` XML and `.pro` protobuf (from community `.proto` definitions), playlists, themes as templates, best-effort stage layouts.
- **M6.4 Song-app importers**
  - Presenter/WorshipTools exports, EasyWorship 6/7 (legitimate exports only), SongShow Plus, SongBeamer, OpenLyrics, OpenSong, ChordPro, CCLI text and `.usr` files, and plain text.
- **M6.5 Presentation importers and PowerPoint live mode**
  - PowerPoint: OpenXML text, plus slide images through PowerPoint (COM) when it's installed, or LibreOffice headless, with a text-only fallback.
  - PDF (pdfium); Keynote (macOS export); Google Slides (OAuth, re-sync; credential-gated); folders of images.
  - Windows **Launch with PowerPoint** controls a slideshow on the audience screen.
- **M6.6 Migration assistant and import report**
  - A guided flow: choose the old app, find its library, preview, import, then review the report with fix links.
- **M6.7 Alpha release pipeline**
  - The Windows installer, updater and support bundle exist since M1. Add the macOS package, an alpha update channel for both platforms, and signing and notarisation once H-15 is done (through OIDC and a protected environment, never a stored secret while the repository is public).
  - Local crash dumps; importer logs in the support bundle; in-app feedback.
- **M6.8 Import quality gate, fuzzing and knowledge base**
  - `tests/fixtures/import-corpus/` with `PROVENANCE.md`; expected outputs; a corpus metric; fuzz targets for every parser. About 25 articles.

### Acceptance criteria
- **M6-A1** (auto) Golden import tests pass for every format.
- **M6-A2** (auto) On the import corpus, at least 98% of songs need no manual fix (measured against expected outputs).
- **M6-A3** (auto) Each parser has been fuzzed for 10 minutes in a cloud session (corpus committed) and passes a 30-second smoke run in CI, with no crash or hang. A worker crash fails the job cleanly and nothing else is affected.
- **M6-A4** (auto) The support bundle includes importer logs and still contains no secrets (test with seeded fake secrets).
- **M6-A5** (H) PowerPoint import and live mode work on a PC with PowerPoint. Google Slides import works with a real OAuth client.
- **M6-A6** (H) The installer installs, updates and uninstalls cleanly on clean Windows and macOS machines.
- **M6-A7** (auto) KB coverage passes; earlier tests pass; `m6.db` is saved.
- **M6-A8** (H) The closed alpha starts with 10 churches (H-22).
- **M6-A9** (auto) Each worker runs with reduced rights (token and job-object checks on Windows, the sandbox profile on macOS). A crafted archive with `..` paths can't write outside its temporary folder.

### Knowledge base
About 25 articles, plus the "Moving from another app" collection.

### Security and safety
SEC-03: importers run in the worker with limits, with 10-minute fuzz runs in cloud sessions and 30-second smoke runs in CI. SEC-01: worker restrictions. SEC-05: the Google OAuth token lives in the keychain. SEC-12: the alpha channel, and signed builds once H-15 is done.

### Out of scope
Partner APIs (SongSelect, Planning Center), which come in M9.

---

## M7. Remote control, API and show control

| | |
|---|---|
| **Goal** | Let phones, tablets, control surfaces, lighting, timecode and a second computer drive Selah safely. |
| **Depends on** | M4 |
| **Read first** | Product plan §4.6; docs/ARCHITECTURE.md §8; ADR-014; prototype screen 07 |
| **Human inputs** | H-08 a MIDI controller, a Stream Deck, a DMX/sACN node (optional; virtual ports and simulators are used in tests) |

### Work packages
- **M7.1 Gateway, pairing and security**
  - axum HTTPS inside core; a self-signed certificate with its fingerprint in the QR code; device pairing; scoped, revocable tokens; rate limits; off by default and LAN-only; API actions recorded in the op log.
- **M7.2 Public API v1 and OpenAPI**
  - REST and WebSocket for every resource in product plan §4.6, with an OpenAPI document generated from proto annotations and examples.
- **M7.3 Web apps**
  - SvelteKit remote, pastor view, stage display and confidence monitor, matching prototype screen 07. Embedded in core; "?" opens the short article.
- **M7.4 SDKs and Companion module**
  - TypeScript and Python SDKs generated from OpenAPI; a Bitfocus Companion module (actions, feedbacks, presets).
- **M7.5 MIDI**
  - In and out; learn; monitor; mapping to commands; notes for per-label hotkeys.
- **M7.6 OSC, DMX/sACN and network triggers**
  - OSC in and out; DMX over sACN and Art-Net as macro actions; Global Caché-style TCP and IR triggers.
- **M7.7 Timelines and timecode**
  - A timeline editor (cue track and audio sync); MTC and LTC input (chase) and an internal clock; recording cues by triggering.
- **M7.8 Multi-machine sync: backup and campus feeds**
  - A hot-standby backup that mirrors state and media, with manual takeover and automatic takeover on heartbeat loss. Follower mode for campus feeds. This completes the Ready for Sunday backup check.
- **M7.9 Knowledge base content for M7**
  - About 30 articles, including an API guide for IT.

### Acceptance criteria
- **M7-A1** (auto) The OpenAPI document validates; contract tests reach every command and query; every response validates against its schema.
- **M7-A2** (auto) Security: no access without a token; scopes are enforced (viewer can't trigger); revoked tokens fail at once; the API is off by default.
- **M7-A3** (auto) Playwright tests pass for the remote, pastor view and stage apps against a headless core.
- **M7-A4** (auto) MIDI loopback on virtual ports triggers the mapped commands; learn mode captures a note.
- **M7-A5** (auto) Timecode: LTC generated in the test chases with ±1 frame accuracy; timeline cues fire on time.
- **M7-A6** (auto) Failover: 2 engine/core pairs on one machine. When the primary is killed, the backup continues within 1 s; state lag stays under 100 ms during normal running.
- **M7-A7** (auto) The Companion module builds and passes its tests against a mock API.
- **M7-A8** (H) A real phone pairs by QR and runs a service; Companion works with a Stream Deck.
- **M7-A9** (auto) KB coverage passes; earlier tests pass; `m7.db` is saved.
- **M7-A10** (auto) The API fuzz smoke run finds no crash. A new MIDI device or network trigger can't fire commands until it's approved. Nothing a remote device sends reaches the audience screens without an operator, except commands its scope allows.

### Knowledge base
About 30 articles.

### Security and safety
SEC-07: the gateway (M7.1). SEC-14: API scopes. SAFE-01 and SAFE-02: what remote devices send (pastor view notes, requests) goes to the operator, not to the screens. SAFE-08: new remote devices, MIDI devices and network triggers are approved once.

---

## M8. Broadcast, creative and intelligence

| | |
|---|---|
| **Goal** | Add broadcast-grade output (SDI fill and key, recording, streaming, multiview), the full creative library, and the features no competitor has: speech cues, translation lines and Content Packs. |
| **Depends on** | M5 |
| **Read first** | ADR-005, ADR-006, ADR-017, ADR-020; product plan §3 (Outputs, Scripture rows) |
| **Human inputs** | H-11 a DeckLink card with its SDK and an AJA card. Test audio recordings of sermons, with permission. |

### Work packages
- **M8.1 Blackmagic DeckLink**
  - Fill and key output, reference and genlock, multiple outputs, input as a video source. A C++ shim through `cxx`; a device simulator for tests.
- **M8.2 AJA**
  - The same features through libajantv2 (MIT); a simulator for tests.
- **M8.3 Recording**
  - Program and ISO recording with hardware encoders; MP4, MOV and fragmented files; a disk-space guard.
- **M8.4 Streaming**
  - RTMP, RTMPS and SRT to several destinations; encoder profiles; stream keys in the OS keychain; health and reconnect.
- **M8.5 Multiview and stream graphics**
  - A multiview output (program, preview, inputs, outputs); stream-only looks (lower thirds from lyrics and scripture, with alpha); safe areas.
- **M8.6 Full transition and effect library, and the shader SDK**
  - Reach about 30 transitions and 25 effects, all new work.
  - The shader SDK: WGSL with a manifest, parameter schema, hot reload and sandboxing; documented with a sample.
- **M8.7 Speech cues**
  - The `selah-listen` worker with whisper.cpp, running locally. It detects scripture references (grammars for 6 languages) and Content Pack citations, and shows suggestion cards in Live with a confidence level.
  - Off by default, with a visible "listening" indicator. Audio never leaves the computer.
- **M8.8 Translation lines and parallel Bibles**
  - Parallel translations for songs and scripture on chosen outputs (for example, line 2 of a stream lower third); 2 Bible translations side by side.
- **M8.9 Content Packs**
  - A signed pack format; a sermon or text corpus with a citation grammar, search and a slide builder (generalising MediaShow's sermons); a pack manager.
- **M8.10 Knowledge base content for M8**
  - About 25 articles.

### Acceptance criteria
- **M8-A1** (auto) DeckLink and AJA simulators receive correct fill and key frames and timing; an input source works.
- **M8-A2** (auto) Recordings pass ffprobe validation; A/V sync is within ±1 frame; the disk guard stops recording safely.
- **M8-A3** (auto) A 30-minute stream to a local MediaMTX server (RTMP and SRT), run in a cloud session (CI runs a 1-minute version), has no reconnect; a forced disconnect reconnects automatically.
- **M8-A4** (auto) Golden frames pass for every new transition and effect, and the SDK sample loads and hot-reloads.
- **M8-A5** (auto) On the test audio set, explicit scripture references are detected with at least 90% precision and 80% recall; nothing triggers live automatically.
- **M8-A6** (auto) Golden frames pass for translation lines and parallel Bibles; a Content Pack installs, searches and builds slides.
- **M8-A7** (H) DeckLink and AJA fill and key are verified on a real switcher; streaming is verified to a real platform.
- **M8-A8** (auto) KB coverage passes; earlier tests pass; `m8.db` is saved.
- **M8-A9** (auto) With speech cues on, the egress test still shows no outgoing connections. The recording, streaming and listening indicators show whenever those are on. An unsigned or tampered Content Pack is refused. A custom shader that goes over its time budget is stopped.

### Knowledge base
About 25 articles.

### Security and safety
SAFE-04: the flash guard covers the full effect library and custom shaders; strobe-like effects ask for confirmation. Custom shaders are sandboxed, with parameter limits and a GPU time budget. SAFE-09: recording, streaming and listening indicators. SEC-05: stream keys live in the keychain. SEC-13: speech audio stays on the computer. SEC-15: Content Packs are signed. SAFE-02: speech suggestions never go to the screens by themselves.

---

## M9. Cloud, teams and enterprise

| | |
|---|---|
| **Goal** | Let a church, or a multi-campus organisation, share a library safely, sign in with their identity provider, manage people and policies, connect partner services, and deploy and update Selah at scale. |
| **Depends on** | M2, M7 |
| **Read first** | Product plan §4.7; ADR-013, ADR-015, ADR-017; docs/KNOWLEDGE-BASE.md §2, §6.4 (church articles) |
| **Human inputs** | H-12 CCLI agreement and credentials. H-13 Planning Center OAuth app. H-14 MultiTracks and Loop partnerships. H-15 signing certificates. H-16 hosting, domains and email. H-17 a payments account and pricing. |

### Work packages
- **M9.1 Sync server and organisation model**
  - `selah-sync-server` (axum, Postgres, S3/MinIO) with `server/docker-compose.yml`.
  - Organisation → campus → room; an ordered op log; media dedupe by hash; storage quotas.
- **M9.2 Client sync engine**
  - An offline queue; field-level last-writer-wins; conflict copies for concurrent lyric edits; sync status in the UI.
- **M9.3 Accounts, roles, SSO and SCIM**
  - Local and email sign-in; OIDC (Entra, Google, Okta); SCIM 2.0; roles and permissions enforced in core and on the server; device management.
- **M9.4 Admin console, policies, audit log and privacy**
  - In-app admin plus web admin pages.
  - Policies: lock Simple mode, text-size default and minimum, update ring, media upload, API.
  - Activity and audit log; data region; GDPR export and delete.
- **M9.5 CCLI SongSelect and reporting**
  - Search and import; automatic usage reporting; licence numbers. Recorded fixtures until H-12.
- **M9.6 Planning Center, MultiTracks and Loop**
  - Two-way Planning Center Services sync (plans, songs, media); MultiTracks and Loop setlist import. Fixtures until H-13 and H-14.
- **M9.7 Licensing and entitlements**
  - Free, Pro and Enterprise tiers; a licence service with a 30-day offline grace period; payments integration (H-17); a licensed Bible store.
- **M9.8 Enterprise packaging and update rings**
  - WiX MSI, MSIX, ADMX/ADML templates, a signed and notarised macOS pkg with a configuration-profile schema; Early Adopter, Stable and LTS rings; CycloneDX SBOM; reproducible-build check.
- **M9.9 Telemetry, crash reporting and fleet dashboard**
  - Opt-in OpenTelemetry metrics (frame drops, crashes, feature use) to Selah's own endpoint; crash upload with consent.
  - A pilot fleet dashboard that measures the v1.0 reliability row.
- **M9.10 Church articles, handbook export and knowledge base**
  - An admin editor for church articles (same Markdown subset and lint), synced and pinned; PDF export of collections with church articles; admin "help insights" (votes and zero-result searches, opt-in). About 35 articles.

### Acceptance criteria
- **M9-A1** (auto) Sync property test: 5 simulated clients with random offline periods and edits all converge with no lost data; concurrent lyric edits produce conflict copies.
- **M9-A2** (auto) SSO end to end against Keycloak in Docker; the SCIM conformance suite passes; the permission matrix is tested for every role × command.
- **M9-A3** (auto) The audit log records every admin and API action; GDPR export and delete work end to end.
- **M9-A4** (auto) CCLI, Planning Center, MultiTracks and Loop contract tests pass on recorded fixtures.
- **M9-A5** (auto) An upgrade from every earlier milestone's DB fixture and installer build to the current one migrates without data loss.
- **M9-A6** (auto) Telemetry and crash upload send nothing when consent is off (network-capture test) and the expected payloads when it's on.
- **M9-A7** (H) Silent MSI install with ADMX policies works on a domain or Intune test machine; the notarised macOS pkg installs with a configuration profile; real partner integrations work once credentials exist.
- **M9-A8** (auto) KB coverage passes; earlier tests pass; `m9.db` is saved.
- **M9-A9** (H) The closed beta starts with 50 churches, including 5 multi-campus and 5 broadcast-heavy (H-22).
- **M9-A10** (auto) The sync server refuses unauthenticated and cross-organisation requests (a test per endpoint). Each release build has a CycloneDX SBOM, and release builds check update signatures.

### Knowledge base
About 35 articles (admin and IT audience), church articles and handbook export.

### Security and safety
SEC-14: full organisation roles. The sync server gets the same protections as the gateway (TLS, authentication, rate limits, request limits). SEC-05: sign-in and partner tokens live in the keychain. SEC-06 and SEC-13: telemetry and crash upload only with consent. SEC-09: an SBOM for each release. SEC-12: signed builds and update rings.

---

## M10. Launch readiness and v1.0

| | |
|---|---|
| **Goal** | Turn every row of the v1.0 bar green with evidence, then release. |
| **Depends on** | M1–M9 |
| **Read first** | Product plan §3 and §7.1; docs/TESTING-AND-QUALITY.md; docs/KNOWLEDGE-BASE.md §7 |
| **Human inputs** | H-18 2i reviewers. H-19 translators. H-20 an accessibility audit with screen-reader users. H-21 a third-party pen test. H-22 beta metrics and usability studies. H-23 reference hardware. |

### Work packages
- **M10.1 Parity and v1.0-bar checklist**
  - `docs/v1-parity-checklist.md` built from product plan §2–3 and §7.1, with an evidence link per item (test, H-check or study). Close the gaps.
- **M10.2 Knowledge base completion and docs website**
  - Coverage reaches 100%; glossary; generated references (shortcuts, settings); collections; screenshots generated by UI automation in both appearances; "What's new in 1.0".
  - Static docs website with a version switcher; `selah-kb-lint --release` is clean; 2i at 100% (H-18).
- **M10.3 Localisation**
  - es, pt, fr, ko and yo for UI and knowledge base (draft, then native review, H-19); layouts checked with long strings; font coverage.
- **M10.4 Accessibility**
  - A WCAG 2.1 AA pass: automated checks, keyboard-only flows, a high-contrast theme, 200% text; findings from the manual NVDA and VoiceOver audit (H-20) fixed.
- **M10.5 Performance, soak and chaos**
  - Budgets signed off on reference hardware (H-23).
  - A 72-hour soak mode (memory, handles, drift) that you start on the reference PC from **Help ▸ Diagnostics**. Its report is attached to the PR. It never runs in CI (ADR-024).
  - A chaos suite: kill and restart, unplug a display, lose the network, fill the disk. Every P0 and P1 fixed.
- **M10.6 Security**
  - Review the threat model in docs/SECURITY.md, and check that every SEC and SAFE item has passing evidence. Dependency and licence audit; secret scan; longer API fuzzing in cloud sessions; pen-test findings fixed (H-21).
- **M10.7 Release operations and v1.0 sign-off**
  - Release checklist, plain-language release notes, LTS branch policy, support playbooks and status page.
  - The final v1.0-bar sign-off, including the human-run metrics (product plan §7.1); tag `v1.0.0`.

### Acceptance criteria
- **M10-A1** Every item in `docs/v1-parity-checklist.md` has passing evidence.
- **M10-A2** (auto) (ci-win) `tools/verify.sh --release` and `tools/verify.ps1 -Release` exit 0: KB coverage 100%, no drafts, every article reviewed, no stale translations.
- **M10-A3** (auto + H) The UI and knowledge base are complete in 6 languages and reviewed by native speakers.
- **M10-A4** (auto + H) Automated accessibility checks pass, and there are no open AA findings from the manual audit.
- **M10-A5** (H) Performance budgets (ARCHITECTURE §12) pass on reference hardware; the 72-hour soak shows no leaks or drift.
- **M10-A6** (auto + H) The chaos suite passes; there are no open P0 or P1 bugs or security findings.
- **M10-A7** (H) Every product plan §7.1 row is green: 99.9% glitch-free services across ≥ 50 churches over 12 weekends; SUS ≥ 80; volunteer findability ≥ 90%; migration ≥ 98%.
- **M10-A8** `v1.0.0` is tagged and signed, release notes are published, and the update feed is live.
- **M10-A9** (auto + H) Every SEC and SAFE item in docs/SECURITY.md has passing evidence in `docs/v1-parity-checklist.md`.

### Knowledge base
About 15 articles plus every translation; everything reviewed.

### Security and safety
Every SEC and SAFE item is verified end to end, the threat model is reviewed, and the pen-test findings are fixed (H-21).

---

## Appendix A. Human tasks (owner: you)

These can't be done by a coding session. PROGRESS.md tracks their status. Start the long ones (partners, legal, pilots) early.

| ID | Task | Needed by |
|---|---|---|
| H-01 | Approve installing toolchains (Rust, VS Build Tools, LLVM, .NET 10 SDK, pnpm, buf). Only needed if you also code locally; cloud sessions install their own. | M1 (optional) |
| H-02 | Confirm the product name and choose the code licence | M1 (placeholder allowed) |
| H-03 | GitHub remote (done). Before CI can run: clean the history, apply the public-period settings and make the repository public (docs/CLOUD-DEVELOPMENT.md §2.2 and §3) | M1 |
| H-04 | macOS checks run in the `macos` CI job. Provide a Mac only for hands-on macOS testing. | M1 onward (optional) |
| H-05 | Legal review: use of the analysis, interoperability importers (product plan §1) | Before M6 |
| H-06 | Confirm rights for bundled Bible translations | M2 |
| H-07 | Accept the NDI SDK licence and install the NDI runtime | M5 |
| H-08 | Hardware lab: projectors or displays, an LED processor, a capture card, a camera, a MIDI controller, a Stream Deck, a DMX node | M5, M7 |
| H-09 | Legitimate sample libraries from churches, with written permission | M6 |
| H-10 | A Google Cloud OAuth client for Google Slides | M6 |
| H-11 | A Blackmagic DeckLink card and SDK download; an AJA card | M8 |
| H-12 | A CCLI partner agreement and API credentials (start talks in M1) | M9 |
| H-13 | A Planning Center OAuth application | M9 |
| H-14 | MultiTracks and Loop Community partnerships | M9 |
| H-15 | Code-signing: Windows (Azure Trusted Signing or EV) and an Apple Developer ID with notarisation | M6 (alpha), M9 |
| H-16 | Cloud hosting, domains and transactional email for the sync service and docs site | M9 |
| H-17 | A payments account (for example Stripe) and pricing | M9 |
| H-18 | A content designer for 2i reviews of knowledge base articles | M2 onward |
| H-19 | Translators and native reviewers for es, pt, fr, ko, yo | M10 |
| H-20 | A manual accessibility audit with screen-reader users | M10 |
| H-21 | A third-party penetration test | M10 |
| H-22 | Pilot churches: 3 testers (M4), 10 alpha (M6), 50 beta (M9); usability studies | M4–M10 |
| H-23 | Reference hardware (ARCHITECTURE §12, TESTING §6) | M10 (useful from M1) |
| H-24 | Supply the private MediaShow format notes when M6.2 starts (they're kept outside the repository) | M6 |
| H-25 | Set up the Claude Code cloud environment: the setup script and network domains (docs/CLOUD-DEVELOPMENT.md §2.1) | M1 |
| H-26 | Before making the repository private again, choose how test builds and updates are delivered (docs/CLOUD-DEVELOPMENT.md §6) | When going private |
