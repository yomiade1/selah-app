#![deny(unsafe_code)]
//! Media decoding, probing and thumbnailing through FFmpeg and Skia (M1.7, M2.6, ADR-005),
//! limited by SEC-04:
//!
//! - **Local files only.** [`check_local_path`] refuses URLs, protocol prefixes, and symlinks
//!   before FFmpeg sees the path, and FFmpeg opens it with `protocol_whitelist=file,pipe`, so a
//!   file can't pull in a network resource either (an HLS playlist pointing at `http://` fails).
//! - **Allowlisted demuxers and decoders.** [`DEMUXERS`] goes to FFmpeg as `format_whitelist`;
//!   [`VIDEO_DECODERS`] and [`AUDIO_DECODERS`] are checked before a decoder opens and form
//!   FFmpeg's `codec_whitelist`.
//!
//! Frames come out as 8-bit NV12 ([`VideoFrame`]): the compositor uploads the two planes and
//! converts them to RGB in a shader. Hardware decode (D3D11VA on Windows, VideoToolbox on
//! macOS) is tried first when asked for, and falls back to software decode.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Once;

use ffmpeg::format::Pixel;
use ffmpeg::util::color;
use ffmpeg_next as ffmpeg;
use serde::{Deserialize, Serialize};

pub mod audio;
mod hw;
pub mod player;

/// FFmpeg demuxers a file item may use. Names as FFmpeg reports them.
pub const DEMUXERS: &[&str] = &[
    "mov", "mp4", "m4a", "3gp", "3g2", "mj2", "matroska", "webm", "mp3", "wav", "flac", "ogg",
];

/// Video decoders a file item may use.
pub const VIDEO_DECODERS: &[&str] = &["h264", "hevc", "vp9", "av1", "prores"];

/// Audio decoders a file item may use (audio playback arrives in M3.4 and M5.7; probing a
/// video or audio file checks its audio stream's decoder against this allowlist).
pub const AUDIO_DECODERS: &[&str] = &[
    "aac",
    "mp3float",
    "mp3",
    "opus",
    "flac",
    "vorbis",
    "pcm_s16le",
    "pcm_s24le",
    "pcm_s32le",
    "pcm_f32le",
    "pcm_u8",
];

/// Protocols FFmpeg may use for a file item (SEC-04). Network protocols are for stream items,
/// which arrive in M5 and go through `selah-net`.
pub const FILE_PROTOCOLS: &str = "file,pipe";

/// The largest frame the decoder accepts (8K), so a crafted header can't ask for huge buffers.
pub const MAX_DIMENSION: u32 = 8192;

/// Maximum encoded image file read when probing or thumbnailing (128 MiB).
pub const MAX_IMAGE_FILE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("only local files can be opened as file items (SEC-04): {0}")]
    NotLocal(String),
    #[error("the file doesn't exist, is a symlink, or isn't a regular file: {0}")]
    NotAFile(PathBuf),
    #[error("FFmpeg refused the file: {0}")]
    Open(ffmpeg::Error),
    #[error("the file has no video stream")]
    NoVideo,
    #[error("the file has no audio stream")]
    NoAudio,
    #[error("the {0} decoder isn't allowed (SEC-04)")]
    DecoderNotAllowed(String),
    #[error("the video is {0}×{1}, larger than the {MAX_DIMENSION} pixel limit (SEC-04)")]
    TooLarge(u32, u32),
    #[error("unsupported pixel format {0:?} (M1 handles 8-bit 4:2:0 video)")]
    PixelFormat(Pixel),
    #[error("decoding failed: {0}")]
    Decode(ffmpeg::Error),
    #[error("invalid or unsupported image: {0}")]
    Image(String),
    #[error("I/O error reading media file: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, MediaError>;

/// Detected kind of a probed media file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaProbeKind {
    Image,
    Video,
    Audio,
}

/// Metadata and optional PNG thumbnail produced by probing a local media file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaProbeResult {
    pub kind: MediaProbeKind,
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
    pub codec: String,
    pub container: String,
    pub thumbnail_width: u32,
    pub thumbnail_height: u32,
    pub thumbnail_png: Vec<u8>,
}

/// How to decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeMode {
    /// Hardware decode when the platform has it, else software.
    Auto,
    /// Software decode only (cloud sessions and CI, which have no video hardware).
    Software,
}

