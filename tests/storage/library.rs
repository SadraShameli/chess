use chess::board::start::BoardStart;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::game::enums::{GameId, GameMoveId};
use chess::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationFolderId,
    GameLibraryOrganizationFolderTarget, GameLibraryOrganizationGameTarget,
    GameLibraryOrganizationName, GameLibraryOrganizationRequest, GameLibraryOrganizationRevision,
    GameLibraryOrganizationTagId, GameLibraryOrganizationTagSet, GameLibraryOrganizationTimestamp,
};
use chess::game::session::{GameMetadata, GameMetadataDate};
use chess::game::storage_repository::GameStorageRevision;
use chess::game::tree::GameTree;
use chess::notation::game::{NotationGame, NotationGameParts};
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::game_document::StorageGameDocument;
use chess::storage::library_prepared::StorageLibraryEnumContentChange;
use chess::storage::library_query::{
    StorageLibraryDateInterval, StorageLibraryEnumFolderFilter, StorageLibraryQuery,
    StorageLibraryQueryOptions,
};
use chess::storage::library_repository::{
    StorageLibraryEnumMutationReceipt, StorageLibraryRepository,
};
use chess::storage::trash::StorageTrashPreviewRequest;

type StorageLibraryTestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn library_boundary_values_reject_noncanonical_revisions_and_invalid_names()
-> StorageLibraryTestResult {
    assert!("01".parse::<GameLibraryOrganizationRevision>().is_err());
    assert!("-1".parse::<GameLibraryOrganizationRevision>().is_err());
    assert_eq!("0".parse::<GameLibraryOrganizationRevision>()?.value(), 0);
    assert!(GameLibraryOrganizationTimestamp::new(-1).is_err());
    assert!(GameLibraryOrganizationName::new("  ".to_owned()).is_err());
    assert!(GameLibraryOrganizationName::new("bad\0name".to_owned()).is_err());
    assert_eq!(
        GameLibraryOrganizationName::new(" Study ".to_owned())?.as_str(),
        "Study"
    );
    Ok(())
}

#[test]
fn interval_requires_complete_ordered_calendar_boundaries() -> StorageLibraryTestResult {
    let full = GameMetadataDate::new(Some(2024), Some(2), Some(29))?;
    assert!(StorageLibraryDateInterval::new(full, full).is_ok());
    assert!(
        StorageLibraryDateInterval::new(GameMetadataDate::new(Some(2024), None, Some(29))?, full)
            .is_err()
    );
    assert!(
        StorageLibraryDateInterval::new(GameMetadataDate::new(Some(2025), Some(1), Some(1))?, full)
            .is_err()
    );
    Ok(())
}

struct StorageLibraryFixture {
    directory: tempfile::TempDir,
    store: StorageDocumentStore,
    executor: CapacityExecutor,
}

impl StorageLibraryFixture {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::discover()?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let store =
            StorageDocumentStore::open(directory.path().join("library.sqlite"), budget).await?;
        Ok(Self {
            directory,
            store,
            executor,
        })
    }

    fn repository(&self) -> StorageLibraryRepository<'_> {
        StorageLibraryRepository::new(self.store.client(), &self.executor)
    }

    fn game() -> Result<GameId, Box<dyn std::error::Error>> {
        Ok(uuid::Uuid::from_u128(1).to_string().parse()?)
    }

    fn body(id: GameId) -> Result<String, Box<dyn std::error::Error>> {
        let root: GameMoveId = uuid::Uuid::from_u128(2).to_string().parse()?;
        let tip: GameMoveId = uuid::Uuid::from_u128(3).to_string().parse()?;
        let start = BoardStart::standard();
        let mut tree = GameTree::new(root, start.position().clone(), None);
        let movement = tree.node(root)?.position().parse_uci("e2e4")?;
        tree.append(root, tip, movement, None)?;
        let game = NotationGame::new(NotationGameParts {
            start,
            tree,
            mainline_tip: tip,
            metadata: GameMetadata::default(),
            result: NotationGameIdentityEnumResult::Unfinished,
            result_header: None,
            time_control: None,
            clocks: Vec::new(),
            warnings: Vec::new(),
        })?;
        Ok(StorageGameDocument::capture_notation(id, &game)?.json()?)
    }

    async fn insert(
        &self,
        id: GameId,
        timestamp: i64,
    ) -> Result<
        chess::storage::library_repository::StorageLibraryContentReceipt,
        Box<dyn std::error::Error>,
    > {
        let repository = self.repository();
        let prepared = repository
            .prepare_write(
                id,
                GameStorageRevision::INITIAL,
                Self::body(id)?,
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(
                    timestamp,
                )?),
            )
            .await?;
        let StorageLibraryEnumMutationReceipt::Saved(receipt) =
            repository.begin_save(prepared)?.wait().await?
        else {
            return Err("unexpected save receipt".into());
        };
        Ok(receipt)
    }

    async fn organize(
        &self,
        mutation: GameLibraryOrganizationEnumMutation,
    ) -> Result<
        chess::game::library_organization::GameLibraryOrganizationReceipt,
        Box<dyn std::error::Error>,
    > {
        Ok(self
            .repository()
            .organize(GameLibraryOrganizationRequest::new(
                GameLibraryOrganizationTimestamp::new(5_000)?,
                mutation,
            )?)
            .await?)
    }

    async fn target(
        &self,
        id: GameId,
    ) -> Result<GameLibraryOrganizationGameTarget, Box<dyn std::error::Error>> {
        let loaded = self.repository().load(id).await?.ok_or("missing game")?;
        Ok(GameLibraryOrganizationGameTarget::new(
            id,
            loaded.organization().revision(),
        ))
    }

    async fn close(self) -> StorageLibraryTestResult {
        self.store.close().await?;
        self.executor.shutdown().await?;
        self.directory.close()?;
        Ok(())
    }
}

