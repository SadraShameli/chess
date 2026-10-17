use std::future::{Future, poll_fn};
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::task::Poll;

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationFolderId,
    GameLibraryOrganizationFolderTarget, GameLibraryOrganizationName,
    GameLibraryOrganizationReceipt, GameLibraryOrganizationRequest,
    GameLibraryOrganizationRevision, GameLibraryOrganizationTagId,
    GameLibraryOrganizationTagTarget, GameLibraryOrganizationTimestamp,
};
use chess::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use chess::storage::library_metadata::{
    StorageLibraryMetadataEnumRequest, StorageLibraryMetadataEnumResponse,
    StorageLibraryMetadataRepository,
};
use chess::storage::library_repository::{StorageLibraryError, StorageLibraryRepository};

type StorageLibraryMetadataTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct StorageLibraryMetadataFixture {
    _directory: tempfile::TempDir,
    budget: CapacityBudget,
    store: StorageDocumentStore,
    executor: CapacityExecutor,
}

impl StorageLibraryMetadataFixture {
    async fn new() -> StorageLibraryMetadataTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid test capacity")?)?;
        let store =
            StorageDocumentStore::open(directory.path().join("metadata.sqlite"), budget.clone())
                .await?;
        let executor = CapacityExecutor::new(budget.clone())?;
        Ok(Self {
            _directory: directory,
            budget,
            store,
            executor,
        })
    }

    fn metadata(&self) -> StorageLibraryMetadataRepository {
        StorageLibraryMetadataRepository::new(self.store.client())
    }

    fn folder_id(value: u64) -> StorageLibraryMetadataTestResult<GameLibraryOrganizationFolderId> {
        Ok(
            uuid::Uuid::from_u128((u128::from(value) << 64) | 0x8000_0000_0000_0042)
                .to_string()
                .parse()?,
        )
    }

    fn tag_id(value: u64) -> StorageLibraryMetadataTestResult<GameLibraryOrganizationTagId> {
        Ok(
            uuid::Uuid::from_u128((u128::from(value) << 64) | 0x8000_0000_0000_0043)
                .to_string()
                .parse()?,
        )
    }

    fn name(value: u64) -> String {
        format!("Dossier {value:03} Å étude ♞")
    }

    async fn organize(
        &self,
        mutation: GameLibraryOrganizationEnumMutation,
    ) -> StorageLibraryMetadataTestResult<GameLibraryOrganizationReceipt> {
        Ok(
            StorageLibraryRepository::new(self.store.client(), &self.executor)
                .organize(GameLibraryOrganizationRequest::new(
                    GameLibraryOrganizationTimestamp::new(1_234)?,
                    mutation,
                )?)
                .await?,
        )
    }

    async fn folder(
        &self,
        value: u64,
        parent: Option<GameLibraryOrganizationFolderId>,
    ) -> StorageLibraryMetadataTestResult<GameLibraryOrganizationFolderId> {
        let id = Self::folder_id(value)?;
        self.organize(GameLibraryOrganizationEnumMutation::CreateFolder(
            id,
            parent.map(|id| {
                GameLibraryOrganizationFolderTarget::new(id, GameLibraryOrganizationRevision::ZERO)
            }),
            GameLibraryOrganizationName::new(Self::name(value))?,
        ))
        .await?;
        Ok(id)
    }

    async fn tag(
        &self,
        value: u64,
    ) -> StorageLibraryMetadataTestResult<GameLibraryOrganizationTagId> {
        let id = Self::tag_id(value)?;
        self.organize(GameLibraryOrganizationEnumMutation::CreateTag(
            id,
            GameLibraryOrganizationName::new(Self::name(value))?,
        ))
        .await?;
        Ok(id)
    }

    async fn populate_roots(&self, count: u64) -> StorageLibraryMetadataTestResult {
        for value in (1..=count).rev() {
            self.folder(value, None).await?;
        }
        Ok(())
    }

    async fn populate_budget_catalog(&self) -> StorageLibraryMetadataTestResult {
        let name = "é".repeat(GameLibraryOrganizationName::MAX_BYTES / "é".len());
        for value in 1..=100 {
            for mutation in [
                GameLibraryOrganizationEnumMutation::CreateFolder(
                    Self::folder_id(value)?,
                    None,
                    GameLibraryOrganizationName::new(name.clone())?,
                ),
                GameLibraryOrganizationEnumMutation::CreateTag(
                    Self::tag_id(value)?,
                    GameLibraryOrganizationName::new(name.clone())?,
                ),
            ] {
                self.organize(mutation).await?;
            }
        }
        Ok(())
    }

    async fn fill_page_budget(
        &self,
    ) -> StorageLibraryMetadataTestResult<Vec<StorageLibraryMetadataEnumResponse>> {
        let metadata = self.metadata();
        let mut retained = Vec::new();
        let bound = StorageDocumentStore::MEMORY_BUDGET
            / (100 * GameLibraryOrganizationName::MAX_BYTES)
            + 1;
        for _ in 0..bound {
            match Self::read(
                &metadata,
                StorageLibraryMetadataEnumRequest::Folders {
                    parent: None,
                    cursor: None,
                },
            )
            .await
            {
                Ok(response) => retained.push(response),
                Err(StorageLibraryError::Store(StorageDocumentError::Saturated)) => {
                    return Ok(retained);
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err("metadata pages exceeded their shared actor byte budget".into())
    }

    async fn assert_retained_clone(
        &self,
        request: StorageLibraryMetadataEnumRequest,
    ) -> StorageLibraryMetadataTestResult {
        let metadata = self.metadata();
        let original = Self::read(&metadata, request).await?;
        let fillers = self.fill_page_budget().await?;
        assert!(!fillers.is_empty());
        let mut retained = Vec::new();
        let mut exhausted = false;
        for _ in 0..=StorageDocumentStore::MEMORY_BUDGET / GameLibraryOrganizationName::MAX_BYTES {
            match Self::read(&metadata, request).await {
                Ok(response) => retained.push(response),
                Err(StorageLibraryError::Store(StorageDocumentError::Saturated)) => {
                    exhausted = true;
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }
        assert!(exhausted);
        let first_clone = original.clone();
        let last_clone = original.clone();
        drop(original);
        for clone in [first_clone, last_clone] {
            assert!(matches!(
                Self::read(&metadata, request).await,
                Err(StorageLibraryError::Store(StorageDocumentError::Saturated))
            ));
            drop(clone);
        }
        let replacement = Self::read(&metadata, request).await?;
        assert!(matches!(
            Self::read(&metadata, request).await,
            Err(StorageLibraryError::Store(StorageDocumentError::Saturated))
        ));
        drop((replacement, retained, fillers));
        drop(Self::read(&metadata, request).await?);
        Ok(())
    }

    async fn read(
        metadata: &StorageLibraryMetadataRepository,
        request: StorageLibraryMetadataEnumRequest,
    ) -> Result<StorageLibraryMetadataEnumResponse, StorageLibraryError> {
        metadata.begin(request)?.wait().await
    }

    fn close(self) -> Pin<Box<dyn Future<Output = StorageLibraryMetadataTestResult>>> {
        Box::pin(async move {
            self.store.close().await?;
            self.executor.shutdown().await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn root_pages_preserve_names_and_full_uuid_order_without_nested_rows()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    fixture.populate_roots(107).await?;
    let parent = StorageLibraryMetadataFixture::folder_id(1)?;
    fixture.folder(1_000, Some(parent)).await?;
    let metadata = fixture.metadata();
    let first = metadata.folders(None, None).await?;
    assert_eq!(first.entries().len(), 100);
    assert_eq!(first.catalog_revision().value(), 108);
    for (index, folder) in first.entries().iter().enumerate() {
        let value = u64::try_from(index)? + 1;
        assert_eq!(
            folder.id(),
            StorageLibraryMetadataFixture::folder_id(value)?
        );
        assert_eq!(
            folder.name().as_str(),
            StorageLibraryMetadataFixture::name(value)
        );
        assert_eq!(folder.parent(), None);
        assert_eq!(folder.revision(), GameLibraryOrganizationRevision::ZERO);
    }
    let cursor = *first.next_cursor().ok_or("missing root continuation")?;
    let second = metadata.folders(None, Some(cursor)).await?;
    assert_eq!(second.entries().len(), 7);
    assert_eq!(second.catalog_revision(), first.catalog_revision());
    assert!(second.next_cursor().is_none());
    for (index, folder) in second.entries().iter().enumerate() {
        assert_eq!(
            folder.id(),
            StorageLibraryMetadataFixture::folder_id(u64::try_from(index)? + 101)?
        );
    }
    drop((first, second));
    fixture.close().await
}

#[tokio::test]
async fn nested_child_pages_keep_exact_parent_and_exclusive_continuation()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    let root = fixture.folder(500, None).await?;
    let sibling = fixture.folder(600, None).await?;
    for value in (1..=103).rev() {
        fixture.folder(value, Some(root)).await?;
    }
    let child = StorageLibraryMetadataFixture::folder_id(1)?;
    fixture.folder(700, Some(child)).await?;
    fixture.folder(800, Some(sibling)).await?;
    let metadata = fixture.metadata();
    let first = metadata.folders(Some(root), None).await?;
    let cursor = *first.next_cursor().ok_or("missing child continuation")?;
    let second = metadata.folders(Some(root), Some(cursor)).await?;
    assert_eq!(first.entries().len(), 100);
    assert_eq!(second.entries().len(), 3);
    assert!(second.next_cursor().is_none());
    for (index, folder) in first.entries().iter().chain(second.entries()).enumerate() {
        let value = u64::try_from(index)? + 1;
        assert_eq!(
            folder.id(),
            StorageLibraryMetadataFixture::folder_id(value)?
        );
        assert_eq!(folder.parent(), Some(root));
        assert_eq!(
            folder.name().as_str(),
            StorageLibraryMetadataFixture::name(value)
        );
    }
    drop((first, second));
    fixture.close().await
}

#[tokio::test]
async fn empty_catalog_and_missing_parent_are_distinct() -> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    let metadata = fixture.metadata();
    let folders = metadata.folders(None, None).await?;
    let tags = metadata.tags(None).await?;
    assert!(folders.entries().is_empty());
    assert!(tags.entries().is_empty());
    assert!(folders.next_cursor().is_none());
    assert!(tags.next_cursor().is_none());
    assert_eq!(folders.catalog_revision().value(), 0);
    assert_eq!(tags.catalog_revision(), folders.catalog_revision());
    let missing = StorageLibraryMetadataFixture::folder_id(99)?;
    assert!(matches!(
        metadata.folders(Some(missing), None).await,
        Err(StorageLibraryError::MissingFolder(id)) if id == missing
    ));
    drop((folders, tags));
    fixture.close().await
}

#[tokio::test]
async fn existing_empty_parent_is_a_successful_empty_child_page() -> StorageLibraryMetadataTestResult
{
    let fixture = StorageLibraryMetadataFixture::new().await?;
    let root = fixture.folder(1, None).await?;
    let page = fixture.metadata().folders(Some(root), None).await?;
    assert!(page.entries().is_empty());
    assert!(page.next_cursor().is_none());
    assert_eq!(page.catalog_revision().value(), 1);
    drop(page);
    fixture.close().await
}

#[tokio::test]
async fn folder_cursor_refuses_root_child_and_other_parent_scopes()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    fixture.populate_roots(102).await?;
    let root = StorageLibraryMetadataFixture::folder_id(1)?;
    let other = StorageLibraryMetadataFixture::folder_id(2)?;
    for value in 200..301 {
        fixture.folder(value, Some(root)).await?;
    }
    let metadata = fixture.metadata();
    let roots = metadata.folders(None, None).await?;
    let root_cursor = *roots.next_cursor().ok_or("missing root cursor")?;
    let children = metadata.folders(Some(root), None).await?;
    let child_cursor = *children.next_cursor().ok_or("missing child cursor")?;
    for (parent, cursor) in [
        (Some(root), root_cursor),
        (None, child_cursor),
        (Some(other), child_cursor),
    ] {
        assert!(matches!(
            metadata.folders(parent, Some(cursor)).await,
            Err(StorageLibraryError::CursorQuery)
        ));
    }
    drop((roots, children));
    fixture.close().await
}

#[tokio::test]
async fn organization_changes_invalidate_folder_and_tag_catalog_cursors()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    fixture.populate_roots(101).await?;
    for value in 1..=101 {
        fixture.tag(value).await?;
    }
    let metadata = fixture.metadata();
    let folders = metadata.folders(None, None).await?;
    let tags = metadata.tags(None).await?;
    let folder_cursor = *folders.next_cursor().ok_or("missing folder cursor")?;
    let tag_cursor = *tags.next_cursor().ok_or("missing tag cursor")?;
    fixture
        .organize(GameLibraryOrganizationEnumMutation::RenameFolder(
            GameLibraryOrganizationFolderTarget::new(
                StorageLibraryMetadataFixture::folder_id(1)?,
                GameLibraryOrganizationRevision::ZERO,
            ),
            GameLibraryOrganizationName::new("Renamed ♞".to_owned())?,
        ))
        .await?;
    assert!(matches!(
        metadata.folders(None, Some(folder_cursor)).await,
        Err(StorageLibraryError::StaleCursor)
    ));
    assert!(matches!(
        metadata.tags(Some(tag_cursor)).await,
        Err(StorageLibraryError::StaleCursor)
    ));
    let fresh = metadata.folders(None, None).await?;
    assert_eq!(fresh.catalog_revision().value(), 203);
    assert_eq!(fresh.entries()[0].name().as_str(), "Renamed ♞");
    assert_eq!(fresh.entries()[0].revision().value(), 1);
    drop((folders, tags, fresh));
    fixture.close().await
}

#[tokio::test]
async fn tag_pages_preserve_actual_names_revisions_and_uuid_order()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    for value in (1..=105).rev() {
        fixture.tag(value).await?;
    }
    let changed = StorageLibraryMetadataFixture::tag_id(102)?;
    fixture
        .organize(GameLibraryOrganizationEnumMutation::RenameTag(
            GameLibraryOrganizationTagTarget::new(changed, GameLibraryOrganizationRevision::ZERO),
            GameLibraryOrganizationName::new("Actual renamed tag".to_owned())?,
        ))
        .await?;
    let metadata = fixture.metadata();
    let first = metadata.tags(None).await?;
    let cursor = *first.next_cursor().ok_or("missing tag continuation")?;
    let second = metadata.tags(Some(cursor)).await?;
    assert_eq!(first.entries().len(), 100);
    assert_eq!(second.entries().len(), 5);
    assert!(second.next_cursor().is_none());
    assert_eq!(first.catalog_revision().value(), 106);
    for (index, tag) in first.entries().iter().chain(second.entries()).enumerate() {
        let value = u64::try_from(index)? + 1;
        assert_eq!(tag.id(), StorageLibraryMetadataFixture::tag_id(value)?);
        if tag.id() == changed {
            assert_eq!(tag.name().as_str(), "Actual renamed tag");
            assert_eq!(tag.revision().value(), 1);
        } else {
            assert_eq!(
                tag.name().as_str(),
                StorageLibraryMetadataFixture::name(value)
            );
            assert_eq!(tag.revision(), GameLibraryOrganizationRevision::ZERO);
        }
    }
    drop((first, second));
    fixture.close().await
}

#[tokio::test]
async fn folder_lookup_supplies_breadcrumb_names_parents_and_entity_revisions()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    let root = fixture.folder(9, None).await?;
    let child = fixture.folder(1, Some(root)).await?;
    fixture
        .organize(GameLibraryOrganizationEnumMutation::RenameFolder(
            GameLibraryOrganizationFolderTarget::new(child, GameLibraryOrganizationRevision::ZERO),
            GameLibraryOrganizationName::new("Child displayed name".to_owned())?,
        ))
        .await?;
    let metadata = fixture.metadata();
    let page = metadata.folders(None, None).await?;
    let catalog = page.catalog_revision();
    let child_read = metadata.folder(child, catalog).await?;
    let root_read = metadata.folder(root, catalog).await?;
    assert_eq!(child_read.catalog_revision(), catalog);
    assert_eq!(child_read.folder().id(), child);
    assert_eq!(child_read.folder().name().as_str(), "Child displayed name");
    assert_eq!(child_read.folder().parent(), Some(root));
    assert_eq!(child_read.folder().revision().value(), 1);
    assert_eq!(root_read.folder().parent(), None);
    assert_eq!(
        root_read.folder().name().as_str(),
        StorageLibraryMetadataFixture::name(9)
    );
    assert!(matches!(
        metadata
            .folder(StorageLibraryMetadataFixture::folder_id(404)?, catalog)
            .await,
        Err(StorageLibraryError::MissingFolder(_))
    ));
    drop((page, child_read, root_read));
    fixture.close().await
}

#[tokio::test]
async fn folder_lookup_validates_catalog_before_resolving_a_missing_identifier()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    let root = fixture.folder(1, None).await?;
    let metadata = fixture.metadata();
    let page = metadata.folders(None, None).await?;
    let stale = page.catalog_revision();
    fixture.tag(1).await?;
    for id in [root, StorageLibraryMetadataFixture::folder_id(404)?] {
        assert!(matches!(
            metadata.folder(id, stale).await,
            Err(StorageLibraryError::StaleCursor)
        ));
    }
    let fresh = metadata.tags(None).await?;
    let folder = metadata.folder(root, fresh.catalog_revision()).await?;
    assert_eq!(folder.folder().id(), root);
    drop((page, fresh, folder));
    fixture.close().await
}

