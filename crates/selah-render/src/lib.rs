#![forbid(unsafe_code)]
//! The compositor interface (ADR-005) and the frame type the engine presents.
pub mod cpu;

use selah_proto::v1::Scene;

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
/// and the wgpu compositor (`gpu`, M1.5).
pub trait Compositor: Send {
    fn name(&self) -> &str;
    fn render(&mut self, scene: &Scene, width: u32, height: u32) -> anyhow::Result<Frame>;
}
