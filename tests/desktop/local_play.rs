use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant as StandardInstant};

use bevy_egui::egui::{self, Context, RawInput, Rect, accesskit};
use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::ClockStateEnumStatus;
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::application::{
    DesktopApplication, DesktopApplicationPort, DesktopApplicationReceipt,
    DesktopApplicationWakeError, DesktopApplicationWakeProvider,
};
use chess::desktop::coordinator::{
    DesktopCoordinator, DesktopCoordinatorDialogProvider, DesktopCoordinatorError,
    DesktopCoordinatorLifecycleProvider, DesktopCoordinatorServices,
};
use chess::desktop::dto::{DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot};
use chess::desktop::error::DesktopEnumFailureCode;
use chess::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileHandleError,
};
use chess::desktop::game_bridge::DesktopGameBridgeEnumOperation;
use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
use chess::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryError, DesktopGameRegistryFactoryProvider,
    DesktopGameRegistryOptions, DesktopGameRegistryPort,
};
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::desktop::game_supervisor::{
    DesktopGameSupervisorEngineFactory, DesktopGameSupervisorEngineProvider,
};
use chess::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEnumResult};
use chess::game::engine_provider::{
    GameEngineContext, GameEngineEnumEvent, GameEngineProvider, GameEngineQuiescenceReceipt,
    GameEngineRequest, GameEngineSearchIdentity, GameEngineStopReceipt,
};
use chess::game::enums::{GameEnumMode, GameEnumPhase, GameEnumSeat, GameId, GameRevision};
use chess::game::session::GameSession;
use chess::game::storage_repository::GameStorageRepository;
use chess::game::takeback::{GameTakebackEnumConsent, GameTakebackEnumResponse};
use chess::presentation::action::registry::PresentationActionEnumId;
use chess::presentation::app::PresentationApp;
use chess::presentation::board::scene::PresentationBoardScene;
use chess::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use chess::scene::camera::SceneCamera;
use chess::scene::enums::SceneEnumLifecycle;
use chess::scene::renderer::SceneRendererGeneration;
use chess::storage::game_document::StorageGameDocumentRepository;
use rand::{SeedableRng, rngs::StdRng};
use tokio::sync::Notify;
use tokio::task::{JoinError, JoinHandle};
use tokio::time::{Instant, timeout};

use crate::desktop_game_registry_fixture::{
    DesktopGameRegistryFixtureClock, DesktopGameRegistryFixtureProof,
    DesktopGameRegistryFixtureRuntime,
};

type DesktopLocalPlayTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, thiserror::Error)]
enum DesktopLocalPlayEngineError {
    #[error("Local play attempted engine creation")]
    Create,
    #[error("Local play attempted an engine search")]
    Start,
    #[error("Local play attempted to read an engine event")]
    Event,
    #[error("Local play attempted an engine stop")]
    Stop,
    #[error("Local play attempted engine quiescence")]
    Quiesce,
    #[error("Local play attempted an engine resource refresh")]
    Refresh,
    #[error("Local play attempted engine shutdown")]
    Shutdown,
}

#[derive(Default)]
struct DesktopLocalPlayEngineCounters {
    wrappers: AtomicU64,
    creates: AtomicU64,
    starts: AtomicU64,
    events: AtomicU64,
    stops: AtomicU64,
    quiesces: AtomicU64,
    refreshes: AtomicU64,
    shutdowns: AtomicU64,
}

impl DesktopLocalPlayEngineCounters {
    fn assert_local_only(&self) {
        assert_eq!(self.wrappers.load(Ordering::SeqCst), 1);
        for count in [
            &self.creates,
            &self.starts,
            &self.events,
            &self.stops,
            &self.quiesces,
            &self.refreshes,
            &self.shutdowns,
        ] {
            assert_eq!(count.load(Ordering::SeqCst), 0);
        }
    }
}

struct DesktopLocalPlayEngine(Arc<DesktopLocalPlayEngineCounters>);

impl GameEngineProvider for DesktopLocalPlayEngine {
    type Error = DesktopLocalPlayEngineError;

