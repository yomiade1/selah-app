//! Media import probe/thumbnail worker host, canonical `.selah` bundles, and backup/restore
//! (M2.6, SEC-03, SEC-04, SEC-15, M2-A6, M2-A11).
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, ensure};
use prost::Message;
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use selah_proto::v1::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::archive::{Compression, DeterministicZipWriter, ZipLimits, read_zip_bytes, visit_zip};
use super::media::MAX_MEDIA_BYTES;
use super::ops::{
    NewOp, check_idempotency_in_tx, check_idempotency_readonly, encode_op_state, insert_op_in_tx,
    record_idempotency_in_tx, resolve_undo_group,
};
use super::repository::read_snapshot_in_tx;
use super::{
    CURRENT_SCHEMA_VERSION, CommandBusError, CommandMeta, Document, DocumentKind, ExecutionOutcome,
    Library, MAX_DOCUMENT_BYTES, MediaStore, MutationEventInfo, check_integrity, new_id,
    open_bible_db, open_library_connection, required_id, schema, search, unix_millis, validate_id,
};

const MAX_DIMENSION: u32 = 8192;
const MAX_PIXELS: u64 = 67_108_864;
const MAX_IMAGE_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_BUNDLE_ENTITIES: usize = 4096;
const MAX_THUMBNAIL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_WORKER_STDIO_BYTES: u64 = 256 * 1024;
const WORKER_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_THUMB_W: u32 = 320;
const DEFAULT_THUMB_H: u32 = 180;
const BUNDLED_MARKER_FILE: &str = ".bundled-bibles.json";
const BACKUP_BUNDLED_MARKER_ENTRY: &str = "meta/bundled-bibles.json";

#[derive(Deserialize)]
struct WorkerReport {
    kind: String,
    width: u32,
    height: u32,
    duration_ms: u64,
    #[allow(dead_code)]
    codec: String,
    #[allow(dead_code)]
    container: String,
    thumbnail_width: u32,
    thumbnail_height: u32,
    thumbnail_written: bool,
    #[serde(default)]
    flash_warning: bool,
}

fn check_no_url_or_protocol(raw: &str) -> Result<(), CommandBusError> {
    let text = raw.trim();
    if text.is_empty() || text.contains('\0') {
        return Err(CommandBusError::Invalid(
            "file path must not be empty or contain NUL bytes".into(),
        ));
    }
    if text.contains("://") {
        return Err(CommandBusError::Invalid(
            "only local file paths are allowed (SEC-04)".into(),
        ));
    }
    if let Some(colon) = text.find(':') {
        let prefix = &text[..colon];
        let drive_letter =
            cfg!(windows) && colon == 1 && prefix.bytes().all(|b| b.is_ascii_alphabetic());
        let looks_like_scheme = !prefix.is_empty()
            && prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-,".contains(c));
        if looks_like_scheme && !drive_letter {
            return Err(CommandBusError::Invalid(
                "only local file paths are allowed (SEC-04)".into(),
            ));
        }
        let after_drive = if drive_letter { &text[2..] } else { text };
        if after_drive.contains(':') {
            return Err(CommandBusError::Invalid(
                "file path contains forbidden colon / stream suffix".into(),
            ));
        }
    }
    Ok(())
}

fn check_local_input_path(raw: &str) -> Result<PathBuf, CommandBusError> {
    check_no_url_or_protocol(raw)?;
    let path = Path::new(raw.trim());
    let meta = std::fs::symlink_metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CommandBusError::NotFound(format!("file not found: {}", path.display()))
        } else {
            CommandBusError::Invalid(format!("cannot inspect path '{}': {e}", path.display()))
        }
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(CommandBusError::Invalid(
            "path must refer to a regular non-symlink file".into(),
        ));
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| CommandBusError::Invalid(format!("cannot resolve path: {e}")))?;
    let canon_meta = std::fs::metadata(&canonical)
        .map_err(|e| CommandBusError::Invalid(format!("cannot read canonical file: {e}")))?;
    if !canon_meta.is_file() {
        return Err(CommandBusError::Invalid(
            "path must resolve to a regular file".into(),
        ));
    }
    Ok(canonical)
}

fn validate_output_file_path(raw: &str) -> Result<PathBuf, CommandBusError> {
    check_no_url_or_protocol(raw)?;
    let trimmed = raw.trim();
    let candidate = Path::new(trimmed);
    let file_name = candidate
        .file_name()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| CommandBusError::Invalid("output path missing file name".into()))?;
    if let Ok(meta) = std::fs::symlink_metadata(candidate)
        && (meta.file_type().is_symlink() || meta.is_dir())
    {
        return Err(CommandBusError::Invalid(
            "output target must not be a symlink or directory".into(),
        ));
    }
    let parent = candidate
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_meta = std::fs::symlink_metadata(parent)
        .map_err(|e| CommandBusError::Invalid(format!("output directory does not exist: {e}")))?;
    if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
        return Err(CommandBusError::Invalid(
            "output parent must be a real non-symlink directory".into(),
        ));
    }
    let canon_parent = parent
        .canonicalize()
        .map_err(|e| CommandBusError::Invalid(format!("cannot resolve output directory: {e}")))?;
    Ok(canon_parent.join(file_name))
}

fn write_atomic_file_checked(target: &Path, bytes: &[u8]) -> Result<(), CommandBusError> {
    let parent = target
        .parent()
        .ok_or_else(|| CommandBusError::Invalid("target has no parent".into()))?;
    let mut temp = tempfile::Builder::new()
        .prefix(".selah-out-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|e| CommandBusError::Internal(e.into()))?;
    temp.write_all(bytes)
        .map_err(|e| CommandBusError::Internal(e.into()))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| CommandBusError::Internal(e.into()))?;
    temp.persist(target)
        .map_err(|e| CommandBusError::Internal(e.error.into()))?;
    Ok(())
}

fn sqlite_path_str(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        raw.into_owned()
    }
}

fn check_dimensions(width: u32, height: u32) -> anyhow::Result<(u32, u32)> {
    ensure!(
        width > 0
            && height > 0
            && width <= MAX_DIMENSION
            && height <= MAX_DIMENSION
            && u64::from(width) * u64::from(height) <= MAX_PIXELS,
        "image dimensions {width}x{height} exceed {MAX_DIMENSION}px limit (SEC-04)"
    );
    Ok((width, height))
}

fn parse_png_ihdr(bytes: &[u8]) -> anyhow::Result<Option<(u32, u32)>> {
    const SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(SIG) {
        return Ok(None);
    }
    ensure!(
        bytes.len() >= 24 && &bytes[12..16] == b"IHDR",
        "corrupt PNG IHDR"
    );
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Ok(Some(check_dimensions(w, h)?))
}

fn parse_bmp_dimensions(bytes: &[u8]) -> anyhow::Result<Option<(u32, u32)>> {
    if !bytes.starts_with(b"BM") {
        return Ok(None);
    }
    ensure!(bytes.len() >= 26, "truncated BMP header");
    let w = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
    let h = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
    ensure!(w > 0 && h != 0, "invalid BMP dimensions");
    Ok(Some(check_dimensions(w as u32, h.unsigned_abs())?))
}

fn parse_gif_dimensions(bytes: &[u8]) -> anyhow::Result<Option<(u32, u32)>> {
    if !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return Ok(None);
    }
    ensure!(bytes.len() >= 10, "truncated GIF header");
    let w = u32::from(u16::from_le_bytes([bytes[6], bytes[7]]));
    let h = u32::from(u16::from_le_bytes([bytes[8], bytes[9]]));
    Ok(Some(check_dimensions(w, h)?))
}

fn parse_jpeg_dimensions(bytes: &[u8]) -> anyhow::Result<Option<(u32, u32)>> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Ok(None);
    }
    let mut i = 2usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i] != 0xFF {
            i += 1;
        }
        while i < bytes.len() && bytes[i] == 0xFF {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let marker = bytes[i];
        i += 1;
        // Standalone markers without length
        if marker == 0x00 || marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            if marker == 0xD9 {
                break;
            }
            continue;
        }
        ensure!(i + 2 <= bytes.len(), "truncated JPEG segment");
        let seg_len = usize::from(u16::from_be_bytes([bytes[i], bytes[i + 1]]));
        ensure!(seg_len >= 2, "invalid JPEG marker length");
        if matches!(
            marker,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF
        ) {
            ensure!(
                seg_len >= 7 && i + 7 <= bytes.len(),
                "truncated JPEG SOF segment"
            );
            let h = u32::from(u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]));
            let w = u32::from(u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]));
            return Ok(Some(check_dimensions(w, h)?));
        }
        if marker == 0xDA {
            // Reached Start of Scan without SOF
            break;
        }
        i = i.saturating_add(seg_len);
    }
    Ok(None)
}

fn precheck_image_header(path: &Path, ext: &str) -> Result<(), CommandBusError> {
    let file = std::fs::File::open(path)
        .map_err(|e| CommandBusError::Invalid(format!("cannot open media file: {e}")))?;
    let meta = file
        .metadata()
        .map_err(|e| CommandBusError::Invalid(format!("cannot stat media file: {e}")))?;
    let is_image_ext = matches!(ext, "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" | "ico");
    if is_image_ext && meta.len() > MAX_IMAGE_FILE_BYTES {
        return Err(CommandBusError::Invalid(format!(
            "image exceeds {MAX_IMAGE_FILE_BYTES} byte limit (SEC-04)"
        )));
    }
    let mut header = Vec::new();
    file.take(64 * 1024)
        .read_to_end(&mut header)
        .map_err(|e| CommandBusError::Invalid(format!("failed reading media header: {e}")))?;

    for parser in [
        parse_png_ihdr,
        parse_bmp_dimensions,
        parse_gif_dimensions,
        parse_jpeg_dimensions,
    ] {
        parser(&header).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
    }
    if is_image_ext && header.len() > MAX_IMAGE_FILE_BYTES as usize {
        return Err(CommandBusError::Invalid("image too large".into()));
    }
    Ok(())
}

