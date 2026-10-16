use chess::storage::document_store::StorageDocumentStore;
use chess::storage::migration::StorageMigration;
use uuid::Uuid;

#[tokio::test]
async fn receipts_are_durable_and_conflicts_preserve_committed_state()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("library.sqlite3");
    let id = Uuid::new_v4();
    let store = StorageDocumentStore::open(path.clone()).await?;
    let receipt = store.save(id, 0, "{\"state\":\"paused\"}".into()).await?;
    assert_eq!(receipt.revision(), 1);
    assert!(
        store
            .save(id, 0, "{\"state\":\"active\"}".into())
            .await
            .is_err()
    );
    store.close().await?;
    let reopened = StorageDocumentStore::open(path).await?;
    let document = reopened.read(id).await?.ok_or("missing durable document")?;
    assert_eq!(document.revision(), receipt.revision());
    assert_eq!(document.body(), "{\"state\":\"paused\"}");
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn concurrent_expected_revisions_accept_exactly_one_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = StorageDocumentStore::open(directory.path().join("library.sqlite3")).await?;
    let id = Uuid::new_v4();
    let (first, second) = tokio::join!(
        store.save(id, 0, "{\"move\":\"e2e4\"}".into()),
        store.save(id, 0, "{\"move\":\"d2d4\"}".into())
    );
    assert_ne!(first.is_ok(), second.is_ok());
    assert_eq!(
        store
            .read(id)
            .await?
            .ok_or("missing saved document")?
            .revision(),
        1
    );
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn failed_migration_rolls_back_the_complete_batch() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("library.sqlite3");
    let migrations = vec![
        StorageMigration::new(
            1,
            "CREATE TABLE should_rollback (id INTEGER PRIMARY KEY);".into(),
        ),
        StorageMigration::new(2, "THIS IS INVALID SQL;".into()),
    ];
    assert!(
        StorageDocumentStore::open_with_migrations(path.clone(), migrations)
            .await
            .is_err()
    );
    let connection = rusqlite::Connection::open(path)?;
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name = 'should_rollback'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, 0);
    Ok(())
}

#[tokio::test]
async fn invalid_documents_and_future_schemas_are_refused() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("library.sqlite3");
    let store = StorageDocumentStore::open(path.clone()).await?;
    let id = Uuid::new_v4();
    assert!(store.save(id, 0, "not json".into()).await.is_err());
    assert!(store.read(id).await?.is_none());
    store.close().await?;
    let connection = rusqlite::Connection::open(&path)?;
    connection.pragma_update(None, "user_version", 1_000_000)?;
    drop(connection);
    assert!(StorageDocumentStore::open(path).await.is_err());
    Ok(())
}

#[test]
fn required_fixtures_are_present_and_nonempty() -> Result<(), Box<dyn std::error::Error>> {
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../fixtures/positions.json"))?;
    assert!(!fixtures.is_empty());
    Ok(())
}
