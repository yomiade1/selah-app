//! SQLite library storage (ADR-012), owned exclusively by core.
//!
//! Repository writes are storage primitives; the command bus adds authorisation,
//! operation logging and events before exposing mutations over IPC.
mod media;
mod ops;
mod policy;
mod repository;
mod schema;

pub use media::{MediaBlob, MediaStore};
pub use ops::{CommandBusError, CommandMeta, ExecutionOutcome, MutationEventInfo};
pub use policy::{EffectiveSetting, PolicyLevel};
pub use repository::{Document, DocumentKind};
pub use schema::CURRENT_SCHEMA_VERSION;

use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use rusqlite::{Connection, OpenFlags};
use selah_proto::v1::Id;

/// Maximum encoded size for a library document. Also bounds protobuf decoding.
pub const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_CHILDREN: usize = 4096;

/// Generate a time-ordered UUIDv7 with OS randomness, represented on the wire as 16 bytes.
pub fn new_id() -> Id {
    Id {
        value: uuid::Uuid::now_v7().as_bytes().to_vec(),
    }
}

/// Reject malformed, nil, non-RFC4122 and non-v7 IDs at the storage boundary.
pub fn validate_id(id: &Id) -> anyhow::Result<()> {
    let value = uuid::Uuid::from_slice(&id.value).context("invalid ID length")?;
    ensure!(
        value.get_version_num() == 7 && value.get_variant() == uuid::Variant::RFC4122,
        "expected a UUIDv7"
    );
    Ok(())
}

pub(crate) fn required_id(id: &Option<Id>) -> anyhow::Result<&[u8]> {
    let id = id.as_ref().context("missing ID")?;
    validate_id(id)?;
    Ok(&id.value)
}

/// The library folder has stable, separate locations for database, media and backups.
#[derive(Clone, Debug)]
pub struct LibraryPaths {
    pub root: PathBuf,
    pub database: PathBuf,
    pub media: PathBuf,
    pub bibles: PathBuf,
    pub packs: PathBuf,
    pub backups: PathBuf,
}

impl LibraryPaths {
    pub fn create(root: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(root)?;
        let root = root.canonicalize()?;
        let paths = Self {
            database: root.join("library.db"),
            media: root.join("media"),
            bibles: root.join("bibles"),
            packs: root.join("packs"),
            backups: root.join("backups"),
            root,
        };
        for directory in [&paths.media, &paths.bibles, &paths.packs, &paths.backups] {
            std::fs::create_dir_all(directory)?;
            ensure!(
                directory.canonicalize()?.starts_with(&paths.root),
                "library directory escaped its root"
            );
        }
        Ok(paths)
    }
}

pub(crate) fn unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub struct Library {
    pub(crate) db: Connection,
    paths: LibraryPaths,
    idempotency_writes: u64,
    #[cfg(test)]
    _memory_dir: Option<tempfile::TempDir>,
}

impl Library {
    /// Open with WAL, FULL synchronous commits, foreign keys and an integrity check (SEC-15).
    /// Corrupt and newer databases fail closed; they are never replaced with an empty library.
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        let paths = LibraryPaths::create(root)?;
        // exists() follows symlinks and reports false for a dangling link. Inspect the
        // directory entry instead so opening SQLite cannot create its outside target.
        match std::fs::symlink_metadata(&paths.database) {
            Ok(_) => ensure!(
                paths.database.canonicalize()?.starts_with(&paths.root),
                "database escaped library root"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let mut db = Connection::open_with_flags(
            &paths.database,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        check_integrity(&db)?;
        // Refuse unsupported versions before changing journal mode or attempting migration.
        schema::check_history(&db)?;
        db.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL; PRAGMA trusted_schema = OFF;")?;
        schema::migrate(&mut db)?;
        Ok(Self {
            db,
            paths,
            idempotency_writes: 0,
            #[cfg(test)]
            _memory_dir: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory() -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let paths = LibraryPaths::create(dir.path())?;
        let mut db = Connection::open_in_memory()?;
        db.execute_batch("PRAGMA foreign_keys = ON; PRAGMA trusted_schema = OFF;")?;
        schema::migrate(&mut db)?;
        Ok(Self {
            db,
            paths,
            idempotency_writes: 0,
            _memory_dir: Some(dir),
        })
    }

    pub fn paths(&self) -> &LibraryPaths {
        &self.paths
    }

    pub fn media_store(&self) -> anyhow::Result<MediaStore> {
        MediaStore::open(&self.paths.media)
    }

    pub fn integrity_check(&self) -> anyhow::Result<()> {
        check_integrity(&self.db)
    }

    pub fn schema_version(&self) -> anyhow::Result<u32> {
        Ok(self
            .db
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }
}

fn check_integrity(db: &Connection) -> anyhow::Result<()> {
    let mut check = db.prepare("PRAGMA integrity_check")?;
    let results = check
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(results == ["ok"], "library integrity check failed");
    let mut foreign_keys = db.prepare("PRAGMA foreign_key_check")?;
    ensure!(
        foreign_keys.query([])?.next()?.is_none(),
        "library foreign key check failed"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_safety_pragmas_are_applied_to_the_library_connection() {
        let root = tempfile::tempdir().unwrap();
        let library = Library::open(root.path()).unwrap();
        for (pragma, expected) in [
            ("synchronous", 2),
            ("foreign_keys", 1),
            ("trusted_schema", 0),
        ] {
            let actual: i32 = library
                .db
                .pragma_query_value(None, pragma, |r| r.get(0))
                .unwrap();
            assert_eq!(actual, expected, "{pragma}");
        }
        let journal: String = library
            .db
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(journal, "wal");
    }
}
