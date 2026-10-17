use crate::storage_sqlite_inspector::{StorageSqliteInspector, StorageSqliteInspectorError};
use std::future::{Future, pending, poll_fn};
use std::num::NonZeroUsize;
use std::panic::UnwindSafe;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering},
};
use std::task::Poll;
use std::time::{Duration, Instant as StandardInstant};

use tokio::time::Instant;

use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::board::start::BoardStart;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::clock::state::{ClockState, ClockStateEnumStatus};
use crate::clock::time_control::ClockTimeControl;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use crate::desktop::game_supervisor::{
    DesktopGameSupervisor, DesktopGameSupervisorEngineFactory, DesktopGameSupervisorEngineProvider,
    DesktopGameSupervisorEnumComputation, DesktopGameSupervisorEnumPersistence,
    DesktopGameSupervisorError, DesktopGameSupervisorOptions, DesktopGameSupervisorPort,
};
use crate::desktop::game_transition::DesktopGameTransitionOptions;
use crate::desktop::identity::DesktopRequestId;
use crate::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use crate::game::engine_provider::{
    GameEngineContext, GameEngineEnumEvent, GameEngineMove, GameEngineProvider,
    GameEngineQuiescenceReceipt, GameEngineRequest, GameEngineSearchConfig,
    GameEngineSearchIdentity, GameEngineStopReceipt,
};
use crate::game::enums::{GameEnumEngineReadiness, GameEnumMode, GameEnumPhase, GameEnumSeat};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::pause::{GamePauseEnumCause, GamePauseEnumEngineBarrier};
use crate::game::premove::GamePremoveIntent;
use crate::game::save::GameSaveEnumStatus;
use crate::game::session::{
    GameMetadata, GameSeats, GameSession, GameSessionEnumInterruption, GameSessionOptions,
};
use crate::game::storage_repository::{
    GameStorageEnumContentChange, GameStorageEnumEditCause, GameStorageRepository,
    GameStorageRevision, GameStorageWallTimeError, GameStorageWallTimeProvider,
};
use crate::game::takeback::{GameTakebackEnumConsent, GameTakebackEnumResponse};
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::library_played::StorageLibraryPlayedRepository;
use crate::storage::library_repository::StorageLibraryError;

type DesktopGameContentTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type DesktopGameContentActor = DesktopGameSupervisor<
    DesktopGameContentFactory,
    DesktopGameContentClock,
    DesktopGameContentProof,
>;

struct DesktopGameContentWall {
    millis: AtomicI64,
    failing: AtomicBool,
    reads: AtomicUsize,
}

impl DesktopGameContentWall {
    fn new() -> Self {
        Self {
            millis: AtomicI64::new(1_000),
            failing: AtomicBool::new(false),
            reads: AtomicUsize::new(0),
        }
    }

    fn set(&self, millis: i64, failing: bool) {
        self.millis.store(millis, Ordering::SeqCst);
        self.failing.store(failing, Ordering::SeqCst);
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl GameStorageWallTimeProvider for DesktopGameContentWall {
    fn now(&self) -> Result<GameLibraryOrganizationTimestamp, GameStorageWallTimeError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.failing.load(Ordering::SeqCst) {
            Err(GameStorageWallTimeError::Unavailable)
        } else {
            Ok(GameLibraryOrganizationTimestamp::new(
                self.millis.load(Ordering::SeqCst),
            )?)
        }
    }
}

#[derive(Default)]
struct DesktopGameContentClock(AtomicU64);

impl DesktopGameContentClock {
    fn set(&self, millis: u64) {
        self.0.store(millis, Ordering::SeqCst);
    }
}

impl ClockTimeProvider for DesktopGameContentClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        ClockInstant::from_millis(self.0.load(Ordering::SeqCst))
    }
}

#[derive(Default)]
struct DesktopGameContentProof(AtomicUsize);

impl GameAdjudicationMateProofProvider for DesktopGameContentProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        self.0.fetch_add(1, Ordering::SeqCst);
        GameAdjudicationEnumMateProof::Possible
    }
}

struct DesktopGameContentEngine(Arc<AtomicBool>);

impl GameEngineProvider for DesktopGameContentEngine {
    type Error = std::io::Error;

    fn is_idle(&self) -> bool {
        true
    }

    async fn start(
        &mut self,
        _request: GameEngineRequest,
    ) -> Result<GameEngineSearchIdentity, Self::Error> {
        Err(std::io::Error::other(
            "Unexpected search in controlled content fixture",
        ))
    }

    async fn next_event(
        &mut self,
        _expected: GameEngineContext,
    ) -> Result<GameEngineEnumEvent, Self::Error> {
        pending().await
    }

    async fn stop(
        &mut self,
        _identity: GameEngineSearchIdentity,
        _received_at: StandardInstant,
    ) -> Result<GameEngineStopReceipt, Self::Error> {
        Err(std::io::Error::other(
            "Unexpected search stop in controlled content fixture",
        ))
    }

    async fn quiesce(
        &mut self,
        expected: GameEngineContext,
        _received_at: StandardInstant,
    ) -> Result<GameEngineQuiescenceReceipt, Self::Error> {
        Ok(GameEngineQuiescenceReceipt::new(
            expected,
            uuid::Uuid::from_u128(30),
            None,
        ))
    }
}

impl DesktopGameSupervisorEngineProvider for DesktopGameContentEngine {
    fn is_shutdown_complete(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    async fn refresh_resources(&mut self) -> Result<(), Self::Error> {
        pending().await
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        self.0.store(true, Ordering::SeqCst);
        Ok(())
    }
}

struct DesktopGameContentFactory;

impl DesktopGameSupervisorEngineFactory for DesktopGameContentFactory {
    type Engine = DesktopGameContentEngine;
    type Error = std::io::Error;

    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        Err(std::io::Error::other(
            "Unexpected factory request in controlled content fixture",
        ))
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct DesktopGameContentFixture {
    directory: tempfile::TempDir,
    store: StorageDocumentStore,
    executor: Arc<CapacityExecutor>,
    clock: Arc<DesktopGameContentClock>,
    wall: Arc<DesktopGameContentWall>,
    proof: Arc<DesktopGameContentProof>,
    actor: DesktopGameContentActor,
    port: DesktopGameSupervisorPort,
}

impl DesktopGameContentFixture {
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(5)
    }

