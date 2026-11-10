//! Structured custom slide elements, shapes, images, videos, clocks, and validation (M3.7 / M3-A5).
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use selah_proto::v1::{EffectKind, EffectSpec, MediaFitMode};
use serde::{Deserialize, Serialize};
use skia_safe::paint::{Cap, Style as PaintStyle};
use skia_safe::{
    AlphaType, ClipOp, Color, ColorType, Data, FilterMode, ImageInfo, MipmapMode, Paint, RRect,
    Rect, SamplingOptions, images, surfaces,
};

use crate::Frame;
use crate::effects::{MAX_EFFECTS_PER_CHAIN, check_forbidden_shader_keys, validate_one_effect};
use crate::media_bg::{
    MAX_BLUR_RADIUS, compute_fit_rect, load_verified_image, render_image_frame,
    validate_media_address,
};
use crate::template::{
    BackgroundBoxStyle, OutlineStyle, SafeRect, ShadowStyle, SlotTextStyle, TextAlignH, TextAlignV,
    deserialize_rgba, deserialize_rgba_opt, serialize_rgba,
};
use crate::text::{TextRenderer, canonical_font_family};
use crate::video::{Nv12Frame, draw_cpu_layer};

/// Maximum serialized `elements_json` size accepted across library and render IPC (`SEC-03`).
pub const MAX_ELEMENTS_JSON_BYTES: usize = 131_072;
/// Maximum number of elements permitted on a single custom slide (`SEC-03`).
pub const MAX_ELEMENTS: usize = 64;
/// Default design-time reference height matching `TemplateStyle::reference_height`.
pub const DEFAULT_ELEMENT_REFERENCE_HEIGHT: u32 = 1080;

fn default_schema_version() -> u32 {
    1
}

fn default_reference_height() -> u32 {
    DEFAULT_ELEMENT_REFERENCE_HEIGHT
}

fn default_true() -> bool {
    true
}

fn default_one() -> f32 {
    1.0
}

fn default_font_family() -> String {
    "Noto Sans".to_owned()
}

fn default_font_size() -> f32 {
    48.0
}

fn default_weight() -> u16 {
    400
}

fn default_line_height() -> f32 {
    1.25
}

fn default_white() -> [u8; 4] {
    [255, 255, 255, 255]
}

fn default_shape_fill() -> [u8; 4] {
    [35, 120, 195, 224]
}

fn default_fit() -> String {
    "fit".to_owned()
}

fn default_padding() -> f32 {
    8.0
}

fn default_corner_radius() -> f32 {
    12.0
}

/// Source abstraction for live NV12 video frames so CPU (`HashMap<String, Nv12Frame>`) and GPU
/// (`HashMap<String, (u64, Nv12Frame)>`) compositors pass video frames without extra clones.
pub trait VideoFrameSource {
    fn get_video(&self, hash: &str) -> Option<&Nv12Frame>;
}

impl VideoFrameSource for std::collections::HashMap<String, Nv12Frame> {
    fn get_video(&self, hash: &str) -> Option<&Nv12Frame> {
        self.get(hash)
    }
}

impl VideoFrameSource for std::collections::HashMap<String, (u64, Nv12Frame)> {
    fn get_video(&self, hash: &str) -> Option<&Nv12Frame> {
        self.get(hash).map(|(_, frame)| frame)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlideElementKind {
    Text,
    Shape,
    Image,
    Video,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    #[default]
    Rect,
    RoundedRect,
    Ellipse,
    Line,
}

/// Serializable closed effect entry embedded in custom slide JSON.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlideEffectEntry {
    pub kind: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_one")]
    pub intensity: f32,
    #[serde(default)]
    pub params_json: String,
}

/// Serializable background metadata embedded in custom slide JSON.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlideBackgroundSpec {
    pub mode: String,
    #[serde(default)]
    pub media_id: String,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default)]
    pub media_extension: String,
    #[serde(default)]
    pub media_kind: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rgba_opt"
    )]
    pub solid_color: Option<[u8; 4]>,
    #[serde(default = "default_fit")]
    pub fit: String,
    #[serde(default)]
    pub blur_radius: f32,
    #[serde(default = "default_one")]
    pub opacity: f32,
}

