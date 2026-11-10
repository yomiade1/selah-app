//! `selah-bench video`: decode → upload → composite under text, paced by a real-time clock at the
//! clip's frame rate (M1.7). A decode thread runs up to [`QUEUE`] frames ahead, as the engine's
//! player will; a frame that isn't ready by its tick is a drop.
//!
//! Cloud sessions and CI decode in software on a software GPU; hardware decode is measured on a
//! real PC with **Help ▸ Diagnostics ▸ Run benchmark** (M1.13), which runs this with a 4K60
//! HEVC clip.
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use selah_media::{DecodeMode, VideoDecoder};
use selah_proto::v1::{Scene, TestPattern};
use selah_render::Compositor;
use selah_render::gpu::{AdapterChoice, GpuCompositor};
use serde::Serialize;

/// Frames decoded ahead of the clock.
const QUEUE: usize = 4;

#[derive(Serialize)]
pub struct VideoReport {
    pub benchmark: &'static str,
    pub os: &'static str,
    pub clip: String,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub decode: String,
    pub adapter: String,
    /// Frames shown.
    pub frames: usize,
    /// Decoded frames that were never shown (a newer one was already due).
    pub dropped: usize,
    /// Ticks where the next frame wasn't decoded in time, so the previous one stayed up.
    pub late_ticks: usize,
    pub decode_p50_ms: f64,
    pub decode_p99_ms: f64,
    pub composite_p50_ms: f64,
    pub composite_p99_ms: f64,
    pub composite_max_ms: f64,
    /// ARCHITECTURE §12 (the full budget is 2 outputs + 4K60 video on reference hardware).
    pub budget_composite_p99_ms: f64,
    pub pass: bool,
}

pub struct Options {
    pub clip: PathBuf,
    pub mode: DecodeMode,
    pub loops: usize,
    pub gpu: AdapterChoice,
}

struct Decoded {
    frame: selah_render::video::Nv12Frame,
    /// Seconds from the start of the run (loops add the clip length).
    due: f64,
    decode_ms: f64,
}

fn percentile(samples: &mut [f64], p: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.sort_by(f64::total_cmp);
    let rank = ((p / 100.0) * (samples.len() - 1) as f64).round() as usize;
    samples[rank.min(samples.len() - 1)]
}

/// Decodes the clip `loops` times; the clock adds each lap's offset to `due`.
fn decode_thread(
    clip: PathBuf,
    mode: DecodeMode,
    loops: usize,
    out: mpsc::SyncSender<Decoded>,
) -> Result<()> {
    for _ in 0..loops {
        let mut decoder = VideoDecoder::open(&clip, mode)?;
        loop {
            let started = Instant::now();
            let Some(frame) = decoder.next_frame()? else {
                break;
            };
            let decode_ms = started.elapsed().as_secs_f64() * 1000.0;
            let decoded = Decoded {
                due: frame.seconds,
                frame: frame.into_nv12(),
                decode_ms,
            };
            if out.send(decoded).is_err() {
                return Ok(());
            }
        }
    }
    Ok(())
}

pub fn run(options: &Options) -> Result<VideoReport> {
    let probe = VideoDecoder::open(&options.clip, options.mode)
        .with_context(|| format!("open {}", options.clip.display()))?;
    let (width, height, codec) = (
        probe.width(),
        probe.height(),
        probe.decoder_name().to_owned(),
    );
    let fps = if probe.frame_rate() > 0.0 {
        probe.frame_rate()
    } else {
        60.0
    };
    let decode = probe.hardware().unwrap_or("software").to_owned();
    drop(probe);
    let period = 1.0 / fps;

    let mut gpu = GpuCompositor::new(options.gpu)?;
    let adapter = gpu.adapter_info().name.clone();
    let scene = Scene {
        revision: 1,
        test_pattern: TestPattern::Unspecified as i32,
        text: "Amazing grace, how sweet the sound".into(),
        cleared: false,
        resolution: None,
        ..Default::default()
    };
    // Warm up: shader compilation and the text layer aren't part of the steady state.
    gpu.set_video(None)?;
    gpu.composite(&scene, width, height)?;

    let (sender, frames) = mpsc::sync_channel(QUEUE);
    let decoder = {
        let (clip, mode, loops) = (options.clip.clone(), options.mode, options.loops.max(1));
        std::thread::spawn(move || decode_thread(clip, mode, loops, sender))
    };
    // Let the decoder fill its queue before the clock starts, as a player does.
    std::thread::sleep(Duration::from_millis(200));

    let mut decode_ms = Vec::new();
    let mut composite_ms = Vec::new();
    let mut dropped = 0_usize;
    let mut late_ticks = 0_usize;
    let mut shown = 0_usize;
    let mut tick = 0_u64;
    let mut lap_offset = 0.0_f64;
    let mut last_due = f64::NEG_INFINITY;
    // A decoded frame that isn't due yet waits here for its tick.
    let mut pending: Option<Decoded> = None;
    let mut finished = false;
    let start = Instant::now();
    'clock: loop {
        let deadline = start + Duration::from_secs_f64(tick as f64 * period);
        if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        let now = tick as f64 * period;
        // The newest frame that's due; an older due frame that was never shown is a drop.
        let mut chosen: Option<Decoded> = None;
        loop {
            let next = match pending.take() {
                Some(decoded) => decoded,
                None => match frames.try_recv() {
                    Ok(mut decoded) => {
                        if decoded.due + 1e-6 < last_due {
                            // The next lap started: shift its times after the previous lap.
                            lap_offset += last_due + period - decoded.due;
                        }
                        last_due = decoded.due;
                        decoded.due += lap_offset;
                        decode_ms.push(decoded.decode_ms);
                        decoded
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        finished = true;
                        break;
                    }
                },
            };
            if next.due > now + period / 2.0 {
                pending = Some(next);
                break;
            }
            if chosen.replace(next).is_some() {
                dropped += 1;
            }
        }
        if finished && chosen.is_none() && pending.is_none() {
            break 'clock;
        }
        match chosen {
            Some(decoded) => {
                let started = Instant::now();
                gpu.set_video(Some(decoded.frame))?;
                gpu.composite(&scene, width, height)?;
                composite_ms.push(started.elapsed().as_secs_f64() * 1000.0);
                shown += 1;
            }
            // Nothing decoded in time for this tick (a frame was due but isn't ready).
            None if pending.is_none() && !finished => late_ticks += 1,
            None => {}
        }
        tick += 1;
        if tick > 100_000 {
            bail!("the clip didn't end");
        }
    }
    decoder
        .join()
        .map_err(|_| anyhow::anyhow!("the decode thread panicked"))??;
    let budget = crate::OUTPUT_P99_BUDGET_MS;
    let composite_p99 = percentile(&mut composite_ms, 99.0);
    Ok(VideoReport {
        benchmark: "video-decode-present",
        os: std::env::consts::OS,
        clip: clip_name(&options.clip),
        codec,
        width,
        height,
        fps,
        decode,
        adapter,
        frames: shown,
        dropped,
        late_ticks,
        decode_p50_ms: percentile(&mut decode_ms, 50.0),
        decode_p99_ms: percentile(&mut decode_ms, 99.0),
        composite_p50_ms: percentile(&mut composite_ms, 50.0),
        composite_max_ms: composite_ms.last().copied().unwrap_or(0.0),
        composite_p99_ms: composite_p99,
        budget_composite_p99_ms: budget,
        pass: dropped == 0 && late_ticks == 0 && composite_p99 <= budget,
    })
}

/// The file name only: reports are attached to public issues, and paths contain user names.
fn clip_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