#[tokio::test]
async fn reading_metadata_does_not_change_the_catalog_or_entity_revision()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    let root = fixture.folder(1, None).await?;
    fixture.tag(1).await?;
    let metadata = fixture.metadata();
    for _ in 0..3 {
        let page = metadata.folders(None, None).await?;
        let tags = metadata.tags(None).await?;
        let folder = metadata.folder(root, page.catalog_revision()).await?;
        assert_eq!(page.catalog_revision().value(), 2);
        assert_eq!(tags.catalog_revision(), page.catalog_revision());
        assert_eq!(folder.catalog_revision(), page.catalog_revision());
        assert_eq!(
            folder.folder().revision(),
            GameLibraryOrganizationRevision::ZERO
        );
    }
    fixture.close().await
}

#[tokio::test]
async fn maximum_utf8_names_survive_folder_and_tag_reads() -> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    let name = "é".repeat(GameLibraryOrganizationName::MAX_BYTES / "é".len());
    let folder = StorageLibraryMetadataFixture::folder_id(1)?;
    let tag = StorageLibraryMetadataFixture::tag_id(1)?;
    for mutation in [
        GameLibraryOrganizationEnumMutation::CreateFolder(
            folder,
            None,
            GameLibraryOrganizationName::new(name.clone())?,
        ),
        GameLibraryOrganizationEnumMutation::CreateTag(
            tag,
            GameLibraryOrganizationName::new(name.clone())?,
        ),
    ] {
        fixture.organize(mutation).await?;
    }
    let metadata = fixture.metadata();
    let folders = metadata.folders(None, None).await?;
    let tags = metadata.tags(None).await?;
    assert_eq!(folders.entries()[0].name().as_str(), name);
    assert_eq!(tags.entries()[0].name().as_str(), name);
    drop((folders, tags));
    fixture.close().await
}

