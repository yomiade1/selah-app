//! Redaction for logs and support bundles (SEC-06).
//!
//! Removes, in order: secret values registered at runtime (from `SecretStore`), email
//! addresses, bearer tokens, `password=`/`token:`-style values, RTMP stream keys, and the
//! user's name in paths and as a word. Every log line passes through [`redact`] before it's
//! written, and support bundles are built only from redacted text.
use std::borrow::Cow;
use std::sync::{LazyLock, RwLock};

use regex::Regex;

pub const REDACTED: &str = "<redacted>";
pub const USER: &str = "<user>";

static SECRETS: RwLock<Vec<String>> = RwLock::new(Vec::new());

static RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    let rule =
        |pattern: &str, replacement| (Regex::new(pattern).expect("valid pattern"), replacement);
    vec![
        rule(
            r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}",
            "<email>",
        ),
        rule(r"(?i)(bearer\s+)[A-Za-z0-9\-._~+/]+=*", "${1}<redacted>"),
        rule(
            r#"(?i)\b(password|passwd|secret|token|api[_-]?key|stream[_-]?key|authorization|client[_-]?secret)(\\?"?\s*[:=]\s*\\?"?)([^\s"\\',;&}]+)"#,
            "${1}${2}<redacted>",
        ),
        rule(
            r"(?i)(rtmps?://[^\s/]+/[^\s/]+/)[^\s\x22\\]+",
            "${1}<redacted>",
        ),
        // C:\Users\name, C:\\Users\\name (JSON-escaped), /home/name, /Users/name
        rule(
            r"(?i)([A-Z]:(?:\\\\|\\|/)+Users(?:\\\\|\\|/)+|/home/|/Users/)[^\\/\s\x22]+",
            "${1}<user>",
        ),
    ]
});

/// Add a value that must never appear in logs or bundles. Values shorter than 4 characters
/// are ignored because they'd redact ordinary text.
pub fn register_secret(value: &str) {
    let value = value.trim();
    if value.len() < 4 {
        return;
    }
    let mut secrets = SECRETS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !secrets.iter().any(|known| known == value) {
        secrets.push(value.to_owned());
        // Longest first, so a secret that contains another is removed whole.
        secrets.sort_by_key(|known| std::cmp::Reverse(known.len()));
    }
}

fn user_name() -> Option<String> {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok()
        .filter(|name| name.len() >= 3)
}

/// Redact one piece of text.
pub fn redact(text: &str) -> Cow<'_, str> {
    let mut output: Cow<'_, str> = Cow::Borrowed(text);
    {
        let secrets = SECRETS
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for secret in secrets.iter() {
            if output.contains(secret.as_str()) {
                output = Cow::Owned(output.replace(secret.as_str(), REDACTED));
            }
        }
    }
    for (pattern, replacement) in RULES.iter() {
        if let Cow::Owned(changed) = pattern.replace_all(&output, *replacement) {
            output = Cow::Owned(changed);
        }
    }
    if let Some(name) = user_name()
        && let Ok(word) = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(&name)))
        && let Cow::Owned(changed) = word.replace_all(&output, USER)
    {
        output = Cow::Owned(changed);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_secrets_and_identifiers_are_removed() {
        // sec: SEC-06
        register_secret("TEST-SECRET-1234");
        let line = concat!(
            r#"{"msg":"connect","key":"TEST-SECRET-1234","mail":"pastor.jo@example.org","#,
            r#""auth":"Bearer abc.def-ghi","url":"rtmp://live.example.com/app/TEST-STREAM-KEY","#,
            r#""path":"C:\\Users\\Jordan\\Documents\\x.mp4","home":"/home/jordan/x","password":"hunter22"}"#
        );
        let clean = redact(line);
        for leaked in [
            "TEST-SECRET-1234",
            "pastor.jo@example.org",
            "abc.def-ghi",
            "TEST-STREAM-KEY",
            "Jordan",
            "jordan",
            "hunter22",
        ] {
            assert!(!clean.contains(leaked), "{leaked} leaked: {clean}");
        }
        assert!(clean.contains(r"C:\\Users\\<user>\\Documents"));
    }

    #[test]
    fn ordinary_text_is_unchanged() {
        let line = "engine started output 0 at 1920x1080, scene revision 3";
        assert!(matches!(redact(line), Cow::Borrowed(_)));
    }

    #[test]
    fn short_values_are_not_registered() {
        register_secret("ab");
        assert_eq!(redact("abba"), "abba");
    }
}
