# Slide generation, templates and metric fit (M2–M3)

Slide generation turns song arrangements and scripture passages into output-aware slides
backed by role-slotted templates and Skia/HarfBuzz typographic measurement (`selah-render::template`,
`selah-render::fit`, `selah-render::text`, and `selah-core`).

## Rule baseline (M2.4, M2.5)

- **Songs (`selah_core::songs::split::generate_slides`):** Breaks strictly at section
  boundaries, chunks verses/choruses to at most `max_lines` (`1..=100`, default `4`),
  tracks `part_index` / `part_count`, preserves aligned Unicode scalar chord offsets and
  optional `translation_lines`, optionally prepends a `title_slide` and appends a blank
  slide, and applies `CopyrightPlacement` (`None`, `First`, `Last`, `FirstAndLast`, `All`).
- **Scripture (`selah_core::scripture::split_scripture_slides`):** Wraps verses at
  whitespace boundaries (falling back to scalar boundaries only for single tokens wider
  than the line budget), respects `one_verse_per_slide` / `verses_per_slide` / `max_lines`,
  optional verse numbers, optional title/blank slides, and `ReferencePlacement`
  (`None`, `SlideHeader`, `SlideFooter`) scoped strictly to the verses on each slide.

## Templates and role slots (M3.2)

`selah_render::template::TemplateStyle` models a reusable template document (`schema_version = 1`,
`reference_height = 1080`, `MAX_STYLE_JSON_BYTES = 65_536`, `MAX_VARIANTS = 32`, `SEC-03` bounds):

- **Role slots (`RoleSlots`):** `body`, `reference`, `title`, `footer` (accepting `copyright`
  as a JSON alias), `chords`, and `translation`.
- **Per-slot typography (`SlotTextStyle`):** `font_family` mapped onto one of the 7 bundled
  OFL variable families (`ALL_FONTS` in `selah-render::text`: `Noto Sans`, `Inter`, `Manrope`,
  `JetBrains Mono`, `Noto Serif`, `Noto Sans KR`, `Noto Naskh Arabic` — never system fonts,
  ADR-005/ADR-009), `size`, `min_size` (≥ `8 px` at 1080p reference), `weight` (`100..=900`
  pre-registered `STANDARD_WEIGHTS`), `color`, optional `OutlineStyle` (`color`, `width`),
  optional `ShadowStyle` (`color`, `offset_x`, `offset_y`, `blur`), optional
  `BackgroundBoxStyle` (`color`, `padding_x`, `padding_y`, `radius`, `full_width`,
  `accent_bar_color`, `accent_bar_width`), `line_height`, `letter_spacing`, `TextAlignH`
  (`Left`, `Center`, `Right`, `Justify`), `TextAlignV` (`Top`, `Middle`, `Bottom`),
  `max_lines`, and `visible`.
- **Safe area (`SafeArea`):** Normalized insets `(left, top, right, bottom)` clamped so at
  least 10% width and height remain (`SafeRect` in output pixels). Built-in presets provide
  `default` (8% margins), `stage` (5%), `led_wide` (4% horizontal, 8% vertical), and
  `lower_third` (66% top, 5% sides/bottom).
- **Aspect variants (`AspectVariantStyle`):** Stored inside `Template.variants` (protobuf) or
  embedded patches (`TemplateStylePatch` with `SlotTextStylePatch`), resolved by closest aspect
  ratio (`width / height`) and minimum resolution distance in `TemplateStyle::resolve_for_output`.
  Legacy M2 shorthand JSON (`{"font":"Manrope","align":"center"}`) remains supported cleanly.
- **Starter presets (`M3.2`, `M3.7`):** Four built-in looks exposed by core and the UI
  Template Editor: **Sanctuary** (`preset_sanctuary`), **Warm Serif** (`preset_warm_serif`),
  **Lower Third** (`preset_lower_third`), and **Stage Monitor** (`preset_stage_monitor`).