    async fn new(timed: bool) -> DesktopGameContentTestResult<Self> {
        Self::with_session(Self::session(timed)?, 0).await
    }

    async fn with_session(
        session: GameSession,
        initial_millis: u64,
    ) -> DesktopGameContentTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store =
            StorageDocumentStore::open(directory.path().join("library.sqlite"), budget).await?;
        let clock = Arc::new(DesktopGameContentClock::default());
        clock.set(initial_millis);
        let wall = Arc::new(DesktopGameContentWall::new());
        let proof = Arc::new(DesktopGameContentProof::default());
        let (actor, port) = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
            session,
            storage_revision: GameStorageRevision::INITIAL,
            storage: store.client(),
            executor: Arc::clone(&executor),
            clock: Arc::clone(&clock),
            wall_time: Arc::<DesktopGameContentWall>::clone(&wall),
            proof: Arc::clone(&proof),
            factory: DesktopGameContentFactory,
            search_config: GameEngineSearchConfig::initial_default(),
        })
        .await?;
        Ok(Self {
            directory,
            store,
            executor,
            clock,
            wall,
            proof,
            actor,
            port,
        })
    }

    fn session(timed: bool) -> DesktopGameContentTestResult<GameSession> {
        Ok(GameSession::new(GameSessionOptions {
            id: uuid::Uuid::from_u128(0x30).to_string().parse()?,
            root: uuid::Uuid::from_u128(1).to_string().parse()?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            clock: ClockState::new(
                if timed {
                    ClockTimeControl::sudden_death(ClockDuration::from_millis(100)?)?
                } else {
                    ClockTimeControl::untimed()
                },
                BoardEnumColor::White,
            ),
            metadata: GameMetadata::default(),
        })?)
    }

    async fn compute(
        &mut self,
        operation: DesktopGameSupervisorEnumComputation,
    ) -> Result<(), DesktopGameSupervisorError> {
        self.actor.schedule(operation, None);
        self.actor.observe_computation(Self::deadline()).await
    }

    async fn action(
        &mut self,
        action: DesktopGameCommandsEnumAction,
    ) -> Result<(), DesktopGameSupervisorError> {
        let request = DesktopGameCommandsRequest {
            request_id: DesktopRequestId::new(),
            expected_revision: self.port.snapshot().snapshot.revision(),
            received_at: self.clock.now()?,
            action,
        };
        self.compute(DesktopGameSupervisorEnumComputation::Action(
            DesktopGameTransitionOptions {
                request,
                engine: GamePauseEnumEngineBarrier::NoEngine,
                readiness: GameEnumEngineReadiness::NotRequired,
            },
        ))
        .await
    }

    async fn flush(&mut self) -> DesktopGameContentTestResult {
        while self.actor.saving.is_some() || !self.actor.writes.is_empty() {
            self.actor.begin_save(true);
            let result = DesktopGameContentActor::save_ready(&mut self.actor.saving).await;
            self.actor.saving = None;
            self.actor.finish_save(result)?;
            if self.actor.save_failure.is_some() {
                break;
            }
        }
        Ok(())
    }

    async fn start(&mut self) -> DesktopGameContentTestResult {
        self.flush().await?;
        self.action(DesktopGameCommandsEnumAction::Start).await?;
        self.flush().await
    }

    async fn play(
        &mut self,
        origin: &str,
        destination: &str,
        node: u128,
    ) -> DesktopGameContentTestResult {
        let state = self.actor.session.take()?;
        let color = state
            .session
            .tree()
            .node(state.session.live())?
            .position()
            .turn();
        self.actor.session.replace(state)?;
        self.action(DesktopGameCommandsEnumAction::Play {
            node: uuid::Uuid::from_u128(node).to_string().parse()?,
            origin: origin.parse()?,
            destination: destination.parse()?,
            promotion: None,
            color,
        })
        .await?;
        Ok(())
    }

    async fn inspect<T, F>(&self, operation: F) -> DesktopGameContentTestResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut StorageSqliteInspector) -> Result<T, StorageSqliteInspectorError>
            + Send
            + UnwindSafe
            + 'static,
    {
        let path = self.directory.path().join("library.sqlite");
        Ok(self
            .executor
            .submit_waiting(move |_cancellation| operation(&mut StorageSqliteInspector::open(path)?))
            .await?
            .join()
            .await??)
    }

    async fn durable(&self) -> DesktopGameContentTestResult<(String, String, Option<i64>)> {
        let game = self.port.snapshot().snapshot.id().as_uuid().to_string();
        self.inspect(move |connection| {
            Ok((
                connection.text(&format!(
                    "SELECT revision AS value FROM documents WHERE id = '{game}'"
                ))?,
                connection.text(&format!(
                    "SELECT content_revision AS value FROM library_entries WHERE game_id = '{game}'"
                ))?,
                connection.optional_integer(&format!(
                    "SELECT modified_ms AS value FROM library_entries WHERE game_id = '{game}'"
                ))?,
            ))
        })
        .await
    }

    async fn trigger(&self, enabled: bool) -> DesktopGameContentTestResult {
        self.inspect(move |connection| connection.batch(if enabled {
            "CREATE TRIGGER content_save_failure BEFORE UPDATE ON library_entries BEGIN SELECT RAISE(ABORT, 'controlled projection failure'); END;"
        } else { "DROP TRIGGER content_save_failure;" })).await
    }

    async fn close(mut self) -> DesktopGameContentTestResult {
        self.actor.shutdown_retained(Self::deadline()).await?;
        drop((self.actor, self.port));
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|_shared| "executor remains owned")?
            .shutdown()
            .await?;
        self.directory.close()?;
        Ok(())
    }

    async fn defer(&mut self) -> DesktopGameContentTestResult {
        self.start().await?;
        self.wall.set(2_000, true);
        self.clock.set(100);
        self.compute(DesktopGameSupervisorEnumComputation::Clock)
            .await?;
        let event = self.port.snapshot();
        assert_eq!(event.snapshot.phase(), GameEnumPhase::Interrupted);
        assert!(event.snapshot.is_save_required());
        let state = self.actor.session.take()?;
        assert_eq!(
            state.session.interruption(),
            Some(GameSessionEnumInterruption::PendingResult)
        );
        assert_eq!(state.session.outcome(), None);
        assert!(state.pending_result.is_some());
        assert_eq!(
            state.session.clock().view(self.clock.now()?)?.status(),
            ClockStateEnumStatus::Flagged(BoardEnumColor::White)
        );
        self.actor.session.replace(state)?;
        Ok(())
    }
}

