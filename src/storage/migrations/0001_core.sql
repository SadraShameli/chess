CREATE TABLE documents (
    id TEXT PRIMARY KEY NOT NULL,
    revision TEXT NOT NULL,
    body TEXT NOT NULL CHECK (json_valid(body))
) STRICT;
