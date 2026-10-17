use std::future::{Future, poll_fn};
use std::io::Cursor;
use std::num::NonZeroUsize;
use std::panic::UnwindSafe;
use std::task::Poll;

use rusqlite::{Connection, params};

use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::board::start::BoardStart;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::ClockInstant;
use crate::clock::state::ClockState;
use crate::clock::time_control::ClockTimeControl;
use crate::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use crate::game::enums::{
    GameEnumEngineReadiness, GameEnumMarkingColor, GameEnumMode, GameEnumSeat, GameId,
};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::move_node::{GameMoveNodeAnnotations, GameMoveNodeMarking, GameMoveNodeNag};
use crate::game::pause::{GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseRequest};
use crate::game::session::{
    GameMetadata, GameMetadataExtension, GameMetadataKnownTags, GameMetadataText, GameSeats,
    GameSession, GameSessionMoveRequest, GameSessionOptions,
};
use crate::game::storage_repository::{
    GameStorageContentEdit, GameStorageContentEditError, GameStorageEnumContentChange,
    GameStorageEnumEditCause, GameStorageRepository, GameStorageRevision, GameStorageSaveRequest,
    GameStorageSaveRequestError,
};
use crate::notation::pgn_import::NotationPgnImport;
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use crate::storage::game_document::{
    StorageGameDocument, StorageGameDocumentEnumContentKind, StorageGameDocumentError,
    StorageGameDocumentRepository,
};
use crate::storage::library_played::StorageLibraryPlayedRepository;
use crate::storage::library_prepared::{
    StorageLibraryEnumContentChange, StorageLibraryPreparedMemory,
};
use crate::storage::library_query::StorageLibraryQuery;
use crate::storage::library_repository::{
    StorageLibraryAdoptionCursor, StorageLibraryEnumAdoptionPreparation,
    StorageLibraryEnumMutationReceipt, StorageLibraryError, StorageLibraryRepository,
};

type StorageLibraryPlayedTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct StorageLibraryPlayedFixture {
    directory: tempfile::TempDir,
    budget: CapacityBudget,
    store: StorageDocumentStore,
    executor: CapacityExecutor,
}

