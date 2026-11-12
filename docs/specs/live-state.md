# Live operation: services, the live state machine, automation, stage and practice (M4)

Milestone 4 lets a church plan a service and run it live. Core owns all live state
(`crates/selah-core/src/live.rs`); the UI only sends commands and draws what core reports
(`LiveStatus`); the engine draws the scenes core sends. This spec describes what M4 built.

## Services and the planner (M4.1)

- **Contracts:** `proto/selah/v1/service.proto`. A `Service` has items of 8 kinds
  (`ServiceItemKind`: song, scripture, media, presentation, countdown, header, note, audio; web
  and camera arrive in M5), an optional `ServiceType`, a `Team` and notes.
- **Commands:** `SaveService`, `DeleteService`, `SaveServiceType`, `SaveTeam` and their `Get`/
  `List` queries; `CreateServiceFromPrevious` ("Inherit from last week": copies the chosen or
  latest service of the same type, a week later, keeping songs); `ProjectServiceRuntime` (start
  and end times from item lengths); `ExportServicePrint` (HTML and a multi-page PDF with
  transposed chord-over-lyric sheets; the output path is checked, SEC-04).
- **UI:** `ServicePlannerView` matches prototype screen 04. The copy's name comes from
  `services-inherited-name`.

## The live state machine (M4.2)

- **State:** `LiveState` holds the loaded service, the current and next item and slide, five
  layer slots (`Messages`, `Props`, `Slide`, `Media`, `VideoInput`), the logo flag, audio and
  the automation state. Every change rebuilds one `Scene` with a new revision and sends it to
  the engine.
- **Moves:** `TriggerItem`, `TriggerSlide`, `NextSlide`, `PreviousSlide`, `NextItem`,
  `PreviousItem` (headers are skipped), `TriggerSectionHotkey` (per-label hotkeys),
  `ClearLayer`, `SetLayerState`, `ShowLogoCommand`, `PlayAudioLayer`, `StopAudioLayer`.
- **Undo in Live:** `LiveUndo` / `LiveRedo` re-trigger the exact previous state; the frame
  checksum after undo equals the earlier one (M4-A2).
- **Media stays:** a media item fills the media layer, and later items draw over it until the
  layer is cleared (F11 or a clear group), as an operator expects.
- **Crash recovery:** `live_state.bin` (the scene) and `live_machine.bin` (the machine) are
  written atomically on every change. A restarted core reads them and resends the same scene,
  so the screens don't change (ADR-001). A corrupt snapshot starts cleared.

## Looks, props, messages, timers, macros and clear groups (M4.3, M4.9)

- **Contracts:** `proto/selah/v1/live.proto`; migration `004_live_and_services.sql`. Each kind
  has `Save…`, `Delete…`, `Get…` and `List…`.
- **Look:** a `LookRule` per output index lists the layers that output shows, with an optional
  template override and mask. An output with no rule shows every layer. Stage outputs ignore
  looks (they draw the stage layout).
- **Prop:** a few words (`label`) over the slide on the Props layer; one at a time; optional
  `auto_clear_seconds`.
