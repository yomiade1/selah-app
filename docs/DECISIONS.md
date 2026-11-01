# Architecture decision records (ADRs)

Every significant technical or product decision is recorded here. Later documents and code must follow these. To change one, add a new ADR that supersedes it (don't edit an accepted ADR's decision text), and update the affected docs in the same commit.

Format: **Status** · **Context** · **Decision** · **Consequences**.

---

### ADR-001: Separate processes for UI, core and engine
- **Status:** Accepted (2026-10-30)
- **Context:** "The show must go on" (plan §4.1). A crash in the operator UI, a GPU driver or a video decoder must never black out the screens or lose data.
- **Decision:** Four processes. `Selah` (Avalonia UI), `selah-core` (data, command bus, live state, gateway, integrations, help index), `selah-engine` (rendering, outputs, media, audio) and `selah-watchdog` (starts and restarts the others). Heavy or risky jobs run in short-lived **workers** (ADR-020).
- **Consequences:** Every interaction crosses IPC, so the contracts in `proto/` are the system's backbone. The engine holds the last frame on every output if core or UI disappears. The watchdog restarts a failed process in under 2 s, and the restarted process resynchronises from snapshots.

### ADR-002: The core process owns all data
- **Status:** Accepted (2026-10-30)
- **Decision:** `selah-core` (Rust) is the only process that reads or writes the library database, settings, the op log and the help index. The UI and the web apps are clients and never touch files directly.
- **Consequences:** There's one source of truth. Every UI feature needs a command or query in `proto/`. This supersedes the plan's earlier "Selah.Core (C#) models and services" wording; the C# side has generated models plus view models only.

### ADR-003: Protobuf contracts, managed with buf
- **Status:** Accepted (2026-10-30)
- **Decision:** All cross-process messages are defined in `proto/selah/v1/*.proto`. Use `buf` (installed through npm as `@bufbuild/buf`) for lint, breaking-change checks and TypeScript generation (`protobuf-es`). Rust uses `prost` (with `protoc-bin-vendored`); C# uses `Google.Protobuf` plus `Grpc.Tools` codegen. After milestone 1, breaking changes are blocked by `buf breaking` against the previous milestone tag; an exception needs a new ADR.
- **Consequences:** You don't need to install protoc. The public REST/JSON API is derived from the same messages (ADR-014).

### ADR-004: Local IPC transport
- **Status:** Accepted (2026-10-30)
- **Decision:** Named pipes on Windows (`\\.\pipe\selah-{user}-{service}`) and Unix domain sockets on macOS. Frames are a 4-byte little-endian length followed by a protobuf `Envelope`. Bulk pixel data (UI previews, thumbnails) moves through shared-memory ring buffers or the thumbnail cache on disk, never through the pipe.
- **Consequences:** The pipe carries control traffic only. The target is p99 round trip ≤ 2 ms locally.

### ADR-005: Rendering pipeline
- **Status:** Accepted (2026-10-30)
- **Decision:** `wgpu` compositor (D3D12 on Windows, Metal on macOS) with one swap chain per output window. Text and vector elements are rasterised by **Skia** (`skia-safe`, CPU raster with HarfBuzz shaping), cached as textures keyed by content, style and output resolution, and composited on the GPU. Transitions and effects are WGSL shaders. Golden-frame tests use the software adapter (WARP on Windows) for determinism.
- **Consequences:** Text looks the same on both platforms. Slides are pre-rasterised when loaded, so a slide change only swaps textures and lands within one frame. GPU-accelerated Skia can come later behind the same interface.

### ADR-006: Video and audio media
- **Status:** Accepted (2026-10-30)
- **Decision:** FFmpeg 7.x **LGPL shared build** via `ffmpeg-next`, with hardware decode (`d3d11va` on Windows, `videotoolbox` on macOS). Frames are uploaded as NV12/P010 and converted in shaders. No GPL components (no x264/x265). Recording and streaming use hardware encoders (NVENC, QSV, AMF, VideoToolbox). Audio output uses `cpal` with Selah's own mixer, routing matrix and per-output delay.
- **Consequences:** FFmpeg DLLs ship next to the engine with LGPL notices. Zero-copy decode-to-texture is an optimisation for later.

