CREATE TABLE opening_positions (
    position_id INTEGER PRIMARY KEY NOT NULL,
    identity TEXT NOT NULL UNIQUE CHECK (length(identity) BETWEEN 1 AND 1024)
) STRICT;

CREATE TABLE opening_games (
    source TEXT NOT NULL CHECK (length(source) BETWEEN 1 AND 4096),
    game_id INTEGER NOT NULL CHECK (game_id BETWEEN 1 AND 2147483647),
    indexed_plies INTEGER NOT NULL CHECK (indexed_plies BETWEEN 40 AND 120),
    PRIMARY KEY (source, game_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE opening_occurrences (
    source TEXT NOT NULL,
    game_id INTEGER NOT NULL,
    position_id INTEGER NOT NULL REFERENCES opening_positions(position_id),
    ply INTEGER NOT NULL CHECK (ply BETWEEN 0 AND 120),
    next_move TEXT CHECK (next_move IS NULL OR length(next_move) BETWEEN 4 AND 5),
    PRIMARY KEY (source, game_id, position_id),
    FOREIGN KEY (source, game_id) REFERENCES opening_games(source, game_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

CREATE INDEX opening_occurrences_position ON opening_occurrences(position_id, source, game_id);

CREATE TRIGGER opening_games_depth_monotonic
BEFORE UPDATE OF indexed_plies ON opening_games
WHEN NEW.indexed_plies < OLD.indexed_plies
BEGIN
    SELECT RAISE(ABORT, 'An indexed opening depth cannot shrink');
END;

CREATE TRIGGER opening_occurrences_immutable
BEFORE UPDATE ON opening_occurrences
BEGIN
    SELECT RAISE(ABORT, 'An opening occurrence is immutable');
END;
