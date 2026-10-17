use std::future::pending;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Instant as StandardInstant;

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use chess::clock::state::{ClockState, ClockStateEnumStatus};
use chess::clock::time_control::ClockTimeControl;
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use chess::desktop::game_supervisor::{
    DesktopGameSupervisor, DesktopGameSupervisorEngineFactory, DesktopGameSupervisorEngineProvider,
    DesktopGameSupervisorOptions,
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
use chess::game::pause::GamePauseEnumCause;
use chess::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
use chess::game::storage_repository::{GameStorageRepository, GameStorageRevision};
use chess::storage::game_document::StorageGameDocumentRepository;
use tokio::time::Instant;

use crate::desktop_game_supervisor_storage::DesktopGameSupervisorFixtureStorage;

type DesktopRecoveryTestResult = Result<(), Box<dyn std::error::Error>>;

const HOUR_NANOS: u64 = 3_600_000_000_000;

#[derive(Default)]
struct DesktopRecoveryFixtureClock(AtomicU64);

impl ClockTimeProvider for DesktopRecoveryFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.0.load(Ordering::SeqCst)))
    }
}

struct DesktopRecoveryFixtureProof;

impl GameAdjudicationMateProofProvider for DesktopRecoveryFixtureProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

struct DesktopRecoveryFixtureEngine;

impl GameEngineProvider for DesktopRecoveryFixtureEngine {
    type Error = std::io::Error;
    fn is_idle(&self) -> bool {
        true
    }
    async fn start(
        &mut self,
        _request: GameEngineRequest,
    ) -> Result<GameEngineSearchIdentity, Self::Error> {
        Err(std::io::Error::other("A local game attempted a search"))
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
        Err(std::io::Error::other("A local game attempted a stop"))
    }
    async fn quiesce(
        &mut self,
        _expected: GameEngineContext,
        _received_at: StandardInstant,
    ) -> Result<GameEngineQuiescenceReceipt, Self::Error> {
        Err(std::io::Error::other("A local game attempted quiescence"))
    }
}

impl DesktopGameSupervisorEngineProvider for DesktopRecoveryFixtureEngine {
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

struct DesktopRecoveryFixtureFactory;

impl DesktopGameSupervisorEngineFactory for DesktopRecoveryFixtureFactory {
    type Engine = DesktopRecoveryFixtureEngine;
    type Error = std::io::Error;
    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        Err(std::io::Error::other("A local game created an engine"))
    }
    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct DesktopRecoveryFixture;

impl DesktopRecoveryFixture {
    fn session() -> Result<GameSession, Box<dyn std::error::Error>> {
        Ok(GameSession::new(GameSessionOptions {
            id: "2e21a5df-ec1f-4199-9a13-7af9b9e1c6bd".parse()?,
            root: "932a32bb-6195-4f55-8bff-e80777c98569".parse()?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            clock: ClockState::new(ClockTimeControl::initial_default(), BoardEnumColor::White),
            metadata: GameMetadata::default(),
        })?)
    }

