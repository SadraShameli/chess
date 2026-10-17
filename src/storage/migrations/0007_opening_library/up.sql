CREATE TABLE opening_library_games (
    game_id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL CHECK (game_id BETWEEN 1 AND 2147483647),
    library_id TEXT NOT NULL UNIQUE REFERENCES library_entries(game_id) ON DELETE CASCADE,
    content_revision TEXT NOT NULL
) STRICT;

CREATE TRIGGER opening_library_games_unindex
AFTER DELETE ON opening_library_games
BEGIN
    DELETE FROM opening_games WHERE source = 'library' AND game_id = OLD.game_id;
END;
