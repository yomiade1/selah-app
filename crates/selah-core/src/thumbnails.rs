//! Offscreen scene thumbnail rasterisation and bounded BLAKE3 disk cache (M3-A3, SEC-03, SEC-04).
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, mpsc};
use std::time::SystemTime;

use prost::Message;
use selah_proto::v1::{
    MediaLayerSpec, RenderSceneThumbnail, Resolution, Scene, SceneThumbnail, layer_spec,
};
use selah_render::Compositor;
use selah_render::cpu::CpuCompositor;
use selah_render::effects::validate_scene_shader_chains;
use selah_render::flash::analyze_transition_risk;
use selah_render::png::{decode as decode_png, encode_rgba_thumbnail, fit_dimensions};
use selah_render::scene::clamp_resolution;
use selah_render::slide_elements::SlideElementsDocument;
use selah_render::template::TemplateStyle;
use selah_render::video::Nv12Frame;

use crate::library::exchange::{
    DEFAULT_THUMB_H, DEFAULT_THUMB_W, ensure_thumbnails_dir, read_validated_png, run_media_worker,
    write_atomic_file_checked,
};
use crate::library::media::validate_address;
use crate::library::{CommandBusError, Library};
use crate::live::LiveState;

/// Hard bounds for the cached scene thumbnail directory (`library/thumbnails/scenes/`).
pub const MAX_SCENE_THUMB_FILES: usize = 512;
pub const MAX_SCENE_THUMB_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RENDER_PIXELS: u64 = 3840 * 2160;
const DETERMINISTIC_THUMB_CLOCK_UNIX_MS: i64 = 1_731_240_000_000;

struct ThumbRenderJob {
    media_root: PathBuf,
    scene: Scene,
    output_index: u32,
    render_w: u32,
    render_h: u32,
    max_w: u32,
    max_h: u32,
    video_posters: Vec<(String, Nv12Frame)>,
    reply: mpsc::SyncSender<anyhow::Result<(u32, u32, Vec<u8>)>>,
}

fn thumb_worker() -> &'static mpsc::Sender<ThumbRenderJob> {
    static WORKER: OnceLock<mpsc::Sender<ThumbRenderJob>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<ThumbRenderJob>();
        std::thread::Builder::new()
            .name("selah-thumb-render".into())
            .spawn(move || {
                let mut compositor = CpuCompositor::default();
                compositor.set_clock_unix_ms(Some(DETERMINISTIC_THUMB_CLOCK_UNIX_MS));
                let mut current_media_root: Option<PathBuf> = None;
                while let Ok(job) = rx.recv() {
                    if current_media_root.as_ref() != Some(&job.media_root) {
                        compositor.set_media_root(Some(job.media_root.clone()));
                        current_media_root = Some(job.media_root);
                    }
                    let result = (|| -> anyhow::Result<(u32, u32, Vec<u8>)> {
                        let mut loaded = Vec::with_capacity(job.video_posters.len());
                        for (hash, nv12) in job.video_posters {
                            compositor.set_video_for_hash(&hash, Some(nv12))?;
                            loaded.push(hash);
                        }
                        let rendered = compositor.render_output(
                            &job.scene,
                            job.output_index,
                            job.render_w,
                            job.render_h,
                        );
                        for hash in loaded {
                            let _ = compositor.set_video_for_hash(&hash, None);
                        }
                        let frame = rendered?;
                        encode_rgba_thumbnail(&frame, job.max_w, job.max_h)
                    })();
                    let _ = job.reply.send(result);
                }
            })
            .expect("spawn selah thumbnail render worker");
        tx
    })
}

fn valid_resolution(res: Resolution) -> bool {
    (16..=7680).contains(&res.width) && (16..=4320).contains(&res.height)
}

fn media_extension_for(m: &MediaLayerSpec) -> &str {
    let ext = m.media_extension.trim();
    if ext.is_empty() {
        if m.media_kind == "video" {
            "mp4"
        } else {
            "png"
        }
    } else {
        ext
    }
}

