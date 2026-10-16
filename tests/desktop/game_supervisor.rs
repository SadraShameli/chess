use std::future::pending;
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant as StandardInstant};

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::{ClockDurationError, ClockInstant};
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use chess::desktop::game_supervisor::{
    DesktopGameSupervisor, DesktopGameSupervisorEngineFactory, DesktopGameSupervisorEngineProvider,
    DesktopGameSupervisorError, DesktopGameSupervisorOptions, DesktopGameSupervisorPort,
};
use chess::desktop::identity::DesktopRequestId;
use chess::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use chess::game::engine_provider::{
    GameEngineContext, GameEngineEnumEvent, GameEngineProvider, GameEngineQuiescenceReceipt,
    GameEngineRequest, GameEngineSearchConfig, GameEngineSearchIdentity, GameEngineStopReceipt,
};
use chess::game::enums::{GameEnumMode, GameEnumPhase, GameEnumSeat, GameRevision};
use chess::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
use chess::game::storage_repository::{GameStorageRepository, GameStorageRevision};
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::game_document::StorageGameDocumentRepository;
use tokio::time::Instant;

type DesktopGameSupervisorTestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Default)]
struct DesktopGameSupervisorFixtureClock(AtomicU64);

impl ClockTimeProvider for DesktopGameSupervisorFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.0.load(Ordering::SeqCst)))
    }
}

struct DesktopGameSupervisorFixtureProof;

impl GameAdjudicationMateProofProvider for DesktopGameSupervisorFixtureProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

struct DesktopGameSupervisorFixtureEngine;

impl GameEngineProvider for DesktopGameSupervisorFixtureEngine {
    type Error = std::io::Error;
    fn is_idle(&self) -> bool {
        true
    }
    async fn start(
        &mut self,
        _request: GameEngineRequest,
    ) -> Result<GameEngineSearchIdentity, Self::Error> {
        Err(std::io::Error::other(
            "A local game attempted an engine search",
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
            "A local game attempted an engine stop",
        ))
    }
    async fn quiesce(
        &mut self,
        _expected: GameEngineContext,
        _received_at: StandardInstant,
    ) -> Result<GameEngineQuiescenceReceipt, Self::Error> {
        Err(std::io::Error::other(
            "A local game attempted engine quiescence",
        ))
    }
}

impl DesktopGameSupervisorEngineProvider for DesktopGameSupervisorFixtureEngine {
    fn is_shutdown_complete(&self) -> bool {
        true
    }
    async fn refresh_resources(&mut self) -> Result<(), Self::Error> {
        pending().await
    }
    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct DesktopGameSupervisorFixtureFactory;

impl DesktopGameSupervisorEngineFactory for DesktopGameSupervisorFixtureFactory {
    type Engine = DesktopGameSupervisorFixtureEngine;
    type Error = std::io::Error;
    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        Err(std::io::Error::other(
            "A local game attempted to create an engine",
        ))
    }
    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct DesktopGameSupervisorFixture;

impl DesktopGameSupervisorFixture {
    fn session() -> Result<GameSession, Box<dyn std::error::Error>> {
        Self::session_with_seats(GameSeats::new(
            GameEnumMode::LocalTwoPlayer,
            GameEnumSeat::Human,
            GameEnumSeat::Human,
        )?)
    }

    fn session_with_seats(seats: GameSeats) -> Result<GameSession, Box<dyn std::error::Error>> {
        Ok(GameSession::new(GameSessionOptions {
            id: "2e21a5df-ec1f-4199-9a13-7af9b9e1c6bd".parse()?,
            root: "932a32bb-6195-4f55-8bff-e80777c98569".parse()?,
            start: BoardStart::standard(),
            seats,
            clock: ClockState::new(ClockTimeControl::initial_default(), BoardEnumColor::White),
            metadata: GameMetadata::default(),
        })?)
    }

