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
