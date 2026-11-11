//! `selah-bench m3`: M3-A6 compositor benchmark measuring **2 × 1080p outputs with a 4K
//! (`3840×2160`) video background** (`OUTPUT_P99_BUDGET_MS = 8.0 ms` on hardware GPUs).
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use selah_media::{DecodeMode, VideoDecoder};
use selah_proto::v1::{
    Color, EffectKind, EffectSpec, LayerKind, LayerSpec, MediaFitMode, MediaLayerSpec, OutputScene,
    PropsLayerSpec, Resolution, Scene, SlideLayerSpec, layer_spec,
};
use selah_render::Compositor;
use selah_render::gpu::{AdapterChoice, GpuCompositor};
use selah_render::template::TemplateStyle;
use selah_render::video::{Nv12Frame, YuvMatrix};
use serde::Serialize;

const VIDEO_W: u32 = 3840;
const VIDEO_H: u32 = 2160;
const OUTPUT_W: u32 = 1920;
const OUTPUT_H: u32 = 1080;
const BENCH_VIDEO_HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
/// Software/virtual GPUs (WARP, lavapipe, VM Metal) emulate 4K NV12 bilinear sampling and
/// full-screen effect shaders on the CPU/paravirt path in CI (~1.3–2.5 s/frame). Keep
/// `hardware_pass` strictly at ARCHITECTURE §12 (8.0 ms) while catching catastrophic CI stalls.
const SOFTWARE_CI_CEILING_MS: f64 = 5_000.0;

#[derive(Serialize)]
pub struct M3Report {
    pub benchmark: &'static str,
    pub os: &'static str,
    pub adapter: String,
    pub software_adapter: bool,
    pub source: String,
    pub decode: String,
    pub outputs: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub video_width: u32,
    pub video_height: u32,
    pub frames: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub hardware_budget_p99_ms: f64,
    pub hardware_pass: bool,
    pub budget_p99_ms: f64,
    pub pass: bool,
}

pub struct Options {
    pub clip: Option<PathBuf>,
    pub mode: DecodeMode,
    pub gpu: AdapterChoice,
    pub iterations: usize,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn synthetic_4k_frames(count: usize) -> Vec<Nv12Frame> {
    let w = VIDEO_W as usize;
    let h = VIDEO_H as usize;
    let cw = VIDEO_W.div_ceil(2) as usize;
    let ch = VIDEO_H.div_ceil(2) as usize;
    let variants = count.clamp(2, 8);
    (0..variants)
        .map(|variant| {
            let mut y = vec![0_u8; w * h];
            let mut uv = vec![0_u8; cw * ch * 2];
            let phase = variant * 29;
            let row_pattern: Vec<u8> = (0..w)
                .map(|x| (24 + ((x / 8 + phase) % 190)) as u8)
                .collect();
            for row in 0..h {
                let dst = &mut y[row * w..(row + 1) * w];
                dst.copy_from_slice(&row_pattern);
            }
            let u_val = (96 + ((variant * 17) % 64)) as u8;
            let v_val = (148_usize.saturating_sub((variant * 11) % 48)) as u8;
            for pair in uv.chunks_exact_mut(2) {
                pair[0] = u_val;
                pair[1] = v_val;
            }
            Nv12Frame {
                width: VIDEO_W,
                height: VIDEO_H,
                y,
                uv,
                matrix: YuvMatrix::Bt709,
                full_range: false,
            }
        })
        .collect()
}

fn load_4k_frames(options: &Options) -> Result<(Vec<Nv12Frame>, String, String)> {
    if let Some(clip_path) = options.clip.as_ref().filter(|p| p.is_file()) {
        let mut decoder = VideoDecoder::open(clip_path, options.mode)
            .with_context(|| format!("open {}", clip_path.display()))?;
        let decode_mode = decoder.hardware().unwrap_or("software").to_owned();
        if decoder.width() >= VIDEO_W && decoder.height() >= VIDEO_H {
            let target = options.iterations.clamp(1, 30);
            let mut frames = Vec::with_capacity(target);
            while frames.len() < target {
                match decoder.next_frame()? {
                    Some(frame) => frames.push(frame.into_nv12()),
                    None => break,
                }
            }
            if !frames.is_empty() {
                let name = clip_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "clip".into());
                return Ok((frames, format!("clip:{name}"), decode_mode));
            }
        }
    }
    Ok((
        synthetic_4k_frames(8),
        "synthetic-4k-nv12".into(),
        "synthetic".into(),
    ))
}

