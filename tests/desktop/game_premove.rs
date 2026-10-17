use std::future::pending;
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant as StandardInstant};

use bevy_egui::egui::{Context, RawInput, Rect};
use rand::{SeedableRng, rngs::StdRng};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::capacity::phase::CapacityPhase;
use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::clock::time_control::ClockTimeControl;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationPort, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider,
};
use crate::desktop::dto::DesktopEnumEvent;
use crate::desktop::game_bridge::{
    DesktopGameBridgeEnumOperation, DesktopGameBridgeProvider, DesktopGameBridgeRequest,
};
use crate::desktop::game_commands::DesktopGameCommandsEnumAction;
use crate::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryFactoryProvider, DesktopGameRegistryOptions,
    DesktopGameRegistryPort,
};
use crate::desktop::game_snapshot::{DesktopGameSnapshot, DesktopGameSnapshotEnumNotice};
use crate::desktop::game_supervisor::{
    DesktopGameSupervisorEngineFactory, DesktopGameSupervisorEngineProvider,
};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::identity::DesktopRevision;
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::desktop::stream::{DesktopEnumStreamClass, DesktopStream};
use crate::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use crate::game::configuration::{
    GameConfiguration, GameConfigurationEnumOpponent, GameConfigurationEnumStart,
    GameConfigurationOptions,
};
use crate::game::engine_provider::{
    GameEngineContext, GameEngineEnumEvent, GameEngineMove, GameEngineProvider,
    GameEngineQuiescenceReceipt, GameEngineRequest, GameEngineSearchConfig,
    GameEngineSearchIdentity, GameEngineStopReceipt,
};
use crate::game::enums::{GameEnumOutcomeReason, GameEnumPhase, GameEnumResult, GameId};
use crate::game::premove::GamePremoveIntent;
use crate::game::session::GameSession;
use crate::game::storage_repository::GameStorageRepository;
use crate::presentation::app::PresentationApp;
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::game_document::{StorageGameDocument, StorageGameDocumentRepository};

type DesktopGamePremoveTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct DesktopGamePremoveClock(AtomicU64);

struct DesktopGamePremoveWake;

impl DesktopApplicationWakeProvider for DesktopGamePremoveWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

impl ClockTimeProvider for DesktopGamePremoveClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.0.load(Ordering::SeqCst)))
    }
}

struct DesktopGamePremoveProof;

impl GameAdjudicationMateProofProvider for DesktopGamePremoveProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

struct DesktopGamePremoveScript {
    elapsed: u64,
}

struct DesktopGamePremoveEngine {
    scripts: mpsc::Receiver<DesktopGamePremoveScript>,
    clock: Arc<DesktopGamePremoveClock>,
    identity: Option<GameEngineSearchIdentity>,
    shutdown_complete: bool,
}

impl DesktopGamePremoveEngine {
    const REPLY_AT: u64 = 1_000_000_000;
    fn identity(&self) -> Result<GameEngineSearchIdentity, std::io::Error> {
        self.identity
            .ok_or_else(|| std::io::Error::other("No scripted search is active"))
    }
}

