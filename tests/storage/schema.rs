use chess::capacity::budget::CapacityBudget;
use chess::storage::document_store::StorageDocumentStore;
use rusqlite::{Connection, params};

#[tokio::test]
async fn position_lookup_never_merges_equal_hashes_with_different_full_identities()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("library.sqlite3");
    StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?)
        .await?
        .close()
        .await?;
    let connection = Connection::open(path)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.execute("INSERT INTO documents VALUES ('game', '1', '{}')", [])?;
    connection.execute("INSERT INTO games VALUES ('game', 'standard', 'fen', 'local_two_player', 'setup', NULL, NULL, '{}')", [])?;
    let hash = [0_u8; 8];
    connection.execute("INSERT INTO move_nodes VALUES ('root', 'game', NULL, 0, NULL, NULL, 'fen', ?1, 'position-a', '{}')", [hash])?;
    connection.execute("INSERT INTO move_nodes VALUES ('child', 'game', 'root', 0, 'e2e4', 'e4', 'fen', ?1, 'position-b', '{}')", [hash])?;
    let selected: String = connection.query_row(
        "SELECT node_id FROM move_nodes WHERE position_hash = ?1 AND position_identity = ?2",
        params![hash, "position-b"],
        |row| row.get(0),
    )?;
    assert_eq!(selected, "child");
    assert!(
        connection
            .execute(
                "UPDATE move_nodes SET parent_id = 'child' WHERE node_id = 'root'",
                []
            )
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn foreign_keys_refuse_cross_game_parents_and_orphan_clocks()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("library.sqlite3");
    StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?)
        .await?
        .close()
        .await?;
    let connection = Connection::open(path)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    for id in ["first", "second"] {
        connection.execute("INSERT INTO documents VALUES (?1, '1', '{}')", [id])?;
        connection.execute("INSERT INTO games VALUES (?1, 'standard', 'fen', 'local_two_player', 'setup', NULL, NULL, '{}')", [id])?;
    }
    let hash = [0_u8; 8];
    connection.execute("INSERT INTO move_nodes VALUES ('root', 'first', NULL, 0, NULL, NULL, 'fen', ?1, 'position', '{}')", [hash])?;
    assert!(connection.execute("INSERT INTO move_nodes VALUES ('cross', 'second', 'root', 0, 'e2e4', 'e4', 'fen', ?1, 'position', '{}')", [hash]).is_err());
    assert!(
        connection
            .execute(
                "INSERT INTO clock_snapshots VALUES ('second', 'root', '{}')",
                []
            )
            .is_err()
    );
    assert!(
        connection
            .execute("INSERT INTO preferences VALUES (2, 1, '1', '{}')", [])
            .is_err()
    );
    Ok(())
}
