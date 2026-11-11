//! The wgpu compositor (ADR-005): D3D12 (WARP when there's no GPU) on Windows, Metal on
//! macOS, Vulkan (lavapipe in the cloud and CI) on Linux.
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use prost::Message;
use selah_proto::v1::{LayerKind, LayerSpec, MediaFitMode, PreloadHint, Scene, layer_spec};

use crate::{Compositor, Frame};

const SHADER: &str = include_str!("patterns.wgsl");
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const OVERLAY_GPU_CACHE_MAX: usize = 64;

/// Which adapter to ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterChoice {
    /// The best hardware adapter, falling back to software.
    Auto,
    /// Software only (WARP, lavapipe): deterministic, used by golden tests.
    Software,
}

impl AdapterChoice {
    /// `SELAH_GPU=software` forces the software adapter.
    pub fn from_env() -> Self {
        match std::env::var("SELAH_GPU").as_deref() {
            Ok("software") => AdapterChoice::Software,
            _ => AdapterChoice::Auto,
        }
    }
}

struct Target {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_row: u32,
}

struct OverlaySlot {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct TexPair {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl TexPair {
    fn new(device: &wgpu::Device, label: &str, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view }
    }
}

struct ScratchSet {
    ping_a: TexPair,
    ping_b: TexPair,
    backdrop: TexPair,
    bg_blur_out: TexPair,
    layer_tr_a: TexPair,
    layer_tr_b: TexPair,
    layer_tr_out: TexPair,
    out_tr_a: TexPair,
    out_tr_b: TexPair,
}

impl ScratchSet {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        Self {
            ping_a: TexPair::new(device, "scratch-ping-a", width, height),
            ping_b: TexPair::new(device, "scratch-ping-b", width, height),
            backdrop: TexPair::new(device, "scratch-backdrop", width, height),
            bg_blur_out: TexPair::new(device, "scratch-bg-blur-out", width, height),
            layer_tr_a: TexPair::new(device, "scratch-layer-tr-a", width, height),
            layer_tr_b: TexPair::new(device, "scratch-layer-tr-b", width, height),
            layer_tr_out: TexPair::new(device, "scratch-layer-tr-out", width, height),
            out_tr_a: TexPair::new(device, "scratch-out-tr-a", width, height),
            out_tr_b: TexPair::new(device, "scratch-out-tr-b", width, height),
        }
    }
}

/// Timing of the last frame, for FrameShown and the benchmark.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameTiming {
    /// When the frame was handed to the GPU queue: the headless equivalent of a swap-chain
    /// present, and what FrameShown reports when there are no windows.
    pub submitted: Option<Instant>,
    /// When the GPU finished drawing it.
    pub rendered: Option<Instant>,
}

pub struct GpuCompositor {
    instance: wgpu::Instance,
    adapter_handle: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter: wgpu::AdapterInfo,
    pipeline: wgpu::RenderPipeline,
    params_layout: wgpu::BindGroupLayout,
    pattern_pool: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    pattern_cursor: usize,
    targets: BTreeMap<u32, Target>,
    scratches: HashMap<(u32, u32), ScratchSet>,
    scratch_order: Vec<(u32, u32)>,
    name: String,
    pub timing: FrameTiming,
    text: Option<crate::text::TextRenderer>,
    overlays: crate::scene::OverlayCache,
    overlay_slots: HashMap<([u8; 32], u32, u32), OverlaySlot>,
    overlay_order: Vec<([u8; 32], u32, u32)>,
    layer_blitter: crate::blit::Blitter,
    video: crate::video::VideoLayer,
    video_frame: Option<crate::video::Nv12Frame>,
    video_frames: HashMap<String, (u64, Arc<crate::video::Nv12Frame>)>,
    video_seq: u64,
    blur: crate::blur::BlurPipeline,
    effects: crate::effects::EffectsPipeline,
    transitions: crate::transitions::TransitionPipeline,
    media_root: Option<PathBuf>,
    clock_unix_ms: Option<i64>,
}

fn backends() -> wgpu::Backends {
    if cfg!(windows) {
        wgpu::Backends::DX12
    } else if cfg!(target_os = "macos") {
        wgpu::Backends::METAL
    } else {
        wgpu::Backends::VULKAN
    }
}

enum PreparedLayer<'a> {
    Overlay {
        key: ([u8; 32], u32, u32),
        effects: &'a crate::effects::ValidatedShaderChain,
    },
    Video {
        hash: &'a str,
        bg_key: Option<([u8; 32], u32, u32)>,
        fit: MediaFitMode,
        blur_radius: f32,
        opacity: f32,
        effects: &'a crate::effects::ValidatedShaderChain,
    },
}

enum PreparedLayerEntry {
    Overlay {
        key: ([u8; 32], u32, u32),
        effects: crate::effects::ValidatedShaderChain,
    },
    Video {
        hash: String,
        bg_key: Option<([u8; 32], u32, u32)>,
        fit: MediaFitMode,
        blur_radius: f32,
        opacity: f32,
        effects: crate::effects::ValidatedShaderChain,
    },
}

