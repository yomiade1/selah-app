//! Safe Rust audio decoding, channel mapping, resampling, ITU-R BS.1770 true-peak limiting,
//! master audio clock, and CPAL output with headless fallback (M3.4, SAFE-05, SEC-04, M3-A8).
//!
//! No `unsafe` and zero `swresample` calls (macOS decode-only FFmpeg compatibility).
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat};
use ffmpeg::format::sample::Sample;
use ffmpeg_next as ffmpeg;

use crate::{
    AUDIO_DECODERS, MediaError, Result, check_demuxer, check_local_path, ffmpeg_url, init,
    open_options,
};

/// Legal master true-peak ceiling in dBTP (**SAFE-05**, **M3-A8**).
pub const MASTER_CEILING_DBTP: f32 = -1.0;

/// Linear limiter target (`~ -1.06 dBTP`) giving strict margin below `-1.0 dBTP` even on inter-sample peaks.
const LIMITER_TARGET_LINEAR: f32 = 0.885;

/// Hard linear safety guard (`~ -1.02 dBTP`) post-limiter so transients cannot exceed `-1.0 dBTP`.
const LIMITER_HARD_GUARD: f32 = 0.889;

/// Default output gain (`-1.94 dB`) when a layer does not specify explicit gain (**SAFE-05**).
pub const DEFAULT_SAFE_VOLUME: f32 = 0.80;

/// Maximum allowed user gain before the `-1 dBTP` master limiter.
pub const MAX_USER_VOLUME: f32 = 2.0;

/// Default clear/mute smooth fade duration in milliseconds (**SAFE-05**).
pub const DEFAULT_FADE_MS: u32 = 25;

const BS1770_PHASES: usize = 3;
const BS1770_TAPS: usize = 12;

fn oversample_taps() -> &'static [[f32; BS1770_TAPS]; BS1770_PHASES] {
    static TAPS: OnceLock<[[f32; BS1770_TAPS]; BS1770_PHASES]> = OnceLock::new();
    TAPS.get_or_init(|| {
        let mut table = [[0.0_f32; BS1770_TAPS]; BS1770_PHASES];
        let half = (BS1770_TAPS / 2) as isize;
        for (p_idx, &frac) in [0.25_f64, 0.5_f64, 0.75_f64].iter().enumerate() {
            let mut sum = 0.0_f64;
            let mut raw = [0.0_f64; BS1770_TAPS];
            for (t_idx, slot) in raw.iter_mut().enumerate() {
                let k = (t_idx as isize) - (half - 1);
                let x = frac - (k as f64);
                let sinc = if x.abs() < 1e-9 {
                    1.0
                } else {
                    let px = std::f64::consts::PI * x;
                    px.sin() / px
                };
                // Blackman window over [-5.75 .. +6.25] centered around x = 0.
                let u = ((t_idx as f64) + 0.5) / (BS1770_TAPS as f64);
                let win = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * u).cos()
                    + 0.08 * (4.0 * std::f64::consts::PI * u).cos();
                let v = sinc * win;
                *slot = v;
                sum += v;
            }
            for (dst, v) in table[p_idx].iter_mut().zip(raw) {
                *dst = (v / sum) as f32;
            }
        }
        table
    })
}

/// Measure ITU-R BS.1770-style 4x oversampled true peak of interleaved PCM audio in `dBTP`.
pub fn measure_true_peak_dbtp(interleaved: &[f32], channels: usize) -> f32 {
    if interleaved.is_empty() || channels == 0 {
        return -120.0;
    }
    let frames = interleaved.len() / channels;
    if frames == 0 {
        return -120.0;
    }
    let taps = oversample_taps();
    let half = (BS1770_TAPS / 2) as isize;
    let mut peak = 0.0_f32;
    for &s in interleaved {
        let a = s.abs();
        if a.is_finite() && a > peak {
            peak = a;
        }
    }
    for ch in 0..channels {
        for i in 0..frames {
            for phase in taps {
                let mut acc = 0.0_f32;
                for (t_idx, &coeff) in phase.iter().enumerate() {
                    let src_idx = (i as isize) + (t_idx as isize) - (half - 1);
                    if src_idx >= 0 && (src_idx as usize) < frames {
                        let sample = interleaved[(src_idx as usize) * channels + ch];
                        if sample.is_finite() {
                            acc += sample * coeff;
                        }
                    }
                }
                let a = acc.abs();
                if a > peak {
                    peak = a;
                }
            }
        }
    }
    20.0 * peak.max(1e-12).log10()
}