- **Message:** `template_text` with `{token}` interpolation (`{timer}` and `{countdown}` are the
  running timer's time; other tokens come from `default_tokens` or the show request), a
  duration, and `stage_only`. Values are bounded (32 tokens, 64-byte keys, 256-character values,
  no NUL).
- **Timer:** count down, count up, or count down to a time of day, on core's virtual clock. At
  zero it stops, keeps counting past zero, or runs a macro (fired once). Drift is ≤ 1 frame over
  60 minutes (M4-A4). The core host ticks the clock once a second while a timer runs.
- **Macro:** up to 32 ordered `MacroAction`s (15 kinds). A macro may not run another macro.
  Macros run under the `Macro` origin, which may only do live actions (SEC-14).
- **Clear group:** a set of layers to clear, plus stop audio, stop timers and hide the logo.
- **Set up tiles (M4.9):** the Producer tab `TilesEditorViewModel` makes and changes all six
  kinds. It checks names (≤ 80), keys (one letter or digit; looks are Shift+key), words (≤ 200),
  minutes (1–600), the time of day (`h:mm`) and that a macro has steps; core checks again
  (SEC-03). Each saved one becomes a tile in **Looks, timers and macros**: looks, props and
  messages light up while on; selecting a lit prop or message tile hides it.

## Producer and Simple mode (M4.4, M4.5)

- **Producer:** a Dock.Avalonia workspace (ADR-002): service order, slides, program, and an
  inspector with **Layers**, **Looks, timers and macros**, **Set up tiles** and **Stage** tabs.
  Panes can't close or float, so the live controls can't be lost. Layouts save as Selah's own
  bounded JSON. Keyboard map: `LiveKeys` (never from a text box).
- **Simple:** prototype screen 02 with big buttons (≥ 64 px), **Message the booth** (≤ 200
  characters, reaches only the producer's UI) and **Quick tips**.
- **Role lock (SEC-14):** `SetRoleLock` keeps the lock in core (policy `live.role-lock`); the PIN
  hash (salted BLAKE3) is in `SecretStore`; 5 wrong tries wait 30 s. A missing PIN record
  unlocks (logged) rather than trapping the church in Simple mode.
- **Booth lock (SAFE-06):** Ctrl/Cmd+L; only Next, Previous, Clear and Show logo work.
- **Emergency clear (SAFE-03):** F12, a held Esc (1 s) or **Clear everything** use the priority
  lane to the engine and work in every Live view, when locked, and while practising.

## Stage display (M4.6)

`crates/selah-core/src/stage.rs` composes Stage-role outputs from the active `StageLayout`:
current words with chords, song, key and section chips, next slide, notes, the booth message
(Slide layer), and the clock and running timer (Props layer, redrawn each second). Layouts are
checked on save (rects within the screen, bounded sizes; SEC-03).

## Ready for Sunday, practice, setup and tours (M4.7)

- **Ready for Sunday:** `crates/selah-core/src/ready.rs`, 9 checks, each with a `SEL-RDY-*` code
  and a fix article (`selah-kb-lint` enforces the link).
- **Practice mode:** `SetPracticeMode`. Live changes go to the engine as a preview scene only,
  nothing is saved, and leaving restores the checkpoint with a newer revision.
- **First-run setup** and **Settings ▸ Church** (M4.9) set `church.ccli-licence` and
  `scripture.default-translation` (machine-level policy settings).
- **Tours:** `content/help/tours/*.yaml`, words in `tours.ftl`; a tour refuses to start while
  the screens are live unless practice mode is on (M4-A9).

## Engine behaviour M4 relies on (M4.8)

- **Warming upcoming slides:** core sends upcoming slides as `PreloadHint`s with each change.
  The engine queues them (at most 64, repeats dropped, oldest first out), draws one per loop
  pass and only while no new scene or clear is waiting, and skips hints already in the overlay
  cache. A Next never waits behind cache warming.
- **What status says:** when the engine disconnects, core forgets its frames
  (`on_engine_connection(false)`), so status never says the screens are restored before a new
  engine reports a frame. A reconnecting engine resends its last frames at once.
- **GPU checks:** debug builds turn on wgpu validation unless `SELAH_GPU_VALIDATION=0`; release
  builds never do. The process-stack tests turn it off so test engines start like shipped ones.

## End-to-end and chaos tests (M4.8)

`crates/selah-testkit/tests/service_e2e.rs` runs the real processes headless, one stack at a
time:

- **12-item service (M4-A1):** 5 public-domain hymns, 2 KJV readings, 2 countdowns, the committed
  1080p clip and 2 headers at 640×360. After a warm-up pass, every Next shows on the frame after
  the one being drawn (at most 1 older frame after the command), every slide has its own
  picture, and re-showing a slide gives the same checksum. On a real GPU
  (`SELAH_TEST_GPU=auto`) each change also lands within 2 frame periods; the software adapter
  only checks for stalls (2 s), because it takes ~25 ms a slide and ~250 ms a frame with 1080p
  video behind the words.
- **50 random kills (M4-A5):** a seeded generator (`SELAH_CHAOS_SEED`) kills the UI, core or
  engine 50 times while a service runs, waiting for the UI to finish starting between kills.
  UI and core kills never change the engine's picture; every engine restart brings back the
  same picture; safe mode ends with **Restart normally** without changing the screens; and
  Next works after every kill. Engine restarts are timed and every one over 2 s without a
  watchdog back-off is reported; more than 5 s (plus the back-off) fails the run.

## Measured (this milestone)

| What | Where | Result |
|---|---|---|
| Slowest slide change, 12-item service | MX350 GPU, release | 24.85 ms (budget 33.3 ms) |
| Slowest slide change, 12-item service | WARP software adapter | 240–370 ms (stall check only) |
| Engine restart, kill to restored picture | WARP, debug engine | 1.7–2.1 s: OS teardown ~0.45 s, GPU start ~1.0 s, first frame ~0.15 s |
