# Unified church presentation platform: analysis of three products and rebuild plan

Working name in this document: **Selah**. Swap in the real name later.

This plan defines Selah: a new cross-platform church presentation app that combines the strengths of today's leading tools (ProPresenter, Presenter by WorshipTools, and MediaShow) and leaves out their weaknesses.

---

## 1. Sources and confidentiality

- This plan draws on product research into the three apps above. The detailed research notes are kept **privately, outside this repository**. This document records only feature-level conclusions and Selah's own decisions.
- Selah is a **clean-room** product (ADR-018). It uses no code, artwork, fonts, shaders, proprietary schemas or interface text from other products.
- Importers are built from public documentation, community specifications and legitimately obtained sample files, after legal review (H-05).
- Product names are used only to describe compatibility, for example "imports from ProPresenter".

---

## 2. The three products at a glance

| | MediaShow | Presenter (WorshipTools) | ProPresenter |
|---|---|---|---|
| **Positioning** | A native Mac app with strong Bible and sermon tools for one church tradition | Free, cloud-first, made for volunteers | The industry standard; broadcast-grade and very deep |
| **Platforms** | macOS | Windows, macOS | Windows, macOS |
| **Best at** | Sermon and Bible content, a clean layer model, output calibration | Planning services and entering songs fast, with licensing handled for you | Production depth, outputs and ecosystem |

### 2.1 What each one does best

**MediaShow**
- **Content:** a deep sermon and Bible library with citation search, and building slides from a passage.
- **Layers:** a clean layer model (video input, media, slide, props, messages, audio) with custom clear groups.
- **Outputs:** looks, test patterns, colour calibration, edge blend and corner pin.
- **Compatibility:** reads ProPresenter files; a reflow editor for whole songs.

**Presenter (WorshipTools)**
- **The service is the centre of the app:** service types, upcoming and past services, reusing last week's service, printing.
- **Fast song entry:** paste lyrics, automatic splitting into labelled slides, lines per slide, slide orders, title and blank slides, ChordPro charts.
- **Wide import support:** other presentation apps, song formats, Bibles, PowerPoint, Keynote, PDF and Google Slides.
- **Licensing handled for you:** CCLI SongSelect search and import, automatic usage reporting, copyright display, lyric videos, setlist sync with multitrack apps.
- **Cloud library sync,** a getting-started tour, and either converting PowerPoint or launching it to keep animations.
- **Clear keys operators expect:** F10 clears text, F11 the background, F12 everything.

**ProPresenter**
- **Production depth:** looks, props, messages, macros, timelines and timecode, clear groups, a stage editor, masks, themes, reflow, arrangements, undo.
- **Outputs:** screen assignment, edge blend, corner pin, alpha key over SDI, NDI, recording, streaming, test patterns.
- **Ecosystem:** Planning Center, SongSelect, MultiTracks, a media marketplace, a Bible store, a REST API, multi-machine sync and cloud workspaces.
- **Craft:** a large library of real-time effects and transitions, and strong library search.

### 2.2 Weaknesses to avoid

- **Presenter:** browser-based rendering limits timing precision and broadcast output (no SDI or key), and much of the app depends on being online.
- **ProPresenter:** a steep learning curve, settings spread across many windows, and a premium price.
- **MediaShow:** Mac only, and built around one tradition's content.

---

## 3. Feature matrix and decisions

Key: ● strong, ◐ partial, ○ absent or not observed. The **Selah** column says where each feature comes from. Under the rule in section 7, every row ships in v1.0.