impl StorageLibraryPlayedFixture {
    async fn new() -> StorageLibraryPlayedTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let store =
            StorageDocumentStore::open(directory.path().join("library.sqlite"), budget.clone())
                .await?;
        let executor = CapacityExecutor::new(budget.clone())?;
        Ok(Self {
            directory,
            budget,
            store,
            executor,
        })
    }

    fn repository(&self) -> StorageLibraryPlayedRepository<'_> {
        StorageLibraryPlayedRepository::new(self.store.client(), &self.executor)
    }

    fn library(&self) -> StorageLibraryRepository<'_> {
        StorageLibraryRepository::new(self.store.client(), &self.executor)
    }

    async fn inspect<T, F>(&self, operation: F) -> StorageLibraryPlayedTestResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, rusqlite::Error> + Send + UnwindSafe + 'static,
    {
        let path = self.directory.path().join("library.sqlite");
        Ok(self
            .executor
            .submit_waiting(move |_cancellation| operation(&Connection::open(path)?))
            .await?
            .join()
            .await??)
    }

    fn session() -> StorageLibraryPlayedTestResult<GameSession> {
        let mut session = GameSession::new(GameSessionOptions {
            id: "14d36b00-5cf2-42ed-a730-9fa472782852".parse()?,
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
        session.start(
            session.revision(),
            Self::now(),
            GameEnumEngineReadiness::Ready,
            &StorageLibraryPlayedFixtureProof,
        )?;
        Self::play(&mut session, "e2e4", 2)?;
        Ok(session)
    }

    fn large_session() -> StorageLibraryPlayedTestResult<GameSession> {
        let session = Self::session()?;
        let mut snapshot = session.restoration(Self::now())?;
        snapshot.session.metadata = GameMetadata::new(
            GameMetadataKnownTags {
                event: Some(GameMetadataText::new("event\n\\\"".repeat(16 * 1024))?),
                white: Some(GameMetadataText::new("White ".repeat(16 * 1024))?),
                black: Some(GameMetadataText::new("Black ".repeat(16 * 1024))?),
                opening: Some(GameMetadataText::new("Opening ".repeat(8 * 1024))?),
                ..GameMetadataKnownTags::default()
            },
            (0..17)
                .map(|index| {
                    GameMetadataExtension::new(
                        format!("Retained{index}"),
                        GameMetadataText::new("extension ".repeat(256))?,
                    )
                })
                .collect::<Result<_, _>>()?,
        )?;
        snapshot.root_annotations = GameMoveNodeAnnotations::new(
            "root\n\\\"\u{1f642}".repeat(16 * 1024),
            vec![GameMoveNodeNag::new(1), GameMoveNodeNag::new(14)],
            vec![
                GameMoveNodeMarking::circle(GameEnumMarkingColor::Green, "e4".parse()?),
                GameMoveNodeMarking::arrow(
                    GameEnumMarkingColor::Red,
                    "d1".parse()?,
                    "h5".parse()?,
                )?,
            ],
        )?;
        snapshot
            .moves
            .first_mut()
            .ok_or("missing large fixture move")?
            .annotations =
            GameMoveNodeAnnotations::new("move\n\\\"".repeat(16 * 1024), Vec::new(), Vec::new())?;
        Ok(GameSession::restore(snapshot)?)
    }

    fn play(session: &mut GameSession, uci: &str, node: u128) -> StorageLibraryPlayedTestResult {
        let position = session.tree().node(session.live())?.position();
        let color = position.turn();
        let movement = position.parse_uci(uci)?;
        session.accept_move(
            GameSessionMoveRequest {
                expected_revision: session.revision(),
                seat: GameEnumSeat::Human,
                color,
                node: uuid::Uuid::from_u128(node).to_string().parse()?,
                movement,
                received_at: Self::now(),
            },
            &StorageLibraryPlayedFixtureProof,
        )?;
        Ok(())
    }

    fn now() -> ClockInstant {
        ClockInstant::from_nanos(0)
    }

    fn changed(
        session: &GameSession,
        expected: GameStorageRevision,
        timestamp: i64,
        cause: GameStorageEnumEditCause,
    ) -> StorageLibraryPlayedTestResult<GameStorageSaveRequest> {
        Ok(GameStorageSaveRequest::capture_with_change(
            session,
            Self::now(),
            expected,
            GameStorageEnumContentChange::Changed(GameStorageContentEdit::new(
                session.revision(),
                session.revision(),
                cause,
                GameLibraryOrganizationTimestamp::new(timestamp)?,
            )?),
        )?)
    }

    async fn counts(&self) -> StorageLibraryPlayedTestResult<(i64, i64)> {
        self.inspect(|connection| {
            connection.query_row(
                "SELECT (SELECT COUNT(*) FROM documents), (SELECT COUNT(*) FROM library_entries)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .await
    }

    async fn assert_saved(
        &self,
        session: &GameSession,
        revision: GameStorageRevision,
        modified: Option<i64>,
    ) -> StorageLibraryPlayedTestResult {
        let library = self.library();
        let recovered = library
            .load(session.id())
            .await?
            .ok_or("missing library record")?;
        assert_eq!(recovered.content_revision(), revision);
        assert_eq!(recovered.modified().map(|time| time.value()), modified);
        assert_eq!(
            recovered.record().identity_line_len()?,
            session.live_moves()?.len()
        );
        let played = self
            .repository()
            .load(session.id())
            .await?
            .ok_or("missing played game")?;
        assert_eq!(played.storage_revision(), revision);
        assert_eq!(played.session().revision(), session.revision());
        assert_eq!(played.session().live(), session.live());
        assert!(!played.session().clock().is_running());
        let game = session.id();
        let projected: (String, String, Option<i64>, String) = self.inspect(move |connection| {
            connection.query_row(
                "SELECT d.revision, e.content_revision, e.modified_ms, e.content_kind FROM documents d JOIN library_entries e ON e.game_id = d.id WHERE d.id = ?1",
                params![game.as_uuid().to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
        }).await?;
        assert_eq!(
            projected,
            (
                revision.value().to_string(),
                revision.value().to_string(),
                modified,
                "played".to_owned()
            )
        );
        Ok(())
    }

    async fn close(self) -> StorageLibraryPlayedTestResult {
        self.store.close().await?;
        self.executor.shutdown().await?;
        Ok(())
    }
}

struct StorageLibraryPlayedFixtureProof;

impl GameAdjudicationMateProofProvider for StorageLibraryPlayedFixtureProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

#[tokio::test]
async fn fresh_changed_save_commits_played_document_and_projection_at_supplied_time()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let session = StorageLibraryPlayedFixture::session()?;
    let request = StorageLibraryPlayedFixture::changed(
        &session,
        GameStorageRevision::INITIAL,
        1_234,
        GameStorageEnumEditCause::Created,
    )?;
    let receipt = fixture.repository().save(request).await?;
    assert_eq!(receipt.game(), session.id());
    assert_eq!(receipt.game_revision(), session.revision());
    assert_eq!(receipt.storage_revision(), GameStorageRevision::new(1));
    assert_eq!(fixture.counts().await?, (1, 1));
    fixture
        .assert_saved(&session, receipt.storage_revision(), Some(1_234))
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn unstamped_new_save_refuses_timestamp_and_leaves_no_document_or_projection()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let session = StorageLibraryPlayedFixture::session()?;
    let request = GameStorageSaveRequest::capture(
        &session,
        StorageLibraryPlayedFixture::now(),
        GameStorageRevision::INITIAL,
    )?;
    assert_eq!(
        request.content_change(),
        GameStorageEnumContentChange::Checkpoint
    );
    let result = fixture.repository().save(request).await;
    assert!(matches!(
        result,
        Err(StorageLibraryError::NewContentTimestamp)
    ));
    assert_eq!(fixture.counts().await?, (0, 0));
    assert!(fixture.repository().load(session.id()).await?.is_none());
    fixture.close().await
}

#[tokio::test]
async fn checkpoint_after_pause_preserves_the_content_edit_time() -> StorageLibraryPlayedTestResult
{
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let mut session = StorageLibraryPlayedFixture::session()?;
    let receipt = fixture
        .repository()
        .save(StorageLibraryPlayedFixture::changed(
            &session,
            GameStorageRevision::INITIAL,
            4_000,
            GameStorageEnumEditCause::Created,
        )?)
        .await?;
    session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Manual,
            received_at: StorageLibraryPlayedFixture::now(),
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &StorageLibraryPlayedFixtureProof,
    )?;
    let receipt = fixture
        .repository()
        .save(GameStorageSaveRequest::capture(
            &session,
            StorageLibraryPlayedFixture::now(),
            receipt.storage_revision(),
        )?)
        .await?;
    assert_eq!(receipt.game_revision(), session.revision());
    assert_eq!(receipt.storage_revision(), GameStorageRevision::new(2));
    fixture
        .assert_saved(&session, receipt.storage_revision(), Some(4_000))
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn accepted_move_times_use_the_monotonic_maximum_of_supplied_utc_values()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let mut session = StorageLibraryPlayedFixture::session()?;
    let mut receipt = fixture
        .repository()
        .save(StorageLibraryPlayedFixture::changed(
            &session,
            GameStorageRevision::INITIAL,
            4_000,
            GameStorageEnumEditCause::Created,
        )?)
        .await?;
    for (uci, node, supplied, expected) in [("e7e5", 3, 3_000, 4_000), ("g1f3", 4, 9_000, 9_000)] {
        StorageLibraryPlayedFixture::play(&mut session, uci, node)?;
        receipt = fixture
            .repository()
            .save(StorageLibraryPlayedFixture::changed(
                &session,
                receipt.storage_revision(),
                supplied,
                GameStorageEnumEditCause::AcceptedMove,
            )?)
            .await?;
        fixture
            .assert_saved(&session, receipt.storage_revision(), Some(expected))
            .await?;
    }
    fixture.close().await
}

#[tokio::test]
async fn expected_revision_conflict_rolls_back_document_projection_and_catalog()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let session = StorageLibraryPlayedFixture::session()?;
    let repository = fixture.repository();
    let receipt = repository
        .save(StorageLibraryPlayedFixture::changed(
            &session,
            GameStorageRevision::INITIAL,
            4_000,
            GameStorageEnumEditCause::Created,
        )?)
        .await?;
    let library = fixture.library();
    let before = library
        .query(StorageLibraryQuery::default())
        .await?
        .catalog_revision();
    let body = fixture
        .store
        .client()
        .read(session.id().as_uuid())
        .await?
        .ok_or("missing document")?
        .body()
        .to_owned();
    let mut changed = session.restoration(StorageLibraryPlayedFixture::now())?;
    changed.session.metadata = GameMetadata::new(
        crate::game::session::GameMetadataKnownTags {
            white: Some(crate::game::session::GameMetadataText::new(
                "Conflict writer".to_owned(),
            )?),
            ..crate::game::session::GameMetadataKnownTags::default()
        },
        Vec::new(),
    )?;
    let changed = GameSession::restore(changed)?;
    let result = repository
        .save(StorageLibraryPlayedFixture::changed(
            &changed,
            GameStorageRevision::INITIAL,
            99_000,
            GameStorageEnumEditCause::Metadata,
        )?)
        .await;
    assert!(
        matches!(result, Err(StorageLibraryError::ContentConflict { expected: GameStorageRevision::INITIAL, actual }) if actual == receipt.storage_revision())
    );
    fixture
        .assert_saved(&session, receipt.storage_revision(), Some(4_000))
        .await?;
    assert_eq!(
        library
            .query(StorageLibraryQuery::default())
            .await?
            .catalog_revision(),
        before
    );
    assert_eq!(
        fixture
            .store
            .client()
            .read(session.id().as_uuid())
            .await?
            .ok_or("missing document")?
            .body(),
        body
    );
    fixture.close().await
}

#[tokio::test]
async fn failed_projection_write_rolls_back_the_new_document_and_catalog()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    fixture.inspect(|connection| connection.execute_batch("CREATE TRIGGER reject_played_projection BEFORE INSERT ON library_entries BEGIN SELECT RAISE(ABORT, 'fixture projection refusal'); END;")).await?;
    let before = fixture
        .library()
        .query(StorageLibraryQuery::default())
        .await?
        .catalog_revision();
    let session = StorageLibraryPlayedFixture::session()?;
    let result = fixture
        .repository()
        .save(StorageLibraryPlayedFixture::changed(
            &session,
            GameStorageRevision::INITIAL,
            1_000,
            GameStorageEnumEditCause::Created,
        )?)
        .await;
    assert!(matches!(result, Err(StorageLibraryError::Sqlite(_))));
    assert_eq!(fixture.counts().await?, (0, 0));
    assert_eq!(
        fixture
            .library()
            .query(StorageLibraryQuery::default())
            .await?
            .catalog_revision(),
        before
    );
    fixture.close().await
}

#[test]
fn edit_stamp_is_validated_against_covered_revision_and_preserved_by_storage_rebasing()
-> StorageLibraryPlayedTestResult {
    let mut session = StorageLibraryPlayedFixture::session()?;
    let accepted = session.revision();
    let timestamp = GameLibraryOrganizationTimestamp::new(4_000)?;
    let edit = GameStorageContentEdit::new(
        accepted,
        accepted,
        GameStorageEnumEditCause::AcceptedMove,
        timestamp,
    )?;
    session.pause(
        GamePauseRequest {
            expected_revision: accepted,
            cause: GamePauseEnumCause::Manual,
            received_at: StorageLibraryPlayedFixture::now(),
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &StorageLibraryPlayedFixtureProof,
    )?;
    let change = GameStorageEnumContentChange::Changed(edit);
    let request = GameStorageSaveRequest::capture_with_change(
        &session,
        StorageLibraryPlayedFixture::now(),
        GameStorageRevision::INITIAL,
        change,
    )?
    .expect_storage_revision(GameStorageRevision::new(73));
    let rebased_clone = request
        .clone()
        .expect_storage_revision(GameStorageRevision::new(74));
    assert_eq!(request.content_change(), change);
    assert_eq!(rebased_clone.content_change(), change);
    assert_eq!(
        rebased_clone.expected_storage_revision(),
        GameStorageRevision::new(74)
    );
    assert_eq!(rebased_clone.game(), request.game());
    assert_eq!(rebased_clone.game_revision(), request.game_revision());
    assert_eq!(
        request.expected_storage_revision(),
        GameStorageRevision::new(73)
    );
    assert_eq!(request.game_revision(), session.revision());
    assert_eq!(edit.revision(), accepted);
    assert_eq!(edit.cause(), GameStorageEnumEditCause::AcceptedMove);
    assert_eq!(edit.timestamp(), timestamp);
    let future = session.revision().next()?;
    assert!(
        matches!(GameStorageContentEdit::new(future, session.revision(), GameStorageEnumEditCause::Metadata, timestamp), Err(GameStorageContentEditError::FutureRevision { accepted: actual, covered }) if actual == future && covered == session.revision())
    );
    let future_edit = GameStorageContentEdit::new(
        future,
        future,
        GameStorageEnumEditCause::Metadata,
        timestamp,
    )?;
    assert!(matches!(
        GameStorageSaveRequest::capture_with_change(
            &session,
            StorageLibraryPlayedFixture::now(),
            GameStorageRevision::INITIAL,
            GameStorageEnumContentChange::Changed(future_edit),
        ),
        Err(GameStorageSaveRequestError::ContentEdit(
            GameStorageContentEditError::FutureRevision { .. }
        ))
    ));
    Ok(())
}

#[test]
fn trusted_accepted_edit_creation_remains_subject_to_snapshot_coverage()
-> StorageLibraryPlayedTestResult {
    let session = StorageLibraryPlayedFixture::session()?;
    let timestamp = GameLibraryOrganizationTimestamp::new(6_000)?;
    let accepted = GameStorageContentEdit::from_accepted_revision(
        session.revision(),
        GameStorageEnumEditCause::AcceptedMove,
        timestamp,
    );
    let request = GameStorageSaveRequest::capture_with_change(
        &session,
        StorageLibraryPlayedFixture::now(),
        GameStorageRevision::INITIAL,
        GameStorageEnumContentChange::Changed(accepted),
    )?;
    assert_eq!(
        request.content_change(),
        GameStorageEnumContentChange::Changed(accepted)
    );
    assert_eq!(accepted.timestamp(), timestamp);
    let future = GameStorageContentEdit::from_accepted_revision(
        session.revision().next()?,
        GameStorageEnumEditCause::Metadata,
        timestamp,
    );
    assert!(matches!(
        GameStorageSaveRequest::capture_with_change(
            &session,
            StorageLibraryPlayedFixture::now(),
            GameStorageRevision::INITIAL,
            GameStorageEnumContentChange::Changed(future),
        ),
        Err(GameStorageSaveRequestError::ContentEdit(
            GameStorageContentEditError::FutureRevision { .. }
        ))
    ));
    Ok(())
}

#[tokio::test]
async fn prepared_save_transfers_one_byte_charge_and_retains_wait_until_actual_commit()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let session = StorageLibraryPlayedFixture::session()?;
    let client = fixture.store.client();
    let repository = fixture.repository();
    let body =
        StorageGameDocument::capture(&session, StorageLibraryPlayedFixture::now())?.json()?;
    let reserved = StorageLibraryPreparedMemory::reservation_bytes(body.capacity())?;
    let prepared = repository
        .prepare(StorageLibraryPlayedFixture::changed(
            &session,
            GameStorageRevision::INITIAL,
            5_000,
            GameStorageEnumEditCause::Created,
        )?)
        .await?;
    let pressure = client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - reserved)?;
    assert_eq!(fixture.counts().await?, (0, 0));
    let capacity = fixture.budget.lease(1).await?;
    let mut pending = repository.begin_save(prepared)?;
    let is_pending = {
        let mut wait = std::pin::pin!(pending.wait());
        poll_fn(|context| Poll::Ready(wait.as_mut().poll(context).is_pending())).await
    };
    assert!(is_pending);
    assert!(matches!(
        client.reserve_library_memory(1),
        Err(StorageDocumentError::Saturated)
    ));
    drop(capacity);
    let receipt = pending.wait().await?;
    assert_eq!(receipt.storage_revision(), GameStorageRevision::new(1));
    assert_eq!(receipt.game_revision(), session.revision());
    let released = client.reserve_library_memory(reserved)?;
    drop((released, pressure));
    fixture
        .assert_saved(&session, receipt.storage_revision(), Some(5_000))
        .await?;
    let all_memory = client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(all_memory);
    fixture.close().await
}

#[tokio::test]
async fn dropping_accepted_wait_preserves_the_queued_atomic_save() -> StorageLibraryPlayedTestResult
{
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let session = StorageLibraryPlayedFixture::session()?;
    let repository = fixture.repository();
    let prepared = repository
        .prepare(StorageLibraryPlayedFixture::changed(
            &session,
            GameStorageRevision::INITIAL,
            8_000,
            GameStorageEnumEditCause::Created,
        )?)
        .await?;
    let capacity = fixture.budget.lease(1).await?;
    let pending = repository.begin_save(prepared)?;
    drop(pending);
    drop(capacity);
    fixture
        .assert_saved(&session, GameStorageRevision::new(1), Some(8_000))
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn cancellation_before_executor_admission_leaves_no_save_or_byte_reservation()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let session = StorageLibraryPlayedFixture::session()?;
    let repository = fixture.repository();
    let request = StorageLibraryPlayedFixture::changed(
        &session,
        GameStorageRevision::INITIAL,
        8_000,
        GameStorageEnumEditCause::Created,
    )?;
    let capacity = fixture.budget.lease(1).await?;
    let is_pending = {
        let mut save = std::pin::pin!(repository.save(request));
        poll_fn(|context| Poll::Ready(save.as_mut().poll(context).is_pending())).await
    };
    drop(capacity);
    assert!(is_pending);
    assert!(repository.load(session.id()).await?.is_none());
    assert_eq!(fixture.counts().await?, (0, 0));
    let all_memory = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(all_memory);
    fixture.close().await
}

#[tokio::test]
async fn exhausted_byte_budget_refuses_played_preparation_before_executor_admission()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let session = StorageLibraryPlayedFixture::session()?;
    let client = fixture.store.client();
    let repository = fixture.repository();
    let request = StorageLibraryPlayedFixture::changed(
        &session,
        GameStorageRevision::INITIAL,
        8_000,
        GameStorageEnumEditCause::Created,
    )?;
    let pressure = client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    let capacity = fixture.budget.lease(1).await?;
    let observed = {
        let mut preparation = std::pin::pin!(repository.prepare(request));
        poll_fn(|context| Poll::Ready(preparation.as_mut().poll(context))).await
    };
    let refused_before_admission = matches!(
        &observed,
        Poll::Ready(Err(StorageLibraryError::Store(
            StorageDocumentError::Saturated
        )))
    );
    drop(observed);
    drop((pressure, capacity));
    fixture
        .executor
        .submit_waiting(|_cancellation| ())
        .await?
        .join()
        .await?;
    let memory_probe = client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET);
    let all_memory_released = memory_probe.is_ok();
    drop(memory_probe);
    let counts = fixture.counts().await?;
    fixture.close().await?;
    assert!(all_memory_released);
    assert_eq!(counts, (0, 0));
    assert!(
        refused_before_admission,
        "exhausted receiving bytes must refuse preparation before waiting for encoding capacity"
    );
    Ok(())
}