#[tokio::test]
async fn fresh_owner_saves_matching_typed_document_projection_and_created_utc()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(false).await?;
    let request = &fixture
        .actor
        .writes
        .front()
        .ok_or("initial save absent")?
        .request;
    let GameStorageEnumContentChange::Changed(edit) = request.content_change() else {
        return Err("initial creation stamp absent".into());
    };
    assert_eq!(edit.cause(), GameStorageEnumEditCause::Created);
    assert_eq!(edit.timestamp().value(), 1_000);
    fixture.wall.set(99_000, true);
    fixture.flush().await?;
    assert_eq!(
        fixture.durable().await?,
        ("1".to_owned(), "1".to_owned(), Some(1_000))
    );
    let recovered = StorageLibraryPlayedRepository::new(fixture.store.client(), &fixture.executor)
        .load(fixture.port.snapshot().snapshot.id())
        .await?
        .ok_or("typed played record absent")?;
    assert_eq!(
        recovered.session().revision(),
        fixture.port.snapshot().snapshot.revision()
    );
    assert_eq!(fixture.wall.reads(), 1);
    fixture.close().await
}

#[tokio::test]
async fn accepted_move_time_survives_pause_sqlite_failure_and_newest_retry()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(false).await?;
    fixture.start().await?;
    fixture.wall.set(2_000, false);
    fixture.clock.set(1);
    fixture.play("e2", "e4", 2).await?;
    fixture.wall.set(50_000, true);
    fixture
        .action(DesktopGameCommandsEnumAction::Pause {
            cause: GamePauseEnumCause::Manual,
        })
        .await?;
    fixture.trigger(true).await?;
    fixture.flush().await?;
    assert!(fixture.actor.save_failure.is_some());
    assert_eq!(fixture.durable().await?.2, Some(1_000));
    fixture
        .compute(DesktopGameSupervisorEnumComputation::SaveFailure {
            engine: GamePauseEnumEngineBarrier::NoEngine,
            received_at: fixture.clock.now()?,
        })
        .await?;
    fixture.trigger(false).await?;
    fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    fixture.flush().await?;
    assert_eq!(fixture.durable().await?.2, Some(2_000));
    assert!(!fixture.port.snapshot().snapshot.is_save_required());
    fixture.close().await
}

#[tokio::test]
async fn older_real_receipt_cannot_erase_a_newer_accepted_edit() -> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(false).await?;
    fixture.start().await?;
    fixture.wall.set(3_000, false);
    fixture.play("e2", "e4", 2).await?;
    fixture.actor.begin_save(false);
    let result = DesktopGameContentActor::save_ready(&mut fixture.actor.saving).await;
    fixture.actor.saving = None;
    fixture.wall.set(2_000, false);
    fixture.play("e7", "e5", 3).await?;
    fixture.actor.finish_save(result)?;
    let state = fixture.actor.session.take()?;
    let edit = state
        .accepted_edit
        .ok_or("newer edit cleared by older receipt")?;
    assert_eq!(edit.revision(), state.session.revision());
    assert_eq!(edit.timestamp().value(), 3_000);
    fixture.actor.session.replace(state)?;
    fixture.flush().await?;
    assert_eq!(fixture.durable().await?.2, Some(3_000));
    fixture.close().await
}

#[tokio::test]
async fn nonterminal_pause_and_clock_do_not_read_an_unavailable_wall_clock()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(true).await?;
    fixture.start().await?;
    let reads = fixture.wall.reads();
    fixture.wall.set(2_000, true);
    fixture.clock.set(20);
    fixture
        .compute(DesktopGameSupervisorEnumComputation::Clock)
        .await?;
    fixture
        .action(DesktopGameCommandsEnumAction::Pause {
            cause: GamePauseEnumCause::Manual,
        })
        .await?;
    fixture.flush().await?;
    assert_eq!(fixture.wall.reads(), reads);
    assert_eq!(fixture.durable().await?.2, Some(1_000));
    fixture.close().await
}

#[tokio::test]
async fn deadline_wall_failure_retains_stopped_pending_result_without_timer_spin()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(true).await?;
    fixture.defer().await?;
    let reads = fixture.wall.reads();
    let proofs = fixture.proof.0.load(Ordering::SeqCst);
    let revision = fixture.port.snapshot().snapshot.revision();
    fixture.clock.set(1_000_000);
    for _ in 0..4 {
        fixture
            .compute(DesktopGameSupervisorEnumComputation::Clock)
            .await?;
        assert!(fixture.actor.deadline()?.is_none());
    }
    {
        let running = fixture.actor.run_loop();
        tokio::pin!(running);
        for _ in 0..4 {
            poll_fn(|context| {
                assert!(running.as_mut().poll(context).is_pending());
                Poll::Ready(())
            })
            .await;
        }
    }
    assert_eq!(fixture.wall.reads(), reads);
    assert_eq!(fixture.proof.0.load(Ordering::SeqCst), proofs);
    assert_eq!(fixture.port.snapshot().snapshot.revision(), revision);
    assert!(matches!(
        fixture.port.snapshot().persistence,
        DesktopGameSupervisorEnumPersistence::Failed(_)
    ));
    fixture.wall.set(3_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    fixture.flush().await?;
    fixture.close().await
}

