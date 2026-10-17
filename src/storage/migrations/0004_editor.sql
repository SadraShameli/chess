CREATE TABLE editor_drafts (
    draft_id TEXT PRIMARY KEY NOT NULL CHECK (length(draft_id) = 36),
    storage_revision TEXT NOT NULL CHECK (length(storage_revision) BETWEEN 1 AND 20),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    body TEXT CHECK (body IS NULL OR length(CAST(body AS BLOB)) BETWEEN 1 AND 16384)
) STRICT;

CREATE INDEX editor_drafts_saved ON editor_drafts(draft_id) WHERE body IS NOT NULL;