#[tokio::test]
async fn large_metadata_and_notes_transfer_one_reservation_until_commit()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let session = StorageLibraryPlayedFixture::large_session()?;
    let client = fixture.store.client();
    let repository = fixture.repository();
    let body =
        StorageGameDocument::capture(&session, StorageLibraryPlayedFixture::now())?.json()?;
    let reserved = StorageLibraryPreparedMemory::reservation_bytes(body.capacity())?;
    assert!(reserved < StorageDocumentStore::MEMORY_BUDGET / 2);
    drop(body);
    let prepared = repository
        .prepare(StorageLibraryPlayedFixture::changed(
            &session,
            GameStorageRevision::INITIAL,
            9_000,
            GameStorageEnumEditCause::Created,
        )?)
        .await?;
    let pressure = client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - reserved)?;
    assert!(matches!(
        client.reserve_library_memory(1),
        Err(StorageDocumentError::Saturated)
    ));
    let capacity = fixture.budget.lease(1).await?;
    let mut pending = repository.begin_save(prepared)?;
    let is_pending = {
        let mut wait = std::pin::pin!(pending.wait());
        poll_fn(|context| Poll::Ready(wait.as_mut().poll(context).is_pending())).await
    };
    assert!(is_pending);
    assert!(matches!(
        client.reserve_library_memory(1),
        Err(StorageDocumentError::Saturated)
    ));
    drop(capacity);
    let receipt = pending.wait().await?;
    let released = client.reserve_library_memory(reserved)?;
    drop((released, pressure));
    let recovered = repository
        .load(session.id())
        .await?
        .ok_or("missing large played save")?;
    assert_eq!(recovered.storage_revision(), receipt.storage_revision());
    assert_eq!(recovered.session().metadata(), session.metadata());
    for node in session.tree().nodes() {
        assert_eq!(
            recovered.session().tree().node(node.id())?.annotations(),
            node.annotations(),
        );
    }
    drop(recovered);
    drop(client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?);
    fixture.close().await
}