fn find_media_worker() -> Result<PathBuf, CommandBusError> {
    let binary_name = if cfg!(windows) {
        "selah-media-worker.exe"
    } else {
        "selah-media-worker"
    };
    if let Some(explicit) = std::env::var_os("SELAH_MEDIA_WORKER")
        && !explicit.is_empty()
    {
        let p = PathBuf::from(explicit);
        if p.is_file() {
            return Ok(p);
        }
        return Err(CommandBusError::Internal(anyhow::anyhow!(
            "SELAH_MEDIA_WORKER does not point to an existing executable: {}",
            p.display()
        )));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sibling = dir.join(binary_name);
        if sibling.is_file() {
            return Ok(sibling);
        }
        if dir.file_name().and_then(|n| n.to_str()) == Some("deps")
            && let Some(parent) = dir.parent()
        {
            let target_bin = parent.join(binary_name);
            if target_bin.is_file() {
                return Ok(target_bin);
            }
        }
        // Fallback when running under nested cargo target directories in dev/tests.
        for ancestor in dir.ancestors().take(4) {
            for profile in ["debug", "release"] {
                let candidate = ancestor.join("target").join(profile).join(binary_name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    Err(CommandBusError::Internal(anyhow::anyhow!(
        "could not locate {binary_name}"
    )))
}

fn read_bounded_pipe(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut pipe)
            .take(MAX_WORKER_STDIO_BYTES + 1)
            .read_to_end(&mut buf);
        buf
    })
}

fn run_media_worker(
    input_path: &Path,
    kind_hint: Option<&str>,
    max_width: u32,
    max_height: u32,
    thumb_out: Option<&Path>,
    scan_flash: bool,
) -> Result<WorkerReport, CommandBusError> {
    let worker = find_media_worker()?;
    let mut cmd = Command::new(&worker);
    cmd.arg("probe").arg("--input").arg(input_path);
    if let Some(kind) = kind_hint {
        cmd.arg("--kind").arg(kind);
    }
    cmd.arg("--max-width")
        .arg(max_width.to_string())
        .arg("--max-height")
        .arg(max_height.to_string());
    if let Some(out) = thumb_out {
        cmd.arg("--thumb-out").arg(out);
    }
    if !scan_flash {
        cmd.arg("--no-flash-scan");
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| CommandBusError::Internal(anyhow::anyhow!("failed spawning worker: {e}")))?;
    let stdout_handle = child.stdout.take().map(read_bounded_pipe);
    let stderr_handle = child.stderr.take().map(read_bounded_pipe);

    let deadline = Instant::now() + WORKER_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CommandBusError::Invalid(
                        "media probe worker timed out (SEC-03)".into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(CommandBusError::Internal(e.into()));
            }
        }
    };

    let stdout_bytes = stdout_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stderr_bytes = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    if !status.success() {
        let err_msg = String::from_utf8_lossy(&stderr_bytes).trim().to_owned();
        let detail = if err_msg.is_empty() {
            format!("media probe worker exited with {status}")
        } else {
            err_msg
        };
        return Err(CommandBusError::Invalid(detail));
    }

    let report: WorkerReport = serde_json::from_slice(&stdout_bytes)
        .map_err(|e| CommandBusError::Internal(anyhow::anyhow!("invalid worker JSON: {e}")))?;

    match report.kind.as_str() {
        "image" | "video" => {
            check_dimensions(report.width, report.height)
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        }
        "audio" => {}
        other => {
            return Err(CommandBusError::Internal(anyhow::anyhow!(
                "worker returned unknown media kind '{other}'"
            )));
        }
    }
    if report.thumbnail_written
        && (report.thumbnail_width == 0
            || report.thumbnail_height == 0
            || report.thumbnail_width > max_width
            || report.thumbnail_height > max_height)
    {
        return Err(CommandBusError::Internal(anyhow::anyhow!(
            "worker returned invalid thumbnail dimensions {}x{}",
            report.thumbnail_width,
            report.thumbnail_height
        )));
    }
    Ok(report)
}

fn ensure_thumbnails_dir(root: &Path) -> Result<PathBuf, CommandBusError> {
    let dir = root.join("thumbnails");
    match std::fs::symlink_metadata(&dir) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(CommandBusError::Invalid(
                    "thumbnails directory must not be a symlink".into(),
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(&dir).map_err(|e| CommandBusError::Internal(e.into()))?;
        }
        Err(e) => return Err(CommandBusError::Internal(e.into())),
    }
    let canon = dir
        .canonicalize()
        .map_err(|e| CommandBusError::Internal(e.into()))?;
    if !canon.starts_with(root) {
        return Err(CommandBusError::Invalid(
            "thumbnails directory escaped library root".into(),
        ));
    }
    Ok(canon)
}

