use std::future::{Future, poll_fn};
use std::io::Cursor;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::task::Poll;

use crate::board::enums::BoardEnumColor;
use crate::board::start::BoardStart;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::clock::state::ClockState;
use crate::clock::time_control::ClockTimeControl;
use crate::game::enums::{GameEnumMode, GameEnumSeat, GameId};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
use crate::game::storage_repository::GameStorageRevision;
use crate::notation::game::NotationGame;
use crate::notation::pgn_import::NotationPgnImport;
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use crate::storage::game_document::{
    StorageGameDocument, StorageGameDocumentEnumContentKind, StorageGameDocumentError,
};
use crate::storage::library_notation::StorageLibraryNotationRepository;
use crate::storage::library_prepared::StorageLibraryEnumContentChange;
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};
use crate::storage::library_repository::{
    StorageLibraryContentReceipt, StorageLibraryEnumMutationReceipt, StorageLibraryError,
    StorageLibraryRepository,
};

type StorageLibraryNotationTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct StorageLibraryNotationFixture {
    _directory: tempfile::TempDir,
    budget: CapacityBudget,
    store: StorageDocumentStore,
    executor: CapacityExecutor,
}

impl StorageLibraryNotationFixture {
    async fn new() -> StorageLibraryNotationTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let store =
            StorageDocumentStore::open(directory.path().join("notation.sqlite"), budget.clone())
                .await?;
        let executor = CapacityExecutor::new(budget.clone())?;
        Ok(Self {
            _directory: directory,
            budget,
            store,
            executor,
        })
    }

    fn id() -> StorageLibraryNotationTestResult<GameId> {
        Ok("e707c2dd-3e4a-4a22-8159-47fa63696071".parse()?)
    }

    fn game() -> StorageLibraryNotationTestResult<Arc<StorageLibraryRecord>> {
        let mut node = 0_u128;
        let mut importer = NotationPgnImport::new(
            Cursor::new(
                "[White \"White\"]\n[Black \"Black\"]\n[WhiteElo \"?\"]\n[TimeControl \"300+2\"]\n[Custom \"retained\"]\n\n{root [%unknown kept]} 1. e4 {note [%clk 0:00:04.123456789]} (1. d4 {branch}) e5 *",
            ),
            move || {
                node += 1;
                uuid::Uuid::from_u128(node).to_string().parse()
            },
        );
        Ok(Arc::new(StorageLibraryRecord::new(
            Self::id()?,
            StorageLibraryEnumContent::Notation(Box::new(
                importer.next_game(&|| false)?.ok_or("missing notation")?,
            )),
        )?))
    }

    fn notation(record: &StorageLibraryRecord) -> StorageLibraryNotationTestResult<&NotationGame> {
        match record.content() {
            StorageLibraryEnumContent::Notation(game) => Ok(game),
            StorageLibraryEnumContent::Played(_) => Err("expected notation source".into()),
        }
    }

    fn repository(&self) -> StorageLibraryNotationRepository<'_> {
        StorageLibraryNotationRepository::new(self.store.client(), &self.executor)
    }

    fn library(&self) -> StorageLibraryRepository<'_> {
        StorageLibraryRepository::new(self.store.client(), &self.executor)
    }

    async fn save(
        &self,
        game: &Arc<StorageLibraryRecord>,
        expected: GameStorageRevision,
        change: StorageLibraryEnumContentChange,
    ) -> StorageLibraryNotationTestResult<StorageLibraryContentReceipt> {
        let repository = self.repository();
        let prepared = repository
            .prepare(expected, Arc::clone(game), change)
            .await?;
        let mut pending = repository.begin_save(prepared)?;
        let StorageLibraryEnumMutationReceipt::Saved(receipt) = pending.wait().await? else {
            return Err("expected notation save receipt".into());
        };
        Ok(receipt)
    }

    fn changed(value: i64) -> StorageLibraryNotationTestResult<StorageLibraryEnumContentChange> {
        Ok(StorageLibraryEnumContentChange::Changed(
            GameLibraryOrganizationTimestamp::new(value)?,
        ))
    }

    async fn close(self) -> StorageLibraryNotationTestResult {
        self.store.close().await?;
        self.executor.shutdown().await?;
        Ok(())
    }
}

