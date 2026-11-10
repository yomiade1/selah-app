//! The video layer (M1.7 / M3.4): decoded NV12 frames are uploaded as two textures (luma `R8`,
//! chroma `Rg8`) and converted to RGB in `video.wgsl`, under the slide layer.
//!
//! The decoder lives in `selah-media`; this module only knows the frame layout, so the render
//! crate doesn't depend on FFmpeg.
use std::collections::HashMap;

use selah_proto::v1::MediaFitMode;

use crate::Frame;
use crate::media_bg::compute_fit_rect;

const SHADER: &str = include_str!("video.wgsl");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YuvMatrix {
    Bt601,
    Bt709,
}

/// One NV12 frame: a full-size luma plane and a half-size interleaved UV plane, tightly packed.
#[derive(Clone, Debug)]
pub struct Nv12Frame {
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub uv: Vec<u8>,
    pub matrix: YuvMatrix,
    pub full_range: bool,
}

impl Nv12Frame {
    pub fn chroma_width(&self) -> u32 {
        self.width.div_ceil(2)
    }

    pub fn chroma_height(&self) -> u32 {
        self.height.div_ceil(2)
    }

    /// Plane sizes match the dimensions (frames come from outside the render crate).
    pub fn validate(&self) -> anyhow::Result<()> {
        let luma = self.width as usize * self.height as usize;
        let chroma = self.chroma_width() as usize * 2 * self.chroma_height() as usize;
        anyhow::ensure!(
            self.width > 0 && self.height > 0 && self.y.len() == luma && self.uv.len() == chroma,
            "NV12 planes don't match {}×{}",
            self.width,
            self.height
        );
        Ok(())
    }

    /// Convert an RGBA8 [`Frame`] into a BT.709 limited-range NV12 frame (e.g. a cached video
    /// poster frame for offscreen scene thumbnails).
    pub fn from_rgba(frame: &Frame) -> anyhow::Result<Self> {
        let expected_len = frame
            .width
            .checked_mul(frame.height)
            .and_then(|px| px.checked_mul(4))
            .ok_or_else(|| anyhow::anyhow!("frame dimensions overflow"))?
            as usize;
        anyhow::ensure!(
            frame.width > 0 && frame.height > 0 && frame.pixels.len() == expected_len,
            "invalid RGBA frame buffer for NV12 conversion ({}×{})",
            frame.width,
            frame.height
        );
        let w = frame.width as usize;
        let h = frame.height as usize;
        let cw = frame.width.div_ceil(2) as usize;
        let ch = frame.height.div_ceil(2) as usize;
        let mut y = vec![16_u8; w * h];
        let mut uv = vec![128_u8; cw * ch * 2];

        for py in 0..h {
            for px in 0..w {
                let idx = (py * w + px) * 4;
                let r = f32::from(frame.pixels[idx]) / 255.0;
                let g = f32::from(frame.pixels[idx + 1]) / 255.0;
                let b = f32::from(frame.pixels[idx + 2]) / 255.0;
                let y_norm = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                y[py * w + px] = (16.0 + 219.0 * y_norm).round().clamp(16.0, 235.0) as u8;
            }
        }

        for cy in 0..ch {
            let y0 = cy * 2;
            let y1 = (y0 + 1).min(h - 1);
            for cx in 0..cw {
                let x0 = cx * 2;
                let x1 = (x0 + 1).min(w - 1);
                let mut sum_r = 0.0_f32;
                let mut sum_g = 0.0_f32;
                let mut sum_b = 0.0_f32;
                for py in [y0, y1] {
                    for px in [x0, x1] {
                        let idx = (py * w + px) * 4;
                        sum_r += f32::from(frame.pixels[idx]) / 255.0;
                        sum_g += f32::from(frame.pixels[idx + 1]) / 255.0;
                        sum_b += f32::from(frame.pixels[idx + 2]) / 255.0;
                    }
                }
                let r = sum_r * 0.25;
                let g = sum_g * 0.25;
                let b = sum_b * 0.25;
                let y_norm = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                let cb_norm = (b - y_norm) / 1.8556;
                let cr_norm = (r - y_norm) / 1.5748;
                let off = (cy * cw + cx) * 2;
                uv[off] = (128.0 + 224.0 * cb_norm).round().clamp(16.0, 240.0) as u8;
                uv[off + 1] = (128.0 + 224.0 * cr_norm).round().clamp(16.0, 240.0) as u8;
            }
        }

        let out = Self {
            width: frame.width,
            height: frame.height,
            y,
            uv,
            matrix: YuvMatrix::Bt709,
            full_range: false,
        };
        out.validate()?;
        Ok(out)
    }
}

