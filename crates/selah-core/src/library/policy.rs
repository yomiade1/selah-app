use anyhow::ensure;
use rusqlite::{Transaction, TransactionBehavior, params};
use serde_json::Value;

use super::Library;

/// User values override defaults; locked machine/organisation policy overrides the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyLevel {
    User = 0,
    Machine = 1,
    Organisation = 2,
}

impl PolicyLevel {
    pub fn from_proto(value: i32) -> Option<Self> {
        match selah_proto::v1::PolicyLevel::try_from(value).ok()? {
            selah_proto::v1::PolicyLevel::User => Some(Self::User),
            selah_proto::v1::PolicyLevel::Machine => Some(Self::Machine),
            selah_proto::v1::PolicyLevel::Organisation => Some(Self::Organisation),
            selah_proto::v1::PolicyLevel::Unspecified => None,
        }
    }

    pub fn to_proto(self) -> selah_proto::v1::PolicyLevel {
        match self {
            Self::User => selah_proto::v1::PolicyLevel::User,
            Self::Machine => selah_proto::v1::PolicyLevel::Machine,
            Self::Organisation => selah_proto::v1::PolicyLevel::Organisation,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveSetting {
    pub value: Value,
    pub level: PolicyLevel,
    pub locked: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PolicyError {
    #[error("{0}")]
    Invalid(anyhow::Error),
    #[error("setting is locked by policy")]
    Locked,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
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

pub(crate) fn set_setting_in_tx(
    tx: &Transaction<'_>,
    key: &str,
    value: &Value,
    level: PolicyLevel,
    locked: bool,
) -> Result<(), PolicyError> {
    validate_key(key).map_err(PolicyError::Invalid)?;
    if level == PolicyLevel::User && locked {
        return Err(PolicyError::Locked);
    }
    let encoded = serde_json::to_string(value).map_err(|e| PolicyError::Invalid(e.into()))?;
    if encoded.len() > 64 * 1024 {
        return Err(PolicyError::Invalid(anyhow::anyhow!(
            "setting is too large"
        )));
    }
    if level == PolicyLevel::User {
        let locks: i64 = tx.query_row(
            "SELECT count(*) FROM settings WHERE key=?1 AND level>0 AND locked=1",
            [key],
            |r| r.get(0),
        )?;
        if locks > 0 {
            return Err(PolicyError::Locked);
        }
    }
    tx.execute(
        "INSERT INTO settings VALUES (?1, ?2, ?3, ?4) ON CONFLICT(key, level) DO UPDATE SET value=excluded.value, locked=excluded.locked",
        params![key, level as i32, encoded, locked],
    )?;
    Ok(())
}

pub(crate) fn remove_setting_in_tx(
    tx: &Transaction<'_>,
    key: &str,
    level: PolicyLevel,
) -> Result<(), PolicyError> {
    validate_key(key).map_err(PolicyError::Invalid)?;
    if level == PolicyLevel::User {
        let locks: i64 = tx.query_row(
            "SELECT count(*) FROM settings WHERE key=?1 AND level>0 AND locked=1",
            [key],
            |r| r.get(0),
        )?;
        if locks > 0 {
            return Err(PolicyError::Locked);
        }
    }
    tx.execute(
        "DELETE FROM settings WHERE key=?1 AND level=?2",
        params![key, level as i32],
    )?;
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
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        set_setting_in_tx(&tx, key, value, level, locked)?;
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
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_key(key)?;
        tx.execute(
            "DELETE FROM settings WHERE key=?1 AND level=?2",
            params![key, level as i32],
        )?;
        tx.commit()?;
        Ok(())
    }
}