#[tokio::test]
async fn explicit_retry_accepts_retained_outcome_at_retry_utc_without_readjudication()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(true).await?;
    fixture.defer().await?;
    let proofs = fixture.proof.0.load(Ordering::SeqCst);
    fixture.clock.set(1_000_000);
    fixture.wall.set(80_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    let state = fixture.actor.session.take()?;
    assert!(state.pending_result.is_none());
    assert!(state.session.outcome().is_some());
    assert_eq!(
        state
            .accepted_edit
            .ok_or("accepted result has no stamp")?
            .cause(),
        GameStorageEnumEditCause::Result
    );
    fixture.actor.session.replace(state)?;
    fixture.flush().await?;
    assert_eq!(fixture.proof.0.load(Ordering::SeqCst), proofs);
    assert_eq!(fixture.durable().await?.2, Some(80_000));
    fixture.close().await
}

#[tokio::test]
async fn failed_wall_retry_preserves_original_token_and_cause() -> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(true).await?;
    fixture.defer().await?;
    let state = fixture.actor.session.take()?;
    let original = Arc::clone(&state.pending_result.as_ref().ok_or("token absent")?.failure);
    let observed = state
        .pending_result
        .as_ref()
        .ok_or("token absent")?
        .token
        .observed_at();
    fixture.actor.session.replace(state)?;
    fixture.clock.set(500);
    let error = fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await;
    assert!(error.is_err());
    let state = fixture.actor.session.take()?;
    let pending = state
        .pending_result
        .as_ref()
        .ok_or("failed retry dropped token")?;
    assert!(Arc::ptr_eq(&pending.failure, &original));
    assert_eq!(pending.token.observed_at(), observed);
    assert_eq!(state.session.outcome(), None);
    fixture.actor.session.replace(state)?;
    fixture.wall.set(4_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    fixture.flush().await?;
    fixture.close().await
}

#[tokio::test]
async fn sqlite_failure_after_deferred_acceptance_keeps_accepted_utc_without_another_wall_read()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(true).await?;
    fixture.defer().await?;
    fixture.wall.set(5_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    fixture.trigger(true).await?;
    fixture.flush().await?;
    fixture
        .compute(DesktopGameSupervisorEnumComputation::SaveFailure {
            engine: GamePauseEnumEngineBarrier::NoEngine,
            received_at: fixture.clock.now()?,
        })
        .await?;
    let reads = fixture.wall.reads();
    fixture.wall.set(90_000, true);
    fixture.trigger(false).await?;
    fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    fixture.flush().await?;
    assert_eq!(fixture.wall.reads(), reads);
    assert_eq!(fixture.durable().await?.2, Some(5_000));
    fixture.close().await
}

#[tokio::test]
async fn storage_failure_then_terminal_wall_failure_retains_both_typed_causes()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(true).await?;
    fixture.start().await?;
    fixture.clock.set(10);
    fixture.wall.set(2_000, false);
    fixture.play("e2", "e4", 2).await?;
    fixture.trigger(true).await?;
    fixture.flush().await?;
    fixture.wall.set(3_000, true);
    fixture.clock.set(110);
    fixture
        .compute(DesktopGameSupervisorEnumComputation::SaveFailure {
            engine: GamePauseEnumEngineBarrier::NoEngine,
            received_at: fixture.clock.now()?,
        })
        .await?;
    let state = fixture.actor.session.take()?;
    assert!(matches!(
        state.storage_failure.as_deref(),
        Some(DesktopGameSupervisorError::Storage(_))
    ));
    assert!(state.pending_result.is_some());
    fixture.actor.session.replace(state)?;
    let DesktopGameSupervisorEnumPersistence::Failed(error) = fixture.port.snapshot().persistence
    else {
        return Err("combined diagnostic absent".into());
    };
    assert!(matches!(
        error.as_ref(),
        DesktopGameSupervisorError::ContentSave { .. }
    ));
    fixture.trigger(false).await?;
    fixture.wall.set(6_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    fixture.flush().await?;
    fixture.close().await
}

#[tokio::test]
async fn pending_result_survives_pause_engine_failure_quit_and_retained_shutdown()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(true).await?;
    fixture.defer().await?;
    let revision = fixture.port.snapshot().snapshot.revision();
    let reads = fixture.wall.reads();
    fixture
        .action(DesktopGameCommandsEnumAction::Pause {
            cause: GamePauseEnumCause::Manual,
        })
        .await?;
    fixture
        .compute(DesktopGameSupervisorEnumComputation::EngineFailure)
        .await?;
    fixture.action(DesktopGameCommandsEnumAction::Quit).await?;
    let stopped = Arc::new(AtomicBool::new(false));
    fixture.actor.engine = Some(DesktopGameContentEngine(Arc::clone(&stopped)));
    assert!(matches!(
        fixture
            .actor
            .shutdown_retained(DesktopGameContentFixture::deadline())
            .await,
        Err(DesktopGameSupervisorError::SaveRequired)
    ));
    assert!(stopped.load(Ordering::SeqCst));
    assert!(!fixture.actor.is_shutdown_complete());
    assert_eq!(fixture.port.snapshot().snapshot.revision(), revision);
    assert_eq!(fixture.wall.reads(), reads);
    let state = fixture.actor.session.take()?;
    assert!(state.pending_result.is_some());
    assert_eq!(state.session.outcome(), None);
    fixture.actor.session.replace(state)?;
    fixture.wall.set(7_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    fixture.flush().await?;
    fixture.close().await
}

