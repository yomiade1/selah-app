//! Synchronized A/V media player with audio master clock, standby loop pre-opening (0-gap loops),
//! in/out points, speed scaling, ITU-R BS.1770 true-peak limiter & smooth fade-out, and
//! deterministic test capture (M3.4, M3-A4, M3-A8, SAFE-05, SEC-04).
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use selah_render::video::Nv12Frame;

use crate::audio::{
    AudioClock, AudioDecoder, AudioOutputHandle, DEFAULT_SAFE_VOLUME, MasterAudioProcessor,
    StreamResampler,
};
use crate::{DecodeMode, MediaError, Result, VideoDecoder, check_local_path};

const VIDEO_QUEUE_TARGET: usize = 20;
const AUDIO_BUFFER_TARGET_S: f64 = 0.50;
const MAX_CAPTURE_SAMPLES: usize = 48_000 * 2 * 30;

/// Configuration to open local verified media for synchronized playback.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaybackConfig {
    pub path: PathBuf,
    pub decode_mode: DecodeMode,
    pub in_ms: u64,
    pub out_ms: Option<u64>,
    pub loop_media: bool,
    pub speed: f32,
    pub volume: f32,
    pub muted: bool,
    pub paused: bool,
    pub start_position_ms: u64,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            decode_mode: DecodeMode::Auto,
            in_ms: 0,
            out_ms: None,
            loop_media: false,
            speed: 1.0,
            volume: DEFAULT_SAFE_VOLUME,
            muted: false,
            paused: false,
            start_position_ms: 0,
        }
    }
}

/// Output sink selection for [`MediaPlayer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackOutputMode {
    /// Open the CPAL default audio output device, falling back to a 10 ms software clock pump when
    /// running headless or when `SELAH_AUDIO=null`.
    DefaultDevice,
    /// Async 10 ms silent software pump advancing the live audio clock without touching OS audio devices.
    SoftwarePump,
    /// Synchronous pull-driven capture mode for deterministic virtual-clock tests.
    SoftwareCapture { sample_rate: u32, channels: u16 },
}

/// Decoded video frame timestamped on the player's unrolled media timeline.
#[derive(Clone, Debug)]
pub struct TimedVideoFrame {
    pub nv12: Nv12Frame,
    /// Cumulative playback timestamp in source media seconds (monotonic across loops).
    pub play_seconds: f64,
    /// Strictly increasing frame sequence number across the player's lifetime (0-gap across loops).
    pub sequence: u64,
    pub lap: u64,
}

struct QueuedAudioChunk {
    sample_rate: u32,
    channels: u16,
    samples: Vec<f32>,
}

impl QueuedAudioChunk {
    fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / usize::from(self.channels)
        }
    }
}

struct AudioShared {
    pending: VecDeque<QueuedAudioChunk>,
    pending_source_seconds: f64,
    resampler: StreamResampler,
    resampled: VecDeque<f32>,
    master: MasterAudioProcessor,
    has_audio_stream: bool,
    speed: f64,
    paused: bool,
    fading_to_stop: bool,
    finished: bool,
    eos: bool,
    capture_enabled: bool,
    captured: Vec<f32>,
    clock: AudioClock,
    sample_rate: u32,
    channels: usize,
}

impl AudioShared {
    fn new(
        sample_rate: u32,
        channels: usize,
        clock: AudioClock,
        config: &PlaybackConfig,
        has_audio: bool,
        capture_enabled: bool,
        start_paused: bool,
    ) -> Self {
        let channels = channels.max(1);
        let sample_rate = sample_rate.max(8_000);
        let mut master = MasterAudioProcessor::new(sample_rate, channels);
        master.set_volume(config.volume);
        master.set_muted(config.muted);
        Self {
            pending: VecDeque::new(),
            pending_source_seconds: 0.0,
            resampler: StreamResampler::new(sample_rate, channels),
            resampled: VecDeque::new(),
            master,
            has_audio_stream: has_audio,
            speed: f64::from(config.speed.clamp(0.1, 4.0)),
            paused: start_paused,
            fading_to_stop: false,
            finished: false,
            eos: false,
            capture_enabled,
            captured: Vec::new(),
            clock,
            sample_rate,
            channels,
        }
    }

    fn buffered_output_seconds_est(&self) -> f64 {
        let resampled_s = self.resampled.len() as f64
            / (f64::from(self.sample_rate.max(1)) * self.channels.max(1) as f64);
        let pending_out_s = self.pending_source_seconds / self.speed.max(0.1);
        resampled_s + pending_out_s
    }

    fn push_chunk(&mut self, chunk: QueuedAudioChunk) {
        let dur = chunk.frames() as f64 / f64::from(chunk.sample_rate.max(1));
        self.pending_source_seconds += dur;
        self.pending.push_back(chunk);
    }

    fn pull_into(&mut self, dst: &mut [f32]) {
        let frames = dst.len() / self.channels;
        if frames == 0 {
            return;
        }
        if self.paused && !self.fading_to_stop {
            dst.fill(0.0);
            return;
        }
        let needed_samples = frames * self.channels;
        if self.has_audio_stream {
            while self.resampled.len() < needed_samples {
                if let Some(chunk) = self.pending.pop_front() {
                    let dur = chunk.frames() as f64 / f64::from(chunk.sample_rate.max(1));
                    self.pending_source_seconds = (self.pending_source_seconds - dur).max(0.0);
                    let out = self.resampler.process_chunk(
                        &chunk.samples,
                        chunk.sample_rate,
                        chunk.channels,
                        self.speed,
                    );
                    self.resampled.extend(out);
                } else {
                    if self.eos {
                        let tail = self.resampler.flush();
                        self.resampled.extend(tail);
                    }
                    break;
                }
            }
            for slot in dst.iter_mut().take(needed_samples) {
                *slot = self.resampled.pop_front().unwrap_or(0.0);
            }
        } else {
            dst.fill(0.0);
        }

        self.master.process_interleaved(&mut dst[..needed_samples]);
        if self.capture_enabled && self.captured.len() < MAX_CAPTURE_SAMPLES {
            let room = MAX_CAPTURE_SAMPLES - self.captured.len();
            self.captured
                .extend_from_slice(&dst[..needed_samples.min(room)]);
        }

        if self.fading_to_stop {
            if self.master.is_fade_out_complete() {
                self.finished = true;
                self.paused = true;
                self.clock.set_playing(false);
            } else {
                self.clock
                    .advance_by_output_frames(frames as u64, self.sample_rate);
            }
            return;
        }

        if self.eos
            && (!self.has_audio_stream || (self.pending.is_empty() && self.resampled.is_empty()))
        {
            self.finished = true;
            self.clock.set_playing(false);
            return;
        }

        self.clock
            .advance_by_output_frames(frames as u64, self.sample_rate);
    }
}

