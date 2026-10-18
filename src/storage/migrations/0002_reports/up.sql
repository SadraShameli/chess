CREATE TABLE report_jobs (
    job_id TEXT PRIMARY KEY NOT NULL,
    game_id TEXT NOT NULL,
    source_revision TEXT NOT NULL,
    line_digest BLOB NOT NULL CHECK (length(line_digest) = 32),
    queue_ordinal INTEGER NOT NULL UNIQUE CHECK (queue_ordinal >= 0),
    state TEXT NOT NULL CHECK (state IN ('queued', 'paused', 'cancelled', 'completed')),
    freshness TEXT NOT NULL CHECK (freshness IN ('current', 'outdated')),
    position_count INTEGER NOT NULL CHECK (position_count > 0 AND position_count <= 100000),
    configuration TEXT NOT NULL CHECK (json_valid(configuration)),
    source TEXT NOT NULL CHECK (json_valid(source))
) STRICT;

CREATE INDEX report_jobs_game ON report_jobs(game_id, freshness, queue_ordinal);

CREATE TABLE report_results (
    job_id TEXT NOT NULL REFERENCES report_jobs(job_id) ON DELETE CASCADE,
    position_index INTEGER NOT NULL CHECK (position_index >= 0 AND position_index < 100000),
    body TEXT NOT NULL CHECK (json_valid(body)),
    PRIMARY KEY (job_id, position_index)
) STRICT;

CREATE TRIGGER report_results_immutable
BEFORE UPDATE ON report_results
BEGIN
    SELECT RAISE(ABORT, 'A completed report position is immutable');
END;

CREATE TRIGGER report_jobs_identity_immutable
BEFORE UPDATE OF job_id, game_id, source_revision, line_digest, position_count, configuration, source, queue_ordinal ON report_jobs
BEGIN
    SELECT RAISE(ABORT, 'A report job identity is immutable');
END;