| Area | Feature | MS | WT | ProP | Selah decision |
|---|---|---|---|---|---|
| **Planning** | Services / playlists as the main object | ◐ | ● | ● | **WT model**: Service → Items, with Service Types and templates. v1.0. |
| | Planning Center sync | ○ | ○ | ● | v1.0: two-way plan, song and media sync. |
| | Team-shared library (multiple sites) | ◐ | ● | ● | v1.0: cloud library with offline-first local cache. |
| **Songs** | Paste lyrics → auto-split and label | ◐ | ● | ◐ | **WT Autoflow** plus improved automatic detection of Verse/Chorus/Bridge. v1.0. |
| | Arrangements / slide orders | ● | ● | ● | v1.0. |
| | Reflow editor (whole song as one text block) | ● | ○ | ● | v1.0. |
| | ChordPro and chord charts on stage | ○ | ● | ● | v1.0 (stage screen shows chords). |
| | SongSelect import + CCLI auto-reporting | ◐ | ● | ◐ | **WT model**: auto-report by default. v1.0 (needs a partner agreement). |
| | Copyright footer automation | ○ | ● | ◐ | v1.0: per-template footer, first/last slide rules. |
| **Scripture** | Bible search, reference entry, verse cards | ● | ● | ● | v1.0: public-domain translations bundled. |
| | Licensed translations | ○ | ● | ● | v1.0: through publisher agreements. |
| | Import of OSIS, USFM, Zefania XML | ○ | ◐ (XML) | ○ | v1.0. |
| | Two translations side by side | ○ | ○ | ◐ | v1.0. |
| | Sermon library + citation parsing | ● | ○ | ○ | **MS**, generalised into "Content Packs" (any sermon or catechism corpus). v1.0. |
| | Live speech → suggested scripture | ○ | ○ | ○ | **New**: local Whisper. v1.0. |
| **Slides and media** | Templates / themes with role-based slots | ● | ● | ● | v1.0: one unified "Template" concept (no separate theme vs template). |
| | Editor: text, shapes, media, web, timers, masks | ● | ◐ | ● | v1.0, including masks. |
| | PowerPoint / Keynote / PDF / Google Slides | ◐ | ● | ◐ | v1.0: convert, plus a Windows "live PowerPoint" mode. |
| | Media bin with tags, cloud upload, video trim | ● | ● | ● | v1.0. |
| | Effects and transitions (GPU) | ◐ | ◐ | ● | v1.0: about 30 transitions and 25 effects, chosen for taste, plus a shader SDK so the community can add more. All shaders written new. |
| **Live** | Layer stack + Clear All, per-layer, Clear Groups | ● | ◐ | ● | **MS layer model** + **WT F10/F11/F12** defaults. v1.0. |
| | Looks (screen × layer presets) | ● | ○ | ● | v1.0. |
| | Props, Messages (tokens), Countdowns / Clocks | ● | ◐ | ● | v1.0. |
| | Macros / slide actions | ● | ◐ (MIDI) | ● | v1.0. |
| | Timelines and timecode (MTC) | ○ | ○ | ● | v1.0. |
| | Stage display editor + chords + notes | ● | ◐ | ● | v1.0. |
| | Undo/redo everywhere, autosave, crash recovery | ◐ | ◐ | ● | v1.0, non-negotiable. |
| **Outputs** | Multiple screens, edge blend, corner pin, colour calibration | ● | ○ | ● | v1.0. |
| | Test patterns | ● | ○ | ● | v1.0. |
| | NDI in/out (with alpha) | ● | ◐ | ● | v1.0. |
| | SDI (Blackmagic, AJA) with fill and key | ◐ | ○ | ● | v1.0 (Blackmagic and AJA). |
| | Texture sharing (Spout/Syphon), virtual camera | ● | ○ | ◐ | v1.0. |
| | Recording + RTMP/SRT streaming | ● | ○ | ● | v1.0 (HW encoders). |
| **Control** | MIDI in/out with learn | ● | ● | ● | v1.0. |
| | Browser remote + stage on any device (QR pairing) | ○ | ○ | ◐ (apps) | **New**: v1.0, no app install. |
| | Public REST + WebSocket API | ◐ | ○ | ● | v1.0: OpenAPI-documented, versioned. |
| | Bitfocus Companion / Stream Deck module | ○ | ○ | ◐ | v1.0 (built on the API). |
| | OSC, DMX/sACN, Global Caché-style triggers | ○ | ○ | ◐ | v1.0. |
| | Multi-machine sync (Network Link) | ◐ | ○ | ● | v1.0. |
| **Platform** | Windows + macOS from one codebase | ○ | ● | ◐ | **v1.0.** |
| | Guided tour + sample service | ◐ | ● | ◐ | v1.0. |
| | Simple/Producer mode | ○ | ○ | ○ | **New**: v1.0 (section 5). |
| | Enterprise admin (SSO, roles, MDM, audit log) | ○ | ◐ | ◐ | **New**: v1.0 (section 4.7). |
| | Built-in knowledge base + contextual help (offline, plain language) | ◐ (tour) | ◐ (tour, web links) | ◐ (online help) | **New**: v1.0 (section 6). |

---

## 4. Architecture

### 4.1 Guiding rules

