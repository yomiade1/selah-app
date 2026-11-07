# Milestone 1 foundations: what is built

This spec describes the parts of M1 that exist so far. It's updated as each work package lands.
The architecture is in [ARCHITECTURE.md](../ARCHITECTURE.md); this file records the details that
code reviewers and later sessions need.

## Toolchains and third-party files (M1.1)

- Rust 1.91.0 (edition 2024), .NET 10 SDK, Node 22 with pnpm 10.20.0, Python 3.11+.
  `tools/setup.sh --check` and `tools/setup.ps1 -CheckOnly` check them without changing the
  machine. LLVM/libclang becomes required when `crates/selah-media` (FFmpeg bindings, M1.7)
  exists.
- `tools/third-party.py` downloads everything pinned in `tools/third_party.lock.json` (FFmpeg
  7.1 LGPL builds, the FFmpeg source for macOS, Skia 0.157 prebuilt binaries, cargo-deny, the
  OFL fonts) and checks each SHA-256. Archives are extracted with path, link, size and entry
  limits. Builds read Skia through `SKIA_BINARIES_URL=file://…`, never from the network.
- `tools/generate-media.py` makes the synthetic test clips and refuses GPL FFmpeg builds.

## Contracts (M1.2)

`proto/selah/v1/` is the single source. Rust uses prost, C# uses Grpc.Tools, TypeScript uses
protobuf-es (generated code is committed and the gate checks it's current). A shared wire
fixture is decoded by all three. Field numbers are only ever added.

## IPC (M1.3, SEC-02, SEC-03)

- A frame is a 4-byte little-endian length and an `Envelope`. Lengths of 0 or over 16 MiB are
  refused before any memory is allocated, and bodies grow only as bytes arrive. An envelope
  needs a known actor origin, a user and a device, and a known message type. A bad frame
  closes that connection only.
- Windows: `\\.\pipe\selah-{namespace}-{service}` with a protected owner-only DACL,
  `PIPE_REJECT_REMOTE_CLIENTS`, `FILE_FLAG_FIRST_PIPE_INSTANCE`, and a same-user SID check on
  both ends. Unix: sockets (mode 0600) in a runtime folder that must be owned by the user with
  mode 0700, and a peer uid check on both ends.
- Services: `core`, `engine`, `emergency`, `watchdog`. `SELAH_IPC_NAMESPACE` separates test
  runs and side-by-side copies.
- Clients reconnect with backoff (25 ms doubling to 500 ms). Requests carry an id; replies and
  errors carry it as `correlation_id`; anything else is an event.
- Preview rings: `<runtime>/<namespace>-preview-0.ring`, 3 slots of 480×270 BGRA, each guarded
  by a seqlock so readers never see a torn frame. Rust writes; Rust and C# read.
- Bench (`selah-bench ipc --check`): cross-process round trip, budget p99 ≤ 2 ms. Named pipes on
  the development PC: p50 0.05 ms, p99 0.10 ms.

## Processes (M1.4)

| Process | Endpoint | Restarted by | On restart |
|---|---|---|---|
| `selah-watchdog` | watchdog | — (the `selah` launcher starts it) | — |
| `selah-engine` | engine, emergency | watchdog | Core re-sends the scene |
| `selah-core` | core | watchdog | Reloads `live_state.bin`, re-sends the scene |
| UI (`Selah.UI`) | — | watchdog | Reconnects and resubscribes |

- Children send a heartbeat every 250 ms. A child that exits, or sends nothing for 1.5 s after
  a 5 s start-up grace, is restarted at once; a child that crashes 5 times in 60 s backs off up
  to 10 s. Three crashes of any process in 60 s start safe mode (SAFE-07): core and the UI are
  restarted with `SELAH_SAFE_MODE=1`; the engine keeps the screens and picks it up on its next
  start. A child that exits with code 3 found another copy running and is adopted.
- One watchdog, core and engine per namespace (file locks in the runtime folder).
- Logs: JSON, one file per process per day, 14 kept, every line redacted (secrets registered at
  run time, emails, bearer tokens, `password=`-style values, stream keys, the user's name).
- Core authorises every command before validating it (SEC-14). In M1 the local owner (UI and
  tour origins) may run every UI command; nobody outside core may send raw render commands; API,
  MIDI, macro, timeline and sync origins are refused until their milestones.
- The emergency lane accepts only Clear everything and Show logo, from the UI origin. The engine
  applies it at once and reports `EmergencyApplied`; scenes at or below that revision are then
  ignored, so a scene core sent earlier can't bring the old content back (SAFE-03).

## Rendering (M1.5, M1.6)

- `selah-render::gpu`: wgpu 30. D3D12 (WARP when there's no GPU), Metal, Vulkan (lavapipe in the
  cloud and CI). `SELAH_GPU=software` forces the software adapter, as the tests do.
- Layers so far: a test pattern (SMPTE bars, grid, gradient, in WGSL with the same integer maths
  as the CPU reference, so they match exactly) and a text layer.
- Text: Skia CPU raster with HarfBuzz shaping and bidi (Skia textlayout), using only the bundled
  Noto Sans, Noto Sans KR and Noto Naskh Arabic fonts, never system fonts. Paragraphs are cached
  by text, style and resolution (64 entries). The text layer is composited with premultiplied
  alpha.
- Output windows: winit, on the main thread; the render thread blits the composite to each swap
  chain (Fifo, non-sRGB) and presents. `--fullscreen N` puts output 1 on display N.
- `FrameShown` is stamped when the frame is presented (headless: when the GPU finished it), and
  carries the time the command arrived, so "within one frame" is measured on the output.

## Quality gate (M1.10)

`tools/verify.py` holds the stages; `tools/verify.sh` and `tools/verify.ps1` are thin wrappers.
Each stage's log and a summary go to `target/verify-report/`, and failures print GitHub
annotations. `tools/test-ledger.py` counts passing tests per suite (rust, dotnet, web, golden,
ui, e2e, kb) and refuses any decrease. Golden references live in `tests/golden/reference/` and
change only through `tools/bless-golden.sh --reason`.