/// Serializable transition metadata embedded in custom slide JSON.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlideTransitionSpec {
    pub kind: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub easing: String,
    #[serde(default)]
    pub duration_ms: u32,
}

/// A single positioned element inside a custom slide canvas (`0.0..=1.0` normalized coordinates).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlideElement {
    pub id: String,
    pub name: String,
    pub kind: SlideElementKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default = "default_one")]
    pub opacity: f32,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub z_index: i32,

    // Text properties
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_weight")]
    pub weight: u16,
    #[serde(
        default = "default_white",
        serialize_with = "serialize_rgba",
        deserialize_with = "deserialize_rgba"
    )]
    pub color: [u8; 4],
    #[serde(default)]
    pub align_h: TextAlignH,
    #[serde(default)]
    pub align_v: TextAlignV,
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    #[serde(default)]
    pub letter_spacing: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<OutlineStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<ShadowStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_box: Option<BackgroundBoxStyle>,
    #[serde(default = "default_padding")]
    pub padding: f32,

    // Dynamic `{clock}` and `{timer}` formatting
    #[serde(default = "default_true")]
    pub clock_show_seconds: bool,
    #[serde(default = "default_true")]
    pub clock_24h: bool,
    #[serde(default)]
    pub clock_offset_minutes: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer_target_unix_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer_duration_secs: Option<u32>,
    #[serde(default)]
    pub timer_count_up: bool,

    // Shape properties
    #[serde(default)]
    pub shape_kind: ShapeKind,
    #[serde(
        default = "default_shape_fill",
        serialize_with = "serialize_rgba",
        deserialize_with = "deserialize_rgba"
    )]
    pub fill_color: [u8; 4],
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rgba_opt"
    )]
    pub stroke_color: Option<[u8; 4]>,
    #[serde(default)]
    pub stroke_width: f32,
    #[serde(default = "default_corner_radius")]
    pub corner_radius: f32,

    // Image / video properties
    #[serde(default)]
    pub content_hash: String,
    #[serde(default)]
    pub media_extension: String,
    #[serde(default = "default_fit")]
    pub fit: String,
    #[serde(default)]
    pub blur_radius: f32,
}

impl Default for SlideElement {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: SlideElementKind::Text,
            group_id: None,
            x: 0.1,
            y: 0.1,
            width: 0.8,
            height: 0.25,
            opacity: 1.0,
            visible: true,
            locked: false,
            z_index: 0,
            text: String::new(),
            font_family: default_font_family(),
            font_size: default_font_size(),
            weight: default_weight(),
            color: default_white(),
            align_h: TextAlignH::Center,
            align_v: TextAlignV::Middle,
            line_height: default_line_height(),
            letter_spacing: 0.0,
            outline: None,
            shadow: None,
            background_box: None,
            padding: default_padding(),
            clock_show_seconds: true,
            clock_24h: true,
            clock_offset_minutes: 0,
            timer_target_unix_ms: None,
            timer_duration_secs: None,
            timer_count_up: false,
            shape_kind: ShapeKind::Rect,
            fill_color: default_shape_fill(),
            stroke_color: None,
            stroke_width: 0.0,
            corner_radius: default_corner_radius(),
            content_hash: String::new(),
            media_extension: String::new(),
            fit: default_fit(),
            blur_radius: 0.0,
        }
    }
}

/// Complete custom slide element document serialized into `SlideLayerSpec.elements_json`,
/// `PropsLayerSpec.elements_json`, or `Slide.elements_json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlideElementsDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_reference_height")]
    pub reference_height: u32,
    #[serde(default)]
    pub elements: Vec<SlideElement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<SlideBackgroundSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<SlideTransitionSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<SlideEffectEntry>,
}

impl Default for SlideElementsDocument {
    fn default() -> Self {
        Self {
            schema_version: 1,
            reference_height: DEFAULT_ELEMENT_REFERENCE_HEIGHT,
            elements: Vec::new(),
            background: None,
            transition: None,
            effects: Vec::new(),
        }
    }
}

