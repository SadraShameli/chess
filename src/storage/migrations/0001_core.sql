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