#[derive(Default)]
struct VideoState {
    queue: VecDeque<TimedVideoFrame>,
    current: Option<TimedVideoFrame>,
    last_decoded_play_s: Option<f64>,
    eos: bool,
    initial_ready: bool,
    completed_seek_seq: u64,
}

#[derive(Default)]
struct WorkerMailbox {
    next_seek_seq: u64,
    seek_to_ms: Option<(u64, u64)>,
}

struct OpenDecoders {
    canonical_path: PathBuf,
    decode_mode: DecodeMode,
    video: Option<VideoDecoder>,
    audio: Option<AudioDecoder>,
}

fn open_decoders(
    canonical_path: &Path,
    decode_mode: DecodeMode,
    seek_ms: u64,
    need_video: bool,
    need_audio: bool,
) -> Result<(Option<VideoDecoder>, Option<AudioDecoder>)> {
    let video = if need_video {
        match VideoDecoder::open(canonical_path, decode_mode) {
            Ok(mut vd) => {
                if seek_ms > 0 {
                    vd.seek_ms(seek_ms)?;
                }
                Some(vd)
            }
            Err(MediaError::NoVideo) => None,
            Err(err) => return Err(err),
        }
    } else {
        None
    };

    let audio = if need_audio {
        match AudioDecoder::open(canonical_path) {
            Ok(mut ad) => {
                if seek_ms > 0 {
                    ad.seek_ms(seek_ms)?;
                }
                Some(ad)
            }
            Err(MediaError::NoAudio) => None,
            Err(err) => return Err(err),
        }
    } else {
        None
    };

    Ok((video, audio))
}

/// Synchronized audio/video media player with master audio clock and pre-opened seamless looping.
pub struct MediaPlayer {
    config: Mutex<PlaybackConfig>,
    clock: AudioClock,
    audio: Arc<(Mutex<AudioShared>, Condvar)>,
    video: Arc<(Mutex<VideoState>, Condvar)>,
    mailbox: Arc<(Mutex<WorkerMailbox>, Condvar)>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    _output: Option<AudioOutputHandle>,
    sample_rate: u32,
    channels: usize,
    video_width: u32,
    video_height: u32,
    has_video: bool,
    has_audio: bool,
    is_capture_mode: bool,
}