/// Estimate peak across the 4x oversampled sub-phases given a tiny per-channel recent history.
#[inline]
fn estimate_sample_true_peak(history: &[f32], write_idx: usize, len: usize) -> f32 {
    let cur = history[(write_idx + len - 1) % len].abs();
    let taps = oversample_taps();
    let mut best = cur;
    for phase in taps {
        let mut acc = 0.0_f32;
        for (k, &coeff) in phase.iter().enumerate() {
            let pos = (write_idx + k) % len;
            acc += history[pos] * coeff;
        }
        let a = acc.abs();
        if a > best {
            best = a;
        }
    }
    best
}

/// Decoded interleaved PCM audio chunk (`f32`, nominal `-1.0..=1.0`).
#[derive(Clone, Debug)]
pub struct AudioChunk {
    pub sample_rate: u32,
    pub channels: u16,
    pub start_seconds: f64,
    pub samples: Vec<f32>,
}

impl AudioChunk {
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / usize::from(self.channels)
        }
    }
}

/// FFmpeg audio stream decoder using only SEC-04 allowlisted containers/codecs and safe plane access.
pub struct AudioDecoder {
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Audio,
    stream_index: usize,
    time_base: f64,
    start: Option<i64>,
    sample_rate: u32,
    channels: u16,
    duration_ms: u64,
    decoder_name: String,
    demuxer_name: String,
    eof_sent: bool,
    emitted_samples: u64,
    skip_until_seconds: f64,
}

impl AudioDecoder {
    /// Open the best audio stream of a local file.
    pub fn open(path: &Path) -> Result<Self> {
        init();
        let canonical = check_local_path(path)?;
        let input = ffmpeg::format::input_with_dictionary(&ffmpeg_url(&canonical), open_options())
            .map_err(MediaError::Open)?;
        let demuxer_name = check_demuxer(&input)?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Audio)
            .ok_or(MediaError::NoAudio)?;
        let stream_index = stream.index();
        let time_base = f64::from(stream.time_base());
        let start = Some(stream.start_time()).filter(|t| *t != ffmpeg::ffi::AV_NOPTS_VALUE);
        let duration_ms = crate::duration_milliseconds(&input, &stream);
        let parameters = stream.parameters();
        let codec = ffmpeg::decoder::find(parameters.id())
            .ok_or_else(|| MediaError::DecoderNotAllowed(format!("{:?}", parameters.id())))?;
        let decoder_name = codec.name().to_owned();
        if !AUDIO_DECODERS.contains(&decoder_name.as_str()) {
            return Err(MediaError::DecoderNotAllowed(decoder_name));
        }
        let context = ffmpeg::codec::context::Context::from_parameters(parameters)
            .map_err(MediaError::Open)?;
        let decoder = context.decoder().audio().map_err(MediaError::Open)?;
        let sample_rate = decoder.rate().max(1);
        let channels = decoder.channels().max(1);
        tracing::info!(
            demuxer = %demuxer_name,
            decoder = %decoder_name,
            sample_rate,
            channels,
            "audio stream opened"
        );
        Ok(Self {
            input,
            decoder,
            stream_index,
            time_base,
            start,
            sample_rate,
            channels,
            duration_ms,
            decoder_name,
            demuxer_name,
            eof_sent: false,
            emitted_samples: 0,
            skip_until_seconds: 0.0,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    pub fn decoder_name(&self) -> &str {
        &self.decoder_name
    }

    pub fn demuxer_name(&self) -> &str {
        &self.demuxer_name
    }

    /// Seek to `position_ms` on the underlying container and discard lead-in frames earlier than
    /// `position_ms`.
    pub fn seek_ms(&mut self, position_ms: u64) -> Result<()> {
        let target_us = (position_ms as i64).saturating_mul(1000);
        let _ = self.input.seek(target_us, ..target_us.saturating_add(1));
        self.decoder.flush();
        self.eof_sent = false;
        self.skip_until_seconds = position_ms as f64 / 1000.0;
        self.emitted_samples =
            ((self.skip_until_seconds * f64::from(self.sample_rate)).round()) as u64;
        Ok(())
    }

    /// Read the next decoded audio chunk as interleaved `f32`, trimming priming / pre-seek samples.
    pub fn next_chunk(&mut self) -> Result<Option<AudioChunk>> {
        let mut decoded = ffmpeg::frame::Audio::empty();
        loop {
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    let rate = decoded.rate().max(self.sample_rate);
                    let channels = decoded.channels().max(self.channels);
                    let frame_count = decoded.samples();
                    if frame_count == 0 {
                        continue;
                    }
                    let mut start_seconds = decoded
                        .timestamp()
                        .or(decoded.pts())
                        .map(|pts| (pts - self.start.unwrap_or(0)) as f64 * self.time_base)
                        .unwrap_or_else(|| self.emitted_samples as f64 / f64::from(rate));
                    let mut samples = audio_frame_to_interleaved_f32(&decoded, channels as usize)?;
                    let chunk_dur = frame_count as f64 / f64::from(rate);
                    if start_seconds + chunk_dur <= self.skip_until_seconds {
                        self.emitted_samples =
                            self.emitted_samples.saturating_add(frame_count as u64);
                        continue;
                    }
                    if start_seconds < self.skip_until_seconds {
                        let skip_s = self.skip_until_seconds - start_seconds;
                        let skip_frames =
                            ((skip_s * f64::from(rate)).round() as usize).min(frame_count);
                        let drop_samples = skip_frames * usize::from(channels);
                        if drop_samples >= samples.len() {
                            self.emitted_samples =
                                self.emitted_samples.saturating_add(frame_count as u64);
                            continue;
                        }
                        samples.drain(..drop_samples);
                        start_seconds += skip_frames as f64 / f64::from(rate);
                    }
                    self.emitted_samples = self.emitted_samples.saturating_add(frame_count as u64);
                    return Ok(Some(AudioChunk {
                        sample_rate: rate,
                        channels,
                        start_seconds,
                        samples,
                    }));
                }
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {}
                Err(error) => return Err(MediaError::Decode(error)),
            }
            if self.eof_sent {
                return Ok(None);
            }
            let mut packet = ffmpeg::Packet::empty();
            match packet.read(&mut self.input) {
                Ok(()) if packet.stream() == self.stream_index => {
                    match self.decoder.send_packet(&packet) {
                        Ok(()) | Err(ffmpeg::Error::InvalidData) => {}
                        Err(error) => return Err(MediaError::Decode(error)),
                    }
                }
                Ok(()) | Err(ffmpeg::Error::InvalidData) => {}
                Err(ffmpeg::Error::Eof) => {
                    self.decoder.send_eof().map_err(MediaError::Decode)?;
                    self.eof_sent = true;
                }
                Err(error) => return Err(MediaError::Decode(error)),
            }
        }
    }
}

