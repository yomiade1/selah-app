//! Closed built-in GPU/CPU layer and output effect chain (M3.5 / SEC-03 / SEC-04).
//!
//! Custom WGSL / GLSL / arbitrary shader code is explicitly rejected until M8: every stage
//! must resolve strictly through [`EffectKind`] and built-in validated parameter structs.

use anyhow::{Result, anyhow, bail};
use selah_proto::v1::{EffectKind, EffectSpec, LayerSpec, OutputScene, Scene, layer_spec};
use serde_json::Value;

use crate::Frame;
use crate::blur::{BlurPipeline, blur_cpu};
use crate::gpu::TARGET_FORMAT;

const SHADER: &str = include_str!("effects.wgsl");

/// Hard cap on effects per layer or output chain (`SEC-03`).
pub const MAX_EFFECTS_PER_CHAIN: usize = 8;
/// Hard cap on `params_json` payload size per effect (`SEC-03`).
pub const MAX_EFFECT_PARAMS_BYTES: usize = 16 * 1024;

/// JSON object keys forbidden across the closed M3 effect pipeline (`SEC-04`).
const FORBIDDEN_SHADER_KEYS: &[&str] = &[
    "shader",
    "wgsl",
    "glsl",
    "hlsl",
    "spirv",
    "code",
    "source",
    "shader_source",
    "custom_shader",
    "entry_point",
    "entrypoint",
    "script",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderInputKind {
    LayerSelf,
    LayerBackdrop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderStageSpec {
    pub kind: EffectKind,
    pub input: ShaderInputKind,
    pub wgsl_entry: &'static str,
}

/// Trait implemented by built-in shader stages (M8 closed-set extension seam).
pub trait BuiltInEffectShader {
    fn effect_kind(&self) -> EffectKind;
    fn stage_spec(&self) -> ShaderStageSpec;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskShape {
    Rect = 0,
    RoundedRect = 1,
    Circle = 2,
    Ellipse = 3,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedEffect {
    Blur {
        sigma: f32,
    },
    ColourAdjust {
        mix: f32,
        brightness: f32,
        contrast: f32,
        saturation: f32,
        hue_radians: f32,
    },
    Vignette {
        mix: f32,
        inner_radius: f32,
        softness: f32,
        strength: f32,
        color: [f32; 3],
    },
    BackgroundBlur {
        mix: f32,
        sigma: f32,
        region: Option<[f32; 4]>,
        tint_rgba: [f32; 4],
    },
    ColourKey {
        mix: f32,
        key_rgb: [f32; 3],
        tolerance: f32,
        softness: f32,
        spill_suppression: f32,
    },
    Mask {
        mix: f32,
        shape: MaskShape,
        bounds: [f32; 4],
        corner_radius: f32,
        feather: f32,
        invert: bool,
    },
}

impl BuiltInEffectShader for ResolvedEffect {
    fn effect_kind(&self) -> EffectKind {
        match self {
            Self::Blur { .. } => EffectKind::Blur,
            Self::ColourAdjust { .. } => EffectKind::ColourAdjust,
            Self::Vignette { .. } => EffectKind::Vignette,
            Self::BackgroundBlur { .. } => EffectKind::BackgroundBlur,
            Self::ColourKey { .. } => EffectKind::ColourKey,
            Self::Mask { .. } => EffectKind::Mask,
        }
    }

    fn stage_spec(&self) -> ShaderStageSpec {
        let (input, wgsl_entry) = match self {
            Self::Blur { .. } => (ShaderInputKind::LayerSelf, "blur_separable"),
            Self::BackgroundBlur { .. } => (ShaderInputKind::LayerBackdrop, "fs_main:bg_blur"),
            Self::ColourAdjust { .. } => (ShaderInputKind::LayerSelf, "fs_main:colour_adjust"),
            Self::Vignette { .. } => (ShaderInputKind::LayerSelf, "fs_main:vignette"),
            Self::ColourKey { .. } => (ShaderInputKind::LayerSelf, "fs_main:colour_key"),
            Self::Mask { .. } => (ShaderInputKind::LayerSelf, "fs_main:mask"),
        };
        ShaderStageSpec {
            kind: self.effect_kind(),
            input,
            wgsl_entry,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValidatedShaderChain {
    pub stages: Vec<ResolvedEffect>,
}

impl ValidatedShaderChain {
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// Strict validation for core IPC (`SEC-03` / `SEC-04`).
    pub fn from_specs_strict(specs: &[EffectSpec]) -> Result<Self> {
        if specs.len() > MAX_EFFECTS_PER_CHAIN {
            bail!(
                "effect chain has {} stages (maximum is {MAX_EFFECTS_PER_CHAIN})",
                specs.len()
            );
        }
        let mut stages = Vec::with_capacity(specs.len());
        for spec in specs {
            if let Some(resolved) = validate_one_effect(spec)? {
                stages.push(resolved);
            }
        }
        Ok(Self { stages })
    }

    /// Lossy normalization for defensive compositor rendering (logs and skips invalid stages).
    pub fn from_specs_lossy(specs: &[EffectSpec]) -> Self {
        let mut stages = Vec::with_capacity(specs.len().min(MAX_EFFECTS_PER_CHAIN));
        for spec in specs.iter().take(MAX_EFFECTS_PER_CHAIN) {
            match validate_one_effect(spec) {
                Ok(Some(resolved)) => stages.push(resolved),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%error, kind = spec.kind, "ignoring invalid effect spec");
                }
            }
        }
        Self { stages }
    }
}

fn check_forbidden_shader_keys(value: &Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let low = k.trim().to_ascii_lowercase();
                if FORBIDDEN_SHADER_KEYS.contains(&low.as_str()) {
                    bail!("custom shaders are not permitted (forbidden key '{k}')");
                }
                check_forbidden_shader_keys(v)?;
            }
        }
        Value::Array(arr) => {
            for v in arr {
                check_forbidden_shader_keys(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn parse_params_object(spec: &EffectSpec) -> Result<serde_json::Map<String, Value>> {
    if spec.params_json.len() > MAX_EFFECT_PARAMS_BYTES {
        bail!(
            "effect params_json exceeds {} byte limit",
            MAX_EFFECT_PARAMS_BYTES
        );
    }
    let trimmed = spec.params_json.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::Map::new());
    }
    let val: Value =
        serde_json::from_str(trimmed).map_err(|e| anyhow!("invalid effect params_json: {e}"))?;
    check_forbidden_shader_keys(&val)?;
    match val {
        Value::Object(map) => Ok(map),
        _ => bail!("effect params_json must be a JSON object"),
    }
}

fn json_f32(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Result<Option<f32>> {
    for &k in keys {
        if let Some(v) = map.get(k) {
            let num = v
                .as_f64()
                .ok_or_else(|| anyhow!("effect param '{k}' must be a number"))?;
            if !num.is_finite() {
                bail!("effect param '{k}' must be finite");
            }
            return Ok(Some(num as f32));
        }
    }
    Ok(None)
}

fn json_bool(map: &serde_json::Map<String, Value>, keys: &[&str], default: bool) -> Result<bool> {
    for &k in keys {
        if let Some(v) = map.get(k) {
            return v
                .as_bool()
                .ok_or_else(|| anyhow!("effect param '{k}' must be a boolean"));
        }
    }
    Ok(default)
}

fn parse_hex_color(s: &str) -> Result<[f32; 4]> {
    let hex = s.trim().strip_prefix('#').unwrap_or(s.trim());
    let byte_at = |i: usize| -> Result<f32> {
        let b = u8::from_str_radix(&hex[i..i + 2], 16)
            .map_err(|_| anyhow!("invalid hex color '{s}'"))?;
        Ok(f32::from(b) / 255.0)
    };
    match hex.len() {
        6 => Ok([byte_at(0)?, byte_at(2)?, byte_at(4)?, 1.0]),
        8 => Ok([byte_at(0)?, byte_at(2)?, byte_at(4)?, byte_at(6)?]),
        _ => bail!("hex color '{s}' must be #RRGGBB or #RRGGBBAA"),
    }
}

fn json_color(
    map: &serde_json::Map<String, Value>,
    keys: &[&str],
    default: [f32; 4],
) -> Result<[f32; 4]> {
    for &k in keys {
        if let Some(v) = map.get(k) {
            match v {
                Value::String(s) => return parse_hex_color(s),
                Value::Array(arr) if arr.len() == 3 || arr.len() == 4 => {
                    let mut out = default;
                    for (idx, item) in arr.iter().enumerate() {
                        let n = item
                            .as_f64()
                            .ok_or_else(|| anyhow!("color channel in '{k}' must be numeric"))?;
                        if !n.is_finite() {
                            bail!("color channel in '{k}' must be finite");
                        }
                        out[idx] = (n as f32).clamp(0.0, 1.0);
                    }
                    if arr.len() == 3 {
                        out[3] = 1.0;
                    }
                    return Ok(out);
                }
                _ => bail!("effect param '{k}' must be #RRGGBB(AA) or [r,g,b(,a)]"),
            }
        }
    }
    Ok(default)
}

fn json_rect(map: &serde_json::Map<String, Value>, key: &str) -> Result<Option<[f32; 4]>> {
    let Some(v) = map.get(key) else {
        return Ok(None);
    };
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow!("effect param '{key}' must be [x,y,w,h]"))?;
    if arr.len() != 4 {
        bail!("effect param '{key}' must have 4 elements");
    }
    let mut out = [0.0_f32; 4];
    for (i, item) in arr.iter().enumerate() {
        let n = item
            .as_f64()
            .ok_or_else(|| anyhow!("effect param '{key}' elements must be numbers"))?;
        if !n.is_finite() {
            bail!("effect param '{key}' elements must be finite");
        }
        out[i] = n as f32;
    }
    out[0] = out[0].clamp(-1.0, 2.0);
    out[1] = out[1].clamp(-1.0, 2.0);
    out[2] = out[2].clamp(0.001, 2.0);
    out[3] = out[3].clamp(0.001, 2.0);
    Ok(Some(out))
}

fn resolve_mix(spec: &EffectSpec) -> Result<f32> {
    if !spec.intensity.is_finite() {
        bail!("effect intensity must be finite");
    }
    if spec.intensity > 0.0 {
        Ok(spec.intensity.clamp(0.0, 1.0))
    } else if spec.params_json.trim().is_empty() {
        Ok(0.5)
    } else {
        Ok(1.0)
    }
}

/// Validate and normalize one [`EffectSpec`]. Returns `Ok(None)` when disabled or zero-effect.
pub fn validate_one_effect(spec: &EffectSpec) -> Result<Option<ResolvedEffect>> {
    // Parse params even when disabled so forbidden shader payloads cannot slip past validation.
    let map = parse_params_object(spec)?;
    let kind = EffectKind::try_from(spec.kind)
        .map_err(|_| anyhow!("unsupported effect kind {}", spec.kind))?;
    if kind == EffectKind::Unspecified {
        bail!("effect kind must not be UNSPECIFIED");
    }
    if !spec.intensity.is_finite() {
        bail!("effect intensity must be finite");
    }
    if !spec.enabled {
        return Ok(None);
    }
    let mix = resolve_mix(spec)?;
    let explicit_scale = if spec.intensity > 0.0 {
        spec.intensity.clamp(0.0, 1.0)
    } else {
        1.0
    };

    let resolved = match kind {
        EffectKind::Unspecified => unreachable!(),
        EffectKind::Blur => {
            let sigma = match json_f32(&map, &["radius", "sigma", "blur_radius"])? {
                Some(r) => r.clamp(0.0, 64.0) * explicit_scale,
                None => 24.0 * mix,
            };
            if sigma <= 0.05 {
                return Ok(None);
            }
            ResolvedEffect::Blur { sigma }
        }
        EffectKind::ColourAdjust => {
            let brightness = json_f32(&map, &["brightness"])?
                .unwrap_or(0.0)
                .clamp(-1.0, 1.0);
            let contrast = json_f32(&map, &["contrast"])?
                .unwrap_or(1.0)
                .clamp(0.0, 3.0);
            let saturation = json_f32(&map, &["saturation"])?
                .unwrap_or(1.0)
                .clamp(0.0, 3.0);
            let hue_degrees = json_f32(&map, &["hue_degrees", "hue"])?
                .unwrap_or(0.0)
                .clamp(-360.0, 360.0);
            ResolvedEffect::ColourAdjust {
                mix,
                brightness,
                contrast,
                saturation,
                hue_radians: hue_degrees.to_radians(),
            }
        }
        EffectKind::Vignette => {
            let inner_radius = json_f32(&map, &["radius", "inner_radius"])?
                .unwrap_or(0.45)
                .clamp(0.0, 1.5);
            let softness = json_f32(&map, &["softness", "feather"])?
                .unwrap_or(0.75)
                .clamp(0.02, 2.0);
            let strength = json_f32(&map, &["strength"])?
                .unwrap_or(0.65)
                .clamp(0.0, 1.0);
            let c = json_color(&map, &["color", "vignette_color"], [0.0, 0.0, 0.0, 1.0])?;
            ResolvedEffect::Vignette {
                mix,
                inner_radius,
                softness,
                strength,
                color: [c[0], c[1], c[2]],
            }
        }
        EffectKind::BackgroundBlur => {
            let sigma = match json_f32(&map, &["radius", "sigma", "blur_radius"])? {
                Some(r) => r.clamp(0.0, 64.0) * explicit_scale,
                None => 18.0 * mix,
            };
            let region = json_rect(&map, "region")?;
            let tint_rgba = json_color(
                &map,
                &["tint_color", "tint", "color"],
                [0.08, 0.10, 0.14, 0.25],
            )?;
            ResolvedEffect::BackgroundBlur {
                mix,
                sigma,
                region,
                tint_rgba,
            }
        }
        EffectKind::ColourKey => {
            let c = json_color(&map, &["key_color", "color"], [0.0, 1.0, 0.0, 1.0])?;
            let tolerance = json_f32(&map, &["tolerance", "similarity"])?
                .unwrap_or(0.30)
                .clamp(0.0, 1.0);
            let softness = json_f32(&map, &["softness", "feather"])?
                .unwrap_or(0.08)
                .clamp(0.0, 1.0);
            let spill_suppression = json_f32(&map, &["spill_suppression", "spill"])?
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            ResolvedEffect::ColourKey {
                mix,
                key_rgb: [c[0], c[1], c[2]],
                tolerance,
                softness,
                spill_suppression,
            }
        }
        EffectKind::Mask => {
            let shape_str = map
                .get("shape")
                .and_then(|v| v.as_str())
                .unwrap_or("rect")
                .to_ascii_lowercase();
            let shape = match shape_str.as_str() {
                "rect" | "rectangle" => MaskShape::Rect,
                "rounded_rect" | "rounded" => MaskShape::RoundedRect,
                "circle" => MaskShape::Circle,
                "ellipse" | "oval" => MaskShape::Ellipse,
                other => bail!("unknown mask shape '{other}'"),
            };
            let bounds = json_rect(&map, "bounds")?.unwrap_or([0.1, 0.1, 0.8, 0.8]);
            let default_corner = if shape == MaskShape::RoundedRect {
                0.06
            } else {
                0.0
            };
            let corner_radius = json_f32(&map, &["corner_radius", "radius"])?
                .unwrap_or(default_corner)
                .clamp(0.0, 0.5);
            let feather = json_f32(&map, &["feather", "softness"])?
                .unwrap_or(0.02)
                .clamp(0.0, 0.5);
            let invert = json_bool(&map, &["invert"], false)?;
            ResolvedEffect::Mask {
                mix,
                shape,
                bounds,
                corner_radius,
                feather,
                invert,
            }
        }
    };
    Ok(Some(resolved))
}

/// Collect raw `EffectSpec`s for a `LayerSpec` (content-level effects followed by layer-level effects).
pub fn collect_layer_effect_specs(spec: &LayerSpec) -> Vec<EffectSpec> {
    let mut all = Vec::new();
    match spec.content.as_ref() {
        Some(layer_spec::Content::VideoInput(v)) => all.extend(v.effects.iter().cloned()),
        Some(layer_spec::Content::Media(m)) => all.extend(m.effects.iter().cloned()),
        Some(layer_spec::Content::Slide(s)) => all.extend(s.effects.iter().cloned()),
        Some(layer_spec::Content::Props(p)) => all.extend(p.effects.iter().cloned()),
        Some(layer_spec::Content::Messages(m)) => all.extend(m.effects.iter().cloned()),
        None => {}
    }
    all.extend(spec.effects.iter().cloned());
    all
}

/// Validate every effect chain across all outputs and layers of `scene` (`SEC-03`/`SEC-04`).
pub fn validate_scene_shader_chains(scene: &Scene) -> Result<()> {
    for out in &scene.outputs {
        validate_output_shader_chains(out)?;
    }
    Ok(())
}

pub fn validate_output_shader_chains(out: &OutputScene) -> Result<()> {
    ValidatedShaderChain::from_specs_strict(&out.effects)?;
    for layer in &out.layers {
        let specs = collect_layer_effect_specs(layer);
        ValidatedShaderChain::from_specs_strict(&specs)?;
    }
    Ok(())
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn mix4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

fn read_px_f32(frame: &Frame, x: u32, y: u32) -> [f32; 4] {
    let idx = (y as usize * frame.width as usize + x as usize) * 4;
    [
        f32::from(frame.pixels[idx]) / 255.0,
        f32::from(frame.pixels[idx + 1]) / 255.0,
        f32::from(frame.pixels[idx + 2]) / 255.0,
        f32::from(frame.pixels[idx + 3]) / 255.0,
    ]
}

fn write_px_f32(frame: &mut Frame, x: u32, y: u32, px: [f32; 4]) {
    let idx = (y as usize * frame.width as usize + x as usize) * 4;
    for (dst, &val) in frame.pixels[idx..idx + 4].iter_mut().zip(&px) {
        *dst = (val.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
}

/// Apply one non-backdrop effect stage to `src` on CPU, returning the transformed frame.
pub fn apply_stage_cpu(src: &Frame, stage: &ResolvedEffect, is_output: bool) -> Frame {
    let (w, h) = (src.width.max(1), src.height.max(1));
    match stage {
        ResolvedEffect::Blur { sigma } => blur_cpu(src, *sigma, 1.0),
        ResolvedEffect::BackgroundBlur { sigma, .. } => {
            if is_output {
                blur_cpu(src, *sigma, 1.0)
            } else {
                src.clone()
            }
        }
        ResolvedEffect::ColourAdjust {
            mix,
            brightness,
            contrast,
            saturation,
            hue_radians,
        } => {
            let mut out = Frame::new(w, h);
            let k = 0.577_350_27_f32;
            let cos_h = hue_radians.cos();
            let sin_h = hue_radians.sin();
            let do_hue = hue_radians.abs() > 1e-5;
            for y in 0..h {
                for x in 0..w {
                    let c = read_px_f32(src, x, y);
                    let a = c[3];
                    let rgb0 = if a > 1e-5 {
                        [
                            (c[0] / a).clamp(0.0, 1.0),
                            (c[1] / a).clamp(0.0, 1.0),
                            (c[2] / a).clamp(0.0, 1.0),
                        ]
                    } else {
                        [0.0, 0.0, 0.0]
                    };
                    let mut rgb = [
                        ((rgb0[0] + brightness - 0.5) * contrast + 0.5).clamp(0.0, 1.0),
                        ((rgb0[1] + brightness - 0.5) * contrast + 0.5).clamp(0.0, 1.0),
                        ((rgb0[2] + brightness - 0.5) * contrast + 0.5).clamp(0.0, 1.0),
                    ];
                    let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
                    rgb = mix3([luma, luma, luma], rgb, *saturation);
                    if do_hue {
                        let dot_k = k * (rgb[0] + rgb[1] + rgb[2]);
                        let cross = [
                            k * (rgb[2] - rgb[1]),
                            k * (rgb[0] - rgb[2]),
                            k * (rgb[1] - rgb[0]),
                        ];
                        rgb = [
                            rgb[0] * cos_h + cross[0] * sin_h + k * dot_k * (1.0 - cos_h),
                            rgb[1] * cos_h + cross[1] * sin_h + k * dot_k * (1.0 - cos_h),
                            rgb[2] * cos_h + cross[2] * sin_h + k * dot_k * (1.0 - cos_h),
                        ];
                    }
                    let final_rgb = mix3(
                        rgb0,
                        [
                            rgb[0].clamp(0.0, 1.0),
                            rgb[1].clamp(0.0, 1.0),
                            rgb[2].clamp(0.0, 1.0),
                        ],
                        *mix,
                    );
                    write_px_f32(
                        &mut out,
                        x,
                        y,
                        [final_rgb[0] * a, final_rgb[1] * a, final_rgb[2] * a, a],
                    );
                }
            }
            out
        }
        ResolvedEffect::Vignette {
            mix,
            inner_radius,
            softness,
            strength,
            color,
        } => {
            let mut out = Frame::new(w, h);
            let s = (*strength * *mix).clamp(0.0, 1.0);
            let soft = (*softness).max(1e-4);
            for y in 0..h {
                let v = (y as f32 + 0.5) / h as f32;
                for x in 0..w {
                    let u = (x as f32 + 0.5) / w as f32;
                    let c = read_px_f32(src, x, y);
                    let dx = (u - 0.5) * 2.0;
                    let dy = (v - 0.5) * 2.0;
                    let r = (dx * dx + dy * dy).sqrt();
                    let edge = smoothstep(*inner_radius, *inner_radius + soft, r);
                    let keep = 1.0 - edge * s;
                    let vig = [color[0] * c[3], color[1] * c[3], color[2] * c[3]];
                    let res = mix3(vig, [c[0], c[1], c[2]], keep);
                    write_px_f32(&mut out, x, y, [res[0], res[1], res[2], c[3]]);
                }
            }
            out
        }
        ResolvedEffect::ColourKey {
            mix,
            key_rgb,
            tolerance,
            softness,
            spill_suppression,
        } => {
            let mut out = Frame::new(w, h);
            let soft = (*softness).max(1e-4);
            let spill = spill_suppression.clamp(0.0, 1.0);
            for y in 0..h {
                for x in 0..w {
                    let c = read_px_f32(src, x, y);
                    let a = c[3];
                    let rgb = if a > 1e-5 {
                        [
                            (c[0] / a).clamp(0.0, 1.0),
                            (c[1] / a).clamp(0.0, 1.0),
                            (c[2] / a).clamp(0.0, 1.0),
                        ]
                    } else {
                        [0.0, 0.0, 0.0]
                    };
                    let d0 = rgb[0] - key_rgb[0];
                    let d1 = rgb[1] - key_rgb[1];
                    let d2 = rgb[2] - key_rgb[2];
                    let dist = (d0 * d0 + d1 * d1 + d2 * d2).sqrt() / 1.732_050_8;
                    let keep = smoothstep(*tolerance, *tolerance + soft, dist);
                    let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
                    let sup = mix3(rgb, [luma, luma, luma], (1.0 - keep) * spill);
                    let factor = 1.0 + (keep - 1.0) * *mix;
                    if is_output {
                        write_px_f32(
                            &mut out,
                            x,
                            y,
                            [
                                sup[0] * a * factor,
                                sup[1] * a * factor,
                                sup[2] * a * factor,
                                a,
                            ],
                        );
                    } else {
                        let na = a * factor;
                        write_px_f32(&mut out, x, y, [sup[0] * na, sup[1] * na, sup[2] * na, na]);
                    }
                }
            }
            out
        }
        ResolvedEffect::Mask {
            mix,
            shape,
            bounds,
            corner_radius,
            feather,
            invert,
        } => {
            let mut out = Frame::new(w, h);
            let cx = bounds[0] + bounds[2] * 0.5;
            let cy = bounds[1] + bounds[3] * 0.5;
            let hex = (bounds[2] * 0.5).max(1e-4);
            let hey = (bounds[3] * 0.5).max(1e-4);
            let corner = corner_radius.clamp(0.0, hex.min(hey));
            let f = (*feather).max(1e-4);
            let aspect = (w as f32 / h as f32).max(1e-4);
            for y in 0..h {
                let v = (y as f32 + 0.5) / h as f32;
                for x in 0..w {
                    let u = (x as f32 + 0.5) / w as f32;
                    let c = read_px_f32(src, x, y);
                    let dist = match shape {
                        MaskShape::Circle => {
                            let dx = (u - cx) * aspect;
                            let dy = v - cy;
                            let rad = (hex * aspect).min(hey);
                            (dx * dx + dy * dy).sqrt() - rad
                        }
                        MaskShape::Ellipse => {
                            let px = (u - cx) / hex;
                            let py = (v - cy) / hey;
                            ((px * px + py * py).sqrt() - 1.0) * hex.min(hey)
                        }
                        MaskShape::Rect | MaskShape::RoundedRect => {
                            let qx = (u - cx).abs() - (hex - corner);
                            let qy = (v - cy).abs() - (hey - corner);
                            let ox = qx.max(0.0);
                            let oy = qy.max(0.0);
                            (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - corner
                        }
                    };
                    let mut inside = 1.0 - smoothstep(-f * 0.5, f * 0.5, dist);
                    if *invert {
                        inside = 1.0 - inside;
                    }
                    let factor = 1.0 + (inside - 1.0) * *mix;
                    if is_output {
                        write_px_f32(
                            &mut out,
                            x,
                            y,
                            [c[0] * factor, c[1] * factor, c[2] * factor, c[3]],
                        );
                    } else {
                        write_px_f32(
                            &mut out,
                            x,
                            y,
                            [c[0] * factor, c[1] * factor, c[2] * factor, c[3] * factor],
                        );
                    }
                }
            }
            out
        }
    }
}

/// Apply `BackgroundBlur` on CPU: blurs `canvas` backdrop behind `layer_overlay` (or `region`)
/// with tint and writes back onto `canvas`.
pub fn apply_background_blur_cpu(
    canvas: &mut Frame,
    layer_overlay: &Frame,
    stage: &ResolvedEffect,
) {
    let ResolvedEffect::BackgroundBlur {
        mix,
        sigma,
        region,
        tint_rgba,
    } = stage
    else {
        return;
    };
    let blurred = if *sigma > 0.05 {
        blur_cpu(canvas, *sigma, 1.0)
    } else {
        canvas.clone()
    };
    let (w, h) = (canvas.width.max(1), canvas.height.max(1));
    let tint_a = tint_rgba[3].clamp(0.0, 1.0);
    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let mask = if let Some([rx, ry, rw, rh]) = region {
                if u >= *rx && u <= *rx + *rw && v >= *ry && v <= *ry + *rh {
                    *mix
                } else {
                    0.0
                }
            } else {
                let lp = read_px_f32(layer_overlay, x, y);
                if lp[3] > 0.002 { *mix } else { 0.0 }
            };
            if mask <= 0.0 {
                continue;
            }
            let orig = read_px_f32(canvas, x, y);
            let b = read_px_f32(&blurred, x, y);
            let tinted = [
                tint_rgba[0] * tint_a + b[0] * (1.0 - tint_a),
                tint_rgba[1] * tint_a + b[1] * (1.0 - tint_a),
                tint_rgba[2] * tint_a + b[2] * (1.0 - tint_a),
                b[3].max(tint_a),
            ];
            write_px_f32(canvas, x, y, mix4(orig, tinted, mask));
        }
    }
}

/// GPU effect stage executor matching [`apply_stage_cpu`] and [`apply_background_blur_cpu`].
pub struct EffectsPipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params_pool: Vec<wgpu::Buffer>,
    pool_cursor: usize,
    _dummy_tex: wgpu::Texture,
    dummy_view: wgpu::TextureView,
}

impl EffectsPipeline {
    pub fn begin_frame(&mut self) {
        self.pool_cursor = 0;
    }
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(1),
                tex_entry(2),
                tex_entry(3),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("effects"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: TARGET_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effects-nearest"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let dummy_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effects-dummy"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let dummy_view = dummy_tex.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            pipeline,
            layout,
            sampler,
            params_pool: Vec::new(),
            pool_cursor: 0,
            _dummy_tex: dummy_tex,
            dummy_view,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn alloc_uniform(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        kind: u32,
        is_output: bool,
        flags: u32,
        mix_amount: f32,
        p0: [f32; 4],
        p1: [f32; 4],
    ) -> usize {
        let slot = self.pool_cursor;
        self.pool_cursor += 1;
        if slot >= self.params_pool.len() {
            self.params_pool
                .push(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("effects-params"),
                    size: 64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
        }
        let mut bytes = [0_u8; 64];
        bytes[0..4].copy_from_slice(&kind.to_le_bytes());
        bytes[4..8].copy_from_slice(&u32::from(is_output).to_le_bytes());
        bytes[8..12].copy_from_slice(&flags.to_le_bytes());
        bytes[12..16].copy_from_slice(&mix_amount.to_le_bytes());
        for (i, v) in p0.iter().enumerate() {
            let off = 16 + i * 4;
            bytes[off..off + 4].copy_from_slice(&v.to_le_bytes());
        }
        for (i, v) in p1.iter().enumerate() {
            let off = 32 + i * 4;
            bytes[off..off + 4].copy_from_slice(&v.to_le_bytes());
        }
        queue.write_buffer(&self.params_pool[slot], 0, &bytes);
        slot
    }

    #[allow(clippy::too_many_arguments)]
    fn run_pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        param_slot: usize,
        primary: &wgpu::TextureView,
        secondary: &wgpu::TextureView,
        tertiary: &wgpu::TextureView,
        dst: &wgpu::TextureView,
    ) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effects-bind"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.params_pool[param_slot].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(primary),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(secondary),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(tertiary),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("effects-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_stage(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        blur: &mut BlurPipeline,
        src_view: &wgpu::TextureView,
        dst_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        stage: &ResolvedEffect,
        is_output: bool,
    ) {
        match stage {
            ResolvedEffect::Blur { sigma } => {
                blur.blur_view_to_view(
                    device, queue, encoder, src_view, dst_view, width, height, *sigma, 1.0, false,
                );
            }
            ResolvedEffect::BackgroundBlur { sigma, .. } => {
                if is_output && *sigma > 0.05 {
                    blur.blur_view_to_view(
                        device, queue, encoder, src_view, dst_view, width, height, *sigma, 1.0,
                        false,
                    );
                }
            }
            ResolvedEffect::ColourAdjust {
                mix,
                brightness,
                contrast,
                saturation,
                hue_radians,
            } => {
                let slot = self.alloc_uniform(
                    device,
                    queue,
                    EffectKind::ColourAdjust as u32,
                    is_output,
                    0,
                    *mix,
                    [*brightness, *contrast, *saturation, *hue_radians],
                    [0.0; 4],
                );
                self.run_pass(
                    device,
                    encoder,
                    slot,
                    src_view,
                    &self.dummy_view,
                    &self.dummy_view,
                    dst_view,
                );
            }
            ResolvedEffect::Vignette {
                mix,
                inner_radius,
                softness,
                strength,
                color,
            } => {
                let slot = self.alloc_uniform(
                    device,
                    queue,
                    EffectKind::Vignette as u32,
                    is_output,
                    0,
                    *mix,
                    [*inner_radius, *softness, *strength, 0.0],
                    [color[0], color[1], color[2], 1.0],
                );
                self.run_pass(
                    device,
                    encoder,
                    slot,
                    src_view,
                    &self.dummy_view,
                    &self.dummy_view,
                    dst_view,
                );
            }
            ResolvedEffect::ColourKey {
                mix,
                key_rgb,
                tolerance,
                softness,
                spill_suppression,
            } => {
                let slot = self.alloc_uniform(
                    device,
                    queue,
                    EffectKind::ColourKey as u32,
                    is_output,
                    0,
                    *mix,
                    [key_rgb[0], key_rgb[1], key_rgb[2], *tolerance],
                    [*softness, *spill_suppression, 0.0, 0.0],
                );
                self.run_pass(
                    device,
                    encoder,
                    slot,
                    src_view,
                    &self.dummy_view,
                    &self.dummy_view,
                    dst_view,
                );
            }
            ResolvedEffect::Mask {
                mix,
                shape,
                bounds,
                corner_radius,
                feather,
                invert,
            } => {
                let flags = u32::from(*invert) | ((*shape as u32) << 1);
                let aspect = width.max(1) as f32 / height.max(1) as f32;
                let slot = self.alloc_uniform(
                    device,
                    queue,
                    EffectKind::Mask as u32,
                    is_output,
                    flags,
                    *mix,
                    *bounds,
                    [*corner_radius, *feather, aspect, 0.0],
                );
                self.run_pass(
                    device,
                    encoder,
                    slot,
                    src_view,
                    &self.dummy_view,
                    &self.dummy_view,
                    dst_view,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_background_blur(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        blur: &mut BlurPipeline,
        backdrop_view: &wgpu::TextureView,
        blurred_view: &wgpu::TextureView,
        layer_view: &wgpu::TextureView,
        dst_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        stage: &ResolvedEffect,
    ) {
        let ResolvedEffect::BackgroundBlur {
            mix,
            sigma,
            region,
            tint_rgba,
        } = stage
        else {
            return;
        };
        let blur_input = if *sigma > 0.05 {
            blur.blur_view_to_view(
                device,
                queue,
                encoder,
                backdrop_view,
                blurred_view,
                width,
                height,
                *sigma,
                1.0,
                false,
            );
            blurred_view
        } else {
            backdrop_view
        };
        let (flags, p0) = match region {
            Some(r) => (1_u32, *r),
            None => (0_u32, [0.0; 4]),
        };
        let slot = self.alloc_uniform(
            device,
            queue,
            EffectKind::BackgroundBlur as u32,
            false,
            flags,
            *mix,
            p0,
            *tint_rgba,
        );
        self.run_pass(
            device,
            encoder,
            slot,
            backdrop_view,
            blur_input,
            layer_view,
            dst_view,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::CpuCompositor;
    use crate::gpu::{AdapterChoice, GpuCompositor};
    use crate::{Compositor, Frame};
    use selah_proto::v1::{
        Color, LayerKind, MediaLayerSpec, OutputScene, PropsLayerSpec, Scene, layer_spec,
    };

    #[test]
    fn rejects_custom_shader_keys_and_enforces_limits() {
        for bad_key in [
            r#"{"shader":"@fragment fn fs_main() {}"}"#,
            r#"{"wgsl":"return vec4(1.0);"}"#,
            r#"{"nested":{"code":"evil"}}"#,
            r#"{"source":"main()"}"#,
            r#"{"entry_point":"custom"}"#,
        ] {
            let err = ValidatedShaderChain::from_specs_strict(&[EffectSpec {
                kind: EffectKind::ColourAdjust as i32,
                enabled: true,
                intensity: 1.0,
                params_json: bad_key.into(),
            }])
            .unwrap_err();
            assert!(
                err.to_string().contains("custom shaders"),
                "unexpected error: {err}"
            );
        }

        let too_many = vec![
            EffectSpec {
                kind: EffectKind::Vignette as i32,
                enabled: true,
                intensity: 0.5,
                params_json: String::new(),
            };
            MAX_EFFECTS_PER_CHAIN + 1
        ];
        assert!(ValidatedShaderChain::from_specs_strict(&too_many).is_err());

        let oversized = EffectSpec {
            kind: EffectKind::ColourAdjust as i32,
            enabled: true,
            intensity: 1.0,
            params_json: format!("{{\"brightness\":0.1,\"pad\":\"{}\"}}", "a".repeat(20_000)),
        };
        assert!(ValidatedShaderChain::from_specs_strict(&[oversized]).is_err());
    }

    #[test]
    fn all_six_effects_cpu_and_gpu_match() {
        let mut cpu = CpuCompositor::default();
        let mut gpu = match GpuCompositor::new(AdapterChoice::from_env()) {
            Ok(g) => g,
            Err(_) => return,
        };
        let (w, h) = (64_u32, 36_u32);

        let base_scene = |effects: Vec<EffectSpec>, is_output: bool| Scene {
            revision: 1,
            outputs: vec![OutputScene {
                output_index: 0,
                layers: vec![
                    LayerSpec {
                        kind: LayerKind::Media as i32,
                        enabled: true,
                        effects: if is_output { vec![] } else { effects.clone() },
                        content: Some(layer_spec::Content::Media(MediaLayerSpec {
                            solid_color: Some(Color {
                                red: 0.1,
                                green: 0.9,
                                blue: 0.2,
                                alpha: 1.0,
                            }),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                    LayerSpec {
                        kind: LayerKind::Props as i32,
                        enabled: true,
                        effects: vec![],
                        content: Some(layer_spec::Content::Props(PropsLayerSpec {
                            background_color: Some(Color {
                                red: 0.85,
                                green: 0.25,
                                blue: 0.15,
                                alpha: 0.5,
                            }),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                ],
                effects: if is_output { effects } else { vec![] },
                ..Default::default()
            }],
            ..Default::default()
        };

        let cases = [
            (
                "blur",
                vec![EffectSpec {
                    kind: EffectKind::Blur as i32,
                    enabled: true,
                    intensity: 1.0,
                    params_json: r#"{"radius":4.0}"#.into(),
                }],
                true,
            ),
            (
                "colour_adjust",
                vec![EffectSpec {
                    kind: EffectKind::ColourAdjust as i32,
                    enabled: true,
                    intensity: 1.0,
                    params_json: r#"{"brightness":0.1,"contrast":1.4,"saturation":0.3,"hue":45}"#
                        .into(),
                }],
                true,
            ),
            (
                "vignette",
                vec![EffectSpec {
                    kind: EffectKind::Vignette as i32,
                    enabled: true,
                    intensity: 1.0,
                    params_json: r#"{"radius":0.3,"softness":0.6,"strength":0.85}"#.into(),
                }],
                true,
            ),
            (
                "colour_key",
                vec![EffectSpec {
                    kind: EffectKind::ColourKey as i32,
                    enabled: true,
                    intensity: 1.0,
                    params_json: r#"{"color":"\#19e533","tolerance":0.2,"softness":0.05}"#.replace("\\#", "#"),
                }],
                false,
            ),
            (
                "mask",
                vec![EffectSpec {
                    kind: EffectKind::Mask as i32,
                    enabled: true,
                    intensity: 1.0,
                    params_json: r#"{"shape":"rounded_rect","bounds":[0.2,0.2,0.6,0.6],"corner_radius":0.08,"feather":0.03}"#.into(),
                }],
                false,
            ),
            (
                "background_blur",
                vec![], // tested separately on upper layer
                false,
            ),
        ];

        let baseline_solid = cpu
            .render_output(&base_scene(vec![], false), 0, w, h)
            .unwrap();

        for (label, effs, is_out) in cases {
            let scene = if label == "blur" {
                Scene {
                    revision: 1,
                    outputs: vec![OutputScene {
                        output_index: 0,
                        test_pattern: selah_proto::v1::TestPattern::SmpteBars as i32,
                        effects: effs,
                        ..Default::default()
                    }],
                    ..Default::default()
                }
            } else if label == "background_blur" {
                Scene {
                    revision: 1,
                    outputs: vec![OutputScene {
                        output_index: 0,
                        test_pattern: selah_proto::v1::TestPattern::SmpteBars as i32,
                        layers: vec![LayerSpec {
                            kind: LayerKind::Props as i32,
                            enabled: true,
                            effects: vec![EffectSpec {
                                kind: EffectKind::BackgroundBlur as i32,
                                enabled: true,
                                intensity: 1.0,
                                params_json: r#"{"radius":6.0,"region":[0.15,0.2,0.7,0.6],"tint_color":"\#ffffff40"}"#.replace("\\#", "#"),
                            }],
                            content: Some(layer_spec::Content::Props(PropsLayerSpec {
                                background_color: Some(Color {
                                    red: 1.0,
                                    green: 1.0,
                                    blue: 1.0,
                                    alpha: 0.15,
                                }),
                                ..Default::default()
                            })),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                    ..Default::default()
                }
            } else {
                base_scene(effs, is_out)
            };

            let cf: Frame = cpu.render_output(&scene, 0, w, h).unwrap();
            let gf: Frame = gpu.render_output(&scene, 0, w, h).unwrap();
            let baseline = if label == "blur" || label == "background_blur" {
                cpu.render_output(
                    &Scene {
                        revision: 1,
                        outputs: vec![OutputScene {
                            output_index: 0,
                            test_pattern: selah_proto::v1::TestPattern::SmpteBars as i32,
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    0,
                    w,
                    h,
                )
                .unwrap()
            } else {
                baseline_solid.clone()
            };
            assert_ne!(
                cf.checksum(),
                baseline.checksum(),
                "effect '{label}' must noticeably alter rendered pixels"
            );
            let mut max_diff = 0_i32;
            let mut bad = 0_usize;
            for (c, g) in cf.pixels.chunks_exact(4).zip(gf.pixels.chunks_exact(4)) {
                for ch in 0..4 {
                    let diff = (i32::from(c[ch]) - i32::from(g[ch])).abs();
                    max_diff = max_diff.max(diff);
                    if diff > 6 {
                        bad += 1;
                    }
                }
            }
            assert!(
                bad == 0,
                "CPU/GPU mismatch for effect '{label}': max_diff={max_diff}, bad={bad}"
            );
        }
    }
}
