CREATE TABLE puzzle_deleted_attempts (
    run_id TEXT NOT NULL REFERENCES puzzle_runs(run_id),
    attempt_id TEXT NOT NULL,
    PRIMARY KEY (run_id, attempt_id)
) STRICT;
