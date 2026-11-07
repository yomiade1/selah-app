//! The macOS Keychain (SEC-05): generic passwords, service `Selah`, account = the secret's name.
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

use crate::{Backend, SecretError};

/// errSecItemNotFound.
const NOT_FOUND: i32 = -25300;
const SERVICE: &str = "Selah";

pub struct Keychain;

impl Backend for Keychain {
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        set_generic_password(SERVICE, key, value.as_bytes())
            .map_err(|error| SecretError::Keychain(error.to_string()))
    }

    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        match get_generic_password(SERVICE, key) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| SecretError::Keychain("the stored secret isn't text".into())),
            Err(error) if error.code() == NOT_FOUND => Ok(None),
            Err(error) => Err(SecretError::Keychain(error.to_string())),
        }
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        match delete_generic_password(SERVICE, key) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == NOT_FOUND => Ok(()),
            Err(error) => Err(SecretError::Keychain(error.to_string())),
        }
    }

    fn name(&self) -> &'static str {
        "macos-keychain"
    }
}
