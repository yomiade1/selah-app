# Render pipeline, transitions, effects, thumbnails and slide editor (M3)

Selah's M3 render pipeline turns normalized multi-output `Scene` graphs into vsync-locked GPU
frames (`wgpu`), drives synchronized looping backgrounds with safe audio limiting, previews scenes
offscreen and over dual shared-memory preview rings, enforces WCAG 2.3.1 flash limits, and
supports structured custom slides (`selah-render`, `selah-media`, `selah-engine`, `selah-core`).

## Scene graph, layers and frame scheduler (M3.1)

- **Canonical 5-layer z-order (`selah_render::scene::LAYER_ORDER`):** Back to front on every
  output:
  1. `VideoInput` (rank 0 — live video/camera placeholder in M3, populated in M5)
  2. `Media` (rank 1 — solid colour, linear/radial gradients, images, or NV12 video backdrops)
  3. `Slide` (rank 2 — template-styled lyric/scripture roles, test patterns, or custom
     `SlideElementsDocument` elements)
  4. `Props` (rank 3 — persistent overlay badges or custom `elements_json`)
  5. `Messages` (rank 4 — urgent top-most alert/message overlays)
- **Independent multi-output rendering (`SetOutputConfigs`, `GetOutputConfigs`, `PresentScene`):**
  Each enabled `OutputScene` renders concurrently at its own resolution (`16..=7680` × `16..=4320`),
  safe area, and variant style inside `GpuCompositor::composite_outputs` and
  `CpuCompositor::composite_outputs`.
- **Caching & pre-rasterisation (`OverlayCache`, `PreloadItems`):** Text, chord rows, boxes,
  shapes, and slide elements rasterised by Skia are keyed in `OverlayCache` (and uploaded into
  `GpuCompositor::overlay_slots`, bounded at 64 GPU textures). `PreloadItems` warms upcoming text
  and custom overlays via `prerasterise_hints` before transitions start, while frames containing
  `{clock}` / `{timer}` elements bust time-varying cache keys cleanly at second boundaries.
- **Vsync scheduler (`selah_engine::scheduler::FrameScheduler`):** Advances against a monotonic
  60 Hz (`FRAME_PERIOD = 16,666,667 ns`) grid (`advance_grid`), skips redundant re-renders when
  no scene/video/transition/clock changed (`wait_timeout`), and gives priority preemption to
  `emergency_pending` so F12 **Clear everything** clears all outputs within one frame (`SAFE-03`).

## Backgrounds, video looping and audio safety (M3.4, M3-A4, SAFE-05)

- **Image & gradient backdrops (`selah_render::media_bg`, `blur.{rs,wgsl}`):** Images load
  exclusively from content-addressed `<media_root>/<hash[0..2]>/<hash>.<ext>` paths after
  `validate_media_address`, symlink/escape checks (`resolve_verified_media_path`), full-file
  BLAKE3 verification, pixel-count guards (`MAX_DIMENSION = 8192`, `MAX_PIXELS = 67,108,864`), and
  `MAX_IMAGE_FILE_BYTES = 128 MiB` (`SEC-04`, `SEC-15`). Supports `Fit`, `Fill`, `Stretch`,
  opacity, isotropic Gaussian blur (`sigma <= 64.0`), and multi-stop linear/radial gradients with
  CPU = GPU parity.
- **Zero-gap NV12 video & A/V sync (`selah_media::player::MediaPlayer`, `selah_render::video`):**
  Decoded 8-bit NV12 planes (`R8` luma + `Rg8` interleaved chroma) upload directly and convert to
  RGB in `video.wgsl` (BT.601 / BT.709, limited or full range) with fit/fill/stretch, blur, and
  opacity support. `MediaPlayer` maintains an audio-driven `AudioClock` master clock (default
  CPAL device, headless 10 ms software pump, or deterministic pull capture), speed `0.1..=4.0`,
  `in_ms` / `out_ms` trim points, and **standby loop decoder pre-opening** so 1080p60 loops switch
  across lap boundaries with zero dropped/repeated frames and A/V sync within ±1 frame (`M3-A4`).