### ADR-007: Desktop UI stack
- **Status:** Accepted (2026-10-30)
- **Decision:** Avalonia (latest stable at M1, pinned) on .NET 10 LTS. CommunityToolkit.Mvvm (source generators), Dock.Avalonia for the Producer workspace, Microsoft.Extensions.DependencyInjection and Logging. Tests use xUnit, Avalonia.Headless and Verify snapshots.
- **Consequences:** One UI codebase for Windows and macOS. Code-behind is limited to view concerns.

### ADR-008: Design language is a USWDS + Apple HIG hybrid
- **Status:** Accepted (2026-10-30)
- **Context:** The user chose this direction over "Calm Studio".
- **Decision:** USWDS decides **meaning**: colour tokens, alerts, tags, step indicators, summary boxes, labels and hints, focus rings and tables. Apple's HIG decides **structure and feel**: toolbars, sidebar source lists, inset grouped lists, segmented controls, switches, large titles and materials. The reference is `design/prototype/`. **No government branding** (no .gov banner, no flag, no "official" wording).
- **Consequences:** `docs/DESIGN-SYSTEM.md` is the spec; the prototype is the visual reference.

### ADR-009: Typography and the text-size setting
- **Status:** Accepted (2026-10-30)
- **Decision:** **Inter** for interface text, **Manrope** for headings and large numbers, **JetBrains Mono** for timecodes and keys. All three are bundled in the app (SIL OFL), never loaded from the web. Fallbacks: Noto Sans KR (Korean), Noto Naskh Arabic, then system fonts. The base size is **15 px** at 100%. A user **Text size** setting has 7 steps (90, 100, 115, 130, 150, 175, 200%) plus "Use my computer's text size". It is saved to the user profile, and admins can set a default and a minimum. It never changes slide or output text.
- **Consequences:** All font sizes and size-dependent spacing come from tokens multiplied by the scale. Layouts must work at 200% (WCAG 1.4.4) and are tested there.

### ADR-010: Localisation with Project Fluent
- **Status:** Accepted (2026-10-30)
- **Decision:** Every user-visible string lives in `content/i18n/<lang>/*.ftl`. The same files serve C# (Linguini), Rust (`fluent-bundle`) and the web apps (`@fluent/bundle`). From M1 there's a pseudo-locale (`en-XA`, strings 40% longer with accents) used in UI tests. Launch languages: en, es, pt, fr, ko, yo.
- **Consequences:** Hard-coded strings fail lint (`tools/lint-strings`).

### ADR-011: Knowledge base as docs-as-code
- **Status:** Accepted (2026-10-30)
- **Decision:** Articles are Markdown with YAML front matter in `content/kb/<lang>/`. The `selah-kb` crate validates them and builds `kb.db` (SQLite FTS5) that ships inside the app, plus a static website from the same source. `selah-kb-lint` enforces plain language (Flesch-Kincaid grade ≤ 6.0 for English), structure, links and coverage. Contextual help connects through declared **help anchors** (`content/help/anchors.yaml`). The full spec is `docs/KNOWLEDGE-BASE.md`.
- **Consequences:** Docs are part of every feature's definition of done. Help works fully offline.

### ADR-012: Storage
- **Status:** Accepted (2026-10-30)
- **Decision:** SQLite through `rusqlite` (bundled, FTS5 on, WAL mode). IDs are UUIDv7. Every mutating command is appended to an `ops` table with its inverse, which provides undo/redo, autosave, audit and sync. Media is stored by BLAKE3 hash. Migrations are append-only (`rusqlite_migration`).
- **Consequences:** Each milestone saves a fixture database (`tests/fixtures/db/m<N>.db`), and every later build must migrate it without loss.

