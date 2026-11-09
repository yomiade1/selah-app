CREATE TABLE undo_entries (
    entry_seq INTEGER PRIMARY KEY,
    op_seq INTEGER NOT NULL REFERENCES ops(seq) ON DELETE CASCADE,
    undo_group BLOB NOT NULL CHECK(length(undo_group) = 16),
    doc_type TEXT NOT NULL,
    doc_id BLOB NOT NULL CHECK(length(doc_id) = 16),
    forward_present INTEGER NOT NULL CHECK(forward_present IN (0, 1)),
    forward_data BLOB NOT NULL,
    inverse_present INTEGER NOT NULL CHECK(inverse_present IN (0, 1)),
    inverse_data BLOB NOT NULL,
    undone INTEGER NOT NULL DEFAULT 0 CHECK(undone >= 0)
) STRICT;

CREATE INDEX undo_entries_group ON undo_entries(undo_group, entry_seq);
CREATE INDEX undo_entries_doc ON undo_entries(doc_id, undone, entry_seq);
CREATE INDEX undo_entries_undone ON undo_entries(undone, entry_seq);
CREATE INDEX ops_doc ON ops(doc_id, seq);

CREATE TABLE command_idempotency (
    actor_origin INTEGER NOT NULL,
    user_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    envelope_id INTEGER NOT NULL,
    command_hash BLOB NOT NULL CHECK(length(command_hash) = 32),
    reply BLOB NOT NULL,
    ts INTEGER NOT NULL,
    PRIMARY KEY(actor_origin, user_id, device_id, envelope_id)
) STRICT;

CREATE INDEX command_idempotency_ts ON command_idempotency(ts);

ALTER TABLE editor_buffers ADD COLUMN doc_type TEXT NOT NULL DEFAULT '';
CREATE INDEX editor_buffers_updated ON editor_buffers(updated_at DESC, doc_id);