fn read_validated_png(
    path: &Path,
    max_w: u32,
    max_h: u32,
) -> Result<(u32, u32, Vec<u8>), CommandBusError> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| CommandBusError::Invalid(format!("cannot stat thumbnail: {e}")))?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_THUMBNAIL_BYTES {
        return Err(CommandBusError::Invalid(
            "invalid thumbnail cache entry".into(),
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| CommandBusError::Invalid(format!("cannot open thumbnail: {e}")))?
        .take(MAX_THUMBNAIL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| CommandBusError::Invalid(format!("cannot read thumbnail: {e}")))?;
    let Some((w, h)) =
        parse_png_ihdr(&bytes).map_err(|e| CommandBusError::Invalid(e.to_string()))?
    else {
        return Err(CommandBusError::Invalid("thumbnail is not a PNG".into()));
    };
    if w > max_w || h > max_h {
        return Err(CommandBusError::Invalid(
            "cached thumbnail exceeds requested dimensions".into(),
        ));
    }
    Ok((w, h, bytes))
}

fn validate_label(value: &str, context: &str) -> Result<(), CommandBusError> {
    if value.trim().is_empty() || value.chars().count() > 512 || value.contains('\0') {
        return Err(CommandBusError::Invalid(format!(
            "{context} must have 1 to 512 characters without NUL bytes"
        )));
    }
    Ok(())
}

// ================================================================================================
// Canonical .selah Bundle Manifest (formats/selah-bundle.schema.json)
// Struct fields are ordered strictly alphabetically so serde_json produces canonical key ordering.
// ================================================================================================

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelahBundleManifest {
    format: String,
    media_items: Vec<BundleMediaItem>,
    presentations: Vec<BundlePresentation>,
    schema_version: u32,
    songs: Vec<BundleSong>,
    tags: Vec<BundleTag>,
    templates: Vec<BundleTemplate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BundleSectionKind {
    Verse,
    Chorus,
    Bridge,
    Tag,
    Intro,
    Outro,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BundleMediaKind {
    Image,
    Video,
    Audio,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleResolution {
    height: u32,
    width: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleColor {
    alpha: f64,
    blue: f64,
    green: f64,
    red: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleChord {
    position: u32,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleLyricLine {
    chords: Vec<BundleChord>,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<BundleColor>,
    hotkey: String,
    id: String,
    kind: BundleSectionKind,
    label: String,
    lines: Vec<BundleLyricLine>,
    translations: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleArrangement {
    id: String,
    name: String,
    section_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleSong {
    arrangements: Vec<BundleArrangement>,
    authors: Vec<String>,
    ccli_number: String,
    copyright: String,
    id: String,
    key: String,
    public_domain: bool,
    sections: Vec<BundleSection>,
    tags: Vec<String>,
    tempo: f64,
    title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleTemplateVariant {
    id: String,
    max_lines: u32,
    resolution: BundleResolution,
    style_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleTemplate {
    id: String,
    name: String,
    variants: Vec<BundleTemplateVariant>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleMediaItem {
    content_hash: String,
    duration_milliseconds: u64,
    extension: String,
    id: String,
    kind: BundleMediaKind,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolution: Option<BundleResolution>,
    size_bytes: u64,
    tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleSlide {
    elements_json: String,
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    media_id: Option<String>,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundlePresentation {
    id: String,
    name: String,
    slides: Vec<BundleSlide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    template_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleTag {
    id: String,
    name: String,
}

fn norm_zero(v: f64) -> f64 {
    if v == 0.0 { 0.0 } else { v }
}

fn norm_zero_f32(v: f32) -> f64 {
    if v == 0.0 { 0.0 } else { f64::from(v) }
}

fn bundle_color_component(v: f64, channel: &str) -> anyhow::Result<f32> {
    ensure!(
        v.is_finite() && (0.0..=1.0).contains(&v),
        "color.{channel} must be between 0.0 and 1.0"
    );
    Ok(norm_zero(v) as f32)
}

fn id_to_uuid_str(id: &Option<Id>) -> anyhow::Result<String> {
    let raw = required_id(id)?;
    let parsed = uuid::Uuid::from_slice(raw)?;
    Ok(parsed.hyphenated().to_string())
}

fn uuid_str_to_id(s: &str) -> anyhow::Result<Id> {
    let parsed = uuid::Uuid::parse_str(s).context("invalid UUID string")?;
    ensure!(
        parsed.get_version_num() == 7
            && parsed.get_variant() == uuid::Variant::RFC4122
            && parsed.hyphenated().to_string() == s,
        "expected canonical lowercase hyphenated UUIDv7"
    );
    Ok(Id {
        value: parsed.as_bytes().to_vec(),
    })
}

fn song_to_bundle(song: &Song) -> anyhow::Result<BundleSong> {
    song.validate()?;
    let id = id_to_uuid_str(&song.id)?;
    ensure!(song.ccli_number.len() <= 64, "ccli_number exceeds 64 bytes");
    ensure!(song.copyright.len() <= 2048, "copyright exceeds 2048 bytes");
    ensure!(song.key.len() <= 32, "key exceeds 32 bytes");
    for a in &song.authors {
        ensure!(a.len() <= 512, "author exceeds 512 bytes");
    }
    for t in &song.tags {
        ensure!(t.len() <= 512, "tag exceeds 512 bytes");
    }

    let mut sections = Vec::with_capacity(song.sections.len());
    for s in &song.sections {
        let kind = match SectionKind::try_from(s.kind) {
            Ok(SectionKind::Verse) => BundleSectionKind::Verse,
            Ok(SectionKind::Chorus) => BundleSectionKind::Chorus,
            Ok(SectionKind::Bridge) => BundleSectionKind::Bridge,
            Ok(SectionKind::Tag) => BundleSectionKind::Tag,
            Ok(SectionKind::Intro) => BundleSectionKind::Intro,
            Ok(SectionKind::Outro) => BundleSectionKind::Outro,
            Ok(SectionKind::Other) => BundleSectionKind::Other,
            _ => anyhow::bail!("invalid section kind"),
        };
        ensure!(s.hotkey.len() <= 16, "section hotkey exceeds 16 bytes");
        let lines = s
            .lines
            .iter()
            .map(|l| {
                ensure!(l.text.len() <= 65_536, "lyric line exceeds 65536 bytes");
                let chords = l
                    .chords
                    .iter()
                    .map(|c| {
                        ensure!(c.position <= 65_535, "chord position exceeds 65535");
                        Ok(BundleChord {
                            position: c.position,
                            text: c.text.clone(),
                        })
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?;
                Ok(BundleLyricLine {
                    chords,
                    text: l.text.clone(),
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let mut translations = BTreeMap::new();
        for (lang, tl) in &s.translations {
            for line in &tl.lines {
                ensure!(line.len() <= 65_536, "translation line exceeds 65536 bytes");
            }
            translations.insert(lang.clone(), tl.lines.clone());
        }
        sections.push(BundleSection {
            color: s.color.as_ref().map(|c| BundleColor {
                alpha: norm_zero_f32(c.alpha),
                blue: norm_zero_f32(c.blue),
                green: norm_zero_f32(c.green),
                red: norm_zero_f32(c.red),
            }),
            hotkey: s.hotkey.clone(),
            id: id_to_uuid_str(&s.id)?,
            kind,
            label: s.label.clone(),
            lines,
            translations,
        });
    }

    let mut arrangements = Vec::with_capacity(song.arrangements.len());
    for a in &song.arrangements {
        let section_ids = a
            .section_ids
            .iter()
            .map(|sid| id_to_uuid_str(&Some(sid.clone())))
            .collect::<anyhow::Result<Vec<_>>>()?;
        arrangements.push(BundleArrangement {
            id: id_to_uuid_str(&a.id)?,
            name: a.name.clone(),
            section_ids,
        });
    }

    Ok(BundleSong {
        arrangements,
        authors: song.authors.clone(),
        ccli_number: song.ccli_number.clone(),
        copyright: song.copyright.clone(),
        id,
        key: song.key.clone(),
        public_domain: song.public_domain,
        sections,
        tags: song.tags.clone(),
        tempo: norm_zero(song.tempo),
        title: song.title.clone(),
    })
}

fn bundle_to_song(b: &BundleSong) -> anyhow::Result<Song> {
    ensure!(b.ccli_number.len() <= 64, "ccli_number exceeds 64 bytes");
    ensure!(b.copyright.len() <= 2048, "copyright exceeds 2048 bytes");
    ensure!(b.key.len() <= 32, "key exceeds 32 bytes");
    for a in &b.authors {
        ensure!(a.len() <= 512, "author exceeds 512 bytes");
    }
    for t in &b.tags {
        ensure!(t.len() <= 512, "tag exceeds 512 bytes");
    }
    let sections = b
        .sections
        .iter()
        .map(|s| {
            ensure!(s.hotkey.len() <= 16, "hotkey exceeds 16 bytes");
            let kind = match s.kind {
                BundleSectionKind::Verse => SectionKind::Verse as i32,
                BundleSectionKind::Chorus => SectionKind::Chorus as i32,
                BundleSectionKind::Bridge => SectionKind::Bridge as i32,
                BundleSectionKind::Tag => SectionKind::Tag as i32,
                BundleSectionKind::Intro => SectionKind::Intro as i32,
                BundleSectionKind::Outro => SectionKind::Outro as i32,
                BundleSectionKind::Other => SectionKind::Other as i32,
            };
            let lines = s
                .lines
                .iter()
                .map(|l| {
                    ensure!(l.text.len() <= 65_536, "lyric line exceeds 65536 bytes");
                    let chords = l
                        .chords
                        .iter()
                        .map(|c| {
                            ensure!(c.position <= 65_535, "chord position exceeds 65535");
                            Ok(Chord {
                                position: c.position,
                                text: c.text.clone(),
                            })
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    Ok(LyricLine {
                        text: l.text.clone(),
                        chords,
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            let mut translations = HashMap::new();
            for (lang, lines) in &s.translations {
                for line in lines {
                    ensure!(line.len() <= 65_536, "translation line exceeds 65536 bytes");
                }
                translations.insert(
                    lang.clone(),
                    TranslationLines {
                        lines: lines.clone(),
                    },
                );
            }
            Ok(Section {
                id: Some(uuid_str_to_id(&s.id)?),
                label: s.label.clone(),
                kind,
                color: s
                    .color
                    .as_ref()
                    .map(|c| -> anyhow::Result<Color> {
                        Ok(Color {
                            red: bundle_color_component(c.red, "red")?,
                            green: bundle_color_component(c.green, "green")?,
                            blue: bundle_color_component(c.blue, "blue")?,
                            alpha: bundle_color_component(c.alpha, "alpha")?,
                        })
                    })
                    .transpose()?,
                hotkey: s.hotkey.clone(),
                lines,
                translations,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let arrangements = b
        .arrangements
        .iter()
        .map(|a| {
            let section_ids = a
                .section_ids
                .iter()
                .map(|sid| uuid_str_to_id(sid))
                .collect::<anyhow::Result<Vec<_>>>()?;
            Ok(Arrangement {
                id: Some(uuid_str_to_id(&a.id)?),
                name: a.name.clone(),
                section_ids,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let song = Song {
        id: Some(uuid_str_to_id(&b.id)?),
        title: b.title.clone(),
        authors: b.authors.clone(),
        ccli_number: b.ccli_number.clone(),
        copyright: b.copyright.clone(),
        key: b.key.clone(),
        tempo: norm_zero(b.tempo),
        tags: b.tags.clone(),
        public_domain: b.public_domain,
        sections,
        arrangements,
    };
    ensure!(
        song.encoded_len() <= MAX_DOCUMENT_BYTES,
        "song exceeds document limit"
    );
    song.validate()?;
    Ok(song)
}

fn template_to_bundle(t: &Template) -> anyhow::Result<BundleTemplate> {
    t.validate()?;
    let variants = t
        .variants
        .iter()
        .map(|v| {
            ensure!(
                v.style_json.len() <= 65_536,
                "style_json exceeds 65536 bytes"
            );
            let r = v.resolution.as_ref().context("missing resolution")?;
            Ok(BundleTemplateVariant {
                id: id_to_uuid_str(&v.id)?,
                max_lines: v.max_lines,
                resolution: BundleResolution {
                    height: r.height,
                    width: r.width,
                },
                style_json: v.style_json.clone(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(BundleTemplate {
        id: id_to_uuid_str(&t.id)?,
        name: t.name.clone(),
        variants,
    })
}

fn bundle_to_template(b: &BundleTemplate) -> anyhow::Result<Template> {
    let variants = b
        .variants
        .iter()
        .map(|v| {
            ensure!(
                v.style_json.len() <= 65_536,
                "style_json exceeds 65536 bytes"
            );
            Ok(TemplateVariant {
                id: Some(uuid_str_to_id(&v.id)?),
                resolution: Some(Resolution {
                    width: v.resolution.width,
                    height: v.resolution.height,
                }),
                max_lines: v.max_lines,
                style_json: v.style_json.clone(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let tmpl = Template {
        id: Some(uuid_str_to_id(&b.id)?),
        name: b.name.clone(),
        variants,
    };
    ensure!(
        tmpl.encoded_len() <= MAX_DOCUMENT_BYTES,
        "template exceeds document limit"
    );
    tmpl.validate()?;
    Ok(tmpl)
}

fn media_to_bundle(m: &MediaItem) -> anyhow::Result<BundleMediaItem> {
    m.validate()?;
    let kind = match MediaKind::try_from(m.kind) {
        Ok(MediaKind::Image) => BundleMediaKind::Image,
        Ok(MediaKind::Video) => BundleMediaKind::Video,
        Ok(MediaKind::Audio) => BundleMediaKind::Audio,
        _ => anyhow::bail!("invalid media kind"),
    };
    if let Some(res) = &m.resolution {
        ensure!(
            (1..=16_384).contains(&res.width) && (1..=16_384).contains(&res.height),
            "invalid media resolution"
        );
    }
    for t in &m.tags {
        ensure!(
            !t.trim().is_empty() && t.chars().count() <= 512 && !t.contains('\0'),
            "invalid media tag"
        );
    }
    Ok(BundleMediaItem {
        content_hash: m.content_hash.clone(),
        duration_milliseconds: m.duration_milliseconds,
        extension: m.extension.clone(),
        id: id_to_uuid_str(&m.id)?,
        kind,
        name: m.name.clone(),
        resolution: m.resolution.as_ref().map(|r| BundleResolution {
            height: r.height,
            width: r.width,
        }),
        size_bytes: m.size_bytes,
        tags: m.tags.clone(),
    })
}

fn bundle_to_media(b: &BundleMediaItem) -> anyhow::Result<MediaItem> {
    if let Some(res) = &b.resolution {
        ensure!(
            (1..=16_384).contains(&res.width) && (1..=16_384).contains(&res.height),
            "invalid media resolution"
        );
    }
    for t in &b.tags {
        ensure!(
            !t.trim().is_empty() && t.chars().count() <= 512 && !t.contains('\0'),
            "invalid media tag"
        );
    }
    let item = MediaItem {
        id: Some(uuid_str_to_id(&b.id)?),
        name: b.name.clone(),
        kind: match b.kind {
            BundleMediaKind::Image => MediaKind::Image as i32,
            BundleMediaKind::Video => MediaKind::Video as i32,
            BundleMediaKind::Audio => MediaKind::Audio as i32,
        },
        content_hash: b.content_hash.clone(),
        extension: b.extension.clone(),
        size_bytes: b.size_bytes,
        resolution: b.resolution.as_ref().map(|r| Resolution {
            width: r.width,
            height: r.height,
        }),
        duration_milliseconds: b.duration_milliseconds,
        flash_warning: b.tags.iter().any(|t| t.eq_ignore_ascii_case("flashing")),
        tags: b.tags.clone(),
    };
    ensure!(
        item.encoded_len() <= MAX_DOCUMENT_BYTES,
        "media item exceeds document limit"
    );
    item.validate()?;
    Ok(item)
}

fn presentation_to_bundle(p: &Presentation) -> anyhow::Result<BundlePresentation> {
    p.validate()?;
    let slides = p
        .slides
        .iter()
        .map(|s| {
            ensure!(s.text.len() <= 65_536, "slide text exceeds 65536 bytes");
            ensure!(
                s.elements_json.len() <= 1_048_576,
                "slide elements_json exceeds 1 MiB"
            );
            let media_id = s
                .media_id
                .as_ref()
                .map(|id| id_to_uuid_str(&Some(id.clone())))
                .transpose()?;
            Ok(BundleSlide {
                elements_json: s.elements_json.clone(),
                id: id_to_uuid_str(&s.id)?,
                media_id,
                text: s.text.clone(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let template_id = p
        .template_id
        .as_ref()
        .map(|id| id_to_uuid_str(&Some(id.clone())))
        .transpose()?;
    Ok(BundlePresentation {
        id: id_to_uuid_str(&p.id)?,
        name: p.name.clone(),
        slides,
        template_id,
    })
}

fn bundle_to_presentation(b: &BundlePresentation) -> anyhow::Result<Presentation> {
    let slides = b
        .slides
        .iter()
        .map(|s| {
            ensure!(s.text.len() <= 65_536, "slide text exceeds 65536 bytes");
            ensure!(
                s.elements_json.len() <= 1_048_576,
                "slide elements_json exceeds 1 MiB"
            );
            Ok(Slide {
                id: Some(uuid_str_to_id(&s.id)?),
                text: s.text.clone(),
                media_id: s.media_id.as_deref().map(uuid_str_to_id).transpose()?,
                elements_json: s.elements_json.clone(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let pres = Presentation {
        id: Some(uuid_str_to_id(&b.id)?),
        name: b.name.clone(),
        slides,
        template_id: b.template_id.as_deref().map(uuid_str_to_id).transpose()?,
    };
    ensure!(
        pres.encoded_len() <= MAX_DOCUMENT_BYTES,
        "presentation exceeds document limit"
    );
    pres.validate()?;
    Ok(pres)
}

fn tag_to_bundle(t: &Tag) -> anyhow::Result<BundleTag> {
    t.validate()?;
    Ok(BundleTag {
        id: id_to_uuid_str(&t.id)?,
        name: t.name.clone(),
    })
}

fn bundle_to_tag(b: &BundleTag) -> anyhow::Result<Tag> {
    let tag = Tag {
        id: Some(uuid_str_to_id(&b.id)?),
        name: b.name.clone(),
    };
    ensure!(
        tag.encoded_len() <= MAX_DOCUMENT_BYTES,
        "tag exceeds document limit"
    );
    tag.validate()?;
    Ok(tag)
}

struct ParsedManifestDocs {
    songs: Vec<Song>,
    templates: Vec<Template>,
    media_items: Vec<MediaItem>,
    presentations: Vec<Presentation>,
    tags: Vec<Tag>,
}

fn validate_and_convert_manifest(
    manifest: &SelahBundleManifest,
) -> anyhow::Result<ParsedManifestDocs> {
    ensure!(
        manifest.format == "selah-bundle",
        "unsupported bundle format"
    );
    ensure!(
        manifest.schema_version == 1,
        "unsupported bundle schema version"
    );
    ensure!(
        manifest.songs.len() <= MAX_BUNDLE_ENTITIES
            && manifest.templates.len() <= MAX_BUNDLE_ENTITIES
            && manifest.media_items.len() <= MAX_BUNDLE_ENTITIES
            && manifest.presentations.len() <= MAX_BUNDLE_ENTITIES
            && manifest.tags.len() <= MAX_BUNDLE_ENTITIES,
        "bundle exceeds {MAX_BUNDLE_ENTITIES} entities per type"
    );

    let mut songs = Vec::with_capacity(manifest.songs.len());
    let mut song_ids = HashSet::new();
    for s in &manifest.songs {
        let doc = bundle_to_song(s)?;
        ensure!(
            song_ids.insert(s.id.clone()),
            "duplicate song ID '{}'",
            s.id
        );
        songs.push(doc);
    }

    let mut templates = Vec::with_capacity(manifest.templates.len());
    let mut template_ids = HashSet::new();
    for t in &manifest.templates {
        let doc = bundle_to_template(t)?;
        ensure!(
            template_ids.insert(t.id.clone()),
            "duplicate template ID '{}'",
            t.id
        );
        templates.push(doc);
    }

    let mut media_items = Vec::with_capacity(manifest.media_items.len());
    let mut media_ids = HashSet::new();
    for m in &manifest.media_items {
        let doc = bundle_to_media(m)?;
        ensure!(
            media_ids.insert(m.id.clone()),
            "duplicate media ID '{}'",
            m.id
        );
        media_items.push(doc);
    }

    let mut presentations = Vec::with_capacity(manifest.presentations.len());
    let mut pres_ids = HashSet::new();
    for p in &manifest.presentations {
        let doc = bundle_to_presentation(p)?;
        ensure!(
            pres_ids.insert(p.id.clone()),
            "duplicate presentation ID '{}'",
            p.id
        );
        presentations.push(doc);
    }

    let mut tags = Vec::with_capacity(manifest.tags.len());
    let mut tag_ids = HashSet::new();
    for t in &manifest.tags {
        let doc = bundle_to_tag(t)?;
        ensure!(tag_ids.insert(t.id.clone()), "duplicate tag ID '{}'", t.id);
        tags.push(doc);
    }

    Ok(ParsedManifestDocs {
        songs,
        templates,
        media_items,
        presentations,
        tags,
    })
}

fn parse_media_entry_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("media/")?;
    if rest.contains('/') {
        return None;
    }
    let (hash, ext) = rest.split_once('.')?;
    super::media::validate_address(hash, ext).ok()?;
    Some((hash, ext))
}

/// Pure in-memory fuzzing entry point for `.selah` bundles (SEC-03, M2-A11).
/// Never panics and never performs filesystem or network I/O.
pub fn fuzz_selah_bundle(bytes: &[u8]) -> usize {
    let Ok(entries) = read_zip_bytes(bytes, ZipLimits::fuzz()) else {
        return 0;
    };
    let mut manifest_bytes: Option<&[u8]> = None;
    let mut media_entries = HashMap::new();
    for (name, data) in &entries {
        if name == "manifest.json" {
            manifest_bytes = Some(data.as_slice());
        } else if let Some((hash, ext)) = parse_media_entry_name(name) {
            let actual = blake3::hash(data).to_hex().to_string();
            if actual != hash {
                return 0;
            }
            media_entries.insert((hash.to_owned(), ext.to_owned()), data.len() as u64);
        } else {
            return 0;
        }
    }
    let Some(mbytes) = manifest_bytes else {
        return 0;
    };
    if mbytes.len() > MAX_MANIFEST_BYTES {
        return 0;
    }
    let Ok(manifest) = serde_json::from_slice::<SelahBundleManifest>(mbytes) else {
        return 0;
    };
    let Ok(docs) = validate_and_convert_manifest(&manifest) else {
        return 0;
    };
    for m in &docs.media_items {
        if let Some(&sz) = media_entries.get(&(m.content_hash.clone(), m.extension.clone()))
            && sz != m.size_bytes
        {
            return 0;
        }
    }
    docs.songs.len()
        + docs.templates.len()
        + docs.media_items.len()
        + docs.presentations.len()
        + docs.tags.len()
}

fn list_all_docs<T: Document>(library: &Library) -> anyhow::Result<Vec<T>> {
    let mut out = Vec::new();
    let mut offset = 0u32;
    loop {
        let page = library.list::<T>(200, offset)?;
        let count = page.len();
        out.extend(page);
        ensure!(
            out.len() <= MAX_BUNDLE_ENTITIES,
            "export exceeds {MAX_BUNDLE_ENTITIES} entities per type"
        );
        if count < 200 {
            break;
        }
        offset = offset.saturating_add(200);
    }
    Ok(out)
}

fn load_requested_docs<T: Document>(
    library: &Library,
    ids: &[Id],
) -> Result<Vec<T>, CommandBusError> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        validate_id(id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        if !seen.insert(id.value.clone()) {
            continue;
        }
        let doc = library
            .get::<T>(id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("requested library item not found".into()))?;
        out.push(doc);
    }
    Ok(out)
}

// ================================================================================================
// Backup Manifest
// ================================================================================================

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupBibleEntry {
    bytes: u64,
    code: String,
    sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupMediaEntry {
    content_hash: String,
    extension: String,
    size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupManifest {
    bibles: Vec<BackupBibleEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bundled_bibles_marker_sha256: Option<String>,
    created_at_unix_ms: i64,
    database_bytes: u64,
    database_sha256: String,
    format: String,
    media_blobs: Vec<BackupMediaEntry>,
    media_count: u32,
    presentation_count: u32,
    schema_version: u32,
    song_count: u32,
    tag_count: u32,
    template_count: u32,
}

fn sha256_file_and_len(path: &Path) -> anyhow::Result<(String, u64)> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .context("file length overflow")?;
        hasher.update(&buf[..n]);
    }
    Ok((format!("{:x}", hasher.finalize()), total))
}

fn collect_verified_media_blobs(media_root: &Path) -> anyhow::Result<Vec<(String, String, u64)>> {
    let store = MediaStore::open(media_root)?;
    let canon_root = media_root.canonicalize()?;
    let mut blobs = Vec::new();
    for entry in std::fs::read_dir(&canon_root)? {
        let entry = entry?;
        let dir_path = entry.path();
        let dir_meta = std::fs::symlink_metadata(&dir_path)?;
        if dir_meta.file_type().is_symlink() || !dir_meta.is_dir() {
            continue;
        }
        let Some(prefix) = dir_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if prefix.starts_with('.')
            || prefix.len() != 2
            || !prefix.bytes().all(|b| b.is_ascii_hexdigit())
        {
            continue;
        }
        for file_entry in std::fs::read_dir(&dir_path)? {
            let file_entry = file_entry?;
            let fpath = file_entry.path();
            let fmeta = std::fs::symlink_metadata(&fpath)?;
            ensure!(
                !fmeta.file_type().is_symlink(),
                "media store contains forbidden symlink"
            );
            if !fmeta.is_file() {
                continue;
            }
            let Some(fname) = fpath.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if fname.starts_with('.') {
                continue;
            }
            let (hash, ext) = fname
                .split_once('.')
                .with_context(|| format!("invalid media blob file name '{fname}'"))?;
            ensure!(
                hash.starts_with(prefix),
                "media blob '{fname}' stored in wrong shard directory"
            );
            super::media::validate_address(hash, ext)?;
            let size = fmeta.len();
            store.verify(hash, ext, size)?;
            blobs.push((hash.to_owned(), ext.to_owned(), size));
        }
    }
    blobs.sort();
    Ok(blobs)
}

fn check_bible_sqlite_integrity(path: &Path, expected_code: &str) -> anyhow::Result<()> {
    let db = open_bible_db(path)?;
    let mut check = db.prepare("PRAGMA integrity_check")?;
    let results = check
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        results == ["ok"],
        "Bible database '{expected_code}' failed integrity check"
    );
    let stored_code: String =
        db.query_row("SELECT value FROM bible_meta WHERE key='code'", [], |r| {
            r.get(0)
        })?;
    ensure!(
        stored_code == expected_code,
        "Bible database code '{stored_code}' mismatch for '{expected_code}'"
    );
    Ok(())
}

fn count_library_entities(db: &Connection) -> anyhow::Result<(u32, u32, u32, u32, u32)> {
    let count = |table: &str| -> anyhow::Result<u32> {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        let n: i64 = db.query_row(&sql, [], |r| r.get(0))?;
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    };
    Ok((
        count("songs")?,
        count("templates")?,
        count("media_items")?,
        count("presentations")?,
        count("tags")?,
    ))
}

fn verify_db_media_references(db: &Connection, store: &MediaStore) -> anyhow::Result<()> {
    let mut stmt = db.prepare("SELECT data FROM media_items ORDER BY id")?;
    let rows = stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
    for row in rows {
        let bytes = row?;
        ensure!(
            bytes.len() <= MAX_DOCUMENT_BYTES,
            "stored MediaItem exceeds limit"
        );
        let item = MediaItem::decode(bytes.as_slice())?;
        item.validate()?;
        store.verify(&item.content_hash, &item.extension, item.size_bytes)?;
    }
    Ok(())
}

impl Library {
    /// Import a local media file (image, video, audio) through `selah-media-worker` (SEC-04),
    /// store its content-addressed BLAKE3 blob, cache its PNG thumbnail, and record an undoable
    /// `MediaItem` document in the command log.
    pub fn import_media_op(
        &mut self,
        req: &ImportMedia,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        if let Some(replayed) = check_idempotency_readonly(&self.db, meta)? {
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        if req.tags.len() > super::MAX_CHILDREN {
            return Err(CommandBusError::Invalid("too many tags".into()));
        }
        let mut clean_tags = Vec::with_capacity(req.tags.len());
        for tag in &req.tags {
            let trimmed = tag.trim();
            validate_label(trimmed, "tag")?;
            clean_tags.push(trimmed.to_owned());
        }

        let canonical_source = check_local_input_path(&req.source_path)?;
        let ext = canonical_source
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| CommandBusError::Invalid("media file needs an extension".into()))?;
        super::media::validate_address(&"0".repeat(64), &ext)
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;

        let display_name = if req.name.trim().is_empty() {
            canonical_source
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::trim)
                .unwrap_or("")
                .to_owned()
        } else {
            req.name.trim().to_owned()
        };
        validate_label(&display_name, "media name")?;

        precheck_image_header(&canonical_source, &ext)?;

        let probe_dir = tempfile::Builder::new()
            .prefix(".probe-")
            .tempdir_in(&self.paths.root)
            .map_err(|e| CommandBusError::Internal(e.into()))?;
        let probe_thumb_path = probe_dir.path().join("thumb.png");

        let report = run_media_worker(
            &canonical_source,
            None,
            DEFAULT_THUMB_W,
            DEFAULT_THUMB_H,
            Some(&probe_thumb_path),
            true,
        )?;

        let thumb_bytes = if report.thumbnail_written {
            let (_, _, bytes) =
                read_validated_png(&probe_thumb_path, DEFAULT_THUMB_W, DEFAULT_THUMB_H)?;
            Some(bytes)
        } else {
            None
        };

        let store = self.media_store().map_err(CommandBusError::Internal)?;
        let blob = store
            .import(&canonical_source)
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;

        if let Some(png) = thumb_bytes {
            let thumbs_dir = ensure_thumbnails_dir(&self.paths.root)?;
            let cache_path = thumbs_dir.join(format!(
                "{}_{}x{}.png",
                blob.content_hash, DEFAULT_THUMB_W, DEFAULT_THUMB_H
            ));
            if std::fs::symlink_metadata(&cache_path).is_err() {
                write_atomic_file_checked(&cache_path, &png)?;
            }
        }

        if report.flash_warning
            && !clean_tags
                .iter()
                .any(|t| t.eq_ignore_ascii_case("flashing"))
            && clean_tags.len() < super::MAX_CHILDREN
        {
            clean_tags.push("Flashing".to_owned());
        }

        let item = MediaItem {
            id: Some(new_id()),
            name: display_name,
            kind: match report.kind.as_str() {
                "image" => MediaKind::Image as i32,
                "video" => MediaKind::Video as i32,
                "audio" => MediaKind::Audio as i32,
                other => {
                    return Err(CommandBusError::Internal(anyhow::anyhow!(
                        "unexpected worker media kind '{other}'"
                    )));
                }
            },
            content_hash: blob.content_hash,
            extension: blob.extension,
            size_bytes: blob.size_bytes,
            resolution: (report.width > 0 && report.height > 0).then_some(Resolution {
                width: report.width,
                height: report.height,
            }),
            duration_milliseconds: report.duration_ms,
            flash_warning: report.flash_warning,
            tags: clean_tags,
        };

        let reply = reply::Result::MediaItem(item.clone());
        self.save_document_with_reply_op(&item, meta, reply)
    }

    /// Fetch (or generate via `selah-media-worker` on cache miss) a PNG thumbnail for an image or
    /// video `MediaItem`. Audio items are cleanly refused.
    pub fn get_media_thumbnail(
        &self,
        req: &GetMediaThumbnail,
    ) -> Result<MediaThumbnail, CommandBusError> {
        let media_id = req
            .media_id
            .as_ref()
            .ok_or_else(|| CommandBusError::Invalid("missing media_id".into()))?;
        validate_id(media_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;

        let item = self
            .get::<MediaItem>(media_id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("media item not found".into()))?;

        let kind_str = match MediaKind::try_from(item.kind) {
            Ok(MediaKind::Image) => "image",
            Ok(MediaKind::Video) => "video",
            Ok(MediaKind::Audio) => {
                return Err(CommandBusError::Invalid(
                    "audio items do not have thumbnails".into(),
                ));
            }
            _ => return Err(CommandBusError::Invalid("unknown media kind".into())),
        };

        let max_w = if req.max_width == 0 {
            DEFAULT_THUMB_W
        } else {
            req.max_width.clamp(16, 1024)
        };
        let max_h = if req.max_height == 0 {
            DEFAULT_THUMB_H
        } else {
            req.max_height.clamp(16, 1024)
        };

        super::media::validate_address(&item.content_hash, &item.extension)
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;

        let blob_path = self
            .paths
            .media
            .join(&item.content_hash[..2])
            .join(format!("{}.{}", item.content_hash, item.extension));

        if self.paths.root.join("thumbnails").exists() {
            let thumbs_dir = ensure_thumbnails_dir(&self.paths.root)?;
            let cache_path =
                thumbs_dir.join(format!("{}_{}x{}.png", item.content_hash, max_w, max_h));
            if std::fs::symlink_metadata(&cache_path).is_ok()
                && let Ok(blob_meta) = std::fs::symlink_metadata(&blob_path)
                && !blob_meta.file_type().is_symlink()
                && blob_meta.is_file()
                && blob_meta.len() == item.size_bytes
                && let Ok((w, h, png_bytes)) = read_validated_png(&cache_path, max_w, max_h)
            {
                return Ok(MediaThumbnail {
                    media_id: Some(media_id.clone()),
                    width: w,
                    height: h,
                    png_bytes,
                });
            }
        }

        let store = self.media_store().map_err(CommandBusError::Internal)?;
        let verified_path = store
            .verified_blob_path(&item.content_hash, &item.extension, item.size_bytes)
            .map_err(|e| {
                CommandBusError::Invalid(format!("media blob verification failed: {e}"))
            })?;

        let probe_dir = tempfile::Builder::new()
            .prefix(".thumb-gen-")
            .tempdir_in(&self.paths.root)
            .map_err(|e| CommandBusError::Internal(e.into()))?;
        let probe_thumb_path = probe_dir.path().join("thumb.png");

        let report = run_media_worker(
            &verified_path,
            Some(kind_str),
            max_w,
            max_h,
            Some(&probe_thumb_path),
            false,
        )?;
        if !report.thumbnail_written {
            return Err(CommandBusError::Invalid(
                "worker did not produce a thumbnail".into(),
            ));
        }

        let (w, h, png_bytes) = read_validated_png(&probe_thumb_path, max_w, max_h)?;
        let thumbs_dir = ensure_thumbnails_dir(&self.paths.root)?;
        let cache_path = thumbs_dir.join(format!("{}_{}x{}.png", item.content_hash, max_w, max_h));
        write_atomic_file_checked(&cache_path, &png_bytes)?;

        Ok(MediaThumbnail {
            media_id: Some(media_id.clone()),
            width: w,
            height: h,
            png_bytes,
        })
    }

    /// Export selected library entities (or the whole library when all selection lists are empty)
    /// to a canonical deterministic `.selah` archive.
    pub fn export_selah_bundle_op(
        &mut self,
        req: &ExportSelahBundle,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        if let Some(replayed) = check_idempotency_readonly(&self.db, meta)? {
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        let target = validate_output_file_path(&req.output_path)?;
        let export_all = req.song_ids.is_empty()
            && req.template_ids.is_empty()
            && req.media_ids.is_empty()
            && req.presentation_ids.is_empty()
            && req.tag_ids.is_empty();

        let songs: Vec<Song> = if export_all {
            list_all_docs(self).map_err(|e| CommandBusError::Invalid(e.to_string()))?
        } else {
            load_requested_docs(self, &req.song_ids)?
        };
        let templates: Vec<Template> = if export_all {
            list_all_docs(self).map_err(|e| CommandBusError::Invalid(e.to_string()))?
        } else {
            load_requested_docs(self, &req.template_ids)?
        };
        let media_items: Vec<MediaItem> = if export_all {
            list_all_docs(self).map_err(|e| CommandBusError::Invalid(e.to_string()))?
        } else {
            load_requested_docs(self, &req.media_ids)?
        };
        let presentations: Vec<Presentation> = if export_all {
            list_all_docs(self).map_err(|e| CommandBusError::Invalid(e.to_string()))?
        } else {
            load_requested_docs(self, &req.presentation_ids)?
        };
        let tags: Vec<Tag> = if export_all {
            list_all_docs(self).map_err(|e| CommandBusError::Invalid(e.to_string()))?
        } else {
            load_requested_docs(self, &req.tag_ids)?
        };

        let mut bundle_songs = songs
            .iter()
            .map(song_to_bundle)
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        bundle_songs.sort_by(|a, b| a.id.cmp(&b.id));

        let mut bundle_templates = templates
            .iter()
            .map(template_to_bundle)
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        bundle_templates.sort_by(|a, b| a.id.cmp(&b.id));

        let mut bundle_media = media_items
            .iter()
            .map(media_to_bundle)
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        bundle_media.sort_by(|a, b| a.id.cmp(&b.id));

        let mut bundle_presentations = presentations
            .iter()
            .map(presentation_to_bundle)
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        bundle_presentations.sort_by(|a, b| a.id.cmp(&b.id));

        let mut bundle_tags = tags
            .iter()
            .map(tag_to_bundle)
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        bundle_tags.sort_by(|a, b| a.id.cmp(&b.id));

        let manifest = SelahBundleManifest {
            format: "selah-bundle".into(),
            media_items: bundle_media,
            presentations: bundle_presentations,
            schema_version: 1,
            songs: bundle_songs,
            tags: bundle_tags,
            templates: bundle_templates,
        };
        let canonical_manifest_json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| CommandBusError::Internal(e.into()))?;
        if canonical_manifest_json.len() > MAX_MANIFEST_BYTES {
            return Err(CommandBusError::Invalid(
                "bundle manifest exceeds 16 MiB limit".into(),
            ));
        }

        let parent = target
            .parent()
            .ok_or_else(|| CommandBusError::Invalid("output has no parent directory".into()))?;
        let temp = tempfile::Builder::new()
            .prefix(".selah-bundle-")
            .suffix(".tmp")
            .tempfile_in(parent)
            .map_err(|e| CommandBusError::Internal(e.into()))?;

        let limits = ZipLimits::bundle();
        let total_bytes = {
            let mut zip = DeterministicZipWriter::new(temp.as_file());
            zip.add_bytes(
                "manifest.json",
                canonical_manifest_json.as_bytes(),
                Compression::Deflated,
            )
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;

            let mut total_uncompressed = canonical_manifest_json.len() as u64;
            let mut entries = 1usize;

            if req.include_media {
                let store = self.media_store().map_err(CommandBusError::Internal)?;
                let mut blobs: BTreeSet<(String, String, u64)> = BTreeSet::new();
                for m in &media_items {
                    blobs.insert((m.content_hash.clone(), m.extension.clone(), m.size_bytes));
                }
                for (hash, ext, size_bytes) in blobs {
                    entries += 1;
                    if entries > limits.max_entries || size_bytes > limits.max_entry_bytes {
                        return Err(CommandBusError::Invalid(
                            "exported bundle exceeds archive entry limits".into(),
                        ));
                    }
                    total_uncompressed = total_uncompressed.saturating_add(size_bytes);
                    if total_uncompressed > limits.max_total_uncompressed_bytes {
                        return Err(CommandBusError::Invalid(
                            "exported bundle exceeds total uncompressed limit".into(),
                        ));
                    }
                    let reader = store
                        .open_verified(&hash, &ext, size_bytes)
                        .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
                    zip.add_reader(
                        &format!("media/{hash}.{ext}"),
                        reader,
                        size_bytes,
                        Compression::Stored,
                    )
                    .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
                }
            }

            let (file, written) = zip
                .finish()
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
            if written > limits.max_archive_bytes {
                return Err(CommandBusError::Invalid(
                    "exported bundle exceeds archive size limit".into(),
                ));
            }
            file.sync_all()
                .map_err(|e| CommandBusError::Internal(e.into()))?;
            written
        };

        temp.persist(&target)
            .map_err(|e| CommandBusError::Internal(e.error.into()))?;

        let result_msg = ExportedSelahBundle {
            output_path: target.to_string_lossy().into_owned(),
            canonical_manifest_json,
            song_count: manifest.songs.len() as u32,
            template_count: manifest.templates.len() as u32,
            media_count: manifest.media_items.len() as u32,
            presentation_count: manifest.presentations.len() as u32,
            tag_count: manifest.tags.len() as u32,
            total_bytes,
        };
        let reply = reply::Result::ExportedSelahBundle(result_msg);

        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(replayed) = check_idempotency_in_tx(&tx, meta)? {
            tx.commit()?;
            return Ok(ExecutionOutcome::Replayed(replayed));
        }
        record_idempotency_in_tx(
            &tx,
            meta,
            &reply,
            unix_millis(),
            &mut self.idempotency_writes,
        )?;
        tx.commit()?;

        Ok(ExecutionOutcome::Applied {
            reply,
            changes: Vec::new(),
            undo_doc_id: None,
        })
    }

    /// Import a `.selah` bundle (from file path or raw bytes) atomically inside one shared undo
    /// group, verifying archive limits, JSON schema rules, and BLAKE3 media blobs before mutating
    /// anything on disk or in SQLite (SEC-03, SEC-15, M2-A6, M2-A11).
    pub fn import_selah_bundle_op(
        &mut self,
        req: &ImportSelahBundle,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        if let Some(replayed) = check_idempotency_readonly(&self.db, meta)? {
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        let limits = ZipLimits::bundle();
        let stage_dir = tempfile::Builder::new()
            .prefix(".import-stage-")
            .tempdir_in(&self.paths.media)
            .map_err(|e| CommandBusError::Internal(e.into()))?;

        let mut manifest_bytes: Option<Vec<u8>> = None;
        let mut staged_media: HashMap<(String, String), (PathBuf, u64)> = HashMap::new();

        {
            let mut handle_entry =
                |name: &str, size: u64, reader: &mut dyn Read| -> anyhow::Result<()> {
                    if name == "manifest.json" {
                        ensure!(manifest_bytes.is_none(), "duplicate manifest.json");
                        ensure!(
                            size <= MAX_MANIFEST_BYTES as u64,
                            "manifest.json exceeds {MAX_MANIFEST_BYTES} byte limit"
                        );
                        let mut buf = Vec::with_capacity((size as usize).min(1024 * 1024));
                        reader
                            .take(MAX_MANIFEST_BYTES as u64 + 1)
                            .read_to_end(&mut buf)?;
                        ensure!(
                            buf.len() <= MAX_MANIFEST_BYTES,
                            "manifest.json exceeds size limit"
                        );
                        manifest_bytes = Some(buf);
                        return Ok(());
                    }
                    let Some((hash, ext)) = parse_media_entry_name(name) else {
                        anyhow::bail!("unexpected archive entry '{name}' in .selah bundle");
                    };
                    let staged_path = stage_dir.path().join(format!("{hash}.{ext}"));
                    let mut out = std::fs::File::create(&staged_path)?;
                    let mut hasher = blake3::Hasher::new();
                    let mut count = 0u64;
                    let mut buf = [0u8; 64 * 1024];
                    loop {
                        let n = reader.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        count += n as u64;
                        hasher.update(&buf[..n]);
                        out.write_all(&buf[..n])?;
                    }
                    out.sync_all()?;
                    ensure!(count == size, "media entry '{name}' size mismatch");
                    let actual_hash = hasher.finalize().to_hex().to_string();
                    ensure!(
                        actual_hash == hash,
                        "media entry '{name}' failed BLAKE3 verification"
                    );
                    staged_media.insert((hash.to_owned(), ext.to_owned()), (staged_path, count));
                    Ok(())
                };

            let visit_res = if !req.raw_bytes.is_empty() {
                if req.raw_bytes.len() as u64 > limits.max_archive_bytes {
                    return Err(CommandBusError::Invalid(
                        "bundle exceeds archive size limit".into(),
                    ));
                }
                visit_zip(Cursor::new(&req.raw_bytes), limits, &mut handle_entry)
            } else if !req.source_path.trim().is_empty() {
                let source = check_local_input_path(&req.source_path)?;
                let file = std::fs::File::open(&source)
                    .map_err(|e| CommandBusError::Invalid(format!("cannot open bundle: {e}")))?;
                visit_zip(file, limits, &mut handle_entry)
            } else {
                return Err(CommandBusError::Invalid(
                    "either source_path or raw_bytes must be provided".into(),
                ));
            };
            visit_res.map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        }

        let mbytes = manifest_bytes
            .ok_or_else(|| CommandBusError::Invalid("bundle missing manifest.json".into()))?;
        let manifest: SelahBundleManifest = serde_json::from_slice(&mbytes)
            .map_err(|e| CommandBusError::Invalid(format!("invalid bundle manifest: {e}")))?;
        let docs = validate_and_convert_manifest(&manifest)
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;

        // Every staged media blob in the ZIP must be referenced by at least one MediaItem,
        // and every MediaItem must have its verified blob either staged or already in MediaStore.
        let store = self.media_store().map_err(CommandBusError::Internal)?;
        let mut referenced_keys = HashSet::new();
        for item in &docs.media_items {
            let key = (item.content_hash.clone(), item.extension.clone());
            referenced_keys.insert(key.clone());
            if let Some((_, staged_size)) = staged_media.get(&key) {
                if *staged_size != item.size_bytes {
                    return Err(CommandBusError::Invalid(format!(
                        "media item '{}' size_bytes does not match archive blob",
                        item.name
                    )));
                }
            } else {
                store
                    .verify(&item.content_hash, &item.extension, item.size_bytes)
                    .map_err(|e| {
                        CommandBusError::Invalid(format!(
                            "media item '{}' blob missing from bundle and local store: {e}",
                            item.name
                        ))
                    })?;
            }
        }
        for key in staged_media.keys() {
            if !referenced_keys.contains(key) {
                return Err(CommandBusError::Invalid(format!(
                    "bundle contains unreferenced media blob '{}.{}'",
                    key.0, key.1
                )));
            }
        }

        let group = resolve_undo_group(meta.undo_group)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(replayed) = check_idempotency_in_tx(&tx, meta)? {
            tx.commit()?;
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        // Decide which items are written before publishing any staged media blobs.
        enum PreparedDoc {
            Tag(Tag),
            Template(Template),
            Media(MediaItem),
            Song(Song),
            Presentation(Presentation),
        }

        impl PreparedDoc {
            fn kind(&self) -> DocumentKind {
                match self {
                    Self::Tag(_) => DocumentKind::Tag,
                    Self::Template(_) => DocumentKind::Template,
                    Self::Media(_) => DocumentKind::Media,
                    Self::Song(_) => DocumentKind::Song,
                    Self::Presentation(_) => DocumentKind::Presentation,
                }
            }
            fn id_bytes(&self) -> &[u8] {
                match self {
                    Self::Tag(d) => d.id.as_ref().map(|i| i.value.as_slice()).unwrap_or(&[]),
                    Self::Template(d) => d.id.as_ref().map(|i| i.value.as_slice()).unwrap_or(&[]),
                    Self::Media(d) => d.id.as_ref().map(|i| i.value.as_slice()).unwrap_or(&[]),
                    Self::Song(d) => d.id.as_ref().map(|i| i.value.as_slice()).unwrap_or(&[]),
                    Self::Presentation(d) => {
                        d.id.as_ref().map(|i| i.value.as_slice()).unwrap_or(&[])
                    }
                }
            }
            fn encode_to_vec(&self) -> Vec<u8> {
                match self {
                    Self::Tag(d) => d.encode_to_vec(),
                    Self::Template(d) => d.encode_to_vec(),
                    Self::Media(d) => d.encode_to_vec(),
                    Self::Song(d) => d.encode_to_vec(),
                    Self::Presentation(d) => d.encode_to_vec(),
                }
            }
            fn write(&self, tx: &rusqlite::Transaction<'_>) -> anyhow::Result<()> {
                match self {
                    Self::Tag(d) => d.write(tx),
                    Self::Template(d) => d.write(tx),
                    Self::Media(d) => d.write(tx),
                    Self::Song(d) => d.write(tx),
                    Self::Presentation(d) => d.write(tx),
                }
            }
        }

        let candidates: Vec<PreparedDoc> = docs
            .tags
            .into_iter()
            .map(PreparedDoc::Tag)
            .chain(docs.templates.into_iter().map(PreparedDoc::Template))
            .chain(docs.media_items.into_iter().map(PreparedDoc::Media))
            .chain(docs.songs.into_iter().map(PreparedDoc::Song))
            .chain(
                docs.presentations
                    .into_iter()
                    .map(PreparedDoc::Presentation),
            )
            .collect();

        let mut to_apply: Vec<(PreparedDoc, Option<Vec<u8>>)> = Vec::new();
        for doc in candidates {
            let before = read_snapshot_in_tx(&tx, doc.kind(), doc.id_bytes())?;
            if before.is_some() && !req.overwrite_existing {
                continue;
            }
            to_apply.push((doc, before));
        }

        // Promote staged blobs only for MediaItems that will actually be written.
        for (doc, _) in &to_apply {
            if let PreparedDoc::Media(item) = doc {
                let key = (item.content_hash.clone(), item.extension.clone());
                if let Some((staged_path, size_bytes)) = staged_media.get(&key) {
                    let file = std::fs::File::open(staged_path)
                        .map_err(|e| CommandBusError::Internal(e.into()))?;
                    let imported = store
                        .import_reader(file, &item.extension, MAX_MEDIA_BYTES)
                        .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
                    if imported.content_hash != item.content_hash
                        || imported.size_bytes != *size_bytes
                    {
                        return Err(CommandBusError::Invalid(
                            "staged media verification failed during commit".into(),
                        ));
                    }
                }
            }
        }

        let ts = unix_millis();
        let mut changes = Vec::with_capacity(to_apply.len());
        let mut counts = (0u32, 0u32, 0u32, 0u32, 0u32);

        for (doc, before) in to_apply {
            let kind = doc.kind();
            let doc_id = doc.id_bytes().to_vec();
            let forward_data = doc.encode_to_vec();
            doc.write(&tx).map_err(super::ops::apply_doc_bus_error)?;

            tx.execute(
                "DELETE FROM undo_entries WHERE doc_id=?1 AND undone>0",
                [&doc_id],
            )?;

            let (inverse_present, inverse_data) = match before {
                Some(bytes) => (true, bytes),
                None => (false, Vec::new()),
            };
            let cmd_blob = encode_op_state(true, &forward_data);
            let inv_blob = encode_op_state(inverse_present, &inverse_data);
            let op_seq = insert_op_in_tx(
                &tx,
                NewOp {
                    ts,
                    actor: meta.actor,
                    kind,
                    doc_id: &doc_id,
                    command: &cmd_blob,
                    inverse: &inv_blob,
                    undo_group: &group,
                },
            )?;
            tx.execute(
                "INSERT INTO undo_entries (op_seq, undo_group, doc_type, doc_id, forward_present, forward_data, inverse_present, inverse_data, undone) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, 0)",
                params![
                    op_seq as i64,
                    &group,
                    kind.as_str(),
                    &doc_id,
                    forward_data,
                    inverse_present as i32,
                    inverse_data,
                ],
            )?;

            match kind {
                DocumentKind::Song => counts.0 += 1,
                DocumentKind::Template => counts.1 += 1,
                DocumentKind::Media => counts.2 += 1,
                DocumentKind::Presentation => counts.3 += 1,
                DocumentKind::Tag => counts.4 += 1,
                _ => {}
            }

            changes.push(MutationEventInfo {
                doc_type: kind,
                doc_id: Id { value: doc_id },
                change: LibraryChangeKind::Saved,
                op_seq,
                undo_group: Id {
                    value: group.clone(),
                },
                from_undo_redo: false,
            });
        }

        let reply = reply::Result::ImportedSelahBundle(ImportedSelahBundle {
            song_count: counts.0,
            template_count: counts.1,
            media_count: counts.2,
            presentation_count: counts.3,
            tag_count: counts.4,
        });
        record_idempotency_in_tx(&tx, meta, &reply, ts, &mut self.idempotency_writes)?;
        tx.commit()?;

        Ok(ExecutionOutcome::Applied {
            reply,
            changes,
            undo_doc_id: None,
        })
    }

    /// Create a full self-verifying library backup ZIP (`library.db`, `bibles/`, verified media
    /// blobs, and SHA-256 `backup-manifest.json`).
    pub fn create_library_backup_op(
        &mut self,
        req: &CreateLibraryBackup,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        if let Some(replayed) = check_idempotency_readonly(&self.db, meta)? {
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        let target = if req.output_path.trim().is_empty() {
            let ts = unix_millis();
            let mut candidate = self.paths.backups.join(format!("selah-backup-{ts}.zip"));
            let mut suffix = 1u32;
            while std::fs::symlink_metadata(&candidate).is_ok() {
                candidate = self
                    .paths
                    .backups
                    .join(format!("selah-backup-{ts}-{suffix}.zip"));
                suffix += 1;
            }
            candidate
        } else {
            validate_output_file_path(&req.output_path)?
        };

        let stage_dir = tempfile::Builder::new()
            .prefix(".backup-stage-")
            .tempdir_in(&self.paths.backups)
            .map_err(|e| CommandBusError::Internal(e.into()))?;

        let db_snap = stage_dir.path().join("library.db");
        self.db
            .execute("VACUUM INTO ?1", [&sqlite_path_str(&db_snap)])
            .map_err(|e| CommandBusError::Internal(e.into()))?;
        let (song_count, template_count, media_count, presentation_count, tag_count) = {
            let snap_conn = Connection::open_with_flags(
                &db_snap,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            snap_conn.execute_batch("PRAGMA trusted_schema = OFF;")?;
            check_integrity(&snap_conn).map_err(CommandBusError::Internal)?;
            schema::check_history(&snap_conn).map_err(CommandBusError::Internal)?;
            count_library_entities(&snap_conn).map_err(CommandBusError::Internal)?
        };
        let (database_sha256, database_bytes) =
            sha256_file_and_len(&db_snap).map_err(CommandBusError::Internal)?;

        let bibles_list = self
            .list_validated_bible_files()
            .map_err(CommandBusError::Internal)?;
        let mut bible_entries = Vec::with_capacity(bibles_list.len());
        let mut bible_snaps = Vec::with_capacity(bibles_list.len());
        for (code, bpath) in bibles_list {
            let snap_path = stage_dir.path().join(format!("bible-{code}.db"));
            {
                let bdb = open_bible_db(&bpath).map_err(CommandBusError::Internal)?;
                bdb.execute("VACUUM INTO ?1", [&sqlite_path_str(&snap_path)])
                    .map_err(|e| CommandBusError::Internal(e.into()))?;
            }
            check_bible_sqlite_integrity(&snap_path, &code).map_err(CommandBusError::Internal)?;
            let (sha256, bytes) =
                sha256_file_and_len(&snap_path).map_err(CommandBusError::Internal)?;
            bible_entries.push(BackupBibleEntry {
                bytes,
                code: code.clone(),
                sha256,
            });
            bible_snaps.push((code, snap_path, bytes));
        }
        bible_entries.sort_by(|a, b| a.code.cmp(&b.code));

        let marker_path = self.paths.bibles.join(BUNDLED_MARKER_FILE);
        let marker_bytes = match std::fs::symlink_metadata(&marker_path) {
            Ok(m) if !m.file_type().is_symlink() && m.is_file() => {
                Some(std::fs::read(&marker_path).map_err(|e| CommandBusError::Internal(e.into()))?)
            }
            _ => None,
        };
        let bundled_bibles_marker_sha256 = marker_bytes.as_ref().map(|bytes| {
            let mut h = Sha256::new();
            h.update(bytes);
            format!("{:x}", h.finalize())
        });

        let store = self.media_store().map_err(CommandBusError::Internal)?;
        verify_db_media_references(&self.db, &store)
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let blobs =
            collect_verified_media_blobs(&self.paths.media).map_err(CommandBusError::Internal)?;
        let media_blobs: Vec<BackupMediaEntry> = blobs
            .iter()
            .map(|(content_hash, extension, size_bytes)| BackupMediaEntry {
                content_hash: content_hash.clone(),
                extension: extension.clone(),
                size_bytes: *size_bytes,
            })
            .collect();

        let limits = ZipLimits::backup();
        let entry_count = 2 + bible_snaps.len() + usize::from(marker_bytes.is_some()) + blobs.len();
        if entry_count > limits.max_entries {
            return Err(CommandBusError::Invalid(
                "backup exceeds maximum entry count".into(),
            ));
        }

        let created_at_unix_ms = unix_millis();
        let manifest = BackupManifest {
            bibles: bible_entries,
            bundled_bibles_marker_sha256,
            created_at_unix_ms,
            database_bytes,
            database_sha256,
            format: "selah-backup".into(),
            media_blobs,
            media_count,
            presentation_count,
            schema_version: self.schema_version().map_err(CommandBusError::Internal)?,
            song_count,
            tag_count,
            template_count,
        };
        let manifest_json = serde_json::to_vec_pretty(&manifest)
            .map_err(|e| CommandBusError::Internal(e.into()))?;

        let parent = target
            .parent()
            .ok_or_else(|| CommandBusError::Invalid("backup target has no parent".into()))?;
        let temp_zip = tempfile::Builder::new()
            .prefix(".backup-zip-")
            .suffix(".tmp")
            .tempfile_in(parent)
            .map_err(|e| CommandBusError::Internal(e.into()))?;

        let size_bytes = {
            let mut zip = DeterministicZipWriter::new(temp_zip.as_file());
            zip.add_bytes(
                "backup-manifest.json",
                &manifest_json,
                Compression::Deflated,
            )
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;

            let mut total_uncompressed = manifest_json.len() as u64 + database_bytes;
            if database_bytes > limits.max_entry_bytes
                || total_uncompressed > limits.max_total_uncompressed_bytes
            {
                return Err(CommandBusError::Invalid(
                    "library database exceeds backup size limits".into(),
                ));
            }
            let db_file =
                std::fs::File::open(&db_snap).map_err(|e| CommandBusError::Internal(e.into()))?;
            zip.add_reader("library.db", db_file, database_bytes, Compression::Deflated)
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;

            if let Some(mbytes) = marker_bytes.as_deref() {
                total_uncompressed = total_uncompressed.saturating_add(mbytes.len() as u64);
                zip.add_bytes(BACKUP_BUNDLED_MARKER_ENTRY, mbytes, Compression::Deflated)
                    .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
            }

            for (code, path, bytes_len) in bible_snaps {
                total_uncompressed = total_uncompressed.saturating_add(bytes_len);
                if bytes_len > limits.max_entry_bytes
                    || total_uncompressed > limits.max_total_uncompressed_bytes
                {
                    return Err(CommandBusError::Invalid(
                        "Bible database exceeds backup size limits".into(),
                    ));
                }
                let f =
                    std::fs::File::open(&path).map_err(|e| CommandBusError::Internal(e.into()))?;
                zip.add_reader(
                    &format!("bibles/{code}.db"),
                    f,
                    bytes_len,
                    Compression::Deflated,
                )
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
            }

            for (hash, ext, size) in &blobs {
                total_uncompressed = total_uncompressed.saturating_add(*size);
                if *size > limits.max_entry_bytes
                    || total_uncompressed > limits.max_total_uncompressed_bytes
                {
                    return Err(CommandBusError::Invalid(
                        "media store exceeds backup size limits".into(),
                    ));
                }
                let reader = store
                    .open_verified(hash, ext, *size)
                    .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
                zip.add_reader(
                    &format!("media/{hash}.{ext}"),
                    reader,
                    *size,
                    Compression::Stored,
                )
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
            }

            let (file, written) = zip
                .finish()
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
            if written > limits.max_archive_bytes {
                return Err(CommandBusError::Invalid(
                    "backup ZIP exceeds maximum archive size".into(),
                ));
            }
            file.sync_all()
                .map_err(|e| CommandBusError::Internal(e.into()))?;
            written
        };

        temp_zip
            .persist(&target)
            .map_err(|e| CommandBusError::Internal(e.error.into()))?;

        let info = LibraryBackupInfo {
            backup_path: target.to_string_lossy().into_owned(),
            schema_version: manifest.schema_version,
            size_bytes,
            media_blob_count: blobs.len() as u32,
            bible_count: manifest.bibles.len() as u32,
        };
        let reply = reply::Result::LibraryBackupInfo(info);

        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(replayed) = check_idempotency_in_tx(&tx, meta)? {
            tx.commit()?;
            return Ok(ExecutionOutcome::Replayed(replayed));
        }
        record_idempotency_in_tx(
            &tx,
            meta,
            &reply,
            unix_millis(),
            &mut self.idempotency_writes,
        )?;
        tx.commit()?;

        Ok(ExecutionOutcome::Applied {
            reply,
            changes: Vec::new(),
            undo_doc_id: None,
        })
    }

    /// Fully extract and verify a library backup archive in staging, close live handles,
    /// swap into place with atomic rollback on any failure, and reopen cleanly (SEC-15, M2-A6).
    pub fn restore_library_backup_op(
        &mut self,
        req: &RestoreLibraryBackup,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        if let Some(replayed) = check_idempotency_readonly(&self.db, meta)? {
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        let backup_path = check_local_input_path(&req.backup_path)?;
        let stage = tempfile::Builder::new()
            .prefix(".restore-stage-")
            .tempdir_in(&self.paths.root)
            .map_err(|e| CommandBusError::Internal(e.into()))?;
        let stage_db = stage.path().join("library.db");
        let stage_bibles = stage.path().join("bibles");
        let stage_media = stage.path().join("media");
        std::fs::create_dir_all(&stage_bibles).map_err(|e| CommandBusError::Internal(e.into()))?;
        std::fs::create_dir_all(&stage_media).map_err(|e| CommandBusError::Internal(e.into()))?;

        let mut manifest_bytes: Option<Vec<u8>> = None;
        let mut marker_bytes: Option<Vec<u8>> = None;
        let mut db_actual: Option<(String, u64)> = None;
        let mut bibles_actual: BTreeMap<String, (PathBuf, String, u64)> = BTreeMap::new();
        let mut media_actual: BTreeMap<(String, String), u64> = BTreeMap::new();

        {
            let file = std::fs::File::open(&backup_path)
                .map_err(|e| CommandBusError::Invalid(format!("cannot open backup: {e}")))?;
            let limits = ZipLimits::backup();
            visit_zip(file, limits, |name, size, reader| {
                if name == "backup-manifest.json" {
                    ensure!(manifest_bytes.is_none(), "duplicate backup-manifest.json");
                    ensure!(
                        size <= MAX_MANIFEST_BYTES as u64,
                        "backup manifest too large"
                    );
                    let mut buf = Vec::with_capacity((size as usize).min(1024 * 1024));
                    reader
                        .take(MAX_MANIFEST_BYTES as u64 + 1)
                        .read_to_end(&mut buf)?;
                    manifest_bytes = Some(buf);
                    return Ok(());
                }
                if name == "library.db" {
                    ensure!(db_actual.is_none(), "duplicate library.db in backup");
                    let mut out = std::fs::File::create(&stage_db)?;
                    let mut hasher = Sha256::new();
                    let mut total = 0u64;
                    let mut buf = [0u8; 64 * 1024];
                    loop {
                        let n = reader.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        total += n as u64;
                        hasher.update(&buf[..n]);
                        out.write_all(&buf[..n])?;
                    }
                    out.sync_all()?;
                    ensure!(total == size, "library.db size mismatch");
                    db_actual = Some((format!("{:x}", hasher.finalize()), total));
                    return Ok(());
                }
                if name == BACKUP_BUNDLED_MARKER_ENTRY {
                    ensure!(size <= 64 * 1024, "bundled Bible marker exceeds limit");
                    let mut buf = Vec::new();
                    reader.take(64 * 1024 + 1).read_to_end(&mut buf)?;
                    ensure!(buf.len() <= 64 * 1024, "bundled Bible marker exceeds limit");
                    marker_bytes = Some(buf);
                    return Ok(());
                }
                if let Some(code) = name
                    .strip_prefix("bibles/")
                    .and_then(|s| s.strip_suffix(".db"))
                    && !code.contains('/')
                    && search::is_valid_translation_code(code)
                {
                    let bpath = stage_bibles.join(format!("{code}.db"));
                    let mut out = std::fs::File::create(&bpath)?;
                    let mut hasher = Sha256::new();
                    let mut total = 0u64;
                    let mut buf = [0u8; 64 * 1024];
                    loop {
                        let n = reader.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        total += n as u64;
                        hasher.update(&buf[..n]);
                        out.write_all(&buf[..n])?;
                    }
                    out.sync_all()?;
                    ensure!(total == size, "Bible '{code}' size mismatch");
                    bibles_actual.insert(
                        code.to_owned(),
                        (bpath, format!("{:x}", hasher.finalize()), total),
                    );
                    return Ok(());
                }
                if let Some((hash, ext)) = parse_media_entry_name(name) {
                    let shard = stage_media.join(&hash[..2]);
                    std::fs::create_dir_all(&shard)?;
                    let mpath = shard.join(format!("{hash}.{ext}"));
                    let mut out = std::fs::File::create(&mpath)?;
                    let mut hasher = blake3::Hasher::new();
                    let mut total = 0u64;
                    let mut buf = [0u8; 64 * 1024];
                    loop {
                        let n = reader.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        total += n as u64;
                        hasher.update(&buf[..n]);
                        out.write_all(&buf[..n])?;
                    }
                    out.sync_all()?;
                    ensure!(total == size, "media '{name}' size mismatch");
                    ensure!(
                        hasher.finalize().to_hex().as_str() == hash,
                        "media '{name}' BLAKE3 mismatch"
                    );
                    media_actual.insert((hash.to_owned(), ext.to_owned()), total);
                    return Ok(());
                }
                anyhow::bail!("unexpected archive entry '{name}' in backup")
            })
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        }

        let mbytes = manifest_bytes.ok_or_else(|| {
            CommandBusError::Invalid("backup missing backup-manifest.json".into())
        })?;
        let manifest: BackupManifest = serde_json::from_slice(&mbytes)
            .map_err(|e| CommandBusError::Invalid(format!("invalid backup manifest: {e}")))?;

        if manifest.format != "selah-backup" {
            return Err(CommandBusError::Invalid("unsupported backup format".into()));
        }
        if manifest.schema_version > CURRENT_SCHEMA_VERSION {
            return Err(CommandBusError::Invalid(
                "backup was written by a newer Selah version".into(),
            ));
        }

        let Some((db_sha, db_len)) = db_actual else {
            return Err(CommandBusError::Invalid("backup missing library.db".into()));
        };
        if db_sha != manifest.database_sha256 || db_len != manifest.database_bytes {
            return Err(CommandBusError::Invalid(
                "backup library.db SHA-256 or size mismatch".into(),
            ));
        }

        match (&manifest.bundled_bibles_marker_sha256, &marker_bytes) {
            (Some(expected), Some(actual)) => {
                let mut h = Sha256::new();
                h.update(actual);
                if format!("{:x}", h.finalize()) != *expected {
                    return Err(CommandBusError::Invalid(
                        "bundled Bibles marker SHA-256 mismatch".into(),
                    ));
                }
            }
            (None, None) => {}
            _ => {
                return Err(CommandBusError::Invalid(
                    "backup bundled Bibles marker mismatch".into(),
                ));
            }
        }

        if manifest.bibles.len() != bibles_actual.len() {
            return Err(CommandBusError::Invalid(
                "backup Bible database list does not match archive".into(),
            ));
        }
        for b in &manifest.bibles {
            let Some((path, actual_sha, actual_bytes)) = bibles_actual.get(&b.code) else {
                return Err(CommandBusError::Invalid(format!(
                    "backup missing Bible '{}'",
                    b.code
                )));
            };
            if actual_sha != &b.sha256 || *actual_bytes != b.bytes {
                return Err(CommandBusError::Invalid(format!(
                    "backup Bible '{}' checksum mismatch",
                    b.code
                )));
            }
            check_bible_sqlite_integrity(path, &b.code)
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        }

        if manifest.media_blobs.len() != media_actual.len() {
            return Err(CommandBusError::Invalid(
                "backup media blob list does not match archive".into(),
            ));
        }
        for m in &manifest.media_blobs {
            let Some(actual_size) =
                media_actual.get(&(m.content_hash.clone(), m.extension.clone()))
            else {
                return Err(CommandBusError::Invalid(format!(
                    "backup missing media blob '{}.{}'",
                    m.content_hash, m.extension
                )));
            };
            if *actual_size != m.size_bytes {
                return Err(CommandBusError::Invalid(format!(
                    "backup media blob '{}.{}' size mismatch",
                    m.content_hash, m.extension
                )));
            }
        }

        if let Some(mbytes) = marker_bytes {
            write_atomic_file_checked(&stage_bibles.join(BUNDLED_MARKER_FILE), &mbytes)?;
        }

        // Verify staged SQLite DB integrity, migration compatibility, FTS search index, and media
        // blob references before closing or modifying the live library.
        let (
            schema_version,
            song_count,
            template_count,
            media_count,
            presentation_count,
            tag_count,
        ) = {
            let mut staged_conn = Connection::open_with_flags(
                &stage_db,
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            staged_conn.busy_timeout(Duration::from_secs(5))?;
            check_integrity(&staged_conn).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
            schema::check_history(&staged_conn)
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
            staged_conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA trusted_schema = OFF;")?;
            schema::migrate(&mut staged_conn)
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
            search::ensure_search_ready(&mut staged_conn)
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;

            let staged_store = MediaStore::open(&stage_media)
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
            verify_db_media_references(&staged_conn, &staged_store)
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;

            let counts = count_library_entities(&staged_conn).map_err(CommandBusError::Internal)?;
            let ver: u32 = staged_conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            staged_conn.execute_batch("PRAGMA journal_mode = DELETE;")?;
            (ver, counts.0, counts.1, counts.2, counts.3, counts.4)
        };

        // Release all live SQLite file handles before moving files on Windows.
        let placeholder =
            Connection::open_in_memory().map_err(|e| CommandBusError::Internal(e.into()))?;
        let old_db = std::mem::replace(&mut self.db, placeholder);
        drop(old_db);

        let rollback = tempfile::Builder::new()
            .prefix(".restore-rollback-")
            .tempdir_in(&self.paths.root)
            .map_err(|e| CommandBusError::Internal(e.into()))?;

        let rb_db = rollback.path().join("library.db");
        let rb_wal = rollback.path().join("library.db-wal");
        let rb_shm = rollback.path().join("library.db-shm");
        let rb_bibles = rollback.path().join("bibles");
        let rb_media = rollback.path().join("media");
        let live_wal = PathBuf::from(format!("{}-wal", self.paths.database.display()));
        let live_shm = PathBuf::from(format!("{}-shm", self.paths.database.display()));

        let move_or_rollback = || -> anyhow::Result<()> {
            if self.paths.database.exists() {
                std::fs::rename(&self.paths.database, &rb_db)?;
            }
            if live_wal.exists() {
                std::fs::rename(&live_wal, &rb_wal)?;
            }
            if live_shm.exists() {
                std::fs::rename(&live_shm, &rb_shm)?;
            }
            if self.paths.bibles.exists() {
                std::fs::rename(&self.paths.bibles, &rb_bibles)?;
            }
            if self.paths.media.exists() {
                std::fs::rename(&self.paths.media, &rb_media)?;
            }
            std::fs::rename(&stage_db, &self.paths.database)?;
            std::fs::rename(&stage_bibles, &self.paths.bibles)?;
            std::fs::rename(&stage_media, &self.paths.media)?;
            let thumbs = self.paths.root.join("thumbnails");
            if thumbs.exists() {
                let _ = std::fs::remove_dir_all(&thumbs);
            }
            Ok(())
        };

        let restored_conn = move_or_rollback().and_then(|()| open_library_connection(&self.paths));

        let new_db = match restored_conn {
            Ok(conn) => conn,
            Err(err) => {
                // Best-effort atomic rollback to previous live library.
                let _ = std::fs::remove_file(&self.paths.database);
                let _ = std::fs::remove_file(&live_wal);
                let _ = std::fs::remove_file(&live_shm);
                if rb_db.exists() {
                    let _ = std::fs::rename(&rb_db, &self.paths.database);
                }
                if rb_wal.exists() {
                    let _ = std::fs::rename(&rb_wal, &live_wal);
                }
                if rb_shm.exists() {
                    let _ = std::fs::rename(&rb_shm, &live_shm);
                }
                if rb_bibles.exists() {
                    let _ = std::fs::remove_dir_all(&self.paths.bibles);
                    let _ = std::fs::rename(&rb_bibles, &self.paths.bibles);
                }
                if rb_media.exists() {
                    let _ = std::fs::remove_dir_all(&self.paths.media);
                    let _ = std::fs::rename(&rb_media, &self.paths.media);
                }
                if let Ok(reopened) = open_library_connection(&self.paths) {
                    self.db = reopened;
                }
                return Err(CommandBusError::Internal(err));
            }
        };

        self.db = new_db;
        self.idempotency_writes = 0;

        let result_msg = RestoreLibraryBackupResult {
            schema_version,
            song_count,
            template_count,
            media_count,
            presentation_count,
            tag_count,
            media_blob_count: manifest.media_blobs.len() as u32,
            bible_count: manifest.bibles.len() as u32,
        };
        let reply = reply::Result::RestoreLibraryBackupResult(result_msg);

        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if meta.envelope_id != 0 {
            tx.execute(
                "DELETE FROM command_idempotency WHERE actor_origin=?1 AND user_id=?2 AND device_id=?3 AND envelope_id=?4",
                params![
                    meta.actor.origin,
                    meta.actor.user_id,
                    meta.actor.device_id,
                    meta.envelope_id as i64
                ],
            )?;
        }
        record_idempotency_in_tx(
            &tx,
            meta,
            &reply,
            unix_millis(),
            &mut self.idempotency_writes,
        )?;
        tx.commit()?;

        Ok(ExecutionOutcome::Applied {
            reply,
            changes: Vec::new(),
            undo_doc_id: None,
        })
    }
}