#[tokio::test]
async fn stale_action_is_rejected_before_wall_sampling_and_failed_preflight_changes_no_revisions()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(false).await?;
    fixture.start().await?;
    let durable = fixture.durable().await?;
    let revision = fixture.port.snapshot().snapshot.revision();
    fixture.wall.set(9_000, true);
    let reads = fixture.wall.reads();
    let operation = DesktopGameSupervisorEnumComputation::Action(DesktopGameTransitionOptions {
        request: DesktopGameCommandsRequest {
            request_id: DesktopRequestId::new(),
            expected_revision: crate::game::enums::GameRevision::ZERO,
            received_at: fixture.clock.now()?,
            action: DesktopGameCommandsEnumAction::Play {
                node: uuid::Uuid::from_u128(2).to_string().parse()?,
                origin: "e2".parse()?,
                destination: "e4".parse()?,
                promotion: None,
                color: BoardEnumColor::White,
            },
        },
        engine: GamePauseEnumEngineBarrier::NoEngine,
        readiness: GameEnumEngineReadiness::NotRequired,
    });
    assert!(fixture.compute(operation).await.is_err());
    assert_eq!(fixture.wall.reads(), reads);
    assert!(fixture.play("e2", "e4", 2).await.is_err());
    assert_eq!(fixture.port.snapshot().snapshot.revision(), revision);
    let state = fixture.actor.session.take()?;
    assert_eq!(state.session.revision(), revision);
    assert_eq!(state.session.tree().len(), 1);
    fixture.actor.session.replace(state)?;
    assert_eq!(fixture.durable().await?, durable);
    fixture.close().await
}

#[tokio::test]
async fn checkpoint_reopen_navigation_preserve_known_and_unknown_modified_time()
-> DesktopGameContentTestResult {
    for modified in [Some(1_000), None] {
        let mut fixture = DesktopGameContentFixture::new(false).await?;
        fixture.start().await?;
        fixture.play("e2", "e4", 2).await?;
        fixture
            .action(DesktopGameCommandsEnumAction::Pause {
                cause: GamePauseEnumCause::Manual,
            })
            .await?;
        fixture.flush().await?;
        if modified.is_none() {
            fixture
                .inspect(|connection| {
                    connection
                        .execute("UPDATE library_entries SET modified_ms = NULL")
                        .map(|_| ())
                })
                .await?;
        }
        let recovered =
            StorageLibraryPlayedRepository::new(fixture.store.client(), &fixture.executor)
                .load(fixture.port.snapshot().snapshot.id())
                .await?
                .ok_or("saved owner absent")?;
        let revision = recovered.storage_revision();
        let session = recovered.into_session();
        let root = session.tree().root();
        fixture
            .actor
            .shutdown_retained(DesktopGameContentFixture::deadline())
            .await?;
        let reads = fixture.wall.reads();
        fixture.wall.set(90_000, true);
        let (actor, port) = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
            session,
            storage_revision: revision,
            storage: fixture.store.client(),
            executor: Arc::clone(&fixture.executor),
            clock: Arc::clone(&fixture.clock),
            wall_time: Arc::<DesktopGameContentWall>::clone(&fixture.wall),
            proof: Arc::clone(&fixture.proof),
            factory: DesktopGameContentFactory,
            search_config: GameEngineSearchConfig::initial_default(),
        })
        .await?;
        fixture.actor = actor;
        fixture.port = port;
        fixture
            .action(DesktopGameCommandsEnumAction::Navigate { node: root })
            .await?;
        fixture
            .action(DesktopGameCommandsEnumAction::RetrySave)
            .await?;
        fixture.flush().await?;
        assert_eq!(fixture.durable().await?.2, modified);
        assert_eq!(fixture.wall.reads(), reads);
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn takeback_request_and_response_observe_deadline_before_content_preflight()
-> DesktopGameContentTestResult {
    for response in [false, true] {
        let mut fixture = DesktopGameContentFixture::new(true).await?;
        fixture.start().await?;
        fixture.clock.set(10);
        fixture.play("e2", "e4", 2).await?;
        fixture.flush().await?;
        if response {
            fixture
                .action(DesktopGameCommandsEnumAction::RequestTakeback {
                    requester: BoardEnumColor::White,
                })
                .await?;
            fixture.flush().await?;
        }
        fixture.clock.set(110);
        fixture.wall.set(3_000, true);
        fixture
            .action(if response {
                DesktopGameCommandsEnumAction::RespondTakeback {
                    responder: BoardEnumColor::Black,
                    response: GameTakebackEnumResponse::Accept,
                }
            } else {
                DesktopGameCommandsEnumAction::RequestTakeback {
                    requester: BoardEnumColor::White,
                }
            })
            .await?;
        let state = fixture.actor.session.take()?;
        assert!(state.pending_result.is_some());
        assert_eq!(state.session.tree().len(), 2);
        assert_eq!(
            state.session.live(),
            uuid::Uuid::from_u128(2).to_string().parse()?
        );
        fixture.actor.session.replace(state)?;
        fixture.wall.set(4_000, false);
        fixture
            .action(DesktopGameCommandsEnumAction::RetrySave)
            .await?;
        fixture.flush().await?;
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn consent_request_and_decline_are_checkpoints_but_accepted_takeback_changes_content()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(false).await?;
    fixture.start().await?;
    fixture.play("e2", "e4", 2).await?;
    fixture.flush().await?;
    fixture.wall.set(2_000, true);
    let reads = fixture.wall.reads();
    fixture
        .action(DesktopGameCommandsEnumAction::RequestTakeback {
            requester: BoardEnumColor::White,
        })
        .await?;
    fixture
        .action(DesktopGameCommandsEnumAction::RespondTakeback {
            responder: BoardEnumColor::Black,
            response: GameTakebackEnumResponse::Decline,
        })
        .await?;
    fixture.flush().await?;
    assert_eq!(fixture.wall.reads(), reads);
    fixture
        .action(DesktopGameCommandsEnumAction::ConfigureTakeback {
            consent: GameTakebackEnumConsent::NotRequired,
        })
        .await?;
    assert!(
        fixture
            .action(DesktopGameCommandsEnumAction::RequestTakeback {
                requester: BoardEnumColor::White
            })
            .await
            .is_err()
    );
    fixture.wall.set(4_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::RequestTakeback {
            requester: BoardEnumColor::White,
        })
        .await?;
    fixture.flush().await?;
    assert_eq!(fixture.durable().await?.2, Some(4_000));
    fixture.close().await
}

#[tokio::test]
async fn engine_checkpoint_and_queued_premove_survive_later_wall_failure()
-> DesktopGameContentTestResult {
    let mut session = DesktopGameContentFixture::session(false)?;
    let options = session.restoration(ClockInstant::from_nanos(0))?;
    let mut options = options;
    options.session.seats = GameSeats::new(
        GameEnumMode::HumanVersusEngine,
        GameEnumSeat::Reckless,
        GameEnumSeat::Human,
    )?;
    session = GameSession::restore(options)?;
    session.set_premoves_enabled(session.revision(), true)?;
    session.start(
        session.revision(),
        ClockInstant::from_nanos(0),
        GameEnumEngineReadiness::Ready,
        &DesktopGameContentProof::default(),
    )?;
    session.queue_premove(
        session.revision(),
        GamePremoveIntent::new("e7".parse()?, "e5".parse()?, None)?,
    )?;
    let context = GameEngineContext::new(session.id(), session.revision());
    let movement = session
        .tree()
        .node(session.live())?
        .position()
        .parse_uci("e2e4")?;
    let mut fixture = DesktopGameContentFixture::with_session(session, 0).await?;
    fixture.flush().await?;
    fixture.wall.set(2_000, false);
    let identity = GameEngineSearchIdentity::new(
        context,
        uuid::Uuid::from_u128(20).to_string().parse()?,
        uuid::Uuid::from_u128(21).to_string().parse()?,
    );
    fixture
        .compute(DesktopGameSupervisorEnumComputation::EngineMove(
            GameEngineMove::new(identity, movement, ClockInstant::from_nanos(0)),
        ))
        .await?;
    let checkpoint = fixture
        .actor
        .writes
        .front()
        .ok_or("engine checkpoint missing")?
        .request
        .clone();
    let pending = fixture
        .actor
        .pending_premove
        .take()
        .ok_or("premove continuation missing")?;
    fixture.wall.set(3_000, true);
    fixture
        .compute(DesktopGameSupervisorEnumComputation::Premove(pending))
        .await?;
    assert!(matches!(
        fixture.port.snapshot().notice.as_deref(),
        Some(DesktopGameSupervisorError::Premove { .. })
    ));
    assert_eq!(
        fixture
            .actor
            .writes
            .front()
            .ok_or("engine checkpoint lost")?
            .request
            .game_revision(),
        checkpoint.game_revision()
    );
    let state = fixture.actor.session.take()?;
    assert!(state.session.premove().pending().is_some());
    assert_eq!(state.session.revision(), checkpoint.game_revision());
    fixture.actor.session.replace(state)?;
    fixture.flush().await?;
    assert_eq!(fixture.durable().await?.2, Some(2_000));
    fixture.actor.engine = Some(DesktopGameContentEngine(Arc::new(AtomicBool::new(false))));
    fixture.close().await
}

#[tokio::test]
async fn pending_result_resave_cannot_accept_or_serialize_it() -> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(true).await?;
    fixture.defer().await?;
    fixture.wall.set(9_000, false);
    let reads = fixture.wall.reads();
    fixture
        .compute(DesktopGameSupervisorEnumComputation::Resave { quit: false })
        .await?;
    assert!(fixture.actor.writes.is_empty());
    assert_eq!(fixture.wall.reads(), reads);
    let state = fixture.actor.session.take()?;
    assert!(state.pending_result.is_some());
    assert_eq!(state.session.outcome(), None);
    fixture.actor.session.replace(state)?;
    fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    fixture.flush().await?;
    fixture.close().await
}

