//! Slide text (ADR-005): Skia CPU raster with HarfBuzz shaping, using only the bundled slide
//! fonts (never system fonts, so output matches on every computer). Rendered paragraphs are
//! cached by content, style and resolution, so a slide change only swaps a texture.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use skia_safe::font_arguments::VariationPosition;
use skia_safe::font_arguments::variation_position::Coordinate;
use skia_safe::textlayout::{
    FontCollection, ParagraphBuilder, ParagraphStyle, TextAlign, TextDirection, TextStyle,
    TypefaceFontProvider,
};
use skia_safe::{
    AlphaType, Color, ColorType, FontArguments, FontMgr, FourByteTag, ImageInfo, surfaces,
};

use crate::Frame;

/// The slide font families, in fallback order. Each file is bundled (OFL, ADR-009).
pub const SLIDE_FONTS: [(&str, &str); 3] = [
    ("Noto Sans", "notosans/NotoSans[wdth,wght].ttf"),
    ("Noto Sans KR", "notosanskr/NotoSansKR[wght].ttf"),
    (
        "Noto Naskh Arabic",
        "notonaskharabic/NotoNaskhArabic[wght].ttf",
    ),
];

/// The weight every slide font is drawn at in M1 (regular).
pub const SLIDE_WEIGHT: f32 = 400.0;

/// How a block of slide text looks. Sizes are in output pixels.
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

type CacheKey = ([u8; 32], [u8; 32], u32, u32);

pub struct TextRenderer {
    fonts: FontCollection,
    families: Vec<String>,
    cache: HashMap<CacheKey, Arc<Frame>>,
    order: Vec<CacheKey>,
    capacity: usize,
    hits: u64,
    misses: u64,
}

impl TextRenderer {
    pub fn new(dir: &Path) -> Result<Self> {
        let decoder = FontMgr::new();
        let mut provider = TypefaceFontProvider::new();
        let mut families = Vec::new();
        for (family, file) in SLIDE_FONTS {
            let path = dir.join(file);
            let bytes = std::fs::read(&path)
                .with_context(|| format!("slide font {} is missing", path.display()))?;
            let typeface = decoder
                .new_from_data(skia_safe::Data::new_copy(&bytes), None)
                .with_context(|| format!("slide font {} couldn't be read", path.display()))?;
            // These are variable fonts, and some default to Thin (Noto Sans KR). Pin every
            // script to the same regular weight; templates choose weights from M3.
            let regular = [Coordinate {
                axis: FourByteTag::from_chars('w', 'g', 'h', 't'),
                value: SLIDE_WEIGHT,
            }];
            let arguments = FontArguments::new().set_variation_design_position(VariationPosition {
                coordinates: &regular,
            });
            let typeface = typeface
                .clone_with_arguments(&arguments)
                .unwrap_or(typeface);
            provider.register_typeface(typeface, Some(family));
            families.push(family.to_owned());
        }
        let mut fonts = FontCollection::new();
        fonts.set_asset_font_manager(Some(provider.into()));
        // No system fonts: what's on the slides mustn't depend on the computer.
        fonts.set_default_font_manager(None, None);
        fonts.enable_font_fallback();
        Ok(Self {
            fonts,
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

    /// Render `text` centred in a `width` × `height` transparent frame (premultiplied RGBA).
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
        let mut paint = skia_safe::Paint::default();
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
            canvas.draw_rect(
                skia_safe::Rect::from_xywh(0.0, top, width as f32, bar_h),
                &paint,
            );
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
            canvas.draw_rect(skia_safe::Rect::from_xywh(left, top, box_w, box_h), &paint);
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

    /// How many glyphs in `text` no slide font can draw (they'd show as boxes).
    pub fn unresolved_glyphs(&self, text: &str) -> usize {
        let mut paragraph = self.paragraph(text, &SlideTextStyle::default());
        paragraph.layout(1000.0);
        paragraph.unresolved_glyphs().unwrap_or(usize::MAX)
    }

    fn paragraph(&self, text: &str, style: &SlideTextStyle) -> skia_safe::textlayout::Paragraph {
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
}
