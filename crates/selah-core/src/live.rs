//! Live state and its snapshot (`live_state.bin`, written on every change; ARCHITECTURE §2).
//!
//! If core crashes, the restarted core reloads the snapshot and re-sends the scene, so the
//! engine keeps showing the same thing.
use std::path::{Path, PathBuf};

use prost::Message;
use selah_proto::v1::{Resolution, Scene, TestPattern};

pub struct LiveState {
    path: PathBuf,
    scene: Scene,
}

/// Something other than nothing (or the logo) is on the screens (SAFE-06).
pub fn is_live(scene: &Scene) -> bool {
    !scene.cleared
        && (scene.test_pattern != TestPattern::Unspecified as i32 || !scene.text.is_empty())
}

impl LiveState {
    pub fn open(config_dir: &Path) -> Self {
        let path = config_dir.join("live_state.bin");
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
            });
        Self { path, scene }
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
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

    #[test]
    fn snapshot_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut live = LiveState::open(dir.path());
        assert!(!is_live(live.scene()));
        live.change(|scene| {
            scene.cleared = false;
            scene.test_pattern = TestPattern::SmpteBars as i32;
        });
        let reopened = LiveState::open(dir.path());
        assert_eq!(reopened.scene().revision, 1);
        assert!(is_live(reopened.scene()));
    }

    #[test]
    fn corrupt_snapshot_starts_cleared() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("live_state.bin"), [0xff, 0xff, 0xff]).unwrap();
        assert!(LiveState::open(dir.path()).scene().cleared);
    }
}
