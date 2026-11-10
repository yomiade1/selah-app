//! Slide text (ADR-005): Skia CPU raster with HarfBuzz shaping, using only the bundled slide
//! fonts (never system fonts, so output matches on every computer). Rendered paragraphs are
//! cached by content, style and resolution, so a slide change only swaps a texture.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use selah_proto::v1::SlideLayerSpec;
use skia_safe::font_arguments::VariationPosition;
use skia_safe::font_arguments::variation_position::Coordinate;
use skia_safe::paint::{Cap, Join, Style as PaintStyle};
use skia_safe::textlayout::{
    FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, TextAlign, TextDirection,
    TextStyle, TypefaceFontProvider,
};
use skia_safe::{
    AlphaType, BlurStyle, Canvas, Color, ColorType, FontArguments, FontMgr, FourByteTag, ImageInfo,
    MaskFilter, Paint, RRect, Rect, Typeface, surfaces,
};

use crate::Frame;
use crate::template::{
    BackgroundBoxStyle, ResolvedTemplate, SafeRect, SlotTextStyle, TextAlignH, TextAlignV,
    parse_chords_json,
};

/// The core M1 slide font families, in fallback order. Each file is bundled (OFL, ADR-009).
pub const SLIDE_FONTS: [(&str, &str); 3] = [
    ("Noto Sans", "notosans/NotoSans[wdth,wght].ttf"),
    ("Noto Sans KR", "notosanskr/NotoSansKR[wght].ttf"),
    (
        "Noto Naskh Arabic",
        "notonaskharabic/NotoNaskhArabic[wght].ttf",
    ),
];

/// All 7 bundled OFL variable font families available to templates and custom slides.
pub const ALL_FONTS: [(&str, &str); 7] = [
    ("Noto Sans", "notosans/NotoSans[wdth,wght].ttf"),
    ("Inter", "inter/Inter[opsz,wght].ttf"),
    ("Manrope", "manrope/Manrope[wght].ttf"),
    ("JetBrains Mono", "jetbrainsmono/JetBrainsMono[wght].ttf"),
    ("Noto Serif", "notoserif/NotoSerif[wdth,wght].ttf"),
    ("Noto Sans KR", "notosanskr/NotoSansKR[wght].ttf"),
    (
        "Noto Naskh Arabic",
        "notonaskharabic/NotoNaskhArabic[wght].ttf",
    ),
];

/// Standard variable weight stops pre-registered on startup so any standard weight renders
/// immediately across every bundled font and script fallback.
pub const STANDARD_WEIGHTS: [u16; 9] = [100, 200, 300, 400, 500, 600, 700, 800, 900];

/// The weight legacy M1 slide text is drawn at (regular).
pub const SLIDE_WEIGHT: f32 = 400.0;

/// Map any user-supplied or legacy font family string onto one of the 7 bundled OFL families.
/// Never resolves system fonts (ADR-005 / ADR-009).
pub fn canonical_font_family(name: &str) -> &'static str {
    let key = name
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    match key.as_str() {
        "inter" | "inter var" | "inter variable" => "Inter",
        "manrope" | "manrope variable" => "Manrope",
        "jetbrains mono" | "jetbrainsmono" | "mono" | "monospace" => "JetBrains Mono",
        "noto serif" | "notoserif" | "serif" => "Noto Serif",
        "noto sans kr" | "notosanskr" => "Noto Sans KR",
        "noto naskh arabic" | "notonaskharabic" | "arabic" => "Noto Naskh Arabic",
        _ => "Noto Sans",
    }
}

/// How a legacy block of slide text looks. Sizes are in output pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct SlideTextStyle {
    pub size: f32,
    pub color: [u8; 4],
    /// Horizontal margin on each side, as a fraction of the output width.
    pub margin: f32,
    pub line_height: f32,
}

impl Default for SlideTextStyle {
    fn default() -> Self {
        Self {
            size: 72.0,
            color: [255, 255, 255, 255],
            margin: 0.08,
            line_height: 1.25,
        }
    }
}

impl SlideTextStyle {
    /// The M1 default for an output of this height: 72 px at 1080p, scaled with the output.
    pub fn for_output(height: u32) -> Self {
        Self {
            size: (72.0 * height as f32 / 1080.0).max(8.0),
            ..Self::default()
        }
    }

    fn key(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.size.to_le_bytes());
        hasher.update(&self.color);
        hasher.update(&self.margin.to_le_bytes());
        hasher.update(&self.line_height.to_le_bytes());
        *hasher.finalize().as_bytes()
    }
}

/// Measured paragraph metrics in Skia/HarfBuzz (used by M3.2 box fitting & M3.3 metric fit).
#[derive(Clone, Debug, PartialEq)]
pub struct SlotMeasurement {
    pub layout_width: f32,
    pub height: f32,
    pub min_intrinsic_width: f32,
    pub max_intrinsic_width: f32,
    pub longest_line: f32,
    pub line_count: usize,
    pub line_widths: Vec<f32>,
    pub min_line_left: f32,
    pub max_line_right: f32,
    pub did_exceed_max_lines: bool,
    pub unresolved_glyphs: usize,
}

impl SlotMeasurement {
    /// Bounding box height including any outer background box padding and stroke bleed.
    pub fn total_height_with_decorations(&self, style: &SlotTextStyle) -> f32 {
        let box_pad_y = style
            .background_box
            .as_ref()
            .map_or(0.0, |b| b.padding_y * 2.0);
        let stroke_pad = style.outline.as_ref().map_or(0.0, |o| o.width * 2.0);
        self.height + box_pad_y + stroke_pad
    }
}

/// Where the bundled fonts are: `SELAH_FONTS_DIR`, `fonts/` next to the executable (installed
/// builds), or `third_party/fonts` in the repository (development and tests).
pub fn fonts_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("SELAH_FONTS_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let exe = std::env::current_exe().context("executable path")?;
    for ancestor in exe.ancestors().skip(1) {
        for candidate in [ancestor.join("fonts"), ancestor.join("third_party/fonts")] {
            if candidate.join(SLIDE_FONTS[0].1).is_file() {
                return Ok(candidate);
            }
        }
    }
    bail!("the slide fonts aren't installed (run tools/setup to fetch them)")
}