- **Master limiter & safe fades (`selah_media::audio::MasterAudioProcessor`, `SAFE-05`):** Audio
  streams decode strictly through `FILE_PROTOCOLS`, `DEMUXERS`, and `AUDIO_DECODERS` allowlists
  (`SEC-04`), default to `-1.94 dB` (`DEFAULT_SAFE_VOLUME = 0.80`), smooth volume changes, execute a
  `25 ms` linear fade-out (`DEFAULT_FADE_MS`) when cleared/stopped instead of clicking, and pass
  through a lookahead ITU-R BS.1770 4× oversampled true-peak brickwall limiter guaranteeing output
  stays strictly below `-1.0 dBTP` even on `+6 dBFS` input (`M3-A8`).

## Transitions, built-in effects and WCAG flash guard (M3.5, SEC-03, SEC-04, SAFE-04)

- **Transitions (`selah_render::transitions.{rs,wgsl}`):** Supports `Cut`, `Fade`, `Dissolve`,
  `FadeThroughBlack`, `Push`, `Wipe`, `Zoom`, and `Slide` across 4 cardinal directions
  (`Left`, `Right`, `Up`, `Down`) and 4 easings (`Linear`, `EaseIn`, `EaseOut`, `EaseInOut`) at
  both scene level and per-layer level, matching identically between WGSL GPU shaders and CPU
  blenders. Emergency clears cancel active transitions immediately.
- **Closed 6 built-in effects (`selah_render::effects.{rs,wgsl}`):** Closed enum `EffectKind`:
  `Blur`, `ColourAdjust` (brightness/contrast/saturation/hue), `Vignette`, `BackgroundBlur`
  (backdrop-sampling frost glass on `Props` / overlay regions), `ColourKey` (chroma key with
  spill suppression), and `Mask` (`Rect`, `RoundedRect`, `Circle`, `Ellipse` + feather/invert).
  Until M8 introduces the signed/sandboxed Shader SDK, custom WGSL/GLSL/script keys
  (`FORBIDDEN_SHADER_KEYS`) anywhere in effect/element JSON are rejected (`SEC-04`), effect chains
  are capped at `MAX_EFFECTS_PER_CHAIN = 8`, and parameters at `MAX_EFFECT_PARAMS_BYTES = 16 KiB`
  (`SEC-03`).
- **WCAG 2.3.1 flash analyser (`selah_render::flash::FlashAnalyzer`, `AnalyzeFlashRisk`):**
  Monitors luma transitions (`>= 0.10` with darker frame `< 0.80` on `>= 25%` of strided samples)
  and saturated-red changes (`> 3` general or red flashes in any trailing 1 s window). Runs in three
  places (`SAFE-04`):
  1. Offline bounded video probe on `ImportMedia` (`selah-media-worker`), tagging flagged clips
     with `flash_warning = true` and `"Flashing"`.
  2. Pre-flight risk analysis (`AnalyzeFlashRisk`, `SplitSongSlides`, `SplitScriptureSlides`).
  3. Live compositor output monitor on every rendered engine frame, raising
     `FrameShown.flash_warning` and `OutputFrameStatus.flash_warning`.

## Offscreen thumbnails and dual Program/Staged previews (M3.6, M3-A3)

- **Scene thumbnail service (`selah_core::thumbnails::render_scene_thumbnail`):** Renders any
  valid `Scene` offscreen on `CpuCompositor` using a deterministic fixed clock for dynamic tokens,
  applies video poster frames extracted safely via `selah-media-worker`, validates inputs strictly
  under `SEC-03`/`SEC-04`, propagates `flash_warning`, converts premultiplied RGBA to straight PNG
  (`encode_rgba_thumbnail`), and caches content-addressed results under `library/thumbnails/scenes/`
  with corrupt-file self-healing and bounded LRU eviction (`MAX_SCENE_THUMB_FILES = 512`,
  `MAX_SCENE_THUMB_BYTES = 64 MiB`). Downscaled thumbnails match live `GpuCompositor` output
  within `<= 3.0` mean channel delta (`M3-A3`).