fn benchmark_scene() -> Result<Scene> {
    let main_style = TemplateStyle::preset_sanctuary().to_json()?;
    let stage_style = TemplateStyle::preset_stage_monitor().to_json()?;

    let video_bg = LayerSpec {
        kind: LayerKind::Media as i32,
        enabled: true,
        content: Some(layer_spec::Content::Media(MediaLayerSpec {
            content_hash: BENCH_VIDEO_HASH.into(),
            media_kind: "video".into(),
            media_extension: "mp4".into(),
            fit: MediaFitMode::Fill as i32,
            use_opacity: true,
            opacity: 1.0,
            effects: vec![EffectSpec {
                kind: EffectKind::Vignette as i32,
                enabled: true,
                intensity: 0.45,
                params_json: String::new(),
            }],
            ..Default::default()
        })),
        ..Default::default()
    };

    let main_slide = LayerSpec {
        kind: LayerKind::Slide as i32,
        enabled: true,
        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
            title_text: "Amazing Grace · Verse 1".into(),
            reference_text: "John Newton".into(),
            text: "Amazing grace! How sweet the sound\nThat saved a wretch like me".into(),
            footer_text: "Public Domain · CCLI 22025".into(),
            style_json: main_style,
            ..Default::default()
        })),
        ..Default::default()
    };

    let main_props = LayerSpec {
        kind: LayerKind::Props as i32,
        enabled: true,
        content: Some(layer_spec::Content::Props(PropsLayerSpec {
            label: "LIVE".into(),
            background_color: Some(Color {
                red: 0.06,
                green: 0.14,
                blue: 0.28,
                alpha: 0.80,
            }),
            text_color: Some(Color {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                alpha: 1.0,
            }),
            ..Default::default()
        })),
        ..Default::default()
    };

    let stage_video_bg = LayerSpec {
        kind: LayerKind::Media as i32,
        enabled: true,
        content: Some(layer_spec::Content::Media(MediaLayerSpec {
            content_hash: BENCH_VIDEO_HASH.into(),
            media_kind: "video".into(),
            media_extension: "mp4".into(),
            fit: MediaFitMode::Fill as i32,
            use_opacity: true,
            opacity: 0.35,
            solid_color: Some(Color {
                red: 0.05,
                green: 0.06,
                blue: 0.09,
                alpha: 1.0,
            }),
            ..Default::default()
        })),
        ..Default::default()
    };

    let stage_slide = LayerSpec {
        kind: LayerKind::Slide as i32,
        enabled: true,
        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
            title_text: "Stage · Verse 1".into(),
            text: "Amazing grace! How sweet the sound\nThat saved a wretch like me".into(),
            translation_text: "Oore-ọ̀fẹ́ àrà ọ̀tọ̀!".into(),
            footer_text: "Next: I once was lost".into(),
            chords_json: r#"[{"line":0,"position":0,"text":"G"},{"line":0,"position":19,"text":"C"},{"line":1,"position":18,"text":"D"}]"#.into(),
            style_json: stage_style,
            ..Default::default()
        })),
        ..Default::default()
    };

    Ok(Scene {
        revision: 1,
        outputs: vec![
            OutputScene {
                output_index: 0,
                resolution: Some(Resolution {
                    width: OUTPUT_W,
                    height: OUTPUT_H,
                }),
                layers: vec![video_bg, main_slide, main_props],
                ..Default::default()
            },
            OutputScene {
                output_index: 1,
                resolution: Some(Resolution {
                    width: OUTPUT_W,
                    height: OUTPUT_H,
                }),
                layers: vec![stage_video_bg, stage_slide],
                ..Default::default()
            },
        ],
        ..Default::default()
    })
}

