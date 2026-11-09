use anyhow::ensure;
use rusqlite::params;
use serde_json::Value;

use super::Library;

/// User values override defaults; locked machine/organisation policy overrides the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyLevel {
    User = 0,
    Machine = 1,
    Organisation = 2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveSetting {
    pub value: Value,
    pub level: PolicyLevel,
    pub locked: bool,
}

fn validate_key(key: &str) -> anyhow::Result<()> {
    ensure!(
        !key.is_empty()
            && key.len() <= 128
            && key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c)),
        "invalid setting key"
    );
    Ok(())
}

impl Library {
    /// This is a storage primitive. Machine and organisation writes require an admin
    /// command in the bus; secrets must go to SecretStore, never to this table.
    pub fn set_setting(
        &mut self,
        key: &str,
        value: &Value,
        level: PolicyLevel,
        locked: bool,
    ) -> anyhow::Result<()> {
        validate_key(key)?;
        ensure!(
            level != PolicyLevel::User || !locked,
            "a user cannot lock a policy"
        );
        let encoded = serde_json::to_string(value)?;
        ensure!(encoded.len() <= 64 * 1024, "setting is too large");
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if level == PolicyLevel::User {
            let locks: i64 = tx.query_row(
                "SELECT count(*) FROM settings WHERE key=?1 AND level>0 AND locked=1",
                [key],
                |r| r.get(0),
            )?;
            ensure!(locks == 0, "setting is locked by policy");
        }
        tx.execute("INSERT INTO settings VALUES (?1, ?2, ?3, ?4) ON CONFLICT(key, level) DO UPDATE SET value=excluded.value, locked=excluded.locked", params![key, level as i32, encoded, locked])?;
        tx.commit()?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> anyhow::Result<Option<EffectiveSetting>> {
        validate_key(key)?;
        let mut statement = self.db.prepare_cached("SELECT value, level, locked FROM settings WHERE key=?1 ORDER BY locked DESC, CASE WHEN locked=1 THEN level ELSE -level END DESC LIMIT 1")?;
        let mut rows = statement.query([key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let value: String = row.get(0)?;
        let level = match row.get::<_, i32>(1)? {
            0 => PolicyLevel::User,
            1 => PolicyLevel::Machine,
            _ => PolicyLevel::Organisation,
        };
        Ok(Some(EffectiveSetting {
            value: serde_json::from_str(&value)?,
            level,
            locked: row.get(2)?,
        }))
    }

    pub fn remove_setting(&mut self, key: &str, level: PolicyLevel) -> anyhow::Result<()> {
        validate_key(key)?;
        self.db.execute(
            "DELETE FROM settings WHERE key=?1 AND level=?2",
            params![key, level as i32],
        )?;
        Ok(())
    }
}