#[tokio::test]
async fn cancelled_save_requeue_keeps_original_content_stamp() -> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(false).await?;
    fixture.start().await?;
    fixture.wall.set(2_000, false);
    fixture.play("e2", "e4", 2).await?;
    fixture.actor.begin_save(false);
    drop(fixture.actor.saving.take());
    fixture
        .actor
        .finish_save(Err(StorageLibraryError::Cancelled))?;
    fixture.actor.begin_save(false);
    drop(fixture.actor.saving.take());
    fixture.actor.finish_save(Err(StorageLibraryError::Codec(
        crate::storage::game_document::StorageGameDocumentError::Cancelled,
    )))?;
    fixture.wall.set(90_000, true);
    fixture.flush().await?;
    assert_eq!(fixture.durable().await?.2, Some(2_000));
    fixture.close().await
}

#[tokio::test]
async fn every_active_safety_path_defers_a_terminal_result_on_wall_failure()
-> DesktopGameContentTestResult {
    for action in [
        DesktopGameCommandsEnumAction::Pause {
            cause: GamePauseEnumCause::Manual,
        },
        DesktopGameCommandsEnumAction::Stop,
        DesktopGameCommandsEnumAction::Quit,
    ] {
        let mut fixture = DesktopGameContentFixture::new(true).await?;
        fixture.start().await?;
        fixture.clock.set(100);
        fixture.wall.set(9_000, true);
        fixture.action(action).await?;
        let state = fixture.actor.session.take()?;
        assert!(state.pending_result.is_some());
        assert_eq!(state.session.outcome(), None);
        assert_eq!(state.session.save_status(), GameSaveEnumStatus::Required);
        fixture.actor.session.replace(state)?;
        fixture.wall.set(10_000, false);
        fixture
            .action(DesktopGameCommandsEnumAction::RetrySave)
            .await?;
        fixture.flush().await?;
        fixture.close().await?;
    }
    for shutdown in [false, true] {
        let mut fixture = DesktopGameContentFixture::new(true).await?;
        fixture.start().await?;
        fixture.clock.set(100);
        fixture.wall.set(9_000, true);
        if shutdown {
            assert!(matches!(
                fixture
                    .actor
                    .shutdown_retained(DesktopGameContentFixture::deadline())
                    .await,
                Err(DesktopGameSupervisorError::SaveRequired)
            ));
        } else {
            fixture
                .compute(DesktopGameSupervisorEnumComputation::EngineFailure)
                .await?;
        }
        let state = fixture.actor.session.take()?;
        assert!(state.pending_result.is_some());
        assert_eq!(state.session.outcome(), None);
        fixture.actor.session.replace(state)?;
        fixture.wall.set(10_000, false);
        fixture
            .action(DesktopGameCommandsEnumAction::RetrySave)
            .await?;
        fixture.flush().await?;
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn pending_navigation_preserves_live_revision_and_token_guards()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(true).await?;
    fixture.start().await?;
    fixture.clock.set(10);
    fixture.play("e2", "e4", 2).await?;
    fixture.flush().await?;
    fixture.clock.set(110);
    fixture.wall.set(3_000, true);
    fixture
        .compute(DesktopGameSupervisorEnumComputation::Clock)
        .await?;
    let state = fixture.actor.session.take()?;
    let root = state.session.tree().root();
    let live = state.session.live();
    let revision = state.session.revision();
    fixture.actor.session.replace(state)?;
    let reads = fixture.wall.reads();
    fixture
        .action(DesktopGameCommandsEnumAction::Navigate { node: root })
        .await?;
    assert!(
        fixture
            .action(DesktopGameCommandsEnumAction::Resume)
            .await
            .is_err()
    );
    assert!(fixture.play("e7", "e5", 3).await.is_err());
    assert!(
        fixture
            .action(DesktopGameCommandsEnumAction::RequestTakeback {
                requester: BoardEnumColor::White
            })
            .await
            .is_err()
    );
    let state = fixture.actor.session.take()?;
    assert_eq!(state.session.live(), live);
    assert_eq!(state.session.view(), root);
    assert_eq!(state.session.revision(), revision);
    fixture.actor.session.replace(state)?;
    assert_eq!(fixture.wall.reads(), reads);
    fixture.wall.set(4_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    fixture.flush().await?;
    fixture.close().await
}

#[tokio::test]
async fn stale_pending_token_is_rejected_before_wall_read_and_remains_owned()
-> DesktopGameContentTestResult {
    let mut original = DesktopGameContentFixture::new(true).await?;
    original.defer().await?;
    let mut other = DesktopGameContentFixture::new(false).await?;
    let mut state = original.actor.session.take()?;
    let token = state.pending_result.take();
    original.actor.session.replace(state)?;
    let mut state = other.actor.session.take()?;
    state.pending_result = token;
    other.actor.session.replace(state)?;
    let reads = other.wall.reads();
    assert!(
        other
            .action(DesktopGameCommandsEnumAction::RetrySave)
            .await
            .is_err()
    );
    assert_eq!(other.wall.reads(), reads);
    let mut state = other.actor.session.take()?;
    let token = state.pending_result.take();
    assert!(token.is_some());
    assert_eq!(state.session.outcome(), None);
    other.actor.session.replace(state)?;
    let mut state = original.actor.session.take()?;
    state.pending_result = token;
    original.actor.session.replace(state)?;
    original.wall.set(4_000, false);
    original
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    original.flush().await?;
    original.close().await?;
    other.close().await
}

#[tokio::test]
async fn fresh_wall_failure_refuses_owner_exposure_without_durable_content()
-> DesktopGameContentTestResult {
    let fixture = DesktopGameContentFixture::new(false).await?;
    fixture.wall.set(3_000, true);
    let result = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
        session: DesktopGameContentFixture::session(false)?,
        storage_revision: GameStorageRevision::INITIAL,
        storage: fixture.store.client(),
        executor: Arc::clone(&fixture.executor),
        clock: Arc::clone(&fixture.clock),
        wall_time: Arc::<DesktopGameContentWall>::clone(&fixture.wall),
        proof: Arc::clone(&fixture.proof),
        factory: DesktopGameContentFactory,
        search_config: GameEngineSearchConfig::initial_default(),
    })
    .await;
    assert!(matches!(
        result,
        Err(DesktopGameSupervisorError::WallTime(
            GameStorageWallTimeError::Unavailable
        ))
    ));
    let counts: (i64, i64) = fixture
        .inspect(|connection| {
            Ok((
                connection.integer("SELECT COUNT(*) AS value FROM documents")?,
                connection.integer("SELECT COUNT(*) AS value FROM library_entries")?,
            ))
        })
        .await?;
    assert_eq!(counts, (0, 0));
    fixture.close().await
}