    fn is_idle(&self) -> bool {
        true
    }

    async fn start(
        &mut self,
        _request: GameEngineRequest,
    ) -> Result<GameEngineSearchIdentity, Self::Error> {
        self.0.starts.fetch_add(1, Ordering::SeqCst);
        Err(DesktopLocalPlayEngineError::Start)
    }

    async fn next_event(
        &mut self,
        _expected: GameEngineContext,
    ) -> Result<GameEngineEnumEvent, Self::Error> {
        self.0.events.fetch_add(1, Ordering::SeqCst);
        Err(DesktopLocalPlayEngineError::Event)
    }

    async fn stop(
        &mut self,
        _identity: GameEngineSearchIdentity,
        _received_at: StandardInstant,
    ) -> Result<GameEngineStopReceipt, Self::Error> {
        self.0.stops.fetch_add(1, Ordering::SeqCst);
        Err(DesktopLocalPlayEngineError::Stop)
    }

    async fn quiesce(
        &mut self,
        _expected: GameEngineContext,
        _received_at: StandardInstant,
    ) -> Result<GameEngineQuiescenceReceipt, Self::Error> {
        self.0.quiesces.fetch_add(1, Ordering::SeqCst);
        Err(DesktopLocalPlayEngineError::Quiesce)
    }
}

impl DesktopGameSupervisorEngineProvider for DesktopLocalPlayEngine {
    fn is_shutdown_complete(&self) -> bool {
        true
    }

    async fn refresh_resources(&mut self) -> Result<(), Self::Error> {
        self.0.refreshes.fetch_add(1, Ordering::SeqCst);
        Err(DesktopLocalPlayEngineError::Refresh)
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        self.0.shutdowns.fetch_add(1, Ordering::SeqCst);
        Err(DesktopLocalPlayEngineError::Shutdown)
    }
}

struct DesktopLocalPlayFactory(Arc<DesktopLocalPlayEngineCounters>);

impl DesktopGameSupervisorEngineFactory for DesktopLocalPlayFactory {
    type Engine = DesktopLocalPlayEngine;
    type Error = DesktopLocalPlayEngineError;

    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        self.0.creates.fetch_add(1, Ordering::SeqCst);
        Err(DesktopLocalPlayEngineError::Create)
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct DesktopLocalPlayFactories(Arc<DesktopLocalPlayEngineCounters>);

impl DesktopGameRegistryFactoryProvider for DesktopLocalPlayFactories {
    type Factory = DesktopLocalPlayFactory;
    type Error = DesktopLocalPlayEngineError;

    fn factory(&mut self, _game: GameId) -> Result<Self::Factory, Self::Error> {
        self.0.wrappers.fetch_add(1, Ordering::SeqCst);
        Ok(DesktopLocalPlayFactory(Arc::clone(&self.0)))
    }
}

#[derive(Default)]
struct DesktopLocalPlayWake(Notify);

impl DesktopApplicationWakeProvider for DesktopLocalPlayWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        self.0.notify_one();
        Ok(())
    }
}

struct DesktopLocalPlayDialogs;

impl DesktopFileDialogProvider for DesktopLocalPlayDialogs {
    async fn select(
        &self,
        _purpose: DesktopFileEnumPurpose,
    ) -> Result<Option<PathBuf>, DesktopFileHandleError> {
        Err(DesktopFileHandleError::Io(std::io::Error::other(
            "Local play unexpectedly opened a file dialog",
        )))
    }
}

impl DesktopCoordinatorDialogProvider for DesktopLocalPlayDialogs {
    fn request_cancel(&self) -> Result<(), DesktopFileHandleError> {
        Ok(())
    }
}

struct DesktopLocalPlayLifecycle {
    registry: DesktopGameRegistryPort,
    completed: Arc<AtomicU64>,
}