/// The YUV → RGB matrix of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matrix {
    Bt601,
    Bt709,
}

/// One decoded frame as NV12: a full-size luma plane and a half-size interleaved chroma plane,
/// both tightly packed.
#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    /// `width × height` bytes.
    pub y: Vec<u8>,
    /// `chroma_width() × 2 × chroma_height()` bytes, U then V.
    pub uv: Vec<u8>,
    pub matrix: Matrix,
    /// True for full-range (0–255) luma; most video is limited range (16–235).
    pub full_range: bool,
    /// Presentation time in seconds from the start of the stream.
    pub seconds: f64,
    /// Position in decode output order, from 0.
    pub index: u64,
    /// The frame was decoded in hardware.
    pub hardware: bool,
}

impl VideoFrame {
    pub fn chroma_width(&self) -> u32 {
        self.width.div_ceil(2)
    }

    pub fn chroma_height(&self) -> u32 {
        self.height.div_ceil(2)
    }

    /// The compositor's frame type (moves the planes, no copy).
    pub fn into_nv12(self) -> selah_render::video::Nv12Frame {
        selah_render::video::Nv12Frame {
            width: self.width,
            height: self.height,
            y: self.y,
            uv: self.uv,
            matrix: match self.matrix {
                Matrix::Bt601 => selah_render::video::YuvMatrix::Bt601,
                Matrix::Bt709 => selah_render::video::YuvMatrix::Bt709,
            },
            full_range: self.full_range,
        }
    }
}

pub(crate) fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if let Err(error) = ffmpeg::init() {
            tracing::error!(%error, "FFmpeg didn't initialise");
        }
        // FFmpeg's own log goes to stderr; keep it to errors.
        ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Error);
    });
}

/// SEC-04: accept only a path to an existing local non-symlink regular file. Anything that looks
/// like a URL or an FFmpeg protocol prefix (`http:`, `concat:`, `subfile,`) is refused, whatever
/// the file system says. A single drive letter (`C:\…`) is a path, not a protocol.
pub fn check_local_path(path: &Path) -> Result<PathBuf> {
    let text = path.to_string_lossy();
    if let Some(colon) = text.find(':') {
        let prefix = &text[..colon];
        let drive_letter = cfg!(windows) && prefix.len() == 1;
        let looks_like_scheme = !prefix.is_empty()
            && prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-,".contains(c));
        if looks_like_scheme && !drive_letter {
            return Err(MediaError::NotLocal(text.into_owned()));
        }
    }
    if text.contains("://") {
        return Err(MediaError::NotLocal(text.into_owned()));
    }
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| MediaError::NotAFile(path.to_owned()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(MediaError::NotAFile(path.to_owned()));
    }
    let canonical =
        std::fs::canonicalize(path).map_err(|_| MediaError::NotAFile(path.to_owned()))?;
    if !canonical.is_file() {
        return Err(MediaError::NotAFile(path.to_owned()));
    }
    Ok(canonical)
}

/// FFmpeg's name for a local path: `file:` forces the file protocol, so nothing in the name
/// can select another one. `canonicalize` on Windows returns `\\?\C:\…`; FFmpeg wants `C:\…`.
pub(crate) fn ffmpeg_url(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    format!("file:{text}")
}

pub(crate) fn open_options() -> ffmpeg::Dictionary<'static> {
    let mut options = ffmpeg::Dictionary::new();
    options.set("protocol_whitelist", FILE_PROTOCOLS);
    options.set("format_whitelist", &DEMUXERS.join(","));
    let codecs: Vec<&str> = VIDEO_DECODERS
        .iter()
        .chain(AUDIO_DECODERS)
        .copied()
        .collect();
    options.set("codec_whitelist", &codecs.join(","));
    options
}

pub(crate) fn check_demuxer(input: &ffmpeg::format::context::Input) -> Result<String> {
    let demuxer_name = input.format().name().to_owned();
    if !demuxer_name.split(',').any(|name| DEMUXERS.contains(&name)) {
        return Err(MediaError::Open(ffmpeg::Error::DemuxerNotFound));
    }
    Ok(demuxer_name)
}