impl PreparedLayerEntry {
    fn as_ref(&self) -> PreparedLayer<'_> {
        match self {
            Self::Overlay { key, effects } => PreparedLayer::Overlay { key: *key, effects },
            Self::Video {
                hash,
                bg_key,
                fit,
                blur_radius,
                opacity,
                effects,
            } => PreparedLayer::Video {
                hash,
                bg_key: *bg_key,
                fit: *fit,
                blur_radius: *blur_radius,
                opacity: *opacity,
                effects,
            },
        }
    }

    fn effects(&self) -> &crate::effects::ValidatedShaderChain {
        match self {
            Self::Overlay { effects, .. } | Self::Video { effects, .. } => effects,
        }
    }
}

enum PreparedDrawOp {
    Direct(PreparedLayerEntry),
    Transition {
        from: Option<PreparedLayerEntry>,
        to: Option<PreparedLayerEntry>,
        kind: selah_proto::v1::TransitionKind,
        direction: selah_proto::v1::TransitionDirection,
        progress: f32,
    },
}

struct PreparedScene {
    pattern_slot: usize,
    is_legacy: bool,
    cleared: bool,
    has_legacy_video: bool,
    legacy_text_key: Option<([u8; 32], u32, u32)>,
    ops: Vec<PreparedDrawOp>,
    output_effects: crate::effects::ValidatedShaderChain,
}