1. **The show must go on.** The output engine runs in its **own process**. If the operator UI crashes, freezes or updates, the screens keep showing the last frame and recover without a black flash.
2. **One engine, one truth.** Every preview, multiview, NDI feed, recording and remote thumbnail is a downscaled copy of real engine output, never a second renderer, so what you preview is exactly what people see.
3. **Offline-first.** Everything works with no internet. The cloud adds sync, sharing and licensing but never blocks a service.
4. **Everything is a command.** The UI, the remote, the API, MIDI, macros and timelines all send the same typed commands over one command bus. That's how undo, the audit log, scripting and remote control stay consistent.
5. **Open, documented formats.** Users own their data. Use JSON plus a media folder, with a published schema.

### 4.2 Process and module layout

```
┌───────────────────────── Selah Operator (UI process) ─────────────────────────┐
│ Avalonia UI (.NET 10 LTS) · MVVM · docking workspace · Simple/Producer modes  │
│ Library · Services · Editor · Bible · Content Packs · Live · Settings · Help   │
└──────────────┬───────────────────────────────────────────────▲────────────────┘
               │ Command bus (local IPC: named pipe / UDS, protobuf) │ State events
┌──────────────▼───────────────────────────────────────────────┴────────────────┐
│ Selah Core (separate process, Rust): the only owner of data                    │
│  • Library store (SQLite), op log (undo, autosave, audit, sync), search        │
│  • Live state machine · gateway (REST/WebSocket) · integrations · help index   │
└──────────────┬────────────────────────────────────────────────────────────────┘
               │ Render commands / engine events
┌──────────────▼────────────────────────────────────────────────────────────────┐
│ Selah Engine (separate process, Rust)                                          │
│  • Scene graph & layer stack per screen  • Look resolver  • Transition/FX GPU  │
│  • wgpu (D3D12 / Metal / Vulkan)  • Skia for text & vector (HarfBuzz shaping)  │
│  • Video: Media Foundation / VideoToolbox HW decode, FFmpeg 7 fallback         │
│  • Audio: WASAPI / CoreAudio, routing matrix, per-output delay                 │
│  • Outputs: windows, NDI, DeckLink, Spout/Syphon, recorder, RTMP/SRT           │
│  • Timers, clocks, timeline/timecode scheduler (frame-accurate)                │
└──────────────┬────────────────────────────────────────────────────────────────┘
Watchdog: starts and restarts UI, core and engine (< 2 s); the engine holds the last frame meanwhile.
Workers: importers and speech recognition run in short-lived separate processes.
```

**Why this stack:**

| Choice | Reason | Alternatives considered |
|---|---|---|
| **Rust engine with wgpu** | One GPU codebase for Windows (D3D12) and macOS (Metal). Memory safety in the process that must never crash. Strong FFmpeg and NDI bindings. | C++ with bgfx (ProPresenter-like, more crash risk). The earlier plan's C# + D3D11 (Windows-only). |
| **Skia for text** | Text fidelity is the main risk to visual parity (see the earlier plan). Skia + HarfBuzz renders identically on both platforms, with complex scripts and right-to-left text, which matters for Yoruba, Arabic and Korean congregations. | DirectWrite/CoreText (two different behaviours), HTML (Presenter's approach, with weak timing). |
| **Avalonia + .NET 10 for the UI** | One UI codebase for Windows and macOS. A mature MVVM ecosystem, fast to build, and C# talent is easy to hire. ProPresenter's WPF shows .NET suits this kind of UI. | WPF (Windows only), Electron (memory use and weaker native integration), SwiftUI + WinUI (two codebases). |
| **SQLite + full-text search (SQLite FTS5, or Tantivy later)** | Proven and widely used. FTS5 is enough for v1; move to Tantivy if fuzzy and faceted search needs it. | |
| **Protobuf on the command bus** | Typed, versioned and fast. The same schema generates the public API's JSON. | |

The detailed design (processes, contracts, data model, pipelines, conventions) is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), and every decision is recorded in [docs/DECISIONS.md](docs/DECISIONS.md). Splitting **core** (data, live state, API) from the **engine** (rendering) means a GPU or video-driver crash can't take down the library or the remote control (ADR-001).

### 4.3 Repository layout