    fn remaining(
        clock: &ClockState,
        now: u64,
    ) -> Result<[Option<ClockDuration>; 2], Box<dyn std::error::Error>> {
        let view = clock.view(ClockInstant::from_nanos(now))?;
        Ok(
            [BoardEnumColor::White, BoardEnumColor::Black]
                .map(|color| view.side(color).remaining()),
        )
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

#[tokio::test]
async fn computer_sleep_pause_survives_restart_without_charging_the_sleep_or_orphaning_work()
-> DesktopRecoveryTestResult {
    let fixture = DesktopGameSupervisorFixtureStorage::new(2).await?;
    let clock = Arc::new(DesktopRecoveryFixtureClock::default());
    let session = DesktopRecoveryFixture::session()?;
    let game = session.id();
    let (mut supervisor, port) = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
        session,
        storage_revision: GameStorageRevision::INITIAL,
        storage: fixture.store.client(),
        executor: Arc::clone(&fixture.executor),
        clock: Arc::clone(&clock),
        wall_time: crate::game_storage_wall_time_fixture::GameStorageWallTimeFixture::shared(),
        proof: Arc::new(DesktopRecoveryFixtureProof),
        factory: DesktopRecoveryFixtureFactory,
        search_config: GameEngineSearchConfig::initial_default(),
    })
    .await?;
    let worker = tokio::spawn(async move {
        let result = supervisor.run().await;
        (supervisor, result)
    });
    let started = port
        .try_submit(DesktopRecoveryFixture::request(
            GameRevision::ZERO,
            0,
            DesktopGameCommandsEnumAction::Start,
        ))?
        .wait()
        .await?;
    clock.0.store(500_000_000, Ordering::SeqCst);
    let moved = port
        .try_submit(DesktopRecoveryFixture::request(
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
    clock.0.store(1_000_000_000, Ordering::SeqCst);
    let paused = port
        .try_submit(DesktopRecoveryFixture::request(
            moved.revision(),
            1_000_000_000,
            DesktopGameCommandsEnumAction::Pause {
                cause: GamePauseEnumCause::ComputerSleep,
            },
        ))?
        .wait()
        .await?;
    assert_eq!(paused.clock().status(), ClockStateEnumStatus::Paused);
    let remaining_at_sleep = DesktopRecoveryFixture::remaining(paused.clock(), 1_000_000_000)?;
    let woke = 10 * HOUR_NANOS;
    clock.0.store(woke, Ordering::SeqCst);
    let after_sleep = port
        .try_submit(DesktopRecoveryFixture::request(
            paused.revision(),
            woke,
            DesktopGameCommandsEnumAction::Quit,
        ))?
        .wait()
        .await?;
    assert_eq!(after_sleep.phase(), GameEnumPhase::Interrupted);
    let remaining_after_sleep = DesktopRecoveryFixture::remaining(after_sleep.clock(), woke)?;
    assert_eq!(remaining_after_sleep, remaining_at_sleep);
    let (supervisor, result) = worker.await?;
    result?;
    assert!(supervisor.is_shutdown_complete());
    drop((supervisor, port));

    let repository = StorageGameDocumentRepository::new(fixture.store.client(), &fixture.executor);
    let recovered = repository.load(game).await?.ok_or("missing saved game")?;
    assert!(!recovered.session().clock().is_running());
    let restored_view = recovered
        .session()
        .clock()
        .view(ClockInstant::from_nanos(woke))?;
    assert_eq!(restored_view.status(), ClockStateEnumStatus::Paused);
    assert_eq!(
        [BoardEnumColor::White, BoardEnumColor::Black]
            .map(|color| restored_view.side(color).remaining()),
        remaining_at_sleep
    );
    let storage_revision = recovered.storage_revision();

    let (mut restarted, restarted_port) =
        DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
            session: recovered.into_session(),
            storage_revision,
            storage: fixture.store.client(),
            executor: Arc::clone(&fixture.executor),
            clock: Arc::clone(&clock),
            wall_time: crate::game_storage_wall_time_fixture::GameStorageWallTimeFixture::shared(),
            proof: Arc::new(DesktopRecoveryFixtureProof),
            factory: DesktopRecoveryFixtureFactory,
            search_config: GameEngineSearchConfig::initial_default(),
        })
        .await?;
    let worker = tokio::spawn(async move {
        let result = restarted.run().await;
        (restarted, result)
    });
    let snapshot = restarted_port.snapshot().snapshot.clone();
    assert_eq!(snapshot.clock().status(), ClockStateEnumStatus::Paused);
    let resumed = restarted_port
        .try_submit(DesktopRecoveryFixture::request(
            snapshot.revision(),
            woke,
            DesktopGameCommandsEnumAction::Resume,
        ))?
        .wait()
        .await?;
    assert_eq!(resumed.clock().status(), ClockStateEnumStatus::Running);
    assert_eq!(
        DesktopRecoveryFixture::remaining(resumed.clock(), woke)?,
        remaining_at_sleep
    );
    let stopped = restarted_port
        .try_submit(DesktopRecoveryFixture::request(
            resumed.revision(),
            woke,
            DesktopGameCommandsEnumAction::Quit,
        ))?
        .wait()
        .await?;
    assert_eq!(stopped.phase(), GameEnumPhase::Interrupted);
    let (restarted, result) = worker.await?;
    result?;
    assert!(restarted.is_shutdown_complete());
    drop((restarted, restarted_port, repository));
    fixture.close().await
}