impl GpuCompositor {
    pub fn new(choice: AdapterChoice) -> Result<Self> {
        static GPU_INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _init_guard = GPU_INIT_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = backends();
        let instance = wgpu::Instance::new(descriptor);
        let request = |force_fallback_adapter| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter,
                compatible_surface: None,
                apply_limit_buckets: false,
            }))
        };
        let adapter = match choice {
            // Metal has no software adapter; its results for these shaders are exact anyway.
            AdapterChoice::Software if cfg!(target_os = "macos") => request(false),
            AdapterChoice::Software => request(true),
            AdapterChoice::Auto => request(false).or_else(|_| request(true)),
        }
        .map_err(|error| anyhow!("no GPU adapter: {error}"))?;
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("selah-compositor"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .context("could not open the GPU device")?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("test-patterns"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout_entries = [wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }];
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pattern-params"),
            entries: &layout_entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pattern-layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pattern-pipeline"),
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
        let layer_blitter = crate::blit::Blitter::new_layer(&device, TARGET_FORMAT);
        let video = crate::video::VideoLayer::new(&device, TARGET_FORMAT);
        let blur = crate::blur::BlurPipeline::new(&device, TARGET_FORMAT);
        let effects = crate::effects::EffectsPipeline::new(&device);
        let transitions = crate::transitions::TransitionPipeline::new(&device);
        let name = format!("wgpu {:?} {}", info.backend, info.name);
        tracing::info!(adapter = %info.name, backend = ?info.backend, device_type = ?info.device_type, "GPU compositor ready");
        Ok(Self {
            instance,
            adapter_handle: adapter,
            device,
            queue,
            adapter: info,
            pipeline,
            params_layout: bind_layout,
            pattern_pool: Vec::new(),
            pattern_cursor: 0,
            targets: BTreeMap::new(),
            scratches: HashMap::new(),
            scratch_order: Vec::new(),
            name,
            timing: FrameTiming::default(),
            text: None,
            overlays: crate::scene::OverlayCache::default(),
            overlay_slots: HashMap::new(),
            overlay_order: Vec::new(),
            layer_blitter,
            video,
            video_frame: None,
            video_frames: HashMap::new(),
            video_seq: 0,
            blur,
            effects,
            transitions,
            media_root: crate::media_bg::default_media_root_from_env(),
            clock_unix_ms: None,
        })
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter
    }

    pub fn is_software_adapter(&self) -> bool {
        self.adapter.device_type == wgpu::DeviceType::Cpu
    }

    pub fn is_virtual_adapter(&self) -> bool {
        self.adapter.device_type == wgpu::DeviceType::VirtualGpu
    }

    pub fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter_handle
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// The composited output of the last render on output 0, for presenting to windows.
    pub fn output_view(&self) -> Option<&wgpu::TextureView> {
        self.output_view_for(0)
    }

    /// The composited output texture view for `output_index`.
    pub fn output_view_for(&self, output_index: u32) -> Option<&wgpu::TextureView> {
        self.targets.get(&output_index).map(|target| &target.view)
    }

    fn alloc_pattern_params(&mut self, bytes: &[u8]) -> usize {
        let slot = self.pattern_cursor;
        self.pattern_cursor += 1;
        if slot >= self.pattern_pool.len() {
            let params = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("pattern-params"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("pattern-params"),
                layout: &self.params_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                }],
            });
            self.pattern_pool.push((params, bg));
        }
        self.queue
            .write_buffer(&self.pattern_pool[slot].0, 0, bytes);
        slot
    }

    fn ensure_target(&mut self, output_index: u32, width: u32, height: u32) {
        let stale = self
            .targets
            .get(&output_index)
            .is_none_or(|t| t.width != width || t.height != height);
        if stale {
            let label = format!("output-{output_index}");
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(&label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TARGET_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let readback_label = format!("output-{output_index}-readback");
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&readback_label),
                size: u64::from(padded_row) * u64::from(height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            self.targets.insert(
                output_index,
                Target {
                    width,
                    height,
                    texture,
                    view,
                    readback,
                    padded_row,
                },
            );
        }
    }

    fn ensure_scratch(&mut self, width: u32, height: u32) {
        let key = (width, height);
        if self.scratches.contains_key(&key) {
            self.scratch_order.retain(|k| *k != key);
            self.scratch_order.push(key);
            return;
        }
        while self.scratches.len() >= 4 && !self.scratch_order.is_empty() {
            let oldest = self.scratch_order.remove(0);
            self.scratches.remove(&oldest);
        }
        self.scratches
            .insert(key, ScratchSet::new(&self.device, width, height));
        self.scratch_order.push(key);
    }

    fn upload_overlay_keyed(&mut self, key: [u8; 32], frame: &Frame) -> ([u8; 32], u32, u32) {
        self.upload_overlay_inner(key, frame, false)
    }

    fn upload_overlay_dynamic(&mut self, key: [u8; 32], frame: &Frame) -> ([u8; 32], u32, u32) {
        self.upload_overlay_inner(key, frame, true)
    }

    fn upload_overlay_inner(
        &mut self,
        key: [u8; 32],
        frame: &Frame,
        force_update: bool,
    ) -> ([u8; 32], u32, u32) {
        let (width, height) = (frame.width.max(1), frame.height.max(1));
        let full_key = (key, width, height);
        if let Some(existing) = self.overlay_slots.get(&full_key) {
            if force_update {
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &existing.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &frame.pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * 4),
                        rows_per_image: Some(height),
                    },
                    wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );
            }
            self.overlay_order.retain(|k| *k != full_key);
            self.overlay_order.push(full_key);
            return full_key;
        }
        while self.overlay_slots.len() >= OVERLAY_GPU_CACHE_MAX && !self.overlay_order.is_empty() {
            let oldest = self.overlay_order.remove(0);
            self.overlay_slots.remove(&oldest);
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer-overlay"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.overlay_slots
            .insert(full_key, OverlaySlot { texture, view });
        self.overlay_order.push(full_key);
        full_key
    }

    fn upload_legacy_text_layer(
        &mut self,
        scene: &Scene,
        width: u32,
        height: u32,
    ) -> Result<Option<([u8; 32], u32, u32)>> {
        if scene.cleared || scene.text.is_empty() {
            return Ok(None);
        }
        let style = crate::text::SlideTextStyle::for_output(height);
        let mut key_hash = blake3::Hasher::new();
        key_hash.update(scene.text.as_bytes());
        key_hash.update(&width.to_le_bytes());
        key_hash.update(&height.to_le_bytes());
        let key = *key_hash.finalize().as_bytes();
        let full_key = (key, width, height);
        if self.overlay_slots.contains_key(&full_key) {
            return Ok(Some(full_key));
        }
        if self.text.is_none() {
            self.text = Some(crate::text::TextRenderer::bundled()?);
        }
        let frame = self.text.as_mut().expect("text renderer exists").render(
            &scene.text,
            &style,
            width,
            height,
        )?;
        Ok(Some(self.upload_overlay_keyed(key, &frame)))
    }

    fn normalize_single_layer(
        spec: &LayerSpec,
        width: u32,
        height: u32,
    ) -> Option<crate::scene::NormalizedLayer> {
        if !spec.enabled || !crate::scene::layer_has_content(spec) {
            return None;
        }
        let kind = LayerKind::try_from(spec.kind).unwrap_or(LayerKind::Unspecified);
        let opacity = crate::scene::layer_opacity(spec);
        let raw_effects = crate::effects::collect_layer_effect_specs(spec);
        let effects = crate::effects::ValidatedShaderChain::from_specs_lossy(&raw_effects);
        let mut hasher = blake3::Hasher::new();
        hasher.update(&(kind as i32).to_le_bytes());
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
        hasher.update(&alpha.to_le_bytes());
        hasher.update(&width.to_le_bytes());
        hasher.update(&height.to_le_bytes());
        hasher.update(&spec.encode_to_vec());
        let cache_key = *hasher.finalize().as_bytes();
        Some(crate::scene::NormalizedLayer {
            kind,
            opacity,
            spec: spec.clone(),
            effects,
            time_varying: crate::scene::layer_is_time_varying(spec),
            cache_key,
        })
    }

    fn prepare_normalized_layer(
        &mut self,
        layer: &crate::scene::NormalizedLayer,
        width: u32,
        height: u32,
    ) -> Result<Option<PreparedLayerEntry>> {
        if layer.kind == LayerKind::Media
            && let Some(layer_spec::Content::Media(m)) = layer.spec.content.as_ref()
            && m.media_kind == "video"
            && let Some((seq, nv12)) = self
                .video_frames
                .get(&m.content_hash)
                .map(|(seq, frame)| (*seq, Arc::clone(frame)))
        {
            let has_static_bg = m.solid_color.is_some_and(|c| c.alpha > 0.0)
                || m.gradient.as_ref().is_some_and(|g| !g.stops.is_empty());
            let bg_key = if has_static_bg {
                let mut key_h = blake3::Hasher::new();
                key_h.update(&layer.cache_key);
                key_h.update(b"video-bg");
                let k = *key_h.finalize().as_bytes();
                let full_key = (k, width.max(1), height.max(1));
                if self.overlay_slots.contains_key(&full_key) {
                    self.overlay_order.retain(|item| *item != full_key);
                    self.overlay_order.push(full_key);
                    Some(full_key)
                } else if let Some(mut bg) = crate::scene::media_backdrop_frame(
                    m,
                    width,
                    height,
                    self.media_root.as_deref(),
                    false,
                )? {
                    crate::scene::scale_frame_opacity(&mut bg, layer.opacity);
                    Some(self.upload_overlay_keyed(k, &bg))
                } else {
                    None
                }
            } else {
                None
            };
            self.video
                .upload_keyed(&self.device, &self.queue, &m.content_hash, seq, &nv12)?;
            let fit = MediaFitMode::try_from(m.fit).unwrap_or(MediaFitMode::Fill);
            if m.blur_radius > 0.05 && m.blur_radius.is_finite() {
                self.blur
                    .ensure_source_view(&self.device, width, height, m.blur_radius);
            }
            return Ok(Some(PreparedLayerEntry::Video {
                hash: m.content_hash.clone(),
                bg_key,
                fit,
                blur_radius: m.blur_radius,
                opacity: layer.opacity,
                effects: layer.effects.clone(),
            }));
        }
        let clock_ms = crate::scene::effective_clock_unix_ms(self.clock_unix_ms);
        if let Some(overlay) = crate::scene::rasterise_layer_overlay(
            layer,
            width,
            height,
            &mut self.text,
            &mut self.overlays,
            self.media_root.as_deref(),
            &self.video_frames,
            clock_ms,
        )? {
            let key = if layer.time_varying {
                self.upload_overlay_dynamic(layer.cache_key, &overlay)
            } else {
                self.upload_overlay_keyed(layer.cache_key, &overlay)
            };
            return Ok(Some(PreparedLayerEntry::Overlay {
                key,
                effects: layer.effects.clone(),
            }));
        }
        Ok(None)
    }

    fn prepare_scene(
        &mut self,
        scene: &Scene,
        output_index: u32,
        width: u32,
        height: u32,
        layer_transitions: &[crate::transitions::LayerTransitionBlend],
    ) -> Result<PreparedScene> {
        let normalized = crate::scene::normalize_output_scene(scene, output_index, width, height);
        let words = [
            normalized.test_pattern as u32,
            u32::from(normalized.cleared),
            width,
            height,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let pattern_slot = self.alloc_pattern_params(&bytes);

        if normalized.is_legacy {
            let legacy_text_key = self.upload_legacy_text_layer(scene, width, height)?;
            let has_legacy_video = match self.video_frame.as_ref() {
                Some(video) if !normalized.cleared => {
                    self.video
                        .upload(&self.device, &self.queue, video, width, height)?;
                    true
                }
                _ => false,
            };
            return Ok(PreparedScene {
                pattern_slot,
                is_legacy: true,
                cleared: normalized.cleared,
                has_legacy_video,
                legacy_text_key,
                ops: Vec::new(),
                output_effects: crate::effects::ValidatedShaderChain::default(),
            });
        }

        let mut ops = Vec::new();
        if !normalized.cleared {
            enum Item<'a> {
                Cur(&'a crate::scene::NormalizedLayer),
                Exit(&'a crate::transitions::LayerTransitionBlend),
            }
            let mut merged: Vec<(u8, Item<'_>)> = normalized
                .layers
                .iter()
                .map(|l| (crate::scene::layer_rank(l.kind), Item::Cur(l)))
                .collect();
            for tr in layer_transitions {
                if !normalized.layers.iter().any(|l| l.kind == tr.kind) {
                    merged.push((crate::scene::layer_rank(tr.kind), Item::Exit(tr)));
                }
            }
            merged.sort_by_key(|(rank, _)| *rank);

            for (_, item) in merged {
                match item {
                    Item::Cur(layer) => {
                        if let Some(tr) = layer_transitions.iter().find(|t| t.kind == layer.kind) {
                            let from = tr
                                .from_layer
                                .as_ref()
                                .and_then(|spec| Self::normalize_single_layer(spec, width, height))
                                .and_then(|nl| {
                                    self.prepare_normalized_layer(&nl, width, height)
                                        .transpose()
                                })
                                .transpose()?;
                            let to = self.prepare_normalized_layer(layer, width, height)?;
                            ops.push(PreparedDrawOp::Transition {
                                from,
                                to,
                                kind: tr.transition,
                                direction: tr.direction,
                                progress: tr.progress,
                            });
                        } else if let Some(prepared) =
                            self.prepare_normalized_layer(layer, width, height)?
                        {
                            ops.push(PreparedDrawOp::Direct(prepared));
                        }
                    }
                    Item::Exit(tr) => {
                        let from = tr
                            .from_layer
                            .as_ref()
                            .and_then(|spec| Self::normalize_single_layer(spec, width, height))
                            .and_then(|nl| {
                                self.prepare_normalized_layer(&nl, width, height)
                                    .transpose()
                            })
                            .transpose()?;
                        if from.is_some() {
                            ops.push(PreparedDrawOp::Transition {
                                from,
                                to: None,
                                kind: tr.transition,
                                direction: tr.direction,
                                progress: tr.progress,
                            });
                        }
                    }
                }
            }
        }

        Ok(PreparedScene {
            pattern_slot,
            is_legacy: false,
            cleared: normalized.cleared,
            has_legacy_video: false,
            legacy_text_key: None,
            ops,
            output_effects: normalized.output_effects,
        })
    }

    fn wait(&self) -> Result<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|error| anyhow!("GPU poll failed: {error}"))
    }
}

