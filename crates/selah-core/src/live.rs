//! Live state and its snapshot (`live_state.bin`, written on every change; ARCHITECTURE §2).
//!
//! If core crashes, the restarted core reloads the snapshot and re-sends the scene, so the
//! engine keeps showing the same thing.
use std::path::{Path, PathBuf};

use prost::Message;
use selah_proto::v1::{OutputConfig, OutputConfigList, OutputRole, Resolution, Scene, TestPattern};

pub struct LiveState {
    path: PathBuf,
    configs_path: PathBuf,
    scene: Scene,
    output_configs: Vec<OutputConfig>,
    explicit_configs: bool,
}

fn default_output_config() -> OutputConfig {
    OutputConfig {
        output_index: 0,
        name: "Main Output".into(),
        role: OutputRole::Main as i32,
        resolution: Some(Resolution {
            width: 1920,
            height: 1080,
        }),
        enabled: true,
        display_index: 0,
        fullscreen: false,
        refresh_rate_mhz: 60_000,
        template_id: None,
    }
}

/// Something other than nothing (or the logo) is on the screens (SAFE-06).
pub fn is_live(scene: &Scene) -> bool {
    selah_render::scene::scene_is_live(scene)
}

impl LiveState {
    pub fn open(config_dir: &Path) -> Self {
        let path = config_dir.join("live_state.bin");
        let configs_path = config_dir.join("output_configs.bin");
        let scene = std::fs::read(&path)
            .ok()
            .and_then(|bytes| Scene::decode(bytes.as_slice()).ok())
            .unwrap_or_else(|| Scene {
                revision: 0,
                test_pattern: TestPattern::Unspecified as i32,
                text: String::new(),
                cleared: true,
                resolution: Some(Resolution {
                    width: 1920,
                    height: 1080,
                }),
                ..Default::default()
            });
        let stored_configs = std::fs::read(&configs_path)
            .ok()
            .and_then(|bytes| OutputConfigList::decode(bytes.as_slice()).ok())
            .filter(|list| !list.outputs.is_empty());
        let (output_configs, explicit_configs) = match stored_configs {
            Some(list) => (list.outputs, true),
            None => (vec![default_output_config()], false),
        };
        Self {
            path,
            configs_path,
            scene,
            output_configs,
            explicit_configs,
        }
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn output_configs(&self) -> &[OutputConfig] {
        &self.output_configs
    }

    pub fn has_explicit_output_configs(&self) -> bool {
        self.explicit_configs
    }

    pub fn set_output_configs(&mut self, outputs: Vec<OutputConfig>) {
        self.output_configs = outputs;
        self.explicit_configs = true;
        let encoded = OutputConfigList {
            outputs: self.output_configs.clone(),
        }
        .encode_to_vec();
        if let Err(error) = selah_runtime::paths::write_atomic(&self.configs_path, &encoded) {
            tracing::error!(%error, "could not save output configs");
        }
    }

    /// Apply a change, bump the revision and save the snapshot before anything is sent.
    pub fn change(&mut self, edit: impl FnOnce(&mut Scene)) -> &Scene {
        let mut next = self.scene.clone();
        edit(&mut next);
        next.revision = self.scene.revision + 1;
        self.scene = next;
        self.save();
        &self.scene
    }

    /// Make sure our next revision is above `floor` (after the emergency lane acted).
    pub fn raise_revision_above(&mut self, floor: u64) {
        if self.scene.revision <= floor {
            self.scene.revision = floor;
        }
    }

    fn save(&self) {
        if let Err(error) =
            selah_runtime::paths::write_atomic(&self.path, &self.scene.encode_to_vec())
        {
            tracing::error!(%error, "could not save the live-state snapshot");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use selah_proto::v1::{LayerKind, LayerSpec, OutputScene, SlideLayerSpec, layer_spec};

    #[test]
    fn snapshot_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut live = LiveState::open(dir.path());
        assert!(!is_live(live.scene()));
        live.change(|scene| {
            scene.cleared = false;
            scene.test_pattern = TestPattern::SmpteBars as i32;
        });
        live.set_output_configs(vec![OutputConfig {
            output_index: 1,
            name: "Wide LED".into(),
            role: OutputRole::Main as i32,
            resolution: Some(Resolution {
                width: 3840,
                height: 1080,
            }),
            enabled: true,
            ..Default::default()
        }]);
        let reopened = LiveState::open(dir.path());
        assert_eq!(reopened.scene().revision, 1);
        assert!(is_live(reopened.scene()));
        assert!(reopened.has_explicit_output_configs());
        assert_eq!(reopened.output_configs()[0].name, "Wide LED");
    }

    #[test]
    fn corrupt_snapshot_starts_cleared() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("live_state.bin"), [0xff, 0xff, 0xff]).unwrap();
        assert!(LiveState::open(dir.path()).scene().cleared);
    }

    #[test]
    fn layer_scene_liveness_tracks_enabled_layers() {
        let mut scene = Scene {
            cleared: false,
            outputs: vec![OutputScene {
                output_index: 0,
                layers: vec![LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        text: "Blessed Assurance".into(),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(is_live(&scene));
        scene.outputs[0].show_logo = true;
        assert!(!is_live(&scene));
    }
}