fn yuv_to_rgb_f32(y8: f32, u8_: f32, v8: f32, matrix: YuvMatrix, full_range: bool) -> [f32; 3] {
    let (y, cb, cr) = if full_range {
        (y8 / 255.0, (u8_ - 128.0) / 255.0, (v8 - 128.0) / 255.0)
    } else {
        (
            (y8 - 16.0) / 219.0,
            (u8_ - 128.0) / 224.0,
            (v8 - 128.0) / 224.0,
        )
    };
    let [r, g, b] = match matrix {
        YuvMatrix::Bt709 => [
            y + 1.5748 * cr,
            y - 0.187324 * cb - 0.468124 * cr,
            y + 1.8556 * cb,
        ],
        YuvMatrix::Bt601 => [
            y + 1.402 * cr,
            y - 0.344136 * cb - 0.714136 * cr,
            y + 1.772 * cb,
        ],
    };
    [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]
}

/// One pixel, the same maths as `video.wgsl`.
pub fn yuv_to_rgb(y8: u8, u8_: u8, v8: u8, matrix: YuvMatrix, full_range: bool) -> [u8; 3] {
    yuv_to_rgb_f32(
        f32::from(y8),
        f32::from(u8_),
        f32::from(v8),
        matrix,
        full_range,
    )
    .map(|c| (c * 255.0).round() as u8)
}

