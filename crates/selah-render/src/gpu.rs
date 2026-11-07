//! The wgpu compositor (ADR-005): D3D12 (WARP when there's no GPU) on Windows, Metal on
//! macOS, Vulkan (lavapipe in the cloud and CI) on Linux.
//!
//! M1 draws the layer stack's first layer, a test pattern, into an offscreen `Rgba8Unorm`
//! target. Text (M1.6) and video (M1.7) layers composite on top of it in later steps.
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use selah_proto::v1::{Scene, TestPattern};

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
    target: Option<Target>,
    name: String,
    pub timing: FrameTiming,
    text: Option<crate::text::TextRenderer>,
    /// What the layer texture holds now (text, width, height), so unchanged text isn't
    /// uploaded again every frame (video plays under still text).
    layer_text: Option<(String, u32, u32)>,
    layer: Option<(u32, u32, wgpu::Texture, wgpu::TextureView)>,
    layer_blitter: crate::blit::Blitter,
    video: crate::video::VideoLayer,
    video_frame: Option<crate::video::Nv12Frame>,
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
            target: None,
            name,
            timing: FrameTiming::default(),
            text: None,
            layer_text: None,
            layer: None,
            layer_blitter,
            video,
            video_frame: None,
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

    /// The composited output of the last render, for presenting to windows.
    pub fn output_view(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|target| &target.view)
    }

    fn target(&mut self, width: u32, height: u32) -> &Target {
        let stale = self
            .target
            .as_ref()
            .is_none_or(|t| t.width != width || t.height != height);
        if stale {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("output-0"),
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
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("output-0-readback"),
                size: u64::from(padded_row) * u64::from(height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            self.target = Some(Target {
                width,
                height,
                texture,
                view,
                readback,
                padded_row,
            });
        }
        self.target.as_ref().expect("target was just created")
    }

    /// Upload the text layer for `scene` (from the paragraph cache) into the layer texture.
    fn text_layer(&mut self, scene: &Scene, width: u32, height: u32) -> Result<bool> {
        if scene.cleared || scene.text.is_empty() {
            return Ok(false);
        }
        if self
            .layer_text
            .as_ref()
            .is_some_and(|(text, w, h)| *text == scene.text && *w == width && *h == height)
        {
            return Ok(true);
        }
        if self.text.is_none() {
            self.text = Some(crate::text::TextRenderer::bundled()?);
        }
        let style = crate::text::SlideTextStyle::for_output(height);
        let frame = self.text.as_mut().expect("text renderer exists").render(
            &scene.text,
            &style,
            width,
            height,
        )?;
        if self
            .layer
            .as_ref()
            .is_none_or(|(w, h, _, _)| *w != width || *h != height)
        {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("text-layer"),
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
            self.layer = Some((width, height, texture, view));
        }
        let (_, _, texture, _) = self.layer.as_ref().expect("layer exists");
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
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
        self.layer_text = Some((scene.text.clone(), width, height));
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

    fn set_video(&mut self, frame: Option<crate::video::Nv12Frame>) -> Result<()> {
        if let Some(frame) = frame.as_ref() {
            frame.validate()?;
        }
        self.video_frame = frame;
        Ok(())
    }

    fn render(&mut self, scene: &Scene, width: u32, height: u32) -> Result<Frame> {
        self.composite(scene, width, height)?;
        self.read_back(width, height)
    }
}

impl GpuCompositor {
    /// Draw the layer stack into the output texture and wait for the GPU, without reading the
    /// frame back (what windows present, and what the video benchmark times).
    pub fn composite(&mut self, scene: &Scene, width: u32, height: u32) -> Result<()> {
        let pattern = TestPattern::try_from(scene.test_pattern).unwrap_or(TestPattern::Unspecified);
        let words = [pattern as u32, u32::from(scene.cleared), width, height];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        self.queue.write_buffer(&self.params, 0, &bytes);
        let has_text = self.text_layer(scene, width, height)?;
        let has_video = match self.video_frame.as_ref() {
            Some(video) if !scene.cleared => {
                self.video
                    .upload(&self.device, &self.queue, video, width, height)?;
                true
            }
            _ => false,
        };
        self.target(width, height);
        let target = self.target.as_ref().expect("target exists");

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
        if has_video {
            self.video.draw(&mut encoder, &target.view);
        }
        if has_text {
            let (_, _, _, layer) = self.layer.as_ref().expect("layer exists");
            self.layer_blitter
                .draw(&self.device, &mut encoder, layer, &target.view);
        }
        self.queue.submit([encoder.finish()]);
        self.timing.submitted = Some(Instant::now());
        self.wait()?;
        self.timing.rendered = Some(Instant::now());
        Ok(())
    }

    /// Copy the output texture to memory, for the checksum and the preview ring.
    fn read_back(&mut self, width: u32, height: u32) -> Result<Frame> {
        let target = self
            .target
            .as_ref()
            .context("nothing has been composited")?;
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
