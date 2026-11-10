//! Scene graph normalization, canonical 5-layer stack, overlay rasterisation, and cache (M3.1).
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use prost::Message;
use selah_proto::v1::{
    Color, GradientKind, GradientSpec, LayerKind, LayerSpec, MediaFitMode, MediaLayerSpec,
    MessagesLayerSpec, OutputConfig, OutputRole, OutputScene, PreloadHint, PropsLayerSpec,
    Resolution, Scene, SlideLayerSpec, TestPattern, VideoInputLayerSpec, layer_spec,
};

use crate::Frame;
use crate::cpu::pattern_pixel;
use crate::template::TemplateStyle;
use crate::text::{SlideTextStyle, TextRenderer, composite_over};

/// Canonical z-order from back (0) to front (4) on every output.
pub const LAYER_ORDER: [LayerKind; 5] = [
    LayerKind::VideoInput,
    LayerKind::Media,
    LayerKind::Slide,
    LayerKind::Props,
    LayerKind::Messages,
];

pub const fn layer_rank(kind: LayerKind) -> u8 {
    match kind {
        LayerKind::VideoInput => 0,
        LayerKind::Media => 1,
        LayerKind::Slide => 2,
        LayerKind::Props => 3,
        LayerKind::Messages => 4,
        LayerKind::Unspecified => u8::MAX,
    }
}

pub fn clamp_resolution(res: Option<Resolution>, fallback: (u32, u32)) -> (u32, u32) {
    res.map(|r| (r.width.clamp(16, 7680), r.height.clamp(16, 4320)))
        .unwrap_or((fallback.0.clamp(16, 7680), fallback.1.clamp(16, 4320)))
}

