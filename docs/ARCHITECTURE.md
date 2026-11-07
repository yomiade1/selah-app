# Selah architecture

This is the technical reference for building Selah. The product plan explains **why**; this document explains **how the parts fit**. Decisions referred to as ADR-nnn are in [DECISIONS.md](DECISIONS.md). If code and this document disagree, fix one of them in the same commit.

## 1. Repository map

The `Selah/` folder is the repository root.

```
Selah/
├─ CLAUDE.md                     session instructions (milestone protocol)
├─ BUILD-PLAN.md                 10 milestones → v1.0
├─ PROGRESS.md                   live status ledger (what's done, what's next)
├─ SECURITY.md                   how to report a vulnerability
├─ CHANGELOG.md                  created in M1
├─ Church-Presentation-Platform-Plan.md   product plan
├─ docs/
│  ├─ ARCHITECTURE.md  DESIGN-SYSTEM.md  KNOWLEDGE-BASE.md
│  ├─ TESTING-AND-QUALITY.md  DECISIONS.md  SECURITY.md  CLOUD-DEVELOPMENT.md
│  └─ specs/                     detailed specs written as each milestone builds them
├─ design/
│  ├─ prototype/                 HTML visual reference (USWDS + Apple hybrid)
│  └─ tokens/tokens.json         design tokens, single source (DTCG format), M1
├─ proto/selah/v1/               protobuf contracts (ADR-003)
├─ crates/                       Rust workspace (Cargo.toml at repo root)
│  ├─ selah-proto/               generated Rust types (prost)
│  ├─ selah-ipc/                 pipes/UDS framing, client/server, shared memory rings, emergency lane
│  ├─ selah-net/                 the single gate for outgoing network connections (SEC-13)
│  ├─ selah-secrets/             SecretStore: Credential Manager, Keychain, in-memory fake (SEC-05)
│  ├─ selah-runtime/             shared plumbing: paths, instance lock, redacted logs, heartbeats, keep-awake
│  ├─ selah-core/                library: data store, command bus, op log, live state, search, updater, support bundles
│  ├─ selah-core-host/           binary `selah-core`: hosts core + gateway + integrations
│  ├─ selah-render/              wgpu compositor, Skia text, templates, transitions, effects
│  ├─ selah-media/               FFmpeg decode, clocks, audio mixer
│  ├─ selah-outputs/             windows, NDI, Spout/Syphon, DeckLink, AJA, recorder, streamer
│  ├─ selah-engine/              binary `selah-engine`: render + outputs + media host
│  ├─ selah-io/                  MIDI, OSC, DMX/sACN, timecode
│  ├─ selah-gateway/             axum REST/WebSocket API, pairing, web app hosting
│  ├─ selah-import/              importers (used by the worker)
│  ├─ selah-import-worker/       binary: runs one import job (ADR-020)
│  ├─ selah-listen/              binary: speech recognition worker (M8)
│  ├─ selah-kb/                  knowledge base parse/validate/index/query
│  ├─ selah-kb-lint/             binary: plain-language and coverage lint
│  ├─ selah-watchdog/            binary: process supervisor
│  ├─ selah-sync-server/         binary: cloud sync service (M9)
│  ├─ selah-bench/               binary: frame-pacing and decode benchmarks
│  └─ selah-testkit/             golden-frame harness, fixtures, headless engine helpers
├─ app/                          .NET solution `Selah.sln`
│  ├─ Selah.Client/              IPC client, generated proto types, typed command/query API
│  ├─ Selah.UI/                  Avalonia app: views, view models, styles, help components
│  ├─ Selah.UI.Generators/       source generator: tokens, help anchors, string keys
│  └─ Selah.UI.Tests/            headless UI tests + snapshots
├─ web/
│  ├─ apps/                      SvelteKit: remote, pastor view, stage, confidence monitor
│  ├─ sdk-ts/                    generated TypeScript SDK (M7)
│  └─ companion-module/          Bitfocus Companion module (M7)
├─ content/                      shipped content (all text is plain-language, see KB doc)
│  ├─ kb/<lang>/<area>/*.md      knowledge base articles
│  ├─ kb/media/                  article images (generated screenshots under generated/)
│  ├─ help/anchors.yaml          help anchor registry
│  ├─ help/tours/*.yaml          guided tours
│  ├─ errors/errors.yaml         error catalogue (code → message → fix article)
│  ├─ i18n/<lang>/*.ftl          UI strings (Fluent)
│  ├─ terms.yaml                 preferred terms and banned synonyms
│  ├─ bibles/                    public-domain translations (after rights check H-06)
│  └─ samples/                   public-domain sample songs and a sample service
├─ tools/                        verify, setup, cloud-setup.sh, open-pr.sh, ci.sh, check-workflows.py,
│                                check-secrets.py, check-unsafe, package.ps1, bless-golden, ledger, codegen
├─ tests/
│  ├─ e2e/                       scripted service runs against headless core + engine
│  ├─ golden/<platform>/         reference frames (+ CHANGES.md)
│  ├─ fixtures/                  songs, media (generated), db/m<N>.db, import-corpus/
│  └─ ledger.json                minimum test counts per suite (ADR-016)
├─ server/                       docker-compose for sync server, Postgres, MinIO, Keycloak (M9)
├─ packaging/                    Velopack (M1), WiX, MSIX, ADMX, macOS pkg (M6, M9)
├─ third_party/                  downloaded SDKs/binaries (git-ignored), see §10
├─ .claude/settings.json         session-start hook that sets up cloud sessions (ADR-023)
└─ .github/                      workflows (ci, release, bless-golden), PR and issue templates (ADR-024, ADR-025)
```