/// Does the text start with a right-to-left letter (Arabic, Hebrew)?
pub fn is_rtl(text: &str) -> bool {
    text.chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(|c| matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF))
}

fn weight_alias(family: &str, weight: u16) -> String {
    format!("{family}@{weight}")
}

fn clone_at_weight(typeface: &Typeface, weight: f32) -> Typeface {
    let coords = [Coordinate {
        axis: FourByteTag::from_chars('w', 'g', 'h', 't'),
        value: weight,
    }];
    let args = FontArguments::new().set_variation_design_position(VariationPosition {
        coordinates: &coords,
    });
    typeface
        .clone_with_arguments(&args)
        .unwrap_or_else(|| typeface.clone())
}

type CacheKey = ([u8; 32], [u8; 32], u32, u32);

pub struct TextRenderer {
    fonts: FontCollection,
    provider: TypefaceFontProvider,
    base_typefaces: Vec<(String, Typeface)>,
    registered_weights: HashSet<u16>,
    families: Vec<String>,
    cache: HashMap<CacheKey, Arc<Frame>>,
    order: Vec<CacheKey>,
    capacity: usize,
    hits: u64,
    misses: u64,
}

impl TextRenderer {
    pub fn new(dir: &Path) -> Result<Self> {
        static FONT_INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = FONT_INIT_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let decoder = FontMgr::new();
        let mut provider = TypefaceFontProvider::new();
        let mut base_typefaces = Vec::with_capacity(ALL_FONTS.len());
        let mut loaded_by_family: HashMap<&'static str, Typeface> = HashMap::new();

        for (family, file) in ALL_FONTS {
            let path = dir.join(file);
            let bytes = std::fs::read(&path)
                .with_context(|| format!("slide font {} is missing", path.display()))?;
            let typeface = decoder
                .new_from_data(skia_safe::Data::new_copy(&bytes), None)
                .with_context(|| format!("slide font {} couldn't be read", path.display()))?;
            // Preserve M1 regular weight alias "<Family>" -> 400.0 so golden_text stays exact.
            let regular = clone_at_weight(&typeface, SLIDE_WEIGHT);
            provider.register_typeface(regular, Some(family));
            base_typefaces.push((family.to_owned(), typeface.clone()));
            loaded_by_family.insert(family, typeface);
        }

        // Pre-register every standard CSS weight (100..=900) under "<Family>@<weight>" so primary
        // fonts and fallbacks both render cleanly at the exact requested variable weight.
        let mut registered_weights = HashSet::new();
        for &w in &STANDARD_WEIGHTS {
            for (family, base) in &base_typefaces {
                let weighted = clone_at_weight(base, f32::from(w));
                provider.register_typeface(weighted, Some(weight_alias(family, w).as_str()));
            }
            registered_weights.insert(w);
        }

        // Maintain exact M1 SLIDE_FONTS fallback order for legacy `render()`.
        let mut families = Vec::new();
        for (family, _) in SLIDE_FONTS {
            if loaded_by_family.contains_key(family) {
                families.push(family.to_owned());
            }
        }

        let mut fonts = FontCollection::new();
        fonts.set_asset_font_manager(Some(provider.clone().into()));
        // No system fonts: what's on the slides mustn't depend on the computer.
        fonts.set_default_font_manager(None, None);
        fonts.enable_font_fallback();

        Ok(Self {
            fonts,
            provider,
            base_typefaces,
            registered_weights,
            families,
            cache: HashMap::new(),
            order: Vec::new(),
            capacity: 128,
            hits: 0,
            misses: 0,
        })
    }

    /// Load the bundled fonts from [`fonts_dir`].
    pub fn bundled() -> Result<Self> {
        Self::new(&fonts_dir()?)
    }

    pub fn cached(&self) -> usize {
        self.cache.len()
    }

    pub fn cache_hits(&self) -> u64 {
        self.hits
    }

    pub fn cache_misses(&self) -> u64 {
        self.misses
    }

    fn ensure_weight_registered(&mut self, weight: u16) -> u16 {
        let clamped = weight.clamp(100, 900);
        if !self.registered_weights.contains(&clamped) {
            for (family, base) in &self.base_typefaces {
                let weighted = clone_at_weight(base, f32::from(clamped));
                self.provider
                    .register_typeface(weighted, Some(weight_alias(family, clamped).as_str()));
            }
            self.registered_weights.insert(clamped);
            self.fonts.clear_caches();
        }
        clamped
    }

    fn slot_font_families(&mut self, primary: &str, weight: u16) -> Vec<String> {
        let canonical = canonical_font_family(primary);
        let w = self.ensure_weight_registered(weight);
        let mut out = Vec::with_capacity(6);
        out.push(weight_alias(canonical, w));
        for fallback in ["Noto Sans", "Noto Sans KR", "Noto Naskh Arabic"] {
            if fallback != canonical {
                out.push(weight_alias(fallback, w));
            }
        }
        out.push(canonical.to_owned());
        out.push("Noto Sans".to_owned());
        out
    }

    /// Render `text` centred in a `width` × `height` transparent frame (premultiplied RGBA).
    /// Preserved cleanly for M1 golden text compatibility.
    pub fn render(
        &mut self,
        text: &str,
        style: &SlideTextStyle,
        width: u32,
        height: u32,
    ) -> Result<Arc<Frame>> {
        let key = (
            *blake3::hash(text.as_bytes()).as_bytes(),
            style.key(),
            width,
            height,
        );
        if let Some(hit) = self.cache.get(&key) {
            let hit = hit.clone();
            self.hits += 1;
            self.order.retain(|k| *k != key);
            self.order.push(key);
            return Ok(hit);
        }
        self.misses += 1;
        let frame = Arc::new(self.rasterise(text, style, width, height)?);
        if self.cache.len() >= self.capacity && !self.order.is_empty() {
            let oldest = self.order.remove(0);
            self.cache.remove(&oldest);
        }
        self.cache.insert(key, frame.clone());
        self.order.push(key);
        Ok(frame)
    }