fn audio_frame_to_interleaved_f32(
    frame: &ffmpeg::frame::Audio,
    channels: usize,
) -> Result<Vec<f32>> {
    let n = frame.samples();
    let total = n * channels;
    let mut out = Vec::with_capacity(total);
    let packed = frame.is_packed();

    macro_rules! extract {
        ($t:ty, $conv:expr) => {{
            let convert = $conv;
            if packed {
                let plane = frame.plane::<$t>(0);
                for &s in plane.iter().take(total) {
                    out.push(convert(s));
                }
            } else {
                let planes: Vec<&[$t]> = (0..channels).map(|c| frame.plane::<$t>(c)).collect();
                for i in 0..n {
                    for plane in &planes {
                        out.push(convert(plane[i]));
                    }
                }
            }
        }};
    }

    match frame.format() {
        Sample::U8(_) => extract!(u8, |v: u8| (f32::from(v) - 128.0) / 128.0),
        Sample::I16(_) => extract!(i16, |v: i16| f32::from(v) / 32768.0),
        Sample::I32(_) => extract!(i32, |v: i32| (f64::from(v) / 2_147_483_648.0) as f32),
        Sample::F32(_) => extract!(f32, |v: f32| if v.is_finite() { v } else { 0.0 }),
        Sample::F64(_) => extract!(f64, |v: f64| if v.is_finite() { v as f32 } else { 0.0 }),
        other => {
            return Err(MediaError::Image(format!(
                "unsupported audio sample format {other:?}"
            )));
        }
    }
    Ok(out)
}

/// Convert interleaved `src` audio (`src_channels`) to `dst_channels` without `swresample`.
pub fn map_channels_interleaved(src: &[f32], src_channels: usize, dst_channels: usize) -> Vec<f32> {
    if src_channels == 0 || dst_channels == 0 {
        return Vec::new();
    }
    if src_channels == dst_channels {
        return src.to_vec();
    }
    let frames = src.len() / src_channels;
    let mut out = Vec::with_capacity(frames * dst_channels);
    for frame in src.chunks_exact(src_channels) {
        if src_channels == 1 && dst_channels >= 2 {
            let s = frame[0];
            for c in 0..dst_channels {
                out.push(if c < 2 { s } else { 0.0 });
            }
        } else if dst_channels == 1 {
            let sum: f32 = frame.iter().copied().sum();
            out.push(sum / (src_channels as f32));
        } else if src_channels >= 6 && dst_channels == 2 {
            // Standard 5.1 fold-down (FL, FR, FC, LFE, BL, BR).
            let fl = frame[0];
            let fr = frame[1];
            let fc = frame[2];
            let bl = frame[4];
            let br = frame[5];
            out.push((0.707 * fl + 0.5 * fc + 0.5 * bl) / 1.2);
            out.push((0.707 * fr + 0.5 * fc + 0.5 * br) / 1.2);
        } else {
            for c in 0..dst_channels {
                out.push(*frame.get(c).unwrap_or(&0.0));
            }
        }
    }
    out
}

