//! Slide template styling, role slots, and aspect-ratio variants (M3.2).
//!
//! Supports both the full role-slotted `TemplateStyle` JSON schema and M2's flat shorthand
//! JSON (e.g. `{"font":"Manrope","align":"center"}`), with deterministic clamping and SEC-03
//! size/count bounds.
use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::text::canonical_font_family;

/// Maximum serialized `style_json` accepted (matches SEC-03 / library exchange bounds).
pub const MAX_STYLE_JSON_BYTES: usize = 65_536;
/// Maximum aspect-ratio variants allowed in a single template.
pub const MAX_VARIANTS: usize = 32;
/// Default template reference height (pixels at 1080p).
pub const DEFAULT_REFERENCE_HEIGHT: u32 = 1080;

fn default_true() -> bool {
    true
}

fn default_schema_version() -> u32 {
    1
}

fn default_reference_height() -> u32 {
    DEFAULT_REFERENCE_HEIGHT
}

fn default_weight() -> u16 {
    400
}

fn default_line_height() -> f32 {
    1.25
}

fn default_font_family() -> String {
    "Noto Sans".to_owned()
}

fn default_min_size() -> f32 {
    24.0
}

fn default_size() -> f32 {
    72.0
}

fn default_white() -> [u8; 4] {
    [255, 255, 255, 255]
}

fn serialize_rgba<S>(rgba: &[u8; 4], serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let hex = if rgba[3] == 255 {
        format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2])
    } else {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            rgba[0], rgba[1], rgba[2], rgba[3]
        )
    };
    serializer.serialize_str(&hex)
}

fn deserialize_rgba<'de, D>(deserializer: D) -> std::result::Result<[u8; 4], D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    parse_rgba_value(&value).map_err(serde::de::Error::custom)
}

fn deserialize_rgba_opt<'de, D>(deserializer: D) -> std::result::Result<Option<[u8; 4]>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    value
        .map(|v| parse_rgba_value(&v).map_err(serde::de::Error::custom))
        .transpose()
}

/// Parse an RGBA color from `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, `[r,g,b(,a)]`,
/// or `{red, green, blue, alpha}` JSON values.
pub fn parse_rgba_value(value: &serde_json::Value) -> Result<[u8; 4]> {
    match value {
        serde_json::Value::String(s) => parse_hex_color(s),
        serde_json::Value::Array(items) => {
            if items.len() != 3 && items.len() != 4 {
                bail!("color array must have 3 or 4 components");
            }
            let comp = |idx: usize, default: u8| -> Result<u8> {
                let Some(v) = items.get(idx) else {
                    return Ok(default);
                };
                if let Some(u) = v.as_u64() {
                    Ok(u.min(255) as u8)
                } else if let Some(f) = v.as_f64() {
                    if (0.0..=1.0).contains(&f) && items.iter().any(|x| x.is_f64()) {
                        Ok((f * 255.0).round().clamp(0.0, 255.0) as u8)
                    } else {
                        Ok(f.round().clamp(0.0, 255.0) as u8)
                    }
                } else {
                    bail!("invalid numeric color component");
                }
            };
            Ok([comp(0, 0)?, comp(1, 0)?, comp(2, 0)?, comp(3, 255)?])
        }
        serde_json::Value::Object(map) => {
            let chan = |k: &str, default: f64| -> f64 {
                map.get(k).and_then(|v| v.as_f64()).unwrap_or(default)
            };
            let to_u8 = |v: f64| -> u8 {
                if (0.0..=1.0).contains(&v) {
                    (v * 255.0).round().clamp(0.0, 255.0) as u8
                } else {
                    v.round().clamp(0.0, 255.0) as u8
                }
            };
            Ok([
                to_u8(chan("red", 1.0)),
                to_u8(chan("green", 1.0)),
                to_u8(chan("blue", 1.0)),
                to_u8(chan("alpha", 1.0)),
            ])
        }
        _ => bail!("expected hex string, array, or color object"),
    }
}

pub fn parse_hex_color(s: &str) -> Result<[u8; 4]> {
    let trimmed = s.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let byte = |pair: &str| u8::from_str_radix(pair, 16).context("invalid hex color");
    match hex.len() {
        3 => {
            let expand = |i: usize| -> Result<u8> {
                let nibble = u8::from_str_radix(&hex[i..i + 1], 16).context("invalid hex color")?;
                Ok(nibble * 17)
            };
            Ok([expand(0)?, expand(1)?, expand(2)?, 255])
        }
        4 => {
            let expand = |i: usize| -> Result<u8> {
                let nibble = u8::from_str_radix(&hex[i..i + 1], 16).context("invalid hex color")?;
                Ok(nibble * 17)
            };
            Ok([expand(0)?, expand(1)?, expand(2)?, expand(3)?])
        }
        6 => Ok([byte(&hex[0..2])?, byte(&hex[2..4])?, byte(&hex[4..6])?, 255]),
        8 => Ok([
            byte(&hex[0..2])?,
            byte(&hex[2..4])?,
            byte(&hex[4..6])?,
            byte(&hex[6..8])?,
        ]),
        _ => bail!("hex color must have 3, 4, 6, or 8 hex digits, got '{s}'"),
    }
}

/// Horizontal text alignment inside a role slot or box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignH {
    Left,
    #[default]
    Center,
    Right,
    Justify,
}

impl<'de> Deserialize<'de> for TextAlignH {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(match raw.trim().to_ascii_lowercase().as_str() {
            "left" | "start" => Self::Left,
            "right" | "end" => Self::Right,
            "justify" | "justified" => Self::Justify,
            _ => Self::Center,
        })
    }
}

/// Vertical alignment inside a role slot's available box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignV {
    Top,
    #[default]
    Middle,
    Bottom,
}

impl<'de> Deserialize<'de> for TextAlignV {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(match raw.trim().to_ascii_lowercase().as_str() {
            "top" | "start" => Self::Top,
            "bottom" | "end" => Self::Bottom,
            _ => Self::Middle,
        })
    }
}

/// Safe margins around all edges of the output frame, as fractions `0.0..=0.45`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SafeArea {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Default for SafeArea {
    fn default() -> Self {
        Self {
            left: 0.08,
            top: 0.08,
            right: 0.08,
            bottom: 0.08,
        }
    }
}

/// Pixel-space rectangle corresponding to a clamped [`SafeArea`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SafeRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub width: f32,
    pub height: f32,
}

impl SafeArea {
    pub fn stage() -> Self {
        Self {
            left: 0.05,
            top: 0.05,
            right: 0.05,
            bottom: 0.05,
        }
    }

    pub fn led_wide() -> Self {
        Self {
            left: 0.04,
            top: 0.08,
            right: 0.04,
            bottom: 0.08,
        }
    }

    pub fn lower_third() -> Self {
        Self {
            left: 0.05,
            top: 0.66,
            right: 0.05,
            bottom: 0.05,
        }
    }