#[tokio::test]
async fn prepared_notation_preserves_the_authoritative_game_and_canonical_annotations()
-> StorageLibraryNotationTestResult {
    let fixture = StorageLibraryNotationFixture::new().await?;
    let mut game = StorageLibraryNotationFixture::game()?;
    let id = StorageLibraryNotationFixture::id()?;
    let original_notes = StorageLibraryNotationFixture::notation(&game)?
        .tree()
        .node(
            StorageLibraryNotationFixture::notation(&game)?
                .tree()
                .root(),
        )?
        .annotations()
        .notes()
        .to_owned();
    let original =
        StorageGameDocument::capture_notation(id, StorageLibraryNotationFixture::notation(&game)?)?
            .json()?;
    let receipt = fixture
        .save(
            &game,
            GameStorageRevision::INITIAL,
            StorageLibraryNotationFixture::changed(1_000)?,
        )
        .await?;
    assert_eq!(receipt.id(), id);
    assert_eq!(receipt.content_revision(), GameStorageRevision::new(1));
    assert_eq!(Arc::strong_count(&game), 1);
    assert_eq!(
        StorageGameDocument::capture_notation(id, StorageLibraryNotationFixture::notation(&game)?)?
            .json()?,
        original
    );
    let recovered = fixture
        .library()
        .load(id)
        .await?
        .ok_or("missing saved notation")?;
    assert_eq!(
        recovered.modified(),
        Some(GameLibraryOrganizationTimestamp::new(1_000)?)
    );
    let record = recovered.into_record();
    let StorageLibraryEnumContent::Notation(saved) = record.into_content() else {
        return Err("notation was changed to played content".into());
    };
    assert_eq!(
        StorageGameDocument::capture_notation(id, &saved)?.json()?,
        original
    );
    let root = StorageLibraryNotationFixture::notation(&game)?
        .tree()
        .root();
    Arc::get_mut(&mut game)
        .ok_or("preparation retained the authoritative game")?
        .notation_mut()
        .ok_or("expected notation")?
        .save_notes_and_nags(root, "new accepted draft".to_owned(), Vec::new())?;
    assert_eq!(
        saved.tree().node(root)?.annotations().notes(),
        original_notes
    );
    fixture.close().await
}

#[tokio::test]
async fn notation_checkpoint_retains_modified_time_and_conflict_preserves_newest_source()
-> StorageLibraryNotationTestResult {
    let fixture = StorageLibraryNotationFixture::new().await?;
    let mut game = StorageLibraryNotationFixture::game()?;
    let first = fixture
        .save(
            &game,
            GameStorageRevision::INITIAL,
            StorageLibraryNotationFixture::changed(2_000)?,
        )
        .await?;
    let checkpoint = fixture
        .save(
            &game,
            first.content_revision(),
            StorageLibraryEnumContentChange::Checkpoint,
        )
        .await?;
    let root = StorageLibraryNotationFixture::notation(&game)?
        .tree()
        .root();
    Arc::get_mut(&mut game)
        .ok_or("source remains shared")?
        .notation_mut()
        .ok_or("expected notation")?
        .save_notes_and_nags(root, "accepted newest notes".to_owned(), Vec::new())?;
    let prepared = fixture
        .repository()
        .prepare(
            first.content_revision(),
            Arc::clone(&game),
            StorageLibraryNotationFixture::changed(3_000)?,
        )
        .await?;
    let mut pending = fixture.repository().begin_save(prepared)?;
    assert!(matches!(
        pending.wait().await,
        Err(StorageLibraryError::ContentConflict { .. })
    ));
    assert_eq!(
        StorageLibraryNotationFixture::notation(&game)?
            .tree()
            .node(root)?
            .annotations()
            .notes(),
        "accepted newest notes"
    );
    assert_eq!(Arc::strong_count(&game), 1);
    let recovered = fixture
        .library()
        .load(StorageLibraryNotationFixture::id()?)
        .await?
        .ok_or("missing checkpoint")?;
    assert_eq!(recovered.content_revision(), checkpoint.content_revision());
    assert_eq!(
        recovered.modified(),
        Some(GameLibraryOrganizationTimestamp::new(2_000)?)
    );
    drop(recovered);
    let retried = fixture
        .save(
            &game,
            checkpoint.content_revision(),
            StorageLibraryNotationFixture::changed(3_000)?,
        )
        .await?;
    let recovered = fixture
        .library()
        .load(retried.id())
        .await?
        .ok_or("missing retry")?;
    assert_eq!(
        recovered.modified(),
        Some(GameLibraryOrganizationTimestamp::new(3_000)?)
    );
    drop(recovered);
    fixture.close().await
}

