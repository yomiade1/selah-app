#![forbid(unsafe_code)]
//! Out-of-process media probing and PNG thumbnail worker (M2.6, ADR-020, SEC-04).
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Serialize)]
struct WorkerReport {
    kind: selah_media::MediaProbeKind,
    width: u32,
    height: u32,
    duration_ms: u64,
    codec: String,
    container: String,
    thumbnail_width: u32,
    thumbnail_height: u32,
    thumbnail_written: bool,
    flash_warning: bool,
}

fn parse_kind(value: &str) -> Result<selah_media::MediaProbeKind, String> {
    match value {
        "image" => Ok(selah_media::MediaProbeKind::Image),
        "video" => Ok(selah_media::MediaProbeKind::Video),
        "audio" => Ok(selah_media::MediaProbeKind::Audio),
        other => Err(format!("unknown media kind '{other}'")),
    }
}

fn write_thumbnail(path: &Path, png_bytes: &[u8]) -> Result<(), String> {
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && (meta.file_type().is_symlink() || !meta.is_file())
    {
        return Err("thumbnail output path must not be a symlink".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| "thumbnail output path has no parent directory".to_owned())?;
    let parent_meta = std::fs::symlink_metadata(parent)
        .map_err(|e| format!("thumbnail output directory error: {e}"))?;
    if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
        return Err("thumbnail output parent must be a real directory".into());
    }
    let mut file =
        std::fs::File::create(path).map_err(|e| format!("failed to create thumbnail: {e}"))?;
    file.write_all(png_bytes)
        .map_err(|e| format!("failed to write thumbnail: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("failed to sync thumbnail: {e}"))?;
    Ok(())
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let subcommand = args.next().ok_or_else(|| "missing subcommand".to_owned())?;
    if subcommand != "probe" {
        return Err(format!("unsupported subcommand '{subcommand}'"));
    }

    let mut input: Option<PathBuf> = None;
    let mut kind_hint: Option<selah_media::MediaProbeKind> = None;
    let mut max_width: u32 = 320;
    let mut max_height: u32 = 180;
    let mut thumb_out: Option<PathBuf> = None;
    let mut scan_flash = true;

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--input" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --input".to_owned())?;
                input = Some(PathBuf::from(value));
            }
            "--kind" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --kind".to_owned())?;
                kind_hint = Some(parse_kind(&value)?);
            }
            "--max-width" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --max-width".to_owned())?;
                max_width = value
                    .parse::<u32>()
                    .map_err(|_| "invalid --max-width".to_owned())?;
            }
            "--max-height" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --max-height".to_owned())?;
                max_height = value
                    .parse::<u32>()
                    .map_err(|_| "invalid --max-height".to_owned())?;
            }
            "--thumb-out" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --thumb-out".to_owned())?;
                thumb_out = Some(PathBuf::from(value));
            }
            "--no-flash-scan" => {
                scan_flash = false;
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }

    let input_path = input.ok_or_else(|| "missing required --input".to_owned())?;
    let (thumb_w, thumb_h) = if thumb_out.is_some() {
        (max_width.clamp(16, 1024), max_height.clamp(16, 1024))
    } else {
        (0, 0)
    };

    let result = selah_media::probe_and_thumbnail_with_options(
        &input_path,
        kind_hint,
        thumb_w,
        thumb_h,
        scan_flash,
    )
    .map_err(|e| e.to_string())?;

    let mut thumbnail_written = false;
    if let Some(out_path) = thumb_out
        && !result.thumbnail_png.is_empty()
    {
        write_thumbnail(&out_path, &result.thumbnail_png)?;
        thumbnail_written = true;
    }

    let report = WorkerReport {
        kind: result.kind,
        width: result.width,
        height: result.height,
        duration_ms: result.duration_ms,
        codec: result.codec,
        container: result.container,
        thumbnail_width: result.thumbnail_width,
        thumbnail_height: result.thumbnail_height,
        thumbnail_written,
        flash_warning: result.flash_warning,
    };
    let json = serde_json::to_string(&report).map_err(|e| e.to_string())?;
    println!("{json}");
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
