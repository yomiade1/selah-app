-- Immutable after merge: append a numbered migration for every schema change.
CREATE TABLE songs (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    title TEXT NOT NULL CHECK(length(title) BETWEEN 1 AND 512),
    authors TEXT NOT NULL,
    ccli_number TEXT NOT NULL,
    tags TEXT NOT NULL,
    data BLOB NOT NULL
) STRICT;
CREATE TABLE song_sections (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    song_id BLOB NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    data BLOB NOT NULL,
    UNIQUE(song_id, position)
) STRICT;
CREATE TABLE song_arrangements (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    song_id BLOB NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    data BLOB NOT NULL,
    UNIQUE(song_id, position)
) STRICT;
CREATE TABLE templates (id BLOB PRIMARY KEY CHECK(length(id) = 16), name TEXT NOT NULL, data BLOB NOT NULL) STRICT;
CREATE TABLE template_variants (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    template_id BLOB NOT NULL REFERENCES templates(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    data BLOB NOT NULL,
    UNIQUE(template_id, position)
) STRICT;
CREATE TABLE media_items (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL,
    content_hash TEXT NOT NULL CHECK(length(content_hash) = 64),
    extension TEXT NOT NULL,
    data BLOB NOT NULL
) STRICT;
CREATE INDEX media_hash ON media_items(content_hash);
CREATE TABLE media_tags (
    media_id BLOB NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY(media_id, tag)
) STRICT;
CREATE TABLE presentations (id BLOB PRIMARY KEY CHECK(length(id) = 16), name TEXT NOT NULL, data BLOB NOT NULL) STRICT;
CREATE TABLE slides (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    presentation_id BLOB NOT NULL REFERENCES presentations(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    data BLOB NOT NULL,
    UNIQUE(presentation_id, position)
) STRICT;
CREATE TABLE service_types (id BLOB PRIMARY KEY CHECK(length(id) = 16), name TEXT NOT NULL, data BLOB NOT NULL) STRICT;
CREATE TABLE teams (id BLOB PRIMARY KEY CHECK(length(id) = 16), name TEXT NOT NULL, data BLOB NOT NULL) STRICT;
CREATE TABLE services (id BLOB PRIMARY KEY CHECK(length(id) = 16), name TEXT NOT NULL, data BLOB NOT NULL) STRICT;
CREATE TABLE service_items (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    service_id BLOB NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    data BLOB NOT NULL,
    UNIQUE(service_id, position)
) STRICT;
CREATE TABLE tags (id BLOB PRIMARY KEY CHECK(length(id) = 16), name TEXT NOT NULL UNIQUE, data BLOB NOT NULL) STRICT;
CREATE TABLE settings (
    key TEXT NOT NULL,
    level INTEGER NOT NULL CHECK(level BETWEEN 0 AND 2),
    value TEXT NOT NULL CHECK(json_valid(value)),
    locked INTEGER NOT NULL CHECK(locked IN (0, 1)),
    CHECK(level > 0 OR locked = 0),
    PRIMARY KEY(key, level)
) STRICT;
CREATE TABLE ops (
    seq INTEGER PRIMARY KEY,
    op_id BLOB NOT NULL UNIQUE CHECK(length(op_id) = 16),
    ts INTEGER NOT NULL,
    actor BLOB NOT NULL,
    origin TEXT NOT NULL,
    doc_type TEXT NOT NULL,
    doc_id BLOB NOT NULL CHECK(length(doc_id) = 16),
    command BLOB NOT NULL,
    inverse BLOB NOT NULL,
    undo_group BLOB NOT NULL CHECK(length(undo_group) = 16),
    sync_status INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE editor_buffers (
    doc_id BLOB PRIMARY KEY CHECK(length(doc_id) = 16),
    content TEXT NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
CREATE TABLE help_feedback (
    seq INTEGER PRIMARY KEY,
    article_id TEXT NOT NULL,
    helpful INTEGER NOT NULL CHECK(helpful IN (0, 1)),
    ts INTEGER NOT NULL
) STRICT;
CREATE VIRTUAL TABLE songs_fts USING fts5(id UNINDEXED, title, lyrics, authors, ccli_number, tags, tokenize = 'unicode61 remove_diacritics 2');
CREATE VIRTUAL TABLE media_fts USING fts5(id UNINDEXED, name, tags, tokenize = 'unicode61 remove_diacritics 2');