/// Stateful linear resampler supporting sample-rate conversion and arbitrary speed (`0.1..=4.0`).
pub struct StreamResampler {
    dst_channels: usize,
    dst_rate: u32,
    pending: VecDeque<f32>,
    phase: f64,
}

impl StreamResampler {
    pub fn new(dst_rate: u32, dst_channels: usize) -> Self {
        Self {
            dst_channels: dst_channels.max(1),
            dst_rate: dst_rate.max(1),
            pending: VecDeque::new(),
            phase: 0.0,
        }
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.phase = 0.0;
    }

    /// Flush any single remaining carry frame on end-of-stream.
    pub fn flush(&mut self) -> Vec<f32> {
        let avail_frames = self.pending.len() / self.dst_channels;
        if avail_frames == 0 {
            self.pending.clear();
            self.phase = 0.0;
            return Vec::new();
        }
        let mut out = Vec::new();
        while self.phase < avail_frames as f64 {
            let idx = (self.phase.floor() as usize).min(avail_frames - 1);
            let next_idx = (idx + 1).min(avail_frames - 1);
            let frac = (self.phase - idx as f64) as f32;
            for ch in 0..self.dst_channels {
                let s0 = self.pending[idx * self.dst_channels + ch];
                let s1 = self.pending[next_idx * self.dst_channels + ch];
                out.push(s0 + (s1 - s0) * frac);
            }
            self.phase += 1.0;
        }
        self.clear();
        out
    }

    /// Append a chunk in `src_channels`, convert channels to `dst_channels`, and resample at
    /// `src_rate * speed -> dst_rate`. Returns newly produced interleaved frames at `dst_rate`.
    pub fn process_chunk(
        &mut self,
        interleaved_src: &[f32],
        src_rate: u32,
        src_channels: u16,
        speed: f64,
    ) -> Vec<f32> {
        let mapped = map_channels_interleaved(
            interleaved_src,
            usize::from(src_channels.max(1)),
            self.dst_channels,
        );
        if mapped.is_empty() {
            return Vec::new();
        }
        let speed = speed.clamp(0.1, 4.0);
        if src_rate == self.dst_rate && (speed - 1.0).abs() < 1e-6 && self.pending.is_empty() {
            self.phase = 0.0;
            return mapped;
        }

        self.pending.extend(mapped);
        let avail_frames = self.pending.len() / self.dst_channels;
        if avail_frames < 2 {
            return Vec::new();
        }

        let step = (f64::from(src_rate.max(1)) * speed) / f64::from(self.dst_rate);
        let limit = (avail_frames - 1) as f64;
        let est = ((limit - self.phase).max(0.0) / step).ceil() as usize + 2;
        let mut out = Vec::with_capacity(est * self.dst_channels);

        while self.phase < limit {
            let idx = self.phase.floor() as usize;
            let frac = (self.phase - idx as f64) as f32;
            for ch in 0..self.dst_channels {
                let s0 = self.pending[idx * self.dst_channels + ch];
                let s1 = self.pending[(idx + 1) * self.dst_channels + ch];
                out.push(s0 + (s1 - s0) * frac);
            }
            self.phase += step;
        }

        let drop_frames = (self.phase.floor() as usize).min(avail_frames - 1);
        if drop_frames > 0 {
            self.pending.drain(..drop_frames * self.dst_channels);
            self.phase -= drop_frames as f64;
        }

        out
    }
}

/// Master lookahead ITU-R BS.1770 true-peak brickwall limiter (`-1.0 dBTP` ceiling) + smooth
/// volume/mute/clear fader (**SAFE-05**, **M3-A8**).
pub struct MasterAudioProcessor {
    channels: usize,
    sample_rate: u32,
    volume_current: f32,
    volume_target: f32,
    volume_step: f32,
    muted: bool,
    fade_current: f32,
    fade_target: f32,
    fade_step: f32,
    lookahead_frames: usize,
    delay: VecDeque<f32>,
    tp_history: Vec<[f32; BS1770_TAPS]>,
    tp_pos: usize,
    limiter_gain: f32,
    limiter_release_coeff: f32,
}

