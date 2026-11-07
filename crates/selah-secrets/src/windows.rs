#![allow(unsafe_code)]
//! Windows Credential Manager (SEC-05). Generic credentials, stored per user and protected by
//! DPAPI. Targets are `Selah/<key>`. There's no safe Rust API for these calls.
use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};

use crate::{Backend, SecretError};

pub struct CredentialManager;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn target(key: &str) -> Vec<u16> {
    wide(&format!("Selah/{key}"))
}

fn last_error(what: &str) -> SecretError {
    // SAFETY: GetLastError reads this thread's last-error value; it has no preconditions.
    let code = unsafe { GetLastError() };
    SecretError::Keychain(format!("{what} failed ({code})"))
}

impl Backend for CredentialManager {
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        let mut target = target(key);
        let mut user = wide("Selah");
        let mut blob = value.as_bytes().to_vec();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            Comment: std::ptr::null_mut(),
            LastWritten: Default::default(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            // Per user, kept across sign-ins on this computer (not roamed).
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: user.as_mut_ptr(),
        };
        // SAFETY: every pointer in `credential` points into a live, NUL-terminated (strings) or
        // correctly sized (blob) buffer owned by this function for the duration of the call.
        let ok = unsafe { CredWriteW(&credential, 0) };
        blob.fill(0);
        if ok == 0 {
            return Err(last_error("CredWriteW"));
        }
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        let target = target(key);
        let mut found: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `target` is NUL-terminated and outlives the call; `found` receives a buffer
        // that Windows allocates and that we free with CredFree below.
        let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) };
        if ok == 0 {
            // SAFETY: as above; reads this thread's last-error value.
            if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(last_error("CredReadW"));
        }
        // SAFETY: CredReadW succeeded, so `found` points to a valid CREDENTIALW whose blob is
        // `CredentialBlobSize` bytes long (or null when empty).
        let bytes = unsafe {
            let credential = &*found;
            if credential.CredentialBlob.is_null() || credential.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
                .to_vec()
            }
        };
        // SAFETY: `found` came from CredReadW and is freed exactly once.
        unsafe { CredFree(found as *const _) };
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| SecretError::Keychain("the stored secret isn't text".into()))
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        let target = target(key);
        // SAFETY: `target` is NUL-terminated and outlives the call.
        let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        // SAFETY: reads this thread's last-error value.
        if ok == 0 && unsafe { GetLastError() } != ERROR_NOT_FOUND {
            return Err(last_error("CredDeleteW"));
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "windows-credential-manager"
    }
}
