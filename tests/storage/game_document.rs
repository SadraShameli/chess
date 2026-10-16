use std::num::NonZeroUsize;

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use chess::game::enums::{
    GameEnumEngineReadiness, GameEnumMarkingColor, GameEnumMode, GameEnumPhase, GameEnumSeat,
    GameMoveId,
};
use chess::game::move_node::{GameMoveNodeAnnotations, GameMoveNodeMarking, GameMoveNodeNag};
use chess::game::pause::GamePauseEnumCause;
use chess::game::session::{
    GameMetadata, GameMetadataDate, GameMetadataExtension, GameMetadataKnownTags,
    GameMetadataOpeningCode, GameMetadataText, GameSeats, GameSession, GameSessionEnumInterruption,
    GameSessionMoveRequest, GameSessionOptions, GameSessionRestoredMove,
};
use chess::game::storage_repository::{
    GameStorageRepository, GameStorageRevision, GameStorageSaveRequest,
};
use chess::game::takeback::GameTakebackEnumConsent;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::game_document::{
    StorageGameDocument, StorageGameDocumentError, StorageGameDocumentRepository,
};

type StorageGameDocumentTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct StorageGameDocumentFixtureProof;

impl GameAdjudicationMateProofProvider for StorageGameDocumentFixtureProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

struct StorageGameDocumentFixture;

impl StorageGameDocumentFixture {
    fn node(value: u128) -> StorageGameDocumentTestResult<GameMoveId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn session() -> StorageGameDocumentTestResult<GameSession> {
        let control = ClockTimeControl::bronstein_delay(
            ClockDuration::from_millis(60_000)?,
            ClockDuration::from_millis(2_000)?,
        )?;
        let known = GameMetadataKnownTags {
            white: Some(GameMetadataText::new("White player".to_owned())?),
            black: Some(GameMetadataText::new("Black player".to_owned())?),
            date: Some(GameMetadataDate::new(Some(2026), Some(10), Some(16))?),
            eco: Some(GameMetadataOpeningCode::new("C20".to_owned())?),
            ..GameMetadataKnownTags::default()
        };
        let mut session = GameSession::new(GameSessionOptions {
            id: "c1b5029c-f686-4f18-b45a-1af832163c24".parse()?,
            root: Self::node(1)?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::HumanVersusEngine,
                GameEnumSeat::Human,
                GameEnumSeat::Reckless,
            )?,
            clock: ClockState::new(control, BoardEnumColor::White),
            metadata: GameMetadata::new(
                known,
                vec![GameMetadataExtension::new(
                    "Study".to_owned(),
                    GameMetadataText::new("private offline study".to_owned())?,
                )?],
            )?,
        })?;
        session.start(
            session.revision(),
            ClockInstant::from_nanos(0),
            GameEnumEngineReadiness::Ready,
            &StorageGameDocumentFixtureProof,
        )?;
        let movement = session
            .tree()
            .node(session.live())?
            .position()
            .parse_uci("e2e4")?;
        session.accept_move(
            GameSessionMoveRequest {
                expected_revision: session.revision(),
                seat: GameEnumSeat::Human,
                color: BoardEnumColor::White,
                node: Self::node(2)?,
                movement,
                received_at: ClockInstant::from_millis(500)?,
            },
            &StorageGameDocumentFixtureProof,
        )?;
        Ok(session)
    }

    fn annotated() -> StorageGameDocumentTestResult<GameSession> {
        let session = Self::session()?;
        let mut snapshot = session.restoration(ClockInstant::from_millis(2_000)?)?;
        snapshot.root_annotations = GameMoveNodeAnnotations::new(
            "Root notes".to_owned(),
            vec![GameMoveNodeNag::new(10)],
            vec![GameMoveNodeMarking::circle(
                GameEnumMarkingColor::Blue,
                "d4".parse()?,
            )],
        )?;
        snapshot
            .moves
            .first_mut()
            .ok_or("missing accepted move")?
            .annotations = GameMoveNodeAnnotations::new(
            "Center control".to_owned(),
            vec![GameMoveNodeNag::new(1)],
            vec![GameMoveNodeMarking::arrow(
                GameEnumMarkingColor::Green,
                "e2".parse()?,
                "e4".parse()?,
            )?],
        )?;
        snapshot.moves.push(GameSessionRestoredMove {
            id: Self::node(3)?,
            parent: snapshot.session.root,
            movement: snapshot.session.start.position().parse_uci("d2d4")?,
            annotations: GameMoveNodeAnnotations::default(),
            before_clock: None,
            clock: None,
        });
        snapshot.view = snapshot.session.root;
        snapshot.warnings_used = [true, false];
        snapshot.takeback_consent = GameTakebackEnumConsent::NotRequired;
        snapshot.premoves_enabled = true;
        Ok(GameSession::restore(snapshot)?)
    }
}

#[test]
fn future_schema_is_rejected_before_parsing_the_game_payload() -> StorageGameDocumentTestResult {
    let json = format!(
        "{{\"schema_version\":{},\"game\":{{\"moves\":42,\"clock\":\"unsupported\"}}}}",
        StorageGameDocument::SCHEMA_VERSION + 1
    );
    assert!(matches!(
        StorageGameDocument::restore(&json),
        Err(StorageGameDocumentError::FutureSchema { .. })
    ));
    Ok(())
}