    fn request(
        revision: GameRevision,
        now: u64,
        action: DesktopGameCommandsEnumAction,
    ) -> DesktopGameCommandsRequest {
        DesktopGameCommandsRequest {
            request_id: DesktopRequestId::new(),
            expected_revision: revision,
            received_at: ClockInstant::from_nanos(now),
            action,
        }
    }
}

struct DesktopGameSupervisorFixtureFailedFactory {
    cleanup_attempts: Arc<AtomicU64>,
    retained: Arc<AtomicBool>,
    cleanup_allowed: Arc<AtomicBool>,
}

impl DesktopGameSupervisorEngineFactory for DesktopGameSupervisorFixtureFailedFactory {
    type Engine = DesktopGameSupervisorFixtureEngine;
    type Error = std::io::Error;

    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        self.retained.store(true, Ordering::SeqCst);
        Err(std::io::Error::other("Engine startup failed"))
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        self.cleanup_attempts.fetch_add(1, Ordering::SeqCst);
        if self.retained.load(Ordering::SeqCst) && !self.cleanup_allowed.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("Engine startup cleanup is blocked"));
        }
        self.retained.store(false, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn failed_engine_creation_observes_retained_cleanup_before_allowing_another_request()
-> DesktopGameSupervisorTestResult {
    let fixture = DesktopGameSupervisorFixtureStorage::new(2).await?;
    let retained = Arc::new(AtomicBool::new(false));
    let cleanup_allowed = Arc::new(AtomicBool::new(false));
    let cleanup_attempts = Arc::new(AtomicU64::new(0));
    let (mut supervisor, port) = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
        session: DesktopGameSupervisorFixture::session_with_seats(GameSeats::new(
            GameEnumMode::HumanVersusEngine,
            GameEnumSeat::Human,
            GameEnumSeat::Reckless,
        )?)?,
        storage_revision: GameStorageRevision::INITIAL,
        storage: fixture.store.client(),
        executor: Arc::clone(&fixture.executor),
        clock: Arc::new(DesktopGameSupervisorFixtureClock::default()),
        proof: Arc::new(DesktopGameSupervisorFixtureProof),
        factory: DesktopGameSupervisorFixtureFailedFactory {
            cleanup_attempts: Arc::clone(&cleanup_attempts),
            retained: Arc::clone(&retained),
            cleanup_allowed: Arc::clone(&cleanup_allowed),
        },
        search_config: GameEngineSearchConfig::initial_default(),
    })
    .await?;
    let worker = tokio::spawn(async move {
        let result = supervisor.run().await;
        (supervisor, result)
    });
    let failed = port
        .try_submit(DesktopGameSupervisorFixture::request(
            GameRevision::ZERO,
            0,
            DesktopGameCommandsEnumAction::Start,
        ))?
        .wait()
        .await;
    assert!(failed.is_err());
    assert!(cleanup_attempts.load(Ordering::SeqCst) > 0);
    let (mut supervisor, result) = tokio::time::timeout(Duration::from_secs(3), worker).await??;
    assert!(result.is_err());
    assert!(!supervisor.is_shutdown_complete());
    assert!(retained.load(Ordering::SeqCst));
    assert!(matches!(
        port.try_submit(DesktopGameSupervisorFixture::request(
            GameRevision::ZERO,
            0,
            DesktopGameCommandsEnumAction::RetryEngine,
        )),
        Err(DesktopGameSupervisorError::Closed)
    ));
    cleanup_allowed.store(true, Ordering::SeqCst);
    supervisor
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(supervisor.is_shutdown_complete());
    assert!(!retained.load(Ordering::SeqCst));
    drop((supervisor, port));
    fixture.close().await
}

pub(crate) struct DesktopGameSupervisorFixtureStorage {
    directory: tempfile::TempDir,
    pub(crate) budget: CapacityBudget,
    pub(crate) executor: Arc<CapacityExecutor>,
    pub(crate) store: StorageDocumentStore,
}