    pub fn clamped(self) -> Self {
        let sanitize = |v: f32| -> f32 {
            if v.is_finite() {
                v.clamp(0.0, 0.85)
            } else {
                0.08
            }
        };
        let mut left = sanitize(self.left);
        let mut right = sanitize(self.right);
        let h_sum = left + right;
        if h_sum > 0.90 {
            let k = 0.90 / h_sum;
            left *= k;
            right *= k;
        }
        let mut top = sanitize(self.top);
        let mut bottom = sanitize(self.bottom);
        let v_sum = top + bottom;
        if v_sum > 0.90 {
            let k = 0.90 / v_sum;
            top *= k;
            bottom *= k;
        }
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Resolve pixel bounds on an output of `width × height`.
    pub fn resolved_rect(self, width: u32, height: u32) -> SafeRect {
        let s = self.clamped();
        let w = width.max(1) as f32;
        let h = height.max(1) as f32;
        let left = (w * s.left).round();
        let top = (h * s.top).round();
        let right = (w - (w * s.right).round()).max(left + 1.0);
        let bottom = (h - (h * s.bottom).round()).max(top + 1.0);
        SafeRect {
            left,
            top,
            right,
            bottom,
            width: (right - left).max(1.0),
            height: (bottom - top).max(1.0),
        }
    }
}

/// Stroke outline drawn cleanly behind the glyph fill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutlineStyle {
    #[serde(
        serialize_with = "serialize_rgba",
        deserialize_with = "deserialize_rgba"
    )]
    pub color: [u8; 4],
    pub width: f32,
}

impl Default for OutlineStyle {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 230],
            width: 3.0,
        }
    }
}

impl OutlineStyle {
    pub fn clamped(self) -> Self {
        Self {
            color: self.color,
            width: if self.width.is_finite() {
                self.width.clamp(0.0, 64.0)
            } else {
                0.0
            },
        }
    }

    pub fn scaled(&self, scale: f32) -> Self {
        let c = self.clone().clamped();
        Self {
            color: c.color,
            width: (c.width * scale).clamp(0.0, 64.0),
        }
    }
}

/// Drop shadow drawn beneath the glyph outline and fill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShadowStyle {
    #[serde(
        serialize_with = "serialize_rgba",
        deserialize_with = "deserialize_rgba"
    )]
    pub color: [u8; 4],
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
}

impl Default for ShadowStyle {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 190],
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 8.0,
        }
    }
}

impl ShadowStyle {
    pub fn clamped(self) -> Self {
        let clamp_off = |v: f32| {
            if v.is_finite() {
                v.clamp(-128.0, 128.0)
            } else {
                0.0
            }
        };
        Self {
            color: self.color,
            offset_x: clamp_off(self.offset_x),
            offset_y: clamp_off(self.offset_y),
            blur: if self.blur.is_finite() {
                self.blur.clamp(0.0, 64.0)
            } else {
                0.0
            },
        }
    }

    pub fn scaled(&self, scale: f32) -> Self {
        let c = self.clone().clamped();
        Self {
            color: c.color,
            offset_x: c.offset_x * scale,
            offset_y: c.offset_y * scale,
            blur: (c.blur * scale).clamp(0.0, 64.0),
        }
    }
}

/// Backing box rendered behind a role slot's text block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackgroundBoxStyle {
    #[serde(
        serialize_with = "serialize_rgba",
        deserialize_with = "deserialize_rgba"
    )]
    pub color: [u8; 4],
    pub padding_x: f32,
    pub padding_y: f32,
    pub radius: f32,
    pub full_width: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rgba_opt"
    )]
    pub accent_bar_color: Option<[u8; 4]>,
    pub accent_bar_width: f32,
}

impl Default for BackgroundBoxStyle {
    fn default() -> Self {
        Self {
            color: [22, 46, 81, 224],
            padding_x: 24.0,
            padding_y: 14.0,
            radius: 10.0,
            full_width: false,
            accent_bar_color: None,
            accent_bar_width: 0.0,
        }
    }
}

impl BackgroundBoxStyle {
    pub fn clamped(self) -> Self {
        let clamp_nonneg = |v: f32, max: f32| {
            if v.is_finite() {
                v.clamp(0.0, max)
            } else {
                0.0
            }
        };
        Self {
            color: self.color,
            padding_x: clamp_nonneg(self.padding_x, 240.0),
            padding_y: clamp_nonneg(self.padding_y, 240.0),
            radius: clamp_nonneg(self.radius, 128.0),
            full_width: self.full_width,
            accent_bar_color: self.accent_bar_color,
            accent_bar_width: clamp_nonneg(self.accent_bar_width, 48.0),
        }
    }

    pub fn scaled(&self, scale: f32) -> Self {
        let c = self.clone().clamped();
        Self {
            color: c.color,
            padding_x: (c.padding_x * scale).round(),
            padding_y: (c.padding_y * scale).round(),
            radius: c.radius * scale,
            full_width: c.full_width,
            accent_bar_color: c.accent_bar_color,
            accent_bar_width: (c.accent_bar_width * scale).round(),
        }
    }
}

/// Complete text formatting style for one template role slot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlotTextStyle {
    #[serde(alias = "font")]
    pub font_family: String,
    pub size: f32,
    pub min_size: f32,
    #[serde(alias = "font_weight")]
    pub weight: u16,
    #[serde(
        serialize_with = "serialize_rgba",
        deserialize_with = "deserialize_rgba"
    )]
    pub color: [u8; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<OutlineStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<ShadowStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_box: Option<BackgroundBoxStyle>,
    pub line_height: f32,
    pub letter_spacing: f32,
    #[serde(alias = "align")]
    pub align_h: TextAlignH,
    pub align_v: TextAlignV,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<u32>,
    #[serde(default = "default_true")]
    pub visible: bool,
}

impl Default for SlotTextStyle {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            size: default_size(),
            min_size: default_min_size(),
            weight: default_weight(),
            color: default_white(),
            outline: None,
            shadow: None,
            background_box: None,
            line_height: default_line_height(),
            letter_spacing: 0.0,
            align_h: TextAlignH::Center,
            align_v: TextAlignV::Middle,
            max_lines: None,
            visible: true,
        }
    }
}

impl SlotTextStyle {
    pub fn validated(mut self) -> Self {
        self.font_family = canonical_font_family(&self.font_family).to_owned();
        if !self.size.is_finite() {
            self.size = default_size();
        }
        self.size = self.size.clamp(8.0, 400.0);
        if !self.min_size.is_finite() {
            self.min_size = default_min_size();
        }
        self.min_size = self.min_size.clamp(8.0, self.size);
        self.weight = self.weight.clamp(100, 900);
        if !self.line_height.is_finite() {
            self.line_height = default_line_height();
        }
        self.line_height = self.line_height.clamp(0.8, 4.0);
        if !self.letter_spacing.is_finite() {
            self.letter_spacing = 0.0;
        }
        self.letter_spacing = self.letter_spacing.clamp(-24.0, 64.0);
        self.outline = self.outline.map(OutlineStyle::clamped);
        self.shadow = self.shadow.map(ShadowStyle::clamped);
        self.background_box = self.background_box.map(BackgroundBoxStyle::clamped);
        self
    }

