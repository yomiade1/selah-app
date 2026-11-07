#![deny(unsafe_code)]
//! `SecretStore`: the one place secrets live (SEC-05).
//!
//! Stream keys, partner tokens and sign-in sessions are kept only in the operating system's
//! keychain: Windows Credential Manager (protected by DPAPI) or the macOS Keychain. Linux is a
//! development platform (ADR-023) and tests use an in-memory store.
//!
//! Every value that passes through the store, in or out, is registered with the log redactor
//! (`selah_runtime::redact`), so it can't appear in logs or support bundles (SEC-06) even if
//! some code tries to print it. Secrets never go into SQLite, settings files, the op log,
//! exports or crash dumps; the seeded-secret tests check those outputs.
use std::collections::HashMap;
use std::sync::Mutex;

#[cfg(target_os = "macos")]
mod keychain;
#[cfg(windows)]
mod windows;

/// The longest secret Selah stores (a sign-in session is the largest, well under this).
pub const MAX_SECRET_BYTES: usize = 2048;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecretError {
    #[error("the secret name isn't valid")]
    BadKey,
    #[error("the secret is too large")]
    TooLarge,
    #[error("the keychain refused: {0}")]
    Keychain(String),
}

/// A keychain. Implementations only move bytes; [`SecretStore`] adds the checks and redaction.
pub trait Backend: Send + Sync {
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError>;
    fn get(&self, key: &str) -> Result<Option<String>, SecretError>;
    fn delete(&self, key: &str) -> Result<(), SecretError>;
    fn name(&self) -> &'static str;
}

/// For tests and Linux development. Nothing is written to disk.
#[derive(Default)]
pub struct MemoryBackend {
    values: Mutex<HashMap<String, String>>,
}

impl Backend for MemoryBackend {
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.values
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        Ok(self
            .values
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(key)
            .cloned())
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        self.values
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(key);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "memory"
    }
}

/// Names look like `stream.youtube.key` or `ccli.token`: lower-case letters, digits, dots,
/// dashes and underscores, at most 100 characters.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 100
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

/// The secret store every part of Selah uses.
pub struct SecretStore {
    backend: Box<dyn Backend>,
}

impl SecretStore {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self { backend }
    }

    /// In memory only (tests).
    pub fn memory() -> Self {
        Self::new(Box::new(MemoryBackend::default()))
    }

    /// The operating system's keychain: Credential Manager on Windows, the Keychain on macOS,
    /// memory elsewhere (development only).
    pub fn platform() -> Self {
        #[cfg(windows)]
        return Self::new(Box::new(windows::CredentialManager));
        #[cfg(target_os = "macos")]
        return Self::new(Box::new(keychain::Keychain));
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            tracing::warn!("no keychain on this platform; secrets are kept in memory only");
            Self::memory()
        }
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        if !valid_key(key) {
            return Err(SecretError::BadKey);
        }
        if value.len() > MAX_SECRET_BYTES {
            return Err(SecretError::TooLarge);
        }
        selah_runtime::redact::register_secret(value);
        self.backend.set(key, value)?;
        tracing::info!(key, backend = self.backend.name(), "secret saved");
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        if !valid_key(key) {
            return Err(SecretError::BadKey);
        }
        let value = self.backend.get(key)?;
        if let Some(value) = &value {
            selah_runtime::redact::register_secret(value);
        }
        Ok(value)
    }

    pub fn delete(&self, key: &str) -> Result<(), SecretError> {
        if !valid_key(key) {
            return Err(SecretError::BadKey);
        }
        self.backend.delete(key)?;
        tracing::info!(key, backend = self.backend.name(), "secret removed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_and_are_redacted_from_logs() {
        // sec: SEC-05, SEC-06
        let store = SecretStore::memory();
        store
            .set("stream.test.key", "TEST-SECRET-STORE-5521")
            .unwrap();
        assert_eq!(
            store.get("stream.test.key").unwrap().as_deref(),
            Some("TEST-SECRET-STORE-5521")
        );
        let line = format!(
            "connecting with {:?}",
            store.get("stream.test.key").unwrap()
        );
        assert!(!selah_runtime::redact::redact(&line).contains("TEST-SECRET-STORE-5521"));
        store.delete("stream.test.key").unwrap();
        assert_eq!(store.get("stream.test.key").unwrap(), None);
    }

    #[test]
    fn names_and_sizes_are_checked() {
        let store = SecretStore::memory();
        for bad in ["", "Upper", "a/b", "..\\x", "with space", &"x".repeat(101)] {
            assert_eq!(store.set(bad, "v"), Err(SecretError::BadKey), "{bad}");
        }
        assert_eq!(
            store.set("big", &"x".repeat(MAX_SECRET_BYTES + 1)),
            Err(SecretError::TooLarge)
        );
    }

    /// The real keychain on this computer: save, read back, delete. Uses a random name so
    /// parallel runs don't collide, and always cleans up.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn the_platform_keychain_round_trips() {
        // sec: SEC-05
        let store = SecretStore::platform();
        let key = format!(
            "selah-test.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        store.set(&key, "TEST-SECRET-KEYCHAIN-0001").unwrap();
        let read = store.get(&key);
        let deleted = store.delete(&key);
        assert_eq!(read.unwrap().as_deref(), Some("TEST-SECRET-KEYCHAIN-0001"));
        deleted.unwrap();
        assert_eq!(store.get(&key).unwrap(), None);
        // Deleting what isn't there is fine.
        store.delete(&key).unwrap();
    }
}
