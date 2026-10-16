CREATE TABLE puzzle_collections (
    collection_id TEXT PRIMARY KEY NOT NULL,
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    digest BLOB NOT NULL CHECK (length(digest) = 32),
    body TEXT NOT NULL CHECK (json_valid(body))
) STRICT;

CREATE TABLE puzzle_runs (
    run_id TEXT PRIMARY KEY NOT NULL,
    collection_id TEXT NOT NULL REFERENCES puzzle_collections(collection_id),
    storage_revision TEXT NOT NULL,
    event_count INTEGER NOT NULL CHECK (event_count >= 0 AND event_count <= 100000),
    journal_version INTEGER NOT NULL CHECK (journal_version = 1),
    journal_digest BLOB NOT NULL CHECK (length(journal_digest) = 32)
) STRICT;

CREATE TABLE puzzle_events (
    run_id TEXT NOT NULL REFERENCES puzzle_runs(run_id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0 AND ordinal < 100000),
    body TEXT NOT NULL CHECK (json_valid(body)),
    PRIMARY KEY (run_id, ordinal)
) STRICT;

CREATE TABLE puzzle_checkpoints (
    run_id TEXT PRIMARY KEY NOT NULL REFERENCES puzzle_runs(run_id),
    body TEXT NOT NULL CHECK (json_valid(body))
) STRICT;

CREATE TABLE puzzle_attempts (
    run_id TEXT NOT NULL REFERENCES puzzle_runs(run_id),
    attempt_id TEXT NOT NULL,
    body TEXT NOT NULL CHECK (json_valid(body)),
    PRIMARY KEY (run_id, attempt_id)
) STRICT;

CREATE TRIGGER puzzle_collections_immutable
BEFORE UPDATE ON puzzle_collections
BEGIN
    SELECT RAISE(ABORT, 'A pinned puzzle collection is immutable');
END;

CREATE TRIGGER puzzle_events_immutable
BEFORE UPDATE ON puzzle_events
BEGIN
    SELECT RAISE(ABORT, 'An accepted puzzle event is immutable');
END;
