//! PNG/image decode, probe and thumbnail rasterisation (through Skia, so no extra codec).
use anyhow::{Context, Result, bail, ensure};
use skia_safe::{
    AlphaType, Codec, Color, ColorType, Data, EncodedImageFormat, FilterMode, Image, ImageInfo,
    MipmapMode, Paint, Rect, SamplingOptions, images, surfaces,
};

use crate::Frame;

pub const MAX_IMAGE_DIMENSION: u32 = 8192;
pub const MAX_IMAGE_PIXELS: u64 = 8192 * 8192;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thumbnail {
    pub source_width: u32,
    pub source_height: u32,
    pub width: u32,
    pub height: u32,
    pub png_bytes: Vec<u8>,
}

fn check_dimensions(width: i32, height: i32) -> Result<(u32, u32)> {
    ensure!(
        width > 0 && height > 0,
        "image size {width}×{height} must be positive"
    );
    let (w, h) = (width as u32, height as u32);
    ensure!(
        w <= MAX_IMAGE_DIMENSION
            && h <= MAX_IMAGE_DIMENSION
            && u64::from(w) * u64::from(h) <= MAX_IMAGE_PIXELS,
        "image size {w}×{h} exceeds the {MAX_IMAGE_DIMENSION}×{MAX_IMAGE_DIMENSION} limit (SEC-04)"
    );
    Ok((w, h))
}

fn fit_dimensions(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let max_w = max_w.clamp(16, 1024);
    let max_h = max_h.clamp(16, 1024);
    if src_w <= max_w && src_h <= max_h {
        return (src_w.max(1), src_h.max(1));
    }
    let scale_w = f64::from(max_w) / f64::from(src_w);
    let scale_h = f64::from(max_h) / f64::from(src_h);
    let scale = scale_w.min(scale_h);
    let tw = ((f64::from(src_w) * scale).round() as u32).clamp(1, max_w);
    let th = ((f64::from(src_h) * scale).round() as u32).clamp(1, max_h);
    (tw, th)
}

/// Inspect image container dimensions without allocating full decoded pixel buffers (SEC-04).
pub fn probe_image_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    let codec = Codec::from_data(Data::new_copy(bytes)).context("not a supported image")?;
    let dims = codec.dimensions();
    check_dimensions(dims.width, dims.height)
}

/// Encode an opaque RGBA frame as PNG.
pub fn encode(frame: &Frame) -> Result<Vec<u8>> {
    let info = ImageInfo::new(
        (frame.width as i32, frame.height as i32),
        ColorType::RGBA8888,
        AlphaType::Unpremul,
        None,
    );
    let image = images::raster_from_data(
        &info,
        Data::new_copy(&frame.pixels),
        frame.width as usize * 4,
    )
    .context("frame image")?;
    let data = image
        .encode(None, EncodedImageFormat::PNG, None)
        .context("PNG encode")?;
    Ok(data.as_bytes().to_vec())
}

/// Decode a PNG into an RGBA frame. Refuses images over 8K × 8K before allocating pixels (SEC-04).
pub fn decode(bytes: &[u8]) -> Result<Frame> {
    let (width, height) = probe_image_dimensions(bytes)?;
    let image = Image::from_encoded(Data::new_copy(bytes)).context("not a PNG image")?;
    let (img_w, img_h) = check_dimensions(image.width(), image.height())?;
    ensure!(
        img_w == width && img_h == height,
        "decoded image dimensions changed unexpectedly"
    );
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Unpremul,
        None,
    );
    let mut frame = Frame::new(width, height);
    if !image.read_pixels(
        &info,
        &mut frame.pixels,
        width as usize * 4,
        (0, 0),
        skia_safe::image::CachingHint::Disallow,
    ) {
        bail!("couldn't read the image pixels");
    }
    Ok(frame)
}

/// Probe and downscale an encoded image (PNG/JPEG/BMP/GIF) to a bounded PNG thumbnail.
pub fn thumbnail_image(bytes: &[u8], max_width: u32, max_height: u32) -> Result<Thumbnail> {
    let (src_w, src_h) = probe_image_dimensions(bytes)?;
    let image = Image::from_encoded(Data::new_copy(bytes)).context("couldn't decode image")?;
    let (img_w, img_h) = check_dimensions(image.width(), image.height())?;
    let (tw, th) = fit_dimensions(img_w, img_h, max_width, max_height);
    let png_bytes = scale_image_to_png(&image, tw, th)?;
    Ok(Thumbnail {
        source_width: src_w,
        source_height: src_h,
        width: tw,
        height: th,
        png_bytes,
    })
}

/// Downscale a decoded RGBA [`Frame`] (e.g. a video keyframe) into a bounded PNG thumbnail.
pub fn encode_rgba_thumbnail(
    frame: &Frame,
    max_width: u32,
    max_height: u32,
) -> Result<(u32, u32, Vec<u8>)> {
    let (src_w, src_h) = check_dimensions(frame.width as i32, frame.height as i32)?;
    let info = ImageInfo::new(
        (src_w as i32, src_h as i32),
        ColorType::RGBA8888,
        AlphaType::Unpremul,
        None,
    );
    let image = images::raster_from_data(&info, Data::new_copy(&frame.pixels), src_w as usize * 4)
        .context("frame image")?;
    let (tw, th) = fit_dimensions(src_w, src_h, max_width, max_height);
    let png_bytes = scale_image_to_png(&image, tw, th)?;
    Ok((tw, th, png_bytes))
}

fn scale_image_to_png(image: &Image, width: u32, height: u32) -> Result<Vec<u8>> {
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let mut surface = surfaces::raster(&info, None, None).context("thumbnail surface")?;
    let canvas = surface.canvas();
    canvas.clear(Color::TRANSPARENT);
    let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear);
    let paint = Paint::default();
    canvas.draw_image_rect_with_sampling_options(
        image,
        None,
        Rect::from_wh(width as f32, height as f32),
        sampling,
        &paint,
    );
    let encoded = surface
        .image_snapshot()
        .encode(None, EncodedImageFormat::PNG, None)
        .context("thumbnail PNG encode")?;
    Ok(encoded.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ieee_crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFF_u32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    #[test]
    fn oversize_png_header_is_rejected_before_decoding_pixels() {
        // Take a real 1×1 PNG and rewrite IHDR dimensions to 9000×9000 with a valid CRC.
        // The small IDAT stays untouched: a decoder must refuse on header dimensions before pixels.
        let mut png = encode(&Frame::new(1, 1)).unwrap();
        assert_eq!(&png[12..16], b"IHDR");
        png[16..20].copy_from_slice(&9000u32.to_be_bytes());
        png[20..24].copy_from_slice(&9000u32.to_be_bytes());
        let crc = ieee_crc32(&png[12..29]);
        png[29..33].copy_from_slice(&crc.to_be_bytes());

        let err = probe_image_dimensions(&png).expect_err("9000x9000 PNG must be refused");
        assert!(err.to_string().contains("8192"), "{err}");
        assert!(thumbnail_image(&png, 320, 180).is_err());
    }

    #[test]
    fn thumbnail_preserves_aspect_ratio_and_decodes() {
        let mut frame = Frame::new(160, 80);
        for pixel in frame.pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[30, 120, 210, 255]);
        }
        let png = encode(&frame).unwrap();
        let thumb = thumbnail_image(&png, 80, 80).unwrap();
        assert_eq!((thumb.source_width, thumb.source_height), (160, 80));
        assert_eq!((thumb.width, thumb.height), (80, 40));
        let decoded = decode(&thumb.png_bytes).unwrap();
        assert_eq!((decoded.width, decoded.height), (80, 40));
    }
}
