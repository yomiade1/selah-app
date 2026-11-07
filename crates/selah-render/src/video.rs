//! The video layer (M1.7): decoded NV12 frames are uploaded as two textures (luma `R8`,
//! chroma `Rg8`) and converted to RGB in `video.wgsl`, under the text layer.
//!
//! The decoder lives in `selah-media`; this module only knows the frame layout, so the render
//! crate doesn't depend on FFmpeg.
use crate::Frame;

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
}

/// One pixel, the same maths as `video.wgsl`.
pub fn yuv_to_rgb(y8: u8, u8_: u8, v8: u8, matrix: YuvMatrix, full_range: bool) -> [u8; 3] {
    let (y8, u8_, v8) = (f32::from(y8), f32::from(u8_), f32::from(v8));
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
    [r, g, b].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The CPU reference: draw `video` over the whole frame (nearest sample, opaque).
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

struct Planes {
    width: u32,
    height: u32,
    luma: wgpu::Texture,
    chroma: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

/// The GPU side: textures, the conversion pipeline and its parameters.
pub struct VideoLayer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    planes: Option<Planes>,
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
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
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
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video"),
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
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video-params"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            params,
            planes: None,
        }
    }

    /// Upload a frame's planes (textures are reused while the size stays the same).
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        video: &Nv12Frame,
        out_width: u32,
        out_height: u32,
    ) -> anyhow::Result<()> {
        video.validate()?;
        let (cw, ch) = (video.chroma_width(), video.chroma_height());
        if self
            .planes
            .as_ref()
            .is_none_or(|p| p.width != video.width || p.height != video.height)
        {
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
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("video"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(
                            &luma.create_view(&wgpu::TextureViewDescriptor::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(
                            &chroma.create_view(&wgpu::TextureViewDescriptor::default()),
                        ),
                    },
                ],
            });
            self.planes = Some(Planes {
                width: video.width,
                height: video.height,
                luma,
                chroma,
                bind_group,
            });
        }
        let planes = self.planes.as_ref().expect("planes exist");
        write_plane(
            queue,
            &planes.luma,
            &video.y,
            video.width,
            video.width,
            video.height,
        );
        write_plane(queue, &planes.chroma, &video.uv, cw * 2, cw, ch);
        let words = [
            out_width,
            out_height,
            video.width,
            video.height,
            u32::from(video.matrix == YuvMatrix::Bt709),
            u32::from(video.full_range),
            0,
            0,
        ];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        queue.write_buffer(&self.params, 0, &bytes);
        Ok(())
    }

    /// Draw the last uploaded frame over the whole target.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(planes) = self.planes.as_ref() else {
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &planes.bind_group, &[]);
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
        // BT.709 limited-range 75% red is Y 51, Cb 109, Cr 212 → about (191, 0, 0).
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
}
