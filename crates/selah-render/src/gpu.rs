//! The wgpu compositor (ADR-005): D3D12 (WARP when there's no GPU) on Windows, Metal on
//! macOS, Vulkan (lavapipe in the cloud and CI) on Linux.
//!
//! M1 draws the layer stack's first layer, a test pattern, into an offscreen `Rgba8Unorm`
//! target. Text (M1.6) and video (M1.7) layers composite on top of it in later steps.
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use selah_proto::v1::{LayerKind, MediaFitMode, PreloadHint, Scene, layer_spec};

use crate::{Compositor, Frame};

const SHADER: &str = include_str!("patterns.wgsl");
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

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
    width: u32,
    height: u32,
    key: [u8; 32],
    texture: wgpu::Texture,
    view: wgpu::TextureView,
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
    params: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    targets: BTreeMap<u32, Target>,
    name: String,
    pub timing: FrameTiming,
    text: Option<crate::text::TextRenderer>,
    overlays: crate::scene::OverlayCache,
    overlay_slots: HashMap<(u32, u8), OverlaySlot>,
    layer_blitter: crate::blit::Blitter,
    video: crate::video::VideoLayer,
    video_frame: Option<crate::video::Nv12Frame>,
    video_frames: HashMap<String, (u64, crate::video::Nv12Frame)>,
    video_seq: u64,
    blur: crate::blur::BlurPipeline,
    media_root: Option<PathBuf>,
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
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pattern-params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pattern-params"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            }],
        });
        let layer_blitter = crate::blit::Blitter::new_layer(&device, TARGET_FORMAT);
        let video = crate::video::VideoLayer::new(&device, TARGET_FORMAT);
        let blur = crate::blur::BlurPipeline::new(&device, TARGET_FORMAT);
        let name = format!("wgpu {:?} {}", info.backend, info.name);
        tracing::info!(adapter = %info.name, backend = ?info.backend, device_type = ?info.device_type, "GPU compositor ready");
        Ok(Self {
            instance,
            adapter_handle: adapter,
            device,
            queue,
            adapter: info,
            pipeline,
            params,
            bind_group,
            targets: BTreeMap::new(),
            name,
            timing: FrameTiming::default(),
            text: None,
            overlays: crate::scene::OverlayCache::default(),
            overlay_slots: HashMap::new(),
            layer_blitter,
            video,
            video_frame: None,
            video_frames: HashMap::new(),
            video_seq: 0,
            blur,
            media_root: crate::media_bg::default_media_root_from_env(),
        })
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter
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

    fn upload_overlay_slot(
        &mut self,
        output_index: u32,
        slot_idx: u8,
        key: [u8; 32],
        frame: &Frame,
    ) {
        let (width, height) = (frame.width, frame.height);
        let slot_key = (output_index, slot_idx);
        let unchanged = self
            .overlay_slots
            .get(&slot_key)
            .is_some_and(|s| s.key == key && s.width == width && s.height == height);
        if unchanged {
            return;
        }
        let recreate = self
            .overlay_slots
            .get(&slot_key)
            .is_none_or(|s| s.width != width || s.height != height);
        if recreate {
            let label = format!("layer-{output_index}-{slot_idx}");
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
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.overlay_slots.insert(
                slot_key,
                OverlaySlot {
                    width,
                    height,
                    key,
                    texture,
                    view,
                },
            );
        }
        let slot = self.overlay_slots.get_mut(&slot_key).expect("slot exists");
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &slot.texture,
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
        slot.key = key;
    }

    /// Upload the legacy text layer for `scene` (from the paragraph cache) into overlay slot 255.
    fn upload_legacy_text_layer(
        &mut self,
        scene: &Scene,
        output_index: u32,
        width: u32,
        height: u32,
    ) -> Result<bool> {
        if scene.cleared || scene.text.is_empty() {
            return Ok(false);
        }
        let style = crate::text::SlideTextStyle::for_output(height);
        let mut key_hash = blake3::Hasher::new();
        key_hash.update(scene.text.as_bytes());
        key_hash.update(&width.to_le_bytes());
        key_hash.update(&height.to_le_bytes());
        let key = *key_hash.finalize().as_bytes();
        let unchanged = self
            .overlay_slots
            .get(&(output_index, 255))
            .is_some_and(|s| s.key == key && s.width == width && s.height == height);
        if unchanged {
            return Ok(true);
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
        self.upload_overlay_slot(output_index, 255, key, &frame);
        Ok(true)
    }

    fn wait(&self) -> Result<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|error| anyhow!("GPU poll failed: {error}"))
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
                    .insert(hash.to_owned(), (self.video_seq, frame));
            }
            None => {
                self.video_frames.remove(hash);
                self.video.remove_key(hash);
            }
        }
        Ok(())
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
}

