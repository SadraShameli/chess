use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::path::Path;

use chess::capacity::budget::CapacityBudget;
use chess::storage::document_store::{StorageDocumentReceipt, StorageDocumentStore};
use chess::storage::migration::StorageMigration;
use uuid::Uuid;

struct StorageDocumentStorePathObservation {
    receipt: StorageDocumentReceipt,
    loaded_revision: u64,
    loaded_body: String,
    reference: (String, String, String),
    file_exists: bool,
}

struct StorageDocumentStorePathFixture;

impl StorageDocumentStorePathFixture {
    const BODY: &'static str = r#"{"path":"kept"}"#;

    async fn inspect(
        name: OsString,
    ) -> Result<StorageDocumentStorePathObservation, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join(name);
        let opened =
            StorageDocumentStore::open(path.clone(), CapacityBudget::new(NonZeroUsize::MIN)?).await;
        let store = match opened {
            Ok(store) => store,
            Err(error) => {
                directory.close()?;
                return Err(error.into());
            }
        };
        let outcome = Self::record(&store).await;
        let closed = store.close().await;
        let file_exists = std::fs::metadata(&path).map(|metadata| metadata.is_file());
        let reference = Self::reference(&path);
        let removed = directory.close();
        closed?;
        removed?;
        let (receipt, loaded_revision, loaded_body) = outcome?;
        Ok(StorageDocumentStorePathObservation {
            receipt,
            loaded_revision,
            loaded_body,
            reference: reference?,
            file_exists: file_exists?,
        })
    }

    async fn record(
        store: &StorageDocumentStore,
    ) -> Result<(StorageDocumentReceipt, u64, String), Box<dyn std::error::Error>> {
        let client = store.client();
        let receipt = client
            .save(Uuid::from_u128(1234), 0, Self::BODY.to_owned())
            .await?;
        let document = client
            .read(receipt.id())
            .await?
            .ok_or("missing native-path document")?;
        Ok((receipt, document.revision(), document.body().to_owned()))
    }

    fn reference(path: &Path) -> Result<(String, String, String), rusqlite::Error> {
        let connection = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let stored =
            connection.query_row("SELECT id, revision, body FROM documents", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;
        connection.close().map_err(|(_connection, error)| error)?;
        Ok(stored)
    }

    fn assert_native_path(observed: StorageDocumentStorePathObservation) {
        assert!(observed.file_exists);
        assert_eq!(observed.receipt.id(), Uuid::from_u128(1234));
        assert_eq!(observed.receipt.revision(), 1);
        assert_eq!(observed.loaded_revision, observed.receipt.revision());
        assert_eq!(observed.loaded_body, Self::BODY);
        assert_eq!(
            observed.reference,
            (
                observed.receipt.id().to_string(),
                observed.receipt.revision().to_string(),
                Self::BODY.to_owned()
            )
        );
    }
}

#[tokio::test]
async fn database_paths_preserve_spaces_unicode_and_literal_uri_characters()
-> Result<(), Box<dyn std::error::Error>> {
    let observed =
        StorageDocumentStorePathFixture::inspect(OsString::from("library #% résumé.sqlite"))
            .await?;
    StorageDocumentStorePathFixture::assert_native_path(observed);
    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn database_paths_preserve_non_utf8_native_bytes() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::ffi::OsStringExt;
    let observed = StorageDocumentStorePathFixture::inspect(OsString::from_vec(
        b"library #?% \xff.sqlite".to_vec(),
    ))
    .await?;
    StorageDocumentStorePathFixture::assert_native_path(observed);
    Ok(())
}

#[tokio::test]
async fn database_paths_reject_null_bytes_without_opening_the_existing_prefix()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("original.sqlite");
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let store = StorageDocumentStore::open(path.clone(), budget.clone()).await?;
    let recorded = StorageDocumentStorePathFixture::record(&store).await;
    let closed = store.close().await;
    closed?;
    let (receipt, _, _) = recorded?;
    let mut invalid = path.as_os_str().to_os_string();
    invalid.push("\0suffix.sqlite");
    let invalid = std::path::PathBuf::from(invalid);
    let reference_error = match rusqlite::Connection::open(&invalid) {
        Ok(connection) => {
            connection.close().map_err(|(_connection, error)| error)?;
            None
        }
        Err(error) => Some(error),
    };
    let actual_error = match StorageDocumentStore::open(invalid, budget).await {
        Ok(store) => {
            store.close().await?;
            None
        }
        Err(error) => Some(error),
    };
    let stored = StorageDocumentStorePathFixture::reference(&path);
    let removed = directory.close();
    removed?;
    let stored = stored?;
    assert!(matches!(
        reference_error,
        Some(rusqlite::Error::NulError(_))
    ));
    assert!(matches!(
        actual_error,
        Some(
            chess::storage::document_store::StorageDocumentError::Database(
                chess::storage::database::StorageDatabaseError::NullByte
            )
        )
    ));
    assert_eq!(
        stored,
        (
            receipt.id().to_string(),
            receipt.revision().to_string(),
            StorageDocumentStorePathFixture::BODY.to_owned()
        )
    );
    Ok(())
}

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