```
Selah/             (this folder is the repository root; full map in docs/ARCHITECTURE.md §1)
  proto/           .proto contracts for commands, queries, events and the document model
  crates/          Rust: core, core host, render, media, outputs, engine, io, gateway,
                   import (+ worker), kb + kb-lint, watchdog, sync server, bench, testkit
  app/             C#: Selah.Client (IPC + generated types), Selah.UI (Avalonia), tests
  web/             SvelteKit: remote, pastor view, stage, confidence monitor; SDK; Companion
  content/         knowledge base, help anchors, tours, error catalogue, UI strings, samples
  design/          prototype (visual reference) and design tokens
  docs/            architecture, design system, knowledge base spec, testing, decisions
  tools/ tests/    verify gate, setup, golden frames, e2e, fixtures, test ledger
```

### 4.4 Document model (simplified)

```
Library ─┬─ Song (metadata: title, authors, CCLI #, copyright, key, tempo, tags)
         │    ├─ Sections [{id, label: Verse 1|Chorus|…, color, hotkey, lines[], chords[]}]
         │    └─ Arrangements [{name, sectionIds[]}]
         ├─ Presentation (free slides: PowerPoint/PDF/Google/custom)
         ├─ Template (role slots: body, reference, title, footer/copyright; per-aspect variants)
         ├─ MediaItem (file hash, trims, loop, fit, tags, cloud state)
         ├─ BibleTranslation / ContentPack (indexed corpus + citation grammar)
         └─ Prop / Message / Timer / Macro / Look / StageLayout / ClearGroup
Service ── Items [{type: song|scripture|media|presentation|countdown|web|camera|
                   header|audio, ref, overrides(template, transition, arrangement), notes}]
```

Songs are **structured text plus a template**, not hand-drawn slides. Slides are generated: when a template changes, line limits change, or the screen changes from 16:9 to an LED wall, every song reflows automatically. This is Presenter's biggest usability win, and it scales better than ProPresenter's per-slide model. Custom slides remain available for free-form presentations.

**File format**: a `.selah` bundle (a zip of `document.json` + `media/` + `manifest.json`) with a published JSON Schema and golden round-trip tests.

### 4.5 Importers (all in v1.0)

| Source | Notes |
|---|---|
| MediaShow libraries, themes and bundles | Format notes are supplied privately when M6 starts (H-24). |
| ProPresenter 6 (`.pro6` XML) and 7 (`.pro`, protobuf) | Use the community `.proto` definitions; never ship Renewed Vision's. Import playlists, themes as templates, and stage layouts best-effort. |
| EasyWorship 6/7 | Only legitimately obtained sample exports are used as fixtures (H-09). |
| SongShow Plus, SongBeamer, OpenLyrics, OpenSong, ChordPro, plain text and clipboard | |
| PowerPoint (OpenXML), PDF (pdfium), Keynote (macOS, through export), Google Slides (OAuth, re-sync) | |
| Bibles: OSIS, USFM, Zefania XML, plus a licensed-translation download store | |
| SongSelect (API), Planning Center (OAuth), MultiTracks / Loop setlists | Partner agreements needed. |

### 4.6 Public API and remote

- **REST + WebSocket** at `https://<host>:<port>/api/v1`. Resources: `services`, `items`, `slides/trigger`, `layers/clear`, `looks`, `props`, `messages`, `timers`, `macros`, `stage/layouts`, `media`, `audio`, `outputs`, `status`. This covers everything a church's control systems need, under Selah's own design, plus subscription streams for live state.
- **Auth**: pair a device by QR code to get a scoped token (roles: *viewer*, *stage*, *operator*, *admin*). Use TLS with a self-signed certificate pinned at pairing.
- **The web remote, stage display and confidence monitor** are served by the gateway, so any phone, tablet or TV browser works with no app install.
- A Companion / Stream Deck module and a Node/Python SDK are generated from the OpenAPI spec.

### 4.7 What "enterprise-grade" means concretely

