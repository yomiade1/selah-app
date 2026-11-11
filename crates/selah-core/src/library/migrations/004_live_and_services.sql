-- Immutable after merge: M4 live operation tables (ARCHITECTURE §4.2).
CREATE TABLE looks (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 512),
    data BLOB NOT NULL
) STRICT;
CREATE TABLE look_rules (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    look_id BLOB NOT NULL REFERENCES looks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    data BLOB NOT NULL,
    UNIQUE(look_id, position)
) STRICT;
CREATE TABLE props (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 512),
    data BLOB NOT NULL
) STRICT;
CREATE TABLE messages (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 512),
    data BLOB NOT NULL
) STRICT;
CREATE TABLE timers (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 512),
    data BLOB NOT NULL
) STRICT;
CREATE TABLE macros (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 512),
    data BLOB NOT NULL
) STRICT;
CREATE TABLE macro_actions (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    macro_id BLOB NOT NULL REFERENCES macros(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    data BLOB NOT NULL,
    UNIQUE(macro_id, position)
) STRICT;
CREATE TABLE clear_groups (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 512),
    data BLOB NOT NULL
) STRICT;
CREATE TABLE stage_layouts (
    id BLOB PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 512),
    data BLOB NOT NULL
) STRICT;
