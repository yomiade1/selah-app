//! Append-only, transactional migrations. Historical SQL is hash-checked on every open.
use anyhow::ensure;
use rusqlite::{Connection, TransactionBehavior, params};

const MIGRATIONS: &[&str] = &[include_str!("migrations/001_library.sql")];

pub(super) fn check_history(db: &Connection) -> anyhow::Result<()> {
    let version = db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))? as usize;
    ensure!(
        version <= MIGRATIONS.len(),
        "library was written by a newer Selah version"
    );
    if version == 0 {
        let tables: i64 = db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        ensure!(tables == 0, "unversioned database is not an empty library");
        return Ok(());
    }
    let mut statement =
        db.prepare("SELECT version, checksum FROM schema_migrations ORDER BY version")?;
    let history = statement
        .query_map([], |r| {
            Ok((r.get::<_, u32>(0)? as usize, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(history.len() == version, "migration history is incomplete");
    for (index, (number, checksum)) in history.iter().enumerate() {
        ensure!(
            *number == index + 1
                && *checksum == blake3::hash(MIGRATIONS[index].as_bytes()).to_hex().as_str(),
            "historical migration was modified"
        );
    }
    Ok(())
}

pub(super) fn migrate(db: &mut Connection) -> anyhow::Result<()> {
    // Lock before rechecking the version so two openers cannot both apply a migration.
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    check_history(&tx)?;
    let version = tx.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))? as usize;
    if version == 0 {
        tx.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, checksum TEXT NOT NULL) STRICT;")?;
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES (?1, ?2)",
            params![
                (index + 1) as u32,
                blake3::hash(sql.as_bytes()).to_hex().as_str()
            ],
        )?;
        tx.pragma_update(None, "user_version", (index + 1) as u32)?;
    }
    tx.commit()?;
    Ok(())
}
