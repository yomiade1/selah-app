//! PNG encode/decode for golden frames and diagnostics (through Skia, so no extra codec).
use anyhow::{Context, Result, bail};
use skia_safe::{AlphaType, ColorType, Data, EncodedImageFormat, Image, ImageInfo, images};

use crate::Frame;

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

/// Decode a PNG into an RGBA frame. Refuses images over 8K × 8K (SEC-04 pixel limit).
pub fn decode(bytes: &[u8]) -> Result<Frame> {
    let image = Image::from_encoded(Data::new_copy(bytes)).context("not a PNG image")?;
    let (width, height) = (image.width() as u32, image.height() as u32);
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 8192 * 8192 {
        bail!("image size {width}×{height} is outside the permitted range");
    }
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