pub fn default_4k_clip_in_repo() -> Option<PathBuf> {
    let rel = Path::new("tests/fixtures/media/generated/hevc-2160p60.mp4");
    rel.is_file().then(|| rel.to_path_buf())
}

pub fn run(options: &Options) -> Result<M3Report> {
    let (frames, source, decode) = load_4k_frames(options)?;
    let scene = benchmark_scene()?;
    let mut gpu = GpuCompositor::new(options.gpu)?;
    let adapter = gpu.adapter_info().name.clone();
    let software_adapter = gpu.is_software_adapter();
    let ci_software_mode =
        options.gpu == AdapterChoice::Software || software_adapter || gpu.is_virtual_adapter();

    // Warm up text caches, pipelines, scratch buffers, and both 1080p render targets before timing.
    gpu.set_video_for_hash(BENCH_VIDEO_HASH, Some(frames[0].clone()))?;
    let bench_outputs = [(0, OUTPUT_W, OUTPUT_H), (1, OUTPUT_W, OUTPUT_H)];
    gpu.composite_outputs(&scene, &bench_outputs)?;

    let mut samples = Vec::with_capacity(options.iterations);
    for idx in 0..options.iterations {
        // Prepare frame ownership outside the timed GPU submission/render span so memory copy of
        // the synthetic test buffer doesn't skew compositor measurement.
        let frame = frames[idx % frames.len()].clone();
        let started = Instant::now();
        gpu.set_video_for_hash(BENCH_VIDEO_HASH, Some(frame))?;
        gpu.composite_outputs(&scene, &bench_outputs)?;
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }

    samples.sort_by(f64::total_cmp);
    let p50 = percentile(&samples, 50.0);
    let p95 = percentile(&samples, 95.0);
    let p99 = percentile(&samples, 99.0);
    let max = *samples.last().unwrap_or(&0.0);
    let mean = samples.iter().sum::<f64>() / samples.len().max(1) as f64;
    let hw_budget = crate::OUTPUT_P99_BUDGET_MS;
    let budget = if ci_software_mode {
        SOFTWARE_CI_CEILING_MS
    } else {
        hw_budget
    };

    Ok(M3Report {
        benchmark: "m3-two-1080p-4k-video",
        os: std::env::consts::OS,
        adapter,
        software_adapter,
        source,
        decode,
        outputs: 2,
        output_width: OUTPUT_W,
        output_height: OUTPUT_H,
        video_width: VIDEO_W,
        video_height: VIDEO_H,
        frames: options.iterations,
        p50_ms: p50,
        p95_ms: p95,
        p99_ms: p99,
        max_ms: max,
        mean_ms: mean,
        hardware_budget_p99_ms: hw_budget,
        hardware_pass: p99 <= hw_budget,
        budget_p99_ms: budget,
        pass: p99 <= budget,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_computes_expected_ranks() {
        let sorted = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&[], 99.0), 0.0);
        assert_eq!(percentile(&sorted, 50.0), 3.0);
        assert_eq!(percentile(&sorted, 99.0), 5.0);
    }

    #[test]
    fn synthetic_4k_frames_validate_cleanly() {
        let frames = synthetic_4k_frames(3);
        assert_eq!(frames.len(), 3);
        for f in &frames {
            assert_eq!((f.width, f.height), (3840, 2160));
            f.validate().expect("synthetic 4K NV12 frame validates");
        }
    }

    #[test]
    fn m3_benchmark_composites_two_1080p_outputs_over_4k_video() {
        let report = run(&Options {
            clip: None,
            mode: DecodeMode::Software,
            gpu: AdapterChoice::Software,
            iterations: 2,
        })
        .expect("M3 benchmark renders on software adapter");
        assert_eq!(report.outputs, 2);
        assert_eq!((report.output_width, report.output_height), (1920, 1080));
        assert_eq!((report.video_width, report.video_height), (3840, 2160));
        assert_eq!(report.frames, 2);
        assert!(report.p99_ms > 0.0);
        assert_eq!(report.hardware_budget_p99_ms, crate::OUTPUT_P99_BUDGET_MS);
        assert!(report.pass, "unexpected CI failure: p99={}", report.p99_ms);
    }
}