fn parse_effect_kind_name(raw: &str) -> Result<EffectKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "blur" => Ok(EffectKind::Blur),
        "colour_adjust" | "color_adjust" => Ok(EffectKind::ColourAdjust),
        "vignette" => Ok(EffectKind::Vignette),
        "background_blur" => Ok(EffectKind::BackgroundBlur),
        "colour_key" | "color_key" => Ok(EffectKind::ColourKey),
        "mask" => Ok(EffectKind::Mask),
        other => bail!("unsupported built-in effect '{other}' (SEC-04)"),
    }
}

pub fn parse_fit_mode(raw: &str) -> MediaFitMode {
    match raw.trim().to_ascii_lowercase().as_str() {
        "fill" | "cover" => MediaFitMode::Fill,
        "stretch" => MediaFitMode::Stretch,
        _ => MediaFitMode::Fit,
    }
}

fn resolve_ext(ext: &str, kind: SlideElementKind) -> String {
    let trimmed = ext.trim();
    if trimmed.is_empty() {
        match kind {
            SlideElementKind::Video => "mp4".to_owned(),
            _ => "png".to_owned(),
        }
    } else {
        trimmed.to_owned()
    }
}

fn validate_effect_entry(entry: &SlideEffectEntry) -> Result<()> {
    let kind = parse_effect_kind_name(&entry.kind)?;
    ensure!(
        entry.intensity.is_finite() && (0.0..=1.0).contains(&entry.intensity),
        "effect intensity must be finite in 0.0..=1.0"
    );
    let spec = EffectSpec {
        kind: kind as i32,
        enabled: entry.enabled,
        intensity: entry.intensity,
        params_json: entry.params_json.clone(),
    };
    let _ = validate_one_effect(&spec)?;
    Ok(())
}

impl SlideElement {
    pub fn validate_strict(&mut self) -> Result<()> {
        ensure!(self.id.len() <= 128, "element id exceeds 128 bytes");
        ensure!(self.name.len() <= 256, "element name exceeds 256 bytes");
        if let Some(g) = &self.group_id {
            ensure!(g.len() <= 128, "element group_id exceeds 128 bytes");
        }
        ensure!(
            self.x.is_finite()
                && self.y.is_finite()
                && self.width.is_finite()
                && self.height.is_finite(),
            "element geometry must be finite"
        );
        ensure!(
            (-0.5..=1.0).contains(&self.x) && (-0.5..=1.0).contains(&self.y),
            "element origin ({}, {}) out of bounds",
            self.x,
            self.y
        );
        ensure!(
            (0.0..=1.5).contains(&self.width) && (0.0..=1.5).contains(&self.height),
            "element size ({}, {}) out of bounds",
            self.width,
            self.height
        );
        if self.kind == SlideElementKind::Shape && self.shape_kind == ShapeKind::Line {
            ensure!(
                self.width > 0.0 || self.height > 0.0,
                "line element must have non-zero span"
            );
        } else {
            ensure!(
                self.width > 0.0 && self.height > 0.0,
                "element width and height must be positive"
            );
        }
        ensure!(
            self.opacity.is_finite() && (0.0..=1.0).contains(&self.opacity),
            "element opacity must be finite in 0.0..=1.0"
        );
        ensure!(
            (-10_000..=10_000).contains(&self.z_index),
            "element z_index out of bounds"
        );

        ensure!(self.text.len() <= 16_384, "element text exceeds 16 KiB");
        ensure!(
            self.font_size.is_finite() && (8.0..=400.0).contains(&self.font_size),
            "element font_size must be in 8..=400"
        );
        ensure!(
            (100..=900).contains(&self.weight),
            "element font weight must be in 100..=900"
        );
        ensure!(
            self.line_height.is_finite() && (0.8..=4.0).contains(&self.line_height),
            "element line_height out of bounds"
        );
        ensure!(
            self.letter_spacing.is_finite() && (-24.0..=64.0).contains(&self.letter_spacing),
            "element letter_spacing out of bounds"
        );
        ensure!(
            self.padding.is_finite() && (0.0..=240.0).contains(&self.padding),
            "element padding out of bounds"
        );
        ensure!(
            (-840..=840).contains(&self.clock_offset_minutes),
            "clock_offset_minutes out of bounds"
        );
        ensure!(
            self.stroke_width.is_finite() && (0.0..=64.0).contains(&self.stroke_width),
            "shape stroke_width out of bounds"
        );
        ensure!(
            self.corner_radius.is_finite() && (0.0..=256.0).contains(&self.corner_radius),
            "shape corner_radius out of bounds"
        );
        ensure!(
            self.blur_radius.is_finite() && (0.0..=MAX_BLUR_RADIUS).contains(&self.blur_radius),
            "element blur_radius out of bounds"
        );

        self.font_family = canonical_font_family(&self.font_family).to_owned();
        if let Some(ol) = self.outline.take() {
            self.outline = Some(ol.clamped());
        }
        if let Some(sh) = self.shadow.take() {
            self.shadow = Some(sh.clamped());
        }
        if let Some(bg) = self.background_box.take() {
            self.background_box = Some(bg.clamped());
        }

        if matches!(self.kind, SlideElementKind::Image | SlideElementKind::Video) {
            let hash = self.content_hash.trim().to_owned();
            let ext = resolve_ext(&self.media_extension, self.kind);
            validate_media_address(&hash, &ext)
                .with_context(|| format!("invalid media reference on {:?} element", self.kind))?;
            self.content_hash = hash;
            self.media_extension = ext;
        }
        Ok(())
    }

