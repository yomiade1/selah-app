//! The updater (M1.11, SEC-12, SAFE-06, SEC-13).
//!
//! - It runs only when the person asks (**Check for updates**), and only if they turned update
//!   checks on and gave a feed address in Settings ▸ Updates. Nothing is built in: no address,
//!   no token.
//! - The feed is a Velopack `releases.<channel>.json` file at an `https://` address. It's parsed
//!   with limits; the package file name must be a plain `.nupkg` name next to the feed.
//! - The package is downloaded with a size limit and its SHA-256 must match the feed before
//!   anything is installed.
//! - Nothing installs while something is on the screens. Core refuses, and checks again just
//!   before handing the verified package to the installer.
//!
//! The network is reached only through `selah-net` (SEC-13). Core allows its update purposes
//! only while update checks are on ([`policy_for`]).
use std::path::{Path, PathBuf};

use selah_net::{Net, Policy, Purpose};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// A feed is a small JSON file.
pub const FEED_LIMIT: u64 = 1024 * 1024;
/// No Selah package is anywhere near this.
pub const PACKAGE_LIMIT: u64 = 1024 * 1024 * 1024;
/// More releases than any channel will ever list.
pub const MAX_ASSETS: usize = 2000;

/// Error catalogue codes (content/errors/errors.yaml).
pub mod codes {
    /// Update checks are off, or there's no feed address.
    pub const OFF: &str = "SEL-UPD-0001";
    /// The feed couldn't be reached or read.
    pub const FEED: &str = "SEL-UPD-0002";
    /// The download didn't match the feed's hash or size.
    pub const VERIFY: &str = "SEL-UPD-0003";
    /// Something is on the screens.
    pub const LIVE: &str = "SEL-UPD-0004";
    /// The installer couldn't be started.
    pub const INSTALL: &str = "SEL-UPD-0005";
}

/// What went wrong, mapped to a catalogue code.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UpdateError {
    #[error("update checks are off")]
    Off,
    #[error("the update feed couldn't be read: {0}")]
    Feed(String),
    #[error("the download didn't match the feed: {0}")]
    Verify(String),
    #[error("something is on the screens")]
    Live,
    #[error("the installer couldn't start: {0}")]
    Install(String),
}

impl UpdateError {
    pub fn code(&self) -> &'static str {
        match self {
            UpdateError::Off => codes::OFF,
            UpdateError::Feed(_) => codes::FEED,
            UpdateError::Verify(_) => codes::VERIFY,
            UpdateError::Live => codes::LIVE,
            UpdateError::Install(_) => codes::INSTALL,
        }
    }

    pub fn message_key(&self) -> &'static str {
        match self {
            UpdateError::Off => "error-update-off",
            UpdateError::Feed(_) => "error-update-feed",
            UpdateError::Verify(_) => "error-update-verify",
            UpdateError::Live => "error-update-live",
            UpdateError::Install(_) => "error-update-install",
        }
    }
}

/// The network policy for these settings: the update purposes only while the person allows
/// update checks and has given a feed (SEC-13). Everything else stays off.
pub fn policy_for(settings: &crate::settings::StoredSettings) -> Policy {
    if settings.allow_update_check && !settings.update_feed_url.is_empty() {
        Policy::default()
            .allow(Purpose::UpdateCheck)
            .allow(Purpose::UpdateDownload)
    } else {
        Policy::default()
    }
}

/// Starts the platform installer with a verified package.
pub trait Install: Send + Sync {
    fn install(&self, package: &Path) -> Result<(), String>;
}

/// One full release from the feed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: semver::Version,
    pub file_name: String,
    pub sha256: [u8; 32],
    pub size: u64,
    pub notes_markdown: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Feed {
    #[serde(default)]
    assets: Vec<FeedAsset>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FeedAsset {
    #[serde(default)]
    package_id: String,
    version: String,
    #[serde(rename = "Type", default)]
    kind: String,
    file_name: String,
    #[serde(rename = "SHA256", default)]
    sha256: String,
    size: u64,
    #[serde(default)]
    notes_markdown: String,
}

/// The feed address must be https:// and end in a `.json` file.
pub fn validate_feed_url(url: &str) -> Result<(), UpdateError> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| UpdateError::Feed("the address must start with https://".into()))?;
    let Some((host, path)) = rest.split_once('/') else {
        return Err(UpdateError::Feed("the address has no file".into()));
    };
    if host.is_empty()
        || host.contains('@')
        || url.len() > 2048
        || url.chars().any(char::is_whitespace)
    {
        return Err(UpdateError::Feed("the address isn't valid".into()));
    }
    let file = path.split(['?', '#']).next().unwrap_or_default();
    if !file.ends_with(".json") {
        return Err(UpdateError::Feed(
            "the address must point to a .json feed".into(),
        ));
    }
    Ok(())
}

fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name.ends_with(".nupkg")
        && !name.starts_with('.')
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn parse_hex(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Parse a Velopack feed. Only full packages for `package_id` with a valid name, hash and size
/// are kept; anything else is ignored.
pub fn parse_feed(bytes: &[u8], package_id: &str) -> Result<Vec<Release>, UpdateError> {
    if bytes.len() as u64 > FEED_LIMIT {
        return Err(UpdateError::Feed("the feed is too large".into()));
    }
    let feed: Feed = serde_json::from_slice(bytes)
        .map_err(|_| UpdateError::Feed("the feed isn't valid JSON".into()))?;
    if feed.assets.len() > MAX_ASSETS {
        return Err(UpdateError::Feed("the feed lists too many releases".into()));
    }
    Ok(feed
        .assets
        .into_iter()
        .filter(|asset| asset.kind.eq_ignore_ascii_case("full"))
        .filter(|asset| asset.package_id.is_empty() || asset.package_id == package_id)
        .filter(|asset| valid_file_name(&asset.file_name))
        .filter(|asset| asset.size > 0 && asset.size <= PACKAGE_LIMIT)
        .filter_map(|asset| {
            Some(Release {
                version: semver::Version::parse(&asset.version).ok()?,
                sha256: parse_hex(&asset.sha256)?,
                file_name: asset.file_name,
                size: asset.size,
                notes_markdown: asset.notes_markdown.chars().take(20_000).collect(),
            })
        })
        .collect())
}

/// The newest release above `current`, if any.
pub fn newest(releases: &[Release], current: &str) -> Option<Release> {
    let current = semver::Version::parse(current).ok()?;
    releases
        .iter()
        .filter(|release| release.version > current)
        .max_by(|a, b| a.version.cmp(&b.version))
        .cloned()
}

/// The package sits next to the feed.
pub fn package_url(feed_url: &str, file_name: &str) -> String {
    let base = feed_url.split(['?', '#']).next().unwrap_or(feed_url);
    match base.rfind('/') {
        Some(slash) => format!("{}/{}", &base[..slash], file_name),
        None => file_name.to_owned(),
    }
}

/// Look for a newer release. Runs only when the person asks.
pub fn check(
    net: &Net,
    feed_url: &str,
    package_id: &str,
    current: &str,
) -> Result<Option<Release>, UpdateError> {
    validate_feed_url(feed_url)?;
    let bytes = net
        .get(Purpose::UpdateCheck, feed_url, FEED_LIMIT)
        .map_err(|error| UpdateError::Feed(error.to_string()))?;
    let releases = parse_feed(&bytes, package_id)?;
    Ok(newest(&releases, current))
}

/// Check a downloaded package against the feed.
pub fn verify(bytes: &[u8], release: &Release) -> Result<(), UpdateError> {
    if bytes.len() as u64 != release.size {
        return Err(UpdateError::Verify(format!(
            "expected {} bytes, got {}",
            release.size,
            bytes.len()
        )));
    }
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    if digest != release.sha256 {
        return Err(UpdateError::Verify("the SHA-256 hash is different".into()));
    }
    Ok(())
}

/// Download and verify a release into `folder`. Returns the verified file.
pub fn download(
    net: &Net,
    feed_url: &str,
    release: &Release,
    folder: &Path,
) -> Result<PathBuf, UpdateError> {
    validate_feed_url(feed_url)?;
    let url = package_url(feed_url, &release.file_name);
    let bytes = net
        .get(Purpose::UpdateDownload, &url, release.size)
        .map_err(|error| UpdateError::Feed(error.to_string()))?;
    verify(&bytes, release)?;
    let path = folder.join(&release.file_name);
    selah_runtime::paths::write_atomic(&path, &bytes)
        .map_err(|error| UpdateError::Install(error.to_string()))?;
    Ok(path)
}

/// The installed package's id (tools/package.ps1).
pub const PACKAGE_ID: &str = "SelahApp";

/// Velopack's installer: `Update.exe` one folder above the running programs (`current/`).
/// It closes Selah, swaps the version in, and starts Selah again.
pub struct VelopackInstaller {
    pub update_exe: PathBuf,
}

impl VelopackInstaller {
    /// The installer for this installation, if Selah was installed by Setup.exe.
    pub fn for_this_installation() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        let update_exe = exe
            .parent()?
            .parent()?
            .join(format!("Update{}", std::env::consts::EXE_SUFFIX));
        update_exe.is_file().then_some(Self { update_exe })
    }
}