## 2. Process model (ADR-001)

```
                 ┌────────────────────────────┐
                 │ selah-watchdog             │ starts, monitors (heartbeat 250 ms), restarts < 2 s
                 └──┬──────────┬──────────┬───┘
                    │          │          │
     ┌──────────────▼───┐ ┌────▼─────────────────┐ ┌──────────────────────────┐
     │ Selah (UI)        │ │ selah-core           │ │ selah-engine             │
     │ Avalonia / .NET   │ │ Rust                 │ │ Rust                     │
     │ views, view models│ │ store, command bus,  │ │ compositor, outputs,     │
     │ help panel        │ │ live state machine,  │ │ media decode, audio,     │
     │                   │ │ gateway (HTTPS/WS),  │ │ NDI/SDI/Spout, recorder, │
     │                   │ │ integrations, KB     │ │ streamer                 │
     └────────┬──────────┘ └───┬─────────────┬────┘ └───────────▲──────────────┘
              │ core pipe       │ workers     │ engine pipe      │
              └────────────────►│ (import,    └──────────────────┘
                                │  listen)
   shared memory: engine → UI preview rings; thumbnail cache on disk
   web apps / API clients ──HTTPS/WSS──► selah-core gateway
```

- **UI ↔ core:** commands, queries and an event subscription. The UI never talks to the engine except for reading preview frames from shared memory, and the emergency lane.
- **Emergency lane (ADR-026, SAFE-03):** a second, tiny pipe from the UI to the engine that accepts only `ClearEverything` and `ShowLogo`. The engine handles it ahead of other work, so the screens can be cleared even if core is frozen, and tells core afterwards.
- **Core → engine:** `RenderCommand`s (set the scene for an output, preload, transition, clear, configure outputs). The engine sends back `EngineEvent`s (frame stats, output health, media ended, errors).
- **Failure behaviour:**
  - If the UI dies, nothing changes on screen. The UI restarts and resubscribes.
  - If core dies, the engine keeps the last frame. Core restarts, reloads its live-state snapshot (`live_state.bin`, written on every change) and re-sends the scene.
  - If the engine dies, the watchdog restarts it. Core re-sends the scene and the outputs come back. A future goal is a frozen-frame handover.
- **Single instance:** a named mutex or lock file per user stops two copies of core.
- **Safe mode (SAFE-07):** after 3 crashes in 60 seconds, or with Shift held at start, the watchdog starts everything with web layers, custom shaders, the gateway, network sources and hardware output drivers turned off.
- **Platforms (ADR-023):** Windows and macOS ship. Linux runs the same processes for development and tests only, with lavapipe, Unix sockets and fakes.

## 3. Contracts (ADR-003, ADR-004)

`proto/selah/v1/`:

| File | Contents |
|---|---|
| `envelope.proto` | `Envelope { uint64 id; uint64 correlation_id; oneof body { Command, Query, Event, Reply, Error } }`, plus `Actor` (user, device, origin: ui/api/midi/macro/timeline/sync/tour) |
| `common.proto` | IDs (UUIDv7 as 16 bytes), timestamps, colour, rect, enums |
| `library.proto` | Song, Section, Arrangement, Template, MediaItem, Presentation, Slide, Tag; their commands and queries |
| `service.proto` | Service, ServiceItem, ServiceType, Team; commands and queries |
| `live.proto` | LiveState, Layer, Look, Prop, Message, Timer, Macro, ClearGroup, StageLayout; live commands (`TriggerSlide`, `Next`, `Previous`, `ClearLayer`, `ClearAll`, `SetLook`, `ShowProp`, `ShowMessage`, `StartTimer`…) |
| `outputs.proto` | DisplayProfile, Output, geometry (blend, corner pin, mask, colour), NDI/SDI/stream configs |
| `render.proto` | Core → engine `RenderCommand` and `EngineEvent` |
| `help.proto` | KB queries: `SearchHelp`, `GetArticle`, `HelpForContext`, feedback |
| `settings.proto` | Typed settings with policy (user, machine, org, locked) |
| `errors.proto` | `Error { string code; string message_key; map<string,string> args; string fix_article_id; }` |

Rules:
- Every command is **validated** in core, **applied** as an op (with an inverse), and **broadcast** as events.
- Commands are idempotent when they carry the same `Envelope.id`.
- Core checks the `Actor`'s permission for every command before validating it (SEC-14).
- Pipes accept only the current user's processes, and frames over 16 MiB are rejected (SEC-02).
- Field numbers are never reused, and `buf breaking` runs in verify.

## 4. Data model and storage (ADR-012)

### 4.1 Locations