impl DesktopCoordinatorLifecycleProvider for DesktopLocalPlayLifecycle {
    async fn shutdown(&mut self, deadline: Instant) -> Result<(), DesktopCoordinatorError> {
        self.registry
            .shutdown_all(deadline)
            .map_err(|source| DesktopCoordinatorError::Shutdown {
                source: Box::new(source),
            })?
            .wait()
            .await
            .map_err(|source| DesktopCoordinatorError::Shutdown {
                source: Box::new(source),
            })?;
        self.completed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

type DesktopLocalPlayRegistry = DesktopGameRegistry<
    DesktopLocalPlayFactories,
    DesktopGameRegistryFixtureClock,
    DesktopGameRegistryFixtureProof,
    StdRng,
>;
type DesktopLocalPlayCoordinator = DesktopCoordinator<
    DesktopGameRegistryFixtureClock,
    DesktopLocalPlayWake,
    DesktopLocalPlayDialogs,
    DesktopLocalPlayLifecycle,
>;

struct DesktopLocalPlayTask<T>(Option<JoinHandle<T>>);

impl<T> DesktopLocalPlayTask<T> {
    async fn wait(&mut self) -> DesktopLocalPlayTestResult<T> {
        let task = self.0.as_mut().ok_or("local-play task already observed")?;
        let result = timeout(DesktopLocalPlayFixture::WAIT, task).await?;
        self.0 = None;
        Ok(result?)
    }

    async fn cancel_remaining(&mut self) -> Result<(), JoinError> {
        let Some(task) = self.0.take() else {
            return Ok(());
        };
        task.abort();
        task.await.map(drop)
    }
}

#[derive(Debug, thiserror::Error)]
#[error(
    "Local-play cleanup failed: {operation}; coordinator cancellation: {coordinator:?}; registry cancellation: {registry:?}; storage cleanup: {storage:?}"
)]
struct DesktopLocalPlayCleanupError {
    #[source]
    operation: Box<dyn std::error::Error>,
    coordinator: Option<JoinError>,
    registry: Option<JoinError>,
    storage: Option<Box<dyn std::error::Error>>,
}

#[derive(Debug, thiserror::Error)]
#[error("Local-play journey failed: {journey}; cleanup also failed: {cleanup}")]
struct DesktopLocalPlayJourneyError {
    #[source]
    journey: Box<dyn std::error::Error>,
    cleanup: Box<dyn std::error::Error>,
}

struct DesktopLocalPlayFixture {
    runtime: DesktopGameRegistryFixtureRuntime,
    context: Context,
    presentation: PresentationApp,
    scene: Option<PresentationBoardScene>,
    port: DesktopApplicationPort<DesktopGameRegistryFixtureClock, DesktopLocalPlayWake>,
    wake: Arc<DesktopLocalPlayWake>,
    counters: Arc<DesktopLocalPlayEngineCounters>,
    shutdowns: Arc<AtomicU64>,
    receipts: Vec<DesktopApplicationReceipt>,
    coordinator: DesktopLocalPlayTask<(
        DesktopLocalPlayCoordinator,
        Result<(), DesktopCoordinatorError>,
    )>,
    registry: DesktopLocalPlayTask<(
        DesktopLocalPlayRegistry,
        Result<(), DesktopGameRegistryError>,
    )>,
}

impl DesktopLocalPlayFixture {
    const WAIT: Duration = Duration::from_secs(5);