type DesktopGameSupervisorFixtureActor = DesktopGameSupervisor<
    DesktopGameSupervisorFixtureFactory,
    DesktopGameSupervisorFixtureClock,
    DesktopGameSupervisorFixtureProof,
>;

impl DesktopGameSupervisorFixtureStorage {
    pub(crate) async fn new(workers: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::new(workers).ok_or("invalid capacity")?)?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store =
            StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget.clone())
                .await?;
        Ok(Self {
            directory,
            budget,
            executor,
            store,
        })
    }

    async fn supervisor(
        &self,
    ) -> Result<
        (DesktopGameSupervisorFixtureActor, DesktopGameSupervisorPort),
        Box<dyn std::error::Error>,
    > {
        Ok(DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
            session: DesktopGameSupervisorFixture::session()?,
            storage_revision: GameStorageRevision::INITIAL,
            storage: self.store.client(),
            executor: Arc::clone(&self.executor),
            clock: Arc::new(DesktopGameSupervisorFixtureClock::default()),
            proof: Arc::new(DesktopGameSupervisorFixtureProof),
            factory: DesktopGameSupervisorFixtureFactory,
            search_config: GameEngineSearchConfig::initial_default(),
        })
        .await?)
    }

    pub(crate) async fn set_save_failure(&self, enabled: bool) -> DesktopGameSupervisorTestResult {
        let path = self.directory.path().join("library.sqlite3");
        let sql = if enabled {
            "CREATE TRIGGER game_supervisor_save_failure BEFORE INSERT ON documents BEGIN SELECT RAISE(FAIL, 'injected autosave failure'); END;"
        } else {
            "DROP TRIGGER game_supervisor_save_failure;"
        };
        self.executor
            .submit_waiting(move |_cancellation| {
                let connection = rusqlite::Connection::open(path)?;
                connection.execute_batch(sql)
            })
            .await?
            .join()
            .await??;
        Ok(())
    }

    pub(crate) async fn close(self) -> DesktopGameSupervisorTestResult {
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|_owner| "executor still borrowed")?
            .shutdown()
            .await?;
        self.directory.close()?;
        Ok(())
    }
}

#[tokio::test]
async fn retained_shutdown_retries_a_real_storage_failure_and_keeps_the_save_gate_until_receipt()
-> DesktopGameSupervisorTestResult {
    let fixture = DesktopGameSupervisorFixtureStorage::new(2).await?;
    let (mut supervisor, port) = fixture.supervisor().await?;
    let game = port.snapshot().snapshot.id();
    fixture.set_save_failure(true).await?;
    assert!(
        supervisor
            .shutdown_retained(Instant::now() + Duration::from_secs(3))
            .await
            .is_err()
    );
    assert!(!supervisor.is_shutdown_complete());
    assert!(port.snapshot().snapshot.is_save_required());
    let unsaved_revision = port.snapshot().snapshot.revision();
    assert!(
        supervisor
            .shutdown_retained(Instant::now() + Duration::from_secs(3))
            .await
            .is_err()
    );
    assert_eq!(port.snapshot().snapshot.revision(), unsaved_revision);
    assert!(port.snapshot().snapshot.is_save_required());
    fixture.set_save_failure(false).await?;
    supervisor
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(supervisor.is_shutdown_complete());
    assert!(!port.snapshot().snapshot.is_save_required());
    let repository = StorageGameDocumentRepository::new(fixture.store.client(), &fixture.executor);
    let restored = repository.load(game).await?.ok_or("missing retried game")?;
    assert_eq!(
        restored.session().revision(),
        port.snapshot().snapshot.revision()
    );
    drop((supervisor, port, repository));
    fixture.close().await
}

struct DesktopGameSupervisorFixturePendingFactory {
    entered: Arc<tokio::sync::Notify>,
    stopped: Arc<AtomicBool>,
}

