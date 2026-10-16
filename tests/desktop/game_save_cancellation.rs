use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::board::start::BoardStart;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::state::ClockState;
use crate::clock::time_control::ClockTimeControl;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::game_supervisor::{
    DesktopGameSupervisor, DesktopGameSupervisorEngineFactory, DesktopGameSupervisorOptions,
    DesktopGameSupervisorWrite,
};
use crate::engine::live_game::EngineLiveGame;
use crate::engine::process_provider::EngineNativeProcess;
use crate::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use crate::game::engine_provider::{GameEngineContext, GameEngineSearchConfig};
use crate::game::enums::{GameEnumEngineReadiness, GameEnumMode, GameEnumSeat};
use crate::game::session::{
    GameMetadata, GameSeats, GameSession, GameSessionMoveRequest, GameSessionOptions,
};
use crate::game::storage_repository::{
    GameStorageRepository, GameStorageRevision, GameStorageSaveRequest,
};
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::game_document::{StorageGameDocumentError, StorageGameDocumentRepository};

struct DesktopGameSaveCancellationClock;

impl ClockTimeProvider for DesktopGameSaveCancellationClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        ClockInstant::from_millis(2)
    }
}

struct DesktopGameSaveCancellationProof;

impl GameAdjudicationMateProofProvider for DesktopGameSaveCancellationProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

struct DesktopGameSaveCancellationFactory;

impl DesktopGameSupervisorEngineFactory for DesktopGameSaveCancellationFactory {
    type Engine = EngineLiveGame<EngineNativeProcess, DesktopGameSaveCancellationClock>;
    type Error = std::io::Error;

    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        Err(std::io::Error::other(
            "A local checkpoint test requested an engine",
        ))
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn cancelled_intermediate_checkpoint_is_retried_before_the_final_move_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
    let proof = DesktopGameSaveCancellationProof;
    let mut session = GameSession::new(GameSessionOptions {
        id: "fba24da7-387d-41fe-bf4b-c43346447b49".parse()?,
        root: "d866426f-c560-47cf-8b98-c23a971b4ba7".parse()?,
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
        ClockInstant::from_nanos(0),
        GameEnumEngineReadiness::NotRequired,
        &proof,
    )?;
    let mut intermediate = None;
    for (node, uci, millis) in [
        ("ae7ed82e-f63f-45d5-8da9-374e541028c1", "e2e4", 1),
        ("cd947cb4-a2ca-448e-a71b-b2fecf441b4f", "e7e5", 2),
    ] {
        let position = session.tree().node(session.live())?.position();
        session.accept_move(
            GameSessionMoveRequest {
                expected_revision: session.revision(),
                seat: GameEnumSeat::Human,
                color: position.turn(),
                node: node.parse()?,
                movement: position.parse_uci(uci)?,
                received_at: ClockInstant::from_millis(millis)?,
            },
            &proof,
        )?;
        if intermediate.is_none() {
            intermediate = Some(GameStorageSaveRequest::capture(
                &session,
                ClockInstant::from_millis(millis)?,
                GameStorageRevision::INITIAL,
            )?);
        }
    }
    let game = session.id();
    let final_revision = session.revision();
    let intermediate = intermediate.ok_or("missing intermediate checkpoint")?;
    let intermediate_revision = intermediate.game_revision();
    let (mut supervisor, port) = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
        session,
        storage_revision: GameStorageRevision::INITIAL,
        storage: store.client(),
        executor: Arc::clone(&executor),
        clock: Arc::new(DesktopGameSaveCancellationClock),
        proof: Arc::new(proof),
        factory: DesktopGameSaveCancellationFactory,
        search_config: GameEngineSearchConfig::initial_default(),
    })
    .await?;
    supervisor.writes.push_front(DesktopGameSupervisorWrite {
        request: intermediate,
        response: None,
        quit: false,
    });
    supervisor.begin_save(false);
    drop(supervisor.saving.take());
    supervisor.finish_save(Err(StorageGameDocumentError::Cancelled))?;
    assert_eq!(
        supervisor
            .writes
            .front()
            .ok_or("cancelled checkpoint disappeared")?
            .request
            .game_revision(),
        intermediate_revision
    );
    supervisor
        .observe_work(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(supervisor.writes.is_empty());
    assert_eq!(supervisor.storage_revision.value(), 2);
    let repository = StorageGameDocumentRepository::new(store.client(), &executor);
    let saved = repository
        .load(game)
        .await?
        .ok_or("missing final checkpoint")?;
    assert_eq!(saved.session().revision(), final_revision);
    assert_eq!(saved.session().tree().len(), 3);
    supervisor
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    drop((supervisor, port, repository));
    store.close().await?;
    Arc::try_unwrap(executor)
        .map_err(|_shared| "executor still owned")?
        .shutdown()
        .await?;
    directory.close()?;
    Ok(())
}
