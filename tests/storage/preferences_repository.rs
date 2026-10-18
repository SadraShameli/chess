use chess::capacity::budget::CapacityBudget;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::preferences_repository::{
    StoragePreferencesError, StoragePreferencesRepository,
};

type StoragePreferencesTestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn saved_preferences_reload_with_a_compare_and_set_revision() -> StoragePreferencesTestResult
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("preferences.sqlite");
    let budget = CapacityBudget::discover()?;
    let store = StorageDocumentStore::open(path.clone(), budget.clone()).await?;
    let repository = StoragePreferencesRepository::new(store.client());
    assert!(repository.load().await?.is_none());
    let first = repository.save(0, 1, r#"{"version":1}"#.to_owned()).await?;
    assert_eq!(first, 1);
    assert!(matches!(
        repository.save(0, 1, "{}".to_owned()).await,
        Err(StoragePreferencesError::Stale {
            expected: 0,
            stored: 1
        })
    ));
    let second = repository
        .save(1, 1, r#"{"version":1,"audio":{"master":30}}"#.to_owned())
        .await?;
    assert_eq!(second, 2);
    store.close().await?;
    let reopened = StorageDocumentStore::open(path.clone(), budget).await?;
    let record = StoragePreferencesRepository::new(reopened.client())
        .load()
        .await?
        .ok_or("missing preferences")?;
    assert_eq!(record.revision(), 2);
    assert_eq!(record.schema_version(), 1);
    assert!(record.body().contains("30"));
    reopened.close().await?;
    directory.close()?;
    Ok(())
}

#[tokio::test]
async fn oversized_bodies_are_refused() -> StoragePreferencesTestResult {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::discover()?;
    let store = StorageDocumentStore::open(directory.path().join("p.sqlite"), budget).await?;
    let repository = StoragePreferencesRepository::new(store.client());
    let body = "x".repeat(1024 * 1024 + 1);
    assert!(matches!(
        repository.save(0, 1, body).await,
        Err(StoragePreferencesError::BodyTooLarge(_))
    ));
    store.close().await?;
    directory.close()?;
    Ok(())
}