#[tokio::test]
async fn start_and_play_result_deltas_use_result_cause_and_actual_accepted_utc()
-> DesktopGameContentTestResult {
    let mut session =
        DesktopGameContentFixture::session(false)?.restoration(ClockInstant::from_nanos(0))?;
    session.session.start = BoardStart::custom(BoardPosition::from_fen(
        "7k/6Q1/5K2/8/8/8/8/8 b - - 0 1",
        crate::board::enums::BoardEnumVariant::Standard,
    )?);
    session.session.clock = ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::Black);
    session.clock = session
        .session
        .clock
        .snapshot(ClockInstant::from_nanos(0))?;
    session.root_clock = Some(session.clock.clone());
    let mut fixture =
        DesktopGameContentFixture::with_session(GameSession::restore(session)?, 0).await?;
    fixture.flush().await?;
    fixture.wall.set(2_000, false);
    fixture.action(DesktopGameCommandsEnumAction::Start).await?;
    let state = fixture.actor.session.take()?;
    assert_eq!(
        state
            .accepted_edit
            .ok_or("start result stamp missing")?
            .cause(),
        GameStorageEnumEditCause::Result
    );
    fixture.actor.session.replace(state)?;
    fixture.flush().await?;
    assert_eq!(fixture.durable().await?.2, Some(2_000));
    fixture.close().await?;

    let mut fixture = DesktopGameContentFixture::new(false).await?;
    fixture.start().await?;
    for (origin, destination, node) in [("f2", "f3", 2), ("e7", "e5", 3), ("g2", "g4", 4)] {
        fixture.play(origin, destination, node).await?;
        fixture.flush().await?;
    }
    fixture.wall.set(4_000, false);
    fixture.play("d8", "h4", 5).await?;
    let state = fixture.actor.session.take()?;
    assert_eq!(
        state
            .accepted_edit
            .ok_or("move result stamp missing")?
            .cause(),
        GameStorageEnumEditCause::Result
    );
    assert!(state.session.outcome().is_some());
    fixture.actor.session.replace(state)?;
    fixture.flush().await?;
    assert_eq!(fixture.durable().await?.2, Some(4_000));
    fixture.close().await
}