impl MasterAudioProcessor {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        let channels = channels.max(1);
        let sample_rate = sample_rate.max(8_000);
        // ~1.5 ms lookahead (~72 samples at 48 kHz) catches inter-sample peaks with sub-frame latency.
        let lookahead_frames = ((u64::from(sample_rate) * 15) / 10_000).clamp(16, 128) as usize;
        let delay = VecDeque::from(vec![0.0_f32; lookahead_frames * channels]);
        let release_samples = (f64::from(sample_rate) * 0.060).max(1.0);
        let release_coeff = (-1.0 / release_samples).exp() as f32;
        Self {
            channels,
            sample_rate,
            volume_current: DEFAULT_SAFE_VOLUME,
            volume_target: DEFAULT_SAFE_VOLUME,
            volume_step: 1.0 / ((f32::from(15_u16) * 0.001) * sample_rate as f32).max(1.0),
            muted: false,
            fade_current: 1.0,
            fade_target: 1.0,
            fade_step: 1.0 / ((DEFAULT_FADE_MS as f32 * 0.001) * sample_rate as f32).max(1.0),
            lookahead_frames,
            delay,
            tp_history: vec![[0.0_f32; BS1770_TAPS]; channels],
            tp_pos: 0,
            limiter_gain: 1.0,
            limiter_release_coeff: release_coeff,
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        let v = if !volume.is_finite() || volume <= 0.0 {
            DEFAULT_SAFE_VOLUME
        } else {
            volume.clamp(0.0, MAX_USER_VOLUME)
        };
        self.volume_target = v;
    }