#[allow(clippy::too_many_arguments)]
fn copy_tex(
    encoder: &mut wgpu::CommandEncoder,
    src: &wgpu::Texture,
    dst: &wgpu::Texture,
    width: u32,
    height: u32,
) {
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: src,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: dst,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

fn clear_tex_transparent(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear-transparent"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
}

#[allow(clippy::too_many_arguments)]
fn draw_raw_prepared_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    overlay_slots: &HashMap<([u8; 32], u32, u32), OverlaySlot>,
    layer_blitter: &crate::blit::Blitter,
    video: &mut crate::video::VideoLayer,
    video_frames: &HashMap<String, (u64, Arc<crate::video::Nv12Frame>)>,
    blur: &mut crate::blur::BlurPipeline,
    video_draw_cursor: &mut u32,
    width: u32,
    height: u32,
    layer: PreparedLayer<'_>,
    dst_tex: &wgpu::Texture,
    dst_view: &wgpu::TextureView,
    dst_already_has_backdrop: bool,
) {
    match layer {
        PreparedLayer::Overlay { key, .. } => {
            if let Some(slot) = overlay_slots.get(&key) {
                if dst_already_has_backdrop {
                    layer_blitter.draw(device, encoder, &slot.view, dst_view);
                } else {
                    copy_tex(encoder, &slot.texture, dst_tex, width, height);
                }
            } else if !dst_already_has_backdrop {
                clear_tex_transparent(encoder, dst_view);
            }
        }
        PreparedLayer::Video {
            hash,
            bg_key,
            fit,
            blur_radius,
            opacity,
            ..
        } => {
            let mut has_bg = false;
            if let Some(bk) = bg_key
                && let Some(slot) = overlay_slots.get(&bk)
            {
                if dst_already_has_backdrop {
                    layer_blitter.draw(device, encoder, &slot.view, dst_view);
                } else {
                    copy_tex(encoder, &slot.texture, dst_tex, width, height);
                }
                has_bg = true;
            }
            if let Some((_, nv12)) = video_frames.get(hash) {
                let dslot = *video_draw_cursor;
                *video_draw_cursor = video_draw_cursor.saturating_add(1);
                if blur_radius > 0.05 && blur_radius.is_finite() {
                    let _ = blur.ensure_source_view(device, width, height, blur_radius);
                    if let Some(blur_src) = blur.targets_src_view() {
                        video.draw_layer(
                            device, queue, encoder, dslot, hash, nv12, width, height, fit, 1.0,
                            true, blur_src,
                        );
                    }
                    if dst_already_has_backdrop || has_bg {
                        blur.blur_and_blend(device, queue, encoder, dst_view, blur_radius, opacity);
                    } else {
                        clear_tex_transparent(encoder, dst_view);
                        blur.blur_and_blend(device, queue, encoder, dst_view, blur_radius, opacity);
                    }
                } else {
                    let clear_first = !dst_already_has_backdrop && !has_bg;
                    video.draw_layer(
                        device,
                        queue,
                        encoder,
                        dslot,
                        hash,
                        nv12,
                        width,
                        height,
                        fit,
                        opacity,
                        clear_first,
                        dst_view,
                    );
                }
            } else if !dst_already_has_backdrop && !has_bg {
                clear_tex_transparent(encoder, dst_view);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_layer_with_effects(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    overlay_slots: &HashMap<([u8; 32], u32, u32), OverlaySlot>,
    layer_blitter: &crate::blit::Blitter,
    video: &mut crate::video::VideoLayer,
    video_frames: &HashMap<String, (u64, Arc<crate::video::Nv12Frame>)>,
    blur: &mut crate::blur::BlurPipeline,
    effects_pipe: &mut crate::effects::EffectsPipeline,
    scratch: Option<&ScratchSet>,
    video_draw_cursor: &mut u32,
    width: u32,
    height: u32,
    layer: PreparedLayer<'_>,
    out_tex: &wgpu::Texture,
    out_view: &wgpu::TextureView,
    out_is_canvas: bool,
    backdrop_canvas: Option<(&wgpu::Texture, &wgpu::TextureView)>,
    bg_blur_scale: f32,
) {
    let effs = match layer {
        PreparedLayer::Overlay { effects, .. } | PreparedLayer::Video { effects, .. } => effects,
    };
    if effs.is_empty() {
        draw_raw_prepared_layer(
            device,
            queue,
            encoder,
            overlay_slots,
            layer_blitter,
            video,
            video_frames,
            blur,
            video_draw_cursor,
            width,
            height,
            layer,
            out_tex,
            out_view,
            out_is_canvas,
        );
        return;
    }

    let sc = scratch.expect("scratch textures ensured for effects");
    draw_raw_prepared_layer(
        device,
        queue,
        encoder,
        overlay_slots,
        layer_blitter,
        video,
        video_frames,
        blur,
        video_draw_cursor,
        width,
        height,
        layer,
        &sc.ping_a.texture,
        &sc.ping_a.view,
        false,
    );

    let mut cur_in_a = true;
    for stage in &effs.stages {
        let (cur_view, next_view) = if cur_in_a {
            (&sc.ping_a.view, &sc.ping_b.view)
        } else {
            (&sc.ping_b.view, &sc.ping_a.view)
        };
        if let crate::effects::ResolvedEffect::BackgroundBlur {
            mix,
            sigma,
            region,
            tint_rgba,
        } = stage
        {
            if let Some((canvas_tex, canvas_view)) = backdrop_canvas
                && bg_blur_scale > 0.0
            {
                copy_tex(encoder, canvas_tex, &sc.backdrop.texture, width, height);
                let scaled = crate::effects::ResolvedEffect::BackgroundBlur {
                    mix: mix * bg_blur_scale.clamp(0.0, 1.0),
                    sigma: *sigma,
                    region: *region,
                    tint_rgba: *tint_rgba,
                };
                effects_pipe.draw_background_blur(
                    device,
                    queue,
                    encoder,
                    blur,
                    &sc.backdrop.view,
                    &sc.bg_blur_out.view,
                    cur_view,
                    canvas_view,
                    width,
                    height,
                    &scaled,
                );
            }
        } else {
            effects_pipe.draw_stage(
                device, queue, encoder, blur, cur_view, next_view, width, height, stage, false,
            );
            cur_in_a = !cur_in_a;
        }
    }

    let final_pair = if cur_in_a { &sc.ping_a } else { &sc.ping_b };
    if out_is_canvas {
        layer_blitter.draw(device, encoder, &final_pair.view, out_view);
    } else {
        copy_tex(encoder, &final_pair.texture, out_tex, width, height);
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_prepared_scene(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    pattern_pipeline: &wgpu::RenderPipeline,
    pattern_pool: &[(wgpu::Buffer, wgpu::BindGroup)],
    overlay_slots: &HashMap<([u8; 32], u32, u32), OverlaySlot>,
    layer_blitter: &crate::blit::Blitter,
    video: &mut crate::video::VideoLayer,
    video_frames: &HashMap<String, (u64, Arc<crate::video::Nv12Frame>)>,
    blur: &mut crate::blur::BlurPipeline,
    effects_pipe: &mut crate::effects::EffectsPipeline,
    transitions_pipe: &mut crate::transitions::TransitionPipeline,
    scratch: Option<&ScratchSet>,
    video_draw_cursor: &mut u32,
    width: u32,
    height: u32,
    prepared: &PreparedScene,
    dst_tex: &wgpu::Texture,
    dst_view: &wgpu::TextureView,
) {
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("layers"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(pattern_pipeline);
        pass.set_bind_group(0, &pattern_pool[prepared.pattern_slot].1, &[]);
        pass.draw(0..3, 0..1);
    }

    if prepared.is_legacy {
        if prepared.has_legacy_video {
            video.draw(encoder, dst_view);
        }
        if let Some(lk) = prepared.legacy_text_key
            && let Some(slot) = overlay_slots.get(&lk)
        {
            layer_blitter.draw(device, encoder, &slot.view, dst_view);
        }
        return;
    }

    for op in &prepared.ops {
        match op {
            PreparedDrawOp::Direct(entry) => {
                draw_layer_with_effects(
                    device,
                    queue,
                    encoder,
                    overlay_slots,
                    layer_blitter,
                    video,
                    video_frames,
                    blur,
                    effects_pipe,
                    scratch,
                    video_draw_cursor,
                    width,
                    height,
                    entry.as_ref(),
                    dst_tex,
                    dst_view,
                    true,
                    Some((dst_tex, dst_view)),
                    1.0,
                );
            }
            PreparedDrawOp::Transition {
                from,
                to,
                kind,
                direction,
                progress,
            } => {
                let sc = scratch.expect("scratch textures ensured for transition");
                if let Some(from_entry) = from {
                    let bg_scale = if to.is_none() { 1.0 - *progress } else { 0.0 };
                    let bg_canvas = (bg_scale > 0.0).then_some((dst_tex, dst_view));
                    draw_layer_with_effects(
                        device,
                        queue,
                        encoder,
                        overlay_slots,
                        layer_blitter,
                        video,
                        video_frames,
                        blur,
                        effects_pipe,
                        scratch,
                        video_draw_cursor,
                        width,
                        height,
                        from_entry.as_ref(),
                        &sc.layer_tr_a.texture,
                        &sc.layer_tr_a.view,
                        false,
                        bg_canvas,
                        bg_scale,
                    );
                } else {
                    clear_tex_transparent(encoder, &sc.layer_tr_a.view);
                }

                if let Some(to_entry) = to {
                    let bg_canvas = (!to_entry.effects().is_empty()).then_some((dst_tex, dst_view));
                    draw_layer_with_effects(
                        device,
                        queue,
                        encoder,
                        overlay_slots,
                        layer_blitter,
                        video,
                        video_frames,
                        blur,
                        effects_pipe,
                        scratch,
                        video_draw_cursor,
                        width,
                        height,
                        to_entry.as_ref(),
                        &sc.layer_tr_b.texture,
                        &sc.layer_tr_b.view,
                        false,
                        bg_canvas,
                        *progress,
                    );
                } else {
                    clear_tex_transparent(encoder, &sc.layer_tr_b.view);
                }

                transitions_pipe.draw(
                    device,
                    queue,
                    encoder,
                    &sc.layer_tr_a.view,
                    &sc.layer_tr_b.view,
                    &sc.layer_tr_out.view,
                    *kind,
                    *direction,
                    *progress,
                    true,
                    false,
                );
                layer_blitter.draw(device, encoder, &sc.layer_tr_out.view, dst_view);
            }
        }
    }

    if !prepared.cleared && !prepared.output_effects.is_empty() {
        let sc = scratch.expect("scratch textures ensured for output effects");
        copy_tex(encoder, dst_tex, &sc.ping_a.texture, width, height);
        let mut cur_in_a = true;
        for stage in &prepared.output_effects.stages {
            let (cur_view, next_view) = if cur_in_a {
                (&sc.ping_a.view, &sc.ping_b.view)
            } else {
                (&sc.ping_b.view, &sc.ping_a.view)
            };
            effects_pipe.draw_stage(
                device, queue, encoder, blur, cur_view, next_view, width, height, stage, true,
            );
            cur_in_a = !cur_in_a;
        }
        let final_pair = if cur_in_a { &sc.ping_a } else { &sc.ping_b };
        copy_tex(encoder, &final_pair.texture, dst_tex, width, height);
    }
}

impl Compositor for GpuCompositor {
    fn name(&self) -> &str {
        &self.name
    }

    fn as_gpu(&mut self) -> Option<&mut GpuCompositor> {
        Some(self)
    }

    fn last_rendered(&self) -> Option<Instant> {
        self.timing.rendered
    }

    fn last_submitted(&self) -> Option<Instant> {
        self.timing.submitted
    }

    fn prerasterise_hints(&mut self, hints: &[PreloadHint]) -> Result<usize> {
        crate::scene::prerasterise_hints(hints, &mut self.text, &mut self.overlays)
    }

    fn cache_stats(&self) -> (u64, u64) {
        let (th, tm) = self
            .text
            .as_ref()
            .map_or((0, 0), |t| (t.cache_hits(), t.cache_misses()));
        (th + self.overlays.hits(), tm + self.overlays.misses())
    }

    fn set_video(&mut self, frame: Option<crate::video::Nv12Frame>) -> Result<()> {
        if let Some(frame) = frame.as_ref() {
            frame.validate()?;
        }
        self.video_frame = frame;
        Ok(())
    }

    fn set_media_root(&mut self, root: Option<PathBuf>) {
        self.media_root = root;
        self.overlays.clear();
        self.overlay_slots.clear();
        self.overlay_order.clear();
    }

    fn set_video_for_hash(
        &mut self,
        hash: &str,
        frame: Option<crate::video::Nv12Frame>,
    ) -> Result<()> {
        match frame {
            Some(frame) => {
                frame.validate()?;
                self.video_seq = self.video_seq.saturating_add(1);
                self.video_frames
                    .insert(hash.to_owned(), (self.video_seq, Arc::new(frame)));
            }
            None => {
                self.video_frames.remove(hash);
                self.video.remove_key(hash);
            }
        }
        Ok(())
    }

    fn set_clock_unix_ms(&mut self, unix_ms: Option<i64>) {
        self.clock_unix_ms = unix_ms;
    }

    fn render(&mut self, scene: &Scene, width: u32, height: u32) -> Result<Frame> {
        self.render_output(scene, 0, width, height)
    }

    fn render_output(
        &mut self,
        scene: &Scene,
        output_index: u32,
        width: u32,
        height: u32,
    ) -> Result<Frame> {
        self.composite_output(scene, output_index, width, height)?;
        self.read_back_output(output_index, width, height)
    }

    fn render_output_with_plan(
        &mut self,
        scene: &Scene,
        output_index: u32,
        width: u32,
        height: u32,
        plan: &crate::transitions::RenderTransitionPlan,
    ) -> Result<Frame> {
        self.composite_output_with_plan(scene, output_index, width, height, plan)?;
        self.read_back_output(output_index, width, height)
    }
}

impl GpuCompositor {
    /// Draw the layer stack for output 0 into its output texture without reading the frame back.
    pub fn composite(&mut self, scene: &Scene, width: u32, height: u32) -> Result<()> {
        self.composite_output(scene, 0, width, height)
    }

    /// Draw the normalized layer stack for `output_index` into its offscreen output texture.
    pub fn composite_output(
        &mut self,
        scene: &Scene,
        output_index: u32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let empty = crate::transitions::RenderTransitionPlan::default();
        self.composite_output_with_plan(scene, output_index, width, height, &empty)
    }

    /// Draw `output_index` with optional active output/layer transitions into its output texture.
    pub fn composite_output_with_plan(
        &mut self,
        scene: &Scene,
        output_index: u32,
        width: u32,
        height: u32,
        plan: &crate::transitions::RenderTransitionPlan,
    ) -> Result<()> {
        self.pattern_cursor = 0;
        self.blur.begin_frame();
        self.effects.begin_frame();
        self.transitions.begin_frame();

        let prepared_to =
            self.prepare_scene(scene, output_index, width, height, &plan.layer_transitions)?;
        let prepared_from =
            if let Some((from_scene, kind, dir, p)) = plan.output_transition.as_ref() {
                Some((
                    self.prepare_scene(from_scene, output_index, width, height, &[])?,
                    *kind,
                    *dir,
                    *p,
                ))
            } else {
                None
            };

        let needs_scratch = prepared_from.is_some()
            || !prepared_to.output_effects.is_empty()
            || prepared_to.ops.iter().any(|op| match op {
                PreparedDrawOp::Direct(e) => !e.effects().is_empty(),
                PreparedDrawOp::Transition { .. } => true,
            });

        self.ensure_target(output_index, width, height);
        if needs_scratch {
            self.ensure_scratch(width, height);
        }

        let target = self
            .targets
            .get(&output_index)
            .expect("target exists for output");
        let scratch_ref = self.scratches.get(&(width, height));
        let mut video_draw_cursor = 0_u32;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        if let Some((prepared_from, kind, dir, p)) = prepared_from {
            let sc = scratch_ref.expect("scratch ensured for output transition");
            encode_prepared_scene(
                &self.device,
                &self.queue,
                &mut encoder,
                &self.pipeline,
                &self.pattern_pool,
                &self.overlay_slots,
                &self.layer_blitter,
                &mut self.video,
                &self.video_frames,
                &mut self.blur,
                &mut self.effects,
                &mut self.transitions,
                scratch_ref,
                &mut video_draw_cursor,
                width,
                height,
                &prepared_from,
                &sc.out_tr_a.texture,
                &sc.out_tr_a.view,
            );
            encode_prepared_scene(
                &self.device,
                &self.queue,
                &mut encoder,
                &self.pipeline,
                &self.pattern_pool,
                &self.overlay_slots,
                &self.layer_blitter,
                &mut self.video,
                &self.video_frames,
                &mut self.blur,
                &mut self.effects,
                &mut self.transitions,
                scratch_ref,
                &mut video_draw_cursor,
                width,
                height,
                &prepared_to,
                &sc.out_tr_b.texture,
                &sc.out_tr_b.view,
            );
            self.transitions.draw(
                &self.device,
                &self.queue,
                &mut encoder,
                &sc.out_tr_a.view,
                &sc.out_tr_b.view,
                &target.view,
                kind,
                dir,
                p,
                false,
                false,
            );
        } else {
            encode_prepared_scene(
                &self.device,
                &self.queue,
                &mut encoder,
                &self.pipeline,
                &self.pattern_pool,
                &self.overlay_slots,
                &self.layer_blitter,
                &mut self.video,
                &self.video_frames,
                &mut self.blur,
                &mut self.effects,
                &mut self.transitions,
                scratch_ref,
                &mut video_draw_cursor,
                width,
                height,
                &prepared_to,
                &target.texture,
                &target.view,
            );
        }

        self.queue.submit([encoder.finish()]);
        self.timing.submitted = Some(Instant::now());
        self.wait()?;
        self.timing.rendered = Some(Instant::now());
        Ok(())
    }

    /// Composite multiple outputs into their offscreen textures in a single GPU submission.
    pub fn composite_outputs(&mut self, scene: &Scene, outputs: &[(u32, u32, u32)]) -> Result<()> {
        if outputs.is_empty() {
            return Ok(());
        }
        self.pattern_cursor = 0;
        self.blur.begin_frame();
        self.effects.begin_frame();
        self.transitions.begin_frame();

        let mut prepared = Vec::with_capacity(outputs.len());
        let mut scratch_sizes = Vec::new();
        for &(output_index, width, height) in outputs {
            let p = self.prepare_scene(scene, output_index, width, height, &[])?;
            let needs_scratch = !p.output_effects.is_empty()
                || p.ops.iter().any(|op| match op {
                    PreparedDrawOp::Direct(e) => !e.effects().is_empty(),
                    PreparedDrawOp::Transition { .. } => true,
                });
            self.ensure_target(output_index, width, height);
            if needs_scratch && !scratch_sizes.contains(&(width, height)) {
                scratch_sizes.push((width, height));
            }
            prepared.push((output_index, width, height, p));
        }
        for (sw, sh) in scratch_sizes {
            self.ensure_scratch(sw, sh);
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("multi-output-frame"),
            });
        let mut video_draw_cursor = 0_u32;

        for (output_index, width, height, p) in &prepared {
            let target = self
                .targets
                .get(output_index)
                .expect("target exists for output");
            let scratch_ref = self.scratches.get(&(*width, *height));
            encode_prepared_scene(
                &self.device,
                &self.queue,
                &mut encoder,
                &self.pipeline,
                &self.pattern_pool,
                &self.overlay_slots,
                &self.layer_blitter,
                &mut self.video,
                &self.video_frames,
                &mut self.blur,
                &mut self.effects,
                &mut self.transitions,
                scratch_ref,
                &mut video_draw_cursor,
                *width,
                *height,
                p,
                &target.texture,
                &target.view,
            );
        }

        self.queue.submit([encoder.finish()]);
        self.timing.submitted = Some(Instant::now());
        self.wait()?;
        self.timing.rendered = Some(Instant::now());
        Ok(())
    }

    /// Copy the output texture for `output_index` to CPU memory.
    fn read_back_output(&mut self, output_index: u32, width: u32, height: u32) -> Result<Frame> {
        let target = self
            .targets
            .get(&output_index)
            .context("nothing has been composited for this output")?;
        let mut copy = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("readback"),
            });
        copy.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &target.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(target.padded_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([copy.finish()]);
        let slice = target.readback.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.wait()?;
        receiver
            .recv()
            .context("readback callback")?
            .map_err(|error| anyhow!("readback failed: {error}"))?;
        let mut frame = Frame::new(width, height);
        {
            let data = slice
                .get_mapped_range()
                .map_err(|error| anyhow!("readback mapping failed: {error}"))?;
            let row = width as usize * 4;
            for y in 0..height as usize {
                let from = y * target.padded_row as usize;
                frame.pixels[y * row..(y + 1) * row].copy_from_slice(&data[from..from + row]);
            }
        }
        target.readback.unmap();
        Ok(frame)
    }
}
