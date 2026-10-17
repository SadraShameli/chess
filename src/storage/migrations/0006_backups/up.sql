CREATE TABLE backup_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    first_user_data_ms TEXT,
    last_automatic_ms TEXT,
    automatic_warning TEXT
) STRICT;

INSERT INTO backup_state VALUES (1, NULL, NULL, NULL);
