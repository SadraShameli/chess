use std::path::PathBuf;
use std::sync::Arc;

use chess::board::enums::BoardEnumColor;
use chess::board::start::BoardStart;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::ClockInstant;
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::library_commands::{
    DesktopLibraryCommandsConfirmation, DesktopLibraryCommandsLifecycleProvider,
};
use chess::game::enums::{GameEnumMode, GameEnumSeat, GameId};
use chess::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationFolderId,
    GameLibraryOrganizationFolderTarget, GameLibraryOrganizationGameTarget,
    GameLibraryOrganizationName, GameLibraryOrganizationRequest, GameLibraryOrganizationTimestamp,
};
use chess::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
use chess::game::storage_repository::GameStorageRevision;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::game_document::StorageGameDocument;
use chess::storage::library_prepared::StorageLibraryEnumContentChange;
use chess::storage::library_repository::{
    StorageLibraryEnumMutationReceipt, StorageLibraryError, StorageLibraryRepository,
};
use chess::storage::trash::{
    StorageTrashEnumAppliedAction, StorageTrashPreviewRequest, StorageTrashRestoreRequest,
};

use crate::storage_sqlite_inspector::StorageSqliteInspector;

type StorageLibraryTrashTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, thiserror::Error)]
#[error("fixture lifecycle has no closed game")]
struct StorageLibraryTrashLifecycleError;

struct StorageLibraryTrashClosed(GameStorageRevision);

impl DesktopLibraryCommandsLifecycleProvider for StorageLibraryTrashClosed {
    type Error = StorageLibraryTrashLifecycleError;

    fn closed_revision(&self, _game: GameId) -> Result<Option<GameStorageRevision>, Self::Error> {
        Ok(Some(self.0))
    }
}

struct StorageLibraryTrashFixture {
    directory: tempfile::TempDir,
    executor: Arc<CapacityExecutor>,
    store: StorageDocumentStore,
}

impl StorageLibraryTrashFixture {
    const FILE: &'static str = "trash.sqlite";