#[tokio::test]
async fn cloned_folder_page_retains_one_shared_reservation_until_final_drop()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    fixture.populate_budget_catalog().await?;
    fixture
        .assert_retained_clone(StorageLibraryMetadataEnumRequest::Folders {
            parent: None,
            cursor: None,
        })
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn cloned_tag_page_shares_the_same_actor_budget_and_releases_it()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    fixture.populate_budget_catalog().await?;
    fixture
        .assert_retained_clone(StorageLibraryMetadataEnumRequest::Tags { cursor: None })
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn cloned_folder_lookup_retains_its_actor_reservation_until_final_drop()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    fixture.populate_budget_catalog().await?;
    let page = fixture.metadata().folders(None, None).await?;
    let catalog = page.catalog_revision();
    drop(page);
    fixture
        .assert_retained_clone(StorageLibraryMetadataEnumRequest::Folder {
            id: StorageLibraryMetadataFixture::folder_id(1)?,
            catalog,
        })
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn cancelled_metadata_wait_keeps_the_accepted_operation_observable()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    fixture.folder(1, None).await?;
    let metadata = fixture.metadata();
    let lease = fixture.budget.lease(1).await?;
    let mut pending = metadata.begin(StorageLibraryMetadataEnumRequest::Folders {
        parent: None,
        cursor: None,
    })?;
    let was_pending = {
        let mut wait = std::pin::pin!(pending.wait());
        poll_fn(|context| Poll::Ready(wait.as_mut().poll(context).is_pending())).await
    };
    assert!(was_pending);
    drop(lease);
    let StorageLibraryMetadataEnumResponse::Folders(page) = pending.wait().await? else {
        return Err("metadata response did not match the accepted read".into());
    };
    assert_eq!(page.entries().len(), 1);
    assert_eq!(
        page.entries()[0].name().as_str(),
        StorageLibraryMetadataFixture::name(1)
    );
    assert!(matches!(
        pending.wait().await,
        Err(StorageLibraryError::AlreadyObserved)
    ));
    drop((pending, page));
    fixture.close().await
}