impl Install for VelopackInstaller {
    fn install(&self, package: &Path) -> Result<(), String> {
        // `apply` stops the programs running from the install folder, swaps the version in
        // and starts Selah again.
        std::process::Command::new(&self.update_exe)
            .arg("apply")
            .arg("--silent")
            .arg("--package")
            .arg(package)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    type Calls = std::sync::Arc<Mutex<Vec<(String, u64)>>>;

    struct FakeTransport {
        responses: Vec<(String, Vec<u8>)>,
        calls: Calls,
    }

    impl selah_net::Transport for FakeTransport {
        fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, selah_net::NetError> {
            self.calls.lock().unwrap().push((url.to_owned(), limit));
            self.responses
                .iter()
                .find(|(u, _)| u == url)
                .map(|(_, body)| body.clone())
                .ok_or_else(|| selah_net::NetError::Failed("not found".into()))
        }
    }

    /// A gate that allows the update purposes, over a fake transport.
    fn net(responses: Vec<(String, Vec<u8>)>) -> (Net, Calls) {
        let calls = Calls::default();
        let net = Net::new(Box::new(FakeTransport {
            responses,
            calls: calls.clone(),
        }));
        net.set_policy(
            Policy::default()
                .allow(Purpose::UpdateCheck)
                .allow(Purpose::UpdateDownload),
        );
        (net, calls)
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn feed(entries: &[(&str, &str, &[u8])]) -> Vec<u8> {
        let assets: Vec<String> = entries
            .iter()
            .map(|(version, kind, package)| {
                format!(
                    r#"{{"PackageId":"SelahApp","Version":"{version}","Type":"{kind}","FileName":"SelahApp-{version}-{}.nupkg","SHA1":"x","SHA256":"{}","Size":{},"NotesMarkdown":"Notes for {version}"}}"#,
                    kind.to_lowercase(),
                    hex(&Sha256::digest(package)),
                    package.len()
                )
            })
            .collect();
        format!(r#"{{"Assets":[{}]}}"#, assets.join(",")).into_bytes()
    }

    const FEED_URL: &str = "https://updates.example.invalid/selah/releases.dev.json";

    #[test]
    fn the_newest_full_release_above_the_current_version_is_offered() {
        let bytes = feed(&[
            ("0.0.3", "Full", b"three"),
            ("0.0.5", "Delta", b"five-delta"),
            ("0.0.4", "Full", b"four"),
            ("0.0.1", "Full", b"one"),
        ]);
        let releases = parse_feed(&bytes, PACKAGE_ID).unwrap();
        assert_eq!(releases.len(), 3);
        let newest = newest(&releases, "0.0.2").unwrap();
        assert_eq!(newest.version.to_string(), "0.0.4");
        assert_eq!(newest.file_name, "SelahApp-0.0.4-full.nupkg");
        assert!(super::newest(&releases, "0.0.4").is_none());
        assert!(super::newest(&releases, "1.0.0").is_none());
    }

    #[test]
    fn feeds_are_parsed_with_limits() {
        // sec: SEC-03, SEC-12
        assert!(parse_feed(&vec![b' '; FEED_LIMIT as usize + 1], PACKAGE_ID).is_err());
        assert!(parse_feed(b"{not json", PACKAGE_ID).is_err());
        let many = format!(
            r#"{{"Assets":[{}]}}"#,
            vec![r#"{"Version":"1.0.0","FileName":"a.nupkg","Size":1}"#; MAX_ASSETS + 1].join(",")
        );
        assert!(parse_feed(many.as_bytes(), PACKAGE_ID).is_err());
        // Unsafe file names, missing hashes, other packages and absurd sizes are ignored.
        let hash = "0".repeat(64);
        let tricky = format!(
            r#"{{"Assets":[
              {{"PackageId":"SelahApp","Version":"9.0.0","Type":"Full","FileName":"../evil.nupkg","SHA256":"{hash}","Size":1}},
              {{"PackageId":"SelahApp","Version":"9.0.1","Type":"Full","FileName":"C:\\evil.nupkg","SHA256":"{hash}","Size":1}},
              {{"PackageId":"SelahApp","Version":"9.0.2","Type":"Full","FileName":"evil.exe","SHA256":"{hash}","Size":1}},
              {{"PackageId":"SelahApp","Version":"9.0.3","Type":"Full","FileName":"ok.nupkg","SHA256":"short","Size":1}},
              {{"PackageId":"Other","Version":"9.0.4","Type":"Full","FileName":"ok.nupkg","SHA256":"{hash}","Size":1}},
              {{"PackageId":"SelahApp","Version":"9.0.5","Type":"Full","FileName":"ok.nupkg","SHA256":"{hash}","Size":99999999999}},
              {{"PackageId":"SelahApp","Version":"not a version","Type":"Full","FileName":"ok.nupkg","SHA256":"{hash}","Size":1}}
            ]}}"#
        );
        assert_eq!(
            parse_feed(tricky.as_bytes(), PACKAGE_ID).unwrap(),
            Vec::new()
        );
    }

    #[test]
    fn only_https_json_feeds_are_accepted() {
        // sec: SEC-12
        assert!(validate_feed_url(FEED_URL).is_ok());
        for bad in [
            "http://updates.example.invalid/releases.dev.json",
            "file:///C:/releases.dev.json",
            "https://updates.example.invalid/",
            "https://user:pass@updates.example.invalid/releases.dev.json",
            "https://updates.example.invalid/setup.exe",
            "releases.dev.json",
        ] {
            assert!(validate_feed_url(bad).is_err(), "{bad}");
        }
        assert_eq!(
            package_url(FEED_URL, "SelahApp-0.0.4-full.nupkg"),
            "https://updates.example.invalid/selah/SelahApp-0.0.4-full.nupkg"
        );
    }

    #[test]
    fn a_package_with_the_wrong_hash_or_size_is_never_saved() {
        // sec: SEC-12
        let good: &[u8] = b"the real package";
        let bytes = feed(&[("0.0.4", "Full", good)]);
        let release = newest(&parse_feed(&bytes, PACKAGE_ID).unwrap(), "0.0.1").unwrap();
        let package = package_url(FEED_URL, &release.file_name);
        let dir = tempfile::tempdir().unwrap();

        let (tampered, _) = net(vec![(package.clone(), b"the fake package".to_vec())]);
        let error = download(&tampered, FEED_URL, &release, dir.path()).unwrap_err();
        assert_eq!(error.code(), codes::VERIFY);
        let (short, _) = net(vec![(package.clone(), b"short".to_vec())]);
        assert_eq!(
            download(&short, FEED_URL, &release, dir.path())
                .unwrap_err()
                .code(),
            codes::VERIFY
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

        let (honest, calls) = net(vec![(package.clone(), good.to_vec())]);
        let saved = download(&honest, FEED_URL, &release, dir.path()).unwrap();
        assert_eq!(std::fs::read(saved).unwrap(), good);
        // The download is limited to the size the feed promised.
        assert_eq!(calls.lock().unwrap().as_slice(), &[(package, release.size)]);
    }

    #[test]
    fn check_reads_only_the_feed() {
        let bytes = feed(&[("0.0.4", "Full", b"four")]);
        let (gate, calls) = net(vec![(FEED_URL.into(), bytes)]);
        let found = check(&gate, FEED_URL, PACKAGE_ID, "0.0.3")
            .unwrap()
            .unwrap();
        assert_eq!(found.notes_markdown, "Notes for 0.0.4");
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            &[(FEED_URL.to_owned(), FEED_LIMIT)]
        );
        // The connection log names the host and purpose only.
        let log = gate.connections();
        assert_eq!(log[0].host, "updates.example.invalid");
        assert_eq!(log[0].purpose, Purpose::UpdateCheck);
    }

    #[test]
    fn the_network_policy_follows_the_update_settings() {
        // sec: SEC-13
        let mut settings = crate::settings::StoredSettings::default();
        assert!(policy_for(&settings).is_offline());
        settings.allow_update_check = true;
        assert!(policy_for(&settings).is_offline(), "no feed, no connection");
        settings.update_feed_url = FEED_URL.into();
        assert!(policy_for(&settings).allows(Purpose::UpdateCheck));
        settings.allow_update_check = false;
        assert!(policy_for(&settings).is_offline());
    }
}