    fn finish(
        journey: DesktopLocalPlayTestResult,
        cleanup: DesktopLocalPlayTestResult,
    ) -> DesktopLocalPlayTestResult {
        match (journey, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(journey), Err(cleanup)) => {
                Err(Box::new(DesktopLocalPlayJourneyError { journey, cleanup }))
            }
        }
    }

    async fn new() -> DesktopLocalPlayTestResult<Self> {
        let runtime =
            DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default())
                .await?;
        let counters = Arc::new(DesktopLocalPlayEngineCounters::default());
        let (mut registry, registry_port) = DesktopGameRegistry::new(DesktopGameRegistryOptions {
            factory_provider: DesktopLocalPlayFactories(Arc::clone(&counters)),
            storage: runtime.store.client(),
            executor: Arc::clone(&runtime.executor),
            clock: Arc::clone(&runtime.clock),
            proof: Arc::new(DesktopGameRegistryFixtureProof::default()),
            random: StdRng::seed_from_u64(96),
        });
        let wake = Arc::new(DesktopLocalPlayWake::default());
        let application = DesktopApplication::new(Arc::clone(&runtime.clock), Arc::clone(&wake));
        let port = application.port();
        let shutdowns = Arc::new(AtomicU64::new(0));
        let mut coordinator = DesktopCoordinator::new(
            application,
            Arc::clone(&runtime.clock),
            DesktopBootstrapSnapshot::new(
                &BoardPosition::standard(),
                NonZeroU32::new(u32::try_from(runtime.budget.workers())?)
                    .ok_or("invalid shared capacity")?,
                DesktopEnumEngineSnapshot::Unavailable {
                    message: "No engine is available for this local game".to_owned(),
                },
            ),
            DesktopCoordinatorServices::new(
                Arc::new(DesktopLocalPlayDialogs),
                Arc::clone(&runtime.executor),
                DesktopLocalPlayLifecycle {
                    registry: registry_port.clone(),
                    completed: Arc::clone(&shutdowns),
                },
            ),
        )
        .with_games(Arc::new(registry_port));
        let context = Context::default();
        context.enable_accesskit();
        Ok(Self {
            runtime,
            context,
            presentation: PresentationApp::new(),
            scene: None,
            port,
            wake,
            counters,
            shutdowns,
            receipts: Vec::new(),
            coordinator: DesktopLocalPlayTask(Some(tokio::spawn(async move {
                let result = coordinator.run().await;
                (coordinator, result)
            }))),
            registry: DesktopLocalPlayTask(Some(tokio::spawn(async move {
                let result = registry.run().await;
                (registry, result)
            }))),
        })
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> DesktopLocalPlayTestResult<egui::FullOutput> {
        let mut result = None;
        let mut output = self.context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                let mut picker = |_point| Ok(None);
                result =
                    Some(
                        self.presentation
                            .show(ui, &self.port, self.scene.as_ref(), &mut picker),
                    );
            },
        );
        output.textures_delta.clear();
        let frame = result.ok_or("missing mounted local-play frame")??;
        if self.presentation.game_snapshot().is_some()
            && let Some(viewport) = frame.viewport()
            && self
                .scene
                .as_ref()
                .is_none_or(|scene| scene.viewport() != viewport)
        {
            self.scene = Some(PresentationBoardScene::new(
                egui::TextureId::User(96),
                SceneRendererGeneration::initial(),
                SceneEnumLifecycle::Ready,
                viewport,
                SceneCamera::initial(BoardEnumColor::White).fit(viewport.physical_size())?,
            ));
        }
        Ok(output)
    }

    async fn settle(&mut self) -> DesktopLocalPlayTestResult {
        let wake = Arc::clone(&self.wake);
        timeout(Self::WAIT, async {
            loop {
                let notified = wake.0.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                self.port.acknowledge_wake();
                self.frame(Vec::new())?;
                if let Some(failure) = self.presentation.failure() {
                    return Err(failure.to_owned().into());
                }
                if !self.presentation.has_pending_work() {
                    return Ok(());
                }
                notified.await;
            }
        })
        .await?
    }

    fn action_event(target: accesskit::NodeId) -> egui::Event {
        egui::Event::AccessKitActionRequest(accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: target,
            data: None,
        })
    }

    async fn click(&mut self, label: &str) -> DesktopLocalPlayTestResult {
        self.frame(Vec::new())?;
        let output = self.frame(Vec::new())?;
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .ok_or("missing mounted AccessKit tree")?
            .nodes;
        let mut matching = nodes.iter().filter(|(_, node)| node.label() == Some(label));
        let (target, _) = matching
            .next()
            .ok_or_else(|| format!("missing mounted control: {label}"))?;
        assert!(
            matching.next().is_none(),
            "ambiguous mounted control: {label}"
        );
        self.frame(vec![Self::action_event(*target)])?;
        self.settle().await
    }

    fn snapshot(&self) -> DesktopLocalPlayTestResult<DesktopGameSnapshot> {
        self.presentation
            .game_snapshot()
            .cloned()
            .ok_or_else(|| "missing authoritative local-game snapshot".into())
    }

    fn advance_to(&self, millis: u64) -> DesktopLocalPlayTestResult {
        self.runtime
            .clock
            .advance_to(ClockInstant::from_millis(millis)?)?;
        Ok(())
    }

    async fn start(&mut self, consent: GameTakebackEnumConsent) -> DesktopLocalPlayTestResult {
        self.presentation
            .activate(PresentationActionEnumId::Refresh, 1, &self.port)?;
        self.settle().await?;
        self.click("New Game").await?;
        self.click("Local two-player").await?;
        if consent == GameTakebackEnumConsent::NotRequired {
            self.click("Require the other player's confirmation for takebacks")
                .await?;
        }
        assert!(self.presentation.game_snapshot().is_none());
        self.click("Start Game").await?;
        let snapshot = self.snapshot()?;
        assert_eq!(snapshot.phase(), GameEnumPhase::Active);
        assert_eq!(snapshot.seats().mode(), GameEnumMode::LocalTwoPlayer);
        for color in [BoardEnumColor::White, BoardEnumColor::Black] {
            assert_eq!(snapshot.seats().seat(color), GameEnumSeat::Human);
        }
        assert_eq!(snapshot.takeback_consent(), consent);
        self.counters.assert_local_only();
        Ok(())
    }

    async fn consent(&mut self, label: &str) -> DesktopLocalPlayTestResult {
        let before = self.snapshot()?;
        self.click(label).await?;
        assert_eq!(self.snapshot()?.revision(), before.revision().next()?);
        Ok(())
    }

    async fn play(&mut self, origin: &str, destination: &str) -> DesktopLocalPlayTestResult {
        self.frame(Vec::new())?;
        self.frame(Vec::new())?;
        let before = self.snapshot()?;
        for square in [origin, destination] {
            self.frame(vec![Self::action_event(
                PresentationBoardSemanticGrid::square_id(square.parse()?).accesskit_id(),
            )])?;
        }
        self.settle().await?;
        let after = self.snapshot()?;
        assert_eq!(after.revision(), before.revision().next()?);
        assert_eq!(after.moves().len(), before.moves().len() + 1);
        assert_eq!(after.live_turn(), before.live_turn().opposite());
        assert!(!after.is_save_required());
        self.counters.assert_local_only();
        Ok(())
    }

    async fn response(
        &self,
        receipt: &DesktopApplicationReceipt,
    ) -> DesktopLocalPlayTestResult<DesktopMessagesEnumResult> {
        timeout(Self::WAIT, async {
            loop {
                let notified = self.wake.0.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                self.port.acknowledge_wake();
                if let Some(response) = receipt.response() {
                    return response.clone();
                }
                notified.await;
            }
        })
        .await
        .map_err(Into::into)
    }

    async fn command(
        &mut self,
        revision: GameRevision,
        action: DesktopGameCommandsEnumAction,
    ) -> DesktopLocalPlayTestResult<DesktopMessagesEnumResult> {
        let operation = DesktopGameBridgeEnumOperation::Session {
            game: self.snapshot()?.id(),
            revision,
            action,
        };
        let request = self.port.request(DesktopCommand::new(
            DesktopEnumCommand::Game(operation.clone()),
            Some(operation.revision()),
        ))?;
        let receipt = self.port.try_submit(&request)?;
        assert_eq!(receipt.received_at(), self.runtime.clock.now()?);
        self.receipts.push(receipt.clone());
        self.response(&receipt).await
    }

    fn assert_clock(
        &self,
        white_millis: u64,
        black_millis: u64,
        active: BoardEnumColor,
    ) -> DesktopLocalPlayTestResult {
        let clock = self.snapshot()?.clock().view(self.runtime.clock.now()?)?;
        assert_eq!(clock.status(), ClockStateEnumStatus::Running);
        assert_eq!(clock.active(), active);
        for (color, millis) in [
            (BoardEnumColor::White, white_millis),
            (BoardEnumColor::Black, black_millis),
        ] {
            assert_eq!(
                clock.side(color).remaining(),
                Some(ClockDuration::from_millis(millis)?)
            );
        }
        Ok(())
    }

    async fn persisted(&self) -> DesktopLocalPlayTestResult<GameSession> {
        let snapshot = self.snapshot()?;
        let saved = timeout(
            Self::WAIT,
            StorageGameDocumentRepository::new(self.runtime.store.client(), &self.runtime.executor)
                .load(snapshot.id()),
        )
        .await??
        .ok_or("missing durable local game")?;
        let session = saved.into_session();
        assert_eq!(session.id(), snapshot.id());
        assert_eq!(session.revision(), snapshot.revision());
        assert_eq!(session.live(), snapshot.live());
        assert_eq!(session.view(), snapshot.view());
        assert_eq!(session.seats(), snapshot.seats());
        assert_eq!(session.takeback().consent(), snapshot.takeback_consent());
        assert!(!session.clock().is_running());
        assert_eq!(
            session.tree().node(session.live())?.position().fen(),
            snapshot.board().fen()
        );
        let now = self.runtime.clock.now()?;
        let restored = session.clock().view(now)?;
        let captured = snapshot.clock().view(snapshot.captured_at())?;
        assert_eq!(restored.active(), captured.active());
        assert_eq!(restored.elapsed(), captured.elapsed());
        for color in [BoardEnumColor::White, BoardEnumColor::Black] {
            assert_eq!(restored.side(color), captured.side(color));
        }
        Ok(session)
    }

    async fn shutdown(&mut self) -> DesktopLocalPlayTestResult {
        let quit = self.port.try_submit(
            &self
                .port
                .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
        )?;
        self.receipts.push(quit.clone());
        let response = self.response(&quit).await?;
        let coordinator_outcome = self.coordinator.wait().await;
        let registry_outcome = self.registry.wait().await;
        let (coordinator, coordinator_result) = coordinator_outcome?;
        let (registry, registry_result) = registry_outcome?;
        coordinator_result?;
        registry_result?;
        assert!(registry.is_shutdown_complete());
        assert!(matches!(response, DesktopMessagesEnumResult::QuitReady));
        assert_eq!(self.shutdowns.load(Ordering::SeqCst), 1);
        assert!(
            self.receipts
                .iter()
                .all(|receipt| receipt.response().is_some())
        );
        self.counters.assert_local_only();
        drop((coordinator, registry));
        Ok(())
    }

    fn close(
        mut self,
    ) -> std::pin::Pin<Box<impl std::future::Future<Output = DesktopLocalPlayTestResult>>> {
        Box::pin(async move {
            let outcome = self.shutdown().await;
            let coordinator = self.coordinator.cancel_remaining().await.err();
            let registry = self.registry.cancel_remaining().await.err();
            drop((self.presentation, self.port));
            let storage = self.runtime.close().await;
            match outcome {
                Ok(()) => storage,
                Err(operation) => Err(DesktopLocalPlayCleanupError {
                    operation,
                    coordinator,
                    registry,
                    storage: storage.err(),
                }
                .into()),
            }
        })
    }
}