impl GameEngineProvider for DesktopGamePremoveEngine {
    type Error = std::io::Error;
    fn is_idle(&self) -> bool {
        self.identity.is_none()
    }
    async fn start(
        &mut self,
        request: GameEngineRequest,
    ) -> Result<GameEngineSearchIdentity, Self::Error> {
        let identity = GameEngineSearchIdentity::new(
            request.context(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
        );
        self.identity = Some(identity);
        Ok(identity)
    }
    async fn next_event(
        &mut self,
        expected: GameEngineContext,
    ) -> Result<GameEngineEnumEvent, Self::Error> {
        let script = self
            .scripts
            .recv()
            .await
            .ok_or_else(|| std::io::Error::other("Script channel closed"))?;
        let identity = self.identity()?;
        if identity.context() != expected {
            return Err(std::io::Error::other("Stale scripted context"));
        }
        let position = BoardPosition::standard();
        let movement = position.parse_uci("e2e4").map_err(std::io::Error::other)?;
        self.clock.0.store(
            Self::REPLY_AT
                .checked_add(script.elapsed)
                .ok_or_else(|| std::io::Error::other("Script time overflow"))?,
            Ordering::SeqCst,
        );
        self.identity = None;
        Ok(GameEngineEnumEvent::Move(GameEngineMove::new(
            identity,
            movement,
            ClockInstant::from_nanos(Self::REPLY_AT),
        )))
    }
    async fn stop(
        &mut self,
        identity: GameEngineSearchIdentity,
        _received_at: StandardInstant,
    ) -> Result<GameEngineStopReceipt, Self::Error> {
        self.identity = None;
        Ok(GameEngineStopReceipt::new(identity))
    }
    async fn quiesce(
        &mut self,
        expected: GameEngineContext,
        _received_at: StandardInstant,
    ) -> Result<GameEngineQuiescenceReceipt, Self::Error> {
        Ok(GameEngineQuiescenceReceipt::new(
            expected,
            uuid::Uuid::new_v4(),
            self.identity.take(),
        ))
    }
}

impl DesktopGameSupervisorEngineProvider for DesktopGamePremoveEngine {
    fn is_shutdown_complete(&self) -> bool {
        self.shutdown_complete
    }
    async fn refresh_resources(&mut self) -> Result<(), Self::Error> {
        pending().await
    }
    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        self.identity = None;
        self.shutdown_complete = true;
        Ok(())
    }
}

struct DesktopGamePremoveFactory(Option<DesktopGamePremoveEngine>);

impl DesktopGameSupervisorEngineFactory for DesktopGamePremoveFactory {
    type Engine = DesktopGamePremoveEngine;
    type Error = std::io::Error;
    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        self.0
            .take()
            .ok_or_else(|| std::io::Error::other("Scripted engine already consumed"))
    }
    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl DesktopGameRegistryFactoryProvider for DesktopGamePremoveFactory {
    type Factory = Self;
    type Error = std::io::Error;
    fn factory(&mut self, _game: GameId) -> Result<Self::Factory, Self::Error> {
        Ok(Self(self.0.take()))
    }
}

struct DesktopGamePremoveFixture {
    directory: tempfile::TempDir,
    executor: Arc<CapacityExecutor>,
    store: StorageDocumentStore,
    clock: Arc<DesktopGamePremoveClock>,
}

#[derive(Clone, Copy)]
enum DesktopGamePremoveEnumStorage {
    Healthy,
    Blocked,
    FailEngineMove,
    FailPremove,
}

struct DesktopGamePremoveScenario {
    control: ClockTimeControl,
    destination: &'static str,
    elapsed: u64,
    expected_moves: usize,
    storage: DesktopGamePremoveEnumStorage,
}

