CREATE TABLE documents (
    id TEXT PRIMARY KEY NOT NULL,
    revision TEXT NOT NULL,
    body TEXT NOT NULL CHECK (json_valid(body))
) STRICT;

CREATE TABLE games (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    variant TEXT NOT NULL CHECK (variant IN ('standard', 'chess960', 'dfrc')),
    initial_fen TEXT NOT NULL CHECK (length(initial_fen) <= 512),
    mode TEXT NOT NULL CHECK (mode IN ('human_versus_engine', 'local_two_player')),
    phase TEXT NOT NULL CHECK (phase IN ('setup', 'active', 'interrupted', 'completed', 'analysis')),
    live_node_id TEXT,
    view_node_id TEXT,
    metadata TEXT NOT NULL CHECK (json_valid(metadata)),
    FOREIGN KEY (live_node_id, game_id) REFERENCES move_nodes(node_id, game_id),
    FOREIGN KEY (view_node_id, game_id) REFERENCES move_nodes(node_id, game_id)
) STRICT;

CREATE TABLE move_nodes (
    node_id TEXT PRIMARY KEY NOT NULL,
    game_id TEXT NOT NULL REFERENCES games(game_id) ON DELETE CASCADE,
    parent_id TEXT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    uci TEXT,
    san TEXT,
    fen TEXT NOT NULL CHECK (length(fen) <= 512),
    position_hash BLOB NOT NULL CHECK (length(position_hash) = 8),
    position_identity TEXT NOT NULL,
    annotations TEXT NOT NULL CHECK (json_valid(annotations)),
    UNIQUE (node_id, game_id),
    UNIQUE (game_id, parent_id, ordinal),
    CHECK (parent_id IS NOT node_id),
    CHECK ((parent_id IS NULL AND uci IS NULL AND san IS NULL) OR
           (parent_id IS NOT NULL AND uci IS NOT NULL AND san IS NOT NULL)),
    FOREIGN KEY (parent_id, game_id) REFERENCES move_nodes(node_id, game_id) ON DELETE CASCADE
) STRICT;

CREATE UNIQUE INDEX move_nodes_one_root ON move_nodes(game_id) WHERE parent_id IS NULL;
CREATE INDEX move_nodes_children ON move_nodes(game_id, parent_id, ordinal);
CREATE INDEX move_nodes_position ON move_nodes(position_hash, position_identity);
CREATE INDEX games_mode_phase ON games(mode, phase);

CREATE TRIGGER move_nodes_immutable_parent
BEFORE UPDATE OF parent_id, game_id ON move_nodes
WHEN NEW.parent_id IS NOT OLD.parent_id OR NEW.game_id IS NOT OLD.game_id
BEGIN
    SELECT RAISE(ABORT, 'A move parent and owning game are immutable');
END;

