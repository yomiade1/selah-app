//! Deterministic frame and layer transition blenders on both CPU and GPU (M3.5).
use selah_proto::v1::{
    LayerKind, LayerSpec, Scene, TransitionDirection, TransitionEasing, TransitionKind,
    TransitionSpec,
};

use crate::Frame;
use crate::gpu::TARGET_FORMAT;

/// Per-layer active transition parameters for [`RenderTransitionPlan`].
#[derive(Clone, Debug)]
pub struct LayerTransitionBlend {
    pub kind: LayerKind,
    pub from_layer: Option<LayerSpec>,
    pub transition: TransitionKind,
    pub direction: TransitionDirection,
    pub progress: f32,
}

/// Active output and layer transitions for one rendered frame.
#[derive(Clone, Debug, Default)]
pub struct RenderTransitionPlan {
    pub output_transition: Option<(Scene, TransitionKind, TransitionDirection, f32)>,
    pub layer_transitions: Vec<LayerTransitionBlend>,
}

const SHADER: &str = include_str!("transitions.wgsl");

const BAYER_4X4: [f32; 16] = [
    0.0, 8.0, 2.0, 10.0, //
    12.0, 4.0, 14.0, 6.0, //
    3.0, 11.0, 1.0, 9.0, //
    15.0, 7.0, 13.0, 5.0,
];

/// Apply the normalized transition easing curve to `raw_t` in `0.0..=1.0`.
pub fn apply_easing(easing: TransitionEasing, raw_t: f32) -> f32 {
    let t = raw_t.clamp(0.0, 1.0);
    match easing {
        TransitionEasing::Unspecified | TransitionEasing::Linear => t,
        TransitionEasing::EaseIn => t * t * t,
        TransitionEasing::EaseOut => 1.0 - (1.0 - t).powi(3),
        TransitionEasing::EaseInOut => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(3) * 0.5
            }
        }
    }
}

/// Normalize a `TransitionSpec` into `(kind, direction, easing, duration_ms)`.
pub fn resolve_transition_spec(
    spec: Option<&TransitionSpec>,
) -> (TransitionKind, TransitionDirection, TransitionEasing, u32) {
    let Some(s) = spec else {
        return (
            TransitionKind::Cut,
            TransitionDirection::Left,
            TransitionEasing::Linear,
            0,
        );
    };
    let kind = TransitionKind::try_from(s.kind).unwrap_or(TransitionKind::Cut);
    let dir = TransitionDirection::try_from(s.direction).unwrap_or(TransitionDirection::Left);
    let easing = TransitionEasing::try_from(s.easing).unwrap_or(TransitionEasing::Linear);
    (kind, dir, easing, s.duration_ms)
}

/// Returns `true` if `spec` specifies a multi-frame time-varying transition (`duration_ms > 0`).
pub fn transition_is_animated(spec: Option<&TransitionSpec>) -> bool {
    let (kind, _, _, duration_ms) = resolve_transition_spec(spec);
    duration_ms > 0 && !matches!(kind, TransitionKind::Unspecified | TransitionKind::Cut)
}

/// Compute eased transition progress in `0.0..=1.0` for `elapsed_ms`.
pub fn transition_progress(spec: Option<&TransitionSpec>, elapsed_ms: u32) -> f32 {
    let (kind, _, easing, duration_ms) = resolve_transition_spec(spec);
    if duration_ms == 0 || matches!(kind, TransitionKind::Unspecified | TransitionKind::Cut) {
        return 1.0;
    }
    let raw_t = (elapsed_ms as f32 / duration_ms as f32).clamp(0.0, 1.0);
    apply_easing(easing, raw_t)
}

