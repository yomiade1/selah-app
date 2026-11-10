# Golden frame changes

Every change to a reference image is recorded here with its reason (TESTING-AND-QUALITY §3,
rule 3). `tools/bless-golden.sh --reason` appends entries for the reference set in
`tests/golden/reference/`; the `bless-golden.yml` workflow appends entries for
platform-specific images in `tests/golden/<platform>/`.

## 2026-11-07 — reference (text-en, text-yo, text-ko, text-ar)

- Reason: first reference set for the M1.6 text spike (Psalm 23:1 and a public-domain hymn line,
  72 px at 1920×1080, bundled Noto fonts at weight 400). Reviewed by eye: Yorùbá tone marks and
  dots below stack correctly (precomposed and combining), Arabic joins right to left, Korean
  uses the regular weight.
- Rendered on: Windows x64, WARP (`SELAH_GPU=software`). Text is rasterised by Skia on the CPU,
  so the frames don't depend on the GPU; other platforms compare within `Tolerance::TEXT`.

## 2026-11-07 — reference (text-en, text-yo, text-ko, text-ar)

- Reason: slide text is now drawn without font hinting. CI showed Linux (FreeType) differing from
  Windows (DirectWrite) on 0.53% of the English frame's pixels; hinting is where the platforms'
  rasterisers differ most, and slide text is large enough not to need it.
- Rendered on: Windows x64, WARP (`SELAH_GPU=software`).

## 2026-11-07 — linux (text-en)

- Reason: on Linux, Skia draws glyphs with FreeType; on Windows, with DirectWrite. The English
  frame has the same layout and line breaks on both but differs in anti-aliased glyph edges on
  0.53% of pixels (limit 0.5%). The other three languages are within tolerance of the reference.
- Rendered on: the PR's `linux` CI job (ubuntu-24.04, lavapipe), run 42356005112, taken from its
  failure report because `bless-golden.yml` can't be dispatched until it's on `main`. Reviewed by
  eye.

## 2026-11-07 — macos (text-en, text-yo)

- Reason: on macOS, Skia draws glyphs with CoreText, which anti-aliases slightly heavier than
  DirectWrite. English and Yorùbá have the same layout, line breaks and tone marks as the
  reference but differ on edge pixels (0.63% and 0.55%, limit 0.5%). Korean and Arabic are
  within tolerance of the reference.
- Rendered on: the PR's `macos` CI job (macos-15, software rendering), run 42410558009, taken
  from its failure report. `bless-golden.yml` can't render on macOS yet because it doesn't build
  the macOS FFmpeg (follow-up in PROGRESS.md). Reviewed by eye against the reference.

## 2026-11-10 — reference (M3.8: 55 `m3-*` frames)

- Reason: M3-A1 golden-frame suite (`crates/selah-testkit/tests/golden_m3.rs`):
  - Style & typography (`m3-style-roles-decorations`, `m3-style-alignments-safe-area`,
    `m3-style-min-size-shrink`, `m3-style-custom-elements`, `m3-style-fonts-weights`,
    `m3-style-aspect-32x9`, `m3-style-aspect-4x3`, `m3-style-aspect-9x16`,
    `m3-style-preset-warm-serif` — 9 frames): covers all 5 text role slots (`title`, `reference`,
    `body` + interleaved `chords` + `translation`, `footer`), outlines, shadows, card boxes with
    accent bars, H/V alignments, safe-area margins, min-size metric shrink via `fit_song_slides`,
    all 7 bundled OFL variable fonts across weights `100..=900` and Latin/Yorùbá/Korean/Arabic,
    custom `SlideElementsDocument` (`Rect`/`RoundedRect`/`Ellipse`/`Line`, fixed `{clock}` and
    `{timer}` tokens, `Props`, `Messages`), and 16:9 / 32:9 / 4:3 / 9:16 aspect variants.
  - Transitions (`m3-tr-{cut,fade,dissolve,fade-through-black,push,wipe,zoom,slide}-t{0.00,0.25,0.50,0.75,1.00}`
    — 40 frames): all 8 transitions across 5 timestamps (`0.00`, `0.25`, `0.50`, `0.75`, `1.00`)
    using deterministic geometric scenes (no glyphs).
  - Built-in effects (`m3-fx-{blur,colour-adjust,vignette,background-blur,colour-key,mask}` — 6 frames):
    all 6 closed built-in post/layer shader effects using SMPTE bars, gradient, and vector shapes.
- Also fixed `TextRenderer::{measure,paint}_body_with_chords` (`crates/selah-render/src/text.rs`) so
  chorded body slides render `background_box`, `outline`, and `shadow` identically to unchorded
  slots, then re-blessed `m3-style-roles-decorations.png`.
- Reviewed by eye: verified card box + amber accent bar, outlines, shadows, chord alignment, all 7
  fonts without fallback boxes, aspect variants, transition midpoints (`fade-through-black` at
  black, `dissolve` dither, `push`/`wipe`/`zoom`/`slide`), and all 6 effects (`colour-key` cleanly
  keying green, `mask` feathered rounded rect, `background-blur` central frost). Also verified GPU
  (`AdapterChoice::Software`) vs CPU compositor parity (`m3_golden_gpu_and_cpu_parity_agree`).
- Rendered on: Windows x64, WARP (`SELAH_GPU=software`). Style frames compare across platforms
  with `TOLERANCE_STYLE` (channel ≤ 10, ≤ 2% pixels for DirectWrite/FreeType/CoreText AA edges);
  transition and effect frames compare with `TOLERANCE_SHADER` (channel ≤ 3, ≤ 0.2% pixels).
