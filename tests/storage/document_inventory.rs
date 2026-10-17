use crate::storage_sqlite_inspector::StorageSqliteInspector;
use chess::capacity::budget::CapacityBudget;
use chess::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use uuid::Uuid;

#[tokio::test]
async fn inventory_pages_use_stable_exclusive_identity_cursors_and_current_revisions()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("library.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let client = store.client();
    assert!(client.inventory(None).await?.is_empty());
    let total = StorageDocumentStore::INVENTORY_PAGE_SIZE + 1;
    for index in (1..=total).rev() {
        client
            .save(Uuid::from_u128(u128::try_from(index)?), 0, "{}".to_owned())
            .await?;
    }
    let first_id = Uuid::from_u128(1);
    client.save(first_id, 1, "[]".to_owned()).await?;
    let page = client.inventory(None).await?;
    assert_eq!(page.len(), StorageDocumentStore::INVENTORY_PAGE_SIZE);
    assert_eq!(page.first().ok_or("missing first identity")?.id(), first_id);
    assert_eq!(page.first().ok_or("missing first revision")?.revision(), 2);
    assert!(page.windows(2).all(|pair| pair[0].id() < pair[1].id()));
    let cursor = page.last().ok_or("missing cursor")?.id();
    let last = client.inventory(Some(cursor)).await?;
    assert_eq!(last.len(), 1);
    assert_eq!(last[0].id(), Uuid::from_u128(u128::try_from(total)?));
    assert!(client.inventory(Some(last[0].id())).await?.is_empty());
    store.close().await?;
    assert!(matches!(
        client.inventory(None).await,
        Err(StorageDocumentError::Closed)
    ));
    let reopened = StorageDocumentStore::open(path, CapacityBudget::discover()?).await?;
    let restored = reopened.client().inventory(Some(cursor)).await?;
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id(), last[0].id());
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn inventory_refuses_corrupt_stored_revisions_without_returning_partial_success()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("library.sqlite3");
    let store = StorageDocumentStore::open(path.clone(), CapacityBudget::discover()?).await?;
    let client = store.client();
    client.save(Uuid::from_u128(1), 0, "{}".to_owned()).await?;
    client.save(Uuid::from_u128(2), 0, "{}".to_owned()).await?;
    let mut connection = StorageSqliteInspector::open(path)?;
    connection.execute(&format!(
        "UPDATE documents SET revision = 'corrupt' WHERE id = '{}'",
        Uuid::from_u128(2)
    ))?;
    assert!(matches!(
        client.inventory(None).await,
        Err(StorageDocumentError::StoredRevision(_))
    ));
    store.close().await?;
    Ok(())
}