    /// Return a copy whose pixel dimensions (`size`, `min_size`, `outline.width`, `shadow`,
    /// `background_box`, `letter_spacing`) are scaled from `reference_height` to `output_height`.
    pub fn scaled_for(&self, reference_height: u32, output_height: u32) -> Self {
        let base = self.clone().validated();
        let ref_h = reference_height.max(1) as f32;
        let out_h = output_height.max(1) as f32;
        let scale = out_h / ref_h;
        let scaled_size = (base.size * scale).max(8.0);
        let scaled_min = (base.min_size * scale).clamp(8.0, scaled_size);
        Self {
            font_family: base.font_family,
            size: scaled_size,
            min_size: scaled_min,
            weight: base.weight,
            color: base.color,
            outline: base.outline.map(|o| o.scaled(scale)),
            shadow: base.shadow.map(|s| s.scaled(scale)),
            background_box: base.background_box.map(|b| b.scaled(scale)),
            line_height: base.line_height,
            letter_spacing: base.letter_spacing * scale,
            align_h: base.align_h,
            align_v: base.align_v,
            max_lines: base.max_lines,
            visible: base.visible,
        }
    }

    pub fn hash_into(&self, hasher: &mut blake3::Hasher) {
        let s = self.clone().validated();
        hasher.update(s.font_family.as_bytes());
        hasher.update(&[0]);
        hasher.update(&s.size.to_bits().to_le_bytes());
        hasher.update(&s.min_size.to_bits().to_le_bytes());
        hasher.update(&s.weight.to_le_bytes());
        hasher.update(&s.color);
        hasher.update(&[u8::from(s.visible)]);
        if let Some(o) = &s.outline {
            hasher.update(&[1]);
            hasher.update(&o.color);
            hasher.update(&o.width.to_bits().to_le_bytes());
        } else {
            hasher.update(&[0]);
        }
        if let Some(sh) = &s.shadow {
            hasher.update(&[1]);
            hasher.update(&sh.color);
            hasher.update(&sh.offset_x.to_bits().to_le_bytes());
            hasher.update(&sh.offset_y.to_bits().to_le_bytes());
            hasher.update(&sh.blur.to_bits().to_le_bytes());
        } else {
            hasher.update(&[0]);
        }
        if let Some(bg) = &s.background_box {
            hasher.update(&[1]);
            hasher.update(&bg.color);
            hasher.update(&bg.padding_x.to_bits().to_le_bytes());
            hasher.update(&bg.padding_y.to_bits().to_le_bytes());
            hasher.update(&bg.radius.to_bits().to_le_bytes());
            hasher.update(&[u8::from(bg.full_width)]);
            if let Some(bar) = bg.accent_bar_color {
                hasher.update(&[1]);
                hasher.update(&bar);
                hasher.update(&bg.accent_bar_width.to_bits().to_le_bytes());
            } else {
                hasher.update(&[0]);
            }
        } else {
            hasher.update(&[0]);
        }
        hasher.update(&s.line_height.to_bits().to_le_bytes());
        hasher.update(&s.letter_spacing.to_bits().to_le_bytes());
        hasher.update(&[s.align_h as u8, s.align_v as u8]);
        hasher.update(&s.max_lines.unwrap_or(0).to_le_bytes());
    }

    pub fn to_full_patch(&self) -> SlotTextStylePatch {
        SlotTextStylePatch {
            font_family: Some(self.font_family.clone()),
            size: Some(self.size),
            min_size: Some(self.min_size),
            weight: Some(self.weight),
            color: Some(self.color),
            outline: Some(self.outline.clone()),
            shadow: Some(self.shadow.clone()),
            background_box: Some(self.background_box.clone()),
            line_height: Some(self.line_height),
            letter_spacing: Some(self.letter_spacing),
            align_h: Some(self.align_h),
            align_v: Some(self.align_v),
            max_lines: Some(self.max_lines),
            visible: Some(self.visible),
        }
    }
}

/// Partial override patch for [`SlotTextStyle`] used inside aspect-ratio variants.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlotTextStylePatch {
    #[serde(alias = "font")]
    pub font_family: Option<String>,
    pub size: Option<f32>,
    pub min_size: Option<f32>,
    #[serde(alias = "font_weight")]
    pub weight: Option<u16>,
    #[serde(default, deserialize_with = "deserialize_rgba_opt")]
    pub color: Option<[u8; 4]>,
    pub outline: Option<Option<OutlineStyle>>,
    pub shadow: Option<Option<ShadowStyle>>,
    pub background_box: Option<Option<BackgroundBoxStyle>>,
    pub line_height: Option<f32>,
    pub letter_spacing: Option<f32>,
    #[serde(alias = "align")]
    pub align_h: Option<TextAlignH>,
    pub align_v: Option<TextAlignV>,
    pub max_lines: Option<Option<u32>>,
    pub visible: Option<bool>,
}

impl SlotTextStylePatch {
    pub fn apply_to(&self, base: &SlotTextStyle) -> SlotTextStyle {
        let mut out = base.clone();
        if let Some(f) = &self.font_family {
            out.font_family = f.clone();
        }
        if let Some(s) = self.size {
            out.size = s;
        }
        if let Some(ms) = self.min_size {
            out.min_size = ms;
        }
        if let Some(w) = self.weight {
            out.weight = w;
        }
        if let Some(c) = self.color {
            out.color = c;
        }
        if let Some(o) = &self.outline {
            out.outline = o.clone();
        }
        if let Some(sh) = &self.shadow {
            out.shadow = sh.clone();
        }
        if let Some(bg) = &self.background_box {
            out.background_box = bg.clone();
        }
        if let Some(lh) = self.line_height {
            out.line_height = lh;
        }
        if let Some(ls) = self.letter_spacing {
            out.letter_spacing = ls;
        }
        if let Some(ah) = self.align_h {
            out.align_h = ah;
        }
        if let Some(av) = self.align_v {
            out.align_v = av;
        }
        if let Some(ml) = self.max_lines {
            out.max_lines = ml;
        }
        if let Some(v) = self.visible {
            out.visible = v;
        }
        out.validated()
    }
}

/// Role slots carried by every template (`body`, `reference`, `title`, `footer` / `copyright`,
/// plus optional `chords` and `translation` styling).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoleSlots {
    pub body: SlotTextStyle,
    pub reference: SlotTextStyle,
    pub title: SlotTextStyle,
    #[serde(alias = "copyright")]
    pub footer: SlotTextStyle,
    pub chords: SlotTextStyle,
    pub translation: SlotTextStyle,
}

impl Default for RoleSlots {
    fn default() -> Self {
        Self {
            body: SlotTextStyle::default(),
            reference: Self::default_reference(),
            title: Self::default_title(),
            footer: Self::default_footer(),
            chords: Self::default_chords(),
            translation: Self::default_translation(),
        }
    }
}

impl RoleSlots {
    pub fn default_reference() -> SlotTextStyle {
        SlotTextStyle {
            font_family: "Noto Sans".into(),
            size: 36.0,
            min_size: 20.0,
            weight: 600,
            color: [224, 232, 240, 240],
            align_h: TextAlignH::Center,
            align_v: TextAlignV::Top,
            line_height: 1.2,
            ..SlotTextStyle::default()
        }
    }