#[tokio::test]
async fn sqlite_metadata_errors_are_not_successful_empty_pages() -> StorageLibraryMetadataTestResult
{
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid test capacity")?)?;
    let path = directory.path().join("missing-tables.sqlite");
    StorageDocumentStore::open(path.clone(), CapacityBudget::new(NonZeroUsize::MIN)?)
        .await?
        .close()
        .await?;
    rusqlite::Connection::open(&path)?.execute_batch(
        "PRAGMA foreign_keys = OFF; DROP TABLE library_entry_tags; DROP TABLE library_folders; DROP TABLE library_tags;",
    )?;
    let store = StorageDocumentStore::open(path, budget).await?;
    let metadata = StorageLibraryMetadataRepository::new(store.client());
    assert!(matches!(
        metadata.folders(None, None).await,
        Err(StorageLibraryError::Diesel(_))
    ));
    assert!(matches!(
        metadata.tags(None).await,
        Err(StorageLibraryError::Diesel(_))
    ));
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn closed_actor_refuses_metadata_admission() -> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    let metadata = fixture.metadata();
    fixture.close().await?;
    assert!(matches!(
        metadata.begin(StorageLibraryMetadataEnumRequest::Folders {
            parent: None,
            cursor: None
        }),
        Err(StorageLibraryError::Store(StorageDocumentError::Closed))
    ));
    Ok(())
}