    async fn new() -> StorageLibraryTrashTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::discover()?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store = StorageDocumentStore::open(directory.path().join(Self::FILE), budget).await?;
        Ok(Self {
            directory,
            executor,
            store,
        })
    }

    fn path(&self) -> PathBuf {
        self.directory.path().join(Self::FILE)
    }

    fn id(value: u128) -> StorageLibraryTrashTestResult<GameId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn timestamp(value: i64) -> StorageLibraryTrashTestResult<GameLibraryOrganizationTimestamp> {
        Ok(GameLibraryOrganizationTimestamp::new(value)?)
    }

    fn repository(&self) -> StorageLibraryRepository<'_> {
        StorageLibraryRepository::new(self.store.client(), &self.executor)
    }

    fn body(id: GameId) -> StorageLibraryTrashTestResult<String> {
        let session = GameSession::new(GameSessionOptions {
            id,
            root: uuid::Uuid::from_u128(10_000).to_string().parse()?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            clock: ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White),
            metadata: GameMetadata::default(),
        })?;
        Ok(StorageGameDocument::capture(&session, ClockInstant::from_nanos(0))?.json()?)
    }

    async fn save(&self, id: GameId) -> StorageLibraryTrashTestResult {
        let repository = self.repository();
        let prepared = repository
            .prepare_write(
                id,
                GameStorageRevision::INITIAL,
                Self::body(id)?,
                StorageLibraryEnumContentChange::Changed(Self::timestamp(1)?),
            )
            .await?;
        assert!(matches!(
            repository.begin_save(prepared)?.wait().await?,
            StorageLibraryEnumMutationReceipt::Saved(_)
        ));
        Ok(())
    }

    fn scalar(&self, id: GameId, column: &str) -> StorageLibraryTrashTestResult<String> {
        Ok(StorageSqliteInspector::open(self.path())?.text(&format!(
            "SELECT COALESCE(CAST({column} AS TEXT), 'NULL') AS value FROM library_entries WHERE game_id = '{}'",
            id.as_uuid()
        ))?)
    }

    fn game_target(
        &self,
        id: GameId,
    ) -> StorageLibraryTrashTestResult<GameLibraryOrganizationGameTarget> {
        Ok(GameLibraryOrganizationGameTarget::new(
            id,
            self.scalar(id, "organization_revision")?.parse()?,
        ))
    }

    async fn folder(
        &self,
        value: u128,
        timestamp: i64,
    ) -> StorageLibraryTrashTestResult<(
        GameLibraryOrganizationFolderId,
        GameLibraryOrganizationFolderTarget,
    )> {
        let folder: GameLibraryOrganizationFolderId =
            uuid::Uuid::from_u128(value).to_string().parse()?;
        let receipt = self
            .repository()
            .organize(GameLibraryOrganizationRequest::new(
                Self::timestamp(timestamp)?,
                GameLibraryOrganizationEnumMutation::CreateFolder(
                    folder,
                    None,
                    GameLibraryOrganizationName::new("Openings".to_owned())?,
                ),
            )?)
            .await?;
        Ok((
            folder,
            GameLibraryOrganizationFolderTarget::new(folder, receipt.revision()),
        ))
    }

    async fn file_into(
        &self,
        id: GameId,
        folder: GameLibraryOrganizationFolderTarget,
    ) -> StorageLibraryTrashTestResult {
        self.repository()
            .organize(GameLibraryOrganizationRequest::new(
                Self::timestamp(3)?,
                GameLibraryOrganizationEnumMutation::MoveToFolder(
                    self.game_target(id)?,
                    Some(folder),
                ),
            )?)
            .await?;
        Ok(())
    }

    async fn destroy(
        &self,
        request: StorageTrashPreviewRequest,
        timestamp: i64,
    ) -> StorageLibraryTrashTestResult<StorageLibraryEnumMutationReceipt> {
        let plan = self.repository().preview_destruction(request).await?;
        let confirmed = DesktopLibraryCommandsConfirmation::confirm(plan).prepare(
            &StorageLibraryTrashClosed(GameStorageRevision::new(1)),
            Self::timestamp(timestamp)?,
        )?;
        Ok(self
            .repository()
            .begin_destruction(confirmed)?
            .wait()
            .await?)
    }

    async fn restore(
        &self,
        id: GameId,
        timestamp: i64,
    ) -> Result<StorageLibraryEnumMutationReceipt, StorageLibraryError> {
        self.repository()
            .begin_restore(StorageTrashRestoreRequest::new(
                self.game_target(id)
                    .map_err(|_error| StorageLibraryError::Protocol)?,
                GameLibraryOrganizationTimestamp::new(timestamp)?,
            ))?
            .wait()
            .await
    }

    async fn close(self) -> StorageLibraryTrashTestResult {
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|_owner| "trash executor still borrowed")?
            .shutdown()
            .await?;
        self.directory.close()?;
        Ok(())
    }
}

#[tokio::test]
async fn restore_keeps_a_surviving_folder_the_same_body_and_has_no_expiry()
-> StorageLibraryTrashTestResult {
    let fixture = StorageLibraryTrashFixture::new().await?;
    let id = StorageLibraryTrashFixture::id(1)?;
    fixture.save(id).await?;
    let (folder, target) = fixture.folder(2, 2).await?;
    fixture.file_into(id, target).await?;
    let body_before = fixture.scalar(id, "hex(content_sha256)")?;
    let receipt = fixture
        .destroy(StorageTrashPreviewRequest::MoveToTrash(id), 10)
        .await?;
    assert!(matches!(
        receipt,
        StorageLibraryEnumMutationReceipt::DestructionApplied(ref applied)
            if applied.action() == StorageTrashEnumAppliedAction::Trashed(id)
    ));
    assert_eq!(fixture.scalar(id, "location")?, "trash");
    assert_eq!(fixture.scalar(id, "trashed_ms")?, "10");
    let far_future = i64::MAX / 2;
    let restored = fixture.restore(id, far_future).await?;
    assert!(matches!(
        restored,
        StorageLibraryEnumMutationReceipt::Restored(ref applied)
            if applied.action() == StorageTrashEnumAppliedAction::Restored(id)
    ));
    assert_eq!(fixture.scalar(id, "location")?, "library");
    assert_eq!(fixture.scalar(id, "trashed_ms")?, "NULL");
    assert_eq!(fixture.scalar(id, "folder_id")?, folder.to_string());
    assert_eq!(fixture.scalar(id, "hex(content_sha256)")?, body_before);
    fixture.close().await
}

