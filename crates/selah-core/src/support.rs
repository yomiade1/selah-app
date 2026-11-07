//! **Help ▸ Report a problem** (M1.13, SEC-05, SEC-06): a support bundle built only from
//! redacted text.
//!
//! The bundle is a `.zip` in the support folder with:
//! - `versions.json`: Selah's version, the operating system and architecture, safe mode;
//! - `settings.json`: the settings (they never hold secrets), with the update address cut down
//!   to its host and path;
//! - `logs/<file>`: the last [`LOG_TAIL_BYTES`] of each log file;
//! - `diagnostics/<file>`: the newest **Run benchmark** report, if there is one.
//!
//! Every text passes through `selah_runtime::redact` (registered secrets, tokens, email
//! addresses and the user's name) before it's written, even though the logs were redacted when
//! they were written. Nothing is sent anywhere: the person attaches the file themselves.
use std::io::Write;
use std::path::{Path, PathBuf};

use selah_runtime::redact::redact;

use crate::settings::StoredSettings;

/// The end of each log file that goes in (the most recent lines).
pub const LOG_TAIL_BYTES: u64 = 4 * 1024 * 1024;
/// At most this many log files (newest first).
pub const MAX_LOG_FILES: usize = 40;

/// Where the problem-report form lives (`.github/ISSUE_TEMPLATE/problem-report.yml`).
pub const REPORT_FORM: &str =
    "https://github.com/yomiade1/selah-app/issues/new?template=problem-report.yml";

/// The form, with the version filled in.
pub fn report_url(version: &str) -> String {
    let version: String = version
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        .collect();
    format!("{REPORT_FORM}&version={version}")
}

/// `2026-11-07T14-03-09Z`-style UTC stamp for file names (no `:` on Windows).
pub fn file_stamp(time: std::time::SystemTime) -> String {
    let secs = time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}-{:02}-{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// What goes in, besides the files on disk.
pub struct BundleInput<'a> {
    pub version: &'a str,
    pub safe_mode: bool,
    pub settings: &'a StoredSettings,
    pub logs_dir: &'a Path,
    /// Where Diagnostics reports are saved (the newest is included).
    pub support_dir: &'a Path,
}

fn settings_for_support(settings: &StoredSettings) -> serde_json::Value {
    let mut copy = settings.clone();
    // Keep only scheme, host and path: a query string could hold a token.
    if let Some(cut) = copy.update_feed_url.find(['?', '#']) {
        copy.update_feed_url.truncate(cut);
    }
    serde_json::to_value(copy).unwrap_or_default()
}

fn tail(path: &Path, limit: u64) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length > limit {
        file.seek(SeekFrom::Start(length - limit))?;
    }
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    // Start at a whole line when the file was cut.
    Ok(if length > limit {
        text.split_once('\n')
            .map(|(_, rest)| rest.to_owned())
            .unwrap_or(text)
    } else {
        text
    })
}

fn files_newest_first(dir: &Path, extension: &str, prefix: &str) -> Vec<PathBuf> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .filter(|path| {
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with(prefix) && n.contains(extension))
                })
                .map(|path| {
                    let modified = path
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    (modified, path)
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.into_iter().map(|(_, path)| path).collect()
}

/// Build the bundle in `input.support_dir` and return its path.
pub fn create(input: &BundleInput<'_>) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(input.support_dir)?;
    let stamp = file_stamp(std::time::SystemTime::now());
    let path = input.support_dir.join(format!("selah-support-{stamp}.zip"));
    let mut zip = ZipWriter::default();

    let versions = serde_json::json!({
        "selah": input.version,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "safe_mode": input.safe_mode,
        "created": stamp,
    });
    zip.add(
        "versions.json",
        redact(&serde_json::to_string_pretty(&versions)?).as_bytes(),
    )?;
    let settings = serde_json::to_string_pretty(&settings_for_support(input.settings))?;
    zip.add("settings.json", redact(&settings).as_bytes())?;

    for log in files_newest_first(input.logs_dir, ".log", "")
        .into_iter()
        .take(MAX_LOG_FILES)
    {
        let Some(name) = log.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let text = tail(&log, LOG_TAIL_BYTES)?;
        zip.add(&format!("logs/{name}"), redact(&text).as_bytes())?;
    }
    if let Some(report) = files_newest_first(input.support_dir, ".json", "selah-diagnostics-")
        .into_iter()
        .next()
        && let Some(name) = report.file_name().and_then(|n| n.to_str())
    {
        let text = std::fs::read_to_string(&report)?;
        zip.add(&format!("diagnostics/{name}"), redact(&text).as_bytes())?;
    }
    selah_runtime::paths::write_atomic(&path, &zip.finish()?)?;
    tracing::info!(file = %path.display(), "support bundle saved");
    Ok(path)
}

/// A minimal zip writer: deflated entries, no data descriptors, no zip64 (bundles are small).
#[derive(Default)]
pub struct ZipWriter {
    data: Vec<u8>,
    central: Vec<u8>,
    entries: u16,
}

impl ZipWriter {
    pub fn add(&mut self, name: &str, contents: &[u8]) -> std::io::Result<()> {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(contents)?;
        let compressed = encoder.finish()?;
        let crc = crc32fast::hash(contents);
        let too_big = || std::io::Error::other("support bundle entry too large");
        let size = u32::try_from(contents.len()).map_err(|_| too_big())?;
        let packed = u32::try_from(compressed.len()).map_err(|_| too_big())?;
        let offset = u32::try_from(self.data.len()).map_err(|_| too_big())?;
        let name_len = u16::try_from(name.len()).map_err(|_| too_big())?;
        // Fixed DOS time: 1980-01-01 00:00 (the stamp is in the bundle's name).
        let (time, date) = (0u16, 0x21u16);
        let mut local = Vec::with_capacity(30 + name.len());
        local.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes()); // version needed
        local.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        local.extend_from_slice(&8u16.to_le_bytes()); // deflate
        local.extend_from_slice(&time.to_le_bytes());
        local.extend_from_slice(&date.to_le_bytes());
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&packed.to_le_bytes());
        local.extend_from_slice(&size.to_le_bytes());
        local.extend_from_slice(&name_len.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes()); // extra
        local.extend_from_slice(name.as_bytes());
        self.data.extend_from_slice(&local);
        self.data.extend_from_slice(&compressed);

        let c = &mut self.central;
        c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes()); // made by
        c.extend_from_slice(&20u16.to_le_bytes()); // needed
        c.extend_from_slice(&0x0800u16.to_le_bytes());
        c.extend_from_slice(&8u16.to_le_bytes());
        c.extend_from_slice(&time.to_le_bytes());
        c.extend_from_slice(&date.to_le_bytes());
        c.extend_from_slice(&crc.to_le_bytes());
        c.extend_from_slice(&packed.to_le_bytes());
        c.extend_from_slice(&size.to_le_bytes());
        c.extend_from_slice(&name_len.to_le_bytes());
        c.extend_from_slice(&[0; 8]); // extra, comment, disk start, internal attributes
        c.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        c.extend_from_slice(&offset.to_le_bytes());
        c.extend_from_slice(name.as_bytes());
        self.entries = self.entries.checked_add(1).ok_or_else(too_big)?;
        Ok(())
    }

    pub fn finish(mut self) -> std::io::Result<Vec<u8>> {
        let too_big = || std::io::Error::other("support bundle too large");
        let central_offset = u32::try_from(self.data.len()).map_err(|_| too_big())?;
        let central_size = u32::try_from(self.central.len()).map_err(|_| too_big())?;
        self.data.extend_from_slice(&self.central);
        let end = &mut self.data;
        end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        end.extend_from_slice(&[0; 4]); // disk numbers
        end.extend_from_slice(&self.entries.to_le_bytes());
        end.extend_from_slice(&self.entries.to_le_bytes());
        end.extend_from_slice(&central_size.to_le_bytes());
        end.extend_from_slice(&central_offset.to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes()); // comment
        Ok(self.data)
    }
}