    pub fn default_title() -> SlotTextStyle {
        SlotTextStyle {
            font_family: "Noto Sans".into(),
            size: 84.0,
            min_size: 36.0,
            weight: 700,
            color: [255, 255, 255, 255],
            align_h: TextAlignH::Center,
            align_v: TextAlignV::Top,
            line_height: 1.15,
            ..SlotTextStyle::default()
        }
    }

    pub fn default_footer() -> SlotTextStyle {
        SlotTextStyle {
            font_family: "Noto Sans".into(),
            size: 24.0,
            min_size: 16.0,
            weight: 400,
            color: [215, 224, 235, 215],
            align_h: TextAlignH::Center,
            align_v: TextAlignV::Bottom,
            line_height: 1.25,
            ..SlotTextStyle::default()
        }
    }

    pub fn default_chords() -> SlotTextStyle {
        SlotTextStyle {
            font_family: "JetBrains Mono".into(),
            size: 36.0,
            min_size: 18.0,
            weight: 700,
            color: [255, 190, 46, 255],
            align_h: TextAlignH::Left,
            align_v: TextAlignV::Top,
            line_height: 1.2,
            visible: false,
            ..SlotTextStyle::default()
        }
    }

    pub fn default_translation() -> SlotTextStyle {
        SlotTextStyle {
            font_family: "Noto Sans".into(),
            size: 44.0,
            min_size: 20.0,
            weight: 400,
            color: [220, 230, 245, 235],
            align_h: TextAlignH::Center,
            align_v: TextAlignV::Middle,
            line_height: 1.25,
            visible: true,
            ..SlotTextStyle::default()
        }
    }

    pub fn validated(self) -> Self {
        Self {
            body: self.body.validated(),
            reference: self.reference.validated(),
            title: self.title.validated(),
            footer: self.footer.validated(),
            chords: self.chords.validated(),
            translation: self.translation.validated(),
        }
    }

    pub fn scaled_for(&self, reference_height: u32, output_height: u32) -> Self {
        Self {
            body: self.body.scaled_for(reference_height, output_height),
            reference: self.reference.scaled_for(reference_height, output_height),
            title: self.title.scaled_for(reference_height, output_height),
            footer: self.footer.scaled_for(reference_height, output_height),
            chords: self.chords.scaled_for(reference_height, output_height),
            translation: self.translation.scaled_for(reference_height, output_height),
        }
    }

    pub fn to_full_patch(&self) -> RoleSlotsPatch {
        RoleSlotsPatch {
            body: Some(self.body.to_full_patch()),
            reference: Some(self.reference.to_full_patch()),
            title: Some(self.title.to_full_patch()),
            footer: Some(self.footer.to_full_patch()),
            chords: Some(self.chords.to_full_patch()),
            translation: Some(self.translation.to_full_patch()),
        }
    }
}

/// Optional role overrides for an [`AspectVariantStyle`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoleSlotsPatch {
    pub body: Option<SlotTextStylePatch>,
    pub reference: Option<SlotTextStylePatch>,
    pub title: Option<SlotTextStylePatch>,
    #[serde(alias = "copyright")]
    pub footer: Option<SlotTextStylePatch>,
    pub chords: Option<SlotTextStylePatch>,
    pub translation: Option<SlotTextStylePatch>,
}

impl RoleSlotsPatch {
    pub fn apply_to(&self, base: &RoleSlots) -> RoleSlots {
        RoleSlots {
            body: self
                .body
                .as_ref()
                .map_or_else(|| base.body.clone(), |p| p.apply_to(&base.body)),
            reference: self
                .reference
                .as_ref()
                .map_or_else(|| base.reference.clone(), |p| p.apply_to(&base.reference)),
            title: self
                .title
                .as_ref()
                .map_or_else(|| base.title.clone(), |p| p.apply_to(&base.title)),
            footer: self
                .footer
                .as_ref()
                .map_or_else(|| base.footer.clone(), |p| p.apply_to(&base.footer)),
            chords: self
                .chords
                .as_ref()
                .map_or_else(|| base.chords.clone(), |p| p.apply_to(&base.chords)),
            translation: self.translation.as_ref().map_or_else(
                || base.translation.clone(),
                |p| p.apply_to(&base.translation),
            ),
        }
        .validated()
    }
}

/// Output-aspect-specific override bundled inside a [`TemplateStyle`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AspectVariantStyle {
    /// Human label such as `"16:9"`, `"32:9"`, `"4:3"`, `"9:16"`.
    pub aspect_ratio: String,
    pub reference_width: u32,
    pub reference_height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_area: Option<SafeArea>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles: Option<RoleSlotsPatch>,
}

impl AspectVariantStyle {
    /// Target width/height aspect ratio for matching against output dimensions.
    pub fn aspect(&self) -> f32 {
        if self.reference_width > 0 && self.reference_height > 0 {
            return self.reference_width as f32 / self.reference_height as f32;
        }
        if let Some((w_str, h_str)) = self.aspect_ratio.split_once(':')
            && let (Ok(w), Ok(h)) = (w_str.trim().parse::<f32>(), h_str.trim().parse::<f32>())
            && w > 0.0
            && h > 0.0
        {
            return w / h;
        }
        16.0 / 9.0
    }
}

/// Complete serialisable template styling definition (stored in `TemplateVariant.style_json`
/// and carried onto `SlideLayerSpec.style_json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TemplateStyle {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub name: String,
    #[serde(default = "default_reference_height")]
    pub reference_height: u32,
    pub safe_area: SafeArea,
    pub roles: RoleSlots,
    pub variants: Vec<AspectVariantStyle>,
}

impl Default for TemplateStyle {
    fn default() -> Self {
        Self {
            schema_version: 1,
            name: String::new(),
            reference_height: DEFAULT_REFERENCE_HEIGHT,
            safe_area: SafeArea::default(),
            roles: RoleSlots::default(),
            variants: Vec::new(),
        }
    }
}

/// Concrete style resolved for a specific `(width, height)` output frame (in output pixels).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTemplate {
    pub matched_variant: Option<String>,
    pub width: u32,
    pub height: u32,
    pub safe_area: SafeArea,
    pub max_lines: Option<u32>,
    pub roles: RoleSlots,
}

impl ResolvedTemplate {
    pub fn cache_hash(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(&self.width.to_le_bytes());
        h.update(&self.height.to_le_bytes());
        h.update(&self.safe_area.left.to_bits().to_le_bytes());
        h.update(&self.safe_area.top.to_bits().to_le_bytes());
        h.update(&self.safe_area.right.to_bits().to_le_bytes());
        h.update(&self.safe_area.bottom.to_bits().to_le_bytes());
        h.update(&self.max_lines.unwrap_or(0).to_le_bytes());
        self.roles.body.hash_into(&mut h);
        self.roles.reference.hash_into(&mut h);
        self.roles.title.hash_into(&mut h);
        self.roles.footer.hash_into(&mut h);
        self.roles.chords.hash_into(&mut h);
        self.roles.translation.hash_into(&mut h);
        *h.finalize().as_bytes()
    }
}