impl DesktopGamePremoveFixture {
    const BASE: u64 = 10_000_000_000;
    const BONUS: u64 = 5_000_000;
    async fn new() -> DesktopGamePremoveTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(
            NonZeroUsize::new(CapacityPhase::MINIMUM_WORKERS).ok_or("zero capacity")?,
        )?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store =
            StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
        let fixture = Self {
            directory,
            executor,
            store,
            clock: Arc::new(DesktopGamePremoveClock::default()),
        };
        fixture.sql(|connection| connection.execute_batch("CREATE TABLE premove_audit (ordinal INTEGER PRIMARY KEY, storage_revision TEXT NOT NULL, body TEXT NOT NULL); CREATE TRIGGER premove_insert AFTER INSERT ON documents BEGIN INSERT INTO premove_audit(storage_revision, body) VALUES (NEW.revision, NEW.body); END; CREATE TRIGGER premove_update AFTER UPDATE ON documents BEGIN INSERT INTO premove_audit(storage_revision, body) VALUES (NEW.revision, NEW.body); END;")).await?;
        Ok(fixture)
    }
    async fn sql<T: Send + 'static>(
        &self,
        action: impl FnOnce(rusqlite::Connection) -> Result<T, rusqlite::Error>
        + Send
        + std::panic::UnwindSafe
        + 'static,
    ) -> DesktopGamePremoveTestResult<T> {
        let path = self.directory.path().join("library.sqlite3");
        Ok(self
            .executor
            .submit_waiting(move |_cancellation| action(rusqlite::Connection::open(path)?))
            .await?
            .join()
            .await??)
    }
    async fn checkpoints(&self) -> DesktopGamePremoveTestResult<Vec<(u64, GameSession)>> {
        let rows = self
            .sql(|connection| {
                connection
                    .prepare("SELECT storage_revision, body FROM premove_audit ORDER BY ordinal")?
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()
            })
            .await?;
        rows.into_iter()
            .map(|(revision, body)| Ok((revision.parse()?, StorageGameDocument::restore(&body)?)))
            .collect()
    }
    async fn verify_saved(
        &self,
        saved: &DesktopGameSnapshot,
        before_count: usize,
        check: impl FnOnce(&DesktopGameSnapshot, &[(u64, GameSession)]) -> DesktopGamePremoveTestResult,
    ) -> DesktopGamePremoveTestResult {
        let checkpoints = self.checkpoints().await?;
        let repository = StorageGameDocumentRepository::new(self.store.client(), &self.executor);
        let restored = repository
            .load(saved.id())
            .await?
            .ok_or("saved game absent")?;
        assert_eq!(restored.session().revision(), saved.revision());
        assert_eq!(restored.session().live(), saved.live());
        let live_clock = saved.clock().view(saved.captured_at())?;
        let restored_clock = restored.session().clock().view(saved.captured_at())?;
        for color in [BoardEnumColor::White, BoardEnumColor::Black] {
            assert_eq!(
                restored_clock.side(color).remaining(),
                live_clock.side(color).remaining()
            );
            assert_eq!(
                restored_clock.side(color).completed_moves(),
                live_clock.side(color).completed_moves()
            );
            assert_eq!(
                restored_clock.side(color).stage_index(),
                live_clock.side(color).stage_index()
            );
        }
        assert_eq!(
            restored
                .session()
                .tree()
                .node(saved.live())?
                .position()
                .fen(),
            saved.board().fen()
        );
        check(saved, &checkpoints[before_count..])
    }
    fn frame(
        app: &mut PresentationApp,
        context: &Context,
        port: &DesktopApplicationPort<DesktopGamePremoveClock, DesktopGamePremoveWake>,
    ) -> DesktopGamePremoveTestResult {
        let mut result = None;
        let mut output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    bevy_egui::egui::Pos2::ZERO,
                    bevy_egui::egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            },
            |ui| {
                let mut picker = |_point| Ok(None);
                result = Some(app.show(ui, port, None, &mut picker));
            },
        );
        output.textures_delta.clear();
        result.ok_or("native notice frame absent")??;
        Ok(())
    }
    async fn action(
        port: &DesktopGameRegistryPort,
        snapshot: &DesktopGameSnapshot,
        action: DesktopGameCommandsEnumAction,
    ) -> DesktopGamePremoveTestResult<DesktopGameSnapshot> {
        Ok(port
            .execute(DesktopGameBridgeRequest {
                request_id: DesktopRequestId::new(),
                received_at: snapshot.captured_at(),
                operation: DesktopGameBridgeEnumOperation::Session {
                    game: snapshot.id(),
                    revision: snapshot.revision(),
                    action,
                },
            })
            .await
            .map_err(|error| std::io::Error::other(error.message().to_owned()))?)
    }
    async fn exercise(
        control: ClockTimeControl,
        destination: &'static str,
        elapsed: u64,
        expected_moves: usize,
        check: impl FnOnce(&DesktopGameSnapshot, &[(u64, GameSession)]) -> DesktopGamePremoveTestResult,
    ) -> DesktopGamePremoveTestResult {
        Self::exercise_storage(
            DesktopGamePremoveScenario {
                control,
                destination,
                elapsed,
                expected_moves,
                storage: DesktopGamePremoveEnumStorage::Healthy,
            },
            check,
        )
        .await
    }

    async fn exercise_storage(
        scenario: DesktopGamePremoveScenario,
        check: impl FnOnce(&DesktopGameSnapshot, &[(u64, GameSession)]) -> DesktopGamePremoveTestResult,
    ) -> DesktopGamePremoveTestResult {
        let DesktopGamePremoveScenario {
            control,
            destination,
            elapsed,
            expected_moves,
            storage,
        } = scenario;
        let fixture = Self::new().await?;
        let (scripts, receiver) = mpsc::channel(1);
        let (mut registry, port) = DesktopGameRegistry::new(DesktopGameRegistryOptions {
            factory_provider: DesktopGamePremoveFactory(Some(DesktopGamePremoveEngine {
                scripts: receiver,
                clock: Arc::clone(&fixture.clock),
                identity: None,
                shutdown_complete: false,
            })),
            storage: fixture.store.client(),
            executor: Arc::clone(&fixture.executor),
            clock: Arc::clone(&fixture.clock),
            wall_time: crate::game_storage_wall_time_fixture::GameStorageWallTimeFixture::shared(),
            proof: Arc::new(DesktopGamePremoveProof),
            random: StdRng::seed_from_u64(81),
        });
        let worker = tokio::spawn(async move {
            let result = registry.run().await;
            (registry, result)
        });
        let first = port
            .execute(DesktopGameBridgeRequest {
                request_id: DesktopRequestId::new(),
                received_at: ClockInstant::from_nanos(0),
                operation: DesktopGameBridgeEnumOperation::Create {
                    owner: DesktopMessagesEntityId::new(),
                    configuration: GameConfiguration::new(
                        GameConfigurationEnumStart::Standard,
                        GameConfigurationEnumOpponent::Reckless {
                            human: BoardEnumColor::Black,
                        },
                        control,
                        GameConfigurationOptions::new(
                            GameEngineSearchConfig::initial_default(),
                            true,
                            true,
                            false,
                        ),
                    ),
                },
            })
            .await
            .map_err(|error| std::io::Error::other(error.message().to_owned()))?;
        let queued = Self::action(
            &port,
            &first,
            DesktopGameCommandsEnumAction::QueuePremove {
                intent: GamePremoveIntent::new("e7".parse()?, destination.parse()?, None)?,
            },
        )
        .await?;
        assert!(queued.premove().is_some());
        let before = fixture.checkpoints().await?;
        let blocked = match storage {
            DesktopGamePremoveEnumStorage::Blocked => {
                let path = fixture.directory.path().join("library.sqlite3");
                let ready = Arc::new(tokio::sync::Notify::new());
                let entered = Arc::clone(&ready);
                let (release, released) = std::sync::mpsc::sync_channel(1);
                let task = fixture.executor.submit_waiting(move |_cancellation| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                    let connection = rusqlite::Connection::open(path)?;
                    connection.execute_batch("BEGIN IMMEDIATE")?;
                    entered.notify_one();
                    released.recv()?;
                    connection.execute_batch("ROLLBACK")?;
                    Ok(())
                }).await?;
                ready.notified().await;
                Some((release, task))
            }
            DesktopGamePremoveEnumStorage::FailEngineMove
            | DesktopGamePremoveEnumStorage::FailPremove => {
                let mut revision = queued.revision().next()?;
                if matches!(storage, DesktopGamePremoveEnumStorage::FailPremove) {
                    revision = revision.next()?;
                }
                fixture.sql(move |connection| {
                    connection.execute_batch("CREATE TABLE premove_failure (revision TEXT NOT NULL); CREATE TRIGGER premove_save_failure BEFORE INSERT ON documents WHEN CAST(json_extract(NEW.body, '$.game.revision') AS INTEGER) >= CAST((SELECT revision FROM premove_failure) AS INTEGER) BEGIN SELECT RAISE(FAIL, 'injected premove checkpoint failure'); END;")?;
                    connection.execute("INSERT INTO premove_failure(revision) VALUES (?1)", [revision.to_string()])?;
                    Ok(())
                }).await?;
                None
            }
            DesktopGamePremoveEnumStorage::Healthy => None,
        };
        scripts.send(DesktopGamePremoveScript { elapsed }).await?;
        let mut snapshots = port.snapshots();
        let accepted = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(snapshot) = snapshots.borrow().iter().find(|snapshot| {
                    snapshot.moves().len() == expected_moves && snapshot.premove().is_none()
                }) {
                    break Ok::<_, tokio::sync::watch::error::RecvError>(snapshot.clone());
                }
                snapshots.changed().await?;
            }
        })
        .await??;
        if let Some((release, task)) = blocked {
            assert_eq!(fixture.checkpoints().await?.len(), before.len());
            release.send(())?;
            task.join()
                .await?
                .map_err(|error| -> Box<dyn std::error::Error> { error })?;
        }
        let (registry, checked) = match storage {
            DesktopGamePremoveEnumStorage::Healthy | DesktopGamePremoveEnumStorage::Blocked => {
                let saved = Self::action(
                    &port,
                    &accepted,
                    DesktopGameCommandsEnumAction::ReturnToLive,
                )
                .await?;
                let checked = fixture.verify_saved(&saved, before.len(), check).await;
                port.shutdown_all(Instant::now() + Duration::from_secs(5))?
                    .wait()
                    .await?;
                let (registry, result) = worker.await?;
                result?;
                (registry, checked)
            }
            DesktopGamePremoveEnumStorage::FailEngineMove
            | DesktopGamePremoveEnumStorage::FailPremove => {
                let interrupted = tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        if let Some(snapshot) = snapshots
                            .borrow()
                            .iter()
                            .find(|snapshot| snapshot.is_save_required())
                        {
                            break Ok::<_, tokio::sync::watch::error::RecvError>(snapshot.clone());
                        }
                        snapshots.changed().await?;
                    }
                })
                .await??;
                assert_eq!(interrupted.moves().len(), expected_moves);
                assert_eq!(interrupted.phase(), GameEnumPhase::Interrupted);
                assert!(!interrupted.clock().is_running());
                assert!(
                    port.shutdown_all(Instant::now() + Duration::from_secs(5))?
                        .wait()
                        .await
                        .is_err()
                );
                let (mut registry, result) = worker.await?;
                assert!(result.is_err());
                assert!(!registry.is_shutdown_complete());
                fixture
                    .sql(|connection| connection.execute_batch("DELETE FROM premove_failure"))
                    .await?;
                let saved = Self::action(
                    &port,
                    &interrupted,
                    DesktopGameCommandsEnumAction::RetrySave,
                )
                .await?;
                assert!(!saved.is_save_required());
                let checked = fixture.verify_saved(&saved, before.len(), check).await;
                registry
                    .shutdown_retained(Instant::now() + Duration::from_secs(5))
                    .await?;
                (registry, checked)
            }
        };
        assert!(registry.is_shutdown_complete());
        drop((registry, port, snapshots, scripts));
        fixture.store.close().await?;
        Arc::try_unwrap(fixture.executor)
            .map_err(|_owner| "executor still borrowed")?
            .shutdown()
            .await?;
        fixture.directory.close()?;
        checked
    }
}