fn validate_scene_inputs(scene: &Scene) -> Result<(), CommandBusError> {
    if let Some(res) = scene.resolution
        && !valid_resolution(res)
    {
        return Err(CommandBusError::Invalid("invalid scene resolution".into()));
    }
    validate_scene_shader_chains(scene).map_err(|e| CommandBusError::Invalid(e.to_string()))?;

    for out in &scene.outputs {
        if let Some(res) = out.resolution
            && !valid_resolution(res)
        {
            return Err(CommandBusError::Invalid("invalid output resolution".into()));
        }
        for layer in &out.layers {
            if layer.use_opacity && !layer.opacity.is_finite() {
                return Err(CommandBusError::Invalid(
                    "layer opacity must be finite".into(),
                ));
            }
            match layer.content.as_ref() {
                Some(layer_spec::Content::VideoInput(v)) => {
                    if v.use_opacity && !v.opacity.is_finite() {
                        return Err(CommandBusError::Invalid(
                            "video input opacity must be finite".into(),
                        ));
                    }
                }
                Some(layer_spec::Content::Media(m)) => {
                    if (m.use_opacity && !m.opacity.is_finite()) || !m.blur_radius.is_finite() {
                        return Err(CommandBusError::Invalid(
                            "media layer numeric parameters must be finite".into(),
                        ));
                    }
                    let hash = m.content_hash.trim();
                    if !hash.is_empty() {
                        let ext = media_extension_for(m);
                        validate_address(hash, ext)
                            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
                    }
                }
                Some(layer_spec::Content::Slide(s)) => {
                    if s.use_opacity && !s.opacity.is_finite() {
                        return Err(CommandBusError::Invalid(
                            "slide layer opacity must be finite".into(),
                        ));
                    }
                    if !s.style_json.trim().is_empty() {
                        TemplateStyle::from_json(&s.style_json).map_err(|e| {
                            CommandBusError::Invalid(format!("invalid style_json: {e}"))
                        })?;
                    }
                    if !s.elements_json.trim().is_empty() {
                        SlideElementsDocument::from_json_strict(&s.elements_json).map_err(|e| {
                            CommandBusError::Invalid(format!("invalid elements_json: {e}"))
                        })?;
                    }
                }
                Some(layer_spec::Content::Props(p)) => {
                    if p.use_opacity && !p.opacity.is_finite() {
                        return Err(CommandBusError::Invalid(
                            "props layer opacity must be finite".into(),
                        ));
                    }
                    if !p.elements_json.trim().is_empty() {
                        SlideElementsDocument::from_json_strict(&p.elements_json).map_err(|e| {
                            CommandBusError::Invalid(format!("invalid elements_json: {e}"))
                        })?;
                    }
                }
                Some(layer_spec::Content::Messages(m)) => {
                    if m.use_opacity && !m.opacity.is_finite() {
                        return Err(CommandBusError::Invalid(
                            "messages layer opacity must be finite".into(),
                        ));
                    }
                }
                None => {}
            }
        }
    }
    Ok(())
}

fn ensure_scenes_thumbnails_dir(library: &Library) -> Result<PathBuf, CommandBusError> {
    let thumbs_dir = ensure_thumbnails_dir(&library.paths().root)?;
    let scenes_dir = thumbs_dir.join("scenes");
    match std::fs::symlink_metadata(&scenes_dir) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(CommandBusError::Invalid(
                    "scenes thumbnail directory must not be a symlink".into(),
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(&scenes_dir)
                .map_err(|e| CommandBusError::Internal(e.into()))?;
        }
        Err(e) => return Err(CommandBusError::Internal(e.into())),
    }
    let canon = scenes_dir
        .canonicalize()
        .map_err(|e| CommandBusError::Internal(e.into()))?;
    if !canon.starts_with(&thumbs_dir) {
        return Err(CommandBusError::Invalid(
            "scenes thumbnail directory escaped thumbnails root".into(),
        ));
    }
    Ok(canon)
}

fn capped_render_dimensions(target_w: u32, target_h: u32) -> (u32, u32) {
    let pixels = u64::from(target_w) * u64::from(target_h);
    if pixels <= MAX_RENDER_PIXELS {
        return (target_w, target_h);
    }
    let scale = (MAX_RENDER_PIXELS as f64 / pixels as f64).sqrt();
    let rw = ((f64::from(target_w) * scale).round() as u32).clamp(16, 3840);
    let rh = ((f64::from(target_h) * scale).round() as u32).clamp(16, 2160);
    (rw, rh)
}

