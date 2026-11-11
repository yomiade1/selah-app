use prost::Message;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use selah_proto::v1::*;

use super::policy::{PolicyError, PolicyLevel, remove_setting_in_tx, set_setting_in_tx};
use super::repository::{apply_snapshot_in_tx, delete_in_tx, read_snapshot_in_tx};
use super::{
    Document, DocumentKind, Library, MAX_DOCUMENT_BYTES, MediaStore, new_id, required_id,
    unix_millis, validate_id,
};

const MAX_IDEMPOTENCY_ROWS: i64 = 50_000;
const IDEMPOTENCY_PRUNE_INTERVAL: u64 = 256;

#[derive(Debug, thiserror::Error)]
pub enum CommandBusError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid library request: {0}")]
    Invalid(String),
    #[error("setting is locked by policy")]
    PolicyLocked,
    #[error("nothing to undo or redo")]
    NothingToUndo,
    #[error("conflicting command id")]
    ConflictingId,
    #[error("database error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl CommandBusError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "SEL-LIB-0001",
            Self::Invalid(_) | Self::Internal(_) => "SEL-LIB-0002",
            Self::PolicyLocked => "SEL-LIB-0003",
            Self::NothingToUndo => "SEL-LIB-0004",
            Self::ConflictingId => "SEL-IPC-0002",
        }
    }

    pub fn message_key(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "error-library-not-found",
            Self::Invalid(_) | Self::Internal(_) => "error-library-invalid",
            Self::PolicyLocked => "error-setting-locked",
            Self::NothingToUndo => "error-nothing-to-undo",
            Self::ConflictingId => "error-invalid-request",
        }
    }

    pub fn fix_article_id(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "troubleshooting.library-item-not-found",
            Self::Invalid(_) | Self::Internal(_) => "troubleshooting.cant-save-library-item",
            Self::PolicyLocked => "troubleshooting.setting-is-locked",
            Self::NothingToUndo => "troubleshooting.nothing-to-undo",
            Self::ConflictingId => "troubleshooting.selah-cant-do-that",
        }
    }
}

impl From<rusqlite::Error> for CommandBusError {
    fn from(error: rusqlite::Error) -> Self {
        match &error {
            rusqlite::Error::SqliteFailure(failure, message)
                if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Self::Invalid(
                    message
                        .clone()
                        .unwrap_or_else(|| "database constraint violated".into()),
                )
            }
            _ => Self::Internal(error.into()),
        }
    }
}

pub struct CommandMeta<'a> {
    pub actor: &'a Actor,
    pub envelope_id: u64,
    pub command_hash: [u8; 32],
    pub undo_group: Option<&'a Id>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MutationEventInfo {
    pub doc_type: DocumentKind,
    pub doc_id: Id,
    pub change: LibraryChangeKind,
    pub op_seq: u64,
    pub undo_group: Id,
    pub from_undo_redo: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExecutionOutcome {
    Replayed(envelope::Body),
    Applied {
        reply: reply::Result,
        changes: Vec<MutationEventInfo>,
        undo_doc_id: Option<Id>,
    },
}

fn origin_name(origin: i32) -> &'static str {
    match ActorOrigin::try_from(origin).unwrap_or(ActorOrigin::Unspecified) {
        ActorOrigin::Ui => "ui",
        ActorOrigin::Api => "api",
        ActorOrigin::Midi => "midi",
        ActorOrigin::Macro => "macro",
        ActorOrigin::Timeline => "timeline",
        ActorOrigin::Sync => "sync",
        ActorOrigin::Tour => "tour",
        ActorOrigin::Core => "core",
        ActorOrigin::Engine => "engine",
        ActorOrigin::Watchdog => "watchdog",
        ActorOrigin::Unspecified => "unspecified",
    }
}

pub(super) fn encode_op_state(present: bool, data: &[u8]) -> Vec<u8> {
    OpState {
        present,
        data: if present { data.to_vec() } else { Vec::new() },
    }
    .encode_to_vec()
}

pub(super) fn check_idempotency_readonly(
    db: &rusqlite::Connection,
    meta: &CommandMeta<'_>,
) -> Result<Option<envelope::Body>, CommandBusError> {
    if meta.envelope_id == 0 {
        return Ok(None);
    }
    let row: Option<(Vec<u8>, Vec<u8>)> = db
        .query_row(
            "SELECT command_hash, reply FROM command_idempotency WHERE actor_origin=?1 AND user_id=?2 AND device_id=?3 AND envelope_id=?4",
            params![
                meta.actor.origin,
                meta.actor.user_id,
                meta.actor.device_id,
                meta.envelope_id as i64
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((stored_hash, stored_reply)) = row else {
        return Ok(None);
    };
    if stored_hash != meta.command_hash.as_slice() {
        return Err(CommandBusError::ConflictingId);
    }
    let decoded = Envelope::decode(stored_reply.as_slice())
        .map_err(|e| CommandBusError::Internal(e.into()))?;
    let body = decoded
        .body
        .ok_or_else(|| CommandBusError::Internal(anyhow::anyhow!("empty cached reply")))?;
    Ok(Some(body))
}

pub(super) fn check_idempotency_in_tx(
    tx: &Transaction<'_>,
    meta: &CommandMeta<'_>,
) -> Result<Option<envelope::Body>, CommandBusError> {
    check_idempotency_readonly(tx, meta)
}

pub(super) fn record_idempotency_in_tx(
    tx: &Transaction<'_>,
    meta: &CommandMeta<'_>,
    reply: &reply::Result,
    ts: i64,
    writes: &mut u64,
) -> Result<(), CommandBusError> {
    if meta.envelope_id == 0 {
        return Ok(());
    }
    let envelope = Envelope {
        id: 0,
        correlation_id: 0,
        actor: None,
        body: Some(envelope::Body::Reply(Reply {
            result: Some(reply.clone()),
        })),
    };
    tx.execute(
        "INSERT INTO command_idempotency (actor_origin, user_id, device_id, envelope_id, command_hash, reply, ts) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            meta.actor.origin,
            meta.actor.user_id,
            meta.actor.device_id,
            meta.envelope_id as i64,
            meta.command_hash.as_slice(),
            envelope.encode_to_vec(),
            ts
        ],
    )?;
    *writes = writes.wrapping_add(1);
    if (*writes).is_multiple_of(IDEMPOTENCY_PRUNE_INTERVAL) {
        tx.execute(
            "DELETE FROM command_idempotency WHERE rowid IN (SELECT rowid FROM command_idempotency ORDER BY ts DESC, rowid DESC LIMIT -1 OFFSET ?1)",
            params![MAX_IDEMPOTENCY_ROWS],
        )?;
    }
    Ok(())
}

pub(super) fn resolve_undo_group(undo_group: Option<&Id>) -> Result<Vec<u8>, CommandBusError> {
    match undo_group {
        Some(id) => {
            validate_id(id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
            Ok(id.value.clone())
        }
        None => Ok(new_id().value),
    }
}

fn undo_state_in_tx(
    tx: &Transaction<'_>,
    doc_id: Option<&[u8]>,
) -> Result<UndoState, CommandBusError> {
    let (undo_depth, redo_depth): (i64, i64) = match doc_id {
        Some(id) => (
            tx.query_row(
                "SELECT COUNT(DISTINCT undo_group) FROM undo_entries WHERE doc_id=?1 AND undone=0",
                [id],
                |r| r.get(0),
            )?,
            tx.query_row(
                "SELECT COUNT(DISTINCT undone) FROM undo_entries WHERE doc_id=?1 AND undone>0",
                [id],
                |r| r.get(0),
            )?,
        ),
        None => (
            tx.query_row(
                "SELECT COUNT(DISTINCT undo_group) FROM undo_entries WHERE undone=0",
                [],
                |r| r.get(0),
            )?,
            tx.query_row(
                "SELECT COUNT(DISTINCT undone) FROM undo_entries WHERE undone>0",
                [],
                |r| r.get(0),
            )?,
        ),
    };
    let undo_depth = u32::try_from(undo_depth).unwrap_or(u32::MAX);
    let redo_depth = u32::try_from(redo_depth).unwrap_or(u32::MAX);
    Ok(UndoState {
        can_undo: undo_depth > 0,
        can_redo: redo_depth > 0,
        undo_depth,
        redo_depth,
    })
}

pub(super) struct NewOp<'a> {
    pub ts: i64,
    pub actor: &'a Actor,
    pub kind: DocumentKind,
    pub doc_id: &'a [u8],
    pub command: &'a [u8],
    pub inverse: &'a [u8],
    pub undo_group: &'a [u8],
}

pub(super) fn insert_op_in_tx(tx: &Transaction<'_>, op: NewOp<'_>) -> Result<u64, CommandBusError> {
    let op_id = new_id().value;
    tx.execute(
        "INSERT INTO ops (op_id, ts, actor, origin, doc_type, doc_id, command, inverse, undo_group, sync_status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)",
        params![
            op_id,
            op.ts,
            op.actor.encode_to_vec(),
            origin_name(op.actor.origin),
            op.kind.as_str(),
            op.doc_id,
            op.command,
            op.inverse,
            op.undo_group,
        ],
    )?;
    Ok(tx.last_insert_rowid() as u64)
}

pub(super) fn apply_doc_bus_error(err: anyhow::Error) -> CommandBusError {
    if let Some(sql_err) = err.downcast_ref::<rusqlite::Error>() {
        return match sql_err {
            rusqlite::Error::SqliteFailure(failure, message)
                if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                CommandBusError::Invalid(
                    message
                        .clone()
                        .unwrap_or_else(|| "database constraint violated".into()),
                )
            }
            other => CommandBusError::Internal(anyhow::anyhow!(other.to_string())),
        };
    }
    CommandBusError::Invalid(err.to_string())
}

