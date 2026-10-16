use std::path::PathBuf;
use std::sync::Arc;

use chess::clock::duration::ClockInstant;
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::dto::{DesktopEnumEvent, DesktopEnumValue};
use chess::desktop::game_bridge::{
    DesktopGameBridgeEnumOperation, DesktopGameBridgeFuture, DesktopGameBridgeProvider,
    DesktopGameBridgeRequest,
};
use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
use chess::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryFactoryProvider, DesktopGameRegistryOptions,
};
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::desktop::game_supervisor::DesktopGameSupervisorEngineFactory;
use chess::desktop::identity::DesktopRevision;
use chess::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEnumResult};
use chess::engine::live_game::EngineLiveGame;
use chess::engine::process_provider::EngineNativeProcess;
use chess::game::configuration::GameConfiguration;
use chess::game::engine_provider::GameEngineContext;
use chess::game::enums::GameId;
use chess::game::pause::{GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseRequest};
use rand::{SeedableRng, rngs::StdRng};
use tokio::sync::{Notify, Semaphore, watch};
use tokio::time::Instant;

use crate::desktop_coordinator::{DesktopCoordinatorFixture, DesktopCoordinatorTestResult};
use crate::desktop_game_registry_fixture::{
    DesktopGameRegistryFixtureClock, DesktopGameRegistryFixtureProof,
};
use crate::desktop_game_supervisor_storage::DesktopGameSupervisorFixtureStorage;
use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

struct DesktopCoordinatorGamesState {
    snapshots: watch::Sender<Vec<DesktopGameSnapshot>>,
    paused: DesktopGameSnapshot,
    started: Semaphore,
    release: Semaphore,
}

struct DesktopCoordinatorGamesPendingFactory(Arc<Notify>);

impl DesktopGameRegistryFactoryProvider for DesktopCoordinatorGamesPendingFactory {
    type Factory = Self;
    type Error = std::io::Error;

    fn factory(&mut self, _game: GameId) -> Result<Self::Factory, Self::Error> {
        Ok(Self(Arc::clone(&self.0)))
    }
}