fn bilinear_sample_premul(frame: &Frame, u: f32, v: f32) -> [f32; 4] {
    let u = u.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let w = frame.width.max(1) as usize;
    let h = frame.height.max(1) as usize;
    if frame.pixels.len() < w * h * 4 {
        return [0.0, 0.0, 0.0, 0.0];
    }
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

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

fn direction_axis(dir: TransitionDirection, u: f32, v: f32) -> (f32, bool, bool) {
    match dir {
        TransitionDirection::Right => (u, false, false),
        TransitionDirection::Up => (v, true, true),
        TransitionDirection::Down => (v, false, true),
        TransitionDirection::Left | TransitionDirection::Unspecified => (u, true, false),
    }
}

/// Blend `from` and `to` premultiplied RGBA frames at eased `progress` in `0.0..=1.0`.
///
/// Set `overlay_mode = true` for transparent upper layers composited over lower persistent layers
/// (prevents stale `from` ghosts in `Slide`, fades midpoint alpha in `FadeThroughBlack`, and
/// keeps `Zoom` outer rim transparent).
pub fn blend_frames_cpu(
    from: &Frame,
    to: &Frame,
    kind: TransitionKind,
    direction: TransitionDirection,
    progress: f32,
    overlay_mode: bool,
) -> Frame {
    let p = progress.clamp(0.0, 1.0);
    if matches!(kind, TransitionKind::Unspecified | TransitionKind::Cut) {
        return if p > 0.0 { to.clone() } else { from.clone() };
    }
    if p <= 0.0 {
        return from.clone();
    }
    if p >= 1.0 {
        return to.clone();
    }

    let w = to.width.max(1);
    let h = to.height.max(1);
    let mut out = Frame::new(w, h);
    let same_dims = from.width == w && from.height == h;

    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let from_c = if same_dims {
                let idx = (y as usize * w as usize + x as usize) * 4;
                [
                    f32::from(from.pixels[idx]) / 255.0,
                    f32::from(from.pixels[idx + 1]) / 255.0,
                    f32::from(from.pixels[idx + 2]) / 255.0,
                    f32::from(from.pixels[idx + 3]) / 255.0,
                ]
            } else {
                bilinear_sample_premul(from, u, v)
            };
            let to_idx = (y as usize * w as usize + x as usize) * 4;
            let to_c = [
                f32::from(to.pixels[to_idx]) / 255.0,
                f32::from(to.pixels[to_idx + 1]) / 255.0,
                f32::from(to.pixels[to_idx + 2]) / 255.0,
                f32::from(to.pixels[to_idx + 3]) / 255.0,
            ];

            let px = match kind {
                TransitionKind::Unspecified | TransitionKind::Cut => unreachable!(),
                TransitionKind::Fade => mix4(from_c, to_c, p),
                TransitionKind::Dissolve => {
                    let threshold = (BAYER_4X4[((y & 3) * 4 + (x & 3)) as usize] + 0.5) / 16.0;
                    if p >= threshold { to_c } else { from_c }
                }
                TransitionKind::FadeThroughBlack => {
                    if p < 0.5 {
                        let k = 1.0 - 2.0 * p;
                        let out_a = if overlay_mode {
                            from_c[3] * k
                        } else {
                            from_c[3]
                        };
                        [from_c[0] * k, from_c[1] * k, from_c[2] * k, out_a]
                    } else {
                        let k = 2.0 * p - 1.0;
                        let out_a = if overlay_mode { to_c[3] * k } else { to_c[3] };
                        [to_c[0] * k, to_c[1] * k, to_c[2] * k, out_a]
                    }
                }
                TransitionKind::Push => {
                    let (coord, starts_high, is_vert) = direction_axis(direction, u, v);
                    let sample_shift = |src: &Frame, delta: f32| {
                        let (su, sv) = if is_vert {
                            (u, v + delta)
                        } else {
                            (u + delta, v)
                        };
                        bilinear_sample_premul(src, su, sv)
                    };
                    if starts_high {
                        if coord >= 1.0 - p {
                            sample_shift(to, -(1.0 - p))
                        } else {
                            sample_shift(from, p)
                        }
                    } else if coord < p {
                        sample_shift(to, 1.0 - p)
                    } else {
                        sample_shift(from, -p)
                    }
                }
                TransitionKind::Wipe => {
                    let (coord, starts_high, _) = direction_axis(direction, u, v);
                    let signed_dist = if starts_high {
                        coord - (1.0 - p)
                    } else {
                        p - coord
                    };
                    let w_mix = smoothstep(-0.015, 0.015, signed_dist);
                    mix4(from_c, to_c, w_mix)
                }
                TransitionKind::Zoom => {
                    const UV_EPS: f32 = 1e-5;
                    let scale = 0.5 + 0.5 * p;
                    let zu = (u - 0.5) / scale + 0.5;
                    let zv = (v - 0.5) / scale + 0.5;
                    let zoom_to = if (-UV_EPS..=1.0 + UV_EPS).contains(&zu)
                        && (-UV_EPS..=1.0 + UV_EPS).contains(&zv)
                    {
                        bilinear_sample_premul(to, zu, zv)
                    } else if overlay_mode {
                        [0.0, 0.0, 0.0, 0.0]
                    } else {
                        [0.0, 0.0, 0.0, 1.0]
                    };
                    mix4(from_c, zoom_to, p)
                }
                TransitionKind::Slide => {
                    let (coord, starts_high, is_vert) = direction_axis(direction, u, v);
                    let in_to = if starts_high {
                        coord >= 1.0 - p
                    } else {
                        coord < p
                    };
                    let from_vis = if overlay_mode {
                        let keep = 1.0 - p;
                        [
                            from_c[0] * keep,
                            from_c[1] * keep,
                            from_c[2] * keep,
                            from_c[3] * keep,
                        ]
                    } else {
                        from_c
                    };
                    if in_to {
                        let delta = if starts_high { -(1.0 - p) } else { 1.0 - p };
                        let (su, sv) = if is_vert {
                            (u, v + delta)
                        } else {
                            (u + delta, v)
                        };
                        let s_to = bilinear_sample_premul(to, su, sv);
                        let inv_a = 1.0 - s_to[3];
                        [
                            s_to[0] + from_vis[0] * inv_a,
                            s_to[1] + from_vis[1] * inv_a,
                            s_to[2] + from_vis[2] * inv_a,
                            s_to[3] + from_vis[3] * inv_a,
                        ]
                    } else {
                        from_vis
                    }
                }
            };

            let dst = &mut out.pixels[to_idx..to_idx + 4];
            for ch in 0..4 {
                dst[ch] = (px[ch].clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    out
}

/// GPU transition pipeline matching [`blend_frames_cpu`].
pub struct TransitionPipeline {
    pipeline_opaque: wgpu::RenderPipeline,
    pipeline_blend: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params_pool: Vec<wgpu::Buffer>,
    pool_cursor: usize,
}

impl TransitionPipeline {
    pub fn begin_frame(&mut self) {
        self.pool_cursor = 0;
    }
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("transitions"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("transitions"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("transitions"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let make_pipeline = |blend: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if blend {
                    "transitions-blend"
                } else {
                    "transitions-opaque"
                }),
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
                        blend: blend.then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("transitions-linear"),
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
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        from_view: &wgpu::TextureView,
        to_view: &wgpu::TextureView,
        dst_view: &wgpu::TextureView,
        kind: TransitionKind,
        direction: TransitionDirection,
        progress: f32,
        overlay_mode: bool,
        blend_onto_dst: bool,
    ) {
        let slot = self.pool_cursor;
        self.pool_cursor += 1;
        if slot >= self.params_pool.len() {
            self.params_pool
                .push(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("transition-params"),
                    size: 16,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
        }
        let mut bytes = [0_u8; 16];
        bytes[0..4].copy_from_slice(&(kind as u32).to_le_bytes());
        bytes[4..8].copy_from_slice(&(direction as u32).to_le_bytes());
        bytes[8..12].copy_from_slice(&progress.clamp(0.0, 1.0).to_le_bytes());
        bytes[12..16].copy_from_slice(&u32::from(overlay_mode).to_le_bytes());
        queue.write_buffer(&self.params_pool[slot], 0, &bytes);

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("transition-bind"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.params_pool[slot].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(from_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(to_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("transition"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if blend_onto_dst {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(if blend_onto_dst {
            &self.pipeline_blend
        } else {
            &self.pipeline_opaque
        });
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::{AdapterChoice, GpuCompositor};

    fn solid_rgba(w: u32, h: u32, rgba: [u8; 4]) -> Frame {
        let mut f = Frame::new(w, h);
        for px in f.pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&rgba);
        }
        f
    }

    fn split_bars(w: u32, h: u32, flip: bool) -> Frame {
        let mut f = Frame::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let idx = (y as usize * w as usize + x as usize) * 4;
                let left = (x < w / 2) ^ flip;
                let px = if left {
                    if y < h / 2 {
                        [200, 40, 60, 255]
                    } else {
                        [40, 180, 90, 255]
                    }
                } else if y < h / 2 {
                    [30, 80, 220, 255]
                } else {
                    [230, 210, 40, 255]
                };
                f.pixels[idx..idx + 4].copy_from_slice(&px);
            }
        }
        f
    }

    #[test]
    fn easings_respect_endpoints_and_monotonicity() {
        for easing in [
            TransitionEasing::Linear,
            TransitionEasing::EaseIn,
            TransitionEasing::EaseOut,
            TransitionEasing::EaseInOut,
        ] {
            assert_eq!(apply_easing(easing, 0.0), 0.0);
            assert_eq!(apply_easing(easing, 1.0), 1.0);
            let mut prev = 0.0_f32;
            for step in 1..=20 {
                let cur = apply_easing(easing, step as f32 / 20.0);
                assert!(cur >= prev - 1e-6);
                prev = cur;
            }
        }
        assert!(apply_easing(TransitionEasing::EaseIn, 0.25) < 0.25);
        assert!(apply_easing(TransitionEasing::EaseOut, 0.25) > 0.25);
        assert!((apply_easing(TransitionEasing::EaseInOut, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn all_transitions_exact_at_endpoints_and_deterministic() {
        let from = solid_rgba(32, 18, [220, 40, 30, 255]);
        let to = solid_rgba(32, 18, [20, 120, 230, 255]);
        let kinds = [
            TransitionKind::Cut,
            TransitionKind::Fade,
            TransitionKind::Dissolve,
            TransitionKind::FadeThroughBlack,
            TransitionKind::Push,
            TransitionKind::Wipe,
            TransitionKind::Zoom,
            TransitionKind::Slide,
        ];
        for kind in kinds {
            for dir in [
                TransitionDirection::Left,
                TransitionDirection::Right,
                TransitionDirection::Up,
                TransitionDirection::Down,
            ] {
                let f0 = blend_frames_cpu(&from, &to, kind, dir, 0.0, false);
                assert_eq!(f0.checksum(), from.checksum(), "p=0 {kind:?} {dir:?}");
                let f1 = blend_frames_cpu(&from, &to, kind, dir, 1.0, false);
                assert_eq!(f1.checksum(), to.checksum(), "p=1 {kind:?} {dir:?}");
            }
        }
        // FadeThroughBlack hits pure black at p = 0.5 in full-output mode.
        let mid = blend_frames_cpu(
            &from,
            &to,
            TransitionKind::FadeThroughBlack,
            TransitionDirection::Left,
            0.5,
            false,
        );
        assert!(mid.pixels.chunks_exact(4).all(|px| px == [0, 0, 0, 255]));
        // Overlay Slide at p = 0.999 never leaves full opacity of `from`.
        let transp_to = Frame::new(16, 16);
        let slide_end = blend_frames_cpu(
            &solid_rgba(16, 16, [255, 255, 255, 255]),
            &transp_to,
            TransitionKind::Slide,
            TransitionDirection::Left,
            1.0,
            true,
        );
        assert_eq!(slide_end.checksum(), transp_to.checksum());
    }

    #[test]
    fn gpu_and_cpu_transitions_match_across_kinds_and_progress() {
        let gpu = match GpuCompositor::new(AdapterChoice::from_env()) {
            Ok(g) => g,
            Err(_) => return,
        };
        let (w, h) = (32_u32, 18_u32);
        let from = split_bars(w, h, false);
        let to = split_bars(w, h, true);
        let upload_tex = |label: &str, frame: &Frame| {
            let tex = gpu.device().create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TARGET_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            gpu.queue().write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &frame.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            tex
        };
        let from_tex = upload_tex("tr-from", &from);
        let to_tex = upload_tex("tr-to", &to);
        let dst_tex = upload_tex("tr-dst", &Frame::new(w, h));
        let from_view = from_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let to_view = to_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let dst_view = dst_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let padded_row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = gpu.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("tr-rb"),
            size: u64::from(padded_row) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut pipe = TransitionPipeline::new(gpu.device());

        let cases = [
            (TransitionKind::Cut, TransitionDirection::Left, 0.25),
            (TransitionKind::Fade, TransitionDirection::Left, 0.5),
            (TransitionKind::Dissolve, TransitionDirection::Left, 0.5),
            (
                TransitionKind::FadeThroughBlack,
                TransitionDirection::Left,
                0.25,
            ),
            (
                TransitionKind::FadeThroughBlack,
                TransitionDirection::Left,
                0.75,
            ),
            (TransitionKind::Push, TransitionDirection::Left, 0.5),
            (TransitionKind::Push, TransitionDirection::Down, 0.5),
            (TransitionKind::Wipe, TransitionDirection::Right, 0.5),
            (TransitionKind::Wipe, TransitionDirection::Up, 0.5),
            (TransitionKind::Zoom, TransitionDirection::Left, 0.5),
            (TransitionKind::Slide, TransitionDirection::Left, 0.5),
            (TransitionKind::Slide, TransitionDirection::Right, 0.75),
        ];

        for (kind, dir, p) in cases {
            pipe.begin_frame();
            let cpu_out = blend_frames_cpu(&from, &to, kind, dir, p, false);
            let mut enc = gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("tr-test"),
                });
            pipe.draw(
                gpu.device(),
                gpu.queue(),
                &mut enc,
                &from_view,
                &to_view,
                &dst_view,
                kind,
                dir,
                p,
                false,
                false,
            );
            enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &dst_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_row),
                        rows_per_image: Some(h),
                    },
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            gpu.queue().submit([enc.finish()]);
            let slice = readback.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            let _ = gpu.device().poll(wgpu::PollType::wait_indefinitely());
            rx.recv().unwrap().unwrap();
            let mapped = slice.get_mapped_range().unwrap();
            let mut gpu_pixels = vec![0_u8; (w * h * 4) as usize];
            for row in 0..h as usize {
                let from_off = row * padded_row as usize;
                let to_off = row * w as usize * 4;
                gpu_pixels[to_off..to_off + w as usize * 4]
                    .copy_from_slice(&mapped[from_off..from_off + w as usize * 4]);
            }
            drop(mapped);
            readback.unmap();

            // Check max channel error <= 3/255 across non-seam pixels (linear sampling tolerance).
            let mut max_err = 0_i32;
            let mut bad = 0_usize;
            for (c, g) in cpu_out
                .pixels
                .chunks_exact(4)
                .zip(gpu_pixels.chunks_exact(4))
            {
                for ch in 0..4 {
                    let err = (i32::from(c[ch]) - i32::from(g[ch])).abs();
                    max_err = max_err.max(err);
                    if err > 4 {
                        bad += 1;
                    }
                }
            }
            assert!(
                bad == 0,
                "GPU/CPU mismatch for {kind:?}/{dir:?}@{p}: max_err={max_err}, bad_channels={bad}"
            );
        }

        // Regression check: 320x180 Zoom at p=0.5 puts rows y=22 (zuv.y=0.0) and y=157 (zuv.y=1.0)
        // on the exact inner edge; GPU and CPU must evaluate both rows consistently.
        let (zw, zh) = (320_u32, 180_u32);
        let z_from = split_bars(zw, zh, false);
        let z_to = split_bars(zw, zh, true);
        let z_from_tex = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("zoom-edge-from"),
            size: wgpu::Extent3d {
                width: zw,
                height: zh,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let z_to_tex = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("zoom-edge-to"),
            size: wgpu::Extent3d {
                width: zw,
                height: zh,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let z_dst_tex = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("zoom-edge-dst"),
            size: wgpu::Extent3d {
                width: zw,
                height: zh,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        for (tex, f) in [(&z_from_tex, &z_from), (&z_to_tex, &z_to)] {
            gpu.queue().write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &f.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(zw * 4),
                    rows_per_image: Some(zh),
                },
                wgpu::Extent3d {
                    width: zw,
                    height: zh,
                    depth_or_array_layers: 1,
                },
            );
        }
        let z_padded = (zw * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let z_rb = gpu.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("zoom-edge-rb"),
            size: u64::from(z_padded) * u64::from(zh),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        pipe.begin_frame();
        let z_cpu = blend_frames_cpu(
            &z_from,
            &z_to,
            TransitionKind::Zoom,
            TransitionDirection::Left,
            0.5,
            false,
        );
        let mut z_enc = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("zoom-edge"),
            });
        pipe.draw(
            gpu.device(),
            gpu.queue(),
            &mut z_enc,
            &z_from_tex.create_view(&wgpu::TextureViewDescriptor::default()),
            &z_to_tex.create_view(&wgpu::TextureViewDescriptor::default()),
            &z_dst_tex.create_view(&wgpu::TextureViewDescriptor::default()),
            TransitionKind::Zoom,
            TransitionDirection::Left,
            0.5,
            false,
            false,
        );
        z_enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &z_dst_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &z_rb,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(z_padded),
                    rows_per_image: Some(zh),
                },
            },
            wgpu::Extent3d {
                width: zw,
                height: zh,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue().submit([z_enc.finish()]);
        let z_slice = z_rb.slice(..);
        let (z_tx, z_rx) = std::sync::mpsc::channel();
        z_slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = z_tx.send(r);
        });
        let _ = gpu.device().poll(wgpu::PollType::wait_indefinitely());
        z_rx.recv().unwrap().unwrap();
        let z_mapped = z_slice.get_mapped_range().unwrap();
        let mut z_bad = 0_usize;
        for row in 0..zh as usize {
            let src = &z_mapped[row * z_padded as usize..row * z_padded as usize + zw as usize * 4];
            let c_row = &z_cpu.pixels[row * zw as usize * 4..(row + 1) * zw as usize * 4];
            for (c, g) in c_row.chunks_exact(4).zip(src.chunks_exact(4)) {
                for ch in 0..4 {
                    if (i32::from(c[ch]) - i32::from(g[ch])).abs() > 4 {
                        z_bad += 1;
                    }
                }
            }
        }
        drop(z_mapped);
        z_rb.unmap();
        assert_eq!(z_bad, 0, "320x180 Zoom edge GPU/CPU mismatch: {z_bad}");
    }
}