fn collect_active_media_refs(scene: &Scene, output_index: u32) -> Vec<(String, String, bool)> {
    if scene.cleared {
        return Vec::new();
    }
    let Some(out) = scene
        .outputs
        .iter()
        .find(|o| o.output_index == output_index)
    else {
        return Vec::new();
    };
    if out.cleared || out.show_logo {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut refs = Vec::new();
    for layer in &out.layers {
        if !layer.enabled {
            continue;
        }
        match layer.content.as_ref() {
            Some(layer_spec::Content::Media(m)) => {
                let hash = m.content_hash.trim();
                if hash.is_empty() {
                    continue;
                }
                let ext = media_extension_for(m).to_owned();
                let is_video = m.media_kind == "video";
                if seen.insert((hash.to_owned(), ext.clone(), is_video)) {
                    refs.push((hash.to_owned(), ext, is_video));
                }
            }
            Some(layer_spec::Content::Slide(s)) if !s.elements_json.trim().is_empty() => {
                for (hash, ext, is_video) in
                    SlideElementsDocument::from_json_lenient(&s.elements_json).collect_media_refs()
                {
                    if seen.insert((hash.clone(), ext.clone(), is_video)) {
                        refs.push((hash, ext, is_video));
                    }
                }
            }
            Some(layer_spec::Content::Props(p)) if !p.elements_json.trim().is_empty() => {
                for (hash, ext, is_video) in
                    SlideElementsDocument::from_json_lenient(&p.elements_json).collect_media_refs()
                {
                    if seen.insert((hash.clone(), ext.clone(), is_video)) {
                        refs.push((hash, ext, is_video));
                    }
                }
            }
            _ => {}
        }
    }
    refs
}

fn compute_scene_flash_warning(
    library: &Library,
    scene: &Scene,
    output_index: u32,
    media_refs: &[(String, String, bool)],
) -> Result<bool, CommandBusError> {
    let mut media_flash = false;
    for (hash, _, _) in media_refs {
        if library
            .media_flash_warning_by_hash(hash)
            .map_err(CommandBusError::Internal)?
        {
            media_flash = true;
            break;
        }
    }
    if media_flash {
        return Ok(true);
    }
    if let Some(out) = scene
        .outputs
        .iter()
        .find(|o| o.output_index == output_index)
    {
        if let Some(tr) = out.transition.as_ref()
            && analyze_transition_risk(Some(tr), 0, None, Some(scene), 0.0, false).flash_warning
        {
            return Ok(true);
        }
        for layer in &out.layers {
            if layer.enabled
                && let Some(tr) = layer.transition.as_ref()
                && analyze_transition_risk(Some(tr), 0, None, Some(scene), 0.0, false).flash_warning
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn media_blob_looks_present(library: &Library, hash: &str, ext: &str) -> bool {
    let candidate = library
        .paths()
        .media
        .join(&hash[..2])
        .join(format!("{hash}.{ext}"));
    match std::fs::symlink_metadata(&candidate) {
        Ok(meta) => !meta.file_type().is_symlink() && meta.is_file(),
        Err(_) => false,
    }
}

fn try_read_valid_scene_cache(cache_path: &Path, thumb_w: u32, thumb_h: u32) -> Option<Vec<u8>> {
    let meta = match std::fs::symlink_metadata(cache_path) {
        Ok(m) => m,
        Err(_) => return None,
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        let _ = std::fs::remove_file(cache_path);
        return None;
    }
    match read_validated_png(cache_path, thumb_w, thumb_h) {
        Ok((w, h, bytes)) => {
            if w == thumb_w && h == thumb_h && decode_png(&bytes).is_ok() {
                Some(bytes)
            } else {
                let _ = std::fs::remove_file(cache_path);
                None
            }
        }
        Err(_) => {
            let _ = std::fs::remove_file(cache_path);
            None
        }
    }
}

fn load_or_extract_video_poster(
    library: &Library,
    hash: &str,
    ext: &str,
) -> Result<Nv12Frame, CommandBusError> {
    let store = library.media_store().map_err(CommandBusError::Internal)?;
    let verified_path = store
        .verified_blob_path_by_hash(hash, ext)
        .map_err(|e| CommandBusError::Invalid(format!("media blob verification failed: {e}")))?;

    let thumbs_dir = ensure_thumbnails_dir(&library.paths().root)?;
    let default_poster = thumbs_dir.join(format!("{hash}_{DEFAULT_THUMB_W}x{DEFAULT_THUMB_H}.png"));
    if let Ok((w, h, bytes)) = read_validated_png(&default_poster, 1024, 1024)
        && w > 0
        && h > 0
        && let Ok(frame) = decode_png(&bytes)
        && let Ok(nv12) = Nv12Frame::from_rgba(&frame)
    {
        return Ok(nv12);
    }

    if let Ok(entries) = std::fs::read_dir(&thumbs_dir) {
        let prefix = format!("{hash}_");
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            if name.starts_with(&prefix)
                && name.ends_with(".png")
                && let Ok((w, h, bytes)) = read_validated_png(&entry.path(), 1024, 1024)
                && w > 0
                && h > 0
                && let Ok(frame) = decode_png(&bytes)
                && let Ok(nv12) = Nv12Frame::from_rgba(&frame)
            {
                return Ok(nv12);
            }
        }
    }

    let probe_dir = tempfile::Builder::new()
        .prefix(".scene-poster-")
        .tempdir_in(&library.paths().root)
        .map_err(|e| CommandBusError::Internal(e.into()))?;
    let probe_thumb_path = probe_dir.path().join("poster.png");
    let report = run_media_worker(
        &verified_path,
        Some("video"),
        DEFAULT_THUMB_W,
        DEFAULT_THUMB_H,
        Some(&probe_thumb_path),
        false,
    )?;
    if !report.thumbnail_written {
        return Err(CommandBusError::Invalid(
            "worker did not produce a video poster frame".into(),
        ));
    }
    let (_, _, png_bytes) =
        read_validated_png(&probe_thumb_path, DEFAULT_THUMB_W, DEFAULT_THUMB_H)?;
    let poster_frame = decode_png(&png_bytes)
        .map_err(|e| CommandBusError::Invalid(format!("bad video poster PNG: {e}")))?;
    let nv12 = Nv12Frame::from_rgba(&poster_frame)
        .map_err(|e| CommandBusError::Internal(anyhow::anyhow!("poster NV12 convert: {e}")))?;
    let _ = write_atomic_file_checked(&default_poster, &png_bytes);
    Ok(nv12)
}

fn evict_scene_cache_if_needed(scenes_dir: &Path, incoming_bytes: u64) {
    let Ok(entries) = std::fs::read_dir(scenes_dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
    let mut total_bytes = 0_u64;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("png") {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let len = meta.len();
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        total_bytes = total_bytes.saturating_add(len);
        files.push((path, modified, len));
    }
    let max_files = MAX_SCENE_THUMB_FILES.saturating_sub(1);
    let max_bytes = MAX_SCENE_THUMB_BYTES.saturating_sub(incoming_bytes);
    if files.len() <= max_files && total_bytes <= max_bytes {
        return;
    }
    files.sort_by_key(|(_, modified, _)| *modified);
    let mut remaining_files = files.len();
    for (path, _, len) in files {
        if remaining_files <= max_files && total_bytes <= max_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            remaining_files = remaining_files.saturating_sub(1);
            total_bytes = total_bytes.saturating_sub(len);
        }
    }
}

/// Render an offscreen scene thumbnail (or return a validated BLAKE3 cache hit from
/// `library/thumbnails/scenes/`). Never touches any live GPU swapchain.
pub fn render_scene_thumbnail(
    library: &Library,
    live: &LiveState,
    req: &RenderSceneThumbnail,
) -> Result<SceneThumbnail, CommandBusError> {
    let Some(req_scene) = req.scene.as_ref() else {
        return Err(CommandBusError::Invalid("missing scene".into()));
    };
    validate_scene_inputs(req_scene)?;

    let matched_out = req_scene
        .outputs
        .iter()
        .find(|o| o.output_index == req.output_index);
    let cfg_res = live
        .output_configs()
        .iter()
        .find(|c| c.output_index == req.output_index)
        .and_then(|c| c.resolution);
    let target_res = matched_out
        .and_then(|o| o.resolution)
        .or(cfg_res)
        .or(req_scene.resolution);
    let (target_w, target_h) = clamp_resolution(target_res, (1920, 1080));

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
    let (thumb_w, thumb_h) = fit_dimensions(target_w, target_h, max_w, max_h);

    let canonical_scene = if let Some(out) = matched_out {
        let mut canonical_out = out.clone();
        canonical_out.resolution = Some(Resolution {
            width: target_w,
            height: target_h,
        });
        canonical_out.transition = None;
        Scene {
            revision: 0,
            test_pattern: req_scene.test_pattern,
            text: req_scene.text.clone(),
            cleared: req_scene.cleared,
            resolution: Some(Resolution {
                width: target_w,
                height: target_h,
            }),
            outputs: vec![canonical_out],
            is_preview: false,
            preload_items: Vec::new(),
        }
    } else if !req_scene.outputs.is_empty() {
        // Output-addressed scene with no entry for `output_index`: renders black cleared output.
        Scene {
            revision: 0,
            test_pattern: 0,
            text: String::new(),
            cleared: true,
            resolution: Some(Resolution {
                width: target_w,
                height: target_h,
            }),
            outputs: Vec::new(),
            is_preview: false,
            preload_items: Vec::new(),
        }
    } else {
        Scene {
            revision: 0,
            test_pattern: req_scene.test_pattern,
            text: req_scene.text.clone(),
            cleared: req_scene.cleared,
            resolution: Some(Resolution {
                width: target_w,
                height: target_h,
            }),
            outputs: Vec::new(),
            is_preview: false,
            preload_items: Vec::new(),
        }
    };

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"selah.scene_thumbnail.v1\0");
    hasher.update(&req.output_index.to_le_bytes());
    hasher.update(&target_w.to_le_bytes());
    hasher.update(&target_h.to_le_bytes());
    hasher.update(&thumb_w.to_le_bytes());
    hasher.update(&thumb_h.to_le_bytes());
    hasher.update(&canonical_scene.encode_to_vec());
    let cache_key = hasher.finalize().to_hex().to_string();

    let media_refs = collect_active_media_refs(&canonical_scene, req.output_index);
    let flash_warning =
        compute_scene_flash_warning(library, req_scene, req.output_index, &media_refs)?;

    let scenes_dir = ensure_scenes_thumbnails_dir(library)?;
    let cache_path = scenes_dir.join(format!("{cache_key}_{thumb_w}x{thumb_h}.png"));
    let all_media_present = media_refs
        .iter()
        .all(|(h, ext, _)| media_blob_looks_present(library, h, ext));

    if all_media_present
        && let Some(cached_png) = try_read_valid_scene_cache(&cache_path, thumb_w, thumb_h)
    {
        return Ok(SceneThumbnail {
            png_bytes: cached_png,
            width: thumb_w,
            height: thumb_h,
            cache_key,
            flash_warning,
        });
    }

    let mut video_posters = Vec::new();
    for (hash, ext, is_video) in &media_refs {
        if *is_video {
            let nv12 = load_or_extract_video_poster(library, hash, ext)?;
            video_posters.push((hash.clone(), nv12));
        }
    }

    let (render_w, render_h) = capped_render_dimensions(target_w, target_h);
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    thumb_worker()
        .send(ThumbRenderJob {
            media_root: library.paths().media.clone(),
            scene: canonical_scene,
            output_index: req.output_index,
            render_w,
            render_h,
            max_w,
            max_h,
            video_posters,
            reply: reply_tx,
        })
        .map_err(|_| CommandBusError::Internal(anyhow::anyhow!("thumbnail worker disconnected")))?;

    let (out_w, out_h, png_bytes) = reply_rx
        .recv()
        .map_err(|_| CommandBusError::Internal(anyhow::anyhow!("thumbnail render cancelled")))?
        .map_err(|e| CommandBusError::Invalid(format!("scene thumbnail render failed: {e}")))?;

    evict_scene_cache_if_needed(&scenes_dir, png_bytes.len() as u64);
    write_atomic_file_checked(&cache_path, &png_bytes)?;

    Ok(SceneThumbnail {
        png_bytes,
        width: out_w,
        height: out_h,
        cache_key,
        flash_warning,
    })
}