    /// Set exact linear target gain in `0.0..=MAX_USER_VOLUME` (use `0.0` explicitly for zero gain).
    pub fn set_exact_volume(&mut self, volume: f32) {
        self.volume_target = if volume.is_finite() {
            volume.clamp(0.0, MAX_USER_VOLUME)
        } else {
            DEFAULT_SAFE_VOLUME
        };
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// Begin a smooth linear fade-out to silence over `fade_ms` milliseconds (**SAFE-05**).
    pub fn start_fade_out(&mut self, fade_ms: u32) {
        let ms = fade_ms.max(1) as f32;
        self.fade_step = 1.0 / (ms * 0.001 * self.sample_rate as f32).max(1.0);
        self.fade_target = 0.0;
    }

    /// Begin a smooth linear fade-in back to unity over `fade_ms` milliseconds.
    pub fn start_fade_in(&mut self, fade_ms: u32) {
        let ms = fade_ms.max(1) as f32;
        self.fade_step = 1.0 / (ms * 0.001 * self.sample_rate as f32).max(1.0);
        self.fade_target = 1.0;
    }

    pub fn is_fade_out_complete(&self) -> bool {
        self.fade_target <= 0.0 && self.fade_current <= 1e-6
    }

    pub fn effective_gain(&self) -> f32 {
        self.volume_current * self.fade_current
    }

    /// Apply smooth gain, mute, fade, and lookahead `-1 dBTP` brickwall limiter in-place on an
    /// interleaved buffer of `frames * channels` samples.
    pub fn process_interleaved(&mut self, interleaved: &mut [f32]) {
        let ch = self.channels;
        for frame in interleaved.chunks_exact_mut(ch) {
            let eff_vol_target = if self.muted { 0.0 } else { self.volume_target };
            if self.volume_current < eff_vol_target {
                self.volume_current = (self.volume_current + self.volume_step).min(eff_vol_target);
            } else if self.volume_current > eff_vol_target {
                self.volume_current = (self.volume_current - self.volume_step).max(eff_vol_target);
            }

            if self.fade_current < self.fade_target {
                self.fade_current = (self.fade_current + self.fade_step).min(self.fade_target);
            } else if self.fade_current > self.fade_target {
                self.fade_current = (self.fade_current - self.fade_step).max(self.fade_target);
            }

            let gain = (self.volume_current * self.fade_current).clamp(0.0, MAX_USER_VOLUME);
            let mut incoming_tp = 0.0_f32;
            for (c_idx, s) in frame.iter().enumerate() {
                let scaled = if s.is_finite() { *s * gain } else { 0.0 };
                self.delay.push_back(scaled);
                self.tp_history[c_idx][self.tp_pos] = scaled;
                let tp = estimate_sample_true_peak(
                    &self.tp_history[c_idx],
                    (self.tp_pos + 1) % BS1770_TAPS,
                    BS1770_TAPS,
                );
                if tp > incoming_tp {
                    incoming_tp = tp;
                }
            }
            self.tp_pos = (self.tp_pos + 1) % BS1770_TAPS;

            // Scan lookahead window + incoming true peak so gain lands smoothly at or below target
            // by the time the peak reaches the output.
            let needed = if incoming_tp > LIMITER_TARGET_LINEAR {
                LIMITER_TARGET_LINEAR / incoming_tp
            } else {
                1.0
            };
            if needed < self.limiter_gain {
                let attack = 1.0 / (self.lookahead_frames as f32).max(1.0);
                // Step proportionally toward needed gain within lookahead, bounded below `needed` once close.
                self.limiter_gain = (self.limiter_gain
                    - (self.limiter_gain - needed) * (attack * 3.0))
                    .max(needed)
                    .min(1.0);
                if self
                    .delay
                    .iter()
                    .take(ch)
                    .any(|v| v.abs() * self.limiter_gain > LIMITER_HARD_GUARD)
                {
                    let head_peak = self
                        .delay
                        .iter()
                        .take(ch)
                        .fold(0.0_f32, |m, v| m.max(v.abs()));
                    if head_peak > LIMITER_TARGET_LINEAR {
                        self.limiter_gain =
                            self.limiter_gain.min(LIMITER_TARGET_LINEAR / head_peak);
                    }
                }
            } else {
                self.limiter_gain = self.limiter_gain * self.limiter_release_coeff
                    + (1.0 - self.limiter_release_coeff);
            }

            let lg = self.limiter_gain;
            for out_s in frame.iter_mut() {
                let delayed = self.delay.pop_front().unwrap_or(0.0);
                *out_s = (delayed * lg).clamp(-LIMITER_HARD_GUARD, LIMITER_HARD_GUARD);
            }
        }
    }
}

/// Thread-safe master audio clock reported in media seconds.
#[derive(Clone)]
pub struct AudioClock {
    epoch: Instant,
    media_ns: Arc<AtomicU64>,
    last_cb_ns: Arc<AtomicU64>,
    speed_bits: Arc<AtomicU32>,
    playing: Arc<AtomicBool>,
    virtual_mode: Arc<AtomicBool>,
}

impl AudioClock {
    pub fn new(initial_seconds: f64, speed: f32) -> Self {
        let initial_ns = (initial_seconds.max(0.0) * 1e9).round() as u64;
        Self {
            epoch: Instant::now(),
            media_ns: Arc::new(AtomicU64::new(initial_ns)),
            last_cb_ns: Arc::new(AtomicU64::new(u64::MAX)),
            speed_bits: Arc::new(AtomicU32::new(speed.clamp(0.1, 4.0).to_bits())),
            playing: Arc::new(AtomicBool::new(false)),
            virtual_mode: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Enable deterministic virtual-clock mode (used in synchronous `SoftwareCapture` tests) so
    /// real-time wall-clock jitter between audio pulls does not perturb the audio clock.
    pub fn set_virtual_clock(&self, enabled: bool) {
        self.virtual_mode.store(enabled, Ordering::Release);
    }

    pub fn set_playing(&self, playing: bool) {
        let was = self.playing.swap(playing, Ordering::SeqCst);
        if self.virtual_mode.load(Ordering::Acquire) {
            return;
        }
        if playing && !was {
            let now = self.epoch.elapsed().as_nanos() as u64;
            self.last_cb_ns.store(now, Ordering::Release);
        } else if !playing && was {
            let frozen =
                (self.seconds_at(self.epoch.elapsed().as_nanos() as u64) * 1e9).round() as u64;
            self.media_ns.store(frozen, Ordering::Release);
            self.last_cb_ns.store(u64::MAX, Ordering::Release);
        }
    }

    pub fn set_speed(&self, speed: f32) {
        self.speed_bits
            .store(speed.clamp(0.1, 4.0).to_bits(), Ordering::Release);
    }

    pub fn speed(&self) -> f64 {
        f64::from(f32::from_bits(self.speed_bits.load(Ordering::Acquire)))
    }

    pub fn reset_seconds(&self, seconds: f64) {
        let ns = (seconds.max(0.0) * 1e9).round() as u64;
        self.media_ns.store(ns, Ordering::Release);
        let now = if self.playing.load(Ordering::Acquire) {
            self.epoch.elapsed().as_nanos() as u64
        } else {
            u64::MAX
        };
        self.last_cb_ns.store(now, Ordering::Release);
    }

    /// Advance the master clock synchronously when audio frames are delivered to hardware/sink.
    pub fn advance_by_output_frames(&self, out_frames: u64, sample_rate: u32) {
        if out_frames == 0 || sample_rate == 0 {
            return;
        }
        let speed = self.speed();
        let delta_ns = ((out_frames as f64 / f64::from(sample_rate)) * speed * 1e9).round() as u64;
        self.media_ns.fetch_add(delta_ns, Ordering::AcqRel);
        let now = self.epoch.elapsed().as_nanos() as u64;
        self.last_cb_ns.store(now, Ordering::Release);
    }

    fn seconds_at(&self, now_ns: u64) -> f64 {
        let base_ns = self.media_ns.load(Ordering::Acquire);
        if self.virtual_mode.load(Ordering::Acquire) || !self.playing.load(Ordering::Acquire) {
            return base_ns as f64 / 1e9;
        }
        let cb_ns = self.last_cb_ns.load(Ordering::Acquire);
        if cb_ns == u64::MAX || now_ns <= cb_ns {
            return base_ns as f64 / 1e9;
        }
        // Smooth sub-callback interpolation, capped at 50 ms so stalled devices halt the clock.
        let elapsed_ns = (now_ns - cb_ns).min(50_000_000);
        let speed = self.speed();
        (base_ns as f64 + (elapsed_ns as f64) * speed) / 1e9
    }

    pub fn seconds(&self) -> f64 {
        self.seconds_at(self.epoch.elapsed().as_nanos() as u64)
    }
}

/// Active audio output backend (CPAL default output stream or headless software pump).
pub enum AudioOutputHandle {
    Cpal {
        _stream: cpal::Stream,
        sample_rate: u32,
        channels: usize,
    },
    Software {
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
        sample_rate: u32,
        channels: usize,
    },
}

impl AudioOutputHandle {
    pub fn sample_rate(&self) -> u32 {
        match self {
            Self::Cpal { sample_rate, .. } | Self::Software { sample_rate, .. } => *sample_rate,
        }
    }

    pub fn channels(&self) -> usize {
        match self {
            Self::Cpal { channels, .. } | Self::Software { channels, .. } => *channels,
        }
    }

    /// Open the default CPAL audio device if available and not disabled via `SELAH_AUDIO=null`,
    /// otherwise launch a silent 10 ms software pump so master clock + limiter behave identically
    /// in headless CI environments.
    pub fn open_default<F>(pull: F) -> Self
    where
        F: FnMut(&mut [f32]) + Send + 'static,
    {
        let pull = Arc::new(Mutex::new(pull));
        if std::env::var("SELAH_AUDIO").as_deref() != Ok("null")
            && let Some(cpal_handle) = try_open_cpal(&pull)
        {
            return cpal_handle;
        }
        open_software_pump(pull, 48_000, 2)
    }

    /// Force the silent software audio pump at `sample_rate` and `channels` (useful in tests).
    pub fn open_software<F>(sample_rate: u32, channels: usize, pull: F) -> Self
    where
        F: FnMut(&mut [f32]) + Send + 'static,
    {
        open_software_pump(Arc::new(Mutex::new(pull)), sample_rate, channels)
    }
}

impl Drop for AudioOutputHandle {
    fn drop(&mut self) {
        if let Self::Software { stop, thread, .. } = self {
            stop.store(true, Ordering::Release);
            if let Some(handle) = thread.take() {
                let _ = handle.join();
            }
        }
    }
}

fn fill_typed_cpal<T, F>(data: &mut [T], channels: usize, pull: &Arc<Mutex<F>>)
where
    T: cpal::Sample + FromSample<f32>,
    F: FnMut(&mut [f32]) + Send + 'static,
{
    let total = data.len();
    let mut scratch = vec![0.0_f32; total];
    if channels > 0 {
        let mut cb = pull.lock().unwrap_or_else(|p| p.into_inner());
        cb(&mut scratch);
    }
    for (dst, &s) in data.iter_mut().zip(&scratch) {
        *dst = T::from_sample(s.clamp(-1.0, 1.0));
    }
}

fn try_open_cpal<F>(pull: &Arc<Mutex<F>>) -> Option<AudioOutputHandle>
where
    F: FnMut(&mut [f32]) + Send + 'static,
{
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let supported = device.default_output_config().ok()?;
    let sample_rate = supported.sample_rate();
    let channels = usize::from(supported.channels().max(1));
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let err_cb = |err| {
        tracing::warn!(%err, "audio output stream error");
    };
    let pull_cb = pull.clone();
    let stream = match sample_format {
        SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], _| fill_typed_cpal(data, channels, &pull_cb),
            err_cb,
            Some(Duration::from_millis(250)),
        ),
        SampleFormat::I16 => device.build_output_stream(
            config,
            move |data: &mut [i16], _| fill_typed_cpal(data, channels, &pull_cb),
            err_cb,
            Some(Duration::from_millis(250)),
        ),
        SampleFormat::U16 => device.build_output_stream(
            config,
            move |data: &mut [u16], _| fill_typed_cpal(data, channels, &pull_cb),
            err_cb,
            Some(Duration::from_millis(250)),
        ),
        SampleFormat::I32 => device.build_output_stream(
            config,
            move |data: &mut [i32], _| fill_typed_cpal(data, channels, &pull_cb),
            err_cb,
            Some(Duration::from_millis(250)),
        ),
        other => {
            tracing::warn!(
                ?other,
                "unsupported default audio sample format; falling back to software pump"
            );
            return None;
        }
    }
    .ok()?;
    if stream.play().is_err() {
        return None;
    }
    Some(AudioOutputHandle::Cpal {
        _stream: stream,
        sample_rate,
        channels,
    })
}

fn open_software_pump<F>(
    pull: Arc<Mutex<F>>,
    sample_rate: u32,
    channels: usize,
) -> AudioOutputHandle
where
    F: FnMut(&mut [f32]) + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    let channels = channels.max(1);
    let sample_rate = sample_rate.max(8_000);
    let block_frames = (sample_rate / 100).max(64) as usize; // 10 ms per block
    let block_dur = Duration::from_secs_f64(block_frames as f64 / f64::from(sample_rate));

