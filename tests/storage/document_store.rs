use chess::capacity::budget::CapacityBudget;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::migration::StorageMigration;
use uuid::Uuid;

#[tokio::test]
async fn receipts_are_durable_and_conflicts_preserve_committed_state()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("library.sqlite3");
    let id = Uuid::new_v4();
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let receipt = store
        .client()
        .save(id, 0, "{\"state\":\"paused\"}".into())
        .await?;
    assert_eq!(receipt.revision(), 1);
    assert!(
        store
            .client()
            .save(id, 0, "{\"state\":\"active\"}".into())
            .await
            .is_err()
    );
    store.close().await?;
    let reopened = StorageDocumentStore::open(path, CapacityBudget::discover()?).await?;
    let document = reopened
        .client()
        .read(id)
        .await?
        .ok_or("missing durable document")?;
    assert_eq!(document.revision(), receipt.revision());
    assert_eq!(document.body(), "{\"state\":\"paused\"}");
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn concurrent_expected_revisions_accept_exactly_one_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = StorageDocumentStore::open(
        directory.path().join("library.sqlite3"),
        CapacityBudget::discover()?,
    )
    .await?;
    let id = Uuid::new_v4();
    let client = store.client();
    let (first, second) = tokio::join!(
        client.save(id, 0, "{\"move\":\"e2e4\"}".into()),
        client.save(id, 0, "{\"move\":\"d2d4\"}".into())
    );
    assert_ne!(first.is_ok(), second.is_ok());
    assert_eq!(
        store
            .client()
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
        StorageDocumentStore::open_with_migrations(
            path.clone(),
            migrations,
            CapacityBudget::discover()?
        )
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
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let id = Uuid::new_v4();
    assert!(store.client().save(id, 0, "not json".into()).await.is_err());
    assert!(store.client().read(id).await?.is_none());
    store.close().await?;
    let connection = rusqlite::Connection::open(&path)?;
    connection.pragma_update(None, "user_version", 1_000_000)?;
    drop(connection);
    assert!(
        StorageDocumentStore::open(path, CapacityBudget::discover()?)
            .await
            .is_err()
    );
    Ok(())
}

#[test]
fn required_fixtures_are_present_and_nonempty() -> Result<(), Box<dyn std::error::Error>> {
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../fixtures/positions.json"))?;
    assert!(!fixtures.is_empty());
    Ok(())
}

#[tokio::test]
async fn idle_storage_releases_cpu_and_waits_for_shared_capacity()
-> Result<(), Box<dyn std::error::Error>> {
    use std::future::{Future, poll_fn};
    use std::num::NonZeroUsize;
    use std::task::Poll;

    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget.clone())
            .await?;
    assert_eq!(budget.available(), 1);
    let held = budget.try_lease(1)?;
    let id = Uuid::new_v4();
    let client = store.client();
    let mut saving = Box::pin(client.save(id, 0, "{}".to_owned()));
    poll_fn(|context| {
        assert!(saving.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(held);
    assert_eq!(saving.await?.revision(), 1);
    assert_eq!(budget.available(), 1);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn queued_database_operations_reserve_only_the_running_worker()
-> Result<(), Box<dyn std::error::Error>> {
    use std::future::{Future, poll_fn};
    use std::num::NonZeroUsize;
    use std::task::Poll;
    use std::time::Duration;

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("library.sqlite3");
    let workers = 4;
    let budget = CapacityBudget::new(NonZeroUsize::new(workers).ok_or("invalid capacity")?)?;
    let store = StorageDocumentStore::open(path.clone(), budget.clone()).await?;
    let connection = rusqlite::Connection::open(path)?;
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let client = store.client();
    let mut saving = Vec::new();
    for _ in 0..workers {
        let mut pending = Box::pin(client.save(Uuid::new_v4(), 0, "{}".to_owned()));
        poll_fn(|context| {
            assert!(pending.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        saving.push(pending);
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while budget.available() == workers {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert_eq!(budget.available(), workers - 1);
    connection.execute_batch("ROLLBACK")?;
    for pending in saving {
        assert_eq!(pending.await?.revision(), 1);
    }
    assert_eq!(budget.available(), workers);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn cloned_clients_do_not_keep_the_closed_database_worker_alive()
-> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    use chess::storage::document_store::StorageDocumentError;

    let directory = tempfile::tempdir()?;
    let store = StorageDocumentStore::open(
        directory.path().join("library.sqlite3"),
        CapacityBudget::discover()?,
    )
    .await?;
    let client = store.client();
    let other = client.clone();
    let id = Uuid::new_v4();
    client.save(id, 0, "{}".to_owned()).await?;
    assert_eq!(
        other
            .read(id)
            .await?
            .ok_or("missing shared document")?
            .revision(),
        1
    );
    tokio::time::timeout(Duration::from_secs(3), store.close()).await??;
    assert!(matches!(
        client.read(id).await,
        Err(StorageDocumentError::Closed)
    ));
    assert!(matches!(
        other.save(id, 1, "{}".to_owned()).await,
        Err(StorageDocumentError::Closed)
    ));
    Ok(())
}