#[tokio::test]
async fn organization_changes_and_folder_removal_preview_leave_content_and_modified_time()
-> StorageLibraryTestResult {
    let fixture = StorageLibraryFixture::new().await?;
    let id = StorageLibraryFixture::game()?;
    let saved = fixture.insert(id, 1_000).await?;
    let folder: GameLibraryOrganizationFolderId = uuid::Uuid::from_u128(10).to_string().parse()?;
    let tag: GameLibraryOrganizationTagId = uuid::Uuid::from_u128(11).to_string().parse()?;
    fixture
        .organize(GameLibraryOrganizationEnumMutation::CreateFolder(
            folder,
            None,
            GameLibraryOrganizationName::new("Study".to_owned())?,
        ))
        .await?;
    fixture
        .organize(GameLibraryOrganizationEnumMutation::CreateTag(
            tag,
            GameLibraryOrganizationName::new("Keep".to_owned())?,
        ))
        .await?;
    fixture
        .organize(GameLibraryOrganizationEnumMutation::MoveToFolder(
            fixture.target(id).await?,
            Some(GameLibraryOrganizationFolderTarget::new(
                folder,
                GameLibraryOrganizationRevision::ZERO,
            )),
        ))
        .await?;
    fixture
        .organize(GameLibraryOrganizationEnumMutation::ReplaceTags(
            fixture.target(id).await?,
            GameLibraryOrganizationTagSet::new(vec![tag])?,
        ))
        .await?;
    let repository = fixture.repository();
    let filed = repository.load(id).await?.ok_or("missing filed game")?;
    let filed_tags = filed.organization().tags().ids().len();
    assert_eq!(filed.organization().folder(), Some(folder));
    assert_eq!(filed.content_revision(), saved.content_revision());
    assert_eq!(
        filed
            .modified()
            .map(GameLibraryOrganizationTimestamp::value),
        Some(5_000)
    );
    drop(filed);

    let plan = repository
        .preview_destruction(StorageTrashPreviewRequest::RemoveFolder(folder))
        .await?;
    assert_eq!(
        plan.action(),
        StorageTrashPreviewRequest::RemoveFolder(folder)
    );
    assert_eq!(plan.affected_folder_count(), 1);
    assert_eq!(plan.affected_game_count(), 1);
    assert_eq!(filed_tags, 1);
    let page = repository
        .query(StorageLibraryQuery::new(StorageLibraryQueryOptions {
            folder: StorageLibraryEnumFolderFilter::Exact(folder),
            ..StorageLibraryQueryOptions::default()
        })?)
        .await?;
    assert_eq!(page.entries().len(), 1);
    drop((page, plan, repository));
    fixture.close().await
}

#[tokio::test]
async fn trash_preview_counts_the_game_and_refuses_unknown_games() -> StorageLibraryTestResult {
    let fixture = StorageLibraryFixture::new().await?;
    let id = StorageLibraryFixture::game()?;
    fixture.insert(id, 1_000).await?;
    let repository = fixture.repository();
    let plan = repository
        .preview_destruction(StorageTrashPreviewRequest::MoveToTrash(id))
        .await?;
    assert_eq!(plan.affected_game_count(), 1);
    let missing: GameId = uuid::Uuid::from_u128(99).to_string().parse()?;
    assert!(
        repository
            .preview_destruction(StorageTrashPreviewRequest::MoveToTrash(missing))
            .await
            .is_err()
    );
    let still = repository.load(id).await?.ok_or("game vanished")?;
    assert_eq!(still.content_revision().value(), 1);
    drop((still, repository));
    fixture.close().await
}