impl DesktopGameSupervisorEngineFactory for DesktopGameSupervisorFixturePendingFactory {
    type Engine = DesktopGameSupervisorFixtureEngine;
    type Error = std::io::Error;

    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        self.entered.notify_one();
        pending().await
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        self.stopped.store(true, Ordering::Release);
        Ok(())
    }
}

#[tokio::test]
async fn preparation_settles_uncommitted_engine_creation_after_observing_factory_cleanup()
-> DesktopGameSupervisorTestResult {
    use chess::capacity::phase::CapacityPhase;

    let fixture = DesktopGameSupervisorFixtureStorage::new(CapacityPhase::MINIMUM_WORKERS).await?;
    let entered = Arc::new(tokio::sync::Notify::new());
    let stopped = Arc::new(AtomicBool::new(false));
    let (mut supervisor, port) = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
        session: DesktopGameSupervisorFixture::session_with_seats(GameSeats::new(
            GameEnumMode::HumanVersusEngine,
            GameEnumSeat::Human,
            GameEnumSeat::Reckless,
        )?)?,
        storage_revision: GameStorageRevision::INITIAL,
        storage: fixture.store.client(),
        executor: Arc::clone(&fixture.executor),
        clock: Arc::new(DesktopGameSupervisorFixtureClock::default()),
        proof: Arc::new(DesktopGameSupervisorFixtureProof),
        factory: DesktopGameSupervisorFixturePendingFactory {
            entered: Arc::clone(&entered),
            stopped: Arc::clone(&stopped),
        },
        search_config: GameEngineSearchConfig::initial_default(),
    })
    .await?;
    let worker = tokio::spawn(async move {
        let result = supervisor.run().await;
        (supervisor, result)
    });
    let mut start = port.try_submit(DesktopGameSupervisorFixture::request(
        GameRevision::ZERO,
        0,
        DesktopGameCommandsEnumAction::Start,
    ))?;
    entered.notified().await;
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    port.try_submit(DesktopGameSupervisorFixture::request(
        GameRevision::ZERO,
        0,
        DesktopGameCommandsEnumAction::QuiesceForPreparation { generation },
    ))?
    .wait()
    .await?;
    let settled = start.try_result()?;
    let stopped_before_response = stopped.load(Ordering::Acquire);
    let rejected_during_preparation = if settled.is_some() {
        let response = port
            .try_submit(DesktopGameSupervisorFixture::request(
                GameRevision::ZERO,
                0,
                DesktopGameCommandsEnumAction::Start,
            ))?
            .wait()
            .await;
        matches!(response, Err(ref error) if matches!(error.as_ref(), DesktopGameSupervisorError::Busy))
    } else {
        false
    };
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    port.try_submit(DesktopGameSupervisorFixture::request(
        GameRevision::ZERO,
        0,
        DesktopGameCommandsEnumAction::Quit,
    ))?
    .wait()
    .await?;
    let (supervisor, result) = worker.await?;
    result?;
    assert!(supervisor.is_shutdown_complete());
    drop((supervisor, port, start, phase));
    fixture.close().await?;
    assert!(
        matches!(settled, Some(Err(ref error)) if matches!(error.as_ref(), DesktopGameSupervisorError::Closed))
    );
    assert!(stopped_before_response);
    assert!(rejected_during_preparation);
    Ok(())
}

