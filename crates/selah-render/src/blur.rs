//! Separable 2-pass Gaussian blur on both CPU and GPU (M3.4 / M3.5).
use crate::Frame;
use crate::gpu::TARGET_FORMAT;

const SHADER: &str = include_str!("blur.wgsl");
const MAX_TAPS: usize = 32;

/// Compute downsampling factor (`1`, `2`, or `4`), integer half-kernel radius, and normalized
/// Gaussian weights for `sigma`.
pub fn kernel_for_sigma(sigma: f32) -> (u32, usize, [f32; 36]) {
    let sigma = if sigma.is_finite() {
        sigma.clamp(0.0, 64.0)
    } else {
        0.0
    };
    let div = if sigma <= 3.0 {
        1
    } else if sigma <= 12.0 {
        2
    } else {
        4
    };
    let eff_sigma = (sigma / div as f32).max(0.25);
    let radius = ((eff_sigma * 2.5).ceil() as usize).clamp(1, MAX_TAPS);
    let mut weights = [0.0_f32; 36];
    let denom = 2.0 * eff_sigma * eff_sigma;
    weights[0] = 1.0;
    let mut sum = 1.0_f32;
    for (k, slot) in weights.iter_mut().enumerate().take(radius + 1).skip(1) {
        let d = k as f32;
        let w = (-(d * d) / denom).exp();
        *slot = w;
        sum += 2.0 * w;
    }
    for slot in weights.iter_mut().take(radius + 1) {
        *slot /= sum;
    }
    (div, radius, weights)
}

fn bilinear_sample_premul(frame: &Frame, u: f32, v: f32) -> [f32; 4] {
    let u = u.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let w = frame.width as usize;
    let h = frame.height as usize;
    let fx = (u * w as f32 - 0.5).clamp(0.0, w.saturating_sub(1) as f32);
    let fy = (v * h as f32 - 0.5).clamp(0.0, h.saturating_sub(1) as f32);
    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;

    let read = |px: usize, py: usize| -> [f32; 4] {
        let idx = (py * w + px) * 4;
        [
            f32::from(frame.pixels[idx]) / 255.0,
            f32::from(frame.pixels[idx + 1]) / 255.0,
            f32::from(frame.pixels[idx + 2]) / 255.0,
            f32::from(frame.pixels[idx + 3]) / 255.0,
        ]
    };
    let c00 = read(x0, y0);
    let c10 = read(x1, y0);
    let c01 = read(x0, y1);
    let c11 = read(x1, y1);
    let mut out = [0.0_f32; 4];
    for ch in 0..4 {
        let top = c00[ch] + (c10[ch] - c00[ch]) * tx;
        let bot = c01[ch] + (c11[ch] - c01[ch]) * tx;
        out[ch] = top + (bot - top) * ty;
    }
    out
}

