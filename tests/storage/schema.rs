use crate::storage_sqlite_inspector::StorageSqliteInspector;
use chess::capacity::budget::CapacityBudget;
use chess::storage::document_store::StorageDocumentStore;

#[tokio::test]
async fn position_lookup_never_merges_equal_hashes_with_different_full_identities()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("library.sqlite3");
    StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?)
        .await?
        .close()
        .await?;
    let mut connection = StorageSqliteInspector::open(path)?;
    connection.batch("PRAGMA foreign_keys = ON")?;
    connection.execute("INSERT INTO documents VALUES ('game', '1', '{}')")?;
    connection.execute("INSERT INTO games VALUES ('game', 'standard', 'fen', 'local_two_player', 'setup', NULL, NULL, '{}')")?;
    connection.execute("INSERT INTO move_nodes VALUES ('root', 'game', NULL, 0, NULL, NULL, 'fen', X'0000000000000000', 'position-a', '{}')")?;
    connection.execute("INSERT INTO move_nodes VALUES ('child', 'game', 'root', 0, 'e2e4', 'e4', 'fen', X'0000000000000000', 'position-b', '{}')")?;
    let selected = connection.text(
        "SELECT node_id AS value FROM move_nodes WHERE position_hash = X'0000000000000000' AND position_identity = 'position-b'",
    )?;
    assert_eq!(selected, "child");
    assert!(
        connection
            .execute("UPDATE move_nodes SET parent_id = 'child' WHERE node_id = 'root'")
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
    let mut connection = StorageSqliteInspector::open(path)?;
    connection.batch("PRAGMA foreign_keys = ON")?;
    for id in ["first", "second"] {
        connection.execute(&format!(
            "INSERT INTO documents VALUES ('{id}', '1', '{{}}')"
        ))?;
        connection.execute(&format!("INSERT INTO games VALUES ('{id}', 'standard', 'fen', 'local_two_player', 'setup', NULL, NULL, '{{}}')"))?;
    }
    connection.execute("INSERT INTO move_nodes VALUES ('root', 'first', NULL, 0, NULL, NULL, 'fen', X'0000000000000000', 'position', '{}')")?;
    assert!(connection.execute("INSERT INTO move_nodes VALUES ('cross', 'second', 'root', 0, 'e2e4', 'e4', 'fen', X'0000000000000000', 'position', '{}')").is_err());
    assert!(
        connection
            .execute("INSERT INTO clock_snapshots VALUES ('second', 'root', '{}')")
            .is_err()
    );
    assert!(
        connection
            .execute("INSERT INTO preferences VALUES (2, 1, '1', '{}')")
            .is_err()
    );
    Ok(())
}