#[tokio::test]
async fn native_local_game_start_move_and_quit_are_durable_and_restore_paused()
-> DesktopGameSupervisorTestResult {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("invalid capacity")?)?;
    let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
    let clock = Arc::new(DesktopGameSupervisorFixtureClock::default());
    let session = DesktopGameSupervisorFixture::session()?;
    let game = session.id();
    let (mut supervisor, port) = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
        session,
        storage_revision: GameStorageRevision::INITIAL,
        storage: store.client(),
        executor: Arc::clone(&executor),
        clock: Arc::clone(&clock),
        proof: Arc::new(DesktopGameSupervisorFixtureProof),
        factory: DesktopGameSupervisorFixtureFactory,
        search_config: GameEngineSearchConfig::initial_default(),
    })
    .await?;
    let worker = tokio::spawn(async move { supervisor.run().await });
    let started = port
        .try_submit(DesktopGameSupervisorFixture::request(
            GameRevision::ZERO,
            0,
            DesktopGameCommandsEnumAction::Start,
        ))?
        .wait()
        .await?;
    assert_eq!(started.phase(), GameEnumPhase::Active);
    clock.0.store(500_000_000, Ordering::SeqCst);
    let moved = port
        .try_submit(DesktopGameSupervisorFixture::request(
            started.revision(),
            500_000_000,
            DesktopGameCommandsEnumAction::Play {
                node: "dc5e04f3-cfb6-4f23-ab28-541851ad0075".parse()?,
                origin: "e2".parse()?,
                destination: "e4".parse()?,
                promotion: None,
                color: BoardEnumColor::White,
            },
        ))?
        .wait()
        .await?;
    assert_eq!(moved.live_turn(), BoardEnumColor::Black);
    assert_eq!(moved.moves().last().ok_or("missing move")?.san(), "e4");
    let stopped = port
        .try_submit(DesktopGameSupervisorFixture::request(
            moved.revision(),
            500_000_000,
            DesktopGameCommandsEnumAction::Quit,
        ))?
        .wait()
        .await?;
    assert_eq!(stopped.phase(), GameEnumPhase::Interrupted);
    tokio::time::timeout(Duration::from_secs(3), worker).await???;
    let repository = StorageGameDocumentRepository::new(store.client(), &executor);
    let recovered = repository.load(game).await?.ok_or("missing durable game")?;
    assert_eq!(recovered.session().revision(), stopped.revision());
    assert!(!recovered.session().clock().is_running());
    assert_eq!(recovered.session().tree().len(), 2);
    drop((repository, port));
    store.close().await?;
    Arc::try_unwrap(executor)
        .map_err(|_owner| "executor still borrowed")?
        .shutdown()
        .await?;
    Ok(())
}

#[tokio::test]
async fn preparation_control_remains_admissible_while_a_save_waits_for_cpu_capacity()
-> DesktopGameSupervisorTestResult {
    use chess::capacity::phase::CapacityPhase;

    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::new(4).ok_or("invalid capacity")?)?;
    let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget.clone())
            .await?;
    let (mut supervisor, port) = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
        session: DesktopGameSupervisorFixture::session()?,
        storage_revision: GameStorageRevision::INITIAL,
        storage: store.client(),
        executor: Arc::clone(&executor),
        clock: Arc::new(DesktopGameSupervisorFixtureClock::default()),
        proof: Arc::new(DesktopGameSupervisorFixtureProof),
        factory: DesktopGameSupervisorFixtureFactory,
        search_config: GameEngineSearchConfig::initial_default(),
    })
    .await?;
    let mut phase = CapacityPhase::new(budget)?;
    let generation = phase.request_preparation()?;
    let worker = tokio::spawn(async move { supervisor.run().await });
    let quiescent = port
        .try_submit(DesktopGameSupervisorFixture::request(
            GameRevision::ZERO,
            0,
            DesktopGameCommandsEnumAction::QuiesceForPreparation { generation },
        ))?
        .wait()
        .await?;
    assert_eq!(quiescent.phase(), GameEnumPhase::Setup);
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    port.try_submit(DesktopGameSupervisorFixture::request(
        GameRevision::ZERO,
        0,
        DesktopGameCommandsEnumAction::ResumeSearches { generation },
    ))?
    .wait()
    .await?;
    port.try_submit(DesktopGameSupervisorFixture::request(
        GameRevision::ZERO,
        0,
        DesktopGameCommandsEnumAction::Quit,
    ))?
    .wait()
    .await?;
    tokio::time::timeout(Duration::from_secs(3), worker).await???;
    drop((phase, port));
    store.close().await?;
    Arc::try_unwrap(executor)
        .map_err(|_owner| "executor still borrowed")?
        .shutdown()
        .await?;
    Ok(())
}

