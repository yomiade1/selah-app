//! Strictly bounded ZIP reader and deterministic ZIP writer (SEC-03, SEC-15, M2.6).
//!
//! Enforces entry count limits, per-entry and total decompressed size limits while streaming,
//! CRC32 verification, Central Directory ↔ Local Header cross-checks, overlap detection,
//! symlink rejection, and archive path normalisation before writing or returning any bytes.
use std::collections::HashSet;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use anyhow::{Context, ensure};

const LFH_SIG: u32 = 0x0403_4b50;
const CDH_SIG: u32 = 0x0201_4b50;
const EOCD_SIG: u32 = 0x0605_4b50;

const METHOD_STORE: u16 = 0;
const METHOD_DEFLATE: u16 = 8;

const FLAG_ENCRYPTED: u16 = 0x0001;
const FLAG_DATA_DESCRIPTOR: u16 = 0x0008;
const FLAG_PATCHED_DATA: u16 = 0x0020;
const FLAG_STRONG_ENCRYPTED: u16 = 0x0040;
const FLAG_UTF8: u16 = 0x0800;
const FLAG_MASKED_VALUES: u16 = 0x2000;
const DISALLOWED_FLAGS: u16 = FLAG_ENCRYPTED
    | FLAG_DATA_DESCRIPTOR
    | FLAG_PATCHED_DATA
    | FLAG_STRONG_ENCRYPTED
    | FLAG_MASKED_VALUES;

const S_IFMT: u32 = 0o17_0000;
const S_IFREG: u32 = 0o10_0000;
const S_IFLNK: u32 = 0o12_0000;

/// Fixed DOS timestamp (`1980-01-01 00:00:00`) for deterministic archives.
const FIXED_DOS_TIME: u16 = 0;
const FIXED_DOS_DATE: u16 = 0x0021;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    Stored,
    Deflated,
}

#[derive(Clone, Copy, Debug)]
pub struct ZipLimits {
    pub max_archive_bytes: u64,
    pub max_entries: usize,
    pub max_entry_bytes: u64,
    pub max_total_uncompressed_bytes: u64,
}

impl ZipLimits {
    pub const fn bundle() -> Self {
        Self {
            max_archive_bytes: 256 * 1024 * 1024,
            max_entries: 8_300,
            max_entry_bytes: 128 * 1024 * 1024,
            max_total_uncompressed_bytes: 512 * 1024 * 1024,
        }
    }

    pub const fn backup() -> Self {
        Self {
            max_archive_bytes: 2 * 1024 * 1024 * 1024,
            max_entries: 65_000,
            max_entry_bytes: 1024 * 1024 * 1024,
            max_total_uncompressed_bytes: 3 * 1024 * 1024 * 1024,
        }
    }