#[tokio::test]
async fn legal_premove_saves_both_accepted_moves_in_order() -> DesktopGamePremoveTestResult {
    DesktopGamePremoveFixture::exercise_storage(
        DesktopGamePremoveScenario {
            control: ClockTimeControl::initial_default(),
            destination: "e5",
            elapsed: 700_001,
            expected_moves: 2,
            storage: DesktopGamePremoveEnumStorage::Blocked,
        },
        |snapshot, checkpoints| {
            assert_eq!(checkpoints.len(), 2);
            assert_eq!(checkpoints[0].0.checked_add(1), Some(checkpoints[1].0));
            assert_eq!(
                checkpoints[0].1.revision().next()?,
                checkpoints[1].1.revision()
            );
            assert_eq!(checkpoints[1].1.revision(), snapshot.revision());
            assert_eq!(
                checkpoints[0]
                    .1
                    .tree()
                    .path_to(checkpoints[0].1.live())?
                    .len(),
                2
            );
            assert_eq!(
                checkpoints[1]
                    .1
                    .tree()
                    .path_to(checkpoints[1].1.live())?
                    .len(),
                3
            );
            Ok(())
        },
    )
    .await
}

#[tokio::test]
async fn failed_engine_or_premove_checkpoint_retains_both_moves_for_retry_and_blocks_shutdown()
-> DesktopGamePremoveTestResult {
    for storage in [
        DesktopGamePremoveEnumStorage::FailEngineMove,
        DesktopGamePremoveEnumStorage::FailPremove,
    ] {
        DesktopGamePremoveFixture::exercise_storage(
            DesktopGamePremoveScenario {
                control: ClockTimeControl::initial_default(),
                destination: "e5",
                elapsed: 700_001,
                expected_moves: 2,
                storage,
            },
            |snapshot, checkpoints| {
                let expected = match storage {
                    DesktopGamePremoveEnumStorage::FailEngineMove => 1,
                    DesktopGamePremoveEnumStorage::FailPremove => 2,
                    DesktopGamePremoveEnumStorage::Healthy
                    | DesktopGamePremoveEnumStorage::Blocked => {
                        return Err("expected a checkpoint fault".into());
                    }
                };
                assert_eq!(checkpoints.len(), expected);
                let (_, final_checkpoint) = checkpoints.last().ok_or("retry checkpoint absent")?;
                assert_eq!(final_checkpoint.revision(), snapshot.revision());
                assert_eq!(final_checkpoint.live(), snapshot.live());
                assert_eq!(snapshot.moves().len(), 2);
                assert_eq!(snapshot.phase(), GameEnumPhase::Interrupted);
                Ok(())
            },
        )
        .await?;
    }
    Ok(())
}