#[test]
fn running_recovery_is_paused_without_charging_downtime() -> StorageGameDocumentTestResult {
    let session = StorageGameDocumentFixture::session()?;
    let original = session
        .clock()
        .snapshot(ClockInstant::from_millis(2_000)?)?;
    let document = StorageGameDocument::capture(&session, ClockInstant::from_millis(2_000)?)?;
    let recovered = StorageGameDocument::restore(&document.json()?)?;
    assert_eq!(recovered.id(), session.id());
    assert_eq!(recovered.revision(), session.revision());
    assert_eq!(recovered.live(), session.live());
    assert_eq!(recovered.phase(), GameEnumPhase::Interrupted);
    assert_eq!(
        recovered.interruption(),
        Some(GameSessionEnumInterruption::Paused(
            GamePauseEnumCause::Relaunch
        ))
    );
    assert!(!recovered.clock().is_running());
    let later = recovered
        .clock()
        .snapshot(ClockInstant::from_millis(999_000)?)?;
    assert_eq!(later.elapsed(), original.elapsed());
    assert_eq!(later.active(), original.active());
    assert_eq!(later.persisted().sides, original.persisted().sides);
    assert_eq!(later.control(), original.control());
    Ok(())
}

#[test]
fn complete_snapshot_preserves_ordered_variations_annotations_metadata_and_warning_flags()
-> StorageGameDocumentTestResult {
    let session = StorageGameDocumentFixture::annotated()?;
    let root = session.tree().root();
    let document = StorageGameDocument::capture(&session, ClockInstant::from_millis(2_000)?)?;
    let recovered = StorageGameDocument::restore(&document.json()?)?;
    assert_eq!(recovered.tree().len(), 3);
    assert_eq!(recovered.tree().root(), root);
    assert_eq!(recovered.live(), session.live());
    assert_eq!(recovered.view(), root);
    assert_eq!(recovered.start_position(), session.start_position());
    assert_eq!(recovered.seats(), session.seats());
    assert_eq!(recovered.metadata(), session.metadata());
    assert_eq!(
        recovered.tree().node(root)?.children(),
        session.tree().node(root)?.children()
    );
    for id in recovered.tree().path_to(recovered.live())? {
        let actual = recovered.tree().node(id)?;
        let expected = session.tree().node(id)?;
        assert_eq!(actual.annotations(), expected.annotations());
        assert_eq!(actual.before_clock(), expected.before_clock());
        assert_eq!(actual.clock(), expected.clock());
        assert_eq!(actual.position(), expected.position());
    }
    assert!(recovered.clock().warning_used(BoardEnumColor::White));
    assert!(!recovered.clock().warning_used(BoardEnumColor::Black));
    assert_eq!(
        recovered.takeback().consent(),
        GameTakebackEnumConsent::NotRequired
    );
    assert_eq!(recovered.takeback().pending(), None);
    assert!(recovered.premove().is_enabled());
    assert_eq!(recovered.premove().pending(), None);
    Ok(())
}

#[test]
fn corrupted_moves_and_noncanonical_identity_never_become_a_recovered_game()
-> StorageGameDocumentTestResult {
    let session = StorageGameDocumentFixture::session()?;
    let json = StorageGameDocument::capture(&session, ClockInstant::from_millis(2_000)?)?.json()?;
    assert!(json.contains("e2e4"));
    assert!(StorageGameDocument::restore(&json.replace("e2e4", "e2e5")).is_err());
    assert!(
        StorageGameDocument::restore(
            &json.replace(&session.id().as_uuid().to_string(), "INVALID_GAME_ID")
        )
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn autosave_commits_one_complete_revision_and_conflict_preserves_it()
-> StorageGameDocumentTestResult {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("invalid fixture capacity")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
    let session = StorageGameDocumentFixture::annotated()?;
    let receipt = {
        let repository = StorageGameDocumentRepository::new(store.client(), &executor);
        let request = GameStorageSaveRequest::capture(
            &session,
            ClockInstant::from_millis(2_000)?,
            GameStorageRevision::INITIAL,
        )?;
        let receipt = repository.save(request).await?;
        assert_eq!(receipt.game(), session.id());
        assert_eq!(receipt.game_revision(), session.revision());
        assert_eq!(receipt.storage_revision().value(), 1);
        let stale = GameStorageSaveRequest::capture(
            &session,
            ClockInstant::from_millis(2_000)?,
            GameStorageRevision::INITIAL,
        )?;
        assert!(repository.save(stale).await.is_err());
        let saved = repository
            .load(session.id())
            .await?
            .ok_or("missing game document")?;
        assert_eq!(saved.storage_revision(), receipt.storage_revision());
        assert_eq!(saved.session().revision(), session.revision());
        assert_eq!(saved.session().live(), session.live());
        assert_eq!(saved.session().tree().len(), session.tree().len());
        receipt
    };
    store.close().await?;
    executor.shutdown().await?;
    let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("invalid fixture capacity")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
    {
        let repository = StorageGameDocumentRepository::new(store.client(), &executor);
        let recovered = repository
            .load(session.id())
            .await?
            .ok_or("missing durable game")?;
        assert_eq!(recovered.storage_revision(), receipt.storage_revision());
        assert_eq!(recovered.session().metadata(), session.metadata());
        assert_eq!(recovered.session().phase(), GameEnumPhase::Interrupted);
        assert!(!recovered.session().clock().is_running());
    }
    store.close().await?;
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn a_document_under_the_wrong_database_identity_is_rejected() -> StorageGameDocumentTestResult
{
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("invalid fixture capacity")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
    let session = StorageGameDocumentFixture::session()?;
    let wrong: chess::game::enums::GameId = "0f6c64aa-4b6a-479f-92bb-7cbb29a73633".parse()?;
    let json = StorageGameDocument::capture(&session, ClockInstant::from_millis(2_000)?)?.json()?;
    store.client().save(wrong.as_uuid(), 0, json).await?;
    {
        let repository = StorageGameDocumentRepository::new(store.client(), &executor);
        assert!(repository.load(wrong).await.is_err());
        assert!(repository.load(session.id()).await?.is_none());
    }
    store.close().await?;
    executor.shutdown().await?;
    Ok(())
}
