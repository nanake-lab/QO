-- Routing and withdrawal survive a lost process. Questions, answers and raw materials do not.
CREATE TABLE callbacks (
    id TEXT PRIMARY KEY,
    session TEXT NOT NULL,
    turn TEXT,
    kind TEXT NOT NULL,
    state TEXT NOT NULL
) STRICT;
CREATE INDEX callbacks_session ON callbacks(session, state);
INSERT INTO callbacks SELECT id, session, NULL, 'approval', 'live'
    FROM approvals WHERE state IN ('pending','expired');
PRAGMA user_version = 2;