| Concern | Implementation |
|---|---|
| **Reliability** | Separate engine process plus a watchdog. Restart in under 2 s with the last frame held on screen. Autosave every change (event-sourced edit log). Crash dumps uploaded only with consent. A 72-hour soak test on every release candidate. |
| **Performance budgets** | Slide change visible on the next vsync (16.7 ms at 60 Hz, 8.3 ms at 120). 4K60 H.264/HEVC background plus 1080p NDI in plus 2 outputs on a mid-range GPU with no dropped frames. Cold start under 4 s. Searching 10k songs under 50 ms. |
| **Multi-site and teams** | Organisation → Campuses → Rooms. Shared libraries with per-campus overrides. Role-based access (Admin, Producer, Operator, Volunteer, Viewer). Activity log ("Last modified by", as Presenter has). |
| **Identity** | Local accounts plus OIDC SSO (Microsoft Entra, Google Workspace, Okta), and SCIM provisioning for large churches and schools. |
| **Deployment** | Signed MSIX and MSI with silent-install switches and Intune/GPO policies (lock settings, force update ring); signed and notarised macOS pkg with an MDM configuration profile; Early Adopter / Stable update rings (Presenter has an opt-in; make it policy-controlled). |
| **Security** | Secure and safe by default from the first milestone ([docs/SECURITY.md](docs/SECURITY.md)). Stream keys and OAuth tokens in DPAPI / Keychain. The API is off by default and LAN-only by default. A signed update feed. An SBOM per release. Reproducible builds with no developer paths embedded. Third-party pen test before 1.0. |
| **Observability** | A local diagnostics panel (frame timing, dropped frames per output, GPU/CPU, NDI health) and an exportable support bundle. Telemetry is opt-in and documented. |
| **Data protection** | Data stays in the region you choose (EU/US). A GDPR export and delete. Media upload can be turned off for privacy-sensitive organisations. |
| **Accessibility and localisation** | WCAG 2.1 AA operator UI, screen-reader labels, high-contrast theme, keyboard-only operation. All UI strings externalised from day 1. Launch languages: EN, ES, PT, FR, KO, YO. |
| **Support** | Diagnostics bundle, an in-app "What's new", a status page, an LTS branch for churches that freeze versions. |
| **Help and documentation** | A bundled, offline knowledge base with contextual help throughout the app, written at a Grade 5–6 reading level, versioned with the app, searchable, with church-specific articles and printable handbooks (section 6). |

---

## 5. UX: easy by default, powerful when needed

### 5.1 Two modes, one app

| | **Simple mode** (default for new installs and volunteer roles) | **Producer mode** |
|---|---|---|
| Layout | Three columns: Service, then Slides, then Live preview with big Next and Prev buttons | Dockable workspace: library, service, slides, layers, looks, props, timers, stage, multiview, macros, audio |
| Visible controls | Clear Text (F10), Clear Background (F11), Clear All (F12), Logo, Countdown | Everything, including Looks, Clear Groups, per-layer transport, timeline |
| Settings | 6 essentials (screens, default template, Bible, CCLI, cloud, language) | Full settings, organised by task with search |
| Who can switch | Anyone, unless an admin has locked it | Producer and Admin roles |

ProPresenter's power stays available but hidden until someone needs it. Presenter's approachability is the default.

### 5.2 Main flows to make fast (target times for a first-time volunteer)

1. **Build Sunday's service** (under 5 minutes): "New service from template" brings in last week's structure ("inherit from previous", as Presenter does), then you search for songs and add them; SongSelect results appear inline.
2. **Add a new song** (under 60 seconds): paste lyrics; sections are detected; the arrangement is suggested; copyright fields are filled in from CCLI.
3. **Add scripture** (under 15 seconds): type `jn 3 16-18` and press Enter; it's split across slides using the template's line limits.
4. **Run the service**: arrow keys or clicker; a persistent "Next up" strip; output health dots (green, amber, red) per screen.
5. **Recover from a mistake**: Ctrl+Z works in Live too (it re-triggers the previous state), with a visible history.

### 5.3 Visual language (decided)

- **Direction:** a **USWDS + Apple Human Interface Guidelines hybrid** (ADR-008).
  - USWDS decides meaning: colour tokens, alerts, tags, step indicators, summary boxes, labels and hints, focus rings, tables.
  - Apple decides structure and feel: toolbars, sidebar lists, inset grouped lists, segmented controls, switches, large titles, translucent materials.
  - No government branding.
- **Typography:** **Inter** for interface text and **Manrope** for headings and big numbers (JetBrains Mono for timecodes), bundled with the app. The base size is 15 px, slightly bigger than typical desktop apps (ADR-009).
- **Text size is a setting:** 90% to 200% in 7 steps, or "use my computer's text size". It's saved to the user profile and admins can set a default and a minimum. It never changes the words on the screens. Layouts are tested at 200%.
- **Appearances:** Light for planning and editing; **Booth** (dark) for live work; plus high contrast and reduced motion.
- **Specs:** [docs/DESIGN-SYSTEM.md](docs/DESIGN-SYSTEM.md). Visual reference: [design/prototype/](design/prototype/index.html), 8 screens including settings and contextual help.
- **Testing:** run usability tests with 5 to 8 church volunteers on the prototype before M4's live UI work.

