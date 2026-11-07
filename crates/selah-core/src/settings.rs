//! The settings store: `settings.json` in the config folder, written atomically.
//!
//! Secrets never live here (SEC-05); they go through `SecretStore`.
use std::path::{Path, PathBuf};

use selah_proto::v1::{Appearance, AppearanceSettings, Settings};
use serde::{Deserialize, Serialize};

/// Text size steps (ADR-009).
pub const TEXT_SCALE_STEPS: [u32; 7] = [90, 100, 115, 130, 150, 175, 200];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("text size {0}% is not one of the allowed steps")]
    TextScale(u32),
    #[error("appearance is not set")]
    Appearance,
    #[error("the update feed must be an https:// address")]
    FeedUrl,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StoredSettings {
    pub appearance: String,
    pub text_scale_percent: u32,
    pub use_computer_text_size: bool,
    /// Empty means "no feed configured": the updater does nothing (SEC-12, SEC-13).
    pub update_feed_url: String,
    /// Off by default: Selah makes no outgoing connections at default settings (SEC-13).
    pub allow_update_check: bool,
}

impl Default for StoredSettings {
    fn default() -> Self {
        Self {
            appearance: "automatic".into(),
            text_scale_percent: 100,
            use_computer_text_size: false,
            update_feed_url: String::new(),
            allow_update_check: false,
        }
    }
}

fn appearance_name(value: Appearance) -> Option<&'static str> {
    match value {
        Appearance::Light => Some("light"),
        Appearance::Booth => Some("booth"),
        Appearance::Automatic => Some("automatic"),
        Appearance::Unspecified => None,
    }
}

fn appearance_value(name: &str) -> Appearance {
    match name {
        "light" => Appearance::Light,
        "booth" => Appearance::Booth,
        _ => Appearance::Automatic,
    }
}

impl StoredSettings {
    pub fn to_proto(&self) -> Settings {
        Settings {
            appearance: Some(AppearanceSettings {
                appearance: appearance_value(&self.appearance) as i32,
                text_scale_percent: self.text_scale_percent,
                use_computer_text_size: self.use_computer_text_size,
            }),
            update_feed_url: self.update_feed_url.clone(),
            allow_update_check: self.allow_update_check,
        }
    }

    /// Validate and apply an appearance change.
    pub fn apply_appearance(&mut self, change: &AppearanceSettings) -> Result<(), SettingsError> {
        if !TEXT_SCALE_STEPS.contains(&change.text_scale_percent) {
            return Err(SettingsError::TextScale(change.text_scale_percent));
        }
        let appearance = Appearance::try_from(change.appearance)
            .ok()
            .and_then(appearance_name)
            .ok_or(SettingsError::Appearance)?;
        self.appearance = appearance.into();
        self.text_scale_percent = change.text_scale_percent;
        self.use_computer_text_size = change.use_computer_text_size;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        if !TEXT_SCALE_STEPS.contains(&self.text_scale_percent) {
            return Err(SettingsError::TextScale(self.text_scale_percent));
        }
        if !self.update_feed_url.is_empty() && !self.update_feed_url.starts_with("https://") {
            return Err(SettingsError::FeedUrl);
        }
        Ok(())
    }
}

pub struct SettingsStore {
    path: PathBuf,
    current: StoredSettings,
}

impl SettingsStore {
    /// Load `settings.json`, falling back to defaults if it's missing or invalid (the bad file
    /// is kept as `settings.json.bad` for support).
    pub fn open(config_dir: &Path) -> Self {
        let path = config_dir.join("settings.json");
        let current = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<StoredSettings>(&bytes) {
                Ok(settings) if settings.validate().is_ok() => settings,
                _ => {
                    tracing::warn!("settings file was invalid; using defaults");
                    let _ = std::fs::rename(&path, path.with_extension("json.bad"));
                    StoredSettings::default()
                }
            },
            Err(_) => StoredSettings::default(),
        };
        Self { path, current }
    }

    pub fn get(&self) -> &StoredSettings {
        &self.current
    }

    pub fn update(
        &mut self,
        change: impl FnOnce(&mut StoredSettings) -> Result<(), SettingsError>,
    ) -> Result<(), SettingsError> {
        let mut next = self.current.clone();
        change(&mut next)?;
        next.validate()?;
        self.current = next;
        if let Err(error) = self.save() {
            tracing::error!(%error, "could not save settings");
        }
        Ok(())
    }

    fn save(&self) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.current).map_err(std::io::Error::other)?;
        selah_runtime::paths::write_atomic(&self.path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_private_and_offline() {
        // sec: SEC-13 (no update checks or feed by default)
        let settings = StoredSettings::default();
        assert!(!settings.allow_update_check);
        assert!(settings.update_feed_url.is_empty());
        assert_eq!(settings.text_scale_percent, 100);
    }

    #[test]
    fn only_the_seven_text_size_steps_are_accepted() {
        let mut settings = StoredSettings::default();
        for scale in [0, 89, 95, 201, 1000] {
            let change = AppearanceSettings {
                appearance: Appearance::Light as i32,
                text_scale_percent: scale,
                use_computer_text_size: false,
            };
            assert_eq!(
                settings.apply_appearance(&change),
                Err(SettingsError::TextScale(scale))
            );
        }
        for scale in TEXT_SCALE_STEPS {
            let change = AppearanceSettings {
                appearance: Appearance::Booth as i32,
                text_scale_percent: scale,
                use_computer_text_size: true,
            };
            settings.apply_appearance(&change).unwrap();
        }
        assert_eq!(settings.appearance, "booth");
    }

    #[test]
    fn settings_survive_a_restart_and_bad_files_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::open(dir.path());
        store
            .update(|s| {
                s.text_scale_percent = 150;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            SettingsStore::open(dir.path()).get().text_scale_percent,
            150
        );
        std::fs::write(dir.path().join("settings.json"), b"{not json").unwrap();
        assert_eq!(
            SettingsStore::open(dir.path()).get(),
            &StoredSettings::default()
        );
        assert!(dir.path().join("settings.json.bad").exists());
    }

    #[test]
    fn plain_http_feed_is_refused() {
        // sec: SEC-12
        let settings = StoredSettings {
            update_feed_url: "http://example.invalid/feed".into(),
            ..StoredSettings::default()
        };
        assert_eq!(settings.validate(), Err(SettingsError::FeedUrl));
    }
}
