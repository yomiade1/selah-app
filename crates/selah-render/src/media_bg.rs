//! Content-addressed background media loading and fit/blur rasterisation (M3.4, SEC-04, SEC-15).
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use selah_proto::v1::MediaFitMode;
use skia_safe::{
    AlphaType, Color, ColorType, Data, FilterMode, Image, ImageInfo, MipmapMode, Paint, Rect,
    SamplingOptions, TileMode, image_filters, surfaces,
};

use crate::Frame;
use crate::png::{MAX_IMAGE_DIMENSION, MAX_IMAGE_PIXELS, probe_image_dimensions};

/// Maximum encoded background image size read from the local media store (128 MiB).
pub const MAX_IMAGE_FILE_BYTES: u64 = 128 * 1024 * 1024;

/// Maximum isotropic Gaussian blur sigma applied to background layers.
pub const MAX_BLUR_RADIUS: f32 = 64.0;

/// Resolve the default media folder from environment variables when a caller does not explicitly
/// call `Compositor::set_media_root`.
pub fn default_media_root_from_env() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SELAH_LIBRARY_DIR")
        && !dir.is_empty()
    {
        return Some(PathBuf::from(dir).join("media"));
    }
    if let Some(home) = std::env::var_os("SELAH_HOME")
        && !home.is_empty()
    {
        return Some(PathBuf::from(home).join("library").join("media"));
    }
    None
}

/// Validate a content-addressed media identifier `(hash, extension)` before touching disk.
pub fn validate_media_address(hash: &str, extension: &str) -> Result<()> {
    ensure!(
        hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid media hash"
    );
    ensure!(
        !extension.is_empty()
            && extension.len() <= 10
            && extension
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
        "invalid media extension"
    );
    Ok(())
}

/// Resolve `<media_root>/<hash[0..2]>/<hash>.<ext>`, refusing symlinks, directory escapes, and
/// malformed identifiers (SEC-04, SEC-15).
pub fn resolve_verified_media_path(root: &Path, hash: &str, extension: &str) -> Result<PathBuf> {
    validate_media_address(hash, extension)?;
    let canon_root = root
        .canonicalize()
        .with_context(|| format!("media root unavailable: {}", root.display()))?;
    let candidate = canon_root
        .join(&hash[..2])
        .join(format!("{hash}.{extension}"));
    let meta = std::fs::symlink_metadata(&candidate)
        .with_context(|| format!("media file missing: {}", candidate.display()))?;
    ensure!(
        !meta.file_type().is_symlink() && meta.is_file(),
        "media path must be a regular non-symlink file (SEC-04)"
    );
    let canonical = candidate.canonicalize()?;
    ensure!(
        canonical.starts_with(&canon_root),
        "media path escaped its store"
    );
    Ok(canonical)
}

/// Compute the destination rectangle `(dst_x, dst_y, dst_w, dst_h)` in output pixels for a
/// source aspect ratio under `Fit`, `Fill` (or `Unspecified`), or `Stretch`.
pub fn compute_fit_rect(
    fit: MediaFitMode,
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
) -> (f32, f32, f32, f32) {
    let sw = src_w.max(1) as f32;
    let sh = src_h.max(1) as f32;
    let ow = out_w.max(1) as f32;
    let oh = out_h.max(1) as f32;
    match fit {
        MediaFitMode::Fit => {
            let scale = (ow / sw).min(oh / sh);
            let dw = sw * scale;
            let dh = sh * scale;
            ((ow - dw) * 0.5, (oh - dh) * 0.5, dw, dh)
        }
        MediaFitMode::Stretch => (0.0, 0.0, ow, oh),
        MediaFitMode::Fill | MediaFitMode::Unspecified => {
            let scale = (ow / sw).max(oh / sh);
            let dw = sw * scale;
            let dh = sh * scale;
            ((ow - dw) * 0.5, (oh - dh) * 0.5, dw, dh)
        }
    }
}