#[tokio::test]
async fn illegal_premove_notice_survives_durable_save_and_registry_forwarding()
-> DesktopGamePremoveTestResult {
    DesktopGamePremoveFixture::exercise(
        ClockTimeControl::initial_default(),
        "e4",
        700_001,
        1,
        |snapshot, checkpoints| {
            assert_eq!(snapshot.phase(), GameEnumPhase::Active);
            assert!(snapshot.clock().is_running());
            assert_eq!(snapshot.live_turn(), BoardEnumColor::Black);
            assert_eq!(checkpoints.len(), 1);
            let Some(DesktopGameSnapshotEnumNotice::IllegalPremove { revision, intent }) =
                snapshot.notice()
            else {
                return Err("correlated premove notice absent".into());
            };
            assert_eq!(revision, snapshot.revision());
            assert_eq!(
                intent,
                GamePremoveIntent::new("e7".parse()?, "e4".parse()?, None)?
            );
            let now = snapshot.captured_at();
            let mut stream = DesktopStream::new(DesktopRequestId::new(), now);
            let mut sequence = DesktopRevision::ZERO;
            for _ in 0..DesktopStream::<DesktopEnumEvent>::MAX_UNACKNOWLEDGED {
                sequence = stream
                    .publish(
                        DesktopEnumEvent::Games(Vec::new()),
                        DesktopEnumStreamClass::Control,
                        now,
                    )?
                    .ok_or("control event missing")?
                    .sequence();
            }
            for _ in 0..2 {
                assert!(
                    stream
                        .publish(
                            DesktopEnumEvent::Games(vec![snapshot.clone()]),
                            DesktopEnumStreamClass::Snapshot,
                            now
                        )?
                        .is_none()
                );
            }
            stream.acknowledge(sequence, now)?;
            let events = stream.flush_pending(now)?;
            let [event] = events.as_slice() else {
                return Err("coalesced game event missing".into());
            };
            let DesktopEnumEvent::Games(snapshots) = event.payload() else {
                return Err("unexpected coalesced event".into());
            };
            let [snapshot] = snapshots.as_slice() else {
                return Err("coalesced game snapshot missing".into());
            };
            let mut app = PresentationApp::new();
            app.accept_game_snapshot(DesktopGameSnapshot::capture(&checkpoints[0].1, now)?);
            assert!(!app.status().contains("premove"));
            assert!(app.accept_game_snapshot(snapshot.clone()));
            assert!(app.status().contains("premove"));
            assert!(app.failure().is_none());
            assert!(!app.accept_game_snapshot(snapshot.clone()));
            let clock = Arc::new(DesktopGamePremoveClock::default());
            clock.0.store(100_000_000_000, Ordering::SeqCst);
            let application =
                DesktopApplication::new(Arc::clone(&clock), Arc::new(DesktopGamePremoveWake));
            let context = Context::default();
            context.enable_accesskit();
            DesktopGamePremoveFixture::frame(&mut app, &context, &application.port())?;
            assert!(app.status().contains("premove"));
            clock.0.store(103_000_000_000, Ordering::SeqCst);
            DesktopGamePremoveFixture::frame(&mut app, &context, &application.port())?;
            assert!(!app.status().contains("premove"));
            assert!(!app.accept_game_snapshot(snapshot.clone()));
            DesktopGamePremoveFixture::frame(&mut app, &context, &application.port())?;
            assert!(!app.status().contains("premove"));
            Ok(())
        },
    )
    .await
}