#[tokio::test]
async fn mounted_local_game_routes_both_humans_and_persists_real_clocks_without_engine_work()
-> DesktopLocalPlayTestResult {
    let mut fixture = DesktopLocalPlayFixture::new().await?;
    let journey: DesktopLocalPlayTestResult = async {
        fixture.start(GameTakebackEnumConsent::Required).await?;
        fixture.assert_clock(600_000, 600_000, BoardEnumColor::White)?;
        fixture.advance_to(3_000)?;
        fixture.play("e2", "e4").await?;
        fixture.assert_clock(602_000, 600_000, BoardEnumColor::Black)?;
        let white = fixture.snapshot()?;
        let saved = fixture.persisted().await?;
        assert_eq!(saved.tree().len(), 2);
        fixture.advance_to(7_000)?;
        fixture.play("e7", "e5").await?;
        fixture.assert_clock(602_000, 601_000, BoardEnumColor::White)?;
        let black = fixture.snapshot()?;
        assert_eq!(black.moves()[0].san(), "e4");
        assert_eq!(black.moves()[1].san(), "e5");
        let saved = fixture.persisted().await?;
        assert_eq!(saved.tree().len(), 3);
        for (origin, destination, color, revision) in [
            ("e4", "e7", BoardEnumColor::White, black.revision()),
            ("b8", "c6", BoardEnumColor::Black, black.revision()),
            ("g1", "f3", BoardEnumColor::White, white.revision()),
        ] {
            let result = fixture
                .command(
                    revision,
                    DesktopGameCommandsEnumAction::Play {
                        node: black.live(),
                        origin: origin.parse()?,
                        destination: destination.parse()?,
                        promotion: None,
                        color,
                    },
                )
                .await?;
            let DesktopMessagesEnumResult::Failure(failure) = result else {
                return Err("invalid local move unexpectedly succeeded".into());
            };
            if revision == white.revision() {
                assert!(matches!(
                    failure.code(),
                    DesktopEnumFailureCode::StaleRevision
                ));
            }
            let unchanged = fixture.persisted().await?;
            assert_eq!(unchanged.tree().len(), saved.tree().len());
            assert_eq!(unchanged.live(), black.live());
        }
        fixture.assert_clock(602_000, 601_000, BoardEnumColor::White)?;
        Ok(())
    }
    .await;
    let cleanup = fixture.close().await;
    DesktopLocalPlayFixture::finish(journey, cleanup)
}

