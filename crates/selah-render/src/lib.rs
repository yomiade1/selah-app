#![forbid(unsafe_code)]
//! The compositor interface (ADR-005) and the frame type the engine presents.
pub mod blit;
pub mod cpu;
pub mod gpu;
pub mod png;
pub mod scene;
pub mod text;
pub mod video;

use selah_proto::v1::{PreloadHint, Scene};

/// One rendered output frame: tightly packed RGBA8, premultiplied alpha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
        }
    }

    /// BLAKE3 of the pixels, used by `FrameShown` and the continuity tests.
    pub fn checksum(&self) -> [u8; 32] {
        *blake3::hash(&self.pixels).as_bytes()
    }

    /// The same frame as BGRA, the format of the preview rings.
    pub fn to_bgra(&self) -> Vec<u8> {
        let mut out = self.pixels.clone();
        for pixel in out.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        out
    }
}

/// Renders a scene for one output. Implementations: [`cpu::CpuCompositor`] (always available)
/// and the wgpu compositor (`gpu`, M1.5). Not `Send`: Skia's text objects stay on the thread
/// that made them, so a compositor is created on its render thread.
pub trait Compositor {
    fn name(&self) -> &str;
    fn render(&mut self, scene: &Scene, width: u32, height: u32) -> anyhow::Result<Frame>;

    /// Render the normalized layer stack for `output_index` at (`width`, `height`).
    fn render_output(
        &mut self,
        scene: &Scene,
        output_index: u32,
        width: u32,
        height: u32,
    ) -> anyhow::Result<Frame> {
        let _ = output_index;
        self.render(scene, width, height)
    }

    /// Warm text and overlay caches for upcoming slides without affecting active outputs.
    fn prerasterise_hints(&mut self, _hints: &[PreloadHint]) -> anyhow::Result<usize> {
        Ok(0)
    }

    /// Cumulative `(hits, misses)` across text and overlay raster caches.
    fn cache_stats(&self) -> (u64, u64) {
        (0, 0)
    }

    /// The video frame drawn under the text on the next renders (`None` removes it). The video
    /// layer replaces the test pattern; a cleared scene shows no video.
    fn set_video(&mut self, frame: Option<video::Nv12Frame>) -> anyhow::Result<()>;

    /// The GPU compositor, when this is one (windows can only present GPU output).
    fn as_gpu(&mut self) -> Option<&mut gpu::GpuCompositor> {
        None
    }

    /// When the last frame finished rendering, if the compositor knows.
    fn last_rendered(&self) -> Option<std::time::Instant> {
        None
    }

    /// When the last frame was handed to the GPU queue, if the compositor knows.
    fn last_submitted(&self) -> Option<std::time::Instant> {
        None
    }
}