#[tokio::test]
async fn adjudication_retry_is_revision_checked_and_durably_preserves_unknown_state()
-> DesktopGameSupervisorTestResult {
    use crate::game_transition_fixture::{
        GameTransitionFixture, GameTransitionFixtureUnresolvedProof,
    };
    use chess::clock::state::ClockStateEnumStatus;
    use chess::desktop::game_snapshot::DesktopGameSnapshot;

    let fixture = DesktopGameSupervisorFixtureStorage::new(2).await?;
    let proof = Arc::new(GameTransitionFixtureUnresolvedProof::new(
        GameAdjudicationEnumMateQuestion::AnySide,
    ));
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    GameTransitionFixture::play_with_proof(&mut session, 2, "e2e4", 0, proof.as_ref())?;
    let before = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let (mut supervisor, port) = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
        session,
        storage_revision: GameStorageRevision::INITIAL,
        storage: fixture.store.client(),
        executor: Arc::clone(&fixture.executor),
        clock: Arc::new(DesktopGameSupervisorFixtureClock::default()),
        proof: Arc::clone(&proof),
        factory: DesktopGameSupervisorFixtureFactory,
        search_config: GameEngineSearchConfig::initial_default(),
    })
    .await?;
    let worker = tokio::spawn(async move {
        let result = supervisor.run().await;
        (supervisor, result)
    });
    for attempt in 2..=3 {
        let revision = port.snapshot().snapshot.revision();
        let retried = port
            .try_submit(DesktopGameSupervisorFixture::request(
                revision,
                0,
                DesktopGameCommandsEnumAction::Resume,
            ))?
            .wait()
            .await?;
        assert_eq!(proof.calls(), attempt);
        assert_eq!(retried.revision(), revision.next()?);
        assert_eq!(retried.phase(), GameEnumPhase::Interrupted);
        assert_eq!(retried.interruption(), before.interruption());
        assert_eq!(retried.outcome(), None);
        assert_eq!(retried.board().fen(), before.board().fen());
        assert_eq!(retried.live(), before.live());
        assert_eq!(retried.view(), before.view());
        assert_eq!(retried.clock(), before.clock());
        assert_eq!(retried.clock().status(), ClockStateEnumStatus::Paused);
        assert!(!retried.is_save_required());
        let error = port
            .try_submit(DesktopGameSupervisorFixture::request(
                revision,
                0,
                DesktopGameCommandsEnumAction::Resume,
            ))?
            .wait()
            .await
            .err()
            .ok_or("stale adjudication retry was accepted")?;
        assert!(matches!(
            error.as_ref(),
            DesktopGameSupervisorError::Revision(_)
        ));
        assert_eq!(proof.calls(), attempt);
        assert_eq!(port.snapshot().snapshot.revision(), retried.revision());
    }
    let repository = StorageGameDocumentRepository::new(fixture.store.client(), &fixture.executor);
    let restored = repository
        .load(before.id())
        .await?
        .ok_or("missing saved adjudication retry")?;
    assert_eq!(
        restored.session().revision(),
        port.snapshot().snapshot.revision()
    );
    assert_eq!(restored.session().live(), before.live());
    assert_eq!(
        restored
            .session()
            .clock()
            .snapshot(ClockInstant::from_nanos(0))?,
        before.clock().snapshot(ClockInstant::from_nanos(0))?
    );
    assert_eq!(
        restored
            .session()
            .tree()
            .node(restored.session().live())?
            .position()
            .fen(),
        before.board().fen()
    );
    assert_eq!(restored.session().outcome(), None);
    port.try_submit(DesktopGameSupervisorFixture::request(
        port.snapshot().snapshot.revision(),
        0,
        DesktopGameCommandsEnumAction::Quit,
    ))?
    .wait()
    .await?;
    let (supervisor, result) = worker.await?;
    result?;
    assert!(supervisor.is_shutdown_complete());
    drop((supervisor, port, repository));
    fixture.close().await
}