#[tokio::test]
async fn exactly_one_hundred_rows_need_no_continuation() -> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    fixture.populate_budget_catalog().await?;
    let metadata = fixture.metadata();
    let folders = metadata.folders(None, None).await?;
    let tags = metadata.tags(None).await?;
    assert_eq!(folders.entries().len(), 100);
    assert_eq!(tags.entries().len(), 100);
    assert!(folders.next_cursor().is_none());
    assert!(tags.next_cursor().is_none());
    drop((folders, tags));
    fixture.close().await
}

#[tokio::test]
async fn reparented_folder_reads_keep_the_actual_parent_and_revision()
-> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    let parent = fixture.folder(1, None).await?;
    let child = fixture.folder(2, Some(parent)).await?;
    let metadata = fixture.metadata();
    let before = metadata.folders(None, None).await?;
    fixture
        .organize(GameLibraryOrganizationEnumMutation::ReparentFolder(
            GameLibraryOrganizationFolderTarget::new(child, GameLibraryOrganizationRevision::ZERO),
            None,
        ))
        .await?;
    assert!(matches!(
        metadata.folder(child, before.catalog_revision()).await,
        Err(StorageLibraryError::StaleCursor)
    ));
    let after = metadata.folders(None, None).await?;
    assert_eq!(after.entries().len(), 2);
    let moved = &after.entries()[1];
    assert_eq!(moved.id(), child);
    assert_eq!(moved.parent(), None);
    assert_eq!(moved.revision().value(), 1);
    let empty = metadata.folders(Some(parent), None).await?;
    assert!(empty.entries().is_empty());
    drop((before, after, empty));
    fixture.close().await
}