#[tokio::test]
async fn premove_charges_actual_zero_submillisecond_and_elapsed_time_with_each_bonus()
-> DesktopGamePremoveTestResult {
    let base = ClockDuration::from_nanos(DesktopGamePremoveFixture::BASE);
    let bonus = ClockDuration::from_nanos(DesktopGamePremoveFixture::BONUS);
    for (control, elapsed, expected) in [
        (
            ClockTimeControl::sudden_death(base)?,
            0,
            DesktopGamePremoveFixture::BASE,
        ),
        (
            ClockTimeControl::sudden_death(base)?,
            700_001,
            DesktopGamePremoveFixture::BASE - 700_001,
        ),
        (
            ClockTimeControl::fischer(base, bonus)?,
            20_000_000,
            DesktopGamePremoveFixture::BASE - 20_000_000 + DesktopGamePremoveFixture::BONUS,
        ),
        (
            ClockTimeControl::simple_delay(base, bonus)?,
            20_000_000,
            DesktopGamePremoveFixture::BASE - 20_000_000 + DesktopGamePremoveFixture::BONUS,
        ),
        (
            ClockTimeControl::bronstein_delay(base, bonus)?,
            700_001,
            DesktopGamePremoveFixture::BASE,
        ),
    ] {
        DesktopGamePremoveFixture::exercise(control, "e5", elapsed, 2, |snapshot, _checkpoints| {
            let view = snapshot.clock().view(snapshot.captured_at())?;
            assert_eq!(
                view.side(BoardEnumColor::Black).remaining(),
                Some(ClockDuration::from_nanos(expected))
            );
            assert_eq!(view.side(BoardEnumColor::Black).completed_moves(), 1);
            Ok(())
        })
        .await?;
    }
    Ok(())
}

#[tokio::test]
async fn premove_at_the_exact_deadline_is_rejected_and_saved_as_flag_fall()
-> DesktopGamePremoveTestResult {
    DesktopGamePremoveFixture::exercise(
        ClockTimeControl::sudden_death(ClockDuration::from_nanos(DesktopGamePremoveFixture::BASE))?,
        "e5",
        DesktopGamePremoveFixture::BASE,
        1,
        |snapshot, checkpoints| {
            assert_eq!(snapshot.phase(), GameEnumPhase::Completed);
            assert!(!snapshot.clock().is_running());
            let outcome = snapshot.outcome().ok_or("flag-fall result absent")?;
            assert_eq!(outcome.reason(), GameEnumOutcomeReason::Flagfall);
            assert_eq!(
                outcome.result(),
                GameEnumResult::Victory(BoardEnumColor::White)
            );
            assert_eq!(
                snapshot
                    .clock()
                    .view(snapshot.captured_at())?
                    .side(BoardEnumColor::Black)
                    .completed_moves(),
                0
            );
            assert_eq!(checkpoints.len(), 2);
            Ok(())
        },
    )
    .await
}