/// The CPU reference: draw `video` over the whole frame (legacy nearest sample, opaque).
pub fn draw_cpu(frame: &mut Frame, video: &Nv12Frame) {
    let (ow, oh) = (frame.width, frame.height);
    let cw = video.chroma_width() as usize;
    for oy in 0..oh {
        let vy = ((oy * video.height / oh.max(1)).min(video.height - 1)) as usize;
        for ox in 0..ow {
            let vx = ((ox * video.width / ow.max(1)).min(video.width - 1)) as usize;
            let luma = video.y[vy * video.width as usize + vx];
            let at = ((vy / 2) * cw + vx / 2) * 2;
            let [r, g, b] = yuv_to_rgb(
                luma,
                video.uv[at],
                video.uv[at + 1],
                video.matrix,
                video.full_range,
            );
            let to = (oy as usize * ow as usize + ox as usize) * 4;
            frame.pixels[to..to + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
}

fn sample_luma_bilinear(video: &Nv12Frame, u: f32, v: f32) -> f32 {
    let vw = video.width as usize;
    let vh = video.height as usize;
    let fx = (u * vw as f32 - 0.5).clamp(0.0, vw.saturating_sub(1) as f32);
    let fy = (v * vh as f32 - 0.5).clamp(0.0, vh.saturating_sub(1) as f32);
    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let x1 = (x0 + 1).min(vw - 1);
    let y1 = (y0 + 1).min(vh - 1);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;

    let y00 = f32::from(video.y[y0 * vw + x0]) / 255.0;
    let y10 = f32::from(video.y[y0 * vw + x1]) / 255.0;
    let y01 = f32::from(video.y[y1 * vw + x0]) / 255.0;
    let y11 = f32::from(video.y[y1 * vw + x1]) / 255.0;
    let top = y00 + (y10 - y00) * tx;
    let bot = y01 + (y11 - y01) * tx;
    (top + (bot - top) * ty) * 255.0
}

fn sample_chroma_bilinear(video: &Nv12Frame, u: f32, v: f32) -> [f32; 2] {
    let cw = video.chroma_width() as usize;
    let ch = video.chroma_height() as usize;
    let fx = (u * cw as f32 - 0.5).clamp(0.0, cw.saturating_sub(1) as f32);
    let fy = (v * ch as f32 - 0.5).clamp(0.0, ch.saturating_sub(1) as f32);
    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let x1 = (x0 + 1).min(cw - 1);
    let y1 = (y0 + 1).min(ch - 1);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;

    let idx = |cx: usize, cy: usize| -> [f32; 2] {
        let off = (cy * cw + cx) * 2;
        [
            f32::from(video.uv[off]) / 255.0,
            f32::from(video.uv[off + 1]) / 255.0,
        ]
    };
    let c00 = idx(x0, y0);
    let c10 = idx(x1, y0);
    let c01 = idx(x0, y1);
    let c11 = idx(x1, y1);
    let top_u = c00[0] + (c10[0] - c00[0]) * tx;
    let bot_u = c01[0] + (c11[0] - c01[0]) * tx;
    let top_v = c00[1] + (c10[1] - c00[1]) * tx;
    let bot_v = c01[1] + (c11[1] - c01[1]) * tx;
    [
        (top_u + (bot_u - top_u) * ty) * 255.0,
        (top_v + (bot_v - top_v) * ty) * 255.0,
    ]
}

/// CPU reference matching `video.wgsl` mode 1: bilinear NV12 sampling, Fit/Fill/Stretch geometry,
/// and premultiplied source-over `opacity` compositing onto `frame`.
pub fn draw_cpu_layer(
    frame: &mut Frame,
    video: &Nv12Frame,
    fit: MediaFitMode,
    opacity: f32,
) -> anyhow::Result<()> {
    video.validate()?;
    let alpha = opacity.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return Ok(());
    }
    let (ow, oh) = (frame.width, frame.height);
    let (rx, ry, rw, rh) = compute_fit_rect(fit, video.width, video.height, ow, oh);
    let rw_safe = rw.max(1e-4);
    let rh_safe = rh.max(1e-4);
    let inv_alpha = 1.0 - alpha;

    for oy in 0..oh {
        let py = oy as f32 + 0.5;
        let v = (py - ry) / rh_safe;
        if !(0.0..1.0).contains(&v) {
            continue;
        }
        for ox in 0..ow {
            let px = ox as f32 + 0.5;
            let u = (px - rx) / rw_safe;
            if !(0.0..1.0).contains(&u) {
                continue;
            }
            let y8 = sample_luma_bilinear(video, u, v);
            let [u8_, v8] = sample_chroma_bilinear(video, u, v);
            let [r, g, b] = yuv_to_rgb_f32(y8, u8_, v8, video.matrix, video.full_range);
            let sr = r * alpha * 255.0;
            let sg = g * alpha * 255.0;
            let sb = b * alpha * 255.0;
            let sa = alpha * 255.0;

            let to = (oy as usize * ow as usize + ox as usize) * 4;
            let dst = &mut frame.pixels[to..to + 4];
            dst[0] = (sr + f32::from(dst[0]) * inv_alpha)
                .round()
                .clamp(0.0, 255.0) as u8;
            dst[1] = (sg + f32::from(dst[1]) * inv_alpha)
                .round()
                .clamp(0.0, 255.0) as u8;
            dst[2] = (sb + f32::from(dst[2]) * inv_alpha)
                .round()
                .clamp(0.0, 255.0) as u8;
            dst[3] = (sa + f32::from(dst[3]) * inv_alpha)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
    Ok(())
}

struct Planes {
    seq: u64,
    width: u32,
    height: u32,
    luma: wgpu::Texture,
    chroma: wgpu::Texture,
    luma_view: wgpu::TextureView,
    chroma_view: wgpu::TextureView,
    legacy_bind_group: wgpu::BindGroup,
    draw_slots: HashMap<u32, (wgpu::Buffer, wgpu::BindGroup)>,
}

/// The GPU side: keyed textures, the conversion pipelines, and uniform parameters.
pub struct VideoLayer {
    pipeline_opaque: wgpu::RenderPipeline,
    pipeline_blend: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    lin_sampler: wgpu::Sampler,
    legacy_params: wgpu::Buffer,
    planes: HashMap<String, Planes>,
}

fn plane_texture(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_plane(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    data: &[u8],
    bytes_per_row: u32,
    width: u32,
    height: u32,
) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn encode_params(
    out_width: u32,
    out_height: u32,
    video_width: u32,
    video_height: u32,
    matrix: YuvMatrix,
    full_range: bool,
    mode: u32,
    rect: (f32, f32, f32, f32),
    opacity: f32,
) -> [u8; 64] {
    let mut out = [0_u8; 64];
    out[0..4].copy_from_slice(&out_width.to_le_bytes());
    out[4..8].copy_from_slice(&out_height.to_le_bytes());
    out[8..12].copy_from_slice(&video_width.to_le_bytes());
    out[12..16].copy_from_slice(&video_height.to_le_bytes());
    out[16..20].copy_from_slice(&u32::from(matrix == YuvMatrix::Bt709).to_le_bytes());
    out[20..24].copy_from_slice(&u32::from(full_range).to_le_bytes());
    out[24..28].copy_from_slice(&mode.to_le_bytes());
    out[28..32].copy_from_slice(&0_u32.to_le_bytes());
    out[32..36].copy_from_slice(&rect.0.to_le_bytes());
    out[36..40].copy_from_slice(&rect.1.to_le_bytes());
    out[40..44].copy_from_slice(&rect.2.to_le_bytes());
    out[44..48].copy_from_slice(&rect.3.to_le_bytes());
    out[48..52].copy_from_slice(&opacity.clamp(0.0, 1.0).to_le_bytes());
    out
}

impl VideoLayer {
    pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
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
            label: Some("video"),
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
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let lin_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video-linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let make_pipeline = |blend: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if blend { "video-blend" } else { "video-opaque" }),
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
                        format: target,
                        blend: blend.then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let legacy_params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video-params"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline_opaque: make_pipeline(false),
            pipeline_blend: make_pipeline(true),
            layout,
            lin_sampler,
            legacy_params,
            planes: HashMap::new(),
        }
    }

    fn upload_internal(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: &str,
        seq: u64,
        video: &Nv12Frame,
    ) -> anyhow::Result<()> {
        video.validate()?;
        if let Some(existing) = self.planes.get(key)
            && seq != 0
            && existing.seq == seq
            && existing.width == video.width
            && existing.height == video.height
        {
            return Ok(());
        }

        let (cw, ch) = (video.chroma_width(), video.chroma_height());
        let recreate = self
            .planes
            .get(key)
            .is_none_or(|p| p.width != video.width || p.height != video.height);
        if recreate {
            let luma = plane_texture(
                device,
                "video-luma",
                video.width,
                video.height,
                wgpu::TextureFormat::R8Unorm,
            );
            let chroma = plane_texture(
                device,
                "video-chroma",
                cw,
                ch,
                wgpu::TextureFormat::Rg8Unorm,
            );
            let luma_view = luma.create_view(&wgpu::TextureViewDescriptor::default());
            let chroma_view = chroma.create_view(&wgpu::TextureViewDescriptor::default());
            let legacy_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("video"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.legacy_params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&luma_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&chroma_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.lin_sampler),
                    },
                ],
            });
            self.planes.insert(
                key.to_owned(),
                Planes {
                    seq,
                    width: video.width,
                    height: video.height,
                    luma,
                    chroma,
                    luma_view,
                    chroma_view,
                    legacy_bind_group,
                    draw_slots: HashMap::new(),
                },
            );
        }
        let planes = self.planes.get_mut(key).expect("planes exist");
        planes.seq = seq;
        write_plane(
            queue,
            &planes.luma,
            &video.y,
            video.width,
            video.width,
            video.height,
        );
        write_plane(queue, &planes.chroma, &video.uv, cw * 2, cw, ch);
        Ok(())
    }

    /// Upload a keyed frame's NV12 planes once per sequence number (skipped cleanly if unchanged
    /// across multiple outputs reading the same video hash).
    pub fn upload_keyed(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: &str,
        seq: u64,
        video: &Nv12Frame,
    ) -> anyhow::Result<()> {
        self.upload_internal(device, queue, key, seq, video)
    }

    /// Remove cached GPU textures for a stopped video key.
    pub fn remove_key(&mut self, key: &str) {
        self.planes.remove(key);
    }

    /// Upload a legacy frame's planes under key `""` (textures are reused while size matches).
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        video: &Nv12Frame,
        out_width: u32,
        out_height: u32,
    ) -> anyhow::Result<()> {
        self.upload_internal(device, queue, "", 0, video)?;
        let bytes = encode_params(
            out_width,
            out_height,
            video.width,
            video.height,
            video.matrix,
            video.full_range,
            0,
            (0.0, 0.0, out_width as f32, out_height as f32),
            1.0,
        );
        queue.write_buffer(&self.legacy_params, 0, &bytes);
        Ok(())
    }

    /// Draw the last uploaded legacy frame over the whole target.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(planes) = self.planes.get("") else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("video"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline_opaque);
        pass.set_bind_group(0, &planes.legacy_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Draw a keyed video frame as a normalized `Media` layer with bilinear scaling, Fit/Fill/Stretch,
    /// and premultiplied opacity, reusing cached uniform buffers & bind groups per `draw_slot`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        draw_slot: u32,
        key: &str,
        video: &Nv12Frame,
        out_width: u32,
        out_height: u32,
        fit: MediaFitMode,
        opacity: f32,
        clear_transparent: bool,
        target: &wgpu::TextureView,
    ) {
        let Some(planes) = self.planes.get_mut(key) else {
            return;
        };
        let alpha = opacity.clamp(0.0, 1.0);
        if alpha <= 0.0 && !clear_transparent {
            return;
        }
        let rect = compute_fit_rect(fit, video.width, video.height, out_width, out_height);
        let bytes = encode_params(
            out_width,
            out_height,
            video.width,
            video.height,
            video.matrix,
            video.full_range,
            1,
            rect,
            alpha,
        );
        let entry = planes.draw_slots.entry(draw_slot).or_insert_with(|| {
            let params = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("video-layer-params"),
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("video-layer"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&planes.luma_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&planes.chroma_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.lin_sampler),
                    },
                ],
            });
            (params, bind_group)
        });
        queue.write_buffer(&entry.0, 0, &bytes);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("video-layer"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if clear_transparent {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(if clear_transparent {
            &self.pipeline_opaque
        } else {
            &self.pipeline_blend
        });
        pass.set_bind_group(0, &entry.1, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_range_black_white_and_grey() {
        for matrix in [YuvMatrix::Bt601, YuvMatrix::Bt709] {
            assert_eq!(yuv_to_rgb(16, 128, 128, matrix, false), [0, 0, 0]);
            assert_eq!(yuv_to_rgb(235, 128, 128, matrix, false), [255, 255, 255]);
            assert_eq!(yuv_to_rgb(126, 128, 128, matrix, false), [128, 128, 128]);
        }
    }

    #[test]
    fn bt709_red_converts_to_red() {
        // BT.709 limited-range 75% red is Y 51, Cb 109, Cr 212 -> about (191, 0, 0).
        let [r, g, b] = yuv_to_rgb(51, 109, 212, YuvMatrix::Bt709, false);
        assert!(r.abs_diff(191) <= 2 && g <= 2 && b <= 2, "{r} {g} {b}");
    }

    #[test]
    fn cpu_draw_maps_chroma_per_two_by_two_block() {
        let video = Nv12Frame {
            width: 4,
            height: 2,
            // Left 2×2 block: white. Right 2×2 block: BT.709 75% red.
            y: vec![235, 235, 51, 51, 235, 235, 51, 51],
            uv: vec![128, 128, 109, 212],
            matrix: YuvMatrix::Bt709,
            full_range: false,
        };
        video.validate().unwrap();
        let mut frame = Frame::new(4, 2);
        draw_cpu(&mut frame, &video);
        assert_eq!(&frame.pixels[0..4], &[255, 255, 255, 255]);
        let right = &frame.pixels[8..11];
        assert!(
            right[0] > 185 && right[1] < 8 && right[2] < 8,
            "right half is red: {right:?}"
        );
    }

    #[test]
    fn mismatched_planes_are_refused() {
        let video = Nv12Frame {
            width: 4,
            height: 2,
            y: vec![0; 7],
            uv: vec![0; 4],
            matrix: YuvMatrix::Bt709,
            full_range: false,
        };
        assert!(video.validate().is_err());
    }

    #[test]
    fn from_rgba_round_trips_solid_colors_and_gradients() {
        let mut src = Frame::new(8, 4);
        for y in 0..4u32 {
            for x in 0..8u32 {
                let idx = (y as usize * 8 + x as usize) * 4;
                let px = if x < 4 {
                    [200, 40, 30, 255]
                } else {
                    [20, 160, 220, 255]
                };
                src.pixels[idx..idx + 4].copy_from_slice(&px);
            }
        }
        let nv12 = Nv12Frame::from_rgba(&src).expect("from_rgba");
        let mut out = Frame::new(8, 4);
        draw_cpu_layer(&mut out, &nv12, MediaFitMode::Fill, 1.0).expect("draw_cpu_layer");
        for (px, exp) in [([1, 2], [200_u8, 40, 30]), ([6, 2], [20, 160, 220])] {
            let idx = (px[1] * 8 + px[0]) * 4;
            assert!(out.pixels[idx].abs_diff(exp[0]) <= 3);
            assert!(out.pixels[idx + 1].abs_diff(exp[1]) <= 3);
            assert!(out.pixels[idx + 2].abs_diff(exp[2]) <= 3);
            assert_eq!(out.pixels[idx + 3], 255);
        }
    }
}
