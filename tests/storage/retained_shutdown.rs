use std::error::Error;
use std::future::Future;
use std::num::NonZeroUsize;
use std::task::{Context, Waker};
use std::time::Duration;

use chess::capacity::budget::CapacityBudget;
use chess::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use tokio::time::Instant;
use uuid::Uuid;

#[tokio::test]
async fn timed_out_close_keeps_the_real_database_worker_owned_until_retry()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("library.sqlite3");
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let mut store = StorageDocumentStore::open(path.clone(), budget.clone()).await?;
    let held = budget.try_lease(1)?;
    let id = Uuid::new_v4();
    {
        let saving = store.save(id, 0, "{\"saved\":true}".to_owned());
        let mut saving = std::pin::pin!(saving);
        assert!(
            saving
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    let error = store
        .close_retained(Instant::now())
        .await
        .err()
        .ok_or("Blocked database closed without its worker")?;
    assert!(matches!(error, StorageDocumentError::CloseDeadline(_)));
    assert!(error.source().is_some());
    assert!(!store.is_closed());
    assert!(matches!(
        store.read(id).await,
        Err(StorageDocumentError::Closed)
    ));
    assert!(matches!(
        store.save(id, 1, "{}".to_owned()).await,
        Err(StorageDocumentError::Closed)
    ));
    drop(held);
    store
        .close_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(store.is_closed());
    store.close_retained(Instant::now()).await?;
    let reopened = StorageDocumentStore::open(path, budget).await?;
    assert_eq!(
        reopened
            .read(id)
            .await?
            .ok_or("Queued commit was lost")?
            .body(),
        "{\"saved\":true}"
    );
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn cancelling_a_close_wait_preserves_worker_ownership_and_closed_admission()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("library.sqlite3");
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let mut store = StorageDocumentStore::open(path.clone(), budget.clone()).await?;
    let held = budget.try_lease(1)?;
    let id = Uuid::new_v4();
    {
        let saving = store.save(id, 0, "{}".to_owned());
        let mut saving = std::pin::pin!(saving);
        assert!(
            saving
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    {
        let closing = store.close_retained(Instant::now() + Duration::from_secs(3));
        let mut closing = std::pin::pin!(closing);
        assert!(
            closing
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    assert!(!store.is_closed());
    assert!(matches!(
        store.read(id).await,
        Err(StorageDocumentError::Closed)
    ));
    drop(held);
    store
        .close_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(store.is_closed());
    let reopened = StorageDocumentStore::open(path, budget).await?;
    assert_eq!(
        reopened
            .read(id)
            .await?
            .ok_or("Cancelled caller lost its accepted commit")?
            .revision(),
        1
    );
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn observed_database_close_is_idempotent_even_with_an_expired_deadline()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let mut store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
    assert!(!store.is_closed());
    store
        .close_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(store.is_closed());
    store.close_retained(Instant::now()).await?;
    store.close().await?;
    Ok(())
}