#[tokio::test]
async fn stale_queued_mutation_cannot_replay_after_an_earlier_revision_is_saved()
-> DesktopGameSupervisorTestResult {
    let fixture = DesktopGameSupervisorFixtureStorage::new(2).await?;
    let (mut supervisor, port) = fixture.supervisor().await?;
    let first = port.try_submit(DesktopGameSupervisorFixture::request(
        GameRevision::ZERO,
        0,
        DesktopGameCommandsEnumAction::Start,
    ))?;
    let duplicate = port.try_submit(DesktopGameSupervisorFixture::request(
        GameRevision::ZERO,
        0,
        DesktopGameCommandsEnumAction::Start,
    ))?;
    let worker = tokio::spawn(async move {
        let result = supervisor.run().await;
        (supervisor, result)
    });
    let started = first.wait().await?;
    let error = duplicate
        .wait()
        .await
        .err()
        .ok_or("stale request succeeded")?;
    assert!(matches!(
        error.as_ref(),
        DesktopGameSupervisorError::Revision(_)
    ));
    assert_eq!(port.snapshot().snapshot.revision(), started.revision());
    port.try_submit(DesktopGameSupervisorFixture::request(
        started.revision(),
        0,
        DesktopGameCommandsEnumAction::Quit,
    ))?
    .wait()
    .await?;
    let (supervisor, result) = worker.await?;
    result?;
    assert!(supervisor.is_shutdown_complete());
    drop((supervisor, port));
    fixture.close().await
}

#[tokio::test]
async fn saturated_ordinary_requests_preserve_control_slots_and_observe_queued_responses()
-> DesktopGameSupervisorTestResult {
    use chess::game::pause::GamePauseEnumCause;

    let fixture = DesktopGameSupervisorFixtureStorage::new(2).await?;
    let (mut supervisor, port) = fixture.supervisor().await?;
    let mut queued = Vec::new();
    for _ in 0..DesktopGameSupervisorPort::ORDINARY_CAPACITY {
        queued.push(port.try_submit(DesktopGameSupervisorFixture::request(
            GameRevision::ZERO,
            0,
            DesktopGameCommandsEnumAction::Navigate {
                node: "932a32bb-6195-4f55-8bff-e80777c98569".parse()?,
            },
        ))?);
    }
    let extra = port.try_submit(DesktopGameSupervisorFixture::request(
        GameRevision::ZERO,
        0,
        DesktopGameCommandsEnumAction::Start,
    ));
    assert!(matches!(extra, Err(DesktopGameSupervisorError::Busy)));
    let pause = port.try_submit(DesktopGameSupervisorFixture::request(
        GameRevision::ZERO,
        0,
        DesktopGameCommandsEnumAction::Pause {
            cause: GamePauseEnumCause::Manual,
        },
    ))?;
    let repeated_pause = port.try_submit(DesktopGameSupervisorFixture::request(
        GameRevision::ZERO,
        0,
        DesktopGameCommandsEnumAction::Pause {
            cause: GamePauseEnumCause::Manual,
        },
    ));
    assert!(matches!(
        repeated_pause,
        Err(DesktopGameSupervisorError::Busy)
    ));
    let quit = port.try_submit(DesktopGameSupervisorFixture::request(
        GameRevision::ZERO,
        0,
        DesktopGameCommandsEnumAction::Quit,
    ))?;
    let worker = tokio::spawn(async move {
        let result = supervisor.run().await;
        (supervisor, result)
    });
    assert!(pause.wait().await.is_err());
    quit.wait().await?;
    let (supervisor, result) = worker.await?;
    result?;
    assert!(supervisor.is_shutdown_complete());
    for receipt in queued {
        let error = receipt
            .wait()
            .await
            .err()
            .ok_or("queued request escaped control priority")?;
        assert!(matches!(error.as_ref(), DesktopGameSupervisorError::Closed));
    }
    drop((supervisor, port));
    fixture.close().await
}