---

## 6. Knowledge base and contextual help

None of the three incumbents ships complete, offline, plain-language help inside the app. Selah will, and it's part of the v1.0 bar. Full spec and style guide: [docs/KNOWLEDGE-BASE.md](docs/KNOWLEDGE-BASE.md).

### 6.1 What ships

- **A bundled knowledge base** of about 260 articles at v1.0, in 6 languages (English, Spanish, Portuguese, French, Korean, Yorùbá).
  - It works fully offline and is versioned with the app.
  - Search understands everyday words and questions ("why is my screen black"), synonyms and typos.
  - Article types: tasks, explanations, fixes, references, quick tips, checklists, glossary.
- **Contextual help in five levels**, placed where people need it:
  1. A hint under every setting (one short sentence).
  2. An info button (ⓘ) that opens a short popover with a link to the full article.
  3. A help panel (F1) showing articles for the current screen, with search and feedback.
  4. "What's this?" mode (Shift+F1): select any control to learn what it does.
  5. Collections (Volunteer handbook, Producer guide, Admin and IT guide, Moving from another app) that print to PDF.

  These are added to by **guided tours**, **practice mode** (nothing goes to the real screens), empty states that teach, and error messages and Ready-for-Sunday checks that link straight to the fix. Help is also in the command palette (Ctrl+K) and in the phone remote.
- **Live safety:**
  - Help never opens by itself during a service.
  - It never covers the program preview or the Next and Clear controls.
  - It never captures live keyboard shortcuts.
  - Tours only run in practice mode while anything is live.
- **Church articles (enterprise):** admins write their own procedures, such as "Our Sunday checklist". These are labelled "From your church", pinned, synced and included in printed handbooks. Admins can hide articles for features their church has turned off.

### 6.2 How it's written

- **Plain language at a Grade 5–6 reading level**, following the content design approach Sarah Winters (formerly Richards) established at the UK Government Digital Service:
  - start from the user's need;
  - put the important words first;
  - use short sentences and common words;
  - number the steps with one action each;
  - use the exact words on the screen;
  - no FAQs;
  - a second person reviews everything ("2i").
- The same rules apply to every piece of text in the interface.
- Three example articles in `content/kb/en/` set the bar. They score between Grade 1 and Grade 4.

### 6.3 How quality is enforced

- **Docs as code:** articles are Markdown files in the repository, built into the app and into a docs website from the same source.
- **Automatic checks on every build:**
  - reading grade ≤ 6;
  - sentence and paragraph length;
  - banned words;
  - broken links;
  - bold UI labels that don't exist in the app;
  - missing alt text;
  - coverage: every screen, setting and error must have help.
- **Human review:** no article is published without a second-person review. Screenshots are generated from the real app for each release, so they never go stale.
- **Learning loop:** "Did this help?" votes and searches that found nothing are stored locally. They're shared only if the church opts in.

---

## 7. Phase plan

> **Rule: v1.0 must beat ProPresenter, Presenter and MediaShow on power, reliability and ease of use at launch.** There's no public MVP. Everything before v1.0 is a closed alpha or beta with pilot churches. Nothing ships publicly until it passes the v1.0 bar in section 7.1.

