//! WCAG 2.3.1 General Flash and Red Flash threshold analyser (SAFE-04 / M3.5 / M3-A8).
//!
//! Shared across:
//! - `selah-media` (bounded offline NV12 import scan)
//! - `selah-engine` (real-time per-output frame stream monitoring)
//! - `selah-core` (transition & media pre-flight risk checks)

use std::collections::VecDeque;
use std::sync::OnceLock;

use selah_proto::v1::{Scene, TransitionSpec};

use crate::cpu::CpuCompositor;
use crate::transitions::{blend_frames_cpu, resolve_transition_spec, transition_progress};
use crate::video::{Nv12Frame, YuvMatrix};
use crate::{Compositor, Frame};

const ONE_SECOND_NS: u64 = 1_000_000_000;
const MAX_SAMPLE_DIM: u32 = 64;

fn srgb_lut() -> &'static [f32; 256] {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut table = [0.0_f32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *slot = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
        }
        table
    })
}

#[derive(Clone, Copy, Debug)]
struct SamplePoint {
    luma: f32,
    red_metric: f32,
    saturated_red: bool,
}

#[inline]
fn sample_from_rgb(lut: &[f32; 256], r: u8, g: u8, b: u8) -> SamplePoint {
    let lr = lut[usize::from(r)];
    let lg = lut[usize::from(g)];
    let lb = lut[usize::from(b)];
    let luma = 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;

    let rf = f32::from(r) / 255.0;
    let gf = f32::from(g) / 255.0;
    let bf = f32::from(b) / 255.0;
    let sum = rf + gf + bf;
    let saturated_red = sum > 1e-4 && (rf / sum) >= 0.80;
    let red_metric = (rf - gf - bf).max(0.0) * 320.0;
    SamplePoint {
        luma,
        red_metric,
        saturated_red,
    }
}