| | Windows | macOS |
|---|---|---|
| Config and settings | `%APPDATA%\Selah\` | `~/Library/Application Support/Selah/` |
| Cache (thumbnails, previews) | `%LOCALAPPDATA%\Selah\cache\` | `~/Library/Caches/Selah/` |
| Library (default, policy can move it) | `Documents\Selah Library\` | `~/Documents/Selah Library/` |
| Logs | `%LOCALAPPDATA%\Selah\logs\` | `~/Library/Logs/Selah/` |

The library folder contains `library.db`, `media/<blake3[0..2]>/<blake3>.<ext>`, `bibles/*.db`, `packs/` and `backups/`.

### 4.2 Main tables (library.db)

`songs`, `song_sections`, `song_arrangements`, `templates`, `template_variants`, `media_items`, `media_tags`, `presentations`, `slides`, `services`, `service_items`, `service_types`, `looks`, `look_rules`, `props`, `messages`, `timers`, `macros`, `macro_actions`, `stage_layouts`, `clear_groups`, `display_profiles`, `outputs`, `settings`, `org_articles` (M9), `ops`, `sync_state` (M9), plus FTS5 tables `songs_fts`, `media_fts`.

`ops(seq INTEGER PRIMARY KEY, op_id BLOB UNIQUE, ts INTEGER, actor BLOB, origin TEXT, doc_type TEXT, doc_id BLOB, command BLOB, inverse BLOB, undo_group BLOB, sync_status INTEGER)`

### 4.3 Songs and slide generation

```
Song ─ sections[] (label, kind: verse|chorus|bridge|tag|intro|outro|other, color, hotkey,
                   lines[] with chords[] (position, chord), translations{lang → lines[]})
     ─ arrangements[] (name, section_ids[])
     ─ metadata (title, authors, ccli_number, copyright, key, tempo, tags, public_domain)

SlideGeneration(song, arrangement, template_variant, output_profile) →
   1. Rule split  (M2): break at sections, then every `max_lines` lines; keep chords aligned.
   2. Metric fit  (M3): measure with Skia using the template's style; shrink down to the
                         template's minimum size; if it still doesn't fit, split again; report "fits" per output.
   3. Decorate    (M2/M3): copyright line rules (first/last/all), title slide, blank end slide, translation line.
```

Generated slides are cached by content hash and never stored as hand-edited data. Custom slides (presentations) are stored.

## 5. Live command flow

```
Key press / remote / MIDI / macro / timeline
   → Command(TriggerSlide{service_item, slide_index}) → core validates
   → live state machine updates LiveState (layers, current/next) → op + inverse logged (undo in Live)
   → RenderCommand(SetScene{output, layers…, transition}) → engine (pre-rasterised textures)
   → swap on next vsync → EngineEvent(FrameShown{output, frame_time}) → core → UI/API events
```

The latency budget from command receipt to photons is one frame plus display latency. The engine timestamps every `FrameShown` so end-to-end tests can assert it.

## 6. Engine internals

- **Scene per output:** an ordered layer stack (video input, media, slide, props, messages), each layer holding content, transform, opacity and transition state.
- **Texture cache:** LRU keyed by (content hash, style hash, resolution). Slides for the current and next service items are pre-rasterised.
- **Outputs:** borderless winit windows on assigned displays, plus virtual outputs (NDI, Spout/Syphon, SDI, recorder, streamer), each with an output-processing stage (colour, blend, corner pin, mask).
- **Clocks:** a monotonic master clock. Video presents at the nearest vsync; audio is master for A/V sync.
- **Headless mode:** `--headless` renders to offscreen targets for tests (`selah-testkit`).

## 7. UI internals

- MVVM with CommunityToolkit.Mvvm. View models subscribe to core events through `Selah.Client`.
- **Design tokens** come from `design/tokens/tokens.json` and are generated into `Selah.UI/Styles/Tokens.g.axaml` (colours) and `Tokens.g.cs` (sizes) by Style Dictionary (`pnpm tokens`, ADR-022); the gate checks they're current. Font sizes, control heights and spacing are `DynamicResource`s that `TextScale` recalculates when Text size changes (ADR-009).
- **Strings** are bound through `Localizer` (`{Binding [key], Source={x:Static s:Localizer.Instance}}`); `en-XA` is a Linguini transform of English. Critical labels carry `views:Critical.IsCritical`, which the UI tests check for clipping.
- **Headless:** `Selah --headless` (or `SELAH_HEADLESS=1`) runs the real UI with Avalonia's headless platform, so the watchdog chaos tests and the installer check use it on machines with no display.
- **Help components:**
  - `help:Help.Anchor="live.layers"` attached property
  - `HintText`
  - `InfoButton` (popover)
  - `HelpPanel` (F1)
  - `WhatsThisOverlay` (Shift+F1)
  - `EmptyState`
  - `ErrorAlert` (with a "How to fix" link)
  - `CoachMark` (tours)
  
  Anchors are constants generated from `content/help/anchors.yaml`.
- **Modes:** Simple and Producer are separate view trees over the same view models. Role policy decides which can be opened.
- **Previews:** a `SharedFrameSource` reads engine preview rings into `WriteableBitmap` at 15–30 fps.

## 8. Gateway and web apps (ADR-014)

- Routes: `/api/v1/...` (REST), `/api/v1/ws` (event stream), `/app/*` (SvelteKit build), `/help/<id>` (short article view for web apps).
- Auth: QR pairing produces a device token (scoped role, revocable). Admin tokens are created only in the desktop UI.
- TLS: a self-signed certificate per installation; its fingerprint is pinned in the QR payload.

## 9. Knowledge base subsystem (ADR-011)

The build step, run by `selah-kb build`, validates `content/kb/**` and `content/help/**`. It then produces `kb.db`, which holds the FTS5 index, a synonym table, articles as an AST in JSON, and an anchor→article map. `kb.db` ships with the app and core serves it through `help.proto`. Organisation articles (M9) live in `library.db` and are merged into results at query time. The full spec is in [KNOWLEDGE-BASE.md](KNOWLEDGE-BASE.md).

## 10. Third-party components

| Component | Use | Licence | Obtained by |
|---|---|---|---|
| wgpu, winit, skia-safe | rendering | MIT/Apache, BSD | Cargo |
| FFmpeg 7.x (LGPL shared) | decode/encode | LGPL-2.1 | `tools/setup` downloads to `third_party/ffmpeg` |
| rusqlite (bundled SQLite) | storage | MIT, public domain | Cargo |
| tokio, axum, prost, serde, tracing | runtime | MIT/Apache | Cargo |
| cpal, midir | audio, MIDI | Apache/MIT | Cargo |
| whisper.cpp (`whisper-rs`) | speech (M8) | MIT | Cargo + model download |
| NDI SDK 6 | NDI I/O (M5) | NDI SDK licence | **H-07**, dynamic load |
| Blackmagic Desktop Video SDK | DeckLink (M8) | Blackmagic SDK licence | **H-11** |
| AJA NTV2 (libajantv2) | AJA SDI (M8) | MIT | Git submodule/download |
| Spout2 / Syphon | texture sharing (M5) | BSD | download |
| CEF (Chromium Embedded) | web page layers (M5) | BSD | download |
| pdfium | PDF import (M6) | BSD/Apache | download |
| Avalonia, Dock.Avalonia, CommunityToolkit.Mvvm | UI | MIT | NuGet |
| Linguini / fluent-rs / @fluent/bundle | localisation | MIT/Apache | NuGet/Cargo/npm |
| Inter, Manrope, JetBrains Mono, Noto Sans KR, Noto Naskh Arabic | fonts | SIL OFL 1.1 | `tools/setup` downloads to `third_party/fonts` |
| SvelteKit, Vite, Vitest, Playwright | web | MIT/Apache | npm (pnpm) |
| Velopack, WiX | packaging (Velopack from M1, WiX in M9) | MIT, MS-RL | NuGet/dotnet tool |
| Mesa lavapipe | software Vulkan for tests on Linux (not shipped) | MIT | apt (`tools/cloud-setup.sh`) |

`third_party/` is git-ignored. `tools/setup` fetches pinned versions and checks SHA-256 hashes listed in `tools/third_party.lock.json`.

## 11. Conventions

- **Rust:**
  - Edition 2024; toolchain pinned in `rust-toolchain.toml`.
  - `clippy -D warnings`.
  - `thiserror` in libraries, `anyhow` in binaries, `tracing` for logs (JSON, rolling daily, 14 days kept).
  - Every crate has `#![forbid(unsafe_code)]`, except FFI crates listed in `tools/unsafe-allowlist.txt`, where each `unsafe` block has a `// SAFETY:` comment (SEC-11).
- **C#:**
  - .NET 10, pinned in `global.json`.
  - `Nullable` enabled and `TreatWarningsAsErrors`.
  - No business logic in views.
  - Every interactive control has `AutomationProperties.Name` and, where relevant, a help anchor.
- **TypeScript:** `strict`, ESLint, no `any` without a comment.
- **Strings:** never hard-coded in UI (ADR-010).
- **Colours and sizes:** never hard-coded. Use tokens.
- **Errors:** every user-facing error uses a catalogue code (`SEL-<AREA>-<NNNN>`) with a plain message and a fix article.
- **Time:** UTC internally; user-facing times use the locale.
- **Paths:** never embed absolute developer paths in builds (`--remap-path-prefix`, `PathMap`).
- **Network:** every outgoing connection goes through `selah-net` (SEC-13). **Secrets:** only through `SecretStore` (SEC-05). **Parsing:** limits on size, depth, count and time (SEC-03).
- **Platform code:** behind a trait with a Linux implementation or a fake, so everything builds and tests in cloud sessions (ADR-023).

## 12. Performance budgets (plan §4.7)

| Budget | Target | Checked by |
|---|---|---|
| Slide change to next vsync | ≤ 1 frame | e2e frame timestamps (M4) |
| Output frame time | p99 ≤ 8 ms at 1080p60, 2 outputs + 4K60 video | `selah-bench` on reference hardware (H-23) |
| UI ↔ core round trip | p99 ≤ 2 ms | IPC bench (M1) |
| Song search, 10k songs | ≤ 50 ms | criterion bench (M2) |
| Cold start to usable UI | ≤ 4 s | startup trace (M10) |
| Restart after crash | ≤ 2 s, last frame held | chaos tests (M1, M10) |

## 13. Security and safety

The threat model and every security (SEC) and safety (SAFE) requirement, with the milestone that delivers it, are in [SECURITY.md](SECURITY.md). The decisions are ADR-024 (CI) and ADR-026 (secure and safe by default).