impl TemplateStyle {
    pub fn validated(mut self) -> Self {
        self.schema_version = self.schema_version.max(1);
        if self.reference_height == 0 {
            self.reference_height = DEFAULT_REFERENCE_HEIGHT;
        }
        self.reference_height = self.reference_height.clamp(64, 4320);
        self.safe_area = self.safe_area.clamped();
        self.roles = self.roles.validated();
        if self.variants.len() > MAX_VARIANTS {
            self.variants.truncate(MAX_VARIANTS);
        }
        for v in &mut self.variants {
            if let Some(sa) = v.safe_area {
                v.safe_area = Some(sa.clamped());
            }
        }
        self
    }

    pub fn to_json(&self) -> Result<String> {
        let json = serde_json::to_string(self)?;
        if json.len() > MAX_STYLE_JSON_BYTES {
            bail!(
                "style_json exceeds {} bytes ({} bytes)",
                MAX_STYLE_JSON_BYTES,
                json.len()
            );
        }
        Ok(json)
    }

    /// Parse `style_json`, supporting empty strings, full M3 `TemplateStyle` JSON,
    /// and legacy M2 flat shorthands (`{"font":"Manrope","align":"center"}`, etc.).
    pub fn from_json(raw: &str) -> Result<Self> {
        if raw.len() > MAX_STYLE_JSON_BYTES {
            bail!(
                "style_json exceeds {} bytes ({} bytes)",
                MAX_STYLE_JSON_BYTES,
                raw.len()
            );
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(Self::default());
        }
        let value: serde_json::Value =
            serde_json::from_str(trimmed).with_context(|| "style_json must be valid JSON")?;
        let Some(obj) = value.as_object() else {
            bail!("style_json root must be a JSON object");
        };
        if let Some(variants) = obj.get("variants").and_then(|v| v.as_array())
            && variants.len() > MAX_VARIANTS
        {
            bail!("template exceeds maximum of {MAX_VARIANTS} variants");
        }

        let is_full = obj.contains_key("roles")
            || obj.contains_key("variants")
            || obj.contains_key("safe_area")
            || obj.contains_key("schema_version");

        let mut style = if is_full {
            serde_json::from_value::<TemplateStyle>(value.clone())
                .context("invalid TemplateStyle JSON")?
        } else {
            Self::from_shorthand_map(obj)?
        };

        // Allow top-level font/align shorthands to apply gracefully if supplied alongside M3 fields.
        if is_full {
            apply_shorthand_overrides(obj, &mut style);
        }

        Ok(style.validated())
    }