enum GpuDrawOp {
    OverlaySlot(u8),
    Video {
        hash: String,
        fit: MediaFitMode,
        blur_radius: f32,
        opacity: f32,
    },
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
        let normalized = crate::scene::normalize_output_scene(scene, output_index, width, height);
        let words = [
            normalized.test_pattern as u32,
            u32::from(normalized.cleared),
            width,
            height,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        self.queue.write_buffer(&self.params, 0, &bytes);

        let mut draw_ops: Vec<GpuDrawOp> = Vec::new();
        let has_legacy_video = if normalized.is_legacy {
            if self.upload_legacy_text_layer(scene, output_index, width, height)? {
                draw_ops.push(GpuDrawOp::OverlaySlot(255));
            }
            match self.video_frame.as_ref() {
                Some(video) if !normalized.cleared => {
                    self.video
                        .upload(&self.device, &self.queue, video, width, height)?;
                    true
                }
                _ => false,
            }
        } else {
            if !normalized.cleared {
                let mut next_slot = 0_u8;
                let mut alloc_slot = || -> u8 {
                    let s = next_slot.min(254);
                    next_slot = next_slot.saturating_add(1);
                    s
                };
                for layer in &normalized.layers {
                    if layer.kind == LayerKind::Media
                        && let Some(layer_spec::Content::Media(m)) = layer.spec.content.as_ref()
                        && m.media_kind == "video"
                        && let Some((seq, nv12)) = self.video_frames.get(&m.content_hash).cloned()
                    {
                        if let Some(mut bg) = crate::scene::media_backdrop_frame(
                            m,
                            width,
                            height,
                            self.media_root.as_deref(),
                            false,
                        )? {
                            crate::scene::scale_frame_opacity(&mut bg, layer.opacity);
                            let mut key_h = blake3::Hasher::new();
                            key_h.update(&layer.cache_key);
                            key_h.update(b"video-bg");
                            let bg_key = *key_h.finalize().as_bytes();
                            let slot = alloc_slot();
                            self.upload_overlay_slot(output_index, slot, bg_key, &bg);
                            draw_ops.push(GpuDrawOp::OverlaySlot(slot));
                        }
                        self.video.upload_keyed(
                            &self.device,
                            &self.queue,
                            &m.content_hash,
                            seq,
                            &nv12,
                        )?;
                        let fit = MediaFitMode::try_from(m.fit).unwrap_or(MediaFitMode::Fill);
                        if m.blur_radius > 0.05 && m.blur_radius.is_finite() {
                            self.blur.ensure_source_view(
                                &self.device,
                                width,
                                height,
                                m.blur_radius,
                            );
                        }
                        draw_ops.push(GpuDrawOp::Video {
                            hash: m.content_hash.clone(),
                            fit,
                            blur_radius: m.blur_radius,
                            opacity: layer.opacity,
                        });
                        continue;
                    }
                    if let Some(overlay) = crate::scene::rasterise_layer_overlay(
                        layer,
                        width,
                        height,
                        &mut self.text,
                        &mut self.overlays,
                        self.media_root.as_deref(),
                    )? {
                        let slot = alloc_slot();
                        self.upload_overlay_slot(output_index, slot, layer.cache_key, &overlay);
                        draw_ops.push(GpuDrawOp::OverlaySlot(slot));
                    }
                }
            }
            false
        };

        self.ensure_target(output_index, width, height);
        let target = self
            .targets
            .get(&output_index)
            .expect("target exists for output");

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layers"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if has_legacy_video {
            self.video.draw(&mut encoder, &target.view);
        }
        for (draw_slot, op) in draw_ops.into_iter().enumerate() {
            match op {
                GpuDrawOp::OverlaySlot(slot_idx) => {
                    if let Some(slot) = self.overlay_slots.get(&(output_index, slot_idx)) {
                        self.layer_blitter.draw(
                            &self.device,
                            &mut encoder,
                            &slot.view,
                            &target.view,
                        );
                    }
                }
                GpuDrawOp::Video {
                    hash,
                    fit,
                    blur_radius,
                    opacity,
                } => {
                    if let Some((_, nv12)) = self.video_frames.get(&hash) {
                        if blur_radius > 0.05 && blur_radius.is_finite() {
                            if let Some(blur_src) = self.blur.targets_src_view() {
                                self.video.draw_layer(
                                    &self.device,
                                    &self.queue,
                                    &mut encoder,
                                    draw_slot as u32,
                                    &hash,
                                    nv12,
                                    width,
                                    height,
                                    fit,
                                    1.0,
                                    true,
                                    blur_src,
                                );
                                self.blur.blur_and_blend(
                                    &self.device,
                                    &mut encoder,
                                    &target.view,
                                    blur_radius,
                                    opacity,
                                );
                            }
                        } else {
                            self.video.draw_layer(
                                &self.device,
                                &self.queue,
                                &mut encoder,
                                draw_slot as u32,
                                &hash,
                                nv12,
                                width,
                                height,
                                fit,
                                opacity,
                                false,
                                &target.view,
                            );
                        }
                    }
                }
            }
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