### ADR-013: Cloud sync model
- **Status:** Accepted (2026-10-30)
- **Decision:** A server-ordered op log replicated to clients, with an offline queue. Most fields merge last-writer-wins per field. Concurrent edits to the same lyrics or slide text create a **conflict copy** for a person to resolve, never silent loss. The server is Rust (axum) with Postgres and S3-compatible storage (MinIO for development and self-hosting).
- **Consequences:** Ops must be deterministic and serialisable from M2 onward.

### ADR-014: Gateway, API and web apps
- **Status:** Accepted (2026-10-30)
- **Decision:** `axum` HTTPS server inside `selah-core`. REST and WebSocket at `/api/v1`, described by OpenAPI generated from proto annotations. It's off by default and LAN-only by default. Devices pair by QR code and get scoped tokens (viewer, stage, operator, admin). Web apps (remote, pastor view, stage, confidence monitor) are a SvelteKit static build embedded in core (`rust-embed`).
- **Consequences:** The Companion module and SDKs are generated from the OpenAPI document.

### ADR-015: Packaging and updates
- **Status:** Accepted (2026-10-30). Timing changed by ADR-023 (2026-10-31): the Windows installer and updater now start in M1; the alpha channel, macOS package and signing stay in M6.
- **Decision:** Velopack installers and delta updates from the alpha (M6). Enterprise packaging in M9: a WiX MSI, MSIX, ADMX/ADML policy templates, a signed and notarised macOS pkg with a configuration-profile schema, and update rings (Early Adopter, Stable, LTS).
- **Consequences:** Signing certificates are human task H-15.

### ADR-016: Testing and regression policy
- **Status:** Accepted (2026-10-30). Tool names, the cloud gate and the golden-frame process are updated by ADR-023 and ADR-024 (see TESTING-AND-QUALITY.md).
- **Decision:** `tools/verify.ps1` (and `verify.sh`) is the single gate. The test-count ledger (`tests/ledger.json`) may never decrease without a new ADR. Golden images change only through `tools/bless-golden.ps1 -Reason`. Databases from earlier milestones must keep migrating. `main` is always green. Details are in `docs/TESTING-AND-QUALITY.md`.

### ADR-017: Credential- and hardware-gated features
- **Status:** Accepted (2026-10-30)
- **Decision:** Integrations that need partner credentials (CCLI, Planning Center, Google, MultiTracks, Loop, payments) or hardware (NDI network, DeckLink, AJA, projectors) are built behind a trait or interface, with a **fake** implementation, recorded-fixture contract tests and a feature flag. The real adapter is switched on when the human task is done. Acceptance criteria marked **(H)** need a person with the hardware or account.
- **Consequences:** A milestone can reach "Done, H-checks pending" without waiting for partners.

### ADR-018: Clean-room interoperability
- **Status:** Accepted (2026-10-30)
- **Decision:** No code, shaders, artwork, fonts, UI copy or proprietary schema files from ProPresenter, Presenter (WorshipTools) or EasyWorship. Importers are written from public documentation, community specifications and legitimately obtained sample files. Unlicensed or cracked software and files must never be used, copied or referenced. (Wording generalised on 2026-10-31.)
- **Consequences:** Import fixtures need provenance notes (`tests/fixtures/import-corpus/PROVENANCE.md`).

### ADR-019: Branching, commits and tags
- **Status:** Superseded by ADR-025 (2026-10-31)
- **Decision:** For each milestone, branch `m<N>` from `main`. Commit after each work package (`M<N>.<k>: summary`). When the milestone's verification passes, merge into `main` with `--no-ff` and tag `m<N>`. Never push unless a remote exists **and** the user has asked for pushing.
- **Consequences:** Every milestone is a bisectable, revertible unit.