impl Library {
    pub fn save_document_op<T: Document>(
        &mut self,
        document: &T,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        self.save_document_with_reply_op(document, meta, reply::Result::Acknowledged(true))
    }

    pub(crate) fn save_document_with_reply_op<T: Document>(
        &mut self,
        document: &T,
        meta: &CommandMeta<'_>,
        reply: reply::Result,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(replayed) = check_idempotency_in_tx(&tx, meta)? {
            tx.commit()?;
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        if document.encoded_len() > MAX_DOCUMENT_BYTES {
            return Err(CommandBusError::Invalid("document is too large".into()));
        }
        document
            .validate()
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let doc_id = required_id(document.id())
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?
            .to_vec();
        let group = resolve_undo_group(meta.undo_group)?;
        let forward_data = document.encode_to_vec();

        if T::KIND == DocumentKind::Media {
            let item = MediaItem::decode(forward_data.as_slice())
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
            MediaStore::open(&self.paths.media)
                .map_err(CommandBusError::Internal)?
                .verify(&item.content_hash, &item.extension, item.size_bytes)
                .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        }

        let before = read_snapshot_in_tx(&tx, T::KIND, &doc_id)?;
        document.write(&tx).map_err(apply_doc_bus_error)?;

        tx.execute(
            "DELETE FROM undo_entries WHERE doc_id=?1 AND undone>0",
            [&doc_id],
        )?;

        let ts = unix_millis();
        let (inverse_present, inverse_data) = match before {
            Some(bytes) => (true, bytes),
            None => (false, Vec::new()),
        };
        let cmd_blob = encode_op_state(true, &forward_data);
        let inv_blob = encode_op_state(inverse_present, &inverse_data);
        let op_seq = insert_op_in_tx(
            &tx,
            NewOp {
                ts,
                actor: meta.actor,
                kind: T::KIND,
                doc_id: &doc_id,
                command: &cmd_blob,
                inverse: &inv_blob,
                undo_group: &group,
            },
        )?;
        tx.execute(
            "INSERT INTO undo_entries (op_seq, undo_group, doc_type, doc_id, forward_present, forward_data, inverse_present, inverse_data, undone) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, 0)",
            params![
                op_seq as i64,
                &group,
                T::KIND.as_str(),
                &doc_id,
                forward_data,
                inverse_present as i32,
                inverse_data,
            ],
        )?;

        record_idempotency_in_tx(&tx, meta, &reply, ts, &mut self.idempotency_writes)?;
        tx.commit()?;

        let doc_id_msg = Id { value: doc_id };
        Ok(ExecutionOutcome::Applied {
            reply,
            changes: vec![MutationEventInfo {
                doc_type: T::KIND,
                doc_id: doc_id_msg.clone(),
                change: LibraryChangeKind::Saved,
                op_seq,
                undo_group: Id { value: group },
                from_undo_redo: false,
            }],
            undo_doc_id: Some(doc_id_msg),
        })
    }

    pub fn delete_document_op(
        &mut self,
        kind: DocumentKind,
        id: &Id,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(replayed) = check_idempotency_in_tx(&tx, meta)? {
            tx.commit()?;
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        validate_id(id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let group = resolve_undo_group(meta.undo_group)?;

        let Some(before) = read_snapshot_in_tx(&tx, kind, &id.value)? else {
            return Err(CommandBusError::NotFound("library item not found".into()));
        };
        delete_in_tx(&tx, kind, &id.value).map_err(apply_doc_bus_error)?;

        tx.execute(
            "DELETE FROM undo_entries WHERE doc_id=?1 AND undone>0",
            [&id.value],
        )?;

        let ts = unix_millis();
        let cmd_blob = encode_op_state(false, &[]);
        let inv_blob = encode_op_state(true, &before);
        let op_seq = insert_op_in_tx(
            &tx,
            NewOp {
                ts,
                actor: meta.actor,
                kind,
                doc_id: &id.value,
                command: &cmd_blob,
                inverse: &inv_blob,
                undo_group: &group,
            },
        )?;
        tx.execute(
            "INSERT INTO undo_entries (op_seq, undo_group, doc_type, doc_id, forward_present, forward_data, inverse_present, inverse_data, undone) VALUES (?1, ?2, ?3, ?4, 0, ?5, 1, ?6, 0)",
            params![
                op_seq as i64,
                &group,
                kind.as_str(),
                &id.value,
                Vec::<u8>::new(),
                before,
            ],
        )?;

        let reply = reply::Result::Acknowledged(true);
        record_idempotency_in_tx(&tx, meta, &reply, ts, &mut self.idempotency_writes)?;
        tx.commit()?;

        Ok(ExecutionOutcome::Applied {
            reply,
            changes: vec![MutationEventInfo {
                doc_type: kind,
                doc_id: id.clone(),
                change: LibraryChangeKind::Deleted,
                op_seq,
                undo_group: Id { value: group },
                from_undo_redo: false,
            }],
            undo_doc_id: Some(id.clone()),
        })
    }

    pub fn undo_op(
        &mut self,
        doc_id: Option<&Id>,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(replayed) = check_idempotency_in_tx(&tx, meta)? {
            tx.commit()?;
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        if let Some(id) = doc_id {
            validate_id(id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        }

        let target_group: Option<Vec<u8>> = match doc_id {
            Some(id) => tx
                .query_row(
                    "SELECT undo_group FROM undo_entries WHERE doc_id=?1 AND undone=0 ORDER BY entry_seq DESC LIMIT 1",
                    [&id.value],
                    |r| r.get(0),
                )
                .optional()?,
            None => tx
                .query_row(
                    "SELECT undo_group FROM undo_entries WHERE undone=0 ORDER BY entry_seq DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .optional()?,
        };
        let Some(group) = target_group else {
            return Err(CommandBusError::NothingToUndo);
        };

        type Row = (i64, String, Vec<u8>, bool, Vec<u8>, bool, Vec<u8>);
        let entries: Vec<Row> = match doc_id {
            Some(id) => {
                let mut stmt = tx.prepare(
                    "SELECT entry_seq, doc_type, doc_id, inverse_present, inverse_data, forward_present, forward_data FROM undo_entries WHERE doc_id=?1 AND undo_group=?2 AND undone=0 ORDER BY entry_seq DESC",
                )?;
                stmt.query_map(params![&id.value, &group], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get::<_, i32>(3)? != 0,
                        r.get(4)?,
                        r.get::<_, i32>(5)? != 0,
                        r.get(6)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
            }
            None => {
                let mut stmt = tx.prepare(
                    "SELECT entry_seq, doc_type, doc_id, inverse_present, inverse_data, forward_present, forward_data FROM undo_entries WHERE undo_group=?1 AND undone=0 ORDER BY entry_seq DESC",
                )?;
                stmt.query_map([&group], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get::<_, i32>(3)? != 0,
                        r.get(4)?,
                        r.get::<_, i32>(5)? != 0,
                        r.get(6)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
            }
        };

        let next_undone: i64 = tx.query_row(
            "SELECT COALESCE(MAX(undone), 0) + 1 FROM undo_entries",
            [],
            |r| r.get(0),
        )?;
        let ts = unix_millis();
        let mut changes = Vec::with_capacity(entries.len());

        for (entry_seq, doc_type_str, entry_doc_id, inv_present, inv_data, fwd_present, fwd_data) in
            entries
        {
            let kind = DocumentKind::parse(&doc_type_str).ok_or_else(|| {
                CommandBusError::Internal(anyhow::anyhow!("unknown document type {doc_type_str}"))
            })?;
            apply_snapshot_in_tx(
                &self.paths.media,
                &tx,
                kind,
                &entry_doc_id,
                inv_present,
                &inv_data,
            )
            .map_err(apply_doc_bus_error)?;
            tx.execute(
                "UPDATE undo_entries SET undone=?1 WHERE entry_seq=?2",
                params![next_undone, entry_seq],
            )?;
            let cmd_blob = encode_op_state(inv_present, &inv_data);
            let inv_blob = encode_op_state(fwd_present, &fwd_data);
            let op_seq = insert_op_in_tx(
                &tx,
                NewOp {
                    ts,
                    actor: meta.actor,
                    kind,
                    doc_id: &entry_doc_id,
                    command: &cmd_blob,
                    inverse: &inv_blob,
                    undo_group: &group,
                },
            )?;
            changes.push(MutationEventInfo {
                doc_type: kind,
                doc_id: Id {
                    value: entry_doc_id,
                },
                change: if inv_present {
                    LibraryChangeKind::Saved
                } else {
                    LibraryChangeKind::Deleted
                },
                op_seq,
                undo_group: Id {
                    value: group.clone(),
                },
                from_undo_redo: true,
            });
        }

        let state = undo_state_in_tx(&tx, doc_id.map(|id| id.value.as_slice()))?;
        let reply = reply::Result::UndoState(state);
        record_idempotency_in_tx(&tx, meta, &reply, ts, &mut self.idempotency_writes)?;
        tx.commit()?;

        Ok(ExecutionOutcome::Applied {
            reply,
            changes,
            undo_doc_id: doc_id.cloned(),
        })
    }

    pub fn redo_op(
        &mut self,
        doc_id: Option<&Id>,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(replayed) = check_idempotency_in_tx(&tx, meta)? {
            tx.commit()?;
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        if let Some(id) = doc_id {
            validate_id(id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        }

        let target: Option<(i64, Vec<u8>)> = match doc_id {
            Some(id) => tx
                .query_row(
                    "SELECT undone, undo_group FROM undo_entries WHERE doc_id=?1 AND undone>0 ORDER BY undone DESC, entry_seq ASC LIMIT 1",
                    [&id.value],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?,
            None => tx
                .query_row(
                    "SELECT undone, undo_group FROM undo_entries WHERE undone>0 ORDER BY undone DESC, entry_seq ASC LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?,
        };
        let Some((undone_gen, group)) = target else {
            return Err(CommandBusError::NothingToUndo);
        };

        type Row = (i64, String, Vec<u8>, bool, Vec<u8>, bool, Vec<u8>);
        let entries: Vec<Row> = match doc_id {
            Some(id) => {
                let mut stmt = tx.prepare(
                    "SELECT entry_seq, doc_type, doc_id, forward_present, forward_data, inverse_present, inverse_data FROM undo_entries WHERE doc_id=?1 AND undone=?2 AND undo_group=?3 ORDER BY entry_seq ASC",
                )?;
                stmt.query_map(params![&id.value, undone_gen, &group], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get::<_, i32>(3)? != 0,
                        r.get(4)?,
                        r.get::<_, i32>(5)? != 0,
                        r.get(6)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
            }
            None => {
                let mut stmt = tx.prepare(
                    "SELECT entry_seq, doc_type, doc_id, forward_present, forward_data, inverse_present, inverse_data FROM undo_entries WHERE undone=?1 AND undo_group=?2 ORDER BY entry_seq ASC",
                )?;
                stmt.query_map(params![undone_gen, &group], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get::<_, i32>(3)? != 0,
                        r.get(4)?,
                        r.get::<_, i32>(5)? != 0,
                        r.get(6)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
            }
        };

        let ts = unix_millis();
        let mut changes = Vec::with_capacity(entries.len());

        for (entry_seq, doc_type_str, entry_doc_id, fwd_present, fwd_data, inv_present, inv_data) in
            entries
        {
            let kind = DocumentKind::parse(&doc_type_str).ok_or_else(|| {
                CommandBusError::Internal(anyhow::anyhow!("unknown document type {doc_type_str}"))
            })?;
            apply_snapshot_in_tx(
                &self.paths.media,
                &tx,
                kind,
                &entry_doc_id,
                fwd_present,
                &fwd_data,
            )
            .map_err(apply_doc_bus_error)?;
            tx.execute(
                "UPDATE undo_entries SET undone=0 WHERE entry_seq=?1",
                [entry_seq],
            )?;
            let cmd_blob = encode_op_state(fwd_present, &fwd_data);
            let inv_blob = encode_op_state(inv_present, &inv_data);
            let op_seq = insert_op_in_tx(
                &tx,
                NewOp {
                    ts,
                    actor: meta.actor,
                    kind,
                    doc_id: &entry_doc_id,
                    command: &cmd_blob,
                    inverse: &inv_blob,
                    undo_group: &group,
                },
            )?;
            changes.push(MutationEventInfo {
                doc_type: kind,
                doc_id: Id {
                    value: entry_doc_id,
                },
                change: if fwd_present {
                    LibraryChangeKind::Saved
                } else {
                    LibraryChangeKind::Deleted
                },
                op_seq,
                undo_group: Id {
                    value: group.clone(),
                },
                from_undo_redo: true,
            });
        }

        let state = undo_state_in_tx(&tx, doc_id.map(|id| id.value.as_slice()))?;
        let reply = reply::Result::UndoState(state);
        record_idempotency_in_tx(&tx, meta, &reply, ts, &mut self.idempotency_writes)?;
        tx.commit()?;

        Ok(ExecutionOutcome::Applied {
            reply,
            changes,
            undo_doc_id: doc_id.cloned(),
        })
    }

    pub fn undo_state(&self, doc_id: Option<&Id>) -> Result<UndoState, CommandBusError> {
        if let Some(id) = doc_id {
            validate_id(id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        }
        let (undo_depth, redo_depth): (i64, i64) = match doc_id {
            Some(id) => (
                self.db.query_row(
                    "SELECT COUNT(DISTINCT undo_group) FROM undo_entries WHERE doc_id=?1 AND undone=0",
                    [&id.value],
                    |r| r.get(0),
                )?,
                self.db.query_row(
                    "SELECT COUNT(DISTINCT undone) FROM undo_entries WHERE doc_id=?1 AND undone>0",
                    [&id.value],
                    |r| r.get(0),
                )?,
            ),
            None => (
                self.db.query_row(
                    "SELECT COUNT(DISTINCT undo_group) FROM undo_entries WHERE undone=0",
                    [],
                    |r| r.get(0),
                )?,
                self.db.query_row(
                    "SELECT COUNT(DISTINCT undone) FROM undo_entries WHERE undone>0",
                    [],
                    |r| r.get(0),
                )?,
            ),
        };
        let undo_depth = u32::try_from(undo_depth).unwrap_or(u32::MAX);
        let redo_depth = u32::try_from(redo_depth).unwrap_or(u32::MAX);
        Ok(UndoState {
            can_undo: undo_depth > 0,
            can_redo: redo_depth > 0,
            undo_depth,
            redo_depth,
        })
    }

    pub fn save_editor_buffer_op(
        &mut self,
        doc_id: &Id,
        doc_type: i32,
        content: &str,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(replayed) = check_idempotency_in_tx(&tx, meta)? {
            tx.commit()?;
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        validate_id(doc_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let kind = DocumentKind::from_proto(doc_type)
            .ok_or_else(|| CommandBusError::Invalid("unknown document type".into()))?;
        if content.len() > MAX_DOCUMENT_BYTES || content.contains('\0') {
            return Err(CommandBusError::Invalid("invalid editor buffer".into()));
        }

        let ts = unix_millis();
        tx.execute(
            "INSERT INTO editor_buffers (doc_id, content, updated_at, doc_type) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(doc_id) DO UPDATE SET content=excluded.content, updated_at=excluded.updated_at, doc_type=excluded.doc_type",
            params![&doc_id.value, content, ts, kind.as_str()],
        )?;

        let reply = reply::Result::Acknowledged(true);
        record_idempotency_in_tx(&tx, meta, &reply, ts, &mut self.idempotency_writes)?;
        tx.commit()?;

        Ok(ExecutionOutcome::Applied {
            reply,
            changes: Vec::new(),
            undo_doc_id: None,
        })
    }

    pub fn discard_editor_buffer_op(
        &mut self,
        doc_id: &Id,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(replayed) = check_idempotency_in_tx(&tx, meta)? {
            tx.commit()?;
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        validate_id(doc_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let ts = unix_millis();
        tx.execute(
            "DELETE FROM editor_buffers WHERE doc_id=?1",
            [&doc_id.value],
        )?;

        let reply = reply::Result::Acknowledged(true);
        record_idempotency_in_tx(&tx, meta, &reply, ts, &mut self.idempotency_writes)?;
        tx.commit()?;

        Ok(ExecutionOutcome::Applied {
            reply,
            changes: Vec::new(),
            undo_doc_id: None,
        })
    }

    pub fn get_editor_buffer(&self, doc_id: &Id) -> Result<EditorBufferReply, CommandBusError> {
        validate_id(doc_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let row: Option<(String, String, i64)> = self
            .db
            .query_row(
                "SELECT doc_type, content, updated_at FROM editor_buffers WHERE doc_id=?1",
                [&doc_id.value],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        Ok(match row {
            Some((doc_type_str, content, updated_at)) => {
                let doc_type = DocumentKind::parse(&doc_type_str)
                    .map(|k| k.to_proto() as i32)
                    .unwrap_or(DocumentType::Unspecified as i32);
                EditorBufferReply {
                    found: true,
                    buffer: Some(EditorBuffer {
                        doc_id: Some(doc_id.clone()),
                        doc_type,
                        content,
                        updated_at: Some(Timestamp {
                            unix_milliseconds: updated_at,
                        }),
                    }),
                }
            }
            None => EditorBufferReply {
                found: false,
                buffer: None,
            },
        })
    }

    pub fn list_editor_buffers(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<EditorBufferList, CommandBusError> {
        if !(1..=200).contains(&limit) {
            return Err(CommandBusError::Invalid(
                "page size must be 1 to 200".into(),
            ));
        }
        let mut stmt = self.db.prepare_cached(
            "SELECT doc_id, doc_type, content, updated_at FROM editor_buffers ORDER BY updated_at DESC, doc_id ASC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt
            .query_map(params![limit, offset], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let buffers = rows
            .into_iter()
            .map(|(doc_id, doc_type_str, content, updated_at)| EditorBuffer {
                doc_id: Some(Id { value: doc_id }),
                doc_type: DocumentKind::parse(&doc_type_str)
                    .map(|k| k.to_proto() as i32)
                    .unwrap_or(DocumentType::Unspecified as i32),
                content,
                updated_at: Some(Timestamp {
                    unix_milliseconds: updated_at,
                }),
            })
            .collect();
        Ok(EditorBufferList { buffers })
    }

    pub fn record_feedback_op(
        &mut self,
        article_id: &str,
        helpful: bool,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(replayed) = check_idempotency_in_tx(&tx, meta)? {
            tx.commit()?;
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        let trimmed = article_id.trim();
        if trimmed.is_empty() || trimmed.len() > 256 || trimmed.contains('\0') {
            return Err(CommandBusError::Invalid("invalid article id".into()));
        }
        let ts = unix_millis();
        tx.execute(
            "INSERT INTO help_feedback (article_id, helpful, ts) VALUES (?1, ?2, ?3)",
            params![trimmed, helpful as i32, ts],
        )?;

        let reply = reply::Result::Acknowledged(true);
        record_idempotency_in_tx(&tx, meta, &reply, ts, &mut self.idempotency_writes)?;
        tx.commit()?;

        Ok(ExecutionOutcome::Applied {
            reply,
            changes: Vec::new(),
            undo_doc_id: None,
        })
    }

    pub fn help_feedback_count(
        &self,
        article_id: &str,
        helpful: Option<bool>,
    ) -> anyhow::Result<u64> {
        let count: i64 = match helpful {
            Some(flag) => self.db.query_row(
                "SELECT count(*) FROM help_feedback WHERE article_id=?1 AND helpful=?2",
                params![article_id, flag as i32],
                |r| r.get(0),
            )?,
            None => self.db.query_row(
                "SELECT count(*) FROM help_feedback WHERE article_id=?1",
                [article_id],
                |r| r.get(0),
            )?,
        };
        Ok(u64::try_from(count).unwrap_or(0))
    }

    pub fn set_policy_setting_op(
        &mut self,
        key: &str,
        value_json: &str,
        level: i32,
        locked: bool,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(replayed) = check_idempotency_in_tx(&tx, meta)? {
            tx.commit()?;
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        let level = PolicyLevel::from_proto(level)
            .ok_or_else(|| CommandBusError::Invalid("unknown policy level".into()))?;
        let value: serde_json::Value = serde_json::from_str(value_json)
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;

        match set_setting_in_tx(&tx, key, &value, level, locked) {
            Ok(()) => {}
            Err(PolicyError::Locked) => return Err(CommandBusError::PolicyLocked),
            Err(PolicyError::Invalid(e)) => return Err(CommandBusError::Invalid(e.to_string())),
            Err(PolicyError::Db(e)) => return Err(e.into()),
        }

        let ts = unix_millis();
        let reply = reply::Result::Acknowledged(true);
        record_idempotency_in_tx(&tx, meta, &reply, ts, &mut self.idempotency_writes)?;
        tx.commit()?;

        Ok(ExecutionOutcome::Applied {
            reply,
            changes: Vec::new(),
            undo_doc_id: None,
        })
    }

    pub fn remove_policy_setting_op(
        &mut self,
        key: &str,
        level: i32,
        meta: &CommandMeta<'_>,
    ) -> Result<ExecutionOutcome, CommandBusError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(replayed) = check_idempotency_in_tx(&tx, meta)? {
            tx.commit()?;
            return Ok(ExecutionOutcome::Replayed(replayed));
        }

        let level = PolicyLevel::from_proto(level)
            .ok_or_else(|| CommandBusError::Invalid("unknown policy level".into()))?;
        match remove_setting_in_tx(&tx, key, level) {
            Ok(()) => {}
            Err(PolicyError::Locked) => return Err(CommandBusError::PolicyLocked),
            Err(PolicyError::Invalid(e)) => return Err(CommandBusError::Invalid(e.to_string())),
            Err(PolicyError::Db(e)) => return Err(e.into()),
        }

        let ts = unix_millis();
        let reply = reply::Result::Acknowledged(true);
        record_idempotency_in_tx(&tx, meta, &reply, ts, &mut self.idempotency_writes)?;
        tx.commit()?;

        Ok(ExecutionOutcome::Applied {
            reply,
            changes: Vec::new(),
            undo_doc_id: None,
        })
    }

    pub fn list_ops(
        &self,
        after_seq: u64,
        limit: u32,
        doc_id: Option<&Id>,
    ) -> Result<OpList, CommandBusError> {
        if !(1..=200).contains(&limit) {
            return Err(CommandBusError::Invalid(
                "page size must be 1 to 200".into(),
            ));
        }
        if let Some(id) = doc_id {
            validate_id(id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        }
        type Row = (
            i64,
            Vec<u8>,
            i64,
            Vec<u8>,
            String,
            String,
            Vec<u8>,
            Vec<u8>,
            Vec<u8>,
            Vec<u8>,
            i32,
        );
        let rows: Vec<Row> = match doc_id {
            Some(id) => {
                let mut stmt = self.db.prepare_cached(
                    "SELECT seq, op_id, ts, actor, origin, doc_type, doc_id, command, inverse, undo_group, sync_status FROM ops WHERE seq>?1 AND doc_id=?2 ORDER BY seq ASC LIMIT ?3",
                )?;
                stmt.query_map(params![after_seq as i64, &id.value, limit], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                        r.get(9)?,
                        r.get(10)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
            }
            None => {
                let mut stmt = self.db.prepare_cached(
                    "SELECT seq, op_id, ts, actor, origin, doc_type, doc_id, command, inverse, undo_group, sync_status FROM ops WHERE seq>?1 ORDER BY seq ASC LIMIT ?2",
                )?;
                stmt.query_map(params![after_seq as i64, limit], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                        r.get(9)?,
                        r.get(10)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
            }
        };

        let ops = rows
            .into_iter()
            .map(
                |(
                    seq,
                    op_id,
                    ts,
                    actor_blob,
                    origin,
                    doc_type_str,
                    row_doc_id,
                    command,
                    inverse,
                    undo_group,
                    sync_status,
                )| {
                    let actor = Actor::decode(actor_blob.as_slice()).unwrap_or_default();
                    let doc_type = DocumentKind::parse(&doc_type_str)
                        .map(|k| k.to_proto() as i32)
                        .unwrap_or(DocumentType::Unspecified as i32);
                    OpRecord {
                        seq: seq as u64,
                        op_id: Some(Id { value: op_id }),
                        timestamp: Some(Timestamp {
                            unix_milliseconds: ts,
                        }),
                        user_id: actor.user_id,
                        device_id: actor.device_id,
                        origin,
                        doc_type,
                        doc_id: Some(Id { value: row_doc_id }),
                        command,
                        inverse,
                        undo_group: Some(Id { value: undo_group }),
                        sync_status,
                    }
                },
            )
            .collect();
        Ok(OpList { ops })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_actor(origin: ActorOrigin, device_id: &str) -> Actor {
        Actor {
            user_id: "local-owner".into(),
            device_id: device_id.into(),
            origin: origin as i32,
        }
    }

    fn meta<'a>(
        actor: &'a Actor,
        envelope_id: u64,
        undo_group: Option<&'a Id>,
        salt: &[u8],
    ) -> CommandMeta<'a> {
        CommandMeta {
            actor,
            envelope_id,
            command_hash: *blake3::hash(salt).as_bytes(),
            undo_group,
        }
    }

    fn sample_song(id: Id, title: &str, verse_text: &str) -> Song {
        Song {
            id: Some(id),
            title: title.into(),
            authors: vec!["Author".into()],
            tags: vec!["Hymn".into()],
            public_domain: true,
            sections: vec![Section {
                id: Some(new_id()),
                label: "Verse 1".into(),
                kind: SectionKind::Verse as i32,
                lines: vec![LyricLine {
                    text: verse_text.into(),
                    chords: vec![],
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn make_random_song(rng: &mut fastrand::Rng, id: Id) -> Song {
        let sec_id = new_id();
        let line_text = format!("Lyric line {}", rng.u32(1..10_000));
        let char_len = line_text.chars().count() as u32;
        let chords = if rng.bool() {
            vec![Chord {
                position: rng.u32(0..=char_len),
                text: ["C", "G/B", "Am7", "F"][rng.usize(0..4)].into(),
            }]
        } else {
            Vec::new()
        };
        let include_arr = rng.bool();
        Song {
            id: Some(id),
            title: format!("Song {}", rng.u32(1..100_000)),
            authors: vec![format!("Writer {}", rng.u32(1..100))],
            tags: vec![format!("Tag {}", rng.u32(1..20))],
            public_domain: rng.bool(),
            tempo: 60.0 + f64::from(rng.u16(0..120)),
            sections: vec![Section {
                id: Some(sec_id.clone()),
                label: "Verse 1".into(),
                kind: SectionKind::Verse as i32,
                lines: vec![LyricLine {
                    text: line_text,
                    chords,
                }],
                ..Default::default()
            }],
            arrangements: if include_arr {
                vec![Arrangement {
                    id: Some(new_id()),
                    name: "Default".into(),
                    section_ids: vec![sec_id],
                }]
            } else {
                Vec::new()
            },
            ..Default::default()
        }
    }

    fn make_random_template(rng: &mut fastrand::Rng, id: Id) -> Template {
        Template {
            id: Some(id),
            name: format!("Template {}", rng.u32(1..100_000)),
            variants: vec![TemplateVariant {
                id: Some(new_id()),
                resolution: Some(Resolution {
                    width: 1920,
                    height: 1080,
                }),
                max_lines: rng.u32(2..8),
                ..Default::default()
            }],
        }
    }

    fn make_random_presentation(rng: &mut fastrand::Rng, id: Id) -> Presentation {
        Presentation {
            id: Some(id),
            name: format!("Presentation {}", rng.u32(1..100_000)),
            slides: vec![Slide {
                id: Some(new_id()),
                text: format!("Slide {}", rng.u32(1..10_000)),
                ..Default::default()
            }],
            template_id: None,
        }
    }

    fn make_random_tag(rng: &mut fastrand::Rng, id: Id) -> Tag {
        Tag {
            id: Some(id),
            name: format!("Tag {}", rng.u32(1..100_000)),
        }
    }

    #[derive(Debug, PartialEq)]
    struct LibrarySnapshot {
        songs: Vec<Song>,
        templates: Vec<Template>,
        presentations: Vec<Presentation>,
        tags: Vec<Tag>,
    }

    fn take_snapshot(library: &Library) -> LibrarySnapshot {
        LibrarySnapshot {
            songs: library.list::<Song>(200, 0).unwrap(),
            templates: library.list::<Template>(200, 0).unwrap(),
            presentations: library.list::<Presentation>(200, 0).unwrap(),
            tags: library.list::<Tag>(200, 0).unwrap(),
        }
    }

    fn reset_in_memory(library: &mut Library) {
        library
            .db
            .execute_batch(
                "DELETE FROM undo_entries;
                 DELETE FROM ops;
                 DELETE FROM command_idempotency;
                 DELETE FROM songs;
                 DELETE FROM songs_fts;
                 DELETE FROM songs_trigram;
                 DELETE FROM media_fts;
                 DELETE FROM media_trigram;
                 DELETE FROM templates;
                 DELETE FROM presentations;
                 DELETE FROM tags;
                 DELETE FROM editor_buffers;
                 DELETE FROM help_feedback;
                 DELETE FROM settings;",
            )
            .unwrap();
    }

    #[test]
    fn m2_a2_ten_thousand_random_op_sequences_round_trip_undo_and_redo() {
        let mut library = Library::open_in_memory().unwrap();
        let actor = test_actor(ActorOrigin::Ui, "prop");
        let mut rng = fastrand::Rng::with_seed(0x5E1A_A200_0002);
        let mut envelope_id = 1_u64;

        for seq_idx in 0..10_000_usize {
            reset_in_memory(&mut library);

            // Up to 4 document slots per sequence (one of each doc type exercised across runs).
            let slot_kinds = [
                DocumentKind::Song,
                DocumentKind::Template,
                DocumentKind::Presentation,
                DocumentKind::Tag,
            ];
            let slots: Vec<(DocumentKind, Id)> =
                slot_kinds.iter().map(|&kind| (kind, new_id())).collect();
            let mut present = [false; 4];

            // Seed 0..=2 pre-existing documents so deletes/updates also start from non-empty state.
            let seed_count = rng.usize(0..=2);
            for _ in 0..seed_count {
                let slot = rng.usize(0..slots.len());
                let (kind, ref id) = slots[slot];
                match kind {
                    DocumentKind::Song => library
                        .save(&make_random_song(&mut rng, id.clone()))
                        .unwrap(),
                    DocumentKind::Template => library
                        .save(&make_random_template(&mut rng, id.clone()))
                        .unwrap(),
                    DocumentKind::Presentation => library
                        .save(&make_random_presentation(&mut rng, id.clone()))
                        .unwrap(),
                    DocumentKind::Tag => library
                        .save(&make_random_tag(&mut rng, id.clone()))
                        .unwrap(),
                    _ => unreachable!(),
                }
                present[slot] = true;
            }

            let initial = take_snapshot(&library);
            let op_count = rng.usize(1..=6);
            let mut active_group: Option<Id> = None;

            for step in 0..op_count {
                let slot = rng.usize(0..slots.len());
                let (kind, ref id) = slots[slot];
                let do_delete = present[slot] && rng.u8(0..4) == 0;
                // 30% chance to keep the previous undo group so multi-op / same-doc groups are covered.
                let group = if step > 0 && rng.u8(0..10) < 3 {
                    active_group.get_or_insert_with(new_id).clone()
                } else {
                    let fresh = new_id();
                    active_group = Some(fresh.clone());
                    fresh
                };
                envelope_id += 1;
                let cmd_meta = meta(
                    &actor,
                    envelope_id,
                    Some(&group),
                    &envelope_id.to_le_bytes(),
                );

                if do_delete {
                    library.delete_document_op(kind, id, &cmd_meta).unwrap();
                    present[slot] = false;
                } else {
                    match kind {
                        DocumentKind::Song => library
                            .save_document_op(&make_random_song(&mut rng, id.clone()), &cmd_meta)
                            .unwrap(),
                        DocumentKind::Template => library
                            .save_document_op(
                                &make_random_template(&mut rng, id.clone()),
                                &cmd_meta,
                            )
                            .unwrap(),
                        DocumentKind::Presentation => library
                            .save_document_op(
                                &make_random_presentation(&mut rng, id.clone()),
                                &cmd_meta,
                            )
                            .unwrap(),
                        DocumentKind::Tag => library
                            .save_document_op(&make_random_tag(&mut rng, id.clone()), &cmd_meta)
                            .unwrap(),
                        _ => unreachable!(),
                    };
                    present[slot] = true;
                }
            }

            let final_snap = take_snapshot(&library);
            let mut undo_steps = 0_usize;
            while library.undo_state(None).unwrap().can_undo {
                envelope_id += 1;
                let cmd_meta = meta(&actor, envelope_id, None, &envelope_id.to_le_bytes());
                library.undo_op(None, &cmd_meta).unwrap();
                undo_steps += 1;
                assert!(
                    undo_steps <= op_count + 1,
                    "sequence {seq_idx} did not terminate undo"
                );
            }
            assert_eq!(
                take_snapshot(&library),
                initial,
                "sequence {seq_idx} undo did not restore initial state"
            );
            let after_undo_state = library.undo_state(None).unwrap();
            assert!(!after_undo_state.can_undo);
            assert!(after_undo_state.can_redo);
            assert_eq!(after_undo_state.redo_depth as usize, undo_steps);

            let mut redo_steps = 0_usize;
            while library.undo_state(None).unwrap().can_redo {
                envelope_id += 1;
                let cmd_meta = meta(&actor, envelope_id, None, &envelope_id.to_le_bytes());
                library.redo_op(None, &cmd_meta).unwrap();
                redo_steps += 1;
                assert!(
                    redo_steps <= op_count + 1,
                    "sequence {seq_idx} did not terminate redo"
                );
            }
            assert_eq!(
                redo_steps, undo_steps,
                "sequence {seq_idx} redo step mismatch"
            );
            assert_eq!(
                take_snapshot(&library),
                final_snap,
                "sequence {seq_idx} redo did not restore final state"
            );
            let after_redo_state = library.undo_state(None).unwrap();
            assert!(!after_redo_state.can_redo);
            assert_eq!(after_redo_state.undo_depth as usize, undo_steps);

            if seq_idx % 1000 == 999 {
                library.integrity_check().unwrap();
            }
        }
        library.integrity_check().unwrap();
    }

    #[test]
    fn per_document_undo_and_redo_stay_isolated_across_edits() {
        let mut library = Library::open_in_memory().unwrap();
        let actor = test_actor(ActorOrigin::Ui, "dev");
        let id_a = new_id();
        let id_b = new_id();

        let a_v1 = sample_song(id_a.clone(), "Song A1", "Line A1");
        let a_v2 = sample_song(id_a.clone(), "Song A2", "Line A2");
        let b_v1 = sample_song(id_b.clone(), "Song B1", "Line B1");
        let b_v2 = sample_song(id_b.clone(), "Song B2", "Line B2");

        library
            .save_document_op(&a_v1, &meta(&actor, 1, None, b"a1"))
            .unwrap();
        library
            .save_document_op(&a_v2, &meta(&actor, 2, None, b"a2"))
            .unwrap();
        library
            .save_document_op(&b_v1, &meta(&actor, 3, None, b"b1"))
            .unwrap();

        library
            .undo_op(Some(&id_a), &meta(&actor, 4, None, b"undo_a"))
            .unwrap();
        assert_eq!(library.get::<Song>(&id_a).unwrap(), Some(a_v1.clone()));
        assert_eq!(library.get::<Song>(&id_b).unwrap(), Some(b_v1.clone()));

        // Forward edit on B clears only B's redo, leaving A's redo intact.
        library
            .save_document_op(&b_v2, &meta(&actor, 5, None, b"b2"))
            .unwrap();
        let state_a = library.undo_state(Some(&id_a)).unwrap();
        assert!(state_a.can_undo);
        assert!(state_a.can_redo);

        library
            .redo_op(Some(&id_a), &meta(&actor, 6, None, b"redo_a"))
            .unwrap();
        assert_eq!(library.get::<Song>(&id_a).unwrap(), Some(a_v2));

        library
            .undo_op(Some(&id_a), &meta(&actor, 7, None, b"undo_a_1"))
            .unwrap();
        library
            .undo_op(Some(&id_a), &meta(&actor, 8, None, b"undo_a_2"))
            .unwrap();
        assert!(library.get::<Song>(&id_a).unwrap().is_none());
        assert_eq!(library.get::<Song>(&id_b).unwrap(), Some(b_v2));
    }

    #[test]
    fn multi_document_undo_group_undoes_and_redoes_in_exact_order() {
        let mut library = Library::open_in_memory().unwrap();
        let actor = test_actor(ActorOrigin::Ui, "dev");
        let group = new_id();
        let song_id = new_id();
        let tag_id = new_id();

        let song_v1 = sample_song(song_id.clone(), "First", "Words 1");
        let tag_v1 = Tag {
            id: Some(tag_id.clone()),
            name: "Hymns".into(),
        };
        let song_v2 = sample_song(song_id.clone(), "Second", "Words 2");

        library
            .save_document_op(&song_v1, &meta(&actor, 1, Some(&group), b"s1"))
            .unwrap();
        library
            .save_document_op(&tag_v1, &meta(&actor, 2, Some(&group), b"t1"))
            .unwrap();
        library
            .save_document_op(&song_v2, &meta(&actor, 3, Some(&group), b"s2"))
            .unwrap();

        let state = library.undo_state(None).unwrap();
        assert_eq!(state.undo_depth, 1);

        let ExecutionOutcome::Applied { changes, .. } = library
            .undo_op(None, &meta(&actor, 4, None, b"undo"))
            .unwrap()
        else {
            panic!("expected applied outcome");
        };
        assert_eq!(changes.len(), 3);
        assert!(changes.iter().all(|c| c.from_undo_redo));
        assert!(library.get::<Song>(&song_id).unwrap().is_none());
        assert!(library.get::<Tag>(&tag_id).unwrap().is_none());

        library
            .redo_op(None, &meta(&actor, 5, None, b"redo"))
            .unwrap();
        assert_eq!(library.get::<Song>(&song_id).unwrap(), Some(song_v2));
        assert_eq!(library.get::<Tag>(&tag_id).unwrap(), Some(tag_v1));
    }

    #[test]
    fn forward_edit_clears_redo_only_for_that_document() {
        let mut library = Library::open_in_memory().unwrap();
        let actor = test_actor(ActorOrigin::Ui, "dev");
        let id = new_id();
        let v1 = sample_song(id.clone(), "One", "A");
        let v2 = sample_song(id.clone(), "Two", "B");
        let v3 = sample_song(id.clone(), "Three", "C");

        library
            .save_document_op(&v1, &meta(&actor, 1, None, b"1"))
            .unwrap();
        library
            .save_document_op(&v2, &meta(&actor, 2, None, b"2"))
            .unwrap();
        library
            .undo_op(Some(&id), &meta(&actor, 3, None, b"u"))
            .unwrap();
        assert!(library.undo_state(Some(&id)).unwrap().can_redo);

        library
            .save_document_op(&v3, &meta(&actor, 4, None, b"3"))
            .unwrap();
        assert!(!library.undo_state(Some(&id)).unwrap().can_redo);
        assert!(matches!(
            library.redo_op(Some(&id), &meta(&actor, 5, None, b"r")),
            Err(CommandBusError::NothingToUndo)
        ));
    }

    #[test]
    fn media_delete_and_undo_reverify_blob_on_disk() {
        let root = tempfile::tempdir().unwrap();
        let mut library = Library::open(root.path()).unwrap();
        let blob = library
            .media_store()
            .unwrap()
            .import_reader(b"synthetic image bytes".as_slice(), "png", 1024)
            .unwrap();
        let item_id = new_id();
        let item = MediaItem {
            id: Some(item_id.clone()),
            name: "Background".into(),
            kind: MediaKind::Image as i32,
            content_hash: blob.content_hash.clone(),
            extension: blob.extension.clone(),
            size_bytes: blob.size_bytes,
            tags: vec![],
            ..Default::default()
        };
        let actor = test_actor(ActorOrigin::Ui, "dev");

        library
            .save_document_op(&item, &meta(&actor, 10, None, b"save"))
            .unwrap();
        library
            .delete_document_op(
                DocumentKind::Media,
                &item_id,
                &meta(&actor, 11, None, b"del"),
            )
            .unwrap();
        assert!(library.get::<MediaItem>(&item_id).unwrap().is_none());

        library
            .undo_op(None, &meta(&actor, 12, None, b"undo"))
            .unwrap();
        assert_eq!(library.get::<MediaItem>(&item_id).unwrap(), Some(item));

        library
            .delete_document_op(
                DocumentKind::Media,
                &item_id,
                &meta(&actor, 13, None, b"del2"),
            )
            .unwrap();
        let blob_path = library
            .paths()
            .media
            .join(&blob.content_hash[..2])
            .join(format!("{}.{}", blob.content_hash, blob.extension));
        std::fs::remove_file(blob_path).unwrap();
        assert!(matches!(
            library.undo_op(None, &meta(&actor, 14, None, b"undo2")),
            Err(CommandBusError::Invalid(_))
        ));
    }

    #[test]
    fn idempotency_replays_identical_requests_and_rejects_conflicting_ids() {
        let mut library = Library::open_in_memory().unwrap();
        let ui_actor = test_actor(ActorOrigin::Ui, "dev1");
        let other_device = test_actor(ActorOrigin::Ui, "dev2");
        let id = new_id();
        let song = sample_song(id.clone(), "Grace", "Amazing");

        let first = library
            .save_document_op(&song, &meta(&ui_actor, 99, None, b"cmd-a"))
            .unwrap();
        assert!(matches!(first, ExecutionOutcome::Applied { .. }));

        // Exact replay returns cached reply without adding another op.
        let replay = library
            .save_document_op(&song, &meta(&ui_actor, 99, None, b"cmd-a"))
            .unwrap();
        assert!(matches!(replay, ExecutionOutcome::Replayed(_)));
        assert_eq!(library.list_ops(0, 20, None).unwrap().ops.len(), 1);

        // Same actor + envelope id with different command hash is refused (SEL-IPC-0002).
        let conflict = library.save_document_op(&song, &meta(&ui_actor, 99, None, b"cmd-b"));
        assert!(matches!(conflict, Err(CommandBusError::ConflictingId)));

        // Different device with the same envelope id is independent.
        let id2 = new_id();
        let song2 = sample_song(id2, "Mercy", "New every morning");
        let separate = library
            .save_document_op(&song2, &meta(&other_device, 99, None, b"cmd-a"))
            .unwrap();
        assert!(matches!(separate, ExecutionOutcome::Applied { .. }));
        assert_eq!(library.list_ops(0, 20, None).unwrap().ops.len(), 2);
    }

    #[test]
    fn command_bus_errors_map_to_expected_catalogue_codes() {
        let mut library = Library::open_in_memory().unwrap();
        let actor = test_actor(ActorOrigin::Ui, "dev");

        let missing = library.delete_document_op(
            DocumentKind::Song,
            &new_id(),
            &meta(&actor, 1, None, b"missing"),
        );
        assert_eq!(missing.unwrap_err().code(), "SEL-LIB-0001");

        let invalid = library.save_document_op(
            &Song {
                id: Some(new_id()),
                title: " ".into(),
                ..Default::default()
            },
            &meta(&actor, 2, None, b"invalid"),
        );
        assert_eq!(invalid.unwrap_err().code(), "SEL-LIB-0002");

        library
            .set_policy_setting_op(
                "appearance.text-size",
                "150",
                selah_proto::v1::PolicyLevel::Machine as i32,
                true,
                &meta(&actor, 3, None, b"lock"),
            )
            .unwrap();
        let locked = library.set_policy_setting_op(
            "appearance.text-size",
            "100",
            selah_proto::v1::PolicyLevel::User as i32,
            false,
            &meta(&actor, 4, None, b"user-write"),
        );
        assert_eq!(locked.unwrap_err().code(), "SEL-LIB-0003");

        let empty_undo = library.undo_op(None, &meta(&actor, 5, None, b"undo-empty"));
        assert_eq!(empty_undo.unwrap_err().code(), "SEL-LIB-0004");
    }

    #[test]
    fn editor_buffers_and_help_feedback_persist_across_reopen() {
        let root = tempfile::tempdir().unwrap();
        let doc_id = new_id();
        let actor = test_actor(ActorOrigin::Ui, "dev");
        {
            let mut library = Library::open(root.path()).unwrap();
            library
                .save_editor_buffer_op(
                    &doc_id,
                    DocumentType::Song as i32,
                    "Verse 1\nDraft words",
                    &meta(&actor, 1, None, b"buf"),
                )
                .unwrap();
            library
                .record_feedback_op(
                    "troubleshooting.selah-started-in-safe-mode",
                    true,
                    &meta(&actor, 2, None, b"fb1"),
                )
                .unwrap();
            library
                .record_feedback_op(
                    "troubleshooting.selah-started-in-safe-mode",
                    false,
                    &meta(&actor, 3, None, b"fb2"),
                )
                .unwrap();
        }
        let mut reopened = Library::open(root.path()).unwrap();
        let reply = reopened.get_editor_buffer(&doc_id).unwrap();
        assert!(reply.found);
        assert_eq!(
            reply.buffer.as_ref().unwrap().content,
            "Verse 1\nDraft words"
        );
        assert_eq!(
            reopened.list_editor_buffers(10, 0).unwrap().buffers.len(),
            1
        );
        assert_eq!(
            reopened
                .help_feedback_count("troubleshooting.selah-started-in-safe-mode", None)
                .unwrap(),
            2
        );
        assert_eq!(
            reopened
                .help_feedback_count("troubleshooting.selah-started-in-safe-mode", Some(true))
                .unwrap(),
            1
        );

        reopened
            .discard_editor_buffer_op(&doc_id, &meta(&actor, 4, None, b"discard"))
            .unwrap();
        assert!(!reopened.get_editor_buffer(&doc_id).unwrap().found);
    }
}