#[tokio::test]
async fn cleanup_deadline_retains_a_blocked_save_until_capacity_reopens()
-> DesktopGameSupervisorTestResult {
    use chess::capacity::phase::CapacityPhase;

    let fixture = DesktopGameSupervisorFixtureStorage::new(CapacityPhase::MINIMUM_WORKERS).await?;
    let (mut supervisor, port) = fixture.supervisor().await?;
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    let failure = supervisor.shutdown_retained(Instant::now()).await;
    assert!(matches!(
        failure,
        Err(DesktopGameSupervisorError::Deadline(_))
    ));
    assert!(!supervisor.is_shutdown_complete());
    assert!(matches!(
        port.try_submit(DesktopGameSupervisorFixture::request(
            GameRevision::ZERO,
            0,
            DesktopGameCommandsEnumAction::Start,
        )),
        Err(DesktopGameSupervisorError::Closed)
    ));
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    supervisor
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(supervisor.is_shutdown_complete());
    drop((supervisor, port, phase));
    fixture.close().await
}

#[tokio::test]
async fn successful_retry_save_clears_the_authoritative_and_response_snapshot_gate()
-> DesktopGameSupervisorTestResult {
    let fixture = DesktopGameSupervisorFixtureStorage::new(2).await?;
    let (mut supervisor, port) = fixture.supervisor().await?;
    let mut events = port.events();
    let worker = tokio::spawn(async move {
        let result = supervisor.run().await;
        (supervisor, result)
    });
    let started = port
        .try_submit(DesktopGameSupervisorFixture::request(
            GameRevision::ZERO,
            0,
            DesktopGameCommandsEnumAction::Start,
        ))?
        .wait()
        .await?;
    fixture.set_save_failure(true).await?;
    let failed = port
        .try_submit(DesktopGameSupervisorFixture::request(
            started.revision(),
            0,
            DesktopGameCommandsEnumAction::Play {
                node: "83b312f6-34db-455e-be3b-c640a7a2bf08".parse()?,
                origin: "e2".parse()?,
                destination: "e4".parse()?,
                promotion: None,
                color: BoardEnumColor::White,
            },
        ))?
        .wait()
        .await;
    assert!(failed.is_err());
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if events.borrow().snapshot.is_save_required() {
                break;
            }
            events.changed().await?;
        }
        Ok::<_, tokio::sync::watch::error::RecvError>(())
    })
    .await??;
    let interrupted = events.borrow().snapshot.clone();
    assert_eq!(interrupted.phase(), GameEnumPhase::Interrupted);
    fixture.set_save_failure(false).await?;
    let saved = port
        .try_submit(DesktopGameSupervisorFixture::request(
            interrupted.revision(),
            0,
            DesktopGameCommandsEnumAction::RetrySave,
        ))?
        .wait()
        .await?;
    assert_eq!(saved.revision(), interrupted.revision());
    assert!(!saved.is_save_required());
    assert!(!port.snapshot().snapshot.is_save_required());
    assert!(matches!(
        port.snapshot().persistence,
        chess::desktop::game_supervisor::DesktopGameSupervisorEnumPersistence::Current
    ));
    let resumed = port
        .try_submit(DesktopGameSupervisorFixture::request(
            saved.revision(),
            0,
            DesktopGameCommandsEnumAction::Resume,
        ))?
        .wait()
        .await?;
    assert_eq!(resumed.phase(), GameEnumPhase::Active);
    port.try_submit(DesktopGameSupervisorFixture::request(
        resumed.revision(),
        0,
        DesktopGameCommandsEnumAction::Quit,
    ))?
    .wait()
    .await?;
    let (supervisor, result) = worker.await?;
    result?;
    assert!(supervisor.is_shutdown_complete());
    drop((supervisor, port, events));
    fixture.close().await
}
