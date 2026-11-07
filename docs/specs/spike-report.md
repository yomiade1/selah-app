# M1 spike report

M1 set out to prove the risky parts of the architecture before building on them: text, video,
IPC speed and surviving crashes (BUILD-PLAN M1). This report gives the measurements against the
budgets in ARCHITECTURE §12 and a go/no-go on Rust (core, engine) + Avalonia (UI).

Measured on the development PC (Windows 11, NVIDIA GeForce MX350, a low-end laptop GPU), on
the CI runners (WARP and lavapipe software rendering), and by the automated tests. Reference
hardware (H-23) sign-off is M10.

## Results

| Area | Budget | Result | Verdict |
|---|---|---|---|
| UI ↔ core round trip | p99 ≤ 2 ms | Named pipes (Windows): p99 **0.096 ms** over 10,000 round trips (`selah-bench ipc`). Unix sockets: the `linux` CI job runs the same bench with `--check` (≤ 2 ms) in every PR. | Pass, with a 20× margin |
| Restart after a crash | ≤ 2 s, last frame held | UI, core and engine each killed: back in < 2 s, the output's frame checksum unchanged for UI and core kills, the scene restored after an engine kill (`continuity` tests, 3 runs in a row) | Pass |
| Slide change to next frame | ≤ 1 frame | `FrameShown` ≤ 1 frame after the engine received **Show test pattern** (`show_test_pattern_reaches_the_output_within_one_frame`) | Pass |
| Emergency clear with core frozen | ≤ 1 frame | Core's threads suspended; **Clear everything** over the emergency lane shows a cleared frame within one frame period (`clear_everything_works_with_core_frozen`) | Pass |
| Text in 4 scripts | Golden match | English, Yorùbá (tone marks), Korean and Arabic (right to left) match the reference set with no missing glyphs; GPU blending = CPU reference (`golden_text`) | Pass |
| 1080p60 H.264 under text | No drops on a 60 fps clock | Software decode in CI: frames in order, 0 drops (`clip_plays_in_order_with_no_drops_on_a_60_fps_virtual_clock`). MX350 with D3D11VA: **0 drops**, composite p99 **5.1 ms** | Pass |
| Output frame time, 4K60 HEVC | p99 ≤ 8 ms (2 outputs + 4K60 on reference hardware) | MX350 with D3D11VA: **9 of 180 frames dropped**, composite p99 **16 ms** | Not yet: see below |
| Startup to usable UI | ≤ 4 s | Not measured formally (M10). The headless stack is ready in about 1–2 s in tests. | — |

## Findings

1. **The 4K path needs zero-copy textures.** Today a hardware-decoded frame is copied from
   the GPU to the CPU (NV12) and uploaded again. At 4K60 that copy alone is ~750 MB/s each
   way, which is why the MX350 drops frames. The fix is the planned zero-copy path: D3D11
   shared textures into wgpu on Windows and IOSurface/CVMetalTexture on macOS. It belongs to
   the render work in M3 and the outputs work in M5. 1080p60 already has a large margin.
2. **IPC is not a bottleneck.** Request/reply is well under a tenth of a millisecond. Preview
   frames travel through shared memory, not the pipe.
3. **The process split works.** Killing any process leaves the audience screens untouched (UI,
   core) or restores them within 2 s (engine). Safe mode, the emergency lane and the watchdog
   behave as designed.
4. **Avalonia is fine for the shell.** The UI meets the text-size matrix (100/150/200 %, Light
   and Booth, `en` and the pseudo-locale) with no clipped critical labels, runs headless for
   tests and the installer check, and its self-contained publish is about 115 MB before
   compression.
5. **Packaging.** Velopack installs per user without admin, updates in place while running
   and uninstalls cleanly. The Rust binaries need the Visual C++ runtime, so it ships
   app-local.

## Hardware results still to collect

**Help ▸ Diagnostics ▸ Run benchmark** on the installed test build saves a report with the
same IPC and 4K60 HEVC measurements. Attach it to the M1 PR (M1-A7 (H)). Reference hardware
results come in M10.

## Recommendation: go

Go with Rust for core and the engine and Avalonia for the UI (ADR-001, ADR-002, ADR-008). Every
budget that M1 can measure passes except 4K60 HEVC on a low-end GPU, and that one has a known
cause and a planned fix (zero-copy textures) that doesn't change the architecture. No ADR
changes are needed.