#[tokio::test]
async fn large_dto_reservation_refuses_before_encoding_and_releases_admission()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let session = StorageLibraryPlayedFixture::large_session()?;
    let client = fixture.store.client();
    let request = StorageLibraryPlayedFixture::changed(
        &session,
        GameStorageRevision::INITIAL,
        9_000,
        GameStorageEnumEditCause::Created,
    )?;
    let (request, retained) = fixture
        .executor
        .submit_waiting(move |cancellation| {
            StorageGameDocument::snapshot_memory_bytes(request.snapshot(), cancellation)
                .map(|retained| (request, retained))
        })
        .await?
        .join()
        .await??;
    let admission = StorageLibraryPreparedMemory::reservation_bytes(0)?;
    assert!(retained > admission);
    let pressure =
        client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - (retained - 1))?;
    let result = fixture.repository().prepare(request).await;
    assert!(matches!(
        result,
        Err(StorageLibraryError::Store(StorageDocumentError::Saturated))
    ));
    drop(client.reserve_library_memory(retained - 1)?);
    drop(pressure);
    drop(client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?);
    assert_eq!(fixture.counts().await?, (0, 0));
    fixture.close().await
}

#[tokio::test]
async fn encoding_growth_refusal_preserves_the_dto_reservation_until_drop()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let session = StorageLibraryPlayedFixture::large_session()?;
    let client = fixture.store.client();
    let request = StorageLibraryPlayedFixture::changed(
        &session,
        GameStorageRevision::INITIAL,
        9_000,
        GameStorageEnumEditCause::Created,
    )?;
    let mut memory = fixture.library().prepared_memory(0)?;
    fixture
        .executor
        .submit_waiting(move |cancellation| -> Result<(), StorageLibraryError> {
            let retained =
                StorageGameDocument::snapshot_memory_bytes(request.snapshot(), cancellation)?;
            memory.grow(retained)?;
            let document =
                StorageGameDocument::from_snapshot(request.snapshot(), Some(cancellation))?;
            let mut pressure = None;
            let mut charged = 0;
            let mut growths = 0;
            let result = document.json_with_memory(cancellation, retained, |required| {
                growths += 1;
                memory.grow(required)?;
                charged = required;
                pressure = Some(
                    client
                        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - required)?,
                );
                Ok(())
            });
            assert_eq!(growths, 2);
            assert!(matches!(
                result,
                Err(StorageGameDocumentError::Store(
                    StorageDocumentError::Saturated
                ))
            ));
            assert!(charged > retained);
            assert!(matches!(
                client.reserve_library_memory(1),
                Err(StorageDocumentError::Saturated)
            ));
            drop(pressure);
            let available =
                client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - charged)?;
            assert!(matches!(
                client.reserve_library_memory(1),
                Err(StorageDocumentError::Saturated)
            ));
            drop(available);
            drop(document);
            drop(memory);
            drop(client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?);
            Ok(())
        })
        .await?
        .join()
        .await??;
    assert_eq!(fixture.counts().await?, (0, 0));
    fixture.close().await
}