#[tokio::test]
async fn exhausted_notation_budget_refuses_before_waiting_for_encoding_capacity()
-> StorageLibraryNotationTestResult {
    let fixture = StorageLibraryNotationFixture::new().await?;
    let game = StorageLibraryNotationFixture::game()?;
    let client = fixture.store.client();
    let pressure = client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    let capacity = fixture.budget.lease(1).await?;
    let repository = fixture.repository();
    let observed = {
        let mut preparing = std::pin::pin!(repository.prepare(
            GameStorageRevision::INITIAL,
            Arc::clone(&game),
            StorageLibraryNotationFixture::changed(4_000)?,
        ));
        poll_fn(|context| Poll::Ready(preparing.as_mut().poll(context))).await
    };
    let refused = matches!(
        observed,
        Poll::Ready(Err(StorageLibraryError::Store(
            StorageDocumentError::Saturated
        )))
    );
    drop((pressure, capacity));
    fixture
        .executor
        .submit_waiting(|_cancellation| ())
        .await?
        .join()
        .await?;
    assert_eq!(Arc::strong_count(&game), 1);
    drop(client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?);
    fixture.close().await?;
    assert!(refused);
    Ok(())
}

#[tokio::test]
async fn cancelling_unadmitted_notation_preparation_releases_source_and_reserved_bytes()
-> StorageLibraryNotationTestResult {
    let fixture = StorageLibraryNotationFixture::new().await?;
    let game = StorageLibraryNotationFixture::game()?;
    let capacity = fixture.budget.lease(1).await?;
    let repository = fixture.repository();
    {
        let mut preparing = std::pin::pin!(repository.prepare(
            GameStorageRevision::INITIAL,
            Arc::clone(&game),
            StorageLibraryNotationFixture::changed(5_000)?,
        ));
        let observed = poll_fn(|context| Poll::Ready(preparing.as_mut().poll(context))).await;
        assert!(observed.is_pending());
        assert_eq!(Arc::strong_count(&game), 2);
    }
    drop(capacity);
    fixture
        .executor
        .submit_waiting(|_cancellation| ())
        .await?
        .join()
        .await?;
    assert_eq!(Arc::strong_count(&game), 1);
    drop(
        fixture
            .store
            .client()
            .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?,
    );
    fixture.close().await
}

#[tokio::test]
async fn prepared_notation_bytes_stay_charged_and_foreign_client_cannot_commit_them()
-> StorageLibraryNotationTestResult {
    let source = StorageLibraryNotationFixture::new().await?;
    let destination = StorageLibraryNotationFixture::new().await?;
    let game = StorageLibraryNotationFixture::game()?;
    let prepared = source
        .repository()
        .prepare(
            GameStorageRevision::INITIAL,
            Arc::clone(&game),
            StorageLibraryNotationFixture::changed(6_000)?,
        )
        .await?;
    assert!(matches!(
        source
            .store
            .client()
            .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET),
        Err(StorageDocumentError::Saturated)
    ));
    assert!(matches!(
        destination.repository().begin_save(prepared),
        Err(StorageLibraryError::ForeignPreparation)
    ));
    drop(
        source
            .store
            .client()
            .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?,
    );
    assert!(
        source
            .library()
            .load(StorageLibraryNotationFixture::id()?)
            .await?
            .is_none()
    );
    assert!(
        destination
            .library()
            .load(StorageLibraryNotationFixture::id()?)
            .await?
            .is_none()
    );
    destination.close().await?;
    source.close().await
}

#[tokio::test]
async fn new_notation_checkpoint_refusal_keeps_the_original_game_available()
-> StorageLibraryNotationTestResult {
    let fixture = StorageLibraryNotationFixture::new().await?;
    let game = StorageLibraryNotationFixture::game()?;
    let prepared = fixture
        .repository()
        .prepare(
            GameStorageRevision::INITIAL,
            Arc::clone(&game),
            StorageLibraryEnumContentChange::Checkpoint,
        )
        .await?;
    let mut pending = fixture.repository().begin_save(prepared)?;
    assert!(matches!(
        pending.wait().await,
        Err(StorageLibraryError::NewContentTimestamp)
    ));
    assert_eq!(Arc::strong_count(&game), 1);
    assert!(
        fixture
            .library()
            .load(StorageLibraryNotationFixture::id()?)
            .await?
            .is_none()
    );
    fixture.close().await
}