impl MediaPlayer {
    /// Open `config.path` under SEC-04 allowlists and begin pre-buffered playback.
    pub fn open(config: PlaybackConfig, output_mode: PlaybackOutputMode) -> Result<Self> {
        let canonical = check_local_path(&config.path)?;
        let out_ms = config.out_ms.filter(|&o| o > config.in_ms);
        let initial_seek_ms = config
            .start_position_ms
            .max(config.in_ms)
            .min(out_ms.map_or(u64::MAX, |o| o.saturating_sub(1)));

        let (initial_video, initial_audio) =
            open_decoders(&canonical, config.decode_mode, initial_seek_ms, true, true)?;
        if initial_video.is_none() && initial_audio.is_none() {
            return Err(MediaError::NoVideo);
        }

        let video_width = initial_video.as_ref().map_or(0, VideoDecoder::width);
        let video_height = initial_video.as_ref().map_or(0, VideoDecoder::height);
        let has_video = initial_video.is_some();
        let has_audio = initial_audio.is_some();

        let clock_start_rel_s = (initial_seek_ms.saturating_sub(config.in_ms) as f64) / 1000.0;
        let clock = AudioClock::new(clock_start_rel_s, config.speed);

        let (audio_shared, output_handle, sample_rate, channels, is_capture_mode) =
            match output_mode {
                PlaybackOutputMode::SoftwareCapture {
                    sample_rate,
                    channels,
                } => {
                    clock.set_virtual_clock(true);
                    let channels = usize::from(channels.max(1));
                    let shared = Arc::new((
                        Mutex::new(AudioShared::new(
                            sample_rate,
                            channels,
                            clock.clone(),
                            &config,
                            has_audio,
                            true,
                            config.paused,
                        )),
                        Condvar::new(),
                    ));
                    (shared, None, sample_rate.max(8_000), channels, true)
                }
                PlaybackOutputMode::DefaultDevice => {
                    // Start audio sink paused until initial pre-buffer completes so clock starts in sync.
                    let placeholder = Arc::new((
                        Mutex::new(AudioShared::new(
                            48_000,
                            2,
                            clock.clone(),
                            &config,
                            has_audio,
                            false,
                            true,
                        )),
                        Condvar::new(),
                    ));
                    let pull_ref = placeholder.clone();
                    let handle = AudioOutputHandle::open_default(move |dst| {
                        let (mtx, cv) = &*pull_ref;
                        let mut guard = mtx.lock().unwrap_or_else(|p| p.into_inner());
                        guard.pull_into(dst);
                        cv.notify_all();
                    });
                    let sr = handle.sample_rate();
                    let ch = handle.channels();
                    {
                        let mut guard = placeholder.0.lock().unwrap_or_else(|p| p.into_inner());
                        if guard.sample_rate != sr || guard.channels != ch {
                            *guard = AudioShared::new(
                                sr,
                                ch,
                                clock.clone(),
                                &config,
                                has_audio,
                                false,
                                true,
                            );
                        }
                    }
                    (placeholder, Some(handle), sr, ch, false)
                }
                PlaybackOutputMode::SoftwarePump => {
                    let shared = Arc::new((
                        Mutex::new(AudioShared::new(
                            48_000,
                            2,
                            clock.clone(),
                            &config,
                            has_audio,
                            false,
                            true,
                        )),
                        Condvar::new(),
                    ));
                    let pull_ref = shared.clone();
                    let handle = AudioOutputHandle::open_software(48_000, 2, move |dst| {
                        let (mtx, cv) = &*pull_ref;
                        let mut guard = mtx.lock().unwrap_or_else(|p| p.into_inner());
                        guard.pull_into(dst);
                        cv.notify_all();
                    });
                    (shared, Some(handle), 48_000, 2, false)
                }
            };

        let video_state = Arc::new((Mutex::new(VideoState::default()), Condvar::new()));
        let mailbox = Arc::new((Mutex::new(WorkerMailbox::default()), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let mut initial_cfg = config.clone();
        initial_cfg.out_ms = out_ms;

        let decoders = OpenDecoders {
            canonical_path: canonical,
            decode_mode: config.decode_mode,
            video: initial_video,
            audio: initial_audio,
        };

        let worker_video = video_state.clone();
        let worker_audio = audio_shared.clone();
        let worker_mailbox = mailbox.clone();
        let worker_stop = stop.clone();
        let worker_cfg = initial_cfg.clone();

        let worker = std::thread::Builder::new()
            .name("selah-media-decode".into())
            .spawn(move || {
                decode_worker(
                    decoders,
                    worker_cfg,
                    worker_video,
                    worker_audio,
                    worker_mailbox,
                    worker_stop,
                );
            })
            .map_err(MediaError::Io)?;

        // Wait briefly for initial prefill before releasing audio clock.
        if has_video {
            let (mtx, cv) = &*video_state;
            let mut guard = mtx.lock().unwrap_or_else(|p| p.into_inner());
            let deadline = Instant::now() + Duration::from_secs(2);
            while !guard.initial_ready {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let (next_g, _) = cv
                    .wait_timeout(guard, deadline - now)
                    .unwrap_or_else(|p| p.into_inner());
                guard = next_g;
            }
        }
        if has_audio {
            let (mtx, cv) = &*audio_shared;
            let mut guard = mtx.lock().unwrap_or_else(|p| p.into_inner());
            let deadline = Instant::now() + Duration::from_secs(2);
            while guard.pending.is_empty() && !guard.eos {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let (next_g, _) = cv
                    .wait_timeout(guard, deadline - now)
                    .unwrap_or_else(|p| p.into_inner());
                guard = next_g;
            }
        }

        if !config.paused {
            {
                let mut guard = audio_shared.0.lock().unwrap_or_else(|p| p.into_inner());
                guard.paused = false;
            }
            clock.set_playing(true);
        }

        Ok(Self {
            config: Mutex::new(initial_cfg),
            clock,
            audio: audio_shared,
            video: video_state,
            mailbox,
            stop,
            worker: Some(worker),
            _output: output_handle,
            sample_rate,
            channels,
            video_width,
            video_height,
            has_video,
            has_audio,
            is_capture_mode,
        })
    }

    pub fn video_dimensions(&self) -> Option<(u32, u32)> {
        self.has_video
            .then_some((self.video_width, self.video_height))
    }

    pub fn has_video(&self) -> bool {
        self.has_video
    }

    pub fn has_audio(&self) -> bool {
        self.has_audio
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn clock(&self) -> &AudioClock {
        &self.clock
    }

    pub fn clock_seconds(&self) -> f64 {
        self.clock.seconds()
    }

    pub fn config(&self) -> PlaybackConfig {
        self.config
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn set_paused(&self, paused: bool) {
        {
            let mut cfg = self.config.lock().unwrap_or_else(|p| p.into_inner());
            if cfg.paused == paused {
                return;
            }
            cfg.paused = paused;
        }
        {
            let mut a = self.audio.0.lock().unwrap_or_else(|p| p.into_inner());
            a.paused = paused;
        }
        self.clock.set_playing(!paused);
    }

    pub fn set_speed(&self, speed: f32) {
        let clamped = if speed.is_finite() {
            speed.clamp(0.1, 4.0)
        } else {
            1.0
        };
        {
            let mut cfg = self.config.lock().unwrap_or_else(|p| p.into_inner());
            if (cfg.speed - clamped).abs() < 1e-4 {
                return;
            }
            cfg.speed = clamped;
        }
        self.clock.set_speed(clamped);
        {
            let mut a = self.audio.0.lock().unwrap_or_else(|p| p.into_inner());
            a.speed = f64::from(clamped);
        }
        self.mailbox.1.notify_all();
    }

    pub fn set_volume(&self, volume: f32) {
        {
            let mut cfg = self.config.lock().unwrap_or_else(|p| p.into_inner());
            cfg.volume = volume;
        }
        let mut a = self.audio.0.lock().unwrap_or_else(|p| p.into_inner());
        a.master.set_volume(volume);
    }

    pub fn set_muted(&self, muted: bool) {
        {
            let mut cfg = self.config.lock().unwrap_or_else(|p| p.into_inner());
            cfg.muted = muted;
        }
        let mut a = self.audio.0.lock().unwrap_or_else(|p| p.into_inner());
        a.master.set_muted(muted);
    }

    /// Initiate a smooth audio fade-out over `fade_ms` milliseconds and mark finished once silent (**SAFE-05**).
    pub fn fade_out_and_stop(&self, fade_ms: u32) {
        let mut a = self.audio.0.lock().unwrap_or_else(|p| p.into_inner());
        a.fading_to_stop = true;
        a.master.start_fade_out(fade_ms);
    }

    pub fn is_finished(&self) -> bool {
        self.audio
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .finished
    }

    pub fn seek_ms(&self, position_ms: u64) {
        let target_seq = {
            let mut mb = self.mailbox.0.lock().unwrap_or_else(|p| p.into_inner());
            mb.next_seek_seq = mb.next_seek_seq.saturating_add(1);
            let seq = mb.next_seek_seq;
            mb.seek_to_ms = Some((seq, position_ms));
            seq
        };
        self.mailbox.1.notify_all();
        self.audio.1.notify_all();
        self.video.1.notify_all();

        let (mtx, cv) = &*self.video;
        if self.has_video {
            let mut guard = mtx.lock().unwrap_or_else(|p| p.into_inner());
            let deadline = Instant::now() + Duration::from_secs(2);
            while (guard.completed_seek_seq < target_seq || !guard.initial_ready)
                && !self.stop.load(Ordering::Acquire)
            {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let (next_g, _) = cv
                    .wait_timeout(guard, deadline - now)
                    .unwrap_or_else(|p| p.into_inner());
                guard = next_g;
            }
        }
    }

    /// Enable or disable recording of post-limiter interleaved output PCM.
    pub fn set_capture_audio(&self, enabled: bool) {
        let mut a = self.audio.0.lock().unwrap_or_else(|p| p.into_inner());
        a.capture_enabled = enabled;
        if !enabled {
            a.captured.clear();
        }
    }

    /// Return a copy of captured post-limiter interleaved PCM frames along with `(sample_rate, channels)`.
    pub fn captured_pcm(&self) -> (u32, usize, Vec<f32>) {
        let a = self.audio.0.lock().unwrap_or_else(|p| p.into_inner());
        (a.sample_rate, a.channels, a.captured.clone())
    }

    /// Deterministically advance audio (and master clock) by `frames` output frames in
    /// `SoftwareCapture` mode, returning the post-limiter interleaved PCM slice.
    pub fn pump_audio_frames(&self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0_f32; frames * self.channels];
        if frames == 0 {
            return out;
        }

        if self.is_capture_mode {
            let dt_media = (frames as f64 / f64::from(self.sample_rate)) * self.clock.speed();
            let target_s = self.clock.seconds() + dt_media;

            if self.has_video {
                let (mtx, cv) = &*self.video;
                let mut v = mtx.lock().unwrap_or_else(|p| p.into_inner());
                let deadline = Instant::now() + Duration::from_secs(3);
                while !v.eos
                    && !self.stop.load(Ordering::Acquire)
                    && v.last_decoded_play_s.is_none_or(|t| t < target_s - 0.000_5)
                {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    let (next_v, _) = cv
                        .wait_timeout(v, (deadline - now).min(Duration::from_millis(20)))
                        .unwrap_or_else(|p| p.into_inner());
                    v = next_v;
                }
            }

            if self.has_audio {
                let (mtx, cv) = &*self.audio;
                let mut a = mtx.lock().unwrap_or_else(|p| p.into_inner());
                let needed_s = (frames as f64 / f64::from(self.sample_rate)) + 0.015;
                let deadline = Instant::now() + Duration::from_secs(3);
                while !a.eos
                    && !a.finished
                    && !a.paused
                    && !self.stop.load(Ordering::Acquire)
                    && a.buffered_output_seconds_est() < needed_s
                {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    let (next_a, _) = cv
                        .wait_timeout(a, (deadline - now).min(Duration::from_millis(20)))
                        .unwrap_or_else(|p| p.into_inner());
                    a = next_a;
                }
            }
        }

        {
            let (mtx, cv) = &*self.audio;
            let mut a = mtx.lock().unwrap_or_else(|p| p.into_inner());
            a.pull_into(&mut out);
            cv.notify_all();
        }
        out
    }

    /// Return the video frame due at `clock.seconds()`, advancing through queued frames if due.
    pub fn frame_for_now(&self) -> Option<TimedVideoFrame> {
        let mut last_seq = None;
        self.frame_at_seconds(self.clock.seconds(), &mut last_seq, false)
    }

    /// Return `Some(frame)` ONLY if the due frame's `sequence` differs from `*last_seq`, updating `*last_seq`.
    pub fn frame_if_changed(&self, last_seq: &mut Option<u64>) -> Option<TimedVideoFrame> {
        self.frame_at_seconds(self.clock.seconds(), last_seq, true)
    }

    fn frame_at_seconds(
        &self,
        now_s: f64,
        last_seq: &mut Option<u64>,
        only_if_changed: bool,
    ) -> Option<TimedVideoFrame> {
        let (mtx, cv) = &*self.video;
        let mut vs = mtx.lock().unwrap_or_else(|p| p.into_inner());
        let mut popped = false;
        while let Some(front) = vs.queue.front() {
            if front.play_seconds <= now_s + 0.000_5 {
                let frame = vs.queue.pop_front().expect("front");
                vs.current = Some(frame);
                popped = true;
            } else {
                break;
            }
        }
        if vs.current.is_none()
            && let Some(front) = vs.queue.pop_front()
        {
            vs.current = Some(front);
            popped = true;
        }
        drop_cv_notify_if(popped, cv);
        let cur = vs.current.as_ref()?;
        if only_if_changed && *last_seq == Some(cur.sequence) {
            return None;
        }
        *last_seq = Some(cur.sequence);
        Some(cur.clone())
    }
}

impl Drop for MediaPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.mailbox.1.notify_all();
        self.video.1.notify_all();
        self.audio.1.notify_all();
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

#[inline]
fn drop_cv_notify_if(cond: bool, cv: &Condvar) {
    if cond {
        cv.notify_all();
    }
}

fn decode_worker(
    decoders: OpenDecoders,
    config: PlaybackConfig,
    video_state: Arc<(Mutex<VideoState>, Condvar)>,
    audio_state: Arc<(Mutex<AudioShared>, Condvar)>,
    mailbox: Arc<(Mutex<WorkerMailbox>, Condvar)>,
    stop: Arc<AtomicBool>,
) {
    let canonical = decoders.canonical_path;
    let decode_mode = decoders.decode_mode;
    let has_video = decoders.video.is_some();
    let has_audio = decoders.audio.is_some();
    let in_s = config.in_ms as f64 / 1000.0;
    let out_s = config
        .out_ms
        .filter(|&o| o > config.in_ms)
        .map(|o| o as f64 / 1000.0);

    let mut current_video = decoders.video;
    let mut current_audio = decoders.audio;
    let mut standby_video: Option<VideoDecoder> = None;
    let mut standby_audio: Option<AudioDecoder> = None;

    let mut next_seq: u64 = 0;
    let mut lap: u64 = 0;
    let mut lap_base_s: f64 = 0.0;
    let mut audio_written_rel_s: f64 = (config
        .start_position_ms
        .max(config.in_ms)
        .saturating_sub(config.in_ms) as f64)
        / 1000.0;
    let mut active_seek_seq: u64 = 0;

    'outer: while !stop.load(Ordering::Acquire) {
        if let Some((seq, seek_pos)) = {
            let mut mb = mailbox.0.lock().unwrap_or_else(|p| p.into_inner());
            mb.seek_to_ms.take()
        } {
            let clamped = seek_pos
                .max(config.in_ms)
                .min(config.out_ms.map_or(u64::MAX, |o| o.saturating_sub(1)));
            let rel_s = (clamped.saturating_sub(config.in_ms) as f64) / 1000.0;
            {
                let mut vs = video_state.0.lock().unwrap_or_else(|p| p.into_inner());
                vs.queue.clear();
                vs.current = None;
                vs.last_decoded_play_s = None;
                vs.eos = false;
                vs.initial_ready = false;
            }
            {
                let mut a = audio_state.0.lock().unwrap_or_else(|p| p.into_inner());
                a.pending.clear();
                a.pending_source_seconds = 0.0;
                a.resampler.clear();
                a.resampled.clear();
                a.eos = false;
                a.finished = false;
                a.clock.reset_seconds(rel_s);
            }
            lap = 0;
            lap_base_s = 0.0;
            audio_written_rel_s = rel_s;
            active_seek_seq = seq;
            standby_video = None;
            standby_audio = None;
            match open_decoders(&canonical, decode_mode, clamped, has_video, has_audio) {
                Ok((v, a)) => {
                    current_video = v;
                    current_audio = a;
                }
                Err(err) => {
                    tracing::error!(%err, "failed to seek media decoder");
                    break 'outer;
                }
            }
        }

        let mut video_done = current_video.is_none();
        let mut audio_done = current_audio.is_none();
        let mut prev_video_pts_s: Option<f64> = None;
        let mut last_video_rel_s: Option<f64> = None;
        let default_period = current_video
            .as_ref()
            .map(VideoDecoder::frame_rate)
            .filter(|fps| fps.is_finite() && *fps > 1.0)
            .map_or(1.0 / 60.0, |fps| 1.0 / fps);
        let mut est_period_s = default_period;
        let mut audio_rate_hint = current_audio
            .as_ref()
            .map_or(48_000, AudioDecoder::sample_rate);
        let mut audio_ch_hint = current_audio.as_ref().map_or(2, AudioDecoder::channels);
        let mut lap_video_frames: usize = 0;

        while !(stop.load(Ordering::Acquire) || (video_done && audio_done)) {
            if mailbox
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .seek_to_ms
                .is_some()
            {
                continue 'outer;
            }

            // Pre-open next lap's standby decoders once the first few frames are safely buffered
            // so looping hits ZERO I/O open stall at the seam (M3-A4).
            if config.loop_media
                && (!has_video || lap_video_frames >= 6 || video_done)
                && standby_video.is_none()
                && standby_audio.is_none()
            {
                match open_decoders(&canonical, decode_mode, config.in_ms, has_video, has_audio) {
                    Ok((v, a)) => {
                        standby_video = v;
                        standby_audio = a;
                    }
                    Err(err) => {
                        tracing::warn!(%err, "failed to pre-open standby loop decoders");
                    }
                }
            }

            let need_video = if video_done {
                false
            } else {
                video_state
                    .0
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .queue
                    .len()
                    < VIDEO_QUEUE_TARGET
            };
            if need_video && let Some(vd) = current_video.as_mut() {
                match vd.next_frame() {
                    Ok(Some(vf)) => {
                        if out_s.is_some_and(|o| vf.seconds >= o - 1e-4) {
                            video_done = true;
                            let (mtx, cv) = &*video_state;
                            let mut vs = mtx.lock().unwrap_or_else(|p| p.into_inner());
                            if !config.loop_media {
                                vs.eos = true;
                            }
                            vs.initial_ready = true;
                            vs.completed_seek_seq = vs.completed_seek_seq.max(active_seek_seq);
                            cv.notify_all();
                        } else {
                            if let Some(prev) = prev_video_pts_s {
                                let dt = vf.seconds - prev;
                                if dt > 0.001 && dt < 0.5 {
                                    est_period_s = dt;
                                }
                            }
                            prev_video_pts_s = Some(vf.seconds);
                            let rel_s = (vf.seconds - in_s).max(0.0);
                            last_video_rel_s = Some(rel_s);
                            let play_s = lap_base_s + rel_s;
                            let frame = TimedVideoFrame {
                                nv12: vf.into_nv12(),
                                play_seconds: play_s,
                                sequence: next_seq,
                                lap,
                            };
                            next_seq = next_seq.saturating_add(1);
                            lap_video_frames += 1;
                            {
                                let (mtx, cv) = &*video_state;
                                let mut vs = mtx.lock().unwrap_or_else(|p| p.into_inner());
                                vs.queue.push_back(frame);
                                vs.last_decoded_play_s = Some(play_s);
                                vs.initial_ready = true;
                                vs.completed_seek_seq = vs.completed_seek_seq.max(active_seek_seq);
                                cv.notify_all();
                            }
                        }
                    }
                    Ok(None) => {
                        video_done = true;
                        let (mtx, cv) = &*video_state;
                        let mut vs = mtx.lock().unwrap_or_else(|p| p.into_inner());
                        if !config.loop_media {
                            vs.eos = true;
                        }
                        vs.initial_ready = true;
                        vs.completed_seek_seq = vs.completed_seek_seq.max(active_seek_seq);
                        cv.notify_all();
                    }
                    Err(err) => {
                        tracing::error!(%err, "video decode failed");
                        video_done = true;
                        let (mtx, cv) = &*video_state;
                        let mut vs = mtx.lock().unwrap_or_else(|p| p.into_inner());
                        if !config.loop_media {
                            vs.eos = true;
                        }
                        vs.initial_ready = true;
                        vs.completed_seek_seq = vs.completed_seek_seq.max(active_seek_seq);
                        cv.notify_all();
                    }
                }
            }

            let need_audio = if audio_done {
                false
            } else {
                audio_state
                    .0
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .pending_source_seconds
                    < AUDIO_BUFFER_TARGET_S
            };
            if need_audio && let Some(ad) = current_audio.as_mut() {
                match ad.next_chunk() {
                    Ok(Some(mut chunk)) => {
                        audio_rate_hint = chunk.sample_rate.max(1);
                        audio_ch_hint = chunk.channels.max(1);
                        let chunk_rel_start = chunk.start_seconds - in_s;
                        let cap_rel_s = if let Some(o) = out_s {
                            Some((o - in_s).max(est_period_s))
                        } else if video_done {
                            last_video_rel_s.map(|last| last + est_period_s)
                        } else {
                            None
                        };

                        if chunk_rel_start < 0.0 {
                            let dur = chunk.frames() as f64 / f64::from(audio_rate_hint);
                            if chunk_rel_start + dur <= 0.0 {
                                continue;
                            }
                            let skip_frames = ((-chunk_rel_start * f64::from(audio_rate_hint))
                                .round() as usize)
                                .min(chunk.frames());
                            chunk
                                .samples
                                .drain(..skip_frames * usize::from(audio_ch_hint));
                        }
                        let eff_start = chunk_rel_start.max(audio_written_rel_s);
                        if let Some(cap) = cap_rel_s {
                            if eff_start >= cap - (0.5 / f64::from(audio_rate_hint)) {
                                audio_done = true;
                                continue;
                            }
                            let max_frames = (((cap - eff_start) * f64::from(audio_rate_hint))
                                .round() as usize)
                                .max(1);
                            if chunk.frames() > max_frames {
                                chunk
                                    .samples
                                    .truncate(max_frames * usize::from(audio_ch_hint));
                                audio_done = true;
                            }
                        }

                        if !chunk.samples.is_empty() {
                            let (mtx, cv) = &*audio_state;
                            let mut a = mtx.lock().unwrap_or_else(|p| p.into_inner());
                            let frames = chunk.frames();
                            if eff_start > audio_written_rel_s + 0.002 {
                                let gap_frames = ((eff_start - audio_written_rel_s)
                                    * f64::from(audio_rate_hint))
                                .round() as usize;
                                if gap_frames > 0 && gap_frames < audio_rate_hint as usize * 4 {
                                    a.push_chunk(QueuedAudioChunk {
                                        sample_rate: audio_rate_hint,
                                        channels: audio_ch_hint,
                                        samples: vec![
                                            0.0_f32;
                                            gap_frames * usize::from(audio_ch_hint)
                                        ],
                                    });
                                    audio_written_rel_s +=
                                        gap_frames as f64 / f64::from(audio_rate_hint);
                                }
                            }
                            a.push_chunk(QueuedAudioChunk {
                                sample_rate: chunk.sample_rate,
                                channels: chunk.channels,
                                samples: chunk.samples,
                            });
                            audio_written_rel_s += frames as f64 / f64::from(audio_rate_hint);
                            cv.notify_all();
                        }
                    }
                    Ok(None) => {
                        audio_done = true;
                    }
                    Err(err) => {
                        tracing::error!(%err, "audio decode failed");
                        audio_done = true;
                    }
                }
            }

            if (video_done || !need_video)
                && (audio_done || !need_audio)
                && !(video_done && audio_done)
            {
                let (mtx, cv) = &*mailbox;
                let g = mtx.lock().unwrap_or_else(|p| p.into_inner());
                if g.seek_to_ms.is_none() {
                    let _ = cv.wait_timeout(g, Duration::from_millis(2));
                }
            }
        }

        if stop.load(Ordering::Acquire) {
            break;
        }

        let lap_dur_s = if let Some(o) = out_s {
            (o - in_s).max(est_period_s)
        } else if let Some(last) = last_video_rel_s {
            last + est_period_s
        } else {
            audio_written_rel_s.max(est_period_s)
        };

        if has_audio && audio_written_rel_s + (0.5 / f64::from(audio_rate_hint)) < lap_dur_s {
            let pad_frames =
                ((lap_dur_s - audio_written_rel_s) * f64::from(audio_rate_hint)).round() as usize;
            if pad_frames > 0 && pad_frames < (audio_rate_hint as usize) * 30 {
                let (mtx, cv) = &*audio_state;
                let mut a = mtx.lock().unwrap_or_else(|p| p.into_inner());
                a.push_chunk(QueuedAudioChunk {
                    sample_rate: audio_rate_hint,
                    channels: audio_ch_hint,
                    samples: vec![0.0_f32; pad_frames * usize::from(audio_ch_hint)],
                });
                cv.notify_all();
            }
        }

        if config.loop_media {
            lap = lap.saturating_add(1);
            lap_base_s += lap_dur_s;
            audio_written_rel_s = 0.0;
            if let Some(sv) = standby_video.take() {
                current_video = Some(sv);
            } else if has_video {
                match open_decoders(&canonical, decode_mode, config.in_ms, true, false) {
                    Ok((v, _)) => current_video = v,
                    Err(err) => {
                        tracing::error!(%err, "failed reopening loop video");
                        break;
                    }
                }
            }
            if let Some(sa) = standby_audio.take() {
                current_audio = Some(sa);
            } else if has_audio {
                match open_decoders(&canonical, decode_mode, config.in_ms, false, true) {
                    Ok((_, a)) => current_audio = a,
                    Err(err) => {
                        tracing::error!(%err, "failed reopening loop audio");
                        break;
                    }
                }
            }
            continue;
        }

        {
            let (mtx, cv) = &*video_state;
            let mut vs = mtx.lock().unwrap_or_else(|p| p.into_inner());
            vs.eos = true;
            vs.initial_ready = true;
            cv.notify_all();
        }
        {
            let (mtx, cv) = &*audio_state;
            let mut a = mtx.lock().unwrap_or_else(|p| p.into_inner());
            a.eos = true;
            cv.notify_all();
        }

        // Non-looping clip hit EOS: wait cleanly for either Seek or Stop.
        let (mtx, cv) = &*mailbox;
        let guard = mtx.lock().unwrap_or_else(|p| p.into_inner());
        if guard.seek_to_ms.is_none() && !stop.load(Ordering::Acquire) {
            let _ = cv.wait_timeout(guard, Duration::from_millis(50));
        } else {
            continue;
        }
        while !stop.load(Ordering::Acquire) {
            let g = mtx.lock().unwrap_or_else(|p| p.into_inner());
            if g.seek_to_ms.is_some() {
                break;
            }
            let _ = cv.wait_timeout(g, Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::measure_true_peak_dbtp;

    fn flash_beep_clip() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/media/committed/flash-beep-360p60.mp4")
    }

    fn loop_1080p_clip() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/media/committed/h264-1080p60.mp4")
    }

    #[test]
    fn flash_and_beep_stay_synchronized_within_one_frame_at_half_one_and_two_x_speed() {
        let clip = flash_beep_clip();
        for speed in [1.0_f32, 0.5_f32, 2.0_f32] {
            let player = MediaPlayer::open(
                PlaybackConfig {
                    path: clip.clone(),
                    decode_mode: DecodeMode::Software,
                    in_ms: 0,
                    out_ms: None,
                    loop_media: false,
                    speed,
                    volume: 1.0,
                    muted: false,
                    paused: false,
                    start_position_ms: 0,
                },
                PlaybackOutputMode::SoftwareCapture {
                    sample_rate: 48_000,
                    channels: 2,
                },
            )
            .expect("open flash-beep clip");

            // Step output audio frames in ~5 ms chunks (240 output frames) across the whole clip.
            let step_frames = 240_usize;
            let total_steps =
                ((1.10 / f64::from(speed)) * 48_000.0 / step_frames as f64).ceil() as usize;
            let mut flash_wall_s: Option<f64> = None;
            let mut beep_wall_s: Option<f64> = None;

            for step in 0..total_steps {
                let wall_s = (step * step_frames) as f64 / 48_000.0;
                if let Some(frame) = player.frame_for_now() {
                    // Flash frame has full white corners (luma >= 230).
                    let corner_luma = frame.nv12.y[8 * (frame.nv12.width as usize) + 8];
                    if corner_luma >= 230 && flash_wall_s.is_none() {
                        flash_wall_s = Some(wall_s);
                    }
                }
                let pcm = player.pump_audio_frames(step_frames);
                let rms =
                    (pcm.iter().step_by(2).map(|v| v * v).sum::<f32>() / step_frames as f32).sqrt();
                if rms > 0.12 && beep_wall_s.is_none() {
                    beep_wall_s = Some(wall_s);
                }
            }

            let t_flash = flash_wall_s.expect("detected white flash frame");
            let t_beep = beep_wall_s.expect("detected 1 kHz beep chunk");
            let diff = (t_flash - t_beep).abs();
            // +/- 1 output frame at source timeline = (1/60) / speed seconds (+ 5ms chunk quantum).
            let tolerance = (1.0 / 60.0) / f64::from(speed) + 0.0055;
            assert!(
                diff <= tolerance,
                "speed={speed}: t_flash={t_flash:.4}s, t_beep={t_beep:.4}s, diff={diff:.4}s > {tolerance:.4}s"
            );
        }
    }

    #[test]
    fn loop_1080p_has_zero_frame_gap_at_loop_point() {
        let player = MediaPlayer::open(
            PlaybackConfig {
                path: loop_1080p_clip(),
                decode_mode: DecodeMode::Software,
                in_ms: 0,
                out_ms: None,
                loop_media: true,
                speed: 1.0,
                volume: DEFAULT_SAFE_VOLUME,
                muted: false,
                paused: false,
                start_position_ms: 0,
            },
            PlaybackOutputMode::SoftwareCapture {
                sample_rate: 48_000,
                channels: 2,
            },
        )
        .expect("open 1080p clip");

        // Tick at exact 60 fps (800 output samples at 48 kHz) across 2 full laps (120 frames).
        let mut seqs = Vec::with_capacity(120);
        let mut laps = Vec::with_capacity(120);
        let mut times = Vec::with_capacity(120);
        for tick in 0..120 {
            let frame = player
                .frame_for_now()
                .unwrap_or_else(|| panic!("missing frame at tick {tick}"));
            seqs.push(frame.sequence);
            laps.push(frame.lap);
            times.push(frame.play_seconds);
            player.pump_audio_frames(800);
        }

        // Zero duplicates and zero skipped frames: tick n renders sequence n, with lap switching at tick 60.
        let expected_seqs: Vec<u64> = (0..120).collect();
        assert_eq!(seqs, expected_seqs, "frame gap or repeat at loop boundary");
        assert_eq!(laps[59], 0);
        assert_eq!(laps[60], 1);
        let seam_dt = times[60] - times[59];
        assert!(
            (seam_dt - (1.0 / 60.0)).abs() <= 0.001,
            "loop seam delta should be 1 frame (0.0167s), got {seam_dt:.5}s"
        );
    }

    #[test]
    fn in_and_out_points_trim_and_loop_cleanly() {
        // Trim [450ms, 600ms] around the flash frame at 500ms and loop twice.
        let player = MediaPlayer::open(
            PlaybackConfig {
                path: flash_beep_clip(),
                decode_mode: DecodeMode::Software,
                in_ms: 450,
                out_ms: Some(600),
                loop_media: true,
                speed: 1.0,
                volume: DEFAULT_SAFE_VOLUME,
                muted: false,
                paused: false,
                start_position_ms: 450,
            },
            PlaybackOutputMode::SoftwareCapture {
                sample_rate: 48_000,
                channels: 2,
            },
        )
        .expect("open trimmed clip");

        let mut lap0_flash = false;
        let mut lap1_flash = false;
        // 300 ms = 2 full 150 ms laps, stepped at 60 fps (800 samples/tick).
        for _tick in 0..18 {
            if let Some(frame) = player.frame_for_now() {
                let corner = frame.nv12.y[8 * (frame.nv12.width as usize) + 8];
                if corner >= 230 {
                    if frame.lap == 0 {
                        lap0_flash = true;
                    } else if frame.lap == 1 {
                        lap1_flash = true;
                    }
                }
            }
            player.pump_audio_frames(800);
        }
        assert!(
            lap0_flash && lap1_flash,
            "expected flash in both trimmed laps"
        );
    }

    #[test]
    fn clear_fade_out_silences_and_finishes_player_cleanly() {
        let player = MediaPlayer::open(
            PlaybackConfig {
                path: flash_beep_clip(),
                decode_mode: DecodeMode::Software,
                in_ms: 480,
                out_ms: Some(900),
                loop_media: false,
                speed: 1.0,
                volume: 1.2,
                muted: false,
                paused: false,
                start_position_ms: 480,
            },
            PlaybackOutputMode::SoftwareCapture {
                sample_rate: 48_000,
                channels: 2,
            },
        )
        .expect("open clip");

        // Pump into the beep (25 ms) so non-zero signal is playing.
        let pre = player.pump_audio_frames(1_200);
        let tp_pre = measure_true_peak_dbtp(&pre, 2);
        assert!(
            tp_pre < -1.0,
            "limiter active during active playback ({tp_pre:.2} dBTP)"
        );

        player.fade_out_and_stop(25);
        // Pump 40 ms so the 25 ms fade completes plus lookahead tail.
        for _ in 0..4 {
            player.pump_audio_frames(480);
        }
        assert!(
            player.is_finished(),
            "fade out must complete and finish player"
        );
        let tail = player.pump_audio_frames(480);
        assert!(
            tail.iter().all(|s| s.abs() <= 1e-6),
            "post-fade output must remain silent"
        );
    }

    #[test]
    fn opening_url_in_media_player_is_refused() {
        let err = MediaPlayer::open(
            PlaybackConfig {
                path: PathBuf::from("http://127.0.0.1:9/live.mp4"),
                ..Default::default()
            },
            PlaybackOutputMode::SoftwareCapture {
                sample_rate: 48_000,
                channels: 2,
            },
        )
        .err()
        .expect("URL must be refused by SEC-04 check");
        assert!(matches!(err, MediaError::NotLocal(_)));
    }

    #[test]
    fn seek_pause_and_speed_change_mid_playback_work_cleanly() {
        let player = MediaPlayer::open(
            PlaybackConfig {
                path: flash_beep_clip(),
                decode_mode: DecodeMode::Software,
                in_ms: 0,
                out_ms: None,
                loop_media: false,
                speed: 1.0,
                volume: DEFAULT_SAFE_VOLUME,
                muted: false,
                paused: false,
                start_position_ms: 0,
            },
            PlaybackOutputMode::SoftwareCapture {
                sample_rate: 48_000,
                channels: 2,
            },
        )
        .expect("open clip");

        let f0 = player.frame_for_now().expect("initial frame");
        assert!(f0.play_seconds < 0.05);

        player.set_paused(true);
        let t_paused_before = player.clock_seconds();
        let silence = player.pump_audio_frames(4_800);
        assert!(silence.iter().all(|s| s.abs() <= 1e-6));
        assert!((player.clock_seconds() - t_paused_before).abs() < 1e-6);

        player.set_paused(false);
        player.set_speed(2.0);
        // Seek right before the white flash at n=30 (500 ms) -> next frames should hit white flash.
        player.seek_ms(490);
        let post_seek = player.frame_for_now().expect("post-seek frame");
        assert!(
            post_seek.play_seconds >= 0.45 && post_seek.play_seconds <= 0.55,
            "unexpected post-seek pts {}",
            post_seek.play_seconds
        );
        let mut saw_flash = false;
        for _ in 0..10 {
            if let Some(frame) = player.frame_for_now()
                && frame.nv12.y[8 * (frame.nv12.width as usize) + 8] >= 230
            {
                saw_flash = true;
                break;
            }
            player.pump_audio_frames(400);
        }
        assert!(
            saw_flash,
            "expected white flash immediately after seek_ms(490)"
        );
    }

    #[test]
    fn software_pump_drives_clock_and_frames_asynchronously() {
        let player = MediaPlayer::open(
            PlaybackConfig {
                path: flash_beep_clip(),
                decode_mode: DecodeMode::Software,
                in_ms: 0,
                out_ms: None,
                loop_media: true,
                speed: 1.0,
                volume: DEFAULT_SAFE_VOLUME,
                muted: true,
                paused: false,
                start_position_ms: 0,
            },
            PlaybackOutputMode::SoftwarePump,
        )
        .expect("open clip with software pump");

        let mut last_seq = None;
        let first = player.frame_if_changed(&mut last_seq).expect("first frame");
        let start = Instant::now();
        let mut changed = 0_usize;
        while start.elapsed() < Duration::from_millis(120) {
            if player.frame_if_changed(&mut last_seq).is_some() {
                changed += 1;
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        assert!(
            changed >= 3 && player.clock_seconds() > first.play_seconds + 0.04,
            "expected async software pump to advance clock & frames, changed={changed}, clock={:.4}",
            player.clock_seconds()
        );
    }
}