### ADR-020: Worker processes for risky or heavy jobs
- **Status:** Accepted (2026-10-30)
- **Decision:** Importers (`selah-import-worker`) and speech recognition (`selah-listen`) run as separate worker processes started by core. If one crashes, the job fails cleanly and nothing else is affected.

### ADR-021: Icons
- **Status:** Accepted (2026-10-30)
- **Decision:** Use Lucide icons (ISC licence), bundled as SVG with a 1.75 px stroke, sized from tokens and scaled with Text size. Don't use SF Symbols (licensed for Apple platforms only) or icons copied from other presentation apps.

### ADR-022: Design tokens pipeline
- **Status:** Accepted (2026-10-30)
- **Decision:** `design/tokens/tokens.json` in the W3C Design Tokens (DTCG) format is the single source. Style Dictionary (npm) generates Avalonia resource dictionaries (`Tokens.g.axaml`), CSS custom properties for the web apps, and Rust constants for engine-drawn UI (stage display chrome, test-pattern labels). A unit test checks the contrast pairs declared in the file (DESIGN-SYSTEM §3).

### ADR-023: Cloud-first development and distribution
- **Status:** Accepted (2026-10-31). Changes the timing in ADR-015 and extends ADR-005's golden-frame rule.
- **Context:** The user builds Selah only through Claude Code cloud sessions (an Ubuntu 24.04 VM with 4 vCPUs, 16 GB of memory, and no GPU or display). Testing on a PC should only mean installing an `.exe`.
- **Decision:**
  1. Cloud sessions are the main development environment. `tools/cloud-setup.sh` sets them up (as the environment's setup script, and from a SessionStart hook in `.claude/settings.json`). `tools/verify.sh --cloud` is the session gate.
  2. **Linux is a development and test platform, not a shipped one.** Every Rust crate, the .NET solution and the web apps must build and test on Linux. Platform code sits behind traits that have a Linux implementation or a fake: Unix sockets for IPC, lavapipe (Mesa's software Vulkan) for wgpu, software video decode, an in-memory `SecretStore`, a null audio sink, and fake displays, cameras and devices.
  3. Windows and macOS are verified by GitHub Actions (ADR-024). Windows-only code is also type-checked in the cloud with `cargo check --target x86_64-pc-windows-gnu`.
  4. **Golden frames:** one reference set, rendered on lavapipe. Windows (WARP) and macOS must match it within each test's tolerance. A platform-specific image is added only through the `bless-golden.yml` workflow, with a reason.
  5. **A Windows installer from M1:** a per-user Velopack `Setup.exe` that needs no administrator rights. Every CI run on a PR that changes code uploads it as an artifact (kept 7 days), and each milestone tag publishes it as a GitHub prerelease.
  6. Hardware and performance checks the cloud can't do are built into the app as **Help ▸ Diagnostics** (benchmarks, output tests, device lists). They write a report the user attaches to the PR or an issue.
- **Consequences:** The cloud never sees a GPU or real hardware, so (H) checks move to the installed test build. Supporting Linux in development adds some abstraction. CI minutes matter once the repository is private again.

### ADR-024: Lean, hardened CI
- **Status:** Accepted (2026-10-31)
- **Context:** The repository is made public for a while so GitHub Actions is free, then private again (where minutes are limited and Windows and macOS minutes count extra). The user wants no long-running, scheduled or frequent CI. A public repository's workflows are also an attack surface.
- **Decision:**
  - Three workflows: `ci.yml`, `release.yml` and `bless-golden.yml`. **No `schedule:` trigger, ever.**
  - `ci.yml` runs on pull requests to `main` (opened, new commits, reopened, ready for review, or labelled `ci:macos`), skipping drafts, and on `workflow_dispatch` (at most once per session). Pushing to a branch doesn't run CI.
  - Jobs: `guard` (5 min: workflow policy, secret scan, deciding what to run), `linux` (30 min: the full gate), `windows` (45 min: the gate and the installer), and `macos` (45 min) only with the `ci:macos` label or a dispatch input. Sessions add that label to the PR that completes a milestone. A PR that changes only documents or the design prototype runs `guard` only; a PR that changes only help content adds `linux`.
  - `concurrency` cancels a PR's older run when a new one starts.
  - Long work never runs in CI. Extended fuzzing and 30-minute stream tests run in cloud sessions; soak tests and hardware benchmarks run on the user's PC through Diagnostics. CI fuzzing is a 30-second smoke run per target.
  - Caches: Cargo, NuGet and build output (`actions/cache`). Artifacts: the installer (7 days) and failure reports (3 days) only.
  - Failures are printed as annotations (`::error file=…,line=…::`), so sessions can read them through the REST API (`tools/ci.sh status`). Job logs are stored on hosts a cloud session may not reach.
  - **Hardening (SEC-10):** top-level `permissions: {}` and the least permission per job; only GitHub-owned actions, pinned to full commit SHAs; `persist-credentials: false`; no `pull_request_target`, `workflow_run` or self-hosted runners; no secrets while public; event text reaches scripts only through `env:`. `tools/check-workflows.py` enforces this in `guard` and in the gate.
- **Consequences:** Windows and macOS feedback arrives with the PR, not with each commit, so the Linux gate in the session must stay honest. A docs-only PR passes quickly, because skipped jobs count as passed checks.

### ADR-025: Pull-request workflow
- **Status:** Accepted (2026-10-31). Supersedes ADR-019.
- **Decision:**
  - Sessions **never** push to `main`, merge a pull request, force-push a shared branch, or create tags.
  - Each session starts its own branch from the latest `main` (the session's branch name is fine). Commit after each work package (`M<N>.<k>: summary`) and push whenever useful.
  - When the session's work is done (the milestone is complete, or the session stops after finishing whole work packages), the session runs the gate, opens **one** PR with `tools/open-pr.sh` using `.github/pull_request_template.md`, waits for CI with `tools/ci.sh status --wait`, fixes any failures, and reports the PR link and the installer link to the user.
  - Only one agent PR is open at a time. A session that finds an unmerged agent PR asks the user to merge it first, unless the user says to build on top of it.
  - The user reviews and merges with a **merge commit**, which keeps each work package's commit for bisecting. Review comments are handled by a new session on the same PR branch, or in a follow-up PR if it can't push there.
  - After a merge, `release.yml` tags `m<N>` and publishes a prerelease with the installer for each milestone that PROGRESS.md marks Done and that has no tag yet.
  - While the repository is public, a ruleset on `main` requires a PR and the `guard`, `linux` and `windows` checks, and blocks force-pushes and deletion (docs/CLOUD-DEVELOPMENT.md §2.2).
- **Consequences:** The user is the only way into `main`. Milestones span as many PRs as they need sessions. Cloud sessions can't use `gh pr` (it needs GraphQL), so the scripts use the REST API through `gh api`.

### ADR-026: Secure and safe by default
- **Status:** Accepted (2026-10-31)
- **Decision:** The requirements in [SECURITY.md](SECURITY.md) (SEC-01 to SEC-15 and SAFE-01 to SAFE-09) are part of every milestone's definition of done. M1 builds the baseline (M1.12). Each later milestone delivers the items mapped to it, with tests. A feature that can put something on screen, make sound, open a port, read files from outside or store a secret isn't done until its SEC and SAFE items pass. Security and safety bugs are P0 or P1 and block merging.
- **Emergency lane:** the UI may send exactly two commands straight to the engine, **Clear everything** and **Show logo**, over a dedicated pipe, so the screens can be cleared even if core is frozen. This is the only exception to the rule that the UI talks only to core (ADR-001, ADR-002). The engine reports the change to core when core is back, and core records it in the op log.
- **Consequences:** Some extra work in every feature. The threat model in SECURITY.md is reviewed at the end of each milestone.