#[tokio::test]
async fn notation_dto_capacity_refusal_retains_large_edited_source_and_releases_preparation()
-> StorageLibraryNotationTestResult {
    let fixture = StorageLibraryNotationFixture::new().await?;
    let mut game = StorageLibraryNotationFixture::game()?;
    let root = StorageLibraryNotationFixture::notation(&game)?
        .tree()
        .root();
    let notes = "escaped\n\\\"unicode🙂".repeat(16 * 1024);
    Arc::get_mut(&mut game)
        .ok_or("source remains shared")?
        .notation_mut()
        .ok_or("expected notation")?
        .save_notes_and_nags(root, notes.clone(), Vec::new())?;
    let client = fixture.store.client();
    let pressure = client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - 4096)?;
    let observed = fixture
        .repository()
        .prepare(
            GameStorageRevision::INITIAL,
            Arc::clone(&game),
            StorageLibraryNotationFixture::changed(7_000)?,
        )
        .await;
    let refused = matches!(
        observed,
        Err(StorageLibraryError::Store(StorageDocumentError::Saturated))
    );
    assert_eq!(Arc::strong_count(&game), 1);
    assert_eq!(
        StorageLibraryNotationFixture::notation(&game)?
            .tree()
            .node(root)?
            .annotations()
            .notes(),
        notes
    );
    drop(pressure);
    drop(client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?);
    let receipt = fixture
        .save(
            &game,
            GameStorageRevision::INITIAL,
            StorageLibraryNotationFixture::changed(7_000)?,
        )
        .await?;
    let recovered = fixture
        .library()
        .load(receipt.id())
        .await?
        .ok_or("missing large notation")?;
    assert_eq!(
        recovered.record().metadata(),
        StorageLibraryNotationFixture::notation(&game)?.metadata()
    );
    let StorageLibraryEnumContent::Notation(saved) = recovered.record().content() else {
        return Err("expected notation".into());
    };
    assert_eq!(saved.tree().node(root)?.annotations().notes(), notes);
    assert_eq!(
        saved.clocks(),
        StorageLibraryNotationFixture::notation(&game)?.clocks()
    );
    assert_eq!(
        saved.warnings(),
        StorageLibraryNotationFixture::notation(&game)?.warnings()
    );
    drop(recovered);
    fixture.close().await?;
    assert!(refused);
    Ok(())
}

#[tokio::test]
async fn played_source_is_refused_without_changing_its_identity_or_storage()
-> StorageLibraryNotationTestResult {
    let fixture = StorageLibraryNotationFixture::new().await?;
    let id = StorageLibraryNotationFixture::id()?;
    let session = GameSession::new(GameSessionOptions {
        id,
        root: uuid::Uuid::from_u128(1).to_string().parse()?,
        start: BoardStart::standard(),
        seats: GameSeats::new(
            GameEnumMode::LocalTwoPlayer,
            GameEnumSeat::Human,
            GameEnumSeat::Human,
        )?,
        clock: ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White),
        metadata: GameMetadata::default(),
    })?;
    let mut source = Arc::new(StorageLibraryRecord::new(
        id,
        StorageLibraryEnumContent::Played(Box::new(session)),
    )?);
    let observed = fixture
        .repository()
        .prepare(
            GameStorageRevision::INITIAL,
            Arc::clone(&source),
            StorageLibraryNotationFixture::changed(8_000)?,
        )
        .await;
    assert!(matches!(
        observed,
        Err(StorageLibraryError::Codec(
            StorageGameDocumentError::ContentKind {
                expected: StorageGameDocumentEnumContentKind::Notation,
                found: StorageGameDocumentEnumContentKind::Played
            }
        ))
    ));
    assert_eq!(source.id(), id);
    assert!(
        Arc::get_mut(&mut source)
            .ok_or("source remains shared")?
            .notation_mut()
            .is_none()
    );
    assert!(fixture.library().load(id).await?.is_none());
    drop(
        fixture
            .store
            .client()
            .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?,
    );
    fixture.close().await
}
