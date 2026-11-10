//! Output windows' swap chains and the preview ring.
use std::time::Duration;

use selah_render::Frame;
use selah_render::blit::Blitter;
use selah_render::gpu::GpuCompositor;

/// Sent from the window thread to the render thread.
pub enum OutputMessage {
    Add {
        id: u32,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    },
    Resize {
        id: u32,
        width: u32,
        height: u32,
    },
}

pub struct OutputSurface {
    id: u32,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    view_format: wgpu::TextureFormat,
    blitter: Blitter,
    reconfigure: bool,
}

/// Pick a non-sRGB format so the composited values reach the screen unchanged. If the
/// surface offers only sRGB formats, render through a non-sRGB view of it.
fn choose_formats(
    caps: &wgpu::SurfaceCapabilities,
) -> Option<(wgpu::TextureFormat, wgpu::TextureFormat)> {
    let linear = caps.formats.iter().copied().find(|f| !f.is_srgb());
    match linear {
        Some(format) => Some((format, format)),
        None => caps
            .formats
            .first()
            .map(|format| (*format, format.remove_srgb_suffix())),
    }
}

pub fn apply_output_message(
    gpu: &mut GpuCompositor,
    surfaces: &mut Vec<OutputSurface>,
    message: OutputMessage,
) {
    match message {
        OutputMessage::Add {
            id,
            surface,
            width,
            height,
        } => {
            let caps = surface.get_capabilities(gpu.adapter());
            let Some((format, view_format)) = choose_formats(&caps) else {
                tracing::error!(id, "the output window has no usable surface format");
                return;
            };
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: width.max(1),
                height: height.max(1),
                // Fifo is vsync: presents land on the display's refresh.
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 1,
                alpha_mode: caps
                    .alpha_modes
                    .first()
                    .copied()
                    .unwrap_or(wgpu::CompositeAlphaMode::Auto),
                // Plain sRGB: the composited values are shown as they are.
                color_space: wgpu::SurfaceColorSpace::Auto,
                view_formats: if view_format == format {
                    Vec::new()
                } else {
                    vec![view_format]
                },
            };
            surface.configure(gpu.device(), &config);
            let blitter = Blitter::new(gpu.device(), view_format);
            tracing::info!(id, ?format, width, height, "output window ready");
            surfaces.retain(|existing| existing.id != id);
            surfaces.push(OutputSurface {
                id,
                surface,
                config,
                view_format,
                blitter,
                reconfigure: false,
            });
        }
        OutputMessage::Resize { id, width, height } => {
            if let Some(output) = surfaces.iter_mut().find(|s| s.id == id) {
                output.config.width = width.max(1);
                output.config.height = height.max(1);
                output.surface.configure(gpu.device(), &output.config);
            }
        }
    }
}

impl OutputSurface {
    /// Blit the composited output to this window and present it. False if nothing was shown.
    pub fn present(&mut self, gpu: &GpuCompositor) -> bool {
        let Some(source) = gpu.output_view_for(self.id) else {
            return false;
        };
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                // Show this frame, then reconfigure for the next one.
                self.reconfigure = true;
                texture
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(gpu.device(), &self.config);
                return false;
            }
            // Minimised or covered windows, and timeouts: try again on the next frame.
            _ => return false,
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view_format),
            ..Default::default()
        });
        let mut encoder = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("present"),
            });
        self.blitter.draw(gpu.device(), &mut encoder, source, &view);
        gpu.queue().submit([encoder.finish()]);
        gpu.queue().present(texture);
        if std::mem::take(&mut self.reconfigure) {
            self.surface.configure(gpu.device(), &self.config);
        }
        true
    }
}

/// Size of the preview frames the UI reads (16:9).
pub const PREVIEW_WIDTH: u32 = 480;
pub const PREVIEW_HEIGHT: u32 = 270;

/// Writes downscaled copies of output 0 to the shared-memory ring for the UI (ADR-004).
pub struct PreviewWriter {
    ring: Option<selah_ipc::shm::RingWriter>,
}

impl PreviewWriter {
    /// `<runtime>/<namespace>-preview-0.ring`. Previews are optional: a failure is logged.
    pub fn open() -> Self {
        let ring = selah_ipc::runtime_dir().and_then(|dir| {
            let namespace = selah_ipc::Endpoint::for_current_user(selah_ipc::Service::Engine)
                .namespace()
                .to_owned();
            selah_ipc::shm::RingWriter::create(
                &dir.join(format!("{namespace}-preview-0.ring")),
                3,
                u64::from(PREVIEW_WIDTH * PREVIEW_HEIGHT * 4),
            )
        });
        match ring {
            Ok(ring) => Self { ring: Some(ring) },
            Err(error) => {
                tracing::warn!(%error, "preview ring unavailable; the UI will show no preview");
                Self { ring: None }
            }
        }
    }

    pub fn publish(&mut self, frame: &Frame, at: Duration) {
        let Some(ring) = self.ring.as_mut() else {
            return;
        };
        let pixels = downscale_bgra(frame, PREVIEW_WIDTH, PREVIEW_HEIGHT);
        if let Err(error) = ring.publish(
            PREVIEW_WIDTH,
            PREVIEW_HEIGHT,
            PREVIEW_WIDTH * 4,
            at.as_nanos() as u64,
            &pixels,
        ) {
            tracing::warn!(%error, "couldn't publish a preview frame");
        }
    }
}

/// Nearest-neighbour downscale of an RGBA frame into BGRA.
pub fn downscale_bgra(frame: &Frame, width: u32, height: u32) -> Vec<u8> {
    let mut out = vec![0_u8; (width * height * 4) as usize];
    for y in 0..height {
        let sy = (u64::from(y) * u64::from(frame.height) / u64::from(height)) as usize;
        for x in 0..width {
            let sx = (u64::from(x) * u64::from(frame.width) / u64::from(width)) as usize;
            let from = (sy * frame.width as usize + sx) * 4;
            let to = ((y * width + x) * 4) as usize;
            let [r, g, b, a] = [
                frame.pixels[from],
                frame.pixels[from + 1],
                frame.pixels[from + 2],
                frame.pixels[from + 3],
            ];
            out[to..to + 4].copy_from_slice(&[b, g, r, a]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscale_keeps_corners_and_swaps_to_bgra() {
        let mut frame = Frame::new(4, 2);
        frame.pixels[0..4].copy_from_slice(&[10, 20, 30, 255]);
        let last = frame.pixels.len() - 4;
        frame.pixels[last..].copy_from_slice(&[1, 2, 3, 255]);
        let small = downscale_bgra(&frame, 2, 1);
        assert_eq!(&small[0..4], &[30, 20, 10, 255]);
    }
}