    pub fn is_time_varying(&self) -> bool {
        if !self.visible || self.opacity <= 0.0 {
            return false;
        }
        match self.kind {
            SlideElementKind::Video => true,
            SlideElementKind::Text => {
                self.text.contains("{clock}") || self.text.contains("{timer}")
            }
            SlideElementKind::Shape | SlideElementKind::Image => false,
        }
    }
}

impl SlideElementsDocument {
    /// Strict validator for core IPC, library `Presentation` writes, and thumbnail rendering (`SEC-03`, `SEC-04`).
    pub fn from_json_strict(raw: &str) -> Result<Self> {
        ensure!(
            raw.len() <= MAX_ELEMENTS_JSON_BYTES,
            "elements_json exceeds {MAX_ELEMENTS_JSON_BYTES} byte limit (SEC-03)"
        );
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(Self::default());
        }

        let value: serde_json::Value =
            serde_json::from_str(trimmed).context("elements_json must be valid JSON")?;
        check_forbidden_shader_keys(&value)?;

        let mut doc = match value {
            serde_json::Value::Array(_) => {
                let elements: Vec<SlideElement> =
                    serde_json::from_value(value).context("invalid SlideElement array")?;
                Self {
                    elements,
                    ..Self::default()
                }
            }
            serde_json::Value::Object(_) => serde_json::from_value::<Self>(value)
                .context("invalid SlideElementsDocument JSON")?,
            _ => bail!("elements_json must be a JSON object or array"),
        };

        ensure!(
            doc.elements.len() <= MAX_ELEMENTS,
            "slide contains {} elements (maximum is {MAX_ELEMENTS})",
            doc.elements.len()
        );
        if doc.reference_height == 0 {
            doc.reference_height = DEFAULT_ELEMENT_REFERENCE_HEIGHT;
        }
        ensure!(
            (64..=4320).contains(&doc.reference_height),
            "reference_height must be in 64..=4320"
        );

        for element in &mut doc.elements {
            element.validate_strict()?;
        }

        if let Some(bg) = &doc.background {
            ensure!(
                bg.opacity.is_finite() && (0.0..=1.0).contains(&bg.opacity),
                "background opacity out of bounds"
            );
            ensure!(
                bg.blur_radius.is_finite() && (0.0..=MAX_BLUR_RADIUS).contains(&bg.blur_radius),
                "background blur_radius out of bounds"
            );
            let hash = bg.content_hash.trim();
            if !hash.is_empty() {
                let ext = if bg.media_extension.trim().is_empty() {
                    if bg.media_kind == "video" {
                        "mp4"
                    } else {
                        "png"
                    }
                } else {
                    bg.media_extension.trim()
                };
                validate_media_address(hash, ext)?;
            }
        }

