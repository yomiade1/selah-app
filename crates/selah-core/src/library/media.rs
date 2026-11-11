use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, ensure};

/// Hard storage limit; image pixel and decoder limits are enforced by the import worker.
pub const MAX_MEDIA_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaBlob {
    pub content_hash: String,
    pub extension: String,
    pub size_bytes: u64,
}

pub struct MediaStore {
    root: PathBuf,
}

pub(crate) fn validate_address(hash: &str, extension: &str) -> anyhow::Result<()> {
    ensure!(
        hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid media hash"
    );
    ensure!(
        !extension.is_empty()
            && extension.len() <= 10
            && extension
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
        "invalid media extension"
    );
    Ok(())
}

fn validate_extension(extension: &str) -> anyhow::Result<String> {
    let extension = extension.to_ascii_lowercase();
    validate_address(&"0".repeat(64), &extension)?;
    Ok(extension)
}

impl MediaStore {
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(root)?;
        Ok(Self {
            root: root.canonicalize()?,
        })
    }

    /// Store through a bounded stream, hash while copying, sync, then publish atomically.
    /// A failed read, oversized source or hash mismatch never publishes a partial object.
    pub fn import(&self, source: &Path) -> anyhow::Result<MediaBlob> {
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .context("media needs an extension")?;
        let extension = validate_extension(extension)?;
        let file = std::fs::File::open(source)?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file() && metadata.len() <= MAX_MEDIA_BYTES,
            "media is not a bounded regular file"
        );
        self.import_reader(file, &extension, MAX_MEDIA_BYTES)
    }

    /// The lower limit is useful to workers with tighter import budgets.
    pub fn import_reader(
        &self,
        mut source: impl Read,
        extension: &str,
        limit: u64,
    ) -> anyhow::Result<MediaBlob> {
        let extension = validate_extension(extension)?;
        ensure!(
            limit <= MAX_MEDIA_BYTES,
            "media budget exceeds storage limit"
        );
        let mut temporary = tempfile::NamedTempFile::new_in(&self.root)?;
        let mut hasher = blake3::Hasher::new();
        let mut count = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            count = count
                .checked_add(read as u64)
                .context("media size overflow")?;
            ensure!(count <= limit, "media exceeds import budget");
            hasher.update(&buffer[..read]);
            temporary.write_all(&buffer[..read])?;
        }
        let hash = hasher.finalize().to_hex().to_string();
        let directory = self.root.join(&hash[..2]);
        std::fs::create_dir_all(&directory)?;
        ensure!(
            directory.canonicalize()?.starts_with(&self.root),
            "media path escaped its store"
        );
        let target = directory.join(format!("{hash}.{extension}"));
        temporary.as_file().sync_all()?;
        match temporary.persist_noclobber(&target) {
            Ok(file) => {
                file.sync_all()?;
            }
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.verify(&hash, &extension, count)?;
            }
            Err(error) => return Err(error.error.into()),
        }
        // On Unix a directory sync makes the name durable across a power failure.
        #[cfg(unix)]
        {
            std::fs::File::open(&directory)?.sync_all()?;
            std::fs::File::open(&self.root)?.sync_all()?;
        }
        Ok(MediaBlob {
            content_hash: hash,
            extension,
            size_bytes: count,
        })
    }

    fn path(&self, hash: &str, extension: &str) -> anyhow::Result<PathBuf> {
        validate_address(hash, extension)?;
        let path = self
            .root
            .join(&hash[..2])
            .join(format!("{hash}.{extension}"));
        ensure!(
            path.canonicalize()?.starts_with(&self.root),
            "media path escaped its store"
        );
        Ok(path)
    }

    /// Verify the exact open handle, then return it for use. Callers must not reopen by path.
    pub fn open_verified(
        &self,
        hash: &str,
        extension: &str,
        expected_size: u64,
    ) -> anyhow::Result<std::fs::File> {
        use std::io::{Seek, SeekFrom};
        ensure!(expected_size <= MAX_MEDIA_BYTES, "media is too large");
        let path = self.path(hash, extension)?;
        let mut file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file() && metadata.len() == expected_size,
            "media size mismatch"
        );
        let mut hasher = blake3::Hasher::new();
        let mut reader = (&mut file).take(expected_size + 1);
        hasher.update_reader(&mut reader)?;
        ensure!(
            hasher.finalize().to_hex().as_str() == hash,
            "media hash mismatch"
        );
        ensure!(
            file.metadata()?.len() == expected_size,
            "media changed during verification"
        );
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    }

    pub fn verify(&self, hash: &str, extension: &str, expected_size: u64) -> anyhow::Result<()> {
        self.open_verified(hash, extension, expected_size)?;
        Ok(())
    }

    pub(super) fn verified_blob_path(
        &self,
        hash: &str,
        extension: &str,
        expected_size: u64,
    ) -> anyhow::Result<PathBuf> {
        let path = self.path(hash, extension)?;
        let meta = std::fs::symlink_metadata(&path)?;
        ensure!(
            !meta.file_type().is_symlink() && meta.is_file(),
            "media blob is a symlink or not a regular file"
        );
        self.verify(hash, extension, expected_size)?;
        Ok(path)
    }

    pub(crate) fn verified_blob_path_by_hash(
        &self,
        hash: &str,
        extension: &str,
    ) -> anyhow::Result<PathBuf> {
        let path = self.path(hash, extension)?;
        let meta = std::fs::symlink_metadata(&path)?;
        ensure!(
            !meta.file_type().is_symlink() && meta.is_file(),
            "media blob is a symlink or not a regular file"
        );
        let size = meta.len();
        ensure!(size <= MAX_MEDIA_BYTES, "media is too large");
        self.verify(hash, extension, size)?;
        Ok(path)
    }
}
