#![deny(unsafe_code)]
//! Video decoding through FFmpeg (M1.7, ADR-005), limited by SEC-04:
//!
//! - **Local files only.** [`check_local_path`] refuses URLs and other protocol prefixes before
//!   FFmpeg sees the path, and FFmpeg opens it with `protocol_whitelist=file,pipe`, so a file
//!   can't pull in a network resource either (an HLS playlist pointing at `http://` fails).
//! - **Allowlisted demuxers and decoders.** [`DEMUXERS`] goes to FFmpeg as `format_whitelist`;
//!   [`VIDEO_DECODERS`] is checked before a decoder opens and is FFmpeg's `codec_whitelist`.
//!
//! Frames come out as 8-bit NV12 ([`VideoFrame`]): the compositor uploads the two planes and
//! converts them to RGB in a shader. Hardware decode (D3D11VA on Windows, VideoToolbox on
//! macOS) is tried first when asked for, and falls back to software decode.
use std::path::{Path, PathBuf};
use std::sync::Once;

use ffmpeg::format::Pixel;
use ffmpeg::util::color;
use ffmpeg_next as ffmpeg;

mod hw;

/// FFmpeg demuxers a file item may use. Names as FFmpeg reports them.
pub const DEMUXERS: &[&str] = &["mov", "mp4", "m4a", "3gp", "3g2", "mj2", "matroska", "webm"];

/// Video decoders a file item may use.
pub const VIDEO_DECODERS: &[&str] = &["h264", "hevc", "vp9", "av1", "prores"];

/// Audio decoders a file item may use (audio playback arrives in M3.4 and M5.7; probing a
/// video file opens its audio stream's decoder too).
pub const AUDIO_DECODERS: &[&str] = &[
    "aac",
    "mp3float",
    "mp3",
    "opus",
    "flac",
    "pcm_s16le",
    "pcm_s24le",
    "pcm_f32le",
];

/// Protocols FFmpeg may use for a file item (SEC-04). Network protocols are for stream items,
/// which arrive in M5 and go through `selah-net`.
pub const FILE_PROTOCOLS: &str = "file,pipe";

/// The largest frame the decoder accepts (8K), so a crafted header can't ask for huge buffers.
pub const MAX_DIMENSION: u32 = 8192;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("only local files can be opened as file items (SEC-04): {0}")]
    NotLocal(String),
    #[error("the file doesn't exist or isn't a regular file: {0}")]
    NotAFile(PathBuf),
    #[error("FFmpeg refused the file: {0}")]
    Open(ffmpeg::Error),
    #[error("the file has no video stream")]
    NoVideo,
    #[error("the {0} decoder isn't allowed (SEC-04)")]
    DecoderNotAllowed(String),
    #[error("the video is {0}×{1}, larger than the {MAX_DIMENSION} pixel limit")]
    TooLarge(u32, u32),
    #[error("unsupported pixel format {0:?} (M1 handles 8-bit 4:2:0 video)")]
    PixelFormat(Pixel),
    #[error("decoding failed: {0}")]
    Decode(ffmpeg::Error),
}

pub type Result<T> = std::result::Result<T, MediaError>;

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
    #[cfg(feature = "render")]
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

fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if let Err(error) = ffmpeg::init() {
            tracing::error!(%error, "FFmpeg didn't initialise");
        }
        // FFmpeg's own log goes to stderr; keep it to errors.
        ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Error);
    });
}

/// SEC-04: accept only a path to an existing local regular file. Anything that looks like a
/// URL or an FFmpeg protocol prefix (`http:`, `concat:`, `subfile,`) is refused, whatever the
/// file system says. A single drive letter (`C:\…`) is a path, not a protocol.
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
    let canonical =
        std::fs::canonicalize(path).map_err(|_| MediaError::NotAFile(path.to_owned()))?;
    if !canonical.is_file() {
        return Err(MediaError::NotAFile(path.to_owned()));
    }
    Ok(canonical)
}

/// FFmpeg's name for a local path: `file:` forces the file protocol, so nothing in the name
/// can select another one. `canonicalize` on Windows returns `\\?\C:\…`; FFmpeg wants `C:\…`.
fn ffmpeg_url(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    format!("file:{text}")
}

fn open_options() -> ffmpeg::Dictionary<'static> {
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

/// A video file opened for decoding.
pub struct VideoDecoder {
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Video,
    stream_index: usize,
    time_base: f64,
    start: Option<i64>,
    frame_rate: f64,
    hardware: Option<hw::Device>,
    eof_sent: bool,
    next_index: u64,
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
        let demuxer_name = input.format().name().to_owned();
        if !demuxer_name.split(',').any(|name| DEMUXERS.contains(&name)) {
            return Err(MediaError::Open(ffmpeg::Error::DemuxerNotFound));
        }
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(MediaError::NoVideo)?;
        let stream_index = stream.index();
        let time_base = f64::from(stream.time_base());
        let start = Some(stream.start_time()).filter(|t| *t != ffmpeg::ffi::AV_NOPTS_VALUE);
        let frame_rate = f64::from(stream.avg_frame_rate());
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
        if decoder.width() > MAX_DIMENSION || decoder.height() > MAX_DIMENSION {
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
            hardware,
            eof_sent: false,
            next_index: 0,
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

    /// The next frame in presentation order, or `None` at the end of the stream.
    pub fn next_frame(&mut self) -> Result<Option<VideoFrame>> {
        let mut decoded = ffmpeg::frame::Video::empty();
        loop {
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => return self.convert(decoded).map(Some),
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
}