#[tokio::test]
async fn orphaned_rows_are_not_reinterpreted_as_roots() -> StorageLibraryMetadataTestResult {
    let fixture = StorageLibraryMetadataFixture::new().await?;
    let root = fixture.folder(1, None).await?;
    let orphan = StorageLibraryMetadataFixture::folder_id(2)?;
    let missing_parent = StorageLibraryMetadataFixture::folder_id(404)?;
    let path = fixture._directory.path().join("metadata.sqlite");
    fixture.store.close().await?;
    {
        let connection = rusqlite::Connection::open(&path)?;
        connection.pragma_update(None, "foreign_keys", false)?;
        connection.execute(
            "INSERT INTO library_folders(folder_id, parent_id, revision, name) VALUES (?1, ?2, '0', ?3)",
            rusqlite::params![orphan.to_string(), missing_parent.to_string(), "Actual orphan"],
        )?;
    }
    let store = StorageDocumentStore::open(path, fixture.budget.clone()).await?;
    let metadata = StorageLibraryMetadataRepository::new(store.client());
    let roots = metadata.folders(None, None).await?;
    assert_eq!(roots.entries().len(), 1);
    assert_eq!(roots.entries()[0].id(), root);
    let read = metadata.folder(orphan, roots.catalog_revision()).await?;
    assert_eq!(read.folder().parent(), Some(missing_parent));
    assert!(matches!(
        metadata.folders(Some(missing_parent), None).await,
        Err(StorageLibraryError::MissingFolder(id)) if id == missing_parent
    ));
    drop((roots, read));
    store.close().await?;
    fixture.executor.shutdown().await?;
    Ok(())
}
