//! A small CPU compositor. Used as the fallback when no GPU adapter is available, and as the
//! reference the GPU test patterns are compared with.
use std::collections::HashMap;
use std::path::PathBuf;

use selah_proto::v1::{LayerKind, MediaFitMode, Scene, TestPattern, layer_spec};

use crate::{Compositor, Frame};

/// SMPTE-style 75% colour bars (top two-thirds), as straight RGB.
pub const BARS: [[u8; 3]; 7] = [
    [191, 191, 191],
    [191, 191, 0],
    [0, 191, 191],
    [0, 191, 0],
    [191, 0, 191],
    [191, 0, 0],
    [0, 0, 191],
];

/// The colour of a test-pattern pixel. `x` and `y` are in `0..width` and `0..height`.
pub fn pattern_pixel(pattern: TestPattern, x: u32, y: u32, width: u32, height: u32) -> [u8; 4] {
    match pattern {
        TestPattern::SmpteBars => {
            let bar = (x * 7 / width.max(1)).min(6) as usize;
            if y < height * 2 / 3 {
                let [r, g, b] = BARS[bar];
                [r, g, b, 255]
            } else if y < height * 3 / 4 {
                // Reverse bars strip: blue, black, magenta, black, cyan, black, grey.
                let [r, g, b] = if bar % 2 == 1 {
                    [0, 0, 0]
                } else {
                    BARS[6 - bar]
                };
                [r, g, b, 255]
            } else {
                // Bottom: a white patch then black, so a clipped output is easy to see.
                let v = if x < width / 4 { 255 } else { 16 };
                [v, v, v, 255]
            }
        }
        TestPattern::Grid => {
            let step = (height / 9).max(1);
            let line = x % step < 2 || y % step < 2 || x + 2 >= width || y + 2 >= height;
            if line {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        }
        TestPattern::Gradient => {
            let v = (x * 255 / width.saturating_sub(1).max(1)) as u8;
            [v, v, v, 255]
        }
        TestPattern::Unspecified => [0, 0, 0, 255],
    }
}

pub struct CpuCompositor {
    text: Option<crate::text::TextRenderer>,
    video: Option<crate::video::Nv12Frame>,
    video_frames: HashMap<String, crate::video::Nv12Frame>,
    overlays: crate::scene::OverlayCache,
    media_root: Option<PathBuf>,
}

impl Default for CpuCompositor {
    fn default() -> Self {
        Self {
            text: None,
            video: None,
            video_frames: HashMap::new(),
            overlays: crate::scene::OverlayCache::default(),
            media_root: crate::media_bg::default_media_root_from_env(),
        }
    }
}

impl CpuCompositor {
    fn draw_text(&mut self, scene: &Scene, frame: &mut Frame) -> anyhow::Result<()> {
        if scene.text.is_empty() {
            return Ok(());
        }
        if self.text.is_none() {
            self.text = Some(crate::text::TextRenderer::bundled()?);
        }
        let renderer = self.text.as_mut().expect("text renderer exists");
        let layer = renderer.render(
            &scene.text,
            &crate::text::SlideTextStyle::for_output(frame.height),
            frame.width,
            frame.height,
        )?;
        crate::text::composite_over(frame, &layer);
        Ok(())
    }
}

impl Compositor for CpuCompositor {
    fn name(&self) -> &str {
        "cpu"
    }

    fn prerasterise_hints(
        &mut self,
        hints: &[selah_proto::v1::PreloadHint],
    ) -> anyhow::Result<usize> {
        crate::scene::prerasterise_hints(hints, &mut self.text, &mut self.overlays)
    }

    fn cache_stats(&self) -> (u64, u64) {
        let (th, tm) = self
            .text
            .as_ref()
            .map_or((0, 0), |t| (t.cache_hits(), t.cache_misses()));
        (th + self.overlays.hits(), tm + self.overlays.misses())
    }

    fn set_video(&mut self, frame: Option<crate::video::Nv12Frame>) -> anyhow::Result<()> {
        if let Some(frame) = frame.as_ref() {
            frame.validate()?;
        }
        self.video = frame;
        Ok(())
    }

    fn set_media_root(&mut self, root: Option<PathBuf>) {
        self.media_root = root;
        self.overlays.clear();
    }

    fn set_video_for_hash(
        &mut self,
        hash: &str,
        frame: Option<crate::video::Nv12Frame>,
    ) -> anyhow::Result<()> {
        match frame {
            Some(frame) => {
                frame.validate()?;
                self.video_frames.insert(hash.to_owned(), frame);
            }
            None => {
                self.video_frames.remove(hash);
            }
        }
        Ok(())
    }

    fn render(&mut self, scene: &Scene, width: u32, height: u32) -> anyhow::Result<Frame> {
        self.render_output(scene, 0, width, height)
    }

    fn render_output(
        &mut self,
        scene: &Scene,
        output_index: u32,
        width: u32,
        height: u32,
    ) -> anyhow::Result<Frame> {
        let normalized = crate::scene::normalize_output_scene(scene, output_index, width, height);
        let mut frame = Frame::new(width, height);
        if normalized.is_legacy {
            let pattern = normalized.test_pattern;
            if !normalized.cleared
                && let Some(video) = self.video.as_ref()
            {
                crate::video::draw_cpu(&mut frame, video);
                self.draw_text(scene, &mut frame)?;
                return Ok(frame);
            }
            if normalized.cleared || pattern == TestPattern::Unspecified {
                for pixel in frame.pixels.chunks_exact_mut(4) {
                    pixel.copy_from_slice(&[0, 0, 0, 255]);
                }
                if !normalized.cleared {
                    self.draw_text(scene, &mut frame)?;
                }
                return Ok(frame);
            }
            for y in 0..height {
                for x in 0..width {
                    let at = (y as usize * width as usize + x as usize) * 4;
                    frame.pixels[at..at + 4]
                        .copy_from_slice(&pattern_pixel(pattern, x, y, width, height));
                }
            }
            self.draw_text(scene, &mut frame)?;
            return Ok(frame);
        }

        if normalized.cleared || normalized.test_pattern == TestPattern::Unspecified {
            for pixel in frame.pixels.chunks_exact_mut(4) {
                pixel.copy_from_slice(&[0, 0, 0, 255]);
            }
        } else {
            for y in 0..height {
                for x in 0..width {
                    let at = (y as usize * width as usize + x as usize) * 4;
                    frame.pixels[at..at + 4].copy_from_slice(&pattern_pixel(
                        normalized.test_pattern,
                        x,
                        y,
                        width,
                        height,
                    ));
                }
            }
        }
        if !normalized.cleared {
            for layer in &normalized.layers {
                if layer.kind == LayerKind::Media
                    && let Some(layer_spec::Content::Media(m)) = layer.spec.content.as_ref()
                    && m.media_kind == "video"
                    && let Some(nv12) = self.video_frames.get(&m.content_hash)
                {
                    if let Some(mut bg) = crate::scene::media_backdrop_frame(
                        m,
                        width,
                        height,
                        self.media_root.as_deref(),
                        false,
                    )? {
                        crate::scene::scale_frame_opacity(&mut bg, layer.opacity);
                        crate::text::composite_over(&mut frame, &bg);
                    }
                    let fit = MediaFitMode::try_from(m.fit).unwrap_or(MediaFitMode::Fill);
                    if m.blur_radius > 0.05 && m.blur_radius.is_finite() {
                        let mut tmp = Frame::new(width, height);
                        crate::video::draw_cpu_layer(&mut tmp, nv12, fit, 1.0)?;
                        let blurred = crate::blur::blur_cpu(&tmp, m.blur_radius, layer.opacity);
                        crate::text::composite_over(&mut frame, &blurred);
                    } else {
                        crate::video::draw_cpu_layer(&mut frame, nv12, fit, layer.opacity)?;
                    }
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
                    crate::text::composite_over(&mut frame, &overlay);
                }
            }
        }
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(pattern: TestPattern, cleared: bool) -> Scene {
        Scene {
            revision: 1,
            test_pattern: pattern as i32,
            text: String::new(),
            cleared,
            resolution: None,
            ..Default::default()
        }
    }

    #[test]
    fn bars_have_seven_columns_in_order() {
        let frame = CpuCompositor::default()
            .render(&scene(TestPattern::SmpteBars, false), 70, 30)
            .unwrap();
        for (bar, expected) in BARS.iter().enumerate() {
            let x = bar as u32 * 10 + 5;
            let at = (2 * 70 + x as usize) * 4;
            assert_eq!(&frame.pixels[at..at + 3], expected);
        }
    }

    #[test]
    fn cleared_scenes_are_black_and_checksums_differ_by_content() {
        let mut cpu = CpuCompositor::default();
        let black = cpu
            .render(&scene(TestPattern::SmpteBars, true), 16, 9)
            .unwrap();
        assert!(black.pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
        let bars = cpu
            .render(&scene(TestPattern::SmpteBars, false), 16, 9)
            .unwrap();
        assert_ne!(black.checksum(), bars.checksum());
        let again = cpu
            .render(&scene(TestPattern::SmpteBars, false), 16, 9)
            .unwrap();
        assert_eq!(bars.checksum(), again.checksum());
    }
}