#[tokio::test]
async fn cancelled_and_abandoned_encoding_keeps_bytes_until_worker_cleanup()
-> StorageLibraryPlayedTestResult {
    for abandon in [false, true] {
        let fixture = StorageLibraryPlayedFixture::new().await?;
        let session = StorageLibraryPlayedFixture::large_session()?;
        let client = fixture.store.client();
        let request = StorageLibraryPlayedFixture::changed(
            &session,
            GameStorageRevision::INITIAL,
            9_000,
            GameStorageEnumEditCause::Created,
        )?;
        let mut memory = fixture.library().prepared_memory(0)?;
        let (started, observing) = tokio::sync::oneshot::channel();
        let started = std::sync::Mutex::new(Some(started));
        let (release, blocked) = std::sync::mpsc::channel();
        let job = fixture
            .executor
            .submit_waiting(move |cancellation| -> Result<(), StorageLibraryError> {
                let retained =
                    StorageGameDocument::snapshot_memory_bytes(request.snapshot(), cancellation)?;
                memory.grow(retained)?;
                let document =
                    StorageGameDocument::from_snapshot(request.snapshot(), Some(cancellation))?;
                let mut started = started
                    .into_inner()
                    .map_err(|_error| StorageDocumentError::Closed)?;
                let mut growths = 0;
                let body = document.json_with_memory(cancellation, retained, |required| {
                    memory.grow(required)?;
                    growths += 1;
                    if growths == 2 {
                        let sender = started.take().ok_or(StorageDocumentError::Closed)?;
                        sender
                            .send(required)
                            .map_err(|_error| StorageDocumentError::Closed)?;
                        blocked
                            .recv()
                            .map_err(|_error| StorageDocumentError::Closed)?;
                    }
                    Ok(())
                })?;
                drop((body, document));
                Ok(())
            })
            .await?;
        let charged = observing.await?;
        let pressure =
            client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET - charged)?;
        job.cancel();
        let retained_job = if abandon {
            drop(job);
            None
        } else {
            Some(job)
        };
        assert!(matches!(
            client.reserve_library_memory(1),
            Err(StorageDocumentError::Saturated)
        ));
        release.send(())?;
        if let Some(job) = retained_job {
            assert!(matches!(
                job.join().await?,
                Err(StorageLibraryError::Codec(
                    StorageGameDocumentError::Cancelled
                ))
            ));
        } else {
            fixture
                .executor
                .submit_waiting(|_cancellation| ())
                .await?
                .join()
                .await?;
        }
        drop(client.reserve_library_memory(charged)?);
        drop(pressure);
        drop(client.reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?);
        assert_eq!(fixture.counts().await?, (0, 0));
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn played_load_refuses_clockless_notation_and_leaves_it_unchanged()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let id: GameId = "14d36b00-5cf2-42ed-a730-9fa472782852".parse()?;
    let mut next = 100_u128;
    let mut parser = NotationPgnImport::new(Cursor::new(b"1. e4 e5 *"), move || {
        next += 1;
        uuid::Uuid::from_u128(next).to_string().parse()
    });
    let game = parser
        .next_game(&|| false)?
        .ok_or("missing notation fixture")?;
    let body = StorageGameDocument::capture_notation(id, &game)?.json()?;
    let library = fixture.library();
    let prepared = library
        .prepare_write(
            id,
            GameStorageRevision::INITIAL,
            body.clone(),
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(2_000)?),
        )
        .await?;
    let receipt = library.begin_save(prepared)?.wait().await?;
    assert!(matches!(
        receipt,
        StorageLibraryEnumMutationReceipt::Saved(_)
    ));
    assert!(matches!(
        fixture.repository().load(id).await,
        Err(StorageLibraryError::Codec(
            StorageGameDocumentError::ContentKind {
                expected: StorageGameDocumentEnumContentKind::Played,
                found: StorageGameDocumentEnumContentKind::Notation,
            }
        ))
    ));
    let stored = fixture
        .store
        .client()
        .read(id.as_uuid())
        .await?
        .ok_or("missing notation document")?;
    assert_eq!(stored.body(), body);
    assert_eq!(stored.revision(), 1);
    drop(stored);
    fixture.close().await
}