#[inline]
fn nv12_pixel_to_rgb(
    y_raw: u8,
    u_raw: u8,
    v_raw: u8,
    matrix: YuvMatrix,
    full_range: bool,
) -> [u8; 3] {
    let (y, u, v) = if full_range {
        (
            f32::from(y_raw) / 255.0,
            (f32::from(u_raw) - 128.0) / 255.0,
            (f32::from(v_raw) - 128.0) / 255.0,
        )
    } else {
        (
            ((f32::from(y_raw) - 16.0) / 219.0).clamp(0.0, 1.0),
            (f32::from(u_raw) - 128.0) / 224.0,
            (f32::from(v_raw) - 128.0) / 224.0,
        )
    };
    let (r, g, b) = match matrix {
        YuvMatrix::Bt601 => (
            y + 1.402 * v,
            y - 0.344_136 * u - 0.714_136 * v,
            y + 1.772 * u,
        ),
        YuvMatrix::Bt709 => (y + 1.5748 * v, y - 0.1873 * u - 0.4681 * v, y + 1.8556 * u),
    };
    [
        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (b.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

/// Summary returned after feeding a frame into [`FlashAnalyzer`] or analysing a sequence.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlashAnalysis {
    pub flash_warning: bool,
    pub flashes_per_second: f32,
    pub peak_flashes_per_second: f32,
    pub general_flashes_per_second: f32,
    pub red_flashes_per_second: f32,
    pub saturated_red_risk: bool,
    pub warning_codes: Vec<String>,
    pub reason: String,
}

/// Stateful per-stream WCAG 2.3.1 flash monitor operating on strided pixel samples.
pub struct FlashAnalyzer {
    prev_dims: (u32, u32),
    prev_samples: Vec<SamplePoint>,
    last_gen_dir: i8,
    last_red_dir: i8,
    gen_transitions: VecDeque<u64>,
    red_transitions: VecDeque<u64>,
    peak_flashes_per_second: f32,
    ever_triggered: bool,
    ever_red_triggered: bool,
}

impl Default for FlashAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl FlashAnalyzer {
    pub fn new() -> Self {
        Self {
            prev_dims: (0, 0),
            prev_samples: Vec::new(),
            last_gen_dir: 0,
            last_red_dir: 0,
            gen_transitions: VecDeque::new(),
            red_transitions: VecDeque::new(),
            peak_flashes_per_second: 0.0,
            ever_triggered: false,
            ever_red_triggered: false,
        }
    }

    pub fn reset(&mut self) {
        self.prev_dims = (0, 0);
        self.prev_samples.clear();
        self.last_gen_dir = 0;
        self.last_red_dir = 0;
        self.gen_transitions.clear();
        self.red_transitions.clear();
        self.peak_flashes_per_second = 0.0;
        self.ever_triggered = false;
        self.ever_red_triggered = false;
    }

    /// Analyze an RGBA8 premultiplied [`Frame`] at monotonic timestamp `timestamp_ns`.
    pub fn push_frame(&mut self, frame: &Frame, timestamp_ns: u64) -> FlashAnalysis {
        if frame.width == 0
            || frame.height == 0
            || frame.pixels.len() < frame.width as usize * frame.height as usize * 4
        {
            return self.current_summary();
        }
        let cols = frame.width.min(MAX_SAMPLE_DIM);
        let rows = frame.height.min(MAX_SAMPLE_DIM);
        let lut = srgb_lut();
        let mut samples = Vec::with_capacity((cols * rows) as usize);
        for ry in 0..rows {
            let y = (ry * frame.height / rows).min(frame.height - 1) as usize;
            for rx in 0..cols {
                let x = (rx * frame.width / cols).min(frame.width - 1) as usize;
                let idx = (y * frame.width as usize + x) * 4;
                // Unpremultiply so composited dark alpha overlays still measure accurately.
                let a = frame.pixels[idx + 3];
                let (r, g, b) = if a == 255 {
                    (
                        frame.pixels[idx],
                        frame.pixels[idx + 1],
                        frame.pixels[idx + 2],
                    )
                } else if a == 0 {
                    (0, 0, 0)
                } else {
                    let scale = 255.0 / f32::from(a);
                    (
                        (f32::from(frame.pixels[idx]) * scale).clamp(0.0, 255.0) as u8,
                        (f32::from(frame.pixels[idx + 1]) * scale).clamp(0.0, 255.0) as u8,
                        (f32::from(frame.pixels[idx + 2]) * scale).clamp(0.0, 255.0) as u8,
                    )
                };
                samples.push(sample_from_rgb(lut, r, g, b));
            }
        }
        self.push_samples((cols, rows), samples, timestamp_ns)
    }

    /// Analyze a decoded [`Nv12Frame`] directly without allocating an RGBA buffer.
    pub fn push_nv12(&mut self, frame: &Nv12Frame, timestamp_ns: u64) -> FlashAnalysis {
        if frame.width == 0 || frame.height == 0 || frame.validate().is_err() {
            return self.current_summary();
        }
        let cols = frame.width.min(MAX_SAMPLE_DIM);
        let rows = frame.height.min(MAX_SAMPLE_DIM);
        let lut = srgb_lut();
        let chroma_stride = frame.chroma_width() as usize * 2;
        let mut samples = Vec::with_capacity((cols * rows) as usize);
        for ry in 0..rows {
            let y = (ry * frame.height / rows).min(frame.height - 1) as usize;
            let cy = y / 2;
            for rx in 0..cols {
                let x = (rx * frame.width / cols).min(frame.width - 1) as usize;
                let cx = x / 2;
                let y_byte = frame.y[y * frame.width as usize + x];
                let uv_at = cy * chroma_stride + cx * 2;
                let u_byte = frame.uv[uv_at];
                let v_byte = frame.uv[uv_at + 1];
                let [r, g, b] =
                    nv12_pixel_to_rgb(y_byte, u_byte, v_byte, frame.matrix, frame.full_range);
                samples.push(sample_from_rgb(lut, r, g, b));
            }
        }
        self.push_samples((cols, rows), samples, timestamp_ns)
    }

    fn push_samples(
        &mut self,
        dims: (u32, u32),
        samples: Vec<SamplePoint>,
        timestamp_ns: u64,
    ) -> FlashAnalysis {
        if self.prev_dims != dims || self.prev_samples.len() != samples.len() {
            self.prev_dims = dims;
            self.prev_samples = samples;
            self.last_gen_dir = 0;
            self.last_red_dir = 0;
            return self.current_summary();
        }

        let n = samples.len().max(1);
        let mut gen_up = 0_usize;
        let mut gen_down = 0_usize;
        let mut red_up = 0_usize;
        let mut red_down = 0_usize;

        for (prev, cur) in self.prev_samples.iter().zip(&samples) {
            let dl = cur.luma - prev.luma;
            if dl >= 0.10 && prev.luma < 0.80 {
                gen_up += 1;
            } else if -dl >= 0.10 && cur.luma < 0.80 {
                gen_down += 1;
            }

            if cur.saturated_red || prev.saturated_red {
                let dr = cur.red_metric - prev.red_metric;
                if dr >= 20.0 {
                    red_up += 1;
                } else if -dr >= 20.0 {
                    red_down += 1;
                }
            }
        }

        let gen_dir: i8 = if gen_up * 4 >= n && gen_up >= gen_down {
            1
        } else if gen_down * 4 >= n {
            -1
        } else if (gen_up + gen_down) * 4 >= n {
            if self.last_gen_dir == 0 {
                1
            } else {
                -self.last_gen_dir
            }
        } else {
            0
        };

        let red_dir: i8 = if red_up * 4 >= n && red_up >= red_down {
            1
        } else if red_down * 4 >= n {
            -1
        } else if (red_up + red_down) * 4 >= n {
            if self.last_red_dir == 0 {
                1
            } else {
                -self.last_red_dir
            }
        } else {
            0
        };

        let prune_before = timestamp_ns.saturating_sub(ONE_SECOND_NS);
        while self
            .gen_transitions
            .front()
            .is_some_and(|&t| t < prune_before)
        {
            self.gen_transitions.pop_front();
        }
        while self
            .red_transitions
            .front()
            .is_some_and(|&t| t < prune_before)
        {
            self.red_transitions.pop_front();
        }

        if gen_dir != 0 && (self.last_gen_dir == 0 || gen_dir == -self.last_gen_dir) {
            self.gen_transitions.push_back(timestamp_ns);
            self.last_gen_dir = gen_dir;
        }
        if red_dir != 0 && (self.last_red_dir == 0 || red_dir == -self.last_red_dir) {
            self.red_transitions.push_back(timestamp_ns);
            self.last_red_dir = red_dir;
        }

        self.prev_samples = samples;

        let summary = self.current_summary();
        if summary.general_flashes_per_second > 3.0 {
            self.ever_triggered = true;
        }
        if summary.red_flashes_per_second > 3.0 {
            self.ever_triggered = true;
            self.ever_red_triggered = true;
        }
        self.peak_flashes_per_second = self.peak_flashes_per_second.max(summary.flashes_per_second);
        self.current_summary()
    }

    pub fn current_summary(&self) -> FlashAnalysis {
        let gen_fps = self.gen_transitions.len() as f32 * 0.5;
        let red_fps = self.red_transitions.len() as f32 * 0.5;
        let cur_fps = gen_fps.max(red_fps);
        let peak = self.peak_flashes_per_second.max(cur_fps);
        let general_warn = gen_fps > 3.0;
        let red_warn = red_fps > 3.0;
        let flash_warning = general_warn || red_warn;
        let mut codes = Vec::new();
        if general_warn || self.ever_triggered {
            codes.push("WCAG_2_3_1_GENERAL_FLASH".to_owned());
        }
        if red_warn || self.ever_red_triggered {
            codes.push("WCAG_2_3_1_RED_FLASH".to_owned());
        }
        let reason = if red_warn {
            format!(
                "Saturated red flash frequency ({red_fps:.1} flashes/s) exceeds 3 Hz WCAG 2.3.1 limit"
            )
        } else if general_warn {
            format!(
                "Luminance flash frequency ({gen_fps:.1} flashes/s) exceeds 3 Hz WCAG 2.3.1 limit"
            )
        } else {
            String::new()
        };
        FlashAnalysis {
            flash_warning,
            flashes_per_second: cur_fps,
            peak_flashes_per_second: peak,
            general_flashes_per_second: gen_fps,
            red_flashes_per_second: red_fps,
            saturated_red_risk: red_warn || self.ever_red_triggered,
            warning_codes: codes,
            reason,
        }
    }

    pub fn ever_triggered(&self) -> bool {
        self.ever_triggered
    }

    pub fn ever_red_triggered(&self) -> bool {
        self.ever_red_triggered
    }
}

/// Evaluate pre-flight WCAG 2.3.1 flash risk across a candidate transition or repeated cadence.
pub fn analyze_transition_risk(
    transition: Option<&TransitionSpec>,
    repeat_cadence_ms: u32,
    from_scene: Option<&Scene>,
    to_scene: Option<&Scene>,
    flashes_per_second_hint: f32,
    media_flash_warning: bool,
) -> FlashAnalysis {
    let mut analyzer = FlashAnalyzer::new();
    let mut codes = Vec::new();
    let mut reason_parts = Vec::new();
    let mut peak = 0.0_f32;
    let mut red_risk = false;

    if media_flash_warning {
        codes.push("MEDIA_FLASH_WARNING".to_owned());
        reason_parts.push("Assigned media asset carries a WCAG 2.3.1 flashing warning".to_owned());
    }
    if flashes_per_second_hint.is_finite() && flashes_per_second_hint > 3.0 {
        peak = peak.max(flashes_per_second_hint);
        codes.push("WCAG_2_3_1_GENERAL_FLASH".to_owned());
        reason_parts.push(format!(
            "Reported flash rate ({flashes_per_second_hint:.1} flashes/s) exceeds 3 Hz WCAG 2.3.1 limit"
        ));
    }

    if repeat_cadence_ms > 0 {
        let mut cpu = CpuCompositor::default();
        let (w, h) = (64_u32, 36_u32);
        let default_dark = Scene {
            cleared: true,
            ..Default::default()
        };
        let s_from = from_scene.unwrap_or(&default_dark);
        let s_to = to_scene.unwrap_or(&default_dark);
        let f_from = cpu
            .render_output(s_from, 0, w, h)
            .unwrap_or_else(|_| Frame::new(w, h));
        let f_to = cpu
            .render_output(s_to, 0, w, h)
            .unwrap_or_else(|_| Frame::new(w, h));
        let (kind, dir, _, dur_ms) = resolve_transition_spec(transition);

        // Simulate 1.25 seconds at ~60 Hz alternating from <-> to on each `repeat_cadence_ms`.
        let step_ms = 16_u64;
        let cadence = u64::from(repeat_cadence_ms.max(16));
        for t_ms in (0..=1250_u64).step_by(step_ms as usize) {
            let cycle_idx = t_ms / cadence;
            let in_cycle_ms = (t_ms % cadence) as u32;
            let (a, b) = if cycle_idx % 2 == 0 {
                (&f_from, &f_to)
            } else {
                (&f_to, &f_from)
            };
            let p = if dur_ms == 0 {
                1.0
            } else {
                transition_progress(transition, in_cycle_ms.min(dur_ms))
            };
            let blended = blend_frames_cpu(a, b, kind, dir, p, false);
            analyzer.push_frame(&blended, t_ms * 1_000_000);
        }

        let sim = analyzer.current_summary();
        peak = peak.max(sim.peak_flashes_per_second);
        red_risk |= sim.saturated_red_risk;
        for c in sim.warning_codes {
            if !codes.contains(&c) {
                codes.push(c);
            }
        }
        if analyzer.ever_triggered() {
            reason_parts.push(format!(
                "Repeated transition every {repeat_cadence_ms} ms reaches {peak:.1} flashes/s (> 3 Hz WCAG 2.3.1 limit)"
            ));
        }
    }

    let flash_warning =
        media_flash_warning || analyzer.ever_triggered() || (flashes_per_second_hint > 3.0);
    FlashAnalysis {
        flash_warning,
        flashes_per_second: peak,
        peak_flashes_per_second: peak,
        general_flashes_per_second: peak,
        red_flashes_per_second: if red_risk { peak } else { 0.0 },
        saturated_red_risk: red_risk,
        warning_codes: codes,
        reason: reason_parts.join("; "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use selah_proto::v1::{
        Color, LayerKind, LayerSpec, MediaLayerSpec, OutputScene, TransitionKind, layer_spec,
    };

    fn solid_frame(w: u32, h: u32, rgba: [u8; 4]) -> Frame {
        let mut f = Frame::new(w, h);
        for px in f.pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&rgba);
        }
        f
    }

    fn box_frame(w: u32, h: u32, frac_area: f32, fg: [u8; 4], bg: [u8; 4]) -> Frame {
        let mut f = solid_frame(w, h, bg);
        let side = frac_area.clamp(0.0, 1.0).sqrt();
        let bw = ((w as f32) * side).round() as u32;
        let bh = ((h as f32) * side).round() as u32;
        for y in 0..bh.min(h) {
            for x in 0..bw.min(w) {
                let idx = (y as usize * w as usize + x as usize) * 4;
                f.pixels[idx..idx + 4].copy_from_slice(&fg);
            }
        }
        f
    }

    fn color_scene(r: f32, g: f32, b: f32) -> Scene {
        Scene {
            revision: 1,
            outputs: vec![OutputScene {
                output_index: 0,
                layers: vec![LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        solid_color: Some(Color {
                            red: r,
                            green: g,
                            blue: b,
                            alpha: 1.0,
                        }),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn wcag_luminance_rate_and_area_thresholds_hold() {
        let black = solid_frame(64, 36, [0, 0, 0, 255]);
        let white = solid_frame(64, 36, [255, 255, 255, 255]);

        // 1. 6 Hz full-frame black/white strobe triggers flash_warning.
        let mut fast = FlashAnalyzer::new();
        for i in 0..60_u64 {
            let f = if (i / 5) % 2 == 0 { &black } else { &white };
            fast.push_frame(f, i * 16_666_667);
        }
        let s = fast.current_summary();
        assert!(fast.ever_triggered());
        assert!(s.peak_flashes_per_second > 3.0);

        // 2. 2 Hz black/white alternating (<= 3.0 Hz) is SAFE.
        let mut slow = FlashAnalyzer::new();
        for i in 0..60_u64 {
            let f = if (i / 15) % 2 == 0 { &black } else { &white };
            slow.push_frame(f, i * 16_666_667);
        }
        assert!(!slow.ever_triggered());

        // 3. High-luminance min(Y) >= 0.80 flicker does NOT trigger WCAG flash.
        let bright_gray = solid_frame(64, 36, [235, 235, 235, 255]); // sRGB 235 -> Y ~= 0.83
        let mut bright = FlashAnalyzer::new();
        for i in 0..60_u64 {
            let f = if (i / 5) % 2 == 0 {
                &bright_gray
            } else {
                &white
            };
            bright.push_frame(f, i * 16_666_667);
        }
        assert!(!bright.ever_triggered());

        // 4. Tiny 10% box strobe does NOT trigger (< 25% area); 36% box strobe DOES trigger.
        let small = box_frame(64, 36, 0.10, [255, 255, 255, 255], [0, 0, 0, 255]);
        let mut small_an = FlashAnalyzer::new();
        for i in 0..60_u64 {
            let f = if (i / 5) % 2 == 0 { &black } else { &small };
            small_an.push_frame(f, i * 16_666_667);
        }
        assert!(!small_an.ever_triggered());

        let large = box_frame(64, 36, 0.36, [255, 255, 255, 255], [0, 0, 0, 255]);
        let mut large_an = FlashAnalyzer::new();
        for i in 0..60_u64 {
            let f = if (i / 5) % 2 == 0 { &black } else { &large };
            large_an.push_frame(f, i * 16_666_667);
        }
        assert!(large_an.ever_triggered());

        // 5. Saturated red flash triggers saturated_red_risk.
        let red = solid_frame(64, 36, [255, 0, 0, 255]);
        let mut red_an = FlashAnalyzer::new();
        for i in 0..60_u64 {
            let f = if (i / 5) % 2 == 0 { &black } else { &red };
            red_an.push_frame(f, i * 16_666_667);
        }
        assert!(red_an.ever_red_triggered());
    }

    #[test]
    fn transition_risk_catches_unsafe_strobe_and_passes_smooth_fade() {
        let dark = color_scene(0.0, 0.0, 0.0);
        let red = color_scene(1.0, 0.0, 0.0);

        let unsafe_cut = analyze_transition_risk(
            Some(&TransitionSpec {
                kind: TransitionKind::Cut as i32,
                duration_ms: 0,
                ..Default::default()
            }),
            100,
            Some(&dark),
            Some(&red),
            0.0,
            false,
        );
        assert!(unsafe_cut.flash_warning);
        assert!(unsafe_cut.saturated_red_risk);

        let safe_single = analyze_transition_risk(
            Some(&TransitionSpec {
                kind: TransitionKind::Fade as i32,
                duration_ms: 500,
                ..Default::default()
            }),
            0,
            Some(&dark),
            Some(&red),
            0.0,
            false,
        );
        assert!(!safe_single.flash_warning);
    }
}