pub(crate) fn duration_milliseconds(
    input: &ffmpeg::format::context::Input,
    stream: &ffmpeg::Stream<'_>,
) -> u64 {
    let container_dur = input.duration();
    if container_dur > 0 && container_dur != ffmpeg::ffi::AV_NOPTS_VALUE {
        return (container_dur as u64) / 1000;
    }
    let stream_dur = stream.duration();
    if stream_dur > 0 && stream_dur != ffmpeg::ffi::AV_NOPTS_VALUE {
        let tb = f64::from(stream.time_base());
        if tb.is_finite() && tb > 0.0 {
            let ms = (stream_dur as f64) * tb * 1000.0;
            if ms.is_finite() && ms >= 0.0 {
                return ms.round() as u64;
            }
        }
    }
    0
}

/// A video file opened for decoding.
pub struct VideoDecoder {
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Video,
    stream_index: usize,
    time_base: f64,
    start: Option<i64>,
    frame_rate: f64,
    duration_ms: u64,
    hardware: Option<hw::Device>,
    eof_sent: bool,
    next_index: u64,
    skip_until_seconds: f64,
    decoder_name: String,
    demuxer_name: String,
}

impl VideoDecoder {
    /// Open the best video stream of a local file.
    pub fn open(path: &Path, mode: DecodeMode) -> Result<Self> {
        init();
        let path = check_local_path(path)?;
        let input = ffmpeg::format::input_with_dictionary(&ffmpeg_url(&path), open_options())
            .map_err(MediaError::Open)?;
        let demuxer_name = check_demuxer(&input)?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(MediaError::NoVideo)?;
        let stream_index = stream.index();
        let time_base = f64::from(stream.time_base());
        let start = Some(stream.start_time()).filter(|t| *t != ffmpeg::ffi::AV_NOPTS_VALUE);
        let frame_rate = f64::from(stream.avg_frame_rate());
        let duration_ms = duration_milliseconds(&input, &stream);
        let parameters = stream.parameters();
        let codec = ffmpeg::decoder::find(parameters.id())
            .ok_or_else(|| MediaError::DecoderNotAllowed(format!("{:?}", parameters.id())))?;
        let decoder_name = codec.name().to_owned();
        if !VIDEO_DECODERS.contains(&decoder_name.as_str()) {
            return Err(MediaError::DecoderNotAllowed(decoder_name));
        }
        let mut context = ffmpeg::codec::context::Context::from_parameters(parameters)
            .map_err(MediaError::Open)?;
        let hardware = match mode {
            DecodeMode::Auto => hw::Device::attach(&mut context),
            DecodeMode::Software => None,
        };
        if hardware.is_none() {
            context.set_threading(ffmpeg::codec::threading::Config::kind(
                ffmpeg::codec::threading::Type::Frame,
            ));
        }
        let decoder = context.decoder().video().map_err(MediaError::Open)?;
        if decoder.width() == 0
            || decoder.height() == 0
            || decoder.width() > MAX_DIMENSION
            || decoder.height() > MAX_DIMENSION
        {
            return Err(MediaError::TooLarge(decoder.width(), decoder.height()));
        }
        tracing::info!(
            demuxer = %demuxer_name,
            decoder = %decoder_name,
            hardware = hardware.as_ref().map(hw::Device::name),
            width = decoder.width(),
            height = decoder.height(),
            "video opened"
        );
        Ok(Self {
            input,
            decoder,
            stream_index,
            time_base,
            start,
            frame_rate,
            duration_ms,
            hardware,
            eof_sent: false,
            next_index: 0,
            skip_until_seconds: 0.0,
            decoder_name,
            demuxer_name,
        })
    }

    pub fn width(&self) -> u32 {
        self.decoder.width()
    }

    pub fn height(&self) -> u32 {
        self.decoder.height()
    }

    /// Average frame rate from the container (0 when unknown).
    pub fn frame_rate(&self) -> f64 {
        self.frame_rate
    }

    /// Stream/container duration in milliseconds (0 when unknown).
    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    pub fn decoder_name(&self) -> &str {
        &self.decoder_name
    }

    pub fn demuxer_name(&self) -> &str {
        &self.demuxer_name
    }