#[tokio::test]
async fn mounted_consent_accept_restores_one_ply_clocks_and_keeps_stable_variations()
-> DesktopLocalPlayTestResult {
    let mut fixture = DesktopLocalPlayFixture::new().await?;
    let journey: DesktopLocalPlayTestResult = async {
        fixture.start(GameTakebackEnumConsent::Required).await?;
        fixture.advance_to(3_000)?;
        fixture.play("e2", "e4").await?;
        let white = fixture.snapshot()?;
        fixture.advance_to(7_000)?;
        fixture.play("e7", "e5").await?;
        let black = fixture.snapshot()?;
        let saved = fixture.persisted().await?;
        let before = saved
            .tree()
            .node(black.live())?
            .before_clock()
            .ok_or("missing authoritative pre-move clock")?
            .clone();
        fixture.advance_to(9_000)?;
        fixture.consent("Black: Request Takeback").await?;
        let pending = fixture
            .snapshot()?
            .takeback_pending()
            .ok_or("missing local consent")?;
        assert_eq!(pending.requester(), BoardEnumColor::Black);
        assert_eq!(pending.live(), black.live());
        assert_eq!(pending.target(), white.live());
        fixture.advance_to(15_000)?;
        fixture.assert_clock(594_000, 601_000, BoardEnumColor::White)?;
        fixture.consent("White: Accept Takeback").await?;
        let undone = fixture.snapshot()?;
        assert_eq!(undone.live(), white.live());
        assert_eq!(undone.view(), white.live());
        assert_eq!(undone.moves().len(), 1);
        assert!(undone.takeback_pending().is_none());
        fixture.assert_clock(602_000, 596_000, BoardEnumColor::Black)?;
        assert_eq!(
            undone.clock().snapshot(fixture.runtime.clock.now()?)?,
            before
        );
        let saved = fixture.persisted().await?;
        assert_eq!(saved.tree().node(white.live())?.children(), &[black.live()]);
        fixture.advance_to(16_000)?;
        fixture.play("c7", "c5").await?;
        let alternative = fixture.snapshot()?;
        fixture.assert_clock(602_000, 600_000, BoardEnumColor::White)?;
        fixture.consent("White: Request Takeback").await?;
        fixture.consent("Black: Accept Takeback").await?;
        fixture.advance_to(17_000)?;
        fixture.play("e7", "e5").await?;
        let replayed = fixture.snapshot()?;
        assert_eq!(replayed.live(), black.live());
        fixture.assert_clock(602_000, 599_000, BoardEnumColor::White)?;
        let saved = fixture.persisted().await?;
        assert_eq!(saved.tree().len(), 4);
        assert_eq!(
            saved.tree().node(white.live())?.children(),
            &[black.live(), alternative.live()]
        );
        assert_ne!(
            saved.tree().node(black.live())?.before_clock(),
            Some(&before)
        );
        Ok(())
    }
    .await;
    let cleanup = fixture.close().await;
    DesktopLocalPlayFixture::finish(journey, cleanup)
}

