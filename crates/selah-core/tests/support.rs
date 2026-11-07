//! M1-A15 (SEC-05, SEC-06): Help ▸ Report a problem produces a bundle with logs and versions,
//! but no seeded secrets, email addresses or user name, even when they reached a log file.
use selah_core::settings::StoredSettings;
use selah_core::support::{BundleInput, create, read_entries};
use selah_secrets::SecretStore;

#[test]
fn a_bundle_has_logs_and_versions_but_no_secrets_or_user_name() {
    // sec: SEC-05, SEC-06
    let home = tempfile::tempdir().unwrap();
    let logs = home.path().join("logs");
    let support = home.path().join("support");
    std::fs::create_dir_all(&logs).unwrap();

    // A secret saved through SecretStore, then printed by careless code into a log file that
    // bypassed the redacting writer (the worst case).
    let secrets = SecretStore::memory();
    secrets
        .set("stream.youtube.key", "TEST-SECRET-BUNDLE-4242")
        .unwrap();
    let secret = secrets.get("stream.youtube.key").unwrap().unwrap();
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default();
    let line = format!(
        concat!(
            r#"{{"level":"INFO","message":"connect","key":"{secret}","#,
            r#""mail":"pastor.jo@example.org","path":"C:\\Users\\{user}\\Videos\\intro.mp4","#,
            r#""home":"/home/{user}/x","who":"started by {user}","password":"hunter22"}}"#
        ),
        secret = secret,
        user = user
    );
    std::fs::write(
        logs.join("core.2026-11-07.log"),
        format!("{line}\nengine ready\n"),
    )
    .unwrap();
    std::fs::write(logs.join("engine.2026-11-07.log"), "frame shown\n").unwrap();
    // A Diagnostics report from earlier is included too.
    std::fs::create_dir_all(&support).unwrap();
    std::fs::write(
        support.join("selah-diagnostics-2026-11-07T10-00-00Z.json"),
        format!(r#"{{"adapter":"x","note":"{secret}"}}"#),
    )
    .unwrap();

    let settings = StoredSettings {
        update_feed_url:
            "https://updates.example.invalid/releases.dev.json?token=TEST-SECRET-FEED-77".into(),
        allow_update_check: true,
        ..StoredSettings::default()
    };
    let path = create(&BundleInput {
        version: "0.1.0-test",
        safe_mode: false,
        settings: &settings,
        logs_dir: &logs,
        support_dir: &support,
    })
    .unwrap();
    assert!(path.starts_with(&support));
    assert!(path.extension().is_some_and(|e| e == "zip"));

    let bytes = std::fs::read(&path).unwrap();
    let entries = read_entries(&bytes).unwrap();
    let names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
    for expected in [
        "versions.json",
        "settings.json",
        "logs/core.2026-11-07.log",
        "logs/engine.2026-11-07.log",
        "diagnostics/selah-diagnostics-2026-11-07T10-00-00Z.json",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }
    let everything: String = entries
        .iter()
        .map(|(name, contents)| format!("== {name}\n{}\n", String::from_utf8_lossy(contents)))
        .collect();
    assert!(
        everything.contains("0.1.0-test"),
        "the version is in the bundle"
    );
    assert!(
        everything.contains("engine ready"),
        "the logs are in the bundle"
    );
    for leaked in [
        "TEST-SECRET-BUNDLE-4242",
        "TEST-SECRET-FEED-77",
        "pastor.jo@example.org",
        "hunter22",
    ] {
        assert!(
            !everything.contains(leaked),
            "{leaked} leaked:\n{everything}"
        );
    }
    if user.len() >= 3 {
        let lower = everything.to_lowercase();
        assert!(
            !lower.contains(&format!("\\users\\{}", user.to_lowercase()))
                && !lower.contains(&format!("/home/{}", user.to_lowercase()))
                && !lower.contains(&format!("started by {}", user.to_lowercase())),
            "the user name leaked:\n{everything}"
        );
    }
    // Nothing is left behind next to the bundle except the bundle and the earlier report.
    assert_eq!(std::fs::read_dir(&support).unwrap().count(), 2);
}
