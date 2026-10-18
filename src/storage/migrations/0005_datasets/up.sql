CREATE TABLE datasets (
    dataset_id INTEGER PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('corpus', 'book', 'puzzles')),
    origin TEXT NOT NULL CHECK (origin IN ('starter', 'personal')),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 256),
    version TEXT NOT NULL CHECK (length(version) BETWEEN 1 AND 256),
    format TEXT NOT NULL CHECK (format IN ('pgn', 'polyglot', 'puzzle_csv', 'prepared_puzzle_catalog_v1')),
    license TEXT NOT NULL CHECK (length(license) BETWEEN 1 AND 256),
    checksum TEXT NOT NULL CHECK (length(checksum) = 64),
    location TEXT NOT NULL CHECK (length(location) BETWEEN 1 AND 4096),
    validation TEXT NOT NULL CHECK (validation IN ('unchecked', 'integrity_verified')),
    records INTEGER CHECK (records IS NULL OR records >= 0),
    superseded INTEGER NOT NULL DEFAULT 0 CHECK (superseded IN (0, 1)),
    UNIQUE (kind, name, version)
) STRICT;

CREATE TABLE dataset_leases (
    lease_id INTEGER PRIMARY KEY NOT NULL,
    dataset_id INTEGER NOT NULL REFERENCES datasets(dataset_id),
    holder TEXT NOT NULL CHECK (holder IN ('run', 'query', 'report', 'history')),
    holder_ref TEXT NOT NULL CHECK (length(holder_ref) BETWEEN 1 AND 256)
) STRICT;

CREATE INDEX dataset_leases_dataset ON dataset_leases(dataset_id);