        ensure!(
            doc.effects.len() <= MAX_EFFECTS_PER_CHAIN,
            "slide elements effects chain exceeds {MAX_EFFECTS_PER_CHAIN}"
        );
        for effect in &doc.effects {
            validate_effect_entry(effect)?;
        }

        Ok(doc)
    }

    /// Lenient parse for defensive compositor rendering (returns empty document on invalid JSON).
    pub fn from_json_lenient(raw: &str) -> Self {
        match Self::from_json_strict(raw) {
            Ok(doc) => doc,
            Err(error) => {
                if !raw.trim().is_empty() {
                    tracing::warn!(%error, "ignoring malformed elements_json on layer");
                }
                Self::default()
            }
        }
    }

    pub fn to_json(&self) -> Result<String> {
        let json = serde_json::to_string(self)?;
        ensure!(
            json.len() <= MAX_ELEMENTS_JSON_BYTES,
            "serialized elements_json exceeds {MAX_ELEMENTS_JSON_BYTES} bytes"
        );
        Ok(json)
    }

    pub fn has_visible_content(&self) -> bool {
        self.elements.iter().any(|e| e.visible && e.opacity > 0.0)
    }

    pub fn has_time_varying_content(&self) -> bool {
        self.elements.iter().any(SlideElement::is_time_varying)
    }

    /// Collect `(content_hash, extension, is_video)` references across visible image/video elements.
    pub fn collect_media_refs(&self) -> Vec<(String, String, bool)> {
        let mut out = Vec::new();
        for el in &self.elements {
            if !el.visible || el.opacity <= 0.0 {
                continue;
            }
            match el.kind {
                SlideElementKind::Image => {
                    let hash = el.content_hash.trim();
                    if !hash.is_empty() {
                        out.push((
                            hash.to_owned(),
                            resolve_ext(&el.media_extension, el.kind),
                            false,
                        ));
                    }
                }
                SlideElementKind::Video => {
                    let hash = el.content_hash.trim();
                    if !hash.is_empty() {
                        out.push((
                            hash.to_owned(),
                            resolve_ext(&el.media_extension, el.kind),
                            true,
                        ));
                    }
                }
                SlideElementKind::Text | SlideElementKind::Shape => {}
            }
        }
        out
    }

    pub fn aligned_positions(elements: &mut [SlideElement], indices: &[usize], mode: AlignMode) {
        if indices.is_empty() {
            return;
        }
        if indices.len() == 1 {
            let idx = indices[0];
            if let Some(el) = elements.get_mut(idx)
                && !el.locked
            {
                match mode {
                    AlignMode::Left => el.x = 0.05,
                    AlignMode::Center => el.x = (1.0 - el.width) * 0.5,
                    AlignMode::Right => el.x = (0.95 - el.width).clamp(0.0, 1.0),
                    AlignMode::Top => el.y = 0.05,
                    AlignMode::Middle => el.y = (1.0 - el.height) * 0.5,
                    AlignMode::Bottom => el.y = (0.95 - el.height).clamp(0.0, 1.0),
                }
            }
            return;
        }
        let mut min_x = f32::MAX;
        let mut max_r = f32::MIN;
        let mut min_y = f32::MAX;
        let mut max_b = f32::MIN;
        for &idx in indices {
            if let Some(el) = elements.get(idx) {
                min_x = min_x.min(el.x);
                max_r = max_r.max(el.x + el.width);
                min_y = min_y.min(el.y);
                max_b = max_b.max(el.y + el.height);
            }
        }
        let mid_x = (min_x + max_r) * 0.5;
        let mid_y = (min_y + max_b) * 0.5;
        for &idx in indices {
            if let Some(el) = elements.get_mut(idx)
                && !el.locked
            {
                match mode {
                    AlignMode::Left => el.x = min_x,
                    AlignMode::Center => el.x = mid_x - el.width * 0.5,
                    AlignMode::Right => el.x = max_r - el.width,
                    AlignMode::Top => el.y = min_y,
                    AlignMode::Middle => el.y = mid_y - el.height * 0.5,
                    AlignMode::Bottom => el.y = max_b - el.height,
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignMode {
    Left,
    Center,
    Right,
    Top,
    Middle,
    Bottom,
}

/// Format `{clock}` and `{timer}` tokens inside `raw_text` using `clock_unix_ms`.
#[allow(clippy::too_many_arguments)]
pub fn format_dynamic_tokens(
    raw_text: &str,
    clock_unix_ms: i64,
    offset_minutes: i32,
    show_seconds: bool,
    clock_24h: bool,
    timer_target_unix_ms: Option<i64>,
    timer_duration_secs: Option<u32>,
    timer_count_up: bool,
) -> String {
    if !raw_text.contains("{clock}") && !raw_text.contains("{timer}") {
        return raw_text.to_owned();
    }
    let shifted_ms =
        clock_unix_ms.saturating_add(i64::from(offset_minutes.clamp(-840, 840)) * 60_000);
    let sec_of_day = shifted_ms.div_euclid(1000).rem_euclid(86_400);
    let h24 = (sec_of_day / 3600) as u32;
    let m = ((sec_of_day % 3600) / 60) as u32;
    let s = (sec_of_day % 60) as u32;
    let clock_str = if clock_24h {
        if show_seconds {
            format!("{h24:02}:{m:02}:{s:02}")
        } else {
            format!("{h24:02}:{m:02}")
        }
    } else {
        let suffix = if h24 < 12 { "AM" } else { "PM" };
        let h12 = match h24 % 12 {
            0 => 12,
            v => v,
        };
        if show_seconds {
            format!("{h12}:{m:02}:{s:02} {suffix}")
        } else {
            format!("{h12}:{m:02} {suffix}")
        }
    };

    let delta_secs = if let Some(target) = timer_target_unix_ms {
        target.saturating_sub(clock_unix_ms).div_euclid(1000)
    } else {
        i64::from(timer_duration_secs.unwrap_or(300))
    };
    let (overrun_prefix, abs_secs) = if delta_secs < 0 {
        if timer_count_up {
            ("+", delta_secs.saturating_neg())
        } else {
            ("", 0)
        }
    } else {
        ("", delta_secs)
    };
    let th = abs_secs / 3600;
    let tm = (abs_secs % 3600) / 60;
    let ts = abs_secs % 60;
    let timer_str = if th > 0 {
        format!("{overrun_prefix}{th}:{tm:02}:{ts:02}")
    } else {
        format!("{overrun_prefix}{tm:02}:{ts:02}")
    };

    raw_text
        .replace("{clock}", &clock_str)
        .replace("{timer}", &timer_str)
}

fn to_skia_color(c: [u8; 4]) -> Color {
    Color::from_argb(c[3], c[0], c[1], c[2])
}

/// Rasterise all visible elements in deterministic `(z_index, index)` order into a `width x height`
/// premultiplied RGBA [`Frame`].
pub fn rasterise_elements(
    doc: &SlideElementsDocument,
    width: u32,
    height: u32,
    renderer: &mut TextRenderer,
    media_root: Option<&Path>,
    video_frames: &impl VideoFrameSource,
    clock_unix_ms: i64,
) -> Result<Frame> {
    let w = width.max(1);
    let h = height.max(1);
    let info = ImageInfo::new(
        (w as i32, h as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let mut surface = surfaces::raster(&info, None, None).context("slide elements surface")?;
    {
        let canvas = surface.canvas();
        canvas.clear(Color::TRANSPARENT);

        let mut order: Vec<(i32, usize)> = doc
            .elements
            .iter()
            .enumerate()
            .filter(|(_, e)| e.visible && e.opacity > 0.0)
            .map(|(idx, e)| (e.z_index, idx))
            .collect();
        order.sort_by_key(|&(z, idx)| (z, idx));

        let scale = h as f32 / doc.reference_height.max(1) as f32;

        for (_, idx) in order {
            let el = &doc.elements[idx];
            let rx = el.x * w as f32;
            let ry = el.y * h as f32;
            let rw = (el.width * w as f32).max(1.0);
            let rh = (el.height * h as f32).max(1.0);
            let rect = Rect::from_xywh(rx, ry, rw, rh);

            let has_layer_alpha = el.opacity < 0.999;
            if has_layer_alpha {
                let bounds = Rect::from_xywh(0.0, 0.0, w as f32, h as f32);
                canvas.save_layer_alpha_f(Some(bounds), el.opacity.clamp(0.0, 1.0));
            }

            match el.kind {
                SlideElementKind::Shape => {
                    draw_shape_element(canvas, el, rect, scale);
                }
                SlideElementKind::Text => {
                    draw_text_element(canvas, renderer, el, rect, scale, clock_unix_ms);
                }
                SlideElementKind::Image => {
                    let root = media_root.with_context(|| {
                        format!(
                            "media root unavailable for slide image element {}",
                            el.content_hash
                        )
                    })?;
                    let ext = resolve_ext(&el.media_extension, el.kind);
                    let image = load_verified_image(root, el.content_hash.trim(), &ext)?;
                    let box_w = rw.round().clamp(1.0, w as f32 * 2.0) as u32;
                    let box_h = rh.round().clamp(1.0, h as f32 * 2.0) as u32;
                    let fit = parse_fit_mode(&el.fit);
                    if el.blur_radius > 0.05 && el.blur_radius.is_finite() {
                        let rendered =
                            render_image_frame(&image, fit, el.blur_radius * scale, box_w, box_h)?;
                        let sub_info = ImageInfo::new(
                            (box_w as i32, box_h as i32),
                            ColorType::RGBA8888,
                            AlphaType::Premul,
                            None,
                        );
                        if let Some(sk_img) = images::raster_from_data(
                            &sub_info,
                            Data::new_copy(&rendered.pixels),
                            box_w as usize * 4,
                        ) {
                            canvas.save();
                            canvas.clip_rect(rect, ClipOp::Intersect, true);
                            canvas.draw_image(sk_img, (rx, ry), None);
                            canvas.restore();
                        }
                    } else {
                        let (fx, fy, fw, fh) = compute_fit_rect(
                            fit,
                            image.width().max(1) as u32,
                            image.height().max(1) as u32,
                            box_w,
                            box_h,
                        );
                        let dst = Rect::from_xywh(rx + fx, ry + fy, fw, fh);
                        let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear);
                        canvas.save();
                        canvas.clip_rect(rect, ClipOp::Intersect, true);
                        canvas.draw_image_rect_with_sampling_options(
                            &image,
                            None,
                            dst,
                            sampling,
                            &Paint::default(),
                        );
                        canvas.restore();
                    }
                }
                SlideElementKind::Video => {
                    if let Some(nv12) = video_frames.get_video(el.content_hash.trim()) {
                        let box_w = rw.round().clamp(1.0, w as f32 * 2.0) as u32;
                        let box_h = rh.round().clamp(1.0, h as f32 * 2.0) as u32;
                        let mut sub = Frame::new(box_w, box_h);
                        let fit = parse_fit_mode(&el.fit);
                        draw_cpu_layer(&mut sub, nv12, fit, 1.0)?;
                        if el.blur_radius > 0.05 && el.blur_radius.is_finite() {
                            sub = crate::media_bg::blur_frame_skia(&sub, el.blur_radius * scale)?;
                        }
                        let sub_info = ImageInfo::new(
                            (box_w as i32, box_h as i32),
                            ColorType::RGBA8888,
                            AlphaType::Premul,
                            None,
                        );
                        if let Some(sk_img) = images::raster_from_data(
                            &sub_info,
                            Data::new_copy(&sub.pixels),
                            box_w as usize * 4,
                        ) {
                            canvas.save();
                            canvas.clip_rect(rect, ClipOp::Intersect, true);
                            canvas.draw_image(sk_img, (rx, ry), None);
                            canvas.restore();
                        }
                    }
                }
            }

            if has_layer_alpha {
                canvas.restore();
            }
        }
    }

    let mut frame = Frame::new(w, h);
    if !surface.read_pixels(&info, &mut frame.pixels, w as usize * 4, (0, 0)) {
        bail!("couldn't read slide elements surface pixels");
    }
    Ok(frame)
}

fn draw_shape_element(canvas: &skia_safe::Canvas, el: &SlideElement, rect: Rect, scale: f32) {
    let radius = (el.corner_radius * scale)
        .min(rect.width() * 0.5)
        .min(rect.height() * 0.5);
    let rrect = RRect::new_rect_xy(rect, radius, radius);

    if el.shape_kind == ShapeKind::Line {
        let color = el.stroke_color.unwrap_or(el.fill_color);
        if color[3] > 0 {
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_style(PaintStyle::Stroke);
            paint.set_stroke_cap(Cap::Round);
            paint.set_stroke_width((el.stroke_width.max(2.0) * scale).clamp(1.0, 128.0));
            paint.set_color(to_skia_color(color));
            let mid_y = rect.top + rect.height() * 0.5;
            canvas.draw_line((rect.left, mid_y), (rect.right, mid_y), &paint);
        }
        return;
    }

    if el.fill_color[3] > 0 {
        let mut fill = Paint::default();
        fill.set_anti_alias(true);
        fill.set_style(PaintStyle::Fill);
        fill.set_color(to_skia_color(el.fill_color));
        match el.shape_kind {
            ShapeKind::Rect => {
                canvas.draw_rect(rect, &fill);
            }
            ShapeKind::RoundedRect => {
                canvas.draw_rrect(rrect, &fill);
            }
            ShapeKind::Ellipse => {
                canvas.draw_oval(rect, &fill);
            }
            ShapeKind::Line => {}
        }
    }

    if let Some(stroke_col) = el.stroke_color
        && stroke_col[3] > 0
        && el.stroke_width > 0.0
    {
        let mut stroke = Paint::default();
        stroke.set_anti_alias(true);
        stroke.set_style(PaintStyle::Stroke);
        stroke.set_stroke_width((el.stroke_width * scale).clamp(0.5, 128.0));
        stroke.set_color(to_skia_color(stroke_col));
        match el.shape_kind {
            ShapeKind::Rect => {
                canvas.draw_rect(rect, &stroke);
            }
            ShapeKind::RoundedRect => {
                canvas.draw_rrect(rrect, &stroke);
            }
            ShapeKind::Ellipse => {
                canvas.draw_oval(rect, &stroke);
            }
            ShapeKind::Line => {}
        }
    }
}

fn draw_text_element(
    canvas: &skia_safe::Canvas,
    renderer: &mut TextRenderer,
    el: &SlideElement,
    rect: Rect,
    scale: f32,
    clock_unix_ms: i64,
) {
    let formatted = format_dynamic_tokens(
        &el.text,
        clock_unix_ms,
        el.clock_offset_minutes,
        el.clock_show_seconds,
        el.clock_24h,
        el.timer_target_unix_ms,
        el.timer_duration_secs,
        el.timer_count_up,
    );
    if formatted.trim().is_empty() {
        return;
    }

    let pad = (el.padding * scale)
        .min((rect.width() - 2.0).max(0.0) * 0.4)
        .min((rect.height() - 2.0).max(0.0) * 0.4);
    let inner = SafeRect {
        left: rect.left + pad,
        top: rect.top + pad,
        right: (rect.right - pad).max(rect.left + pad + 1.0),
        bottom: (rect.bottom - pad).max(rect.top + pad + 1.0),
        width: (rect.width() - pad * 2.0).max(1.0),
        height: (rect.height() - pad * 2.0).max(1.0),
    };

    let slot_style = SlotTextStyle {
        font_family: el.font_family.clone(),
        size: (el.font_size * scale).clamp(8.0, 400.0),
        min_size: (el.font_size * scale).clamp(8.0, 400.0),
        weight: el.weight.clamp(100, 900),
        color: el.color,
        outline: el.outline.as_ref().map(|o| o.scaled(scale)),
        shadow: el.shadow.as_ref().map(|s| s.scaled(scale)),
        background_box: el.background_box.as_ref().map(|b| b.scaled(scale)),
        line_height: el.line_height.clamp(0.8, 4.0),
        letter_spacing: el.letter_spacing * scale,
        align_h: el.align_h,
        align_v: el.align_v,
        max_lines: None,
        visible: true,
    };

    canvas.save();
    canvas.clip_rect(rect, ClipOp::Intersect, true);
    renderer.paint_styled_text(&formatted, &slot_style, inner, canvas);
    canvas.restore();
}