    pub const fn fuzz() -> Self {
        Self {
            max_archive_bytes: 8 * 1024 * 1024,
            max_entries: 512,
            max_entry_bytes: 2 * 1024 * 1024,
            max_total_uncompressed_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Validate a relative forward-slash ZIP entry path (`manifest.json`, `media/<hash>.png`).
pub fn validate_entry_name(name: &str) -> anyhow::Result<()> {
    ensure!(!name.is_empty(), "archive entry name cannot be empty");
    ensure!(name.len() <= 240, "archive entry name exceeds 240 bytes");
    ensure!(
        !name.starts_with('/') && !name.starts_with('\\'),
        "archive entry '{name}' must be relative"
    );
    ensure!(
        !name.contains('\\') && !name.contains('\0'),
        "archive entry '{name}' contains forbidden separator or NUL"
    );
    ensure!(
        !name.chars().any(char::is_control),
        "archive entry '{name}' contains control characters"
    );
    for segment in name.split('/') {
        ensure!(
            !segment.is_empty() && segment != "." && segment != "..",
            "archive entry '{name}' contains path traversal or empty segment"
        );
        ensure!(
            !segment
                .chars()
                .any(|c| matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*')),
            "archive entry '{name}' contains forbidden filename characters"
        );
    }
    Ok(())
}

/// Streaming deterministic ZIP writer (no data descriptors, UTF-8 names, fixed timestamp).
pub struct DeterministicZipWriter<W: Write> {
    writer: W,
    written: u64,
    central: Vec<u8>,
    entries: u16,
    seen: HashSet<String>,
}

impl<W: Write> DeterministicZipWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            written: 0,
            central: Vec::new(),
            entries: 0,
            seen: HashSet::new(),
        }
    }

    pub fn add_bytes(
        &mut self,
        name: &str,
        contents: &[u8],
        compression: Compression,
    ) -> anyhow::Result<()> {
        validate_entry_name(name)?;
        ensure!(
            self.seen.insert(name.to_ascii_lowercase()),
            "duplicate archive entry '{name}'"
        );
        let size = u32::try_from(contents.len()).context("archive entry exceeds 4 GiB")?;
        let crc = crc32fast::hash(contents);
        let (method, payload): (u16, Vec<u8>) = match compression {
            Compression::Stored => (METHOD_STORE, contents.to_vec()),
            Compression::Deflated => {
                let mut enc =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                enc.write_all(contents)?;
                (METHOD_DEFLATE, enc.finish()?)
            }
        };
        self.write_prepared(name, method, crc, size, &payload)
    }

    pub fn add_reader<R: Read>(
        &mut self,
        name: &str,
        reader: R,
        expected_size: u64,
        compression: Compression,
    ) -> anyhow::Result<()> {
        validate_entry_name(name)?;
        ensure!(
            self.seen.insert(name.to_ascii_lowercase()),
            "duplicate archive entry '{name}'"
        );
        let size = u32::try_from(expected_size).context("archive entry exceeds 4 GiB")?;
        let mut raw = Vec::with_capacity((expected_size as usize).min(4 * 1024 * 1024));
        reader.take(expected_size + 1).read_to_end(&mut raw)?;
        ensure!(
            raw.len() as u64 == expected_size,
            "archive entry size changed while reading"
        );
        let crc = crc32fast::hash(&raw);
        let (method, payload): (u16, Vec<u8>) = match compression {
            Compression::Stored => (METHOD_STORE, raw),
            Compression::Deflated => {
                let mut enc =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                enc.write_all(&raw)?;
                (METHOD_DEFLATE, enc.finish()?)
            }
        };
        self.write_prepared(name, method, crc, size, &payload)
    }

    fn write_prepared(
        &mut self,
        name: &str,
        method: u16,
        crc: u32,
        size: u32,
        payload: &[u8],
    ) -> anyhow::Result<()> {
        let packed = u32::try_from(payload.len()).context("compressed entry exceeds 4 GiB")?;
        let offset = u32::try_from(self.written).context("archive exceeds 4 GiB")?;
        let name_len = u16::try_from(name.len()).context("entry name exceeds 64 KiB")?;
        let count_next = self
            .entries
            .checked_add(1)
            .context("archive exceeds 65535 entries")?;

        let mut local = Vec::with_capacity(30 + name.len());
        local.extend_from_slice(&LFH_SIG.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&FLAG_UTF8.to_le_bytes());
        local.extend_from_slice(&method.to_le_bytes());
        local.extend_from_slice(&FIXED_DOS_TIME.to_le_bytes());
        local.extend_from_slice(&FIXED_DOS_DATE.to_le_bytes());
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&packed.to_le_bytes());
        local.extend_from_slice(&size.to_le_bytes());
        local.extend_from_slice(&name_len.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(name.as_bytes());
        self.writer.write_all(&local)?;
        self.writer.write_all(payload)?;
        self.written = self
            .written
            .checked_add(local.len() as u64)
            .and_then(|n| n.checked_add(payload.len() as u64))
            .context("archive size overflow")?;

        let c = &mut self.central;
        c.extend_from_slice(&CDH_SIG.to_le_bytes());
        c.extend_from_slice(&((3u16 << 8) | 20u16).to_le_bytes()); // UNIX + v2.0
        c.extend_from_slice(&20u16.to_le_bytes());
        c.extend_from_slice(&FLAG_UTF8.to_le_bytes());
        c.extend_from_slice(&method.to_le_bytes());
        c.extend_from_slice(&FIXED_DOS_TIME.to_le_bytes());
        c.extend_from_slice(&FIXED_DOS_DATE.to_le_bytes());
        c.extend_from_slice(&crc.to_le_bytes());
        c.extend_from_slice(&packed.to_le_bytes());
        c.extend_from_slice(&size.to_le_bytes());
        c.extend_from_slice(&name_len.to_le_bytes());
        c.extend_from_slice(&[0u8; 8]); // extra, comment, disk start, internal attrs
        c.extend_from_slice(&((S_IFREG | 0o644) << 16).to_le_bytes());
        c.extend_from_slice(&offset.to_le_bytes());
        c.extend_from_slice(name.as_bytes());

        self.entries = count_next;
        Ok(())
    }

    pub fn finish(mut self) -> anyhow::Result<(W, u64)> {
        let central_offset = u32::try_from(self.written).context("archive exceeds 4 GiB")?;
        let central_size =
            u32::try_from(self.central.len()).context("central dir exceeds 4 GiB")?;
        self.writer.write_all(&self.central)?;
        let mut end = Vec::with_capacity(22);
        end.extend_from_slice(&EOCD_SIG.to_le_bytes());
        end.extend_from_slice(&[0u8; 4]);
        end.extend_from_slice(&self.entries.to_le_bytes());
        end.extend_from_slice(&self.entries.to_le_bytes());
        end.extend_from_slice(&central_size.to_le_bytes());
        end.extend_from_slice(&central_offset.to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        self.writer.write_all(&end)?;
        let total = self
            .written
            .checked_add(self.central.len() as u64)
            .and_then(|n| n.checked_add(end.len() as u64))
            .context("archive size overflow")?;
        Ok((self.writer, total))
    }
}

#[derive(Clone, Debug)]
struct CdEntry {
    name: String,
    flags: u16,
    method: u16,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    local_header_offset: u32,
}

fn le_u16(b: &[u8], at: usize) -> anyhow::Result<u16> {
    let slice = b.get(at..at + 2).context("unexpected EOF reading u16")?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn le_u32(b: &[u8], at: usize) -> anyhow::Result<u32> {
    let slice = b.get(at..at + 4).context("unexpected EOF reading u32")?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn find_eocd<R: Read + Seek>(
    reader: &mut R,
    file_len: u64,
) -> anyhow::Result<(u64, u16, u64, u64)> {
    ensure!(file_len >= 22, "archive is too small to be a ZIP file");
    let tail_len = file_len.min(22 + 65_535) as usize;
    let tail_start = file_len - tail_len as u64;
    reader.seek(SeekFrom::Start(tail_start))?;
    let mut tail = vec![0u8; tail_len];
    reader.read_exact(&mut tail)?;

    for idx in (0..=tail_len - 22).rev() {
        if le_u32(&tail, idx)? == EOCD_SIG {
            let disk_num = le_u16(&tail, idx + 4)?;
            let cd_disk = le_u16(&tail, idx + 6)?;
            let entries_disk = le_u16(&tail, idx + 8)?;
            let entries_total = le_u16(&tail, idx + 10)?;
            let cd_size = le_u32(&tail, idx + 12)?;
            let cd_offset = le_u32(&tail, idx + 16)?;
            let comment_len = le_u16(&tail, idx + 20)? as usize;

            let eocd_offset = tail_start + idx as u64;
            ensure!(
                eocd_offset + 22 + comment_len as u64 == file_len,
                "archive contains trailing data after end-of-central-directory"
            );
            ensure!(
                disk_num == 0 && cd_disk == 0 && entries_disk == entries_total,
                "multi-disk ZIP archives are not supported"
            );
            ensure!(
                entries_total != u16::MAX && cd_size != u32::MAX && cd_offset != u32::MAX,
                "ZIP64 archives are not supported"
            );
            return Ok((
                eocd_offset,
                entries_total,
                u64::from(cd_offset),
                u64::from(cd_size),
            ));
        }
    }
    anyhow::bail!("missing ZIP end-of-central-directory record")
}

/// Inspect and stream-verify every entry of a bounded ZIP archive without buffering all
/// entries at once. Callers receive verified decompressed streams/readers in CD order.
pub fn visit_zip<R: Read + Seek, F>(
    mut reader: R,
    limits: ZipLimits,
    mut visitor: F,
) -> anyhow::Result<()>
where
    F: FnMut(&str, u64, &mut dyn Read) -> anyhow::Result<()>,
{
    let file_len = reader.seek(SeekFrom::End(0))?;
    ensure!(
        file_len <= limits.max_archive_bytes,
        "archive exceeds {} byte limit",
        limits.max_archive_bytes
    );
    let (eocd_offset, entry_count, cd_offset, cd_size) = find_eocd(&mut reader, file_len)?;
    ensure!(
        usize::from(entry_count) <= limits.max_entries,
        "archive has too many entries (limit {})",
        limits.max_entries
    );
    ensure!(
        cd_size <= 32 * 1024 * 1024,
        "archive central directory exceeds size limit"
    );
    ensure!(
        cd_offset
            .checked_add(cd_size)
            .is_some_and(|end| end == eocd_offset),
        "archive central directory offsets do not match EOCD"
    );

    reader.seek(SeekFrom::Start(cd_offset))?;
    let mut cd = vec![0u8; cd_size as usize];
    reader.read_exact(&mut cd)?;

    let mut pos = 0usize;
    let mut entries = Vec::with_capacity(usize::from(entry_count));
    let mut seen_lower = HashSet::new();
    let mut total_declared = 0u64;

    for _ in 0..entry_count {
        ensure!(pos + 46 <= cd.len(), "truncated central directory entry");
        ensure!(le_u32(&cd, pos)? == CDH_SIG, "invalid CD header signature");
        let version_needed = le_u16(&cd, pos + 6)?;
        ensure!(
            version_needed <= 45,
            "unsupported ZIP version {version_needed}"
        );
        let flags = le_u16(&cd, pos + 8)?;
        ensure!(
            flags & DISALLOWED_FLAGS == 0,
            "ZIP encryption, data descriptors, or masked headers are not allowed"
        );
        let method = le_u16(&cd, pos + 10)?;
        ensure!(
            method == METHOD_STORE || method == METHOD_DEFLATE,
            "unsupported ZIP compression method {method}"
        );
        let crc32 = le_u32(&cd, pos + 16)?;
        let compressed_size = le_u32(&cd, pos + 20)?;
        let uncompressed_size = le_u32(&cd, pos + 24)?;
        let name_len = usize::from(le_u16(&cd, pos + 28)?);
        let extra_len = usize::from(le_u16(&cd, pos + 30)?);
        let comment_len = usize::from(le_u16(&cd, pos + 32)?);
        let disk_start = le_u16(&cd, pos + 34)?;
        ensure!(disk_start == 0, "multi-disk ZIP entry is not supported");
        let external_attrs = le_u32(&cd, pos + 38)?;
        let unix_mode = (external_attrs >> 16) & S_IFMT;
        ensure!(
            unix_mode != S_IFLNK,
            "archive symlinks are forbidden (SEC-03)"
        );
        ensure!(
            unix_mode == 0 || unix_mode == S_IFREG,
            "archive entry has unsupported file type bits"
        );
        let local_header_offset = le_u32(&cd, pos + 42)?;

        let name_start = pos + 46;
        let record_end = name_start + name_len + extra_len + comment_len;
        ensure!(record_end <= cd.len(), "truncated CD variable fields");
        let name = std::str::from_utf8(&cd[name_start..name_start + name_len])
            .context("archive entry name is not valid UTF-8")?
            .to_owned();
        validate_entry_name(&name)?;
        ensure!(
            seen_lower.insert(name.to_ascii_lowercase()),
            "duplicate or case-colliding archive entry '{name}'"
        );

        ensure!(
            u64::from(uncompressed_size) <= limits.max_entry_bytes,
            "entry '{name}' exceeds {} byte limit",
            limits.max_entry_bytes
        );
        total_declared = total_declared
            .checked_add(u64::from(uncompressed_size))
            .context("archive total size overflow")?;
        ensure!(
            total_declared <= limits.max_total_uncompressed_bytes,
            "archive uncompressed size exceeds {} byte limit",
            limits.max_total_uncompressed_bytes
        );
        if method == METHOD_STORE {
            ensure!(
                compressed_size == uncompressed_size,
                "stored entry '{name}' compressed/uncompressed size mismatch"
            );
        }

        entries.push(CdEntry {
            name,
            flags,
            method,
            crc32,
            compressed_size,
            uncompressed_size,
            local_header_offset,
        });
        pos = record_end;
    }
    ensure!(
        pos == cd.len(),
        "central directory contains unexpected trailing bytes"
    );

    // Check Local File Headers and ensure local data extents stay within [0..cd_offset) without overlaps.
    let mut spans: Vec<(u64, u64)> = Vec::with_capacity(entries.len());
    let mut data_ranges: Vec<(u64, u64)> = Vec::with_capacity(entries.len());
    for entry in &entries {
        let lfh_start = u64::from(entry.local_header_offset);
        ensure!(
            lfh_start + 30 <= cd_offset,
            "local header for '{}' extends into central directory",
            entry.name
        );
        reader.seek(SeekFrom::Start(lfh_start))?;
        let mut lfh = [0u8; 30];
        reader.read_exact(&mut lfh)?;
        ensure!(
            le_u32(&lfh, 0)? == LFH_SIG,
            "invalid local header signature"
        );
        let flags = le_u16(&lfh, 6)?;
        let method = le_u16(&lfh, 8)?;
        let crc32 = le_u32(&lfh, 14)?;
        let compressed_size = le_u32(&lfh, 18)?;
        let uncompressed_size = le_u32(&lfh, 22)?;
        let name_len = usize::from(le_u16(&lfh, 26)?);
        let extra_len = usize::from(le_u16(&lfh, 28)?);

        ensure!(
            flags == entry.flags
                && method == entry.method
                && crc32 == entry.crc32
                && compressed_size == entry.compressed_size
                && uncompressed_size == entry.uncompressed_size,
            "local header disagrees with central directory for '{}'",
            entry.name
        );
        let data_start = lfh_start + 30 + name_len as u64 + extra_len as u64;
        let data_end = data_start + u64::from(entry.compressed_size);
        ensure!(
            data_end <= cd_offset,
            "entry '{}' data extends past central directory",
            entry.name
        );
        let mut lfh_name = vec![0u8; name_len];
        reader.read_exact(&mut lfh_name)?;
        ensure!(
            lfh_name == entry.name.as_bytes(),
            "local header name disagrees with central directory"
        );
        spans.push((lfh_start, data_end));
        data_ranges.push((data_start, u64::from(entry.compressed_size)));
    }
    spans.sort_unstable();
    for pair in spans.windows(2) {
        ensure!(
            pair[0].1 <= pair[1].0,
            "archive contains overlapping local entry ranges"
        );
    }

    let mut total_actual = 0u64;
    for (entry, (data_start, comp_len)) in entries.iter().zip(data_ranges) {
        reader.seek(SeekFrom::Start(data_start))?;
        let mut bounded_comp = (&mut reader).take(comp_len);
        let mut crc_reader = CrcCheckedReader {
            inner: match entry.method {
                METHOD_STORE => DecompSource::Stored(&mut bounded_comp),
                METHOD_DEFLATE => DecompSource::Deflated(Box::new(
                    flate2::read::DeflateDecoder::new(&mut bounded_comp),
                )),
                _ => unreachable!(),
            },
            hasher: crc32fast::Hasher::new(),
            read_bytes: 0,
            expected_bytes: u64::from(entry.uncompressed_size),
            max_entry_bytes: limits.max_entry_bytes,
            total_actual: &mut total_actual,
            max_total_bytes: limits.max_total_uncompressed_bytes,
            expected_crc32: entry.crc32,
            finished_ok: false,
        };
        visitor(
            &entry.name,
            u64::from(entry.uncompressed_size),
            &mut crc_reader,
        )?;
        crc_reader.verify_eof()?;
    }

    Ok(())
}

enum DecompSource<'a> {
    Stored(&'a mut dyn Read),
    Deflated(Box<flate2::read::DeflateDecoder<&'a mut dyn Read>>),
}

impl Read for DecompSource<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Stored(r) => r.read(buf),
            Self::Deflated(d) => d.read(buf),
        }
    }
}

struct CrcCheckedReader<'a> {
    inner: DecompSource<'a>,
    hasher: crc32fast::Hasher,
    read_bytes: u64,
    expected_bytes: u64,
    max_entry_bytes: u64,
    total_actual: &'a mut u64,
    max_total_bytes: u64,
    expected_crc32: u32,
    finished_ok: bool,
}

impl Read for CrcCheckedReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let n = self.inner.read(buf)?;
        if n == 0 {
            if self.read_bytes != self.expected_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "archive entry ended before declared uncompressed size",
                ));
            }
            let actual_crc = self.hasher.clone().finalize();
            if actual_crc != self.expected_crc32 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "archive entry CRC32 mismatch",
                ));
            }
            self.finished_ok = true;
            return Ok(0);
        }
        self.read_bytes = self
            .read_bytes
            .checked_add(n as u64)
            .ok_or_else(|| std::io::Error::other("byte counter overflow"))?;
        if self.read_bytes > self.expected_bytes || self.read_bytes > self.max_entry_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "archive entry decompressed past declared size limit (zip bomb)",
            ));
        }
        *self.total_actual = self
            .total_actual
            .checked_add(n as u64)
            .ok_or_else(|| std::io::Error::other("total byte counter overflow"))?;
        if *self.total_actual > self.max_total_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "archive total decompressed size exceeds limit",
            ));
        }
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
}

impl CrcCheckedReader<'_> {
    fn verify_eof(mut self) -> anyhow::Result<()> {
        if !self.finished_ok {
            let mut probe = [0u8; 64];
            let leftover = self.read(&mut probe)?;
            ensure!(
                leftover == 0 && self.finished_ok,
                "archive entry has unread or trailing decompressed data"
            );
        }
        Ok(())
    }
}

/// Read all entries from an in-memory ZIP byte slice into memory under `limits`.
pub fn read_zip_bytes(bytes: &[u8], limits: ZipLimits) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    ensure!(
        bytes.len() as u64 <= limits.max_archive_bytes,
        "archive exceeds {} byte limit",
        limits.max_archive_bytes
    );
    let mut entries = Vec::new();
    visit_zip(Cursor::new(bytes), limits, |name, size, reader| {
        let mut buf = Vec::with_capacity((size as usize).min(1024 * 1024));
        reader.read_to_end(&mut buf)?;
        entries.push((name.to_owned(), buf));
        Ok(())
    })?;
    Ok(entries)
}