/// Read back a bundle written by [`ZipWriter`] (tests and the support team's tools).
pub fn read_entries(bytes: &[u8]) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    use std::io::Read;
    let bad = || std::io::Error::other("not a Selah support bundle");
    let mut entries = Vec::new();
    let mut at = 0usize;
    let u16_at = |b: &[u8], i: usize| -> std::io::Result<u16> {
        Ok(u16::from_le_bytes(
            b.get(i..i + 2)
                .ok_or_else(bad)?
                .try_into()
                .map_err(|_| bad())?,
        ))
    };
    let u32_at = |b: &[u8], i: usize| -> std::io::Result<u32> {
        Ok(u32::from_le_bytes(
            b.get(i..i + 4)
                .ok_or_else(bad)?
                .try_into()
                .map_err(|_| bad())?,
        ))
    };
    while u32_at(bytes, at)? == 0x0403_4b50 {
        let packed = u32_at(bytes, at + 18)? as usize;
        let name_len = u16_at(bytes, at + 26)? as usize;
        let extra = u16_at(bytes, at + 28)? as usize;
        let name_start = at + 30;
        let name = String::from_utf8(
            bytes
                .get(name_start..name_start + name_len)
                .ok_or_else(bad)?
                .to_vec(),
        )
        .map_err(|_| bad())?;
        let data_start = name_start + name_len + extra;
        let data = bytes.get(data_start..data_start + packed).ok_or_else(bad)?;
        let mut contents = Vec::new();
        flate2::read::DeflateDecoder::new(data).read_to_end(&mut contents)?;
        entries.push((name, contents));
        at = data_start + packed;
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_stamps_are_utc_and_safe_for_file_names() {
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_793_973_789);
        assert_eq!(file_stamp(at), "2026-11-06T14-03-09Z");
        assert_eq!(file_stamp(std::time::UNIX_EPOCH), "1970-01-01T00-00-00Z");
    }

    #[test]
    fn the_report_form_gets_the_version_and_nothing_else() {
        assert_eq!(report_url("0.1.0"), format!("{REPORT_FORM}&version=0.1.0"));
        assert!(report_url("1.0&title=<x>").ends_with("version=1.0titlex"));
    }

    #[test]
    fn the_zip_round_trips() {
        let mut zip = ZipWriter::default();
        zip.add("a.txt", b"hello").unwrap();
        zip.add("logs/b.log", &vec![b'x'; 100_000]).unwrap();
        let bytes = zip.finish().unwrap();
        let entries = read_entries(&bytes).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], ("a.txt".into(), b"hello".to_vec()));
        assert_eq!(entries[1].1.len(), 100_000);
        assert!(bytes.len() < 10_000, "deflated");
    }

    #[test]
    fn long_logs_keep_only_their_most_recent_whole_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("core.log");
        let mut text = String::new();
        for i in 0..1000 {
            text.push_str(&format!("line {i}\n"));
        }
        std::fs::write(&path, &text).unwrap();
        let end = tail(&path, 100).unwrap();
        assert!(end.starts_with("line "));
        assert!(end.ends_with("line 999\n"));
        assert!(end.len() <= 100);
    }
}