impl DesktopGameSupervisorEngineFactory for DesktopCoordinatorGamesPendingFactory {
    type Engine = EngineLiveGame<EngineNativeProcess, DesktopGameRegistryFixtureClock>;
    type Error = std::io::Error;

    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        self.0.notify_one();
        std::future::pending().await
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn quit_reaches_lifecycle_after_preparation_cancels_pending_engine_creation()
-> DesktopCoordinatorTestResult {
    use std::time::Duration;

    use chess::capacity::phase::CapacityPhase;

    let storage = DesktopGameSupervisorFixtureStorage::new(CapacityPhase::MINIMUM_WORKERS).await?;
    let mut fixture = DesktopCoordinatorFixture::new(PathBuf::new())?;
    let entered = Arc::new(Notify::new());
    let (mut registry, registry_port) = DesktopGameRegistry::new(DesktopGameRegistryOptions {
        factory_provider: DesktopCoordinatorGamesPendingFactory(Arc::clone(&entered)),
        storage: storage.store.client(),
        executor: Arc::clone(&storage.executor),
        clock: Arc::new(DesktopGameRegistryFixtureClock::default()),
        proof: Arc::new(DesktopGameRegistryFixtureProof::default()),
        random: StdRng::seed_from_u64(81),
    });
    let registry_worker = tokio::spawn(async move {
        let result = registry.run().await;
        (registry, result)
    });
    fixture.coordinator = fixture
        .coordinator
        .with_games(Arc::new(registry_port.clone()));
    let operation = DesktopGameBridgeEnumOperation::Create {
        owner: chess::desktop::messages::DesktopMessagesEntityId::new(),
        configuration: GameConfiguration::initial_default(),
    };
    let creation = fixture
        .port
        .try_submit(&fixture.port.request(DesktopCommand::new(
            DesktopEnumCommand::Game(operation),
            Some(DesktopRevision::ZERO),
        ))?)?;
    let mut coordinator = fixture.coordinator;
    let coordinator_worker = tokio::spawn(async move {
        let result = coordinator.run().await;
        (coordinator, result)
    });
    entered.notified().await;
    let mut phase = CapacityPhase::new(storage.budget.clone())?;
    let generation = phase.request_preparation()?;
    registry_port
        .quiesce_all(generation, Instant::now() + Duration::from_secs(3))?
        .wait()
        .await?;
    let quit = fixture.port.try_submit(
        &fixture
            .port
            .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
    )?;
    let reached_lifecycle =
        tokio::time::timeout(Duration::from_secs(3), fixture.shutdown.started.notified()).await;
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    phase.complete_preparation(generation)?;
    registry_port
        .shutdown_all(Instant::now() + Duration::from_secs(3))?
        .wait()
        .await?;
    fixture.shutdown.allow.notify_one();
    let (coordinator, result) = coordinator_worker.await?;
    result?;
    let (registry, result) = registry_worker.await?;
    result?;
    assert!(creation.response().is_some());
    assert!(matches!(
        quit.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    drop((coordinator, registry, registry_port, phase));
    storage.close().await?;
    reached_lifecycle?;
    Ok(())
}

struct DesktopCoordinatorGamesProvider(Arc<DesktopCoordinatorGamesState>);

impl DesktopCoordinatorGamesProvider {
    fn new() -> DesktopCoordinatorTestResult<Self> {
        let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
        let initial = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
        session.pause(
            GamePauseRequest {
                expected_revision: session.revision(),
                cause: GamePauseEnumCause::Manual,
                received_at: ClockInstant::from_nanos(0),
                engine: GamePauseEnumEngineBarrier::NoEngine,
            },
            &GameTransitionFixtureProof,
        )?;
        let paused = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
        let (snapshots, _) = watch::channel(vec![initial]);
        Ok(Self(Arc::new(DesktopCoordinatorGamesState {
            snapshots,
            paused,
            started: Semaphore::new(0),
            release: Semaphore::new(0),
        })))
    }
}

impl DesktopGameBridgeProvider for DesktopCoordinatorGamesProvider {
    fn execute(&self, request: DesktopGameBridgeRequest) -> DesktopGameBridgeFuture {
        let state = Arc::clone(&self.0);
        let is_control = matches!(
            request.operation,
            DesktopGameBridgeEnumOperation::Session {
                action: DesktopGameCommandsEnumAction::Pause { .. },
                ..
            }
        );
        Box::pin(async move {
            state.started.add_permits(1);
            if !is_control {
                state
                    .release
                    .acquire()
                    .await
                    .map_err(|source| {
                        chess::desktop::error::DesktopFailure::new(
                            chess::desktop::error::DesktopEnumFailureCode::ResourceRefusal,
                            source.to_string(),
                            false,
                        )
                    })?
                    .forget();
            }
            Ok(state.paused.clone())
        })
    }

    fn snapshots(&self) -> watch::Receiver<Vec<DesktopGameSnapshot>> {
        self.0.snapshots.subscribe()
    }
}

#[tokio::test]
async fn subscriptions_and_resynchronization_include_existing_game_snapshots()
-> DesktopCoordinatorTestResult {
    let mut fixture = DesktopCoordinatorFixture::new(PathBuf::new())?;
    let games = Arc::new(DesktopCoordinatorGamesProvider::new()?);
    let expected = games.0.snapshots.borrow()[0].clone();
    fixture.coordinator = fixture.coordinator.with_games(games);
    let subscribed = fixture.submit(DesktopEnumCommand::StreamSubscribe)?;
    let mut coordinator = fixture.coordinator;
    let task = tokio::spawn(async move {
        let result = coordinator.run().await;
        (coordinator, result)
    });
    let DesktopMessagesEnumResult::Value {
        value: DesktopEnumValue::StreamSubscribe {
            subscription_id, ..
        },
        ..
    } = DesktopCoordinatorFixture::response(&fixture.port, &fixture.wake, &subscribed).await
    else {
        return Err("missing game subscription".into());
    };
    let initial = fixture
        .port
        .try_event()?
        .ok_or("missing initial game snapshots")?;
    assert!(
        matches!(initial.payload(), DesktopEnumEvent::Games(snapshots) if snapshots.len() == 1 && snapshots[0].id() == expected.id())
    );
    let resync = fixture
        .port
        .try_submit(&fixture.port.request(DesktopCommand::new(
            DesktopEnumCommand::StreamResync { subscription_id },
            None,
        ))?)?;
    let DesktopMessagesEnumResult::Value {
        value: DesktopEnumValue::StreamResync { sequence, .. },
        ..
    } = DesktopCoordinatorFixture::response(&fixture.port, &fixture.wake, &resync).await
    else {
        return Err("missing game resynchronization".into());
    };
    let bootstrap = fixture
        .port
        .try_event()?
        .ok_or("missing resynchronized bootstrap")?;
    assert_eq!(bootstrap.sequence(), sequence);
    let restored = fixture
        .port
        .try_event()?
        .ok_or("missing resynchronized games")?;
    assert_eq!(restored.sequence(), sequence.next()?);
    assert!(
        matches!(restored.payload(), DesktopEnumEvent::Games(snapshots) if snapshots.len() == 1 && snapshots[0].revision() == expected.revision())
    );
    fixture.port.try_submit(
        &fixture
            .port
            .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
    )?;
    fixture.shutdown.started.notified().await;
    fixture.shutdown.allow.notify_one();
    let (_coordinator, result) = task.await?;
    result?;
    Ok(())
}

#[tokio::test]
async fn idle_game_pause_uses_reserved_admission_and_returns_its_authoritative_state()
-> DesktopCoordinatorTestResult {
    let mut fixture = DesktopCoordinatorFixture::new(PathBuf::new())?;
    let games = Arc::new(DesktopCoordinatorGamesProvider::new()?);
    let initial = games.0.snapshots.borrow()[0].clone();
    let expected = games.0.paused.clone();
    fixture.coordinator = fixture.coordinator.with_games(games);
    let operation = DesktopGameBridgeEnumOperation::Session {
        game: initial.id(),
        revision: initial.revision(),
        action: DesktopGameCommandsEnumAction::Pause {
            cause: GamePauseEnumCause::Manual,
        },
    };
    let command = DesktopEnumCommand::Game(operation.clone());
    assert!(command.is_control());
    let receipt = fixture.port.try_submit(
        &fixture
            .port
            .request(DesktopCommand::new(command, Some(operation.revision())))?,
    )?;
    let mut coordinator = fixture.coordinator;
    let task = tokio::spawn(async move {
        let result = coordinator.run().await;
        (coordinator, result)
    });
    let response =
        DesktopCoordinatorFixture::response(&fixture.port, &fixture.wake, &receipt).await;
    assert!(
        matches!(response, DesktopMessagesEnumResult::Value { revision: Some(revision), value: DesktopEnumValue::Game(snapshot) } if snapshot.id() == expected.id() && revision == DesktopRevision::for_game(expected.revision()) && snapshot.phase() == expected.phase())
    );
    fixture.port.try_submit(
        &fixture
            .port
            .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
    )?;
    fixture.shutdown.started.notified().await;
    fixture.shutdown.allow.notify_one();
    let (_coordinator, result) = task.await?;
    result?;
    Ok(())
}

#[tokio::test]
async fn quit_observes_a_committing_game_response_before_closing_services()
-> DesktopCoordinatorTestResult {
    let mut fixture = DesktopCoordinatorFixture::new(PathBuf::new())?;
    let games = Arc::new(DesktopCoordinatorGamesProvider::new()?);
    let initial = games.0.snapshots.borrow()[0].clone();
    fixture.coordinator = fixture
        .coordinator
        .with_games(Arc::clone(&games) as Arc<dyn DesktopGameBridgeProvider>);
    let operation = DesktopGameBridgeEnumOperation::Session {
        game: initial.id(),
        revision: initial.revision(),
        action: DesktopGameCommandsEnumAction::Navigate {
            node: initial.root(),
        },
    };
    let mutation = fixture
        .port
        .try_submit(&fixture.port.request(DesktopCommand::new(
            DesktopEnumCommand::Game(operation.clone()),
            Some(operation.revision()),
        ))?)?;
    let mut coordinator = fixture.coordinator;
    let task = tokio::spawn(async move {
        let result = coordinator.run().await;
        (coordinator, result)
    });
    games.0.started.acquire().await?.forget();
    let quit = fixture.port.try_submit(
        &fixture
            .port
            .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
    )?;
    assert!(mutation.response().is_none());
    assert!(quit.response().is_none());
    games.0.release.add_permits(1);
    fixture.shutdown.started.notified().await;
    assert!(matches!(
        mutation.response(),
        Some(DesktopMessagesEnumResult::Value {
            value: DesktopEnumValue::Game(_),
            ..
        })
    ));
    fixture.shutdown.allow.notify_one();
    let (_coordinator, result) = task.await?;
    result?;
    assert!(matches!(
        quit.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    Ok(())
}