The team assumed (larger, because v1.0 now has to match ProPresenter's depth): 4 Rust engine engineers (render, video, I/O, timing), 5 C# UI engineers, 2 web/integration engineers, 2 designers (product + UX research), 2 QA/automation engineers, 1 SRE/security engineer, 1 PM and 1 partnerships lead, plus **2 content designers** (the knowledge base, all interface text, and 2i review) and 1 localisation coordinator. Durations are calendar estimates.

**Engineering execution** is broken into 10 milestones in [BUILD-PLAN.md](BUILD-PLAN.md), tracked in [PROGRESS.md](PROGRESS.md). The phases below map onto them: Phase 0 is M1, Phase 1 is M2–M6, Phase 2 is M7–M9, and launch is M10.

### 7.1 The v1.0 bar (every item is a release blocker)

| Dimension | v1.0 must… | How it's measured |
|---|---|---|
| **Power** | Ship every feature marked v1.0 in section 3: everything ProPresenter's Windows build has that churches use (Looks, Props, Macros, Timelines/timecode, Masks, Clear Groups, edge blend, corner pin, NDI with alpha, Blackmagic and AJA SDI fill and key, recording, RTMP/SRT, multi-machine sync, REST API, MIDI, DMX), plus everything Presenter has (service planning, autoflow, CCLI auto-reporting, broad importers, cloud sync, guided tour). | A parity checklist built from sections 2 and 3, signed off feature by feature against real ProPresenter and Presenter installs in a test lab. |
| **Beyond them** | Ship at least five things none of the three has: live scripture cue from speech, songs that fit every screen, browser remote and pastor view, the "Ready for Sunday" pre-flight check, Simple/Producer modes, and an engine that survives a UI crash. | Demo script; each item is used in pilot services. |
| **Reliability** | Run a full service with no output glitch at least 99.9% of the time across the pilot fleet. The UI can be killed and restarted with no visible break on screen. 72-hour soak test with zero leaks or drift. | Fleet telemetry (opt-in) from at least 50 pilot churches over 12 weekends; automated chaos tests in CI. |
| **Ease of use** | A first-time volunteer runs a service in Simple mode after 5 minutes of training. A new producer builds a full service in under 10 minutes. SUS score of at least 80 (ProPresenter and Presenter are each benchmarked in the same study). | Moderated usability studies with at least 30 participants; side-by-side task-timing tests against both incumbents. |
| **Migration** | Import a ProPresenter 6/7, Presenter, EasyWorship or MediaShow library with at least 98% of songs needing no manual fix. | Import corpus of at least 20 real church libraries (with permission). |
| **Help and documentation** | Every feature, screen, setting and error has contextual help and a knowledge base article. Every article reads at Grade 6 or below, has had a second-person review, and is available in all 6 launch languages. A first-time volunteer finds the answer to a common question in under 60 seconds. | Automated coverage and readability checks (release mode); 2i log; findability tasks (≥ 90% success) in the usability studies. |

### Phase 0: risk spikes (8 weeks)
- **Engine**: a wgpu + Skia text slide on two monitors, Windows and macOS, compared pixel-for-pixel across platforms (Yoruba diacritics, Korean, Arabic).
- **Video**: 4K60 HEVC loop under text plus a 1080p NDI input with no drops for 8 hours.
- **Process split**: kill the UI process and confirm the outputs hold; kill the engine and confirm the watchdog restarts it with no visible black frame on the projector.
- **Formats**: import a sample song library, round-trip it through the `.selah` format, and diff the output.
- **Exit gate**: performance budgets met; final go/no-go on Rust + Avalonia.

### Phase 1: core build, closed alpha (7 months)
- Library, songs (paste, autoflow, arrangements, reflow, chords), templates that fit every screen, Bible (public-domain translations, OSIS/USFM/Zefania import), media bin, audio playlists and routing.
- Service planning (types, templates, inherit, print, pre-flight check).
- Live: full layer stack, F9–F12, Looks, Props, Messages, Countdowns/Clocks, Macros, Clear Groups, stage editor with chords and notes, test patterns, undo in Live.
- Outputs: multiple screens, edge blend, corner pin, masks, colour calibration, NDI in/out with alpha, Spout/Syphon.
- Importers: MediaShow, ProPresenter 6/7, Presenter, EasyWorship, SongShow Plus, OpenLyrics, ChordPro, text, PowerPoint, PDF.
- Browser remote, pastor view and stage over QR pairing; REST/WebSocket API (full read and write); MIDI learn; Companion module.
- Simple and Producer modes, guided tour, practice mode, autosave, crash recovery, watchdog, signed installers and auto-update.
- The help system (help panel, info popovers, hints, "What's this?", tours) and knowledge base articles for every Phase 1 feature.
- **Gate**: 10 alpha churches run 6 consecutive Sundays with no show-stopping bugs; the volunteer task-timing targets in section 5.2 are met.

### Phase 2: broadcast and team depth, closed beta (6 months)
- Blackmagic and AJA SDI fill and key, recording, RTMP/SRT with hardware encoders, multiview.
- Timelines and timecode (MTC/LTC), OSC, DMX/sACN cues.
- Multi-machine sync: hot-standby backup operator and campus feeds over the same command bus.
- Cloud org/campus library sync (offline-first, conflict resolution, media dedupe by hash).
- SongSelect + CCLI auto-reporting, Planning Center two-way sync, MultiTracks and Loop setlists, Google Slides, Keynote.
- Live scripture cue from speech (local Whisper) and translation lines on songs and scripture.
- Content Packs (sermon corpora with citation search, generalising MediaShow's sermon feature).
- OIDC SSO, roles, audit log, MSI/MSIX/pkg with MDM policies; pen test, accessibility audit, localisation into 6 languages.
- Church articles, printable handbooks, and knowledge base articles for every Phase 2 feature; every article reviewed and translated before launch.
- **Gate**: 50 beta churches, including 5 multi-campus and 5 broadcast-heavy, pass every row of the v1.0 bar in section 7.1.

### Public launch: v1.0
Launch only when section 7.1 is fully green. The total is about 13 months after Phase 0, roughly 15 months from kickoff with the larger team. If a v1.0-bar item slips, move the date; don't cut the item.

### After v1.0 (ranked by value)
1. Live AI translation captions (speech to second-language subtitles) on the stream and secondary screens.
2. Scripting (sandboxed JavaScript or Lua) on the command bus; a plugin marketplace.
3. AI-assisted lyric clean-up and an automatic arrangement from a MultiTracks or Loop setlist.
4. ATEM/HyperDeck control bridge, NDI|HX3 optimisation, Linux engine build for appliance use.

---

## 8. Risks and decisions you need to make

| # | Risk or decision | Recommendation |
|---|---|---|
| 1 | **Intellectual property** | Clean-room implementation (ADR-018); legal review of interoperability importers (H-05); detailed competitive research kept private, outside the repository. |
| 2 | **Bundled content rights** (sermon libraries, songbooks, Bible translations) | Ship only public-domain content in the base install. Licensed corpora become Content Packs, each with a written licence. |
| 3 | **Partner agreements**: CCLI SongSelect and reporting, Planning Center, MultiTracks, Loop, NDI, Blackmagic SDK | Start conversations in Phase 0. They take months. |
| 4 | **Rust + Avalonia skills** on the team | Phase 0 is the decision point. The fallback is a C++ engine with the same IPC, or a Windows-only WPF UI. |
| 5 | **Text-rendering parity** with imported ProPresenter/MediaShow slides | Accept "faithful, not identical" for imports; offer one-click "convert to template-driven song". |
| 6 | **Cloud cost and privacy** | Offline-first, media dedupe by hash, an optional self-hosted sync server for enterprise (S3-compatible storage plus Postgres). |
| 7 | **Business model** | Suggested: a free tier for one screen with Simple mode (competes with WorshipTools), Pro per room (competes with ProPresenter), and Enterprise per organisation (SSO, MDM, multi-campus, SLA). |
| 8 | **Unlicensed software or files** | Never used as fixtures, references or tools; only legitimately obtained samples (ADR-018). |
| 9 | **Help content goes out of date, or drifts above Grade 6** | Docs as code with build-blocking checks (readability, UI labels that must exist, coverage), screenshots regenerated each release, and 2i review before publishing. |
| 10 | **AI-drafted articles published without a person checking them** | Drafts stay `status: review` until a content designer signs off; release builds refuse unreviewed articles. |

---

## 9. Next steps

1. Legal review of section 1 and section 8, risks 1 to 3.
2. Approve the Phase 0 team and budget, then start building: set up the cloud environment ([docs/CLOUD-DEVELOPMENT.md](docs/CLOUD-DEVELOPMENT.md)), start a Claude Code cloud session and say **"Complete milestone 1"** (see [CLAUDE.md](CLAUDE.md)). Each session ends with a pull request for you to review and merge.
3. Record operator walkthroughs of the leading apps to settle the remaining interaction questions.
4. Recruit 10 pilot churches across sizes and traditions, including one Yoruba-language congregation to exercise text shaping.
5. Usability-test the prototype in [design/prototype/](design/prototype/index.html) with volunteers before M4's live UI work.
6. Hire or assign a content designer early (H-18), to review the knowledge base articles each milestone produces.
7. Start the long-lead human tasks now: partner agreements (CCLI, Planning Center, MultiTracks, Loop), signing certificates, Bible rights and pilot churches (BUILD-PLAN Appendix A).