    let thread = std::thread::Builder::new()
        .name("selah-audio-pump".into())
        .spawn(move || {
            let mut scratch = vec![0.0_f32; block_frames * channels];
            let mut next_tick = Instant::now() + block_dur;
            while !stop_flag.load(Ordering::Acquire) {
                scratch.fill(0.0);
                {
                    let mut cb = pull.lock().unwrap_or_else(|p| p.into_inner());
                    cb(&mut scratch);
                }
                let now = Instant::now();
                if next_tick > now {
                    std::thread::sleep(next_tick - now);
                } else if now.duration_since(next_tick) > Duration::from_millis(100) {
                    next_tick = now;
                }
                next_tick += block_dur;
            }
        })
        .ok();

    AudioOutputHandle::Software {
        stop,
        thread,
        sample_rate,
        channels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_limiter_keeps_plus_6dbfs_sine_under_minus_1_dbtp() {
        let sample_rate = 48_000;
        let channels = 2;
        let mut proc = MasterAudioProcessor::new(sample_rate, channels);
        proc.set_exact_volume(1.0);

        // Generate 0.5 s of +6 dBFS (amplitude 2.0) 997 Hz stereo sine wave + initial full-scale step.
        let frames = sample_rate as usize / 2;
        let mut buf = Vec::with_capacity(frames * channels);
        for n in 0..frames {
            let s = 2.0_f32
                * (2.0 * std::f32::consts::PI * 997.0 * (n as f32) / sample_rate as f32).sin();
            buf.push(s);
            buf.push(s);
        }
        proc.process_interleaved(&mut buf);

        let tp = measure_true_peak_dbtp(&buf, channels);
        assert!(
            tp < MASTER_CEILING_DBTP,
            "true peak {tp:.3} dBTP must stay strictly below {MASTER_CEILING_DBTP} dBTP"
        );
        // Ensure audible bounded output wasn't muted to zero.
        let peak_amp = buf.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
        assert!(
            peak_amp > 0.75,
            "expected strong limited signal, got {peak_amp}"
        );
    }

    #[test]
    fn clear_fade_ramps_smoothly_to_silence() {
        let sample_rate = 48_000;
        let channels = 2;
        let mut proc = MasterAudioProcessor::new(sample_rate, channels);
        proc.set_exact_volume(1.0);

        let frames = (sample_rate / 10) as usize; // 100 ms
        let mut buf = vec![0.5_f32; frames * channels];
        // Process 20 ms steady first so limiter delay fills.
        proc.process_interleaved(&mut buf[..960 * channels]);
        proc.start_fade_out(25);
        proc.process_interleaved(&mut buf[960 * channels..]);

        assert!(proc.is_fade_out_complete());
        let first_post_fade_start = (960 + 25 * 48 + 256) * channels;
        assert!(
            buf[first_post_fade_start..].iter().all(|s| s.abs() <= 1e-6),
            "post-fade tail must be completely silent"
        );
        // Verify gradual transition (not an instant 0.5 -> 0.0 cliff on sample 960).
        assert!(buf[965 * channels].abs() > 0.1);
    }

    #[test]
    fn opening_url_in_audio_decoder_is_refused() {
        let err = AudioDecoder::open(Path::new("https://example.invalid/audio.aac"))
            .err()
            .expect("URL must be refused");
        assert!(matches!(err, MediaError::NotLocal(_)));
    }
}
