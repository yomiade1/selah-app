//! Synchronized background video playback manager for `selah-engine` (M3.4, SAFE-05, SEC-04).
//!
//! Opening containers and pre-buffering initial video/audio frames happens asynchronously on
//! short-lived opener workers so `SetScene` and SAFE-03 `emergency` clears NEVER stall the render
//! thread.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use selah_media::DecodeMode;
use selah_media::audio::DEFAULT_SAFE_VOLUME;
use selah_media::player::{MediaPlayer, PlaybackConfig, PlaybackOutputMode};
use selah_proto::v1::{MediaLayerSpec, Scene, layer_spec};
use selah_render::Compositor;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct StructuralKey {
    hash: String,
    extension: String,
    in_ms: u64,
    out_ms: Option<u64>,
    loop_media: bool,
}

struct ActiveStream {
    structural: StructuralKey,
    player: MediaPlayer,
    last_seek_ms: u64,
    last_seq: Option<u64>,
}

struct OpenedResult {
    open_id: u64,
    structural: StructuralKey,
    start_seek_ms: u64,
    result: std::result::Result<MediaPlayer, String>,
}

/// Manages live [`MediaPlayer`] instances keyed by media content hash across all outputs.
pub(crate) struct MediaPlaybackManager {
    active: HashMap<String, ActiveStream>,
    pending: HashMap<String, (u64, StructuralKey)>,
    next_open_id: u64,
    opened_tx: mpsc::Sender<OpenedResult>,
    opened_rx: mpsc::Receiver<OpenedResult>,
    on_ready: Arc<dyn Fn() + Send + Sync>,
    failed: HashSet<StructuralKey>,
    fading: Vec<(MediaPlayer, Instant)>,
}

impl MediaPlaybackManager {
    pub(crate) fn new(on_ready: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (opened_tx, opened_rx) = mpsc::channel();
        Self {
            active: HashMap::new(),
            pending: HashMap::new(),
            next_open_id: 1,
            opened_tx,
            opened_rx,
            on_ready,
            failed: HashSet::new(),
            fading: Vec::new(),
        }
    }