#[tokio::test]
async fn mounted_decline_cancel_and_another_move_preserve_history_and_running_clocks()
-> DesktopLocalPlayTestResult {
    let mut fixture = DesktopLocalPlayFixture::new().await?;
    let journey: DesktopLocalPlayTestResult = async {
        fixture.start(GameTakebackEnumConsent::Required).await?;
        fixture.advance_to(3_000)?;
        fixture.play("e2", "e4").await?;
        let white = fixture.snapshot()?;
        fixture.advance_to(4_000)?;
        fixture.consent("White: Request Takeback").await?;
        fixture.advance_to(9_000)?;
        fixture.assert_clock(602_000, 594_000, BoardEnumColor::Black)?;
        fixture.consent("Black: Decline Takeback").await?;
        fixture.advance_to(10_000)?;
        fixture.consent("Black: Request Takeback").await?;
        fixture.advance_to(12_000)?;
        fixture.consent("Black: Cancel Request").await?;
        assert!(fixture.snapshot()?.takeback_pending().is_none());
        assert_eq!(fixture.snapshot()?.live(), white.live());
        assert_eq!(fixture.snapshot()?.moves().len(), 1);
        fixture.assert_clock(602_000, 591_000, BoardEnumColor::Black)?;
        fixture.consent("White: Request Takeback").await?;
        let pending = fixture.snapshot()?;
        fixture.advance_to(14_000)?;
        fixture.play("e7", "e5").await?;
        assert!(fixture.snapshot()?.takeback_pending().is_none());
        fixture.assert_clock(602_000, 594_000, BoardEnumColor::White)?;
        let response = fixture.command(pending.revision(), DesktopGameCommandsEnumAction::RespondTakeback {
            responder: BoardEnumColor::Black,
            response: GameTakebackEnumResponse::Accept,
        }).await?;
        assert!(matches!(response, DesktopMessagesEnumResult::Failure(failure) if matches!(failure.code(), DesktopEnumFailureCode::StaleRevision)));
        fixture.consent("Black: Request Takeback").await?;
        fixture.consent("White: Decline Takeback").await?;
        assert!(fixture.snapshot()?.takeback_pending().is_none());
        let saved = fixture.persisted().await?;
        assert_eq!(saved.tree().len(), 3);
        assert_eq!(fixture.snapshot()?.moves().len(), 2);
        fixture.assert_clock(602_000, 594_000, BoardEnumColor::White)?;
        Ok(())
    }.await;
    let cleanup = fixture.close().await;
    DesktopLocalPlayFixture::finish(journey, cleanup)
}

#[tokio::test]
async fn mounted_new_game_can_disable_local_consent_and_undo_immediately()
-> DesktopLocalPlayTestResult {
    let mut fixture = DesktopLocalPlayFixture::new().await?;
    let journey: DesktopLocalPlayTestResult = async {
        fixture.start(GameTakebackEnumConsent::NotRequired).await?;
        fixture.advance_to(3_000)?;
        fixture.play("e2", "e4").await?;
        let white = fixture.snapshot()?;
        fixture.advance_to(5_000)?;
        fixture.consent("White: Request Takeback").await?;
        let undone = fixture.snapshot()?;
        assert_eq!(undone.live(), white.root());
        assert!(undone.moves().is_empty());
        assert!(undone.takeback_pending().is_none());
        fixture.assert_clock(597_000, 600_000, BoardEnumColor::White)?;
        let saved = fixture.persisted().await?;
        assert_eq!(saved.tree().node(saved.live())?.children(), &[white.live()]);
        Ok(())
    }
    .await;
    let cleanup = fixture.close().await;
    DesktopLocalPlayFixture::finish(journey, cleanup)
}
