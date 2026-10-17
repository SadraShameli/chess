use std::sync::Arc;

use diesel::prelude::*;
use diesel::sql_types::Text;

use chess::board::enums::BoardEnumColor;
use chess::board::start::BoardStart;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::ClockInstant;
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::game::enums::{GameEnumMode, GameEnumSeat, GameId};
use chess::game::library_organization::GameLibraryOrganizationTimestamp;
use chess::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
use chess::game::storage_repository::GameStorageRevision;
use chess::game::tree::GameTree;
use chess::notation::game::{NotationGame, NotationGameParts};
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::storage::connection::StorageConnection;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::game_document::StorageGameDocument;
use chess::storage::library_inventory::{
    StorageLibraryInventoryCursor, StorageLibraryInventoryRepository,
};
use chess::storage::library_prepared::StorageLibraryEnumContentChange;
use chess::storage::library_repository::{
    StorageLibraryEnumMutationReceipt, StorageLibraryRepository,
};

type StorageLibraryInventoryTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct StorageLibraryInventoryFixture {
    directory: tempfile::TempDir,
    budget: CapacityBudget,
    executor: Arc<CapacityExecutor>,
    store: StorageDocumentStore,
}

impl StorageLibraryInventoryFixture {
    fn id(value: u128) -> StorageLibraryInventoryTestResult<GameId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    async fn new() -> StorageLibraryInventoryTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::discover()?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store =
            StorageDocumentStore::open(directory.path().join("inventory.sqlite"), budget.clone())
                .await?;
        Ok(Self {
            directory,
            budget,
            executor,
            store,
        })
    }

    async fn played(
        &self,
        value: u128,
        timestamp: i64,
    ) -> StorageLibraryInventoryTestResult<GameId> {
        let id = Self::id(value)?;
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
        let body = StorageGameDocument::capture(&session, ClockInstant::from_nanos(0))?.json()?;
        self.write(id, body, timestamp).await?;
        Ok(id)
    }

    fn notation(id: GameId) -> StorageLibraryInventoryTestResult<String> {
        let start = BoardStart::standard();
        let root = uuid::Uuid::from_u128(10_000).to_string().parse()?;
        let game = NotationGame::new(NotationGameParts {
            tree: GameTree::new(root, start.position().clone(), None),
            start,
            mainline_tip: root,
            metadata: GameMetadata::default(),
            result: NotationGameIdentityEnumResult::Unfinished,
            result_header: None,
            time_control: None,
            clocks: Vec::new(),
            warnings: Vec::new(),
        })?;
        Ok(StorageGameDocument::capture_notation(id, &game)?.json()?)
    }

    async fn write(
        &self,
        id: GameId,
        body: String,
        timestamp: i64,
    ) -> StorageLibraryInventoryTestResult {
        let repository = StorageLibraryRepository::new(self.store.client(), &self.executor);
        let prepared = repository
            .prepare_write(
                id,
                GameStorageRevision::INITIAL,
                body,
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(
                    timestamp,
                )?),
            )
            .await?;
        assert!(matches!(
            repository.begin_save(prepared)?.wait().await?,
            StorageLibraryEnumMutationReceipt::Saved(_)
        ));
        Ok(())
    }

    async fn close(self) -> StorageLibraryInventoryTestResult {
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|_owner| "inventory executor still borrowed")?
            .shutdown()
            .await?;
        self.directory.close()?;
        Ok(())
    }
}

#[tokio::test]
async fn played_inventory_pages_by_id_and_excludes_notation_trash_and_stale_projection()
-> StorageLibraryInventoryTestResult {
    let mut fixture = StorageLibraryInventoryFixture::new().await?;
    for value in (1..=105).rev() {
        fixture.played(value, i64::try_from(value)? * 100).await?;
    }
    let notation = StorageLibraryInventoryFixture::id(106)?;
    fixture
        .write(
            notation,
            StorageLibraryInventoryFixture::notation(notation)?,
            1_000_000,
        )
        .await?;
    let trash = fixture.played(107, 2_000_000).await?;
    let stale = fixture.played(108, 3_000_000).await?;
    fixture.store.close().await?;
    {
        let mut connection =
            StorageConnection::open(&fixture.directory.path().join("inventory.sqlite"))?;
        diesel::sql_query(
            "UPDATE library_entries SET location = 'trash', trashed_ms = 1234 WHERE game_id = ?",
        )
        .bind::<Text, _>(trash.as_uuid().to_string())
        .execute(connection.diesel())?;
        diesel::sql_query("UPDATE documents SET revision = '2' WHERE id = ?")
            .bind::<Text, _>(stale.as_uuid().to_string())
            .execute(connection.diesel())?;
    }
    fixture.store = StorageDocumentStore::open(
        fixture.directory.path().join("inventory.sqlite"),
        fixture.budget.clone(),
    )
    .await?;
    let inventory = StorageLibraryInventoryRepository::new(fixture.store.client());
    let first = inventory
        .page(StorageLibraryInventoryCursor::start())
        .await?;
    assert_eq!(first.entries().len(), 100);
    for (entry, value) in first.entries().iter().zip(1..=100) {
        assert_eq!(entry.id(), StorageLibraryInventoryFixture::id(value)?);
        assert_eq!(entry.content_revision(), GameStorageRevision::new(1));
    }
    let cursor = first.next_cursor().ok_or("missing ID continuation")?;
    let second = inventory.page(cursor).await?;
    assert_eq!(second.entries().len(), 5);
    assert_eq!(
        second.entries()[0].id(),
        StorageLibraryInventoryFixture::id(101)?
    );
    assert_eq!(
        second.entries()[4].id(),
        StorageLibraryInventoryFixture::id(105)?
    );
    assert!(second.next_cursor().is_none());
    let repeated = inventory.page(cursor).await?;
    assert_eq!(repeated.entries(), second.entries());
    drop(repeated);
    drop(second);
    drop(first);
    fixture.close().await
}

#[tokio::test]
async fn retained_inventory_page_uses_the_existing_document_byte_budget()
-> StorageLibraryInventoryTestResult {
    let fixture = StorageLibraryInventoryFixture::new().await?;
    fixture.played(1, 1_000).await?;
    let client = fixture.store.client();
    let inventory = StorageLibraryInventoryRepository::new(client.clone());
    let page = inventory
        .page(StorageLibraryInventoryCursor::start())
        .await?;
    let id = StorageLibraryInventoryFixture::id(500)?;
    let reserved = client
        .save(
            id.as_uuid(),
            0,
            "x".repeat(StorageDocumentStore::MEMORY_BUDGET),
        )
        .await;
    assert!(matches!(
        reserved,
        Err(chess::storage::document_store::StorageDocumentError::Saturated)
    ));
    drop(page);
    let released = client
        .save(
            id.as_uuid(),
            0,
            "x".repeat(StorageDocumentStore::MEMORY_BUDGET),
        )
        .await;
    assert!(matches!(
        released,
        Err(chess::storage::document_store::StorageDocumentError::Json(
            _
        ))
    ));
    assert!(client.read(id.as_uuid()).await?.is_none());
    fixture.close().await
}