    /// Effective usable paragraph layout width inside a slot box of width `max_width`.
    pub fn slot_layout_width(max_width: f32, style: &SlotTextStyle) -> f32 {
        let box_pad_left = style
            .background_box
            .as_ref()
            .map_or(0.0, |b| b.padding_x + b.accent_bar_width);
        let box_pad_right = style.background_box.as_ref().map_or(0.0, |b| b.padding_x);
        let stroke_pad = style.outline.as_ref().map_or(0.0, |o| o.width);
        let shadow_pad = style
            .shadow
            .as_ref()
            .filter(|s| s.color[3] > 0)
            .map_or(0.0, |s| s.offset_x.abs() + s.blur * 0.5);
        // Reserve 2.0px for pixel-grid ceil/floor rounding so glyphs + strokes + shadow never exit slot_rect.
        (max_width - box_pad_left - box_pad_right - stroke_pad * 2.0 - shadow_pad * 2.0 - 2.0)
            .max(1.0)
    }

    /// Measure a single styled slot paragraph cleanly within `max_width` pixels.
    pub fn measure_slot(
        &mut self,
        text: &str,
        style: &SlotTextStyle,
        max_width: f32,
    ) -> SlotMeasurement {
        let families = self.slot_font_families(&style.font_family, style.weight);
        let effective_w = Self::slot_layout_width(max_width, style);
        let mut p = build_slot_paragraph(&self.fonts, &families, text, style, None);
        p.layout(effective_w);
        measure_laid_out_paragraph(&mut p, effective_w)
    }

    /// Measure a body slide's text lines with chord rows and optional translation using the exact
    /// geometry of [`TextRenderer::paint_body_with_chords`].
    pub fn measure_body_with_chords(
        &mut self,
        text: &str,
        chords_by_line: &HashMap<usize, Vec<(usize, String)>>,
        body_style: &SlotTextStyle,
        chord_style: &SlotTextStyle,
        translation: Option<(&str, &SlotTextStyle)>,
        area_width: f32,
    ) -> (f32, f32, usize, f32) {
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() {
            return (0.0, 0.0, 0, 0.0);
        }
        let body_families = self.slot_font_families(&body_style.font_family, body_style.weight);
        let chord_families = self.slot_font_families(&chord_style.font_family, chord_style.weight);
        let stroke_pad = body_style.outline.as_ref().map_or(0.0, |o| o.width);
        let shadow_pad = body_style
            .shadow
            .as_ref()
            .filter(|s| s.color[3] > 0)
            .map_or(0.0, |s| s.offset_x.abs() + s.blur * 0.5);
        let layout_w = (area_width - stroke_pad * 2.0 - shadow_pad * 2.0 - 2.0).max(1.0);
        let line_gap = (body_style.size * 0.2).max(2.0).round();
        let unclipped_body = SlotTextStyle {
            max_lines: None,
            ..body_style.clone()
        };

        let mut total_h = 0.0f32;
        let mut max_w = 0.0f32;
        let mut min_intrinsic = 0.0f32;
        let mut visual_lines = 0usize;
        for (idx, line) in lines.iter().enumerate() {
            let mut p =
                build_slot_paragraph(&self.fonts, &body_families, line, &unclipped_body, None);
            p.layout(layout_w);
            let m = measure_laid_out_paragraph(&mut p, layout_w);
            max_w = max_w.max(m.longest_line);
            min_intrinsic = min_intrinsic.max(m.min_intrinsic_width);
            visual_lines += m.line_count.max(1);
            let line_chords = chords_by_line.get(&idx).map_or(&[][..], Vec::as_slice);
            let chord_h = if line_chords.is_empty() {
                0.0
            } else {
                let mut cp =
                    build_slot_paragraph(&self.fonts, &chord_families, "G", chord_style, None);
                cp.layout(layout_w);
                cp.height().ceil()
            };
            total_h += chord_h + m.height;
            if idx + 1 < lines.len() {
                total_h += line_gap;
            }
        }
        total_h += stroke_pad * 2.0;

        if let Some((t_text, t_style)) = translation
            && !t_text.trim().is_empty()
        {
            let trans_meas = self.measure_slot(
                t_text,
                &SlotTextStyle {
                    max_lines: None,
                    ..t_style.clone()
                },
                area_width,
            );
            total_h += trans_meas.total_height_with_decorations(t_style)
                + (body_style.size * 0.3).max(4.0).round();
            max_w = max_w.max(trans_meas.longest_line);
            min_intrinsic = min_intrinsic.max(trans_meas.min_intrinsic_width);
        }

        (total_h, max_w, visual_lines, min_intrinsic)
    }