    /// Build a [`TemplateStyle`] from a stored protobuf [`selah_proto::v1::Template`] and its
    /// [`selah_proto::v1::TemplateVariant`] list (M3.2 / M3.3).
    pub fn from_proto_template(doc: &selah_proto::v1::Template) -> Result<Self> {
        if doc.variants.len() > MAX_VARIANTS {
            bail!(
                "template '{}' exceeds maximum of {MAX_VARIANTS} variants",
                doc.name
            );
        }
        if doc.variants.is_empty() {
            return Ok(Self {
                name: doc.name.clone(),
                ..Self::default()
            }
            .validated());
        }

        if doc.variants.len() == 1 {
            let v = &doc.variants[0];
            let mut parsed = Self::from_json(&v.style_json)?;
            if !doc.name.trim().is_empty() && parsed.name.trim().is_empty() {
                parsed.name = doc.name.clone();
            }
            if let Some(res) = v.resolution
                && res.width > 0
                && res.height > 0
            {
                let out_ar = res.width as f32 / res.height as f32;
                let max_lines = (v.max_lines > 0).then_some(v.max_lines);
                let matching = parsed
                    .variants
                    .iter_mut()
                    .find(|ev| (out_ar / ev.aspect() - 1.0).abs() <= 0.15);
                if let Some(ev) = matching {
                    if max_lines.is_some() {
                        ev.max_lines = max_lines;
                    }
                    if ev.reference_width == 0 {
                        ev.reference_width = res.width;
                    }
                    if ev.reference_height == 0 {
                        ev.reference_height = res.height;
                    }
                } else {
                    parsed.variants.push(AspectVariantStyle {
                        aspect_ratio: format_aspect_ratio(res.width, res.height),
                        reference_width: res.width,
                        reference_height: res.height,
                        max_lines,
                        safe_area: None,
                        roles: None,
                    });
                }
            }
            return Ok(parsed.validated());
        }

        // Multiple proto variants: choose base variant closest to 16:9, and translate every variant
        // into an AspectVariantStyle so per-output matching picks the exact intended layout.
        let target_ar = 16.0f32 / 9.0;
        let base_idx = doc
            .variants
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = a.resolution.map_or(f32::MAX, |r| {
                    if r.height > 0 {
                        (r.width as f32 / r.height as f32 - target_ar).abs()
                    } else {
                        f32::MAX
                    }
                });
                let db = b.resolution.map_or(f32::MAX, |r| {
                    if r.height > 0 {
                        (r.width as f32 / r.height as f32 - target_ar).abs()
                    } else {
                        f32::MAX
                    }
                });
                da.total_cmp(&db)
            })
            .map_or(0, |(idx, _)| idx);

        let base_proto = &doc.variants[base_idx];
        let mut base = Self::from_json(&base_proto.style_json)?;
        if !doc.name.trim().is_empty() && base.name.trim().is_empty() {
            base.name = doc.name.clone();
        }
        if let Some(res) = base_proto.resolution
            && res.height > 0
            && base_proto.style_json.trim().is_empty()
        {
            base.reference_height = res.height;
        }

        let mut variants = Vec::with_capacity(doc.variants.len());
        for v in &doc.variants {
            let trimmed = v.style_json.trim();
            let parsed_v = Self::from_json(trimmed)?;
            let res = v.resolution.unwrap_or(selah_proto::v1::Resolution {
                width: 1920,
                height: 1080,
            });
            let rw = res.width.max(1);
            let rh = res.height.max(1);
            let max_lines = (v.max_lines > 0)
                .then_some(v.max_lines)
                .or(parsed_v.roles.body.max_lines);
            let has_explicit_sa = parsed_v.safe_area != SafeArea::default();
            variants.push(AspectVariantStyle {
                aspect_ratio: format_aspect_ratio(rw, rh),
                reference_width: rw,
                reference_height: rh,
                max_lines,
                safe_area: has_explicit_sa.then_some(parsed_v.safe_area),
                roles: (!trimmed.is_empty()).then(|| parsed_v.roles.to_full_patch()),
            });
        }
        base.variants = variants;
        Ok(base.validated())
    }

    fn from_shorthand_map(obj: &serde_json::Map<String, serde_json::Value>) -> Result<Self> {
        let mut style = Self::default();
        apply_shorthand_overrides(obj, &mut style);
        if let Some(margin) = obj.get("margin").and_then(|v| v.as_f64()) {
            let m = (margin as f32).clamp(0.0, 0.45);
            style.safe_area.left = m;
            style.safe_area.right = m;
        }
        if let Some(family) = obj
            .get("font")
            .or_else(|| obj.get("font_family"))
            .and_then(|v| v.as_str())
        {
            let canonical = canonical_font_family(family).to_owned();
            style.roles.body.font_family = canonical.clone();
            style.roles.title.font_family = canonical.clone();
            style.roles.reference.font_family = canonical.clone();
            style.roles.footer.font_family = canonical;
        }
        Ok(style)
    }

    /// Resolve a variant and output-pixel role styles for `width × height`.
    pub fn resolve_for_output(
        &self,
        width: u32,
        height: u32,
        max_lines_override: Option<u32>,
    ) -> ResolvedTemplate {
        let w = width.max(1);
        let h = height.max(1);
        let out_aspect = w as f32 / h as f32;

        // Pick best matching aspect variant if one matches closely (<= 22% relative AR delta).
        let best_variant = self
            .variants
            .iter()
            .filter_map(|v| {
                let va = v.aspect();
                if va <= 0.0 {
                    return None;
                }
                let rel_diff = (out_aspect / va - 1.0).abs();
                (rel_diff <= 0.22).then_some((rel_diff, v))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, v)| v);

        let (matched_variant, ref_h, safe_area, roles, variant_max_lines) = match best_variant {
            Some(v) => {
                let ref_h = if v.reference_height > 0 {
                    v.reference_height
                } else {
                    self.reference_height.max(1)
                };
                let sa = v.safe_area.unwrap_or(self.safe_area).clamped();
                let patched = v
                    .roles
                    .as_ref()
                    .map_or_else(|| self.roles.clone(), |p| p.apply_to(&self.roles));
                (
                    Some(v.aspect_ratio.clone()),
                    ref_h,
                    sa,
                    patched,
                    v.max_lines,
                )
            }
            None => (
                None,
                self.reference_height.max(1),
                self.safe_area.clamped(),
                self.roles.clone().validated(),
                None,
            ),
        };

        let scaled_roles = roles.scaled_for(ref_h, h);
        ResolvedTemplate {
            matched_variant,
            width: w,
            height: h,
            safe_area,
            max_lines: max_lines_override
                .or(variant_max_lines)
                .or(scaled_roles.body.max_lines),
            roles: scaled_roles,
        }
    }

    /// Built-in "Sanctuary Lyrics" preset (Manrope, shadow + crisp outline, 16:9 + 32:9 LED + 4:3 + 9:16 variants).
    pub fn preset_sanctuary() -> Self {
        Self {
            schema_version: 1,
            name: "Sanctuary Lyrics".into(),
            reference_height: 1080,
            safe_area: SafeArea::default(),
            roles: RoleSlots {
                body: SlotTextStyle {
                    font_family: "Manrope".into(),
                    size: 74.0,
                    min_size: 32.0,
                    weight: 700,
                    color: [255, 255, 255, 255],
                    outline: Some(OutlineStyle {
                        color: [10, 14, 24, 210],
                        width: 2.5,
                    }),
                    shadow: Some(ShadowStyle {
                        color: [0, 0, 0, 185],
                        offset_x: 0.0,
                        offset_y: 4.0,
                        blur: 10.0,
                    }),
                    background_box: None,
                    line_height: 1.25,
                    letter_spacing: 0.5,
                    align_h: TextAlignH::Center,
                    align_v: TextAlignV::Middle,
                    max_lines: Some(4),
                    visible: true,
                },
                reference: SlotTextStyle {
                    font_family: "Manrope".into(),
                    size: 36.0,
                    min_size: 22.0,
                    weight: 700,
                    color: [216, 232, 255, 245],
                    shadow: Some(ShadowStyle {
                        color: [0, 0, 0, 180],
                        offset_x: 0.0,
                        offset_y: 2.0,
                        blur: 6.0,
                    }),
                    align_h: TextAlignH::Center,
                    align_v: TextAlignV::Top,
                    ..RoleSlots::default_reference()
                },
                title: SlotTextStyle {
                    font_family: "Manrope".into(),
                    size: 88.0,
                    min_size: 40.0,
                    weight: 800,
                    letter_spacing: -0.6,
                    shadow: Some(ShadowStyle::default()),
                    ..RoleSlots::default_title()
                },
                footer: SlotTextStyle {
                    font_family: "Inter".into(),
                    size: 24.0,
                    min_size: 16.0,
                    weight: 500,
                    color: [230, 236, 245, 220],
                    shadow: Some(ShadowStyle {
                        color: [0, 0, 0, 175],
                        offset_x: 0.0,
                        offset_y: 2.0,
                        blur: 4.0,
                    }),
                    ..RoleSlots::default_footer()
                },
                chords: RoleSlots::default_chords(),
                translation: SlotTextStyle {
                    font_family: "Inter".into(),
                    size: 46.0,
                    min_size: 22.0,
                    weight: 500,
                    color: [215, 228, 248, 235],
                    shadow: Some(ShadowStyle {
                        color: [0, 0, 0, 180],
                        offset_x: 0.0,
                        offset_y: 2.0,
                        blur: 6.0,
                    }),
                    ..RoleSlots::default_translation()
                },
            },
            variants: vec![
                AspectVariantStyle {
                    aspect_ratio: "16:9".into(),
                    reference_width: 1920,
                    reference_height: 1080,
                    max_lines: Some(4),
                    safe_area: None,
                    roles: None,
                },
                AspectVariantStyle {
                    aspect_ratio: "32:9".into(),
                    reference_width: 3840,
                    reference_height: 1080,
                    max_lines: Some(2),
                    safe_area: Some(SafeArea::led_wide()),
                    roles: Some(RoleSlotsPatch {
                        body: Some(SlotTextStylePatch {
                            size: Some(68.0),
                            min_size: Some(32.0),
                            max_lines: Some(Some(2)),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
                AspectVariantStyle {
                    aspect_ratio: "4:3".into(),
                    reference_width: 1440,
                    reference_height: 1080,
                    max_lines: Some(4),
                    safe_area: Some(SafeArea {
                        left: 0.07,
                        top: 0.08,
                        right: 0.07,
                        bottom: 0.08,
                    }),
                    roles: Some(RoleSlotsPatch {
                        body: Some(SlotTextStylePatch {
                            size: Some(64.0),
                            min_size: Some(28.0),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
                AspectVariantStyle {
                    aspect_ratio: "9:16".into(),
                    reference_width: 1080,
                    reference_height: 1920,
                    max_lines: Some(5),
                    safe_area: Some(SafeArea {
                        left: 0.10,
                        top: 0.14,
                        right: 0.10,
                        bottom: 0.14,
                    }),
                    roles: Some(RoleSlotsPatch {
                        body: Some(SlotTextStylePatch {
                            size: Some(82.0),
                            min_size: Some(36.0),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
            ],
        }
        .validated()
    }

    /// Built-in "Warm Serif" preset using Noto Serif.
    pub fn preset_warm_serif() -> Self {
        let mut s = Self::preset_sanctuary();
        s.name = "Warm Serif".into();
        s.roles.body.font_family = "Noto Serif".into();
        s.roles.body.weight = 600;
        s.roles.body.color = [255, 248, 232, 255];
        s.roles.title.font_family = "Noto Serif".into();
        s.roles.title.color = [255, 244, 214, 255];
        s.roles.reference.font_family = "Noto Serif".into();
        s.validated()
    }

    /// Built-in "Lower Third Stream" preset with dark card, cyan USWDS accent stripe, and bottom-left alignment.
    pub fn preset_lower_third() -> Self {
        Self {
            schema_version: 1,
            name: "Stream Lower Third".into(),
            reference_height: 1080,
            safe_area: SafeArea::lower_third(),
            roles: RoleSlots {
                body: SlotTextStyle {
                    font_family: "Inter".into(),
                    size: 46.0,
                    min_size: 26.0,
                    weight: 600,
                    color: [255, 255, 255, 255],
                    outline: None,
                    shadow: None,
                    background_box: Some(BackgroundBoxStyle {
                        color: [22, 46, 81, 230],
                        padding_x: 24.0,
                        padding_y: 14.0,
                        radius: 8.0,
                        full_width: false,
                        accent_bar_color: Some([0, 189, 227, 255]),
                        accent_bar_width: 8.0,
                    }),
                    line_height: 1.25,
                    letter_spacing: 0.0,
                    align_h: TextAlignH::Left,
                    align_v: TextAlignV::Bottom,
                    max_lines: Some(2),
                    visible: true,
                },
                reference: SlotTextStyle {
                    font_family: "Inter".into(),
                    size: 28.0,
                    min_size: 18.0,
                    weight: 700,
                    color: [0, 189, 227, 255],
                    align_h: TextAlignH::Left,
                    align_v: TextAlignV::Top,
                    ..RoleSlots::default_reference()
                },
                title: SlotTextStyle {
                    font_family: "Manrope".into(),
                    size: 52.0,
                    min_size: 28.0,
                    weight: 700,
                    align_h: TextAlignH::Left,
                    align_v: TextAlignV::Top,
                    ..RoleSlots::default_title()
                },
                footer: SlotTextStyle {
                    font_family: "Inter".into(),
                    size: 20.0,
                    min_size: 14.0,
                    weight: 400,
                    color: [200, 214, 228, 220],
                    align_h: TextAlignH::Left,
                    align_v: TextAlignV::Bottom,
                    ..RoleSlots::default_footer()
                },
                chords: RoleSlots::default_chords(),
                translation: SlotTextStyle {
                    font_family: "Inter".into(),
                    size: 32.0,
                    min_size: 18.0,
                    weight: 400,
                    align_h: TextAlignH::Left,
                    align_v: TextAlignV::Bottom,
                    ..RoleSlots::default_translation()
                },
            },
            variants: Vec::new(),
        }
        .validated()
    }

    /// Built-in "Stage Monitor" preset: left-aligned lyrics with JetBrains Mono chords visible above each line.
    pub fn preset_stage_monitor() -> Self {
        Self {
            schema_version: 1,
            name: "Stage Monitor".into(),
            reference_height: 1080,
            safe_area: SafeArea::stage(),
            roles: RoleSlots {
                body: SlotTextStyle {
                    font_family: "Inter".into(),
                    size: 62.0,
                    min_size: 28.0,
                    weight: 600,
                    color: [245, 245, 247, 255],
                    line_height: 1.3,
                    align_h: TextAlignH::Left,
                    align_v: TextAlignV::Middle,
                    max_lines: Some(4),
                    ..SlotTextStyle::default()
                },
                chords: SlotTextStyle {
                    visible: true,
                    font_family: "JetBrains Mono".into(),
                    size: 38.0,
                    min_size: 20.0,
                    weight: 700,
                    color: [255, 190, 46, 255],
                    align_h: TextAlignH::Left,
                    align_v: TextAlignV::Top,
                    ..RoleSlots::default_chords()
                },
                title: SlotTextStyle {
                    font_family: "Manrope".into(),
                    size: 44.0,
                    min_size: 24.0,
                    weight: 700,
                    color: [0, 189, 227, 255],
                    align_h: TextAlignH::Left,
                    align_v: TextAlignV::Top,
                    ..RoleSlots::default_title()
                },
                reference: SlotTextStyle {
                    align_h: TextAlignH::Left,
                    align_v: TextAlignV::Top,
                    ..RoleSlots::default_reference()
                },
                footer: SlotTextStyle {
                    align_h: TextAlignH::Left,
                    align_v: TextAlignV::Bottom,
                    ..RoleSlots::default_footer()
                },
                translation: SlotTextStyle {
                    align_h: TextAlignH::Left,
                    ..RoleSlots::default_translation()
                },
            },
            variants: Vec::new(),
        }
        .validated()
    }

    pub fn builtin_presets() -> Vec<Self> {
        vec![
            Self::preset_sanctuary(),
            Self::preset_warm_serif(),
            Self::preset_lower_third(),
            Self::preset_stage_monitor(),
        ]
    }
}

fn apply_shorthand_overrides(
    obj: &serde_json::Map<String, serde_json::Value>,
    style: &mut TemplateStyle,
) {
    if let Some(font) = obj
        .get("font")
        .or_else(|| obj.get("font_family"))
        .and_then(|v| v.as_str())
    {
        style.roles.body.font_family = canonical_font_family(font).to_owned();
    }
    if let Some(align) = obj
        .get("align")
        .or_else(|| obj.get("align_h"))
        .and_then(|v| serde_json::from_value::<TextAlignH>(v.clone()).ok())
    {
        style.roles.body.align_h = align;
    }
    if let Some(align_v) = obj
        .get("align_v")
        .and_then(|v| serde_json::from_value::<TextAlignV>(v.clone()).ok())
    {
        style.roles.body.align_v = align_v;
    }
    if let Some(size) = obj.get("size").and_then(|v| v.as_f64()) {
        style.roles.body.size = size as f32;
    }
    if let Some(min_size) = obj.get("min_size").and_then(|v| v.as_f64()) {
        style.roles.body.min_size = min_size as f32;
    }
    if let Some(weight) = obj
        .get("weight")
        .or_else(|| obj.get("font_weight"))
        .and_then(|v| v.as_u64())
    {
        style.roles.body.weight = weight.clamp(100, 900) as u16;
    }
    if let Some(c) = obj.get("color").and_then(|v| parse_rgba_value(v).ok()) {
        style.roles.body.color = c;
    }
    if let Some(lh) = obj.get("line_height").and_then(|v| v.as_f64()) {
        style.roles.body.line_height = lh as f32;
    }
    if let Some(ls) = obj.get("letter_spacing").and_then(|v| v.as_f64()) {
        style.roles.body.letter_spacing = ls as f32;
    }
    if let Some(o) = obj
        .get("outline")
        .and_then(|v| serde_json::from_value::<OutlineStyle>(v.clone()).ok())
    {
        style.roles.body.outline = Some(o);
    }
    if let Some(sh) = obj
        .get("shadow")
        .and_then(|v| serde_json::from_value::<ShadowStyle>(v.clone()).ok())
    {
        style.roles.body.shadow = Some(sh);
    }
    if let Some(bg) = obj
        .get("background_box")
        .and_then(|v| serde_json::from_value::<BackgroundBoxStyle>(v.clone()).ok())
    {
        style.roles.body.background_box = Some(bg);
    }
    if let Some(ml) = obj.get("max_lines").and_then(|v| v.as_u64()) {
        style.roles.body.max_lines = Some(ml as u32);
    }
}

/// Decoded chord anchor inside a single lyric line (`chords_json`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlideChordAnchor {
    pub line: usize,
    pub position: usize,
    pub text: String,
}

/// Parse `SlideLayerSpec.chords_json` (supports array of `{line, position, text}` or
/// nested `Vec<Vec<Chord>>` style arrays) with SEC-03 size/count clamping.
pub fn parse_chords_json(raw: &str) -> HashMap<usize, Vec<(usize, String)>> {
    let mut by_line: HashMap<usize, Vec<(usize, String)>> = HashMap::new();
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_STYLE_JSON_BYTES {
        return by_line;
    }
    if let Ok(anchors) = serde_json::from_str::<Vec<SlideChordAnchor>>(trimmed) {
        for a in anchors.into_iter().take(256) {
            if !a.text.trim().is_empty() {
                by_line
                    .entry(a.line)
                    .or_default()
                    .push((a.position, a.text));
            }
        }
    } else if let Ok(lines) = serde_json::from_str::<Vec<Vec<ChordPosOnly>>>(trimmed) {
        for (line_idx, chords) in lines.into_iter().take(64).enumerate() {
            for c in chords.into_iter().take(32) {
                if !c.text.trim().is_empty() {
                    by_line
                        .entry(line_idx)
                        .or_default()
                        .push((c.position, c.text));
                }
            }
        }
    }
    for entry in by_line.values_mut() {
        entry.sort_by_key(|(pos, _)| *pos);
    }
    by_line
}

#[derive(Deserialize)]
struct ChordPosOnly {
    position: usize,
    text: String,
}

fn format_aspect_ratio(width: u32, height: u32) -> String {
    let mut a = width.max(1);
    let mut b = height.max(1);
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    let g = a.max(1);
    format!("{}:{}", width.max(1) / g, height.max(1) / g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_m2_shorthand_style_json() {
        let style = TemplateStyle::from_json(r#"{"font":"Manrope","align":"center"}"#).unwrap();
        assert_eq!(style.roles.body.font_family, "Manrope");
        assert_eq!(style.roles.body.align_h, TextAlignH::Center);
        assert_eq!(style.roles.title.font_family, "Manrope");
    }

    #[test]
    fn parses_full_template_with_copyright_alias_and_hex_colors() {
        let json = r##"{
            "name": "Custom",
            "safe_area": { "left": 0.1, "top": 0.05, "right": 0.1, "bottom": 0.06 },
            "roles": {
                "body": {
                    "font": "Inter",
                    "size": 68,
                    "min_size": 28,
                    "weight": 800,
                    "color": "#FFF0AA",
                    "outline": { "color": "#000000", "width": 3 },
                    "shadow": { "color": "#000000B0", "offset_x": 2, "offset_y": 5, "blur": 6 },
                    "background_box": { "color": "#162E51CC", "padding_x": 20, "padding_y": 12, "radius": 8 },
                    "line_height": 1.3,
                    "letter_spacing": 1.5,
                    "align_h": "left",
                    "align_v": "bottom"
                },
                "copyright": { "size": 22, "color": "#CCCCCC" }
            },
            "variants": [
                {
                    "aspect_ratio": "32:9",
                    "reference_width": 3840,
                    "reference_height": 1080,
                    "max_lines": 2,
                    "roles": { "body": { "size": 58, "align_h": "center" } }
                }
            ]
        }"##;
        let parsed = TemplateStyle::from_json(json).unwrap();
        assert_eq!(parsed.roles.body.font_family, "Inter");
        assert_eq!(parsed.roles.body.weight, 800);
        assert_eq!(parsed.roles.body.color, [255, 240, 170, 255]);
        assert_eq!(parsed.roles.body.outline.as_ref().unwrap().width, 3.0);
        assert_eq!(parsed.roles.footer.size, 22.0);
        assert_eq!(parsed.roles.footer.color, [204, 204, 204, 255]);

        let led = parsed.resolve_for_output(3840, 1080, None);
        assert_eq!(led.matched_variant.as_deref(), Some("32:9"));
        assert_eq!(led.roles.body.size, 58.0);
        assert_eq!(led.roles.body.align_h, TextAlignH::Center);
        assert_eq!(led.max_lines, Some(2));
        // Inherits properties not overridden in variant patch:
        assert_eq!(led.roles.body.font_family, "Inter");
        assert_eq!(led.roles.body.weight, 800);
    }

    #[test]
    fn enforces_sec03_style_json_bounds() {
        let oversized = format!(r#"{{"name":"{}"}}"#, "a".repeat(MAX_STYLE_JSON_BYTES + 8));
        assert!(TemplateStyle::from_json(&oversized).is_err());

        let safe = SafeArea {
            left: 0.7,
            top: 0.7,
            right: 0.7,
            bottom: 0.7,
        };
        let rect = safe.resolved_rect(1920, 1080);
        assert!(rect.width >= 1.0 && rect.height >= 1.0);
        assert!(rect.right <= 1920.0 && rect.bottom <= 1080.0);
    }

    #[test]
    fn converts_multi_variant_and_single_full_proto_templates() {
        use selah_proto::v1::{Resolution, Template, TemplateVariant};

        let multi = Template {
            name: "Multi Output".into(),
            variants: vec![
                TemplateVariant {
                    resolution: Some(Resolution {
                        width: 1920,
                        height: 1080,
                    }),
                    max_lines: 4,
                    style_json: r#"{"font":"Manrope","size":72}"#.into(),
                    ..Default::default()
                },
                TemplateVariant {
                    resolution: Some(Resolution {
                        width: 3840,
                        height: 1080,
                    }),
                    max_lines: 2,
                    style_json: String::new(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let converted = TemplateStyle::from_proto_template(&multi).unwrap();
        assert_eq!(converted.name, "Multi Output");
        assert_eq!(converted.roles.body.font_family, "Manrope");
        let r_led = converted.resolve_for_output(3840, 1080, None);
        assert_eq!(r_led.matched_variant.as_deref(), Some("32:9"));
        assert_eq!(r_led.max_lines, Some(2));
        // Empty variant inherits base font cleanly.
        assert_eq!(r_led.roles.body.font_family, "Manrope");

        let single_full = Template {
            name: "Full Sanctuary".into(),
            variants: vec![TemplateVariant {
                resolution: Some(Resolution {
                    width: 1920,
                    height: 1080,
                }),
                max_lines: 4,
                style_json: TemplateStyle::preset_sanctuary().to_json().unwrap(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let from_full = TemplateStyle::from_proto_template(&single_full).unwrap();
        assert_eq!(from_full.variants.len(), 4);
        assert_eq!(
            from_full
                .resolve_for_output(1440, 1080, None)
                .matched_variant
                .as_deref(),
            Some("4:3")
        );
    }
}