#[tokio::test]
async fn legacy_save_remains_unindexed_and_adopted_unknown_time_survives_checkpoint()
-> StorageLibraryPlayedTestResult {
    let fixture = StorageLibraryPlayedFixture::new().await?;
    let session = StorageLibraryPlayedFixture::session()?;
    let request = GameStorageSaveRequest::capture(
        &session,
        StorageLibraryPlayedFixture::now(),
        GameStorageRevision::INITIAL,
    )?;
    let legacy = StorageGameDocumentRepository::new(fixture.store.client(), &fixture.executor);
    let receipt = legacy.save(request).await?;
    assert_eq!(fixture.counts().await?, (1, 0));
    let library = fixture.library();
    let page = library
        .adoption_page(StorageLibraryAdoptionCursor::start())
        .await?;
    let candidate = *page
        .candidates()
        .iter()
        .find(|candidate| candidate.id() == session.id().as_uuid())
        .ok_or("missing played adoption candidate")?;
    drop(page);
    let prepared = library.prepare_adoption(candidate).await?;
    let StorageLibraryEnumAdoptionPreparation::Prepared(prepared) = prepared else {
        return Err("legacy played adoption was not prepared".into());
    };
    library.begin_adopt(prepared)?.wait().await?;
    fixture
        .assert_saved(&session, receipt.storage_revision(), None)
        .await?;
    let saved = fixture
        .repository()
        .save(GameStorageSaveRequest::capture(
            &session,
            StorageLibraryPlayedFixture::now(),
            receipt.storage_revision(),
        )?)
        .await?;
    assert_eq!(saved.storage_revision(), GameStorageRevision::new(2));
    fixture
        .assert_saved(&session, saved.storage_revision(), None)
        .await?;
    fixture.close().await
}