    /// The hardware device in use (`d3d11va`, `videotoolbox`), if any.
    pub fn hardware(&self) -> Option<&'static str> {
        self.hardware.as_ref().map(hw::Device::name)
    }

    /// Seek to `position_ms` on the container and skip catchup frames earlier than `position_ms`.
    pub fn seek_ms(&mut self, position_ms: u64) -> Result<()> {
        if position_ms == 0 && self.next_index == 0 && !self.eof_sent {
            self.skip_until_seconds = 0.0;
            return Ok(());
        }
        let target_us = (position_ms as i64).saturating_mul(1000);
        let _ = self.input.seek(target_us, ..target_us.saturating_add(1));
        self.decoder.flush();
        self.eof_sent = false;
        self.skip_until_seconds = if position_ms == 0 {
            0.0
        } else {
            (position_ms as f64 / 1000.0) - 0.000_5
        };
        Ok(())
    }

    /// The next frame in presentation order, or `None` at the end of the stream.
    pub fn next_frame(&mut self) -> Result<Option<VideoFrame>> {
        let mut decoded = ffmpeg::frame::Video::empty();
        loop {
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    if self.skip_until_seconds > 0.0 {
                        let pts = decoded.timestamp().or(decoded.pts()).unwrap_or(0);
                        let seconds = (pts - self.start.unwrap_or(0)) as f64 * self.time_base;
                        if seconds < self.skip_until_seconds {
                            continue;
                        }
                    }
                    return self.convert(decoded).map(Some);
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

    fn convert(&mut self, decoded: ffmpeg::frame::Video) -> Result<VideoFrame> {
        let hardware = self
            .hardware
            .as_ref()
            .is_some_and(|device| decoded.format() == device.pixel_format());
        let frame = if hardware {
            hw::download(&decoded).map_err(MediaError::Decode)?
        } else {
            decoded
        };
        let (width, height) = (frame.width(), frame.height());
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(MediaError::TooLarge(width, height));
        }
        let (y, uv) = match frame.format() {
            Pixel::NV12 => (
                copy_plane(&frame, 0, width as usize, height as usize),
                copy_plane(
                    &frame,
                    1,
                    width.div_ceil(2) as usize * 2,
                    height.div_ceil(2) as usize,
                ),
            ),
            Pixel::YUV420P | Pixel::YUVJ420P => (
                copy_plane(&frame, 0, width as usize, height as usize),
                interleave(
                    &frame,
                    width.div_ceil(2) as usize,
                    height.div_ceil(2) as usize,
                ),
            ),
            other => return Err(MediaError::PixelFormat(other)),
        };
        let matrix = match frame.color_space() {
            color::Space::BT470BG | color::Space::SMPTE170M => Matrix::Bt601,
            color::Space::BT709 => Matrix::Bt709,
            // Unspecified: HD and larger is BT.709, SD is BT.601 (the usual convention).
            _ if height > 576 => Matrix::Bt709,
            _ => Matrix::Bt601,
        };
        let full_range =
            frame.color_range() == color::Range::JPEG || frame.format() == Pixel::YUVJ420P;
        let pts = frame.timestamp().or(frame.pts()).unwrap_or(0);
        let seconds = (pts - self.start.unwrap_or(0)) as f64 * self.time_base;
        let index = self.next_index;
        self.next_index += 1;
        Ok(VideoFrame {
            width,
            height,
            y,
            uv,
            matrix,
            full_range,
            seconds,
            index,
            hardware,
        })
    }
}

/// Probe a local media file (image, video, or audio) and optionally render a bounded PNG
/// thumbnail (`max_width > 0 && max_height > 0`) using software decoding and Skia (SEC-04).
pub fn probe_and_thumbnail(
    path: &Path,
    kind_hint: Option<MediaProbeKind>,
    max_width: u32,
    max_height: u32,
) -> Result<MediaProbeResult> {
    let canonical = check_local_path(path)?;
    let ext = canonical
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    let kind = kind_hint.unwrap_or(match ext.as_str() {
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "ico" | "webp" => MediaProbeKind::Image,
        "mp3" | "wav" | "flac" | "ogg" | "opus" | "m4a" | "aac" => MediaProbeKind::Audio,
        _ => MediaProbeKind::Video,
    });

    match kind {
        MediaProbeKind::Image => probe_image(&canonical, &ext, max_width, max_height),
        MediaProbeKind::Video => match probe_video(&canonical, max_width, max_height) {
            Ok(result) => Ok(result),
            Err(MediaError::NoVideo) if kind_hint.is_none() => probe_audio(&canonical),
            Err(err) => Err(err),
        },
        MediaProbeKind::Audio => probe_audio(&canonical),
    }
}

