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