fn downsample_cpu(src: &Frame, dst_w: u32, dst_h: u32) -> Frame {
    let mut out = Frame::new(dst_w, dst_h);
    for y in 0..dst_h {
        let v = (y as f32 + 0.5) / dst_h as f32;
        for x in 0..dst_w {
            let u = (x as f32 + 0.5) / dst_w as f32;
            let s = bilinear_sample_premul(src, u, v);
            let idx = (y as usize * dst_w as usize + x as usize) * 4;
            for (dst, &val) in out.pixels[idx..idx + 4].iter_mut().zip(&s) {
                *dst = (val.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    out
}

fn blur_pass_1d_cpu(
    src: &Frame,
    dir_x: f32,
    dir_y: f32,
    radius: usize,
    weights: &[f32; 36],
    opacity: f32,
) -> Frame {
    let w = src.width.max(1);
    let h = src.height.max(1);
    let mut out = Frame::new(w, h);
    let texel_x = dir_x / w as f32;
    let texel_y = dir_y / h as f32;
    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let c0 = bilinear_sample_premul(src, u, v);
            let mut accum = [
                c0[0] * weights[0],
                c0[1] * weights[0],
                c0[2] * weights[0],
                c0[3] * weights[0],
            ];
            for (k, &wk) in weights.iter().enumerate().take(radius + 1).skip(1) {
                let ox = texel_x * k as f32;
                let oy = texel_y * k as f32;
                let c1 = bilinear_sample_premul(src, u + ox, v + oy);
                let c2 = bilinear_sample_premul(src, u - ox, v - oy);
                for ch in 0..4 {
                    accum[ch] += (c1[ch] + c2[ch]) * wk;
                }
            }
            let idx = (y as usize * w as usize + x as usize) * 4;
            for (dst, &val) in out.pixels[idx..idx + 4].iter_mut().zip(&accum) {
                *dst = ((val * opacity).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    out
}

/// Apply the separable Gaussian blur to a premultiplied RGBA [`Frame`], returning a new
/// blurred frame scaled by `opacity` in `0.0..=1.0`.
pub fn blur_cpu(src: &Frame, sigma: f32, opacity: f32) -> Frame {
    let opacity = opacity.clamp(0.0, 1.0);
    if sigma <= 0.05 || !sigma.is_finite() {
        if (opacity - 1.0).abs() <= f32::EPSILON {
            return src.clone();
        }
        let mut copy = src.clone();
        for px in copy.pixels.chunks_exact_mut(4) {
            for ch in px.iter_mut() {
                *ch = (f32::from(*ch) * opacity).round().clamp(0.0, 255.0) as u8;
            }
        }
        return copy;
    }

    let (div, radius, weights) = kernel_for_sigma(sigma);
    let work = if div > 1 {
        let dw = (src.width / div).max(16).min(src.width.max(1));
        let dh = (src.height / div).max(16).min(src.height.max(1));
        downsample_cpu(src, dw, dh)
    } else {
        src.clone()
    };
    let horiz = blur_pass_1d_cpu(&work, 1.0, 0.0, radius, &weights, 1.0);
    let vert = blur_pass_1d_cpu(&horiz, 0.0, 1.0, radius, &weights, 1.0);
    if div == 1 && (opacity - 1.0).abs() <= f32::EPSILON {
        return vert;
    }
    let mut out = Frame::new(src.width, src.height);
    for y in 0..src.height {
        let v = (y as f32 + 0.5) / src.height.max(1) as f32;
        for x in 0..src.width {
            let u = (x as f32 + 0.5) / src.width.max(1) as f32;
            let s = bilinear_sample_premul(&vert, u, v);
            let idx = (y as usize * src.width as usize + x as usize) * 4;
            for (dst, &val) in out.pixels[idx..idx + 4].iter_mut().zip(&s) {
                *dst = ((val * opacity).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    out
}

struct BlurTargets {
    full_w: u32,
    full_h: u32,
    work_w: u32,
    work_h: u32,
    _src_tex: wgpu::Texture,
    src_view: wgpu::TextureView,
    _work_a: wgpu::Texture,
    work_a_view: wgpu::TextureView,
    _work_b: wgpu::Texture,
    work_b_view: wgpu::TextureView,
}

fn rgba_render_tex(device: &wgpu::Device, label: &str, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
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
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// GPU separable Gaussian blur & upsample pipeline matching [`blur_cpu`].
pub struct BlurPipeline {
    pipeline_opaque: wgpu::RenderPipeline,
    pipeline_blend: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params_pool: Vec<wgpu::Buffer>,
    pool_cursor: usize,
    targets: Option<BlurTargets>,
}

impl BlurPipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let make_pipeline = |blend: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if blend { "blur-blend" } else { "blur-opaque" }),
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
                        format,
                        blend: blend.then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blur-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self {
            pipeline_opaque: make_pipeline(false),
            pipeline_blend: make_pipeline(true),
            layout,
            sampler,
            params_pool: Vec::new(),
            pool_cursor: 0,
            targets: None,
        }
    }

    /// Reset uniform buffer reuse cursor at the start of each frame submission.
    pub fn begin_frame(&mut self) {
        self.pool_cursor = 0;
    }

    #[allow(clippy::too_many_arguments)]
    fn alloc_params_slot(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        dir_x: f32,
        dir_y: f32,
        radius: usize,
        weights: &[f32; 36],
        opacity: f32,
    ) -> usize {
        let idx = self.pool_cursor;
        self.pool_cursor += 1;
        if idx >= self.params_pool.len() {
            self.params_pool
                .push(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("blur-params"),
                    size: 176,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
        }
        Self::write_params_buffer(
            queue,
            &self.params_pool[idx],
            width,
            height,
            dir_x,
            dir_y,
            radius,
            weights,
            opacity,
        );
        idx
    }

    /// Ensure the reusable scratch textures exist and return the intermediate full-resolution
    /// view where a caller (like `VideoLayer`) can render an unblurred RGBA frame prior to
    /// calling [`Self::blur_and_blend`].
    pub fn targets_src_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.src_view)
    }

    pub fn ensure_source_view(
        &mut self,
        device: &wgpu::Device,
        full_w: u32,
        full_h: u32,
        sigma: f32,
    ) -> &wgpu::TextureView {
        let (div, _, _) = kernel_for_sigma(sigma);
        let work_w = (full_w / div).max(16).min(full_w.max(1));
        let work_h = (full_h / div).max(16).min(full_h.max(1));
        let recreate = self.targets.as_ref().is_none_or(|t| {
            t.full_w != full_w || t.full_h != full_h || t.work_w != work_w || t.work_h != work_h
        });
        if recreate {
            let src_tex = rgba_render_tex(device, "blur-src", full_w, full_h);
            let work_a = rgba_render_tex(device, "blur-work-a", work_w, work_h);
            let work_b = rgba_render_tex(device, "blur-work-b", work_w, work_h);
            self.targets = Some(BlurTargets {
                full_w,
                full_h,
                work_w,
                work_h,
                src_view: src_tex.create_view(&wgpu::TextureViewDescriptor::default()),
                _src_tex: src_tex,
                work_a_view: work_a.create_view(&wgpu::TextureViewDescriptor::default()),
                _work_a: work_a,
                work_b_view: work_b.create_view(&wgpu::TextureViewDescriptor::default()),
                _work_b: work_b,
            });
        }
        &self.targets.as_ref().expect("targets created").src_view
    }

    #[allow(clippy::too_many_arguments)]
    fn write_params_buffer(
        queue: &wgpu::Queue,
        buffer: &wgpu::Buffer,
        width: u32,
        height: u32,
        dir_x: f32,
        dir_y: f32,
        radius: usize,
        weights: &[f32; 36],
        opacity: f32,
    ) {
        let mut bytes = [0_u8; 176];
        bytes[0..4].copy_from_slice(&width.to_le_bytes());
        bytes[4..8].copy_from_slice(&height.to_le_bytes());
        bytes[8..12].copy_from_slice(&dir_x.to_le_bytes());
        bytes[12..16].copy_from_slice(&dir_y.to_le_bytes());
        bytes[16..20].copy_from_slice(&(radius as i32).to_le_bytes());
        bytes[20..24].copy_from_slice(&opacity.to_le_bytes());
        for (i, w) in weights.iter().enumerate() {
            let off = 32 + i * 4;
            bytes[off..off + 4].copy_from_slice(&w.to_le_bytes());
        }
        queue.write_buffer(buffer, 0, &bytes);
    }

    fn run_pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        src: &wgpu::TextureView,
        dst: &wgpu::TextureView,
        blend: bool,
        params: &wgpu::Buffer,
    ) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur-pass"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(src),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blur-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if blend {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(if blend {
            &self.pipeline_blend
        } else {
            &self.pipeline_opaque
        });
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Run separable blur on `self.targets.src_view` and composite onto `final_target` using
    /// premultiplied-alpha blending with `opacity`.
    pub fn blur_and_blend(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        final_target: &wgpu::TextureView,
        sigma: f32,
        opacity: f32,
    ) {
        if self.targets.is_none() {
            return;
        }
        self.run_blur_sequence(
            device,
            queue,
            encoder,
            None,
            final_target,
            sigma,
            opacity,
            true,
        );
    }

    /// Run separable blur from `src_view` into `dst_view` (must be backed by distinct textures).
    #[allow(clippy::too_many_arguments)]
    pub fn blur_view_to_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        src_view: &wgpu::TextureView,
        dst_view: &wgpu::TextureView,
        full_w: u32,
        full_h: u32,
        sigma: f32,
        opacity: f32,
        blend_onto_dst: bool,
    ) {
        let _ = self.ensure_source_view(device, full_w, full_h, sigma);
        self.run_blur_sequence(
            device,
            queue,
            encoder,
            Some(src_view),
            dst_view,
            sigma,
            opacity,
            blend_onto_dst,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn run_blur_sequence(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        external_src: Option<&wgpu::TextureView>,
        final_target: &wgpu::TextureView,
        sigma: f32,
        opacity: f32,
        blend_onto_dst: bool,
    ) {
        let Some((full_w, full_h, work_w, work_h)) = self
            .targets
            .as_ref()
            .map(|t| (t.full_w, t.full_h, t.work_w, t.work_h))
        else {
            return;
        };
        let (div, radius, weights) = kernel_for_sigma(sigma);
        let mut copy_weights = [0.0_f32; 36];
        copy_weights[0] = 1.0;

        let ds_slot = (div > 1).then(|| {
            self.alloc_params_slot(
                device,
                queue,
                full_w,
                full_h,
                0.0,
                0.0,
                0,
                &copy_weights,
                1.0,
            )
        });
        let h_slot = self.alloc_params_slot(
            device, queue, work_w, work_h, 1.0, 0.0, radius, &weights, 1.0,
        );
        let v_opacity = if div == 1 {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let v_slot = self.alloc_params_slot(
            device, queue, work_w, work_h, 0.0, 1.0, radius, &weights, v_opacity,
        );
        let up_slot = (div > 1).then(|| {
            self.alloc_params_slot(
                device,
                queue,
                work_w,
                work_h,
                0.0,
                0.0,
                0,
                &copy_weights,
                opacity.clamp(0.0, 1.0),
            )
        });

        let t = self.targets.as_ref().expect("targets exist");
        let base_src = external_src.unwrap_or(&t.src_view);
        let work_input: &wgpu::TextureView = if let Some(ds) = ds_slot {
            self.run_pass(
                device,
                encoder,
                base_src,
                &t.work_b_view,
                false,
                &self.params_pool[ds],
            );
            &t.work_b_view
        } else {
            base_src
        };

        self.run_pass(
            device,
            encoder,
            work_input,
            &t.work_a_view,
            false,
            &self.params_pool[h_slot],
        );

        if let Some(up) = up_slot {
            self.run_pass(
                device,
                encoder,
                &t.work_a_view,
                &t.work_b_view,
                false,
                &self.params_pool[v_slot],
            );
            self.run_pass(
                device,
                encoder,
                &t.work_b_view,
                final_target,
                blend_onto_dst,
                &self.params_pool[up],
            );
        } else {
            self.run_pass(
                device,
                encoder,
                &t.work_a_view,
                final_target,
                blend_onto_dst,
                &self.params_pool[v_slot],
            );
        }
    }
}