- **Dual preview rings & staging (`StagePreviewScene`):** The engine maintains separate lock-free
  seqlock shared-memory rings (480×270 BGRA): `<ns>-preview-{n}.ring` for live outputs and
  `<ns>-stage-0.ring` for staged next items (`FrameShown.is_preview = true`). Staging an item
  never mutates live outputs (`SAFE-01`, `SAFE-06`), and the UI displays live **Program** and
  **Staged Next** previews side by side with `SAFE-04` warning badges.

## Custom slides and editors (M3.7, M3-A5)

- **Structured slide elements (`selah_render::slide_elements::SlideElementsDocument`):** Strict
  v1 JSON schema (`MAX_ELEMENTS_JSON_BYTES = 131,072`, `MAX_ELEMENTS = 64`) supporting normalised
  coordinates, grouping (`group_id`), locking (`locked`), z-order, **Text** elements with live
  `{clock}` and `{timer}` dynamic tokens (`timer_duration_secs`), **Shape** elements (`Rect`,
  `RoundedRect`, `Ellipse`, `Line`), **Image** elements (`fit`/`fill`/`stretch`), **Video**
  elements, embedded background/transition specs, and closed built-in effect entries.
- **Avalonia Template Editor & Slide Editor (`app/Selah.UI`):**
  - `TemplateEditorView`: starter looks, role-slot styling, bundled font weights, outline/shadow/card
    box controls, aspect variant editing, safe-area inset sliders, default song/scripture/screen
    assignment, and live `RenderSceneThumbnail` preview.
  - `SlideEditorView`: multi-slide deck editor with Text/Clock/Timer/Shape/Image/Video creation,
    multi-select alignment (`AlignLeft`/`Center`/`Right`/`Top`/`Middle`/`Bottom`), group/ungroup,
    bring-forward/send-backward, element lock, per-document Undo/Redo, and an operator
    acknowledgement guard blocking unconfirmed staging of `flash_warning` items (`SAFE-04`).

## Golden suite and performance benchmark (M3.8, M3-A1, M3-A6)

- **Golden suite (`crates/selah-testkit/tests/golden_m3.rs`, Gate Stage 7):** Covers `55` M3
  reference frames in `tests/golden/reference/`:
  - `m3-style-*` (9 frames): roles/decorations, alignments/safe-area, weights across all 7 bundled
    fonts, min-size shrink-to-fit, Warm Serif preset, custom slide elements, plus 32:9 LED, 4:3,
    and 9:16 aspect variants (`TOLERANCE_STYLE`).
  - `m3-tr-*` (40 frames): all 8 transitions at `t = 0.00, 0.25, 0.50, 0.75, 1.00` (`TOLERANCE_SHADER`).
  - `m3-fx-*` (6 frames): all 6 built-in WGSL effects (`TOLERANCE_SHADER`).
- **M3 Benchmark (`selah-bench m3`, Gate Stage 6):** Renders **2 × 1080p outputs over a 4K
  (`3840×2160`) NV12 video background** with template typography, vignette, and props badge. Tracks
  strict `hardware_pass = (p99 <= 8.0 ms)` (`OUTPUT_P99_BUDGET_MS`, verified at `p99 = 14.86 ms` on
  an NVIDIA MX350 laptop GPU; full reference hardware sign-off is `H-23` in M10) alongside a
  software/virtual adapter CI ceiling (`SOFTWARE_CI_CEILING_MS = 5000.0`) so WARP/lavapipe/Metal VM
  runners validate correctness without hiding hardware timings.