    /// Reconcile active video layers in `scene` against running players, spawn non-blocking
    /// openers for new clips, apply parameter updates in-place, fade deactivated streams over
    /// 25 ms (SAFE-05), upload newly due NV12 frames, and return whether compositor frames changed.
    pub(crate) fn sync_and_push(
        &mut self,
        scene: &Scene,
        media_root: &Path,
        decode_mode: DecodeMode,
        compositor: &mut dyn Compositor,
    ) -> bool {
        self.retire_finished_fades();

        let desired = collect_video_specs(scene);
        let mut changed = false;

        // Drain any finished async opens first, discarding obsolete/superseded ones immediately.
        while let Ok(opened) = self.opened_rx.try_recv() {
            let hash = opened.structural.hash.clone();
            let still_wanted = self
                .pending
                .get(&hash)
                .is_some_and(|(id, _)| *id == opened.open_id)
                && desired
                    .get(&hash)
                    .is_some_and(|spec| structural_key(&hash, spec) == opened.structural);
            if still_wanted {
                self.pending.remove(&hash);
                match opened.result {
                    Ok(player) => {
                        if let Some(spec) = desired.get(&hash) {
                            player.set_paused(spec.paused);
                            player.set_speed(effective_speed(spec));
                            player.set_volume(effective_volume(spec));
                            player.set_muted(spec.muted);
                            if spec.seek_ms != opened.start_seek_ms {
                                player.seek_ms(spec.seek_ms);
                            }
                            self.active.insert(
                                hash,
                                ActiveStream {
                                    structural: opened.structural,
                                    player,
                                    last_seek_ms: spec.seek_ms,
                                    last_seq: None,
                                },
                            );
                        } else {
                            player.fade_out_and_stop(1);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(hash, %err, "background video open failed");
                        self.failed.insert(opened.structural);
                    }
                }
            } else if let Ok(player) = opened.result {
                player.fade_out_and_stop(1);
            }
        }

        if desired.is_empty() && self.active.is_empty() && self.pending.is_empty() {
            if scene.cleared
                && let Err(error) = compositor.set_video(None)
            {
                tracing::warn!(%error, "failed clearing legacy video frame");
            }
            return false;
        }

        // Remove failed & pending markers for clips no longer requested (or structurally changed).
        self.failed.retain(|k| desired.contains_key(&k.hash));
        self.pending.retain(|hash, (_, key)| {
            desired
                .get(hash)
                .is_some_and(|spec| structural_key(hash, spec) == *key)
        });

        // Detect removed streams or streams whose structural bounds/extension changed.
        let mut to_stop = Vec::new();
        for (hash, stream) in &self.active {
            match desired.get(hash) {
                Some(spec) => {
                    let wanted_key = structural_key(hash, spec);
                    if wanted_key != stream.structural {
                        to_stop.push(hash.clone());
                    }
                }
                None => to_stop.push(hash.clone()),
            }
        }

        for hash in to_stop {
            changed = true;
            if let Err(error) = compositor.set_video_for_hash(&hash, None) {
                tracing::warn!(hash, %error, "failed clearing video frame from compositor");
            }
            if let Some(stream) = self.active.remove(&hash) {
                stream.player.fade_out_and_stop(25);
                self.fading.push((stream.player, Instant::now()));
            }
        }

        if scene.cleared
            && let Err(error) = compositor.set_video(None)
        {
            tracing::warn!(%error, "failed clearing legacy video frame");
        }

        // Open new streams asynchronously or apply in-place playback updates on existing streams.
        for (hash, spec) in desired {
            let speed = effective_speed(&spec);
            let volume = effective_volume(&spec);
            let structural = structural_key(&hash, &spec);

            if let Some(existing) = self.active.get_mut(&hash) {
                existing.player.set_paused(spec.paused);
                existing.player.set_speed(speed);
                existing.player.set_volume(volume);
                existing.player.set_muted(spec.muted);
                if spec.seek_ms != existing.last_seek_ms {
                    existing.player.seek_ms(spec.seek_ms);
                    existing.last_seek_ms = spec.seek_ms;
                    existing.last_seq = None;
                }
                continue;
            }

            if self.pending.contains_key(&hash) || self.failed.contains(&structural) {
                continue;
            }

            match selah_render::media_bg::resolve_verified_media_path(
                media_root,
                &structural.hash,
                &structural.extension,
            ) {
                Ok(path) => {
                    let open_id = self.next_open_id;
                    self.next_open_id = self.next_open_id.saturating_add(1);
                    self.pending
                        .insert(hash.clone(), (open_id, structural.clone()));

                    let config = PlaybackConfig {
                        path,
                        decode_mode,
                        in_ms: structural.in_ms,
                        out_ms: structural.out_ms,
                        loop_media: structural.loop_media,
                        speed,
                        volume,
                        muted: spec.muted,
                        paused: spec.paused,
                        start_position_ms: spec.seek_ms,
                    };
                    let start_seek_ms = spec.seek_ms;
                    let tx = self.opened_tx.clone();
                    let ready = self.on_ready.clone();
                    let spawn_res = std::thread::Builder::new()
                        .name("selah-media-open".into())
                        .spawn(move || {
                            let result =
                                MediaPlayer::open(config, PlaybackOutputMode::DefaultDevice)
                                    .map_err(|e| e.to_string());
                            let _ = tx.send(OpenedResult {
                                open_id,
                                structural,
                                start_seek_ms,
                                result,
                            });
                            ready();
                        });
                    if let Err(err) = spawn_res {
                        tracing::warn!(%err, "failed spawning background media opener");
                        self.pending.remove(&hash);
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        hash = %structural.hash,
                        %error,
                        "media store path validation failed for background video"
                    );
                    self.failed.insert(structural);
                }
            }
        }

        // Upload only frames that changed since the previous render tick.
        for stream in self.active.values_mut() {
            if let Some(frame) = stream.player.frame_if_changed(&mut stream.last_seq) {
                if let Err(error) =
                    compositor.set_video_for_hash(&stream.structural.hash, Some(frame.nv12))
                {
                    tracing::warn!(
                        hash = %stream.structural.hash,
                        %error,
                        "failed uploading video frame"
                    );
                } else {
                    changed = true;
                }
            }
        }
        changed
    }

    pub(crate) fn active_stream_count(&self) -> usize {
        self.active.len()
    }

    fn retire_finished_fades(&mut self) {
        self.fading.retain(|(player, started)| {
            !player.is_finished() && started.elapsed() < Duration::from_millis(250)
        });
    }
}

impl Drop for MediaPlaybackManager {
    fn drop(&mut self) {
        for (_, stream) in self.active.drain() {
            stream.player.fade_out_and_stop(10);
        }
        self.fading.clear();
    }
}

fn collect_video_specs(scene: &Scene) -> BTreeMap<String, MediaLayerSpec> {
    let mut specs = BTreeMap::new();
    if scene.cleared {
        return specs;
    }
    for output in &scene.outputs {
        if output.cleared || output.show_logo {
            continue;
        }
        for layer in &output.layers {
            if !layer.enabled || selah_render::scene::layer_opacity(layer) <= 0.0 {
                continue;
            }
            if let Some(layer_spec::Content::Media(media)) = layer.content.as_ref()
                && media.media_kind == "video"
            {
                let hash = media.content_hash.trim().to_ascii_lowercase();
                if !hash.is_empty() {
                    specs.entry(hash).or_insert_with(|| media.clone());
                }
            }
        }
    }
    specs
}

fn structural_key(hash: &str, spec: &MediaLayerSpec) -> StructuralKey {
    let ext = spec.media_extension.trim().to_ascii_lowercase();
    let extension = if ext.is_empty() {
        "mp4".to_owned()
    } else {
        ext
    };
    StructuralKey {
        hash: hash.to_owned(),
        extension,
        in_ms: spec.in_ms,
        out_ms: (spec.out_ms > spec.in_ms).then_some(spec.out_ms),
        loop_media: spec.loop_media,
    }
}

fn effective_speed(spec: &MediaLayerSpec) -> f32 {
    if spec.speed > 0.0 && spec.speed.is_finite() {
        spec.speed.clamp(0.1, 4.0)
    } else {
        1.0
    }
}

fn effective_volume(spec: &MediaLayerSpec) -> f32 {
    if spec.volume > 0.0 && spec.volume.is_finite() {
        spec.volume.clamp(0.0, 2.0)
    } else {
        DEFAULT_SAFE_VOLUME
    }
}

pub(crate) fn resolve_media_root() -> PathBuf {
    if let Some(root) = selah_render::media_bg::default_media_root_from_env() {
        return root;
    }
    selah_runtime::paths::Paths::from_env()
        .library
        .join("media")
}

pub(crate) fn default_decode_mode() -> DecodeMode {
    if std::env::var_os("SELAH_GPU").is_some_and(|v| v == "software") || selah_runtime::safe_mode()
    {
        DecodeMode::Software
    } else {
        DecodeMode::Auto
    }
}