/// Read, BLAKE3-verify, and Skia-decode a bounded local background image from `<root>` (SEC-04).
pub fn load_verified_image(root: &Path, hash: &str, extension: &str) -> Result<Image> {
    let path = resolve_verified_media_path(root, hash, extension)?;
    let file = std::fs::File::open(&path)?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && meta.len() <= MAX_IMAGE_FILE_BYTES,
        "image exceeds {MAX_IMAGE_FILE_BYTES} bytes (SEC-04)"
    );
    let mut bytes = Vec::with_capacity((meta.len() as usize).min(8 * 1024 * 1024));
    file.take(MAX_IMAGE_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_IMAGE_FILE_BYTES,
        "image exceeds {MAX_IMAGE_FILE_BYTES} bytes (SEC-04)"
    );
    ensure!(
        blake3::hash(&bytes).to_hex().as_str() == hash,
        "background image BLAKE3 mismatch (SEC-15)"
    );
    let (probed_w, probed_h) = probe_image_dimensions(&bytes)?;
    let image = Image::from_encoded(Data::new_copy(&bytes)).context("couldn't decode image")?;
    let (img_w, img_h) = (image.width().max(0) as u32, image.height().max(0) as u32);
    ensure!(
        img_w == probed_w
            && img_h == probed_h
            && img_w <= MAX_IMAGE_DIMENSION
            && img_h <= MAX_IMAGE_DIMENSION
            && u64::from(img_w) * u64::from(img_h) <= MAX_IMAGE_PIXELS,
        "decoded image dimensions invalid or exceed {MAX_IMAGE_DIMENSION} limit (SEC-04)"
    );
    Ok(image)
}

/// Rasterise a decoded image onto an `out_w × out_h` premultiplied RGBA [`Frame`] using the
/// requested fit mode and Gaussian blur sigma.
pub fn render_image_frame(
    image: &Image,
    fit: MediaFitMode,
    blur_radius: f32,
    out_w: u32,
    out_h: u32,
) -> Result<Frame> {
    let info = ImageInfo::new(
        (out_w as i32, out_h as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let mut surface = surfaces::raster(&info, None, None).context("image background surface")?;
    let (dx, dy, dw, dh) = compute_fit_rect(
        fit,
        image.width().max(1) as u32,
        image.height().max(1) as u32,
        out_w,
        out_h,
    );
    let dst = Rect::from_xywh(dx, dy, dw, dh);
    let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear);
    {
        let canvas = surface.canvas();
        canvas.clear(Color::TRANSPARENT);
        let paint = Paint::default();
        canvas.draw_image_rect_with_sampling_options(image, None, dst, sampling, &paint);
    }

    let sigma = if blur_radius.is_finite() {
        blur_radius.clamp(0.0, MAX_BLUR_RADIUS)
    } else {
        0.0
    };
    if sigma > 0.05 {
        let snapshot = surface.image_snapshot();
        let mut blurred = surfaces::raster(&info, None, None).context("blurred surface")?;
        let mut paint = Paint::default();
        let filter = image_filters::blur((sigma, sigma), TileMode::Clamp, None, None);
        paint.set_image_filter(filter);
        let canvas = blurred.canvas();
        canvas.clear(Color::TRANSPARENT);
        canvas.draw_image(&snapshot, (0, 0), Some(&paint));
        return read_surface_frame(&mut blurred, out_w, out_h);
    }

    read_surface_frame(&mut surface, out_w, out_h)
}

/// Apply Skia Gaussian blur to an existing premultiplied RGBA [`Frame`].
pub fn blur_frame_skia(frame: &Frame, blur_radius: f32) -> Result<Frame> {
    let sigma = if blur_radius.is_finite() {
        blur_radius.clamp(0.0, MAX_BLUR_RADIUS)
    } else {
        0.0
    };
    if sigma <= 0.05 {
        return Ok(frame.clone());
    }
    let info = ImageInfo::new(
        (frame.width as i32, frame.height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let Some(image) = skia_safe::images::raster_from_data(
        &info,
        Data::new_copy(&frame.pixels),
        frame.width as usize * 4,
    ) else {
        bail!("failed to wrap frame pixels in Skia image");
    };
    let mut surface = surfaces::raster(&info, None, None).context("blur target surface")?;
    let mut paint = Paint::default();
    paint.set_image_filter(image_filters::blur(
        (sigma, sigma),
        TileMode::Clamp,
        None,
        None,
    ));
    let canvas = surface.canvas();
    canvas.clear(Color::TRANSPARENT);
    canvas.draw_image(&image, (0, 0), Some(&paint));
    read_surface_frame(&mut surface, frame.width, frame.height)
}

fn read_surface_frame(surface: &mut skia_safe::Surface, width: u32, height: u32) -> Result<Frame> {
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let mut frame = Frame::new(width, height);
    if !surface.read_pixels(&info, &mut frame.pixels, width as usize * 4, (0, 0)) {
        bail!("couldn't read background image pixels");
    }
    Ok(frame)
}
