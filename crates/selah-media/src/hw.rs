//! Hardware decode (M1.7): D3D11VA on Windows, VideoToolbox on macOS. Linux (cloud sessions
//! and CI) has no video hardware and always decodes in software.
//!
//! This module is listed in `tools/unsafe-allowlist.txt`: ffmpeg-next has no safe API for
//! hardware device contexts, the `get_format` callback or frame download. Every `unsafe` block
//! calls FFmpeg with pointers owned by live ffmpeg-next objects, and every reference this module
//! takes on a buffer is released in `Drop`.
#![allow(unsafe_code)]

use ffmpeg::ffi::{AVBufferRef, AVCodecContext, AVHWDeviceType, AVPixelFormat};
use ffmpeg::format::Pixel;
use ffmpeg_next as ffmpeg;

#[cfg(windows)]
const PLATFORM: Option<(AVHWDeviceType, AVPixelFormat, &str)> = Some((
    AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
    AVPixelFormat::AV_PIX_FMT_D3D11,
    "d3d11va",
));
#[cfg(target_os = "macos")]
const PLATFORM: Option<(AVHWDeviceType, AVPixelFormat, &str)> = Some((
    AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
    AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX,
    "videotoolbox",
));
#[cfg(not(any(windows, target_os = "macos")))]
const PLATFORM: Option<(AVHWDeviceType, AVPixelFormat, &str)> = None;

/// A hardware device context attached to one decoder.
pub struct Device {
    buffer: *mut AVBufferRef,
    format: AVPixelFormat,
    name: &'static str,
}

// SAFETY: an AVBufferRef's reference count is atomic, and this struct only ever unrefs its own
// reference (in Drop); the device context itself is thread-safe for decoding.
unsafe impl Send for Device {}

/// Chooses the hardware format when the decoder offers it, otherwise FFmpeg's default
/// (software) choice, so a stream the hardware can't decode still plays.
// SAFETY: only FFmpeg calls this, as AVCodecContext.get_format, with a live context and a
// format list terminated by AV_PIX_FMT_NONE.
unsafe extern "C" fn get_format(
    context: *mut AVCodecContext,
    formats: *const AVPixelFormat,
) -> AVPixelFormat {
    if let Some((_, wanted, _)) = PLATFORM {
        let mut at = formats;
        // SAFETY: FFmpeg passes a list terminated by AV_PIX_FMT_NONE that lives for this call.
        unsafe {
            while *at != AVPixelFormat::AV_PIX_FMT_NONE {
                if *at == wanted {
                    return wanted;
                }
                at = at.add(1);
            }
        }
        tracing::info!("hardware decode isn't offered for this stream; decoding in software");
    }
    // SAFETY: the same arguments FFmpeg gave this callback.
    unsafe { ffmpeg::ffi::avcodec_default_get_format(context, formats) }
}

impl Device {
    /// Create the platform's hardware device and attach it to a decoder that hasn't been opened
    /// yet. `None` (software decode) when the platform or codec has no hardware path.
    pub fn attach(context: &mut ffmpeg::codec::context::Context) -> Option<Self> {
        let (kind, format, name) = PLATFORM?;
        // SAFETY: the context is allocated and not opened; its codec_id was set from the stream.
        let codec_id = unsafe { (*context.as_ptr()).codec_id };
        // SAFETY: plain lookup in FFmpeg's static codec tables.
        let codec = unsafe { ffmpeg::ffi::avcodec_find_decoder(codec_id) };
        if codec.is_null() || !supports(codec, kind) {
            return None;
        }
        let mut buffer: *mut AVBufferRef = std::ptr::null_mut();
        // SAFETY: `buffer` is a valid out-pointer; the device and options arguments may be null.
        let result = unsafe {
            ffmpeg::ffi::av_hwdevice_ctx_create(
                &mut buffer,
                kind,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            )
        };
        if result < 0 || buffer.is_null() {
            tracing::info!(
                device = name,
                error = %ffmpeg::Error::from(result),
                "no hardware decoder; decoding in software"
            );
            return None;
        }
        // SAFETY: the context isn't opened yet, so setting hw_device_ctx and get_format is
        // allowed; the context takes its own reference, released when the context is freed.
        unsafe {
            let raw = context.as_mut_ptr();
            (*raw).hw_device_ctx = ffmpeg::ffi::av_buffer_ref(buffer);
            (*raw).get_format = Some(get_format);
        }
        Some(Self {
            buffer,
            format,
            name,
        })
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The pixel format of frames that are still in GPU memory.
    pub fn pixel_format(&self) -> Pixel {
        Pixel::from(self.format)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: this releases the one reference `attach` created; the pointer is not used again.
        unsafe { ffmpeg::ffi::av_buffer_unref(&mut self.buffer) };
    }
}

/// Does `codec` have a hardware configuration for `kind` through a device context?
fn supports(codec: *const ffmpeg::ffi::AVCodec, kind: AVHWDeviceType) -> bool {
    for index in 0.. {
        // SAFETY: `codec` is a static FFmpeg codec; the function returns null past the last entry.
        let config = unsafe { ffmpeg::ffi::avcodec_get_hw_config(codec, index) };
        if config.is_null() {
            return false;
        }
        // SAFETY: a non-null config points into the codec's static table.
        let (methods, device_type) = unsafe { ((*config).methods, (*config).device_type) };
        let device_context = ffmpeg::ffi::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32;
        if methods & device_context != 0 && device_type == kind {
            return true;
        }
    }
    false
}

/// Copy a frame from GPU memory into system memory (NV12 for 8-bit video).
pub fn download(frame: &ffmpeg::frame::Video) -> Result<ffmpeg::frame::Video, ffmpeg::Error> {
    let mut software = ffmpeg::frame::Video::empty();
    // SAFETY: both frames are live ffmpeg-next frames; `software` is empty, so FFmpeg allocates
    // its buffers, which ffmpeg-next frees when the frame drops.
    let result =
        unsafe { ffmpeg::ffi::av_hwframe_transfer_data(software.as_mut_ptr(), frame.as_ptr(), 0) };
    if result < 0 {
        return Err(ffmpeg::Error::from(result));
    }
    // SAFETY: the same two live frames; this copies timestamps and colour properties.
    let result = unsafe { ffmpeg::ffi::av_frame_copy_props(software.as_mut_ptr(), frame.as_ptr()) };
    if result < 0 {
        return Err(ffmpeg::Error::from(result));
    }
    Ok(software)
}