pub fn color_to_rgba(color: Option<Color>, default: [u8; 4]) -> [u8; 4] {
    let Some(c) = color else {
        return default;
    };
    [
        (c.red.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.green.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.blue.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn layer_kind_of(spec: &LayerSpec) -> LayerKind {
    match LayerKind::try_from(spec.kind) {
        Ok(kind) if kind != LayerKind::Unspecified => kind,
        _ => match spec.content.as_ref() {
            Some(layer_spec::Content::VideoInput(_)) => LayerKind::VideoInput,
            Some(layer_spec::Content::Media(_)) => LayerKind::Media,
            Some(layer_spec::Content::Slide(_)) => LayerKind::Slide,
            Some(layer_spec::Content::Props(_)) => LayerKind::Props,
            Some(layer_spec::Content::Messages(_)) => LayerKind::Messages,
            None => LayerKind::Unspecified,
        },
    }
}

pub fn layer_opacity(spec: &LayerSpec) -> f32 {
    if spec.use_opacity {
        return spec.opacity.clamp(0.0, 1.0);
    }
    match spec.content.as_ref() {
        Some(layer_spec::Content::VideoInput(v)) if v.use_opacity => v.opacity.clamp(0.0, 1.0),
        Some(layer_spec::Content::Media(m)) if m.use_opacity => m.opacity.clamp(0.0, 1.0),
        Some(layer_spec::Content::Slide(s)) if s.use_opacity => s.opacity.clamp(0.0, 1.0),
        Some(layer_spec::Content::Props(p)) if p.use_opacity => p.opacity.clamp(0.0, 1.0),
        Some(layer_spec::Content::Messages(m)) if m.use_opacity => m.opacity.clamp(0.0, 1.0),
        _ => 1.0,
    }
}

pub fn layer_has_content(spec: &LayerSpec) -> bool {
    if !spec.enabled {
        return false;
    }
    if layer_opacity(spec) <= 0.0 {
        return false;
    }
    match spec.content.as_ref() {
        Some(layer_spec::Content::VideoInput(v)) => video_input_has_content(v),
        Some(layer_spec::Content::Media(m)) => {
            !m.content_hash.trim().is_empty()
                || m.solid_color.is_some_and(|c| c.alpha > 0.0)
                || m.gradient.as_ref().is_some_and(|g| !g.stops.is_empty())
        }
        Some(layer_spec::Content::Slide(s)) => slide_has_content(s),
        Some(layer_spec::Content::Props(p)) => props_has_content(p),
        Some(layer_spec::Content::Messages(m)) => !m.text.trim().is_empty(),
        None => false,
    }
}

fn video_input_has_content(v: &VideoInputLayerSpec) -> bool {
    !v.source_id.trim().is_empty() || v.fallback_color.is_some_and(|c| c.alpha > 0.0)
}

fn slide_has_content(s: &SlideLayerSpec) -> bool {
    s.test_pattern != TestPattern::Unspecified as i32
        || !s.text.is_empty()
        || !s.elements_json.trim().is_empty()
        || !s.title_text.is_empty()
        || !s.reference_text.is_empty()
        || !s.translation_text.is_empty()
        || !s.footer_text.is_empty()
}

fn props_has_content(p: &PropsLayerSpec) -> bool {
    !p.label.trim().is_empty()
        || !p.elements_json.trim().is_empty()
        || p.background_color.is_some_and(|c| c.alpha > 0.0)
}

pub fn layer_is_time_varying(spec: &LayerSpec) -> bool {
    if !spec.enabled {
        return false;
    }
    if spec
        .transition
        .as_ref()
        .is_some_and(|t| t.duration_ms > 0 && t.kind > 1)
    {
        return true;
    }
    match spec.content.as_ref() {
        Some(layer_spec::Content::Media(m)) => m.media_kind == "video" && !m.paused,
        Some(layer_spec::Content::Slide(s)) => {
            s.text.contains("{clock}")
                || s.text.contains("{timer}")
                || s.elements_json.contains("{clock}")
                || s.elements_json.contains("{timer}")
        }
        Some(layer_spec::Content::Props(p)) => {
            p.label.contains("{clock}")
                || p.label.contains("{timer}")
                || p.elements_json.contains("{clock}")
                || p.elements_json.contains("{timer}")
        }
        Some(layer_spec::Content::Messages(m)) => {
            m.ticker || m.text.contains("{clock}") || m.text.contains("{timer}")
        }
        _ => false,
    }
}

/// Whether a single output scene displays something other than nothing or logo (SAFE-06).
pub fn output_scene_is_live(output: &OutputScene) -> bool {
    if output.cleared || output.show_logo {
        return false;
    }
    output.test_pattern != TestPattern::Unspecified as i32
        || output.layers.iter().any(layer_has_content)
}

/// Canonical liveness check across both legacy scenes and multi-output layer graphs (SAFE-06).
pub fn scene_is_live(scene: &Scene) -> bool {
    if scene.cleared {
        return false;
    }
    if scene.outputs.is_empty() {
        scene.test_pattern != TestPattern::Unspecified as i32 || !scene.text.is_empty()
    } else {
        scene.outputs.iter().any(output_scene_is_live)
    }
}

/// Whether any enabled output has time-varying layers or active transitions.
pub fn scene_is_time_varying(scene: &Scene) -> bool {
    if scene.cleared {
        return false;
    }
    scene.outputs.iter().any(|out| {
        !out.cleared
            && !out.show_logo
            && (out
                .transition
                .as_ref()
                .is_some_and(|t| t.duration_ms > 0 && t.kind > 1)
                || out.layers.iter().any(layer_is_time_varying))
    })
}

/// Sorted, normalized layer entry ready for composition and caching.
#[derive(Clone, Debug)]
pub struct NormalizedLayer {
    pub kind: LayerKind,
    pub opacity: f32,
    pub spec: LayerSpec,
    pub time_varying: bool,
    pub cache_key: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct NormalizedOutputScene {
    pub output_index: u32,
    pub role: OutputRole,
    pub width: u32,
    pub height: u32,
    pub cleared: bool,
    pub show_logo: bool,
    pub is_legacy: bool,
    pub test_pattern: TestPattern,
    pub legacy_text: String,
    pub layers: Vec<NormalizedLayer>,
    pub time_varying: bool,
    pub live: bool,
}

fn layer_cache_key(
    kind: LayerKind,
    opacity: f32,
    spec: &LayerSpec,
    width: u32,
    height: u32,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(kind as i32).to_le_bytes());
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
    hasher.update(&alpha.to_le_bytes());
    hasher.update(&width.to_le_bytes());
    hasher.update(&height.to_le_bytes());
    hasher.update(&spec.encode_to_vec());
    *hasher.finalize().as_bytes()
}

/// Determine active render targets `(output_index, (width, height))` for a scene and config list.
pub fn resolve_targets(
    scene: &Scene,
    configs: &[OutputConfig],
    explicit_configs: bool,
) -> Vec<(u32, (u32, u32))> {
    if !explicit_configs {
        if scene.outputs.is_empty() {
            return vec![(0, clamp_resolution(scene.resolution, (1920, 1080)))];
        }
        let mut seen = Vec::new();
        for out in &scene.outputs {
            if !seen.iter().any(|(idx, _)| *idx == out.output_index) {
                let res = clamp_resolution(out.resolution.or(scene.resolution), (1920, 1080));
                seen.push((out.output_index, res));
            }
        }
        return seen;
    }

    let mut targets = Vec::new();
    for cfg in configs.iter().filter(|c| c.enabled) {
        let out_res = scene
            .outputs
            .iter()
            .find(|o| o.output_index == cfg.output_index)
            .and_then(|o| o.resolution);
        let res = clamp_resolution(
            out_res.or(cfg.resolution).or(scene.resolution),
            (1920, 1080),
        );
        if !targets.iter().any(|(idx, _)| *idx == cfg.output_index) {
            targets.push((cfg.output_index, res));
        }
    }
    if targets.is_empty() {
        targets.push((0, clamp_resolution(scene.resolution, (1920, 1080))));
    }
    targets
}

/// Normalize one output's scene graph with layers sorted strictly in canonical 5-layer z-order.
pub fn normalize_output_scene(
    scene: &Scene,
    output_index: u32,
    width: u32,
    height: u32,
) -> NormalizedOutputScene {
    if scene.outputs.is_empty() {
        let test_pattern =
            TestPattern::try_from(scene.test_pattern).unwrap_or(TestPattern::Unspecified);
        let live =
            !scene.cleared && (test_pattern != TestPattern::Unspecified || !scene.text.is_empty());
        return NormalizedOutputScene {
            output_index,
            role: OutputRole::Main,
            width,
            height,
            cleared: scene.cleared,
            show_logo: false,
            is_legacy: true,
            test_pattern,
            legacy_text: scene.text.clone(),
            layers: Vec::new(),
            time_varying: false,
            live,
        };
    }

    let Some(out) = scene
        .outputs
        .iter()
        .find(|o| o.output_index == output_index)
    else {
        return NormalizedOutputScene {
            output_index,
            role: OutputRole::Main,
            width,
            height,
            cleared: true,
            show_logo: false,
            is_legacy: false,
            test_pattern: TestPattern::Unspecified,
            legacy_text: String::new(),
            layers: Vec::new(),
            time_varying: false,
            live: false,
        };
    };

    let role = OutputRole::try_from(out.role).unwrap_or(OutputRole::Main);
    let cleared = scene.cleared || out.cleared || out.show_logo;
    if cleared {
        return NormalizedOutputScene {
            output_index,
            role,
            width,
            height,
            cleared: true,
            show_logo: out.show_logo,
            is_legacy: false,
            test_pattern: TestPattern::Unspecified,
            legacy_text: String::new(),
            layers: Vec::new(),
            time_varying: false,
            live: false,
        };
    }

    let test_pattern = TestPattern::try_from(out.test_pattern).unwrap_or(TestPattern::Unspecified);
    let mut indexed: Vec<(u8, usize, NormalizedLayer)> = Vec::new();
    let mut time_varying = out
        .transition
        .as_ref()
        .is_some_and(|t| t.duration_ms > 0 && t.kind > 1);

    for (pos, spec) in out.layers.iter().enumerate() {
        let kind = layer_kind_of(spec);
        if kind == LayerKind::Unspecified || !layer_has_content(spec) {
            continue;
        }
        if layer_is_time_varying(spec) {
            time_varying = true;
        }
        let opacity = layer_opacity(spec);
        let cache_key = layer_cache_key(kind, opacity, spec, width, height);
        indexed.push((
            layer_rank(kind),
            pos,
            NormalizedLayer {
                kind,
                opacity,
                spec: spec.clone(),
                time_varying: layer_is_time_varying(spec),
                cache_key,
            },
        ));
    }

    indexed.sort_by_key(|(rank, pos, _)| (*rank, *pos));
    let layers: Vec<NormalizedLayer> = indexed.into_iter().map(|(_, _, l)| l).collect();
    let live = test_pattern != TestPattern::Unspecified || !layers.is_empty();

    NormalizedOutputScene {
        output_index,
        role,
        width,
        height,
        cleared: false,
        show_logo: false,
        is_legacy: false,
        test_pattern,
        legacy_text: String::new(),
        layers,
        time_varying,
        live,
    }
}

/// LRU cache for rasterised premultiplied RGBA overlays across layer types.
pub struct OverlayCache {
    cache: HashMap<[u8; 32], Arc<Frame>>,
    order: Vec<[u8; 32]>,
    capacity: usize,
    hits: u64,
    misses: u64,
}

impl Default for OverlayCache {
    fn default() -> Self {
        Self::new(128)
    }
}

impl OverlayCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: HashMap::new(),
            order: Vec::new(),
            capacity: capacity.max(8),
            hits: 0,
            misses: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.order.clear();
    }

    pub fn get(&mut self, key: &[u8; 32]) -> Option<Arc<Frame>> {
        if let Some(hit) = self.cache.get(key) {
            self.hits += 1;
            let hit = hit.clone();
            self.order.retain(|k| k != key);
            self.order.push(*key);
            Some(hit)
        } else {
            self.misses += 1;
            None
        }
    }

    pub fn insert(&mut self, key: [u8; 32], frame: Arc<Frame>) {
        if let Some(existing) = self.cache.get_mut(&key) {
            *existing = frame;
            self.order.retain(|k| *k != key);
            self.order.push(key);
            return;
        }
        if self.cache.len() >= self.capacity && !self.order.is_empty() {
            let oldest = self.order.remove(0);
            self.cache.remove(&oldest);
        }
        self.cache.insert(key, frame);
        self.order.push(key);
    }
}

fn premultiply(rgba: [u8; 4]) -> [u8; 4] {
    let a = u32::from(rgba[3]);
    [
        ((u32::from(rgba[0]) * a + 127) / 255) as u8,
        ((u32::from(rgba[1]) * a + 127) / 255) as u8,
        ((u32::from(rgba[2]) * a + 127) / 255) as u8,
        rgba[3],
    ]
}

pub fn scale_frame_opacity(frame: &mut Frame, opacity: f32) {
    let scale = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
    if scale >= 255 {
        return;
    }
    for pixel in frame.pixels.chunks_exact_mut(4) {
        for ch in pixel.iter_mut() {
            *ch = ((u32::from(*ch) * scale + 127) / 255) as u8;
        }
    }
}

fn solid_frame(rgba: [u8; 4], width: u32, height: u32) -> Frame {
    let premul = premultiply(rgba);
    let mut frame = Frame::new(width, height);
    for px in frame.pixels.chunks_exact_mut(4) {
        px.copy_from_slice(&premul);
    }
    frame
}

fn lerp_color(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let mix = |ca: u8, cb: u8| -> u8 {
        (f32::from(ca) + (f32::from(cb) - f32::from(ca)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    [
        mix(a[0], b[0]),
        mix(a[1], b[1]),
        mix(a[2], b[2]),
        mix(a[3], b[3]),
    ]
}

fn gradient_frame(gradient: &GradientSpec, width: u32, height: u32) -> Frame {
    let mut stops: Vec<(f32, [u8; 4])> = gradient
        .stops
        .iter()
        .map(|s| {
            (
                s.position.clamp(0.0, 1.0),
                color_to_rgba(s.color, [0, 0, 0, 255]),
            )
        })
        .collect();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let sample = |t: f32| -> [u8; 4] {
        let t = t.clamp(0.0, 1.0);
        if stops.is_empty() {
            return [0, 0, 0, 255];
        }
        if t <= stops[0].0 {
            return premultiply(stops[0].1);
        }
        for pair in stops.windows(2) {
            if t <= pair[1].0 {
                let span = (pair[1].0 - pair[0].0).max(1e-5);
                let local = (t - pair[0].0) / span;
                return premultiply(lerp_color(pair[0].1, pair[1].1, local));
            }
        }
        premultiply(stops.last().expect("non-empty").1)
    };
    let mut frame = Frame::new(width, height);
    let kind = GradientKind::try_from(gradient.kind).unwrap_or(GradientKind::Linear);
    let rad = gradient.angle_degrees.to_radians();
    let (dx, dy) = (rad.cos(), rad.sin());
    let w_f = width.saturating_sub(1).max(1) as f32;
    let h_f = height.saturating_sub(1).max(1) as f32;
    for y in 0..height {
        let ny = y as f32 / h_f - 0.5;
        for x in 0..width {
            let nx = x as f32 / w_f - 0.5;
            let t = match kind {
                GradientKind::Radial => ((nx * nx + ny * ny).sqrt() * 2.0).clamp(0.0, 1.0),
                GradientKind::Linear | GradientKind::Unspecified => {
                    (nx * dx + ny * dy + 0.5).clamp(0.0, 1.0)
                }
            };
            let px = sample(t);
            let at = (y as usize * width as usize + x as usize) * 4;
            frame.pixels[at..at + 4].copy_from_slice(&px);
        }
    }
    frame
}

/// Rasterise static `solid_color`, `gradient`, and (when `include_image`) verified `content_hash`
/// background image for a [`MediaLayerSpec`].
pub fn media_backdrop_frame(
    m: &MediaLayerSpec,
    width: u32,
    height: u32,
    media_root: Option<&Path>,
    include_image: bool,
) -> Result<Option<Frame>> {
    let mut base = m
        .solid_color
        .filter(|c| c.alpha > 0.0)
        .map(|c| solid_frame(color_to_rgba(Some(c), [0, 0, 0, 255]), width, height));

    if let Some(grad) = m.gradient.as_ref().filter(|g| !g.stops.is_empty()) {
        let grad_frame = gradient_frame(grad, width, height);
        match &mut base {
            Some(b) => composite_over(b, &grad_frame),
            None => base = Some(grad_frame),
        }
    }

    if include_image && !m.content_hash.trim().is_empty() && m.media_kind != "video" {
        if let Some(b) = base.take() {
            base = Some(crate::media_bg::blur_frame_skia(&b, m.blur_radius)?);
        }
        let root = media_root.with_context(|| {
            format!(
                "media root unavailable for background image {}",
                m.content_hash
            )
        })?;
        let ext = if m.media_extension.trim().is_empty() {
            "png"
        } else {
            m.media_extension.trim()
        };
        let image = crate::media_bg::load_verified_image(root, m.content_hash.trim(), ext)?;
        let fit = MediaFitMode::try_from(m.fit).unwrap_or(MediaFitMode::Fill);
        let img_frame =
            crate::media_bg::render_image_frame(&image, fit, m.blur_radius, width, height)?;
        match &mut base {
            Some(b) => composite_over(b, &img_frame),
            None => base = Some(img_frame),
        }
        return Ok(base);
    }

    match base {
        Some(b) => Ok(Some(crate::media_bg::blur_frame_skia(&b, m.blur_radius)?)),
        None => Ok(None),
    }
}

/// Rasterise (or return cached) premultiplied RGBA overlay for one normalized layer.
pub fn rasterise_layer_overlay(
    layer: &NormalizedLayer,
    width: u32,
    height: u32,
    text_renderer: &mut Option<TextRenderer>,
    cache: &mut OverlayCache,
    media_root: Option<&Path>,
) -> Result<Option<Arc<Frame>>> {
    if let Some(hit) = cache.get(&layer.cache_key) {
        return Ok(Some(hit));
    }

    let built: Option<Frame> = match layer.spec.content.as_ref() {
        Some(layer_spec::Content::VideoInput(v)) => {
            let color = color_to_rgba(v.fallback_color, [24, 34, 48, 255]);
            if color[3] == 0 {
                None
            } else {
                Some(solid_frame(color, width, height))
            }
        }
        Some(layer_spec::Content::Media(m)) => {
            media_backdrop_frame(m, width, height, media_root, true)?
        }
        Some(layer_spec::Content::Slide(s)) => {
            let pattern = TestPattern::try_from(s.test_pattern).unwrap_or(TestPattern::Unspecified);
            let mut base = if pattern != TestPattern::Unspecified {
                let mut f = Frame::new(width, height);
                for y in 0..height {
                    for x in 0..width {
                        let at = (y as usize * width as usize + x as usize) * 4;
                        f.pixels[at..at + 4]
                            .copy_from_slice(&pattern_pixel(pattern, x, y, width, height));
                    }
                }
                Some(f)
            } else {
                None
            };
            if slide_has_renderable_text(s) {
                if text_renderer.is_none() {
                    *text_renderer = Some(TextRenderer::bundled()?);
                }
                let text_frame = rasterise_slide_text(
                    s,
                    width,
                    height,
                    text_renderer.as_mut().expect("text renderer exists"),
                )?;
                match &mut base {
                    Some(b) => composite_over(b, &text_frame),
                    None => base = Some((*text_frame).clone()),
                }
            }
            base
        }
        Some(layer_spec::Content::Props(p)) => {
            if p.label.trim().is_empty() {
                p.background_color
                    .filter(|c| c.alpha > 0.0)
                    .map(|c| solid_frame(color_to_rgba(Some(c), [20, 40, 80, 220]), width, height))
            } else {
                if text_renderer.is_none() {
                    *text_renderer = Some(TextRenderer::bundled()?);
                }
                let bg = color_to_rgba(p.background_color, [20, 40, 80, 220]);
                let fg = color_to_rgba(p.text_color, [255, 255, 255, 255]);
                let frame = text_renderer
                    .as_mut()
                    .expect("text renderer exists")
                    .render_overlay_box(&p.label, bg, fg, false, width, height)?;
                Some((*frame).clone())
            }
        }
        Some(layer_spec::Content::Messages(MessagesLayerSpec {
            text,
            background_color,
            text_color,
            ..
        })) => {
            if text.trim().is_empty() {
                None
            } else {
                if text_renderer.is_none() {
                    *text_renderer = Some(TextRenderer::bundled()?);
                }
                let bg = color_to_rgba(*background_color, [140, 32, 32, 235]);
                let fg = color_to_rgba(*text_color, [255, 255, 255, 255]);
                let frame = text_renderer
                    .as_mut()
                    .expect("text renderer exists")
                    .render_overlay_box(text, bg, fg, true, width, height)?;
                Some((*frame).clone())
            }
        }
        None => None,
    };

    let Some(mut frame) = built else {
        return Ok(None);
    };
    scale_frame_opacity(&mut frame, layer.opacity);
    let arc = Arc::new(frame);
    cache.insert(layer.cache_key, arc.clone());
    Ok(Some(arc))
}

fn slide_has_renderable_text(s: &SlideLayerSpec) -> bool {
    !s.text.is_empty()
        || !s.title_text.is_empty()
        || !s.reference_text.is_empty()
        || !s.translation_text.is_empty()
        || !s.footer_text.is_empty()
}

fn rasterise_slide_text(
    s: &SlideLayerSpec,
    width: u32,
    height: u32,
    renderer: &mut TextRenderer,
) -> Result<Arc<Frame>> {
    if s.style_json.trim().is_empty()
        && s.title_text.is_empty()
        && s.reference_text.is_empty()
        && s.translation_text.is_empty()
        && s.footer_text.is_empty()
        && s.chords_json.is_empty()
    {
        let style = SlideTextStyle::for_output(height);
        return renderer.render(&s.text, &style, width, height);
    }
    let template = match TemplateStyle::from_json(&s.style_json) {
        Ok(t) => t,
        Err(error) => {
            tracing::warn!(%error, "ignoring malformed template style_json on slide");
            TemplateStyle::default()
        }
    };
    let resolved = template.resolve_for_output(width, height, None);
    let frame = renderer.render_slide(s, &resolved, width, height)?;
    Ok(Arc::new(frame))
}

/// Pre-rasterise slide text and normalized layers for upcoming hints or scene outputs.
pub fn prerasterise_hints(
    hints: &[PreloadHint],
    text_renderer: &mut Option<TextRenderer>,
    overlay_cache: &mut OverlayCache,
) -> Result<usize> {
    let mut warmed = 0usize;
    for hint in hints {
        if hint.text.is_empty() {
            continue;
        }
        let (width, height) = clamp_resolution(hint.resolution, (1920, 1080));
        if text_renderer.is_none() {
            *text_renderer = Some(TextRenderer::bundled()?);
        }
        let slide = SlideLayerSpec {
            text: hint.text.clone(),
            style_json: hint.style_json.clone(),
            elements_json: hint.elements_json.clone(),
            ..Default::default()
        };
        let frame = rasterise_slide_text(
            &slide,
            width,
            height,
            text_renderer.as_mut().expect("text renderer exists"),
        )?;
        let spec = LayerSpec {
            kind: LayerKind::Slide as i32,
            enabled: true,
            content: Some(layer_spec::Content::Slide(slide)),
            ..Default::default()
        };
        let key = layer_cache_key(LayerKind::Slide, 1.0, &spec, width, height);
        overlay_cache.insert(key, frame);
        warmed += 1;
    }
    Ok(warmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use selah_proto::v1::MediaLayerSpec;

    #[test]
    fn normalize_sorts_all_five_layers_in_canonical_z_order() {
        let reverse_layers = vec![
            LayerSpec {
                kind: LayerKind::Messages as i32,
                enabled: true,
                content: Some(layer_spec::Content::Messages(MessagesLayerSpec {
                    text: "Nursery 12".into(),
                    ..Default::default()
                })),
                ..Default::default()
            },
            LayerSpec {
                kind: LayerKind::Props as i32,
                enabled: true,
                content: Some(layer_spec::Content::Props(PropsLayerSpec {
                    label: "LIVE".into(),
                    ..Default::default()
                })),
                ..Default::default()
            },
            LayerSpec {
                kind: LayerKind::Slide as i32,
                enabled: true,
                content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                    text: "Amazing grace".into(),
                    ..Default::default()
                })),
                ..Default::default()
            },
            LayerSpec {
                kind: LayerKind::Media as i32,
                enabled: true,
                content: Some(layer_spec::Content::Media(MediaLayerSpec {
                    solid_color: Some(Color {
                        red: 0.1,
                        green: 0.2,
                        blue: 0.5,
                        alpha: 1.0,
                    }),
                    ..Default::default()
                })),
                ..Default::default()
            },
            LayerSpec {
                kind: LayerKind::VideoInput as i32,
                enabled: true,
                content: Some(layer_spec::Content::VideoInput(VideoInputLayerSpec {
                    source_id: "cam-1".into(),
                    ..Default::default()
                })),
                ..Default::default()
            },
        ];
        let scene = Scene {
            revision: 1,
            outputs: vec![OutputScene {
                output_index: 0,
                layers: reverse_layers,
                ..Default::default()
            }],
            ..Default::default()
        };
        let norm = normalize_output_scene(&scene, 0, 1920, 1080);
        let kinds: Vec<_> = norm.layers.iter().map(|l| l.kind).collect();
        assert_eq!(kinds, LAYER_ORDER);
        assert!(norm.live);
    }

    #[test]
    fn canonical_liveness_covers_legacy_and_layer_scenes() {
        let empty = Scene {
            cleared: false,
            ..Default::default()
        };
        assert!(!scene_is_live(&empty));

        let with_prop = Scene {
            cleared: false,
            outputs: vec![OutputScene {
                output_index: 0,
                layers: vec![LayerSpec {
                    kind: LayerKind::Props as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Props(PropsLayerSpec {
                        label: "Logo Bug".into(),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(scene_is_live(&with_prop));

        let logo_only = Scene {
            cleared: false,
            outputs: vec![OutputScene {
                output_index: 0,
                show_logo: true,
                layers: with_prop.outputs[0].layers.clone(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(!scene_is_live(&logo_only));
    }
}