CREATE TABLE clock_snapshots (
    game_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    body TEXT NOT NULL CHECK (json_valid(body)),
    PRIMARY KEY (game_id, node_id),
    FOREIGN KEY (node_id, game_id) REFERENCES move_nodes(node_id, game_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE preferences (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    revision TEXT NOT NULL,
    body TEXT NOT NULL CHECK (json_valid(body))
) STRICT;

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

CREATE TABLE library_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    revision TEXT NOT NULL
) STRICT;

INSERT INTO library_state VALUES (1, '0');

CREATE TABLE library_folders (
    folder_id TEXT PRIMARY KEY NOT NULL,
    parent_id TEXT REFERENCES library_folders(folder_id) ON DELETE CASCADE,
    revision TEXT NOT NULL,
    name TEXT NOT NULL,
    CHECK (parent_id IS NULL OR parent_id <> folder_id)
) STRICT;

CREATE INDEX library_folders_parent ON library_folders(parent_id, folder_id);

CREATE TABLE library_tags (
    tag_id TEXT PRIMARY KEY NOT NULL,
    revision TEXT NOT NULL,
    name TEXT NOT NULL
) STRICT;

CREATE TABLE library_entries (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    content_revision TEXT NOT NULL,
    organization_revision TEXT NOT NULL,
    content_kind TEXT NOT NULL CHECK (content_kind IN ('played', 'notation')),
    rules_variant TEXT NOT NULL CHECK (rules_variant IN ('standard', 'chess960')),
    start_kind TEXT NOT NULL CHECK (start_kind IN ('standard', 'chess960', 'dfrc', 'custom')),
    mode TEXT CHECK (mode IN ('human_versus_engine', 'local_two_player')),
    result TEXT NOT NULL CHECK (result IN ('*', '1-0', '0-1', '1/2-1/2')),
    white_display TEXT,
    white_key TEXT,
    black_display TEXT,
    black_key TEXT,
    date_year INTEGER CHECK (date_year BETWEEN 1 AND 9999),
    date_month INTEGER CHECK (date_month BETWEEN 1 AND 12),
    date_day INTEGER CHECK (date_day BETWEEN 1 AND 31),
    opening_display TEXT,
    opening_key TEXT,
    opening_code TEXT,
    plies INTEGER NOT NULL CHECK (plies BETWEEN 0 AND 100000),
    identity_initial_fen TEXT NOT NULL CHECK (length(identity_initial_fen) <= 512),
    identity_mainline_uci TEXT NOT NULL CHECK (json_valid(identity_mainline_uci) AND json_type(identity_mainline_uci) = 'array'),
    content_sha256 BLOB NOT NULL CHECK (length(content_sha256) = 32),
    modified_ms INTEGER CHECK (modified_ms >= 0),
    favorite INTEGER NOT NULL CHECK (favorite IN (0, 1)),
    folder_id TEXT REFERENCES library_folders(folder_id) ON DELETE SET NULL,
    location TEXT NOT NULL CHECK (location IN ('library', 'trash')),
    trashed_ms INTEGER CHECK (trashed_ms >= 0),
    CHECK ((location = 'library' AND trashed_ms IS NULL) OR (location = 'trash' AND trashed_ms IS NOT NULL)),
    CHECK ((content_kind = 'notation' AND mode IS NULL) OR (content_kind = 'played' AND mode IS NOT NULL))
) STRICT;

CREATE TABLE library_entry_tags (
    game_id TEXT NOT NULL REFERENCES library_entries(game_id) ON DELETE CASCADE,
    tag_id TEXT NOT NULL REFERENCES library_tags(tag_id) ON DELETE CASCADE,
    PRIMARY KEY (game_id, tag_id)
) STRICT;

CREATE INDEX library_entries_updated ON library_entries(location, modified_ms DESC, game_id);
CREATE INDEX library_entries_folder ON library_entries(folder_id, location, modified_ms DESC, game_id);
CREATE INDEX library_entries_white ON library_entries(white_key, game_id);
CREATE INDEX library_entries_black ON library_entries(black_key, game_id);
CREATE INDEX library_entries_date ON library_entries(date_year, date_month, date_day, game_id);
CREATE INDEX library_entries_plies ON library_entries(location, plies, game_id);
CREATE INDEX library_entries_opening ON library_entries(opening_code, game_id);
CREATE INDEX library_entries_structural ON library_entries(rules_variant, identity_initial_fen, plies, game_id);
CREATE INDEX library_entries_favorite ON library_entries(location, favorite, modified_ms DESC, game_id);
CREATE INDEX library_entries_classification ON library_entries(location, rules_variant, start_kind, mode, result, game_id);
CREATE INDEX library_entry_tags_tag ON library_entry_tags(tag_id, game_id);

CREATE TABLE editor_drafts (
    draft_id TEXT PRIMARY KEY NOT NULL CHECK (length(draft_id) = 36),
    storage_revision TEXT NOT NULL CHECK (length(storage_revision) BETWEEN 1 AND 20),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    body TEXT CHECK (body IS NULL OR length(CAST(body AS BLOB)) BETWEEN 1 AND 16384)
) STRICT;

CREATE INDEX editor_drafts_saved ON editor_drafts(draft_id) WHERE body IS NOT NULL;