## Template precedence

When `SplitSongSlides` or `SplitScriptureSlides` runs via `CoreService`, style resolution per
target output follows this strict priority order:

1. Per-target explicit `FitTargetOutput.style_json`
2. Per-target explicit `FitTargetOutput.template_id`
3. Request-level `style_json` or `template_id`
4. Active output configuration's assigned `OutputConfig.template_id`
5. Library policy setting `templates.default.song` or `templates.default.scripture`
   (32-char lowercase hex `Id` across `User` / `Machine` / `Organisation` policy precedence)
6. Built-in `TemplateStyle::default()`

`SplitSongSlides` / `SplitScriptureSlides` activate metric fitting whenever `shrink_to_fit = true`,
any `style_json` / `template_id` / `target_outputs` is supplied, or a default template policy
is set; callers without template parameters retain pure rule-based M2 behaviour.

## Metric fit and multi-output splitting (M3.3, M3-A2)

`selah_render::fit` (`fit_song_slides_with_renderer`, `fit_scripture_slides_with_renderer`) shapes
text with the exact Skia HarfBuzz layout width (`TextRenderer::slot_layout_width`, accounting for
outline stroke, box padding, accent bar, and drop shadow bleed):

1. **Measure at base size:** Each rule-split chunk is checked inside `SafeRect` after reserving
   vertical space for visible `header` / `reference` / `footer` slots and any inline chord row /
   translation band height (`reserve_header_footer_space`).
2. **Shrink-to-`min_size` first:** If a chunk overflows height or width at `slot.size`, binary search
   (`maximize_size_for_chunk`) finds the largest font size in `[min_size, size]` where the chunk
   renders cleanly inside `SafeRect` with zero clipping and zero unresolved glyphs where covered by
   bundled fonts.
3. **Clean re-split before dropping below `min_size`:** If even `min_size` cannot fit a chunk,
   `fit_chunk_sequence` keeps `applied_size >= min_size` and splits cleanly: first across lines,
   then long wrapped lines or translations at word boundaries (`split_single_line_at_word_boundary`,
   `split_translation_if_oversized`), preserving aligned chord positions across split fragments
   (`split_line_at_char_index`) and recounting section `part_index` / `part_count`.
4. **Copyright & reference guard:** `apply_copyright_with_fit_guard` re-verifies every slide carrying
   a footer or verse reference after footer assignment and re-splits any slide tightened by footer
   reservation.
5. **Fit report (`OutputFitReport`):** Each target output receives `output_index`, `output_name`,
   `resolution`, `matched_variant`, `fits`, `applied_size`, `min_size`, `original_slide_count`,
   `final_slide_count`, `extra_splits`, `overflows_safe_area`, warning codes (`SHRUNK_TO_FIT`,
   `EXTRA_SPLIT`, `OVERFLOWS_SAFE_AREA`, `AT_MIN_SIZE`), human summary, and `flash_warning`
   propagated from any referenced background media or rapid transition (`SAFE-04`).

## Validation

- `crates/selah-render/tests/templates.rs`: role slot styling, weight registration across all 7
  OFL fonts, aspect variant selection, outline/shadow/card box/accent bar CPU=GPU parity.
- `crates/selah-core/tests/metric_fit.rs`: song & scripture metric fit unit/integration cases plus
  **M3-A2** property test (`5,000` random multilingual/chorded/translated lyric sets across 16:9
  `1920×1080`, 32:9 LED `3840×1080` max 2 lines, and 4:3 `1440×1080` — `15,000` output splits
  verifying zero safe-area overflow, `applied_size >= min_size`, honest `OutputFitReport` counts,
  and pixel boundary checks).
- `crates/selah-testkit/tests/golden_m3.rs`: style golden frames (`m3-style-*`) across 9:16,
  4:3, 32:9 LED, 16:9, alignments, weights, shrink-to-fit, Warm Serif preset, and custom elements.
