DROP TRIGGER opening_library_games_unindex;
DELETE FROM opening_games WHERE source = 'library';
DROP TABLE opening_library_games;