struct DesktopGameContentUnresolvedProof;

impl GameAdjudicationMateProofProvider for DesktopGameContentUnresolvedProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Unknown(
            crate::game::adjudication::GameAdjudicationEnumInterruption::Unresolved,
        )
    }
}

#[tokio::test]
async fn resume_from_unresolved_flagfall_stamps_the_actual_result_delta()
-> DesktopGameContentTestResult {
    let mut session = DesktopGameContentFixture::session(true)?;
    session.start(
        session.revision(),
        ClockInstant::from_nanos(0),
        GameEnumEngineReadiness::NotRequired,
        &DesktopGameContentProof::default(),
    )?;
    session.poll_deadline(
        session.revision(),
        ClockInstant::from_millis(100)?,
        &DesktopGameContentUnresolvedProof,
    )?;
    assert_eq!(session.phase(), GameEnumPhase::Interrupted);
    assert_eq!(session.outcome(), None);
    let mut fixture = DesktopGameContentFixture::with_session(session, 100).await?;
    fixture.flush().await?;
    fixture.clock.set(10_000);
    fixture.wall.set(4_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::Resume)
        .await?;
    let state = fixture.actor.session.take()?;
    assert_eq!(state.session.phase(), GameEnumPhase::Completed);
    assert_eq!(
        state
            .accepted_edit
            .ok_or("resume result stamp absent")?
            .cause(),
        GameStorageEnumEditCause::Result,
    );
    fixture.actor.session.replace(state)?;
    fixture.flush().await?;
    assert_eq!(fixture.durable().await?.2, Some(4_000));
    fixture.close().await
}

#[tokio::test]
async fn study_branch_changes_modified_but_begin_analysis_and_view_changes_do_not()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(false).await?;
    fixture.flush().await?;
    fixture
        .action(DesktopGameCommandsEnumAction::BeginAnalysis)
        .await?;
    fixture.flush().await?;
    assert_eq!(fixture.durable().await?.2, Some(1_000));
    fixture.wall.set(3_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::Branch {
            node: uuid::Uuid::from_u128(2).to_string().parse()?,
            origin: "e2".parse()?,
            destination: "e4".parse()?,
            promotion: None,
        })
        .await?;
    let state = fixture.actor.session.take()?;
    assert_eq!(
        state.accepted_edit.ok_or("study stamp absent")?.cause(),
        GameStorageEnumEditCause::StudyTree
    );
    let live = state.session.live();
    fixture.actor.session.replace(state)?;
    fixture.flush().await?;
    fixture.wall.set(50_000, true);
    fixture
        .action(DesktopGameCommandsEnumAction::ReturnToLive)
        .await?;
    let state = fixture.actor.session.take()?;
    assert_eq!(state.session.view(), live);
    fixture.actor.session.replace(state)?;
    fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    fixture.flush().await?;
    assert_eq!(fixture.durable().await?.2, Some(3_000));
    fixture.close().await
}

#[tokio::test]
async fn pending_quit_returns_save_required_while_actor_stays_available()
-> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(true).await?;
    fixture.defer().await?;
    let receipt = fixture.port.try_submit(DesktopGameCommandsRequest {
        request_id: DesktopRequestId::new(),
        expected_revision: fixture.port.snapshot().snapshot.revision(),
        received_at: fixture.clock.now()?,
        action: DesktopGameCommandsEnumAction::Quit,
    })?;
    let result = {
        let running = fixture.actor.run_loop();
        tokio::pin!(running);
        tokio::select! {
            result = receipt.wait() => result,
            result = &mut running => return Err(format!("pending-result actor exited: {result:?}").into()),
        }
    };
    assert!(matches!(
        result.as_ref().err().map(Arc::as_ref),
        Some(DesktopGameSupervisorError::SaveRequired)
    ));
    assert!(fixture.port.snapshot().snapshot.is_save_required());
    assert!(!fixture.port.closed.load(Ordering::Acquire));
    fixture.wall.set(5_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::RetrySave)
        .await?;
    fixture.flush().await?;
    fixture.close().await
}

#[tokio::test]
async fn repeated_existing_study_branch_preserves_modified_time() -> DesktopGameContentTestResult {
    let mut fixture = DesktopGameContentFixture::new(false).await?;
    fixture.flush().await?;
    fixture
        .action(DesktopGameCommandsEnumAction::BeginAnalysis)
        .await?;
    fixture.flush().await?;
    fixture.wall.set(3_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::Branch {
            node: uuid::Uuid::from_u128(2).to_string().parse()?,
            origin: "e2".parse()?,
            destination: "e4".parse()?,
            promotion: None,
        })
        .await?;
    fixture.flush().await?;
    let before = fixture.durable().await?.2;
    let state = fixture.actor.session.take()?;
    let tree_size = state.session.tree().len();
    let tip = state.session.study_mainline_tip();
    let live = state.session.live();
    fixture.actor.session.replace(state)?;
    fixture
        .action(DesktopGameCommandsEnumAction::ReturnToLive)
        .await?;
    fixture.wall.set(9_000, false);
    fixture
        .action(DesktopGameCommandsEnumAction::Branch {
            node: uuid::Uuid::from_u128(3).to_string().parse()?,
            origin: "e2".parse()?,
            destination: "e4".parse()?,
            promotion: None,
        })
        .await?;
    fixture.flush().await?;
    let after = fixture.durable().await?.2;
    let state = fixture.actor.session.take()?;
    let unchanged = state.session.tree().len() == tree_size
        && state.session.study_mainline_tip() == tip
        && state.session.live() == live
        && state.session.view() == tip;
    fixture.actor.session.replace(state)?;
    fixture.close().await?;
    assert!(
        unchanged,
        "the repeated branch must select the existing continuation"
    );
    assert_eq!(before, Some(3_000));
    assert_eq!(
        after, before,
        "selecting an existing study continuation changed modified time"
    );
    Ok(())
}