    /// Render a complete multi-slot [`SlideLayerSpec`] with full [`ResolvedTemplate`] styling
    /// (background box, shadow, outline, fill, variable weight, spacing, H/V align, safe area,
    /// title, reference, body, translation, chords, footer) into a premultiplied RGBA frame.
    pub fn render_slide(
        &mut self,
        slide: &SlideLayerSpec,
        resolved: &ResolvedTemplate,
        width: u32,
        height: u32,
    ) -> Result<Frame> {
        let info = ImageInfo::new(
            (width as i32, height as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );
        let mut surface = surfaces::raster(&info, None, None).context("slide text surface")?;
        let canvas = surface.canvas();
        canvas.clear(Color::TRANSPARENT);

        let safe = resolved.safe_area.resolved_rect(width, height);
        let gap = (safe.height * 0.015).max(4.0).round();
        let mut avail_top = safe.top;
        let mut avail_bottom = safe.bottom;

        let has_title = resolved.roles.title.visible && !slide.title_text.trim().is_empty();
        let has_ref = resolved.roles.reference.visible && !slide.reference_text.trim().is_empty();
        let has_body = resolved.roles.body.visible && !slide.text.trim().is_empty();
        let has_trans =
            resolved.roles.translation.visible && !slide.translation_text.trim().is_empty();
        let has_footer = resolved.roles.footer.visible && !slide.footer_text.trim().is_empty();

        // 1. Reserve footer/copyright slot first so main content never covers it.
        if has_footer {
            let footer_style = &resolved.roles.footer;
            let meas = self.measure_slot(&slide.footer_text, footer_style, safe.width);
            let slot_h = meas
                .total_height_with_decorations(footer_style)
                .min((avail_bottom - avail_top).max(1.0));
            let slot_rect = match footer_style.align_v {
                TextAlignV::Top => {
                    let r = SafeRect {
                        left: safe.left,
                        top: avail_top,
                        right: safe.right,
                        bottom: (avail_top + slot_h).min(avail_bottom),
                        width: safe.width,
                        height: slot_h,
                    };
                    avail_top = (r.bottom + gap).min(avail_bottom);
                    r
                }
                TextAlignV::Middle | TextAlignV::Bottom => {
                    let top = (avail_bottom - slot_h).max(avail_top);
                    let r = SafeRect {
                        left: safe.left,
                        top,
                        right: safe.right,
                        bottom: avail_bottom,
                        width: safe.width,
                        height: (avail_bottom - top).max(1.0),
                    };
                    avail_bottom = (top - gap).max(avail_top);
                    r
                }
            };
            self.paint_slot(&slide.footer_text, footer_style, slot_rect, canvas);
        }

        // 2. Title slot: dedicated title slide uses the full available safe area, while combined
        // title+body slides place title above the body without overlapping it.
        if has_title {
            let title_style = &resolved.roles.title;
            if !has_body && !has_trans && !has_ref {
                let slot_rect = SafeRect {
                    left: safe.left,
                    top: avail_top,
                    right: safe.right,
                    bottom: avail_bottom,
                    width: safe.width,
                    height: (avail_bottom - avail_top).max(1.0),
                };
                self.paint_slot(&slide.title_text, title_style, slot_rect, canvas);
            } else {
                let meas = self.measure_slot(&slide.title_text, title_style, safe.width);
                let slot_h = meas
                    .total_height_with_decorations(title_style)
                    .min((avail_bottom - avail_top).max(1.0));
                let slot_rect = match title_style.align_v {
                    TextAlignV::Bottom => {
                        let top = (avail_bottom - slot_h).max(avail_top);
                        let r = SafeRect {
                            left: safe.left,
                            top,
                            right: safe.right,
                            bottom: avail_bottom,
                            width: safe.width,
                            height: (avail_bottom - top).max(1.0),
                        };
                        avail_bottom = (top - gap).max(avail_top);
                        r
                    }
                    TextAlignV::Top | TextAlignV::Middle => {
                        let r = SafeRect {
                            left: safe.left,
                            top: avail_top,
                            right: safe.right,
                            bottom: (avail_top + slot_h).min(avail_bottom),
                            width: safe.width,
                            height: slot_h,
                        };
                        avail_top = (r.bottom + gap).min(avail_bottom);
                        r
                    }
                };
                self.paint_slot(&slide.title_text, title_style, slot_rect, canvas);
            }
        }

        // 3. Reference slot (header or footer placement inside remaining safe bounds).
        if has_ref {
            let ref_style = &resolved.roles.reference;
            let meas = self.measure_slot(&slide.reference_text, ref_style, safe.width);
            let slot_h = meas
                .total_height_with_decorations(ref_style)
                .min((avail_bottom - avail_top).max(1.0));
            let slot_rect = match ref_style.align_v {
                TextAlignV::Top => {
                    let r = SafeRect {
                        left: safe.left,
                        top: avail_top,
                        right: safe.right,
                        bottom: (avail_top + slot_h).min(avail_bottom),
                        width: safe.width,
                        height: slot_h,
                    };
                    avail_top = (r.bottom + gap).min(avail_bottom);
                    r
                }
                TextAlignV::Middle | TextAlignV::Bottom => {
                    let top = (avail_bottom - slot_h).max(avail_top);
                    let r = SafeRect {
                        left: safe.left,
                        top,
                        right: safe.right,
                        bottom: avail_bottom,
                        width: safe.width,
                        height: (avail_bottom - top).max(1.0),
                    };
                    avail_bottom = (top - gap).max(avail_top);
                    r
                }
            };
            self.paint_slot(&slide.reference_text, ref_style, slot_rect, canvas);
        }

        // 4. Main body (+ optional interleaved chords and/or translation slot).
        if has_body || has_trans {
            let body_rect = SafeRect {
                left: safe.left,
                top: avail_top,
                right: safe.right,
                bottom: avail_bottom.max(avail_top + 1.0),
                width: safe.width,
                height: (avail_bottom - avail_top).max(1.0),
            };
            let chords = if resolved.roles.chords.visible {
                parse_chords_json(&slide.chords_json)
            } else {
                HashMap::new()
            };

            if !chords.is_empty() && has_body {
                self.paint_body_with_chords(
                    &slide.text,
                    &chords,
                    &resolved.roles.body,
                    &resolved.roles.chords,
                    if has_trans {
                        Some((&slide.translation_text, &resolved.roles.translation))
                    } else {
                        None
                    },
                    body_rect,
                    canvas,
                );
            } else if has_body && has_trans {
                let body_meas =
                    self.measure_slot(&slide.text, &resolved.roles.body, body_rect.width);
                let trans_meas = self.measure_slot(
                    &slide.translation_text,
                    &resolved.roles.translation,
                    body_rect.width,
                );
                let inner_gap = (resolved.roles.body.size * 0.3).max(4.0).round();
                let body_h = body_meas.total_height_with_decorations(&resolved.roles.body);
                let trans_h = trans_meas.total_height_with_decorations(&resolved.roles.translation);
                let combined_h = body_h + inner_gap + trans_h;
                let block_top = match resolved.roles.body.align_v {
                    TextAlignV::Top => body_rect.top,
                    TextAlignV::Middle => {
                        body_rect.top + ((body_rect.height - combined_h) / 2.0).max(0.0).round()
                    }
                    TextAlignV::Bottom => (body_rect.bottom - combined_h).max(body_rect.top),
                };
                self.paint_slot(
                    &slide.text,
                    &resolved.roles.body,
                    SafeRect {
                        left: body_rect.left,
                        top: block_top,
                        right: body_rect.right,
                        bottom: (block_top + body_h).min(body_rect.bottom),
                        width: body_rect.width,
                        height: body_h.min(body_rect.height),
                    },
                    canvas,
                );
                let trans_top = (block_top + body_h + inner_gap).min(body_rect.bottom);
                let trans_style = SlotTextStyle {
                    align_v: TextAlignV::Top,
                    ..resolved.roles.translation.clone()
                };
                self.paint_slot(
                    &slide.translation_text,
                    &trans_style,
                    SafeRect {
                        left: body_rect.left,
                        top: trans_top,
                        right: body_rect.right,
                        bottom: body_rect.bottom.max(trans_top + 1.0),
                        width: body_rect.width,
                        height: (body_rect.bottom - trans_top).max(1.0),
                    },
                    canvas,
                );
            } else if has_body {
                self.paint_slot(&slide.text, &resolved.roles.body, body_rect, canvas);
            } else {
                self.paint_slot(
                    &slide.translation_text,
                    &resolved.roles.translation,
                    body_rect,
                    canvas,
                );
            }
        }

        let mut frame = Frame::new(width, height);
        if !surface.read_pixels(&info, &mut frame.pixels, width as usize * 4, (0, 0)) {
            bail!("couldn't read the slide text pixels");
        }
        Ok(frame)
    }

    fn paint_slot(
        &mut self,
        text: &str,
        style: &SlotTextStyle,
        slot_rect: SafeRect,
        canvas: &Canvas,
    ) {
        if !style.visible || text.is_empty() {
            return;
        }
        let families = self.slot_font_families(&style.font_family, style.weight);
        let box_pad_left = style
            .background_box
            .as_ref()
            .map_or(0.0, |b| b.padding_x + b.accent_bar_width);
        let box_pad_right = style.background_box.as_ref().map_or(0.0, |b| b.padding_x);
        let box_pad_y = style.background_box.as_ref().map_or(0.0, |b| b.padding_y);
        let stroke_pad = style.outline.as_ref().map_or(0.0, |o| o.width);
        let shadow_pad = style
            .shadow
            .as_ref()
            .filter(|s| s.color[3] > 0)
            .map_or(0.0, |s| s.offset_x.abs() + s.blur * 0.5);
        let inner_left = slot_rect.left + box_pad_left + stroke_pad + shadow_pad;
        let text_x = inner_left.ceil();
        let right_limit = (slot_rect.right - box_pad_right - stroke_pad - shadow_pad).floor();
        let layout_width = (right_limit - text_x).max(1.0);

        let mut fill_para = build_slot_paragraph(&self.fonts, &families, text, style, None);
        fill_para.layout(layout_width);
        let metrics = measure_laid_out_paragraph(&mut fill_para, layout_width);

        let total_h = metrics.height + box_pad_y * 2.0 + stroke_pad * 2.0;
        let box_top = match style.align_v {
            TextAlignV::Top => slot_rect.top,
            TextAlignV::Middle => {
                slot_rect.top + ((slot_rect.height - total_h) / 2.0).max(0.0).floor()
            }
            TextAlignV::Bottom => (slot_rect.bottom - total_h).max(slot_rect.top).floor(),
        };
        let text_y = (box_top + box_pad_y + stroke_pad).ceil();

        // 1. Optional rounded background card + left accent bar.
        if let Some(bg) = &style.background_box
            && bg.color[3] > 0
        {
            draw_background_box(bg, slot_rect, &metrics, text_x, text_y, stroke_pad, canvas);
        }

        // 2. Optional drop shadow pass.
        if let Some(sh) = &style.shadow
            && sh.color[3] > 0
        {
            let mut sp = Paint::default();
            sp.set_anti_alias(true);
            sp.set_style(PaintStyle::Fill);
            sp.set_color(Color::from_argb(
                sh.color[3],
                sh.color[0],
                sh.color[1],
                sh.color[2],
            ));
            if sh.blur > 0.1 {
                sp.set_mask_filter(MaskFilter::blur(
                    BlurStyle::Normal,
                    (sh.blur * 0.5).max(0.1),
                    false,
                ));
            }
            let mut shadow_para =
                build_slot_paragraph(&self.fonts, &families, text, style, Some(&sp));
            shadow_para.layout(layout_width);
            shadow_para.paint(canvas, (text_x + sh.offset_x, text_y + sh.offset_y));
        }

        // 3. Optional glyph outline pass (2x stroke width so inner half is filled cleanly).
        if let Some(ol) = &style.outline
            && ol.width > 0.0
            && ol.color[3] > 0
        {
            let mut op = Paint::default();
            op.set_anti_alias(true);
            op.set_style(PaintStyle::Stroke);
            op.set_stroke_width((ol.width * 2.0).clamp(0.5, 128.0));
            op.set_stroke_join(Join::Round);
            op.set_stroke_cap(Cap::Round);
            op.set_color(Color::from_argb(
                ol.color[3],
                ol.color[0],
                ol.color[1],
                ol.color[2],
            ));
            let mut outline_para =
                build_slot_paragraph(&self.fonts, &families, text, style, Some(&op));
            outline_para.layout(layout_width);
            outline_para.paint(canvas, (text_x, text_y));
        }

        // 4. Primary glyph fill pass.
        fill_para.paint(canvas, (text_x, text_y));
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_body_with_chords(
        &mut self,
        text: &str,
        chords_by_line: &HashMap<usize, Vec<(usize, String)>>,
        body_style: &SlotTextStyle,
        chord_style: &SlotTextStyle,
        translation: Option<(&str, &SlotTextStyle)>,
        area: SafeRect,
        canvas: &Canvas,
    ) {
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() {
            return;
        }
        let body_families = self.slot_font_families(&body_style.font_family, body_style.weight);
        let chord_families = self.slot_font_families(&chord_style.font_family, chord_style.weight);

        struct LineRow {
            line_text: String,
            chords: Vec<(f32, String)>,
            lyric_h: f32,
            chord_h: f32,
            content_left: f32,
        }

        let stroke_pad = body_style.outline.as_ref().map_or(0.0, |o| o.width);
        let shadow_pad = body_style
            .shadow
            .as_ref()
            .filter(|s| s.color[3] > 0)
            .map_or(0.0, |s| s.offset_x.abs() + s.blur * 0.5);
        let content_left = (area.left + stroke_pad + shadow_pad).ceil();
        let layout_w = ((area.right - stroke_pad - shadow_pad).floor() - content_left).max(1.0);
        let mut rows = Vec::with_capacity(lines.len());
        let mut total_h = stroke_pad * 2.0;
        let line_gap = (body_style.size * 0.2).max(2.0).round();

        for (idx, line) in lines.iter().enumerate() {
            let mut p = build_slot_paragraph(&self.fonts, &body_families, line, body_style, None);
            p.layout(layout_w);
            let m = measure_laid_out_paragraph(&mut p, layout_w);
            let line_chords = chords_by_line.get(&idx).cloned().unwrap_or_default();
            let chord_h = if line_chords.is_empty() {
                0.0
            } else {
                let mut cp =
                    build_slot_paragraph(&self.fonts, &chord_families, "G", chord_style, None);
                cp.layout(layout_w);
                cp.height().ceil()
            };
            let mut placed_chords = Vec::with_capacity(line_chords.len());
            if !line_chords.is_empty() {
                let mut cursor_right = m.min_line_left;
                for (char_pos, chord_text) in line_chords {
                    let byte_idx = line
                        .char_indices()
                        .nth(char_pos)
                        .map_or(line.len(), |(b, _)| b);
                    let prefix = &line[..byte_idx];
                    let prefix_w = if prefix.is_empty() {
                        0.0
                    } else {
                        let mut pp = build_slot_paragraph(
                            &self.fonts,
                            &body_families,
                            prefix,
                            body_style,
                            None,
                        );
                        pp.layout(layout_w);
                        pp.max_intrinsic_width()
                    };
                    let desired_x = (m.min_line_left + prefix_w).max(cursor_right);
                    let mut cp = build_slot_paragraph(
                        &self.fonts,
                        &chord_families,
                        &chord_text,
                        chord_style,
                        None,
                    );
                    cp.layout(layout_w);
                    let cw = cp.max_intrinsic_width().max(chord_style.size * 0.5);
                    let x = desired_x.min((layout_w - cw).max(0.0));
                    cursor_right = x + cw + 8.0;
                    placed_chords.push((x, chord_text));
                }
            }
            total_h += chord_h + m.height;
            if idx + 1 < lines.len() {
                total_h += line_gap;
            }
            rows.push(LineRow {
                line_text: (*line).to_owned(),
                chords: placed_chords,
                lyric_h: m.height,
                chord_h,
                content_left,
            });
        }

        let trans_h = translation.map_or(0.0, |(t, ts)| {
            self.measure_slot(t, ts, area.width)
                .total_height_with_decorations(ts)
                + (body_style.size * 0.3).max(4.0).round()
        });
        let full_h = total_h + trans_h;

        let mut y = match body_style.align_v {
            TextAlignV::Top => area.top + stroke_pad,
            TextAlignV::Middle => {
                area.top + ((area.height - full_h) / 2.0).max(0.0).floor() + stroke_pad
            }
            TextAlignV::Bottom => (area.bottom - full_h).max(area.top).floor() + stroke_pad,
        }
        .ceil();

        for row in rows {
            if row.chord_h > 0.0 {
                let single_chord_style = SlotTextStyle {
                    align_h: TextAlignH::Left,
                    ..chord_style.clone()
                };
                for (cx, chord_text) in row.chords {
                    let cp = build_slot_paragraph(
                        &self.fonts,
                        &chord_families,
                        &chord_text,
                        &single_chord_style,
                        None,
                    );
                    let mut cp = cp;
                    cp.layout(layout_w);
                    cp.paint(canvas, (row.content_left + cx, y));
                }
                y += row.chord_h;
            }
            let mut lp = build_slot_paragraph(
                &self.fonts,
                &body_families,
                &row.line_text,
                body_style,
                None,
            );
            lp.layout(layout_w);
            lp.paint(canvas, (row.content_left, y));
            y += row.lyric_h + line_gap;
        }

        if let Some((t_text, t_style)) = translation {
            let trans_style = SlotTextStyle {
                align_v: TextAlignV::Top,
                ..t_style.clone()
            };
            self.paint_slot(
                t_text,
                &trans_style,
                SafeRect {
                    left: area.left,
                    top: y.min(area.bottom),
                    right: area.right,
                    bottom: area.bottom.max(y + 1.0),
                    width: area.width,
                    height: (area.bottom - y).max(1.0),
                },
                canvas,
            );
        }
    }

    /// Render a solid or pill-backed overlay box with bundled-font text in a deterministic premultiplied RGBA frame.
    pub fn render_overlay_box(
        &mut self,
        text: &str,
        box_rgba: [u8; 4],
        text_rgba: [u8; 4],
        bottom_bar: bool,
        width: u32,
        height: u32,
    ) -> Result<Arc<Frame>> {
        let mut style_hash = blake3::Hasher::new();
        style_hash.update(if bottom_bar { b"bar" } else { b"badge" });
        style_hash.update(&box_rgba);
        style_hash.update(&text_rgba);
        let key = (
            *blake3::hash(text.as_bytes()).as_bytes(),
            *style_hash.finalize().as_bytes(),
            width,
            height,
        );
        if let Some(hit) = self.cache.get(&key) {
            let hit = hit.clone();
            self.hits += 1;
            self.order.retain(|k| *k != key);
            self.order.push(key);
            return Ok(hit);
        }
        self.misses += 1;
        let font_size = (height as f32 * if bottom_bar { 0.042 } else { 0.032 }).clamp(10.0, 64.0);
        let style = SlideTextStyle {
            size: font_size,
            color: text_rgba,
            margin: 0.04,
            line_height: 1.2,
        };
        let mut paragraph = self.paragraph(text, &style);
        let max_text_w = if bottom_bar {
            (width as f32 * 0.92).max(1.0)
        } else {
            (width as f32 * 0.45).max(1.0)
        };
        paragraph.layout(max_text_w);
        let info = ImageInfo::new(
            (width as i32, height as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );
        let mut surface = surfaces::raster(&info, None, None).context("overlay surface")?;
        let canvas = surface.canvas();
        canvas.clear(Color::TRANSPARENT);
        let mut paint = Paint::default();
        paint.set_color(Color::from_argb(
            box_rgba[3],
            box_rgba[0],
            box_rgba[1],
            box_rgba[2],
        ));
        paint.set_anti_alias(false);
        if bottom_bar {
            let bar_h = (paragraph.height() + font_size * 0.8)
                .clamp((height as f32 * 0.12).max(20.0), height as f32 * 0.35)
                .round();
            let top = (height as f32 - bar_h).max(0.0);
            canvas.draw_rect(Rect::from_xywh(0.0, top, width as f32, bar_h), &paint);
            let text_top = top + ((bar_h - paragraph.height()) / 2.0).max(0.0).round();
            let left = (width as f32 - max_text_w) / 2.0;
            paragraph.paint(canvas, (left, text_top));
        } else {
            let text_w = paragraph.max_intrinsic_width().min(max_text_w).max(24.0);
            let pad_x = (font_size * 0.6).round();
            let pad_y = (font_size * 0.35).round();
            let box_w = (text_w + pad_x * 2.0).min(width as f32 * 0.6).round();
            let box_h = (paragraph.height() + pad_y * 2.0)
                .min(height as f32 * 0.3)
                .round();
            let margin_x = (width as f32 * 0.04).round();
            let margin_y = (height as f32 * 0.04).round();
            let left = (width as f32 - margin_x - box_w).max(0.0);
            let top = margin_y;
            canvas.draw_rect(Rect::from_xywh(left, top, box_w, box_h), &paint);
            paragraph.layout((box_w - pad_x * 2.0).max(1.0));
            paragraph.paint(canvas, (left + pad_x, top + pad_y));
        }
        let mut frame = Frame::new(width, height);
        if !surface.read_pixels(&info, &mut frame.pixels, width as usize * 4, (0, 0)) {
            bail!("couldn't read the overlay pixels");
        }
        let frame = Arc::new(frame);
        if self.cache.len() >= self.capacity && !self.order.is_empty() {
            let oldest = self.order.remove(0);
            self.cache.remove(&oldest);
        }
        self.cache.insert(key, frame.clone());
        self.order.push(key);
        Ok(frame)
    }

    /// How many glyphs in `text` no bundled slide font can draw (they'd show as boxes).
    pub fn unresolved_glyphs(&self, text: &str) -> usize {
        let mut paragraph = self.paragraph(text, &SlideTextStyle::default());
        paragraph.layout(1000.0);
        paragraph.unresolved_glyphs().unwrap_or(usize::MAX)
    }

    /// How many glyphs in `text` a specific font family + weight leaves unresolved.
    pub fn unresolved_glyphs_for(&mut self, text: &str, family: &str, weight: u16) -> usize {
        let style = SlotTextStyle {
            font_family: family.to_owned(),
            weight,
            ..SlotTextStyle::default()
        };
        self.measure_slot(text, &style, 1000.0).unresolved_glyphs
    }

    fn paragraph(&self, text: &str, style: &SlideTextStyle) -> Paragraph {
        let mut paragraph_style = ParagraphStyle::new();
        paragraph_style.set_text_align(TextAlign::Center);
        // No hinting: slide text is large, and hinting is where DirectWrite (Windows), FreeType
        // (Linux) and CoreText (macOS) differ most, so this keeps frames alike everywhere.
        paragraph_style.turn_hinting_off();
        paragraph_style.set_text_direction(if is_rtl(text) {
            TextDirection::RTL
        } else {
            TextDirection::LTR
        });
        let mut text_style = TextStyle::new();
        text_style.set_font_families(&self.families);
        text_style.set_font_size(style.size);
        text_style.set_height(style.line_height);
        text_style.set_height_override(true);
        let [r, g, b, a] = style.color;
        text_style.set_color(Color::from_argb(a, r, g, b));
        let mut builder = ParagraphBuilder::new(&paragraph_style, &self.fonts);
        builder.push_style(&text_style);
        builder.add_text(text);
        builder.build()
    }

    fn rasterise(
        &self,
        text: &str,
        style: &SlideTextStyle,
        width: u32,
        height: u32,
    ) -> Result<Frame> {
        let mut paragraph = self.paragraph(text, style);
        let margin = (width as f32 * style.margin).round();
        let layout_width = (width as f32 - 2.0 * margin).max(1.0);
        paragraph.layout(layout_width);

        let info = ImageInfo::new(
            (width as i32, height as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );
        let mut surface = surfaces::raster(&info, None, None).context("text surface")?;
        let canvas = surface.canvas();
        canvas.clear(Color::TRANSPARENT);
        let top = ((height as f32 - paragraph.height()) / 2.0)
            .max(0.0)
            .round();
        paragraph.paint(canvas, (margin, top));
        let mut frame = Frame::new(width, height);
        if !surface.read_pixels(&info, &mut frame.pixels, width as usize * 4, (0, 0)) {
            bail!("couldn't read the text pixels");
        }
        Ok(frame)
    }
}

fn build_slot_paragraph(
    fonts: &FontCollection,
    families: &[String],
    text: &str,
    style: &SlotTextStyle,
    foreground_paint: Option<&Paint>,
) -> Paragraph {
    let mut paragraph_style = ParagraphStyle::new();
    let align = match style.align_h {
        TextAlignH::Left => TextAlign::Left,
        TextAlignH::Center => TextAlign::Center,
        TextAlignH::Right => TextAlign::Right,
        TextAlignH::Justify => TextAlign::Justify,
    };
    paragraph_style.set_text_align(align);
    // Keep cross-platform Skia metrics identical between DirectWrite / FreeType / CoreText.
    paragraph_style.turn_hinting_off();
    paragraph_style.set_text_direction(if is_rtl(text) {
        TextDirection::RTL
    } else {
        TextDirection::LTR
    });
    if let Some(max_lines) = style.max_lines
        && max_lines > 0
    {
        paragraph_style.set_max_lines(max_lines as usize);
        paragraph_style.set_ellipsis("…");
    }

    let mut text_style = TextStyle::new();
    text_style.set_font_families(families);
    text_style.set_font_size(style.size.max(8.0));
    text_style.set_height(style.line_height.clamp(0.8, 4.0));
    text_style.set_height_override(true);
    if style.letter_spacing.abs() > 0.01 {
        text_style.set_letter_spacing(style.letter_spacing);
    }
    if let Some(paint) = foreground_paint {
        text_style.set_foreground_paint(paint);
    } else {
        let [r, g, b, a] = style.color;
        text_style.set_color(Color::from_argb(a, r, g, b));
    }

    let mut builder = ParagraphBuilder::new(&paragraph_style, fonts);
    builder.push_style(&text_style);
    builder.add_text(text);
    builder.build()
}

fn measure_laid_out_paragraph(p: &mut Paragraph, layout_width: f32) -> SlotMeasurement {
    let line_metrics = p.get_line_metrics();
    let mut line_widths = Vec::with_capacity(line_metrics.len());
    let mut min_left = f32::MAX;
    let mut max_right = 0.0f32;
    for lm in &line_metrics {
        let w = lm.width as f32;
        let l = lm.left as f32;
        line_widths.push(w);
        if w > 0.0 {
            min_left = min_left.min(l);
            max_right = max_right.max(l + w);
        }
    }
    if min_left == f32::MAX {
        min_left = 0.0;
        max_right = 0.0;
    }
    SlotMeasurement {
        layout_width,
        height: p.height().ceil(),
        min_intrinsic_width: p.min_intrinsic_width().ceil(),
        max_intrinsic_width: p.max_intrinsic_width().ceil(),
        longest_line: p.longest_line().ceil(),
        line_count: line_metrics.len(),
        line_widths,
        min_line_left: min_left,
        max_line_right: max_right,
        did_exceed_max_lines: p.did_exceed_max_lines(),
        unresolved_glyphs: p.unresolved_glyphs().unwrap_or(0),
    }
}

fn draw_background_box(
    bg: &BackgroundBoxStyle,
    slot_rect: SafeRect,
    metrics: &SlotMeasurement,
    text_x: f32,
    text_y: f32,
    stroke_pad: f32,
    canvas: &Canvas,
) {
    let box_top = (text_y - bg.padding_y - stroke_pad).max(slot_rect.top);
    let box_bottom = (text_y + metrics.height + bg.padding_y + stroke_pad).min(slot_rect.bottom);
    let (box_left, box_right) = if bg.full_width {
        (slot_rect.left, slot_rect.right)
    } else if metrics.max_right_nonzero() {
        let l = (text_x + metrics.min_line_left - bg.padding_x - bg.accent_bar_width - stroke_pad)
            .max(slot_rect.left);
        let r = (text_x + metrics.max_line_right + bg.padding_x + stroke_pad).min(slot_rect.right);
        (l, r.max(l + 1.0))
    } else {
        (slot_rect.left, slot_rect.right)
    };

    let rect = Rect::from_xywh(
        box_left.round(),
        box_top.round(),
        (box_right - box_left).max(1.0).round(),
        (box_bottom - box_top).max(1.0).round(),
    );
    let rrect = RRect::new_rect_xy(rect, bg.radius, bg.radius);
    let mut bg_paint = Paint::default();
    bg_paint.set_anti_alias(true);
    bg_paint.set_color(Color::from_argb(
        bg.color[3],
        bg.color[0],
        bg.color[1],
        bg.color[2],
    ));
    canvas.draw_rrect(rrect, &bg_paint);

    if let Some(bar_rgba) = bg.accent_bar_color
        && bg.accent_bar_width > 0.0
        && bar_rgba[3] > 0
    {
        canvas.save();
        canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);
        let mut bar_paint = Paint::default();
        bar_paint.set_anti_alias(false);
        bar_paint.set_color(Color::from_argb(
            bar_rgba[3],
            bar_rgba[0],
            bar_rgba[1],
            bar_rgba[2],
        ));
        let bar_rect = Rect::from_xywh(
            rect.left,
            rect.top,
            bg.accent_bar_width.min(rect.width()),
            rect.height(),
        );
        canvas.draw_rect(bar_rect, &bar_paint);
        canvas.restore();
    }
}

impl SlotMeasurement {
    fn max_right_nonzero(&self) -> bool {
        self.max_line_right > self.min_line_left
    }
}

/// Composite premultiplied `layer` over `base` in place (source-over).
pub fn composite_over(base: &mut Frame, layer: &Frame) {
    debug_assert_eq!((base.width, base.height), (layer.width, layer.height));
    for (dst, src) in base
        .pixels
        .chunks_exact_mut(4)
        .zip(layer.pixels.chunks_exact(4))
    {
        let inverse = 255 - u32::from(src[3]);
        for channel in 0..4 {
            let blended = u32::from(src[channel]) + (u32::from(dst[channel]) * inverse + 127) / 255;
            dst[channel] = blended.min(255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_follows_the_first_letter() {
        assert!(is_rtl("الرب راعيّ"));
        assert!(is_rtl("  «الرب»"));
        assert!(!is_rtl("Amazing grace"));
        assert!(!is_rtl("주님은"));
    }

    #[test]
    fn composite_over_black_keeps_premultiplied_source() {
        let mut base = Frame::new(1, 1);
        base.pixels.copy_from_slice(&[0, 0, 0, 255]);
        let mut layer = Frame::new(1, 1);
        layer.pixels.copy_from_slice(&[128, 128, 128, 128]);
        composite_over(&mut base, &layer);
        assert_eq!(base.pixels, [128, 128, 128, 255]);
    }

    #[test]
    fn canonical_font_aliases_map_only_to_bundled_fonts() {
        assert_eq!(canonical_font_family("Inter"), "Inter");
        assert_eq!(canonical_font_family("manrope"), "Manrope");
        assert_eq!(canonical_font_family("jetbrains-mono"), "JetBrains Mono");
        assert_eq!(canonical_font_family("Noto Serif"), "Noto Serif");
        assert_eq!(canonical_font_family("Arial"), "Noto Sans");
    }
}