fn probe_image(
    canonical: &Path,
    ext: &str,
    max_width: u32,
    max_height: u32,
) -> Result<MediaProbeResult> {
    let file = std::fs::File::open(canonical)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_IMAGE_FILE_BYTES {
        return Err(MediaError::Image(format!(
            "image file exceeds {MAX_IMAGE_FILE_BYTES} bytes"
        )));
    }
    let mut bytes = Vec::with_capacity((meta.len() as usize).min(4 * 1024 * 1024));
    file.take(MAX_IMAGE_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_IMAGE_FILE_BYTES {
        return Err(MediaError::Image(format!(
            "image file exceeds {MAX_IMAGE_FILE_BYTES} bytes"
        )));
    }

    if max_width > 0 && max_height > 0 {
        let thumb = selah_render::png::thumbnail_image(&bytes, max_width, max_height)
            .map_err(|e| MediaError::Image(e.to_string()))?;
        Ok(MediaProbeResult {
            kind: MediaProbeKind::Image,
            width: thumb.source_width,
            height: thumb.source_height,
            duration_ms: 0,
            codec: ext.to_owned(),
            container: ext.to_owned(),
            thumbnail_width: thumb.width,
            thumbnail_height: thumb.height,
            thumbnail_png: thumb.png_bytes,
        })
    } else {
        let (width, height) = selah_render::png::probe_image_dimensions(&bytes)
            .map_err(|e| MediaError::Image(e.to_string()))?;
        Ok(MediaProbeResult {
            kind: MediaProbeKind::Image,
            width,
            height,
            duration_ms: 0,
            codec: ext.to_owned(),
            container: ext.to_owned(),
            thumbnail_width: 0,
            thumbnail_height: 0,
            thumbnail_png: Vec::new(),
        })
    }
}

fn fit_stage_size(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let bound_w = (max_w.clamp(16, 1024)) * 2;
    let bound_h = (max_h.clamp(16, 1024)) * 2;
    if src_w <= bound_w && src_h <= bound_h {
        return (src_w.max(1), src_h.max(1));
    }
    let scale = (f64::from(bound_w) / f64::from(src_w)).min(f64::from(bound_h) / f64::from(src_h));
    let w = ((f64::from(src_w) * scale).round() as u32).clamp(1, src_w);
    let h = ((f64::from(src_h) * scale).round() as u32).clamp(1, src_h);
    (w, h)
}

fn probe_video(canonical: &Path, max_width: u32, max_height: u32) -> Result<MediaProbeResult> {
    let mut decoder = VideoDecoder::open(canonical, DecodeMode::Software)?;
    let width = decoder.width();
    let height = decoder.height();
    let duration_ms = decoder.duration_ms();
    let codec = decoder.decoder_name().to_owned();
    let container = decoder.demuxer_name().to_owned();

    let first_frame = decoder.next_frame()?.ok_or(MediaError::NoVideo)?;
    let (thumbnail_width, thumbnail_height, thumbnail_png) = if max_width > 0 && max_height > 0 {
        let nv12 = first_frame.into_nv12();
        let (stage_w, stage_h) = fit_stage_size(nv12.width, nv12.height, max_width, max_height);
        let mut frame = selah_render::Frame::new(stage_w, stage_h);
        selah_render::video::draw_cpu(&mut frame, &nv12);
        selah_render::png::encode_rgba_thumbnail(&frame, max_width, max_height)
            .map_err(|e| MediaError::Image(e.to_string()))?
    } else {
        (0, 0, Vec::new())
    };

    Ok(MediaProbeResult {
        kind: MediaProbeKind::Video,
        width,
        height,
        duration_ms,
        codec,
        container,
        thumbnail_width,
        thumbnail_height,
        thumbnail_png,
    })
}

fn probe_audio(canonical: &Path) -> Result<MediaProbeResult> {
    init();
    let mut input = ffmpeg::format::input_with_dictionary(&ffmpeg_url(canonical), open_options())
        .map_err(MediaError::Open)?;
    let container = check_demuxer(&input)?;
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .ok_or(MediaError::NoAudio)?;
    let stream_index = stream.index();
    let duration_ms = duration_milliseconds(&input, &stream);
    let parameters = stream.parameters();
    let codec = ffmpeg::decoder::find(parameters.id())
        .ok_or_else(|| MediaError::DecoderNotAllowed(format!("{:?}", parameters.id())))?;
    let decoder_name = codec.name().to_owned();
    if !AUDIO_DECODERS.contains(&decoder_name.as_str()) {
        return Err(MediaError::DecoderNotAllowed(decoder_name));
    }
    let context =
        ffmpeg::codec::context::Context::from_parameters(parameters).map_err(MediaError::Open)?;
    let mut decoder = context.decoder().audio().map_err(MediaError::Open)?;

    // Decode up to one audio frame to confirm the stream payload is readable and valid.
    let mut decoded = ffmpeg::frame::Audio::empty();
    let mut eof_sent = false;
    loop {
        match decoder.receive_frame(&mut decoded) {
            Ok(()) => break,
            Err(ffmpeg::Error::Eof) => break,
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {}
            Err(error) => return Err(MediaError::Decode(error)),
        }
        if eof_sent {
            break;
        }
        let mut packet = ffmpeg::Packet::empty();
        match packet.read(&mut input) {
            Ok(()) if packet.stream() == stream_index => match decoder.send_packet(&packet) {
                Ok(()) | Err(ffmpeg::Error::InvalidData) => {}
                Err(error) => return Err(MediaError::Decode(error)),
            },
            Ok(()) | Err(ffmpeg::Error::InvalidData) => {}
            Err(ffmpeg::Error::Eof) => {
                decoder.send_eof().map_err(MediaError::Decode)?;
                eof_sent = true;
            }
            Err(error) => return Err(MediaError::Decode(error)),
        }
    }

    Ok(MediaProbeResult {
        kind: MediaProbeKind::Audio,
        width: 0,
        height: 0,
        duration_ms,
        codec: decoder_name,
        container,
        thumbnail_width: 0,
        thumbnail_height: 0,
        thumbnail_png: Vec::new(),
    })
}

/// Copy `rows` rows of `row_bytes` from a plane, dropping the stride padding.
fn copy_plane(
    frame: &ffmpeg::frame::Video,
    plane: usize,
    row_bytes: usize,
    rows: usize,
) -> Vec<u8> {
    let stride = frame.stride(plane);
    let data = frame.data(plane);
    let mut out = Vec::with_capacity(row_bytes * rows);
    for row in 0..rows {
        out.extend_from_slice(&data[row * stride..row * stride + row_bytes]);
    }
    out
}

/// Planar U and V (YUV 4:2:0) into NV12's interleaved UV plane.
fn interleave(frame: &ffmpeg::frame::Video, width: usize, rows: usize) -> Vec<u8> {
    let (u_stride, v_stride) = (frame.stride(1), frame.stride(2));
    let (u, v) = (frame.data(1), frame.data(2));
    let mut out = Vec::with_capacity(width * 2 * rows);
    for row in 0..rows {
        let u_row = &u[row * u_stride..row * u_stride + width];
        let v_row = &v[row * v_stride..row * v_stride + width];
        for (cb, cr) in u_row.iter().zip(v_row) {
            out.push(*cb);
            out.push(*cr);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_protocol_prefixes_are_refused_before_ffmpeg() {
        for refused in [
            "http://127.0.0.1:9/clip.mp4",
            "https://example.invalid/clip.mp4",
            "rtmp://127.0.0.1/live",
            "concat:a.mp4|b.mp4",
            "subfile,,start,0,end,0,,:clip.mp4",
            "tcp://127.0.0.1:9",
            "file://clip.mp4",
        ] {
            assert!(
                matches!(
                    check_local_path(Path::new(refused)),
                    Err(MediaError::NotLocal(_))
                ),
                "{refused} should be refused"
            );
        }
    }

    #[test]
    fn missing_files_and_folders_are_refused() {
        let folder = tempfile::tempdir().unwrap();
        assert!(matches!(
            check_local_path(folder.path()),
            Err(MediaError::NotAFile(_))
        ));
        assert!(matches!(
            check_local_path(&folder.path().join("missing.mp4")),
            Err(MediaError::NotAFile(_))
        ));
        let file = folder.path().join("clip.mp4");
        std::fs::write(&file, b"not really a video").unwrap();
        assert!(check_local_path(&file).is_ok());
    }

    #[test]
    fn opening_an_http_url_is_refused() {
        let error = VideoDecoder::open(
            Path::new("http://127.0.0.1:9/clip.mp4"),
            DecodeMode::Software,
        )
        .err()
        .expect("an http:// file item must not open");
        assert!(matches!(error, MediaError::NotLocal(_)), "{error}");
    }

    #[test]
    fn a_local_playlist_cant_reach_the_network() {
        // An HLS playlist is a local file, but its demuxer would fetch http:// segments. It isn't
        // on the demuxer allowlist, and the protocol whitelist would refuse the fetch anyway.
        let folder = tempfile::tempdir().unwrap();
        let playlist = folder.path().join("stream.m3u8");
        std::fs::write(
            &playlist,
            "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\nhttp://127.0.0.1:9/segment.ts\n#EXT-X-ENDLIST\n",
        )
        .unwrap();
        let error = VideoDecoder::open(&playlist, DecodeMode::Software)
            .err()
            .expect("a playlist must not open");
        assert!(matches!(error, MediaError::Open(_)), "{error}");
    }

    #[test]
    fn a_file_that_isnt_video_is_refused_cleanly() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("clip.mp4");
        std::fs::write(&file, vec![0x5a_u8; 4096]).unwrap();
        assert!(VideoDecoder::open(&file, DecodeMode::Software).is_err());
    }

    #[test]
    fn probes_committed_image_and_generates_png_thumbnail() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/reference/text-en.png");
        let probed = probe_and_thumbnail(&path, Some(MediaProbeKind::Image), 160, 90).unwrap();
        assert_eq!(probed.kind, MediaProbeKind::Image);
        assert!(probed.width > 0 && probed.height > 0);
        assert!(probed.thumbnail_width <= 160 && probed.thumbnail_height <= 90);
        assert!(!probed.thumbnail_png.is_empty());
        let decoded = selah_render::png::decode(&probed.thumbnail_png).unwrap();
        assert_eq!(
            (decoded.width, decoded.height),
            (probed.thumbnail_width, probed.thumbnail_height)
        );
    }

    #[test]
    fn probes_committed_video_and_generates_keyframe_thumbnail() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/media/committed/h264-1080p60.mp4");
        let probed = probe_and_thumbnail(&path, Some(MediaProbeKind::Video), 320, 180).unwrap();
        assert_eq!(probed.kind, MediaProbeKind::Video);
        assert_eq!((probed.width, probed.height), (1920, 1080));
        assert_eq!(
            (probed.thumbnail_width, probed.thumbnail_height),
            (320, 180)
        );
        assert!(probed.duration_ms > 0);
        let decoded = selah_render::png::decode(&probed.thumbnail_png).unwrap();
        assert_eq!((decoded.width, decoded.height), (320, 180));
    }

    #[test]
    fn probes_synthetic_wav_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        let sample_rate = 48_000_u32;
        let samples = sample_rate / 2; // 500 ms mono 16-bit PCM
        let data_bytes = samples * 2;
        let mut wav = Vec::with_capacity(44 + data_bytes as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1_u16.to_le_bytes()); // mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_bytes.to_le_bytes());
        wav.resize(44 + data_bytes as usize, 0);
        std::fs::write(&path, wav).unwrap();

        let probed = probe_and_thumbnail(&path, Some(MediaProbeKind::Audio), 320, 180).unwrap();
        assert_eq!(probed.kind, MediaProbeKind::Audio);
        assert_eq!(probed.codec, "pcm_s16le");
        assert!(probed.duration_ms >= 450 && probed.duration_ms <= 550);
        assert!(probed.thumbnail_png.is_empty());
    }

    #[test]
    fn rejects_oversized_png_bomb() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bomb.png");
        let mut png = selah_render::png::encode(&selah_render::Frame::new(1, 1)).unwrap();
        png[16..20].copy_from_slice(&9000u32.to_be_bytes());
        png[20..24].copy_from_slice(&9000u32.to_be_bytes());
        let mut crc = 0xFFFF_FFFF_u32;
        for &byte in &png[12..29] {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        png[29..33].copy_from_slice(&(!crc).to_be_bytes());
        std::fs::write(&path, png).unwrap();

        let err = probe_and_thumbnail(&path, Some(MediaProbeKind::Image), 320, 180)
            .expect_err("9000x9000 PNG must be refused");
        assert!(err.to_string().contains("8192"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_input_files() {
        let dir = tempfile::tempdir().unwrap();
        let target =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/reference/text-en.png");
        let link = dir.path().join("link.png");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            check_local_path(&link),
            Err(MediaError::NotAFile(_))
        ));
    }
}
