CREATE TABLE tasks (
    body TEXT NOT NULL,
    id TEXT GENERATED ALWAYS AS (body ->> '$.taskId') NOT NULL UNIQUE,
    state TEXT GENERATED ALWAYS AS (body ->> '$.state') NOT NULL
) STRICT;
CREATE INDEX tasks_state ON tasks(state);

CREATE TABLE sessions (
    body TEXT NOT NULL,
    id TEXT GENERATED ALWAYS AS (body ->> '$.sessionId') NOT NULL UNIQUE,
    parent TEXT GENERATED ALWAYS AS (body ->> '$.taskId') NOT NULL,
    state TEXT GENERATED ALWAYS AS (body ->> '$.state') NOT NULL
) STRICT;
CREATE INDEX sessions_state ON sessions(state);
CREATE INDEX sessions_parent ON sessions(parent);

CREATE TABLE turns (
    body TEXT NOT NULL,
    id TEXT GENERATED ALWAYS AS (body ->> '$.turnId') NOT NULL UNIQUE,
    parent TEXT GENERATED ALWAYS AS (body ->> '$.sessionId') NOT NULL,
    state TEXT GENERATED ALWAYS AS (body ->> '$.state') NOT NULL
) STRICT;
CREATE INDEX turns_state ON turns(state);
CREATE INDEX turns_parent ON turns(parent);

CREATE TABLE items (
    body TEXT NOT NULL,
    id TEXT GENERATED ALWAYS AS (body ->> '$.itemId') NOT NULL UNIQUE,
    parent TEXT GENERATED ALWAYS AS (body ->> '$.turnId') NOT NULL,
    state TEXT GENERATED ALWAYS AS (body ->> '$.state') NOT NULL
) STRICT;
CREATE INDEX items_state ON items(state);
CREATE INDEX items_parent ON items(parent);

CREATE TABLE usage (
    body TEXT NOT NULL,
    session TEXT GENERATED ALWAYS AS (body ->> '$.sessionId') NOT NULL,
    source TEXT GENERATED ALWAYS AS (body ->> '$.source') NOT NULL,
    dimensions TEXT GENERATED ALWAYS AS (json_array(body ->> '$.turnId', body ->> '$.model',
        body ->> '$.reasoningEffort', body ->> '$.serviceTier')) NOT NULL,
    UNIQUE(session, source, dimensions)
) STRICT;

CREATE TABLE owners (
    task TEXT PRIMARY KEY,
    key_hash BLOB,
    creation_hash BLOB NOT NULL
) STRICT;

-- One namespace for every client-generated ID, including approval decisions.
CREATE TABLE receipts (
    id TEXT PRIMARY KEY,
    method TEXT NOT NULL,
    task TEXT NOT NULL,
    approval TEXT,
    params_hash BLOB NOT NULL,
    reply TEXT NOT NULL
) STRICT;

CREATE TABLE approvals (
    id TEXT PRIMARY KEY,
    task TEXT NOT NULL,
    session TEXT NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    digest TEXT NOT NULL,
    decision_id TEXT,
    choice TEXT,
    decided_at TEXT,
    delivery TEXT
) STRICT;

CREATE TABLE metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    high_water INTEGER NOT NULL,
    retained_bytes INTEGER NOT NULL
) STRICT;
INSERT INTO metadata VALUES (1, 0, 0);

CREATE TABLE events (
    cursor INTEGER PRIMARY KEY,
    body TEXT NOT NULL,
    at REAL GENERATED ALWAYS AS (unixepoch(body ->> '$.at', 'subsec')) STORED NOT NULL,
    kind TEXT GENERATED ALWAYS AS (body ->> '$.kind') NOT NULL,
    task TEXT GENERATED ALWAYS AS (body ->> '$.taskId') NOT NULL,
    bytes INTEGER GENERATED ALWAYS AS (octet_length(body)) STORED NOT NULL
) STRICT;
CREATE INDEX events_at ON events(at);
CREATE TRIGGER event_inserted AFTER INSERT ON events BEGIN
    UPDATE metadata SET high_water = NEW.cursor, retained_bytes = retained_bytes + NEW.bytes;
END;
CREATE TRIGGER event_pruned AFTER DELETE ON events BEGIN
    UPDATE metadata SET retained_bytes = retained_bytes - OLD.bytes;
END;
PRAGMA user_version = 1;