#[tokio::test]
async fn restore_after_the_folder_was_removed_lands_in_unfiled() -> StorageLibraryTrashTestResult {
    let fixture = StorageLibraryTrashFixture::new().await?;
    let id = StorageLibraryTrashFixture::id(1)?;
    fixture.save(id).await?;
    let (folder, target) = fixture.folder(2, 2).await?;
    fixture.file_into(id, target).await?;
    fixture
        .destroy(StorageTrashPreviewRequest::MoveToTrash(id), 10)
        .await?;
    let plan = fixture
        .repository()
        .preview_destruction(StorageTrashPreviewRequest::RemoveFolder(folder))
        .await?;
    assert_eq!(plan.affected_game_count(), 1);
    let confirmed = DesktopLibraryCommandsConfirmation::confirm(plan).prepare(
        &StorageLibraryTrashClosed(GameStorageRevision::new(1)),
        StorageLibraryTrashFixture::timestamp(11)?,
    )?;
    fixture
        .repository()
        .begin_destruction(confirmed)?
        .wait()
        .await?;
    assert_eq!(fixture.scalar(id, "location")?, "trash");
    fixture.restore(id, 12).await?;
    assert_eq!(fixture.scalar(id, "location")?, "library");
    assert_eq!(fixture.scalar(id, "folder_id")?, "NULL");
    fixture.close().await
}

#[tokio::test]
async fn restore_refuses_library_games_stale_revisions_and_never_replaces_an_id()
-> StorageLibraryTrashTestResult {
    let fixture = StorageLibraryTrashFixture::new().await?;
    let id = StorageLibraryTrashFixture::id(1)?;
    fixture.save(id).await?;
    assert!(matches!(
        fixture.restore(id, 5).await,
        Err(StorageLibraryError::InLibrary(found)) if found == id
    ));
    fixture
        .destroy(StorageTrashPreviewRequest::MoveToTrash(id), 10)
        .await?;
    let stale = fixture
        .repository()
        .begin_restore(StorageTrashRestoreRequest::new(
            GameLibraryOrganizationGameTarget::new(id, "999".parse()?),
            StorageLibraryTrashFixture::timestamp(11)?,
        ))?
        .wait()
        .await;
    assert!(stale.is_err());
    let before = fixture.scalar(id, "hex(content_sha256)")?;
    let repository = fixture.repository();
    let replacement = repository
        .prepare_write(
            id,
            GameStorageRevision::INITIAL,
            StorageLibraryTrashFixture::body(id)?,
            StorageLibraryEnumContentChange::Changed(StorageLibraryTrashFixture::timestamp(12)?),
        )
        .await?;
    assert!(repository.begin_save(replacement)?.wait().await.is_err());
    assert_eq!(fixture.scalar(id, "hex(content_sha256)")?, before);
    assert_eq!(fixture.scalar(id, "location")?, "trash");
    fixture.close().await
}

#[tokio::test]
async fn permanent_delete_requires_trash_and_removes_the_game_and_its_projection()
-> StorageLibraryTrashTestResult {
    let fixture = StorageLibraryTrashFixture::new().await?;
    let id = StorageLibraryTrashFixture::id(1)?;
    fixture.save(id).await?;
    assert!(matches!(
        fixture
            .repository()
            .preview_destruction(StorageTrashPreviewRequest::PermanentlyDelete(id))
            .await,
        Err(StorageLibraryError::InLibrary(found)) if found == id
    ));
    fixture
        .destroy(StorageTrashPreviewRequest::MoveToTrash(id), 10)
        .await?;
    let receipt = fixture
        .destroy(StorageTrashPreviewRequest::PermanentlyDelete(id), 11)
        .await?;
    assert!(matches!(
        receipt,
        StorageLibraryEnumMutationReceipt::DestructionApplied(ref applied)
            if applied.action() == StorageTrashEnumAppliedAction::Deleted(id)
    ));
    assert!(fixture.repository().load(id).await?.is_none());
    let remaining = StorageSqliteInspector::open(fixture.path())?.integer(&format!(
        "SELECT COUNT(*) AS value FROM library_entries WHERE game_id = '{}'",
        id.as_uuid()
    ))?;
    assert_eq!(remaining, 0);
    assert!(matches!(
        fixture.restore(id, 12).await,
        Err(StorageLibraryError::MissingGame(_)) | Err(StorageLibraryError::Protocol)
    ));
    fixture.close().await
}
