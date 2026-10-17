use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use thiserror::Error;
use tokio::runtime::Builder;
use tokio::time::timeout;

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::r#move::{BoardMove, BoardMoveError};
use chess::board::position::{BoardPosition, BoardPositionError};
use chess::board::square::{BoardSquare, BoardSquareError};
use chess::capacity::budget::{CapacityBudget, CapacityBudgetError};
use chess::capacity::phase::CapacityPhaseEnumState;
use chess::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use chess::clock::state::{ClockState, ClockStateError, ClockStateView};
use chess::clock::time_control::{ClockTimeControl, ClockTimeControlError};
use chess::desktop::application::DesktopApplicationError;
use chess::desktop::dto::{DesktopEnumEngineSnapshot, DesktopEnumEvent, DesktopEnumValue};
use chess::desktop::error::{DesktopError, DesktopFailure};
use chess::desktop::game_bridge::DesktopGameBridgeEnumOperation;
use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::desktop::identity::{DesktopRequestId, DesktopRevision};
use chess::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
};
use chess::desktop::native::bootstrap::{DesktopNativeBootstrap, DesktopNativeBootstrapError};
use chess::desktop::native::host::{DesktopNativeHostEvent, DesktopNativeHostServices};
use chess::desktop::native::startup::{
    DesktopNativeStartup, DesktopNativeStartupEnumAdmission, DesktopNativeStartupError,
    DesktopNativeStartupOwner,
};
use chess::desktop::profile::{
    DesktopProfileError, DesktopProfileId, DesktopProfileOsLock, DesktopProfileSecurity,
};
use chess::engine::package::host::{EnginePackageHost, EnginePackageHostError};
use chess::engine::package::manifest::EnginePackageManifest;
use chess::engine::package::selector::{EnginePackageSelector, EnginePackageSelectorError};
use chess::game::configuration::{
    GameConfiguration, GameConfigurationEnumOpponent, GameConfigurationEnumStart,
    GameConfigurationOptions,
};
use chess::game::enums::{
    GameEnumOutcomeReason, GameEnumPhase, GameEnumResult, GameEnumSeat, GameIdentityError,
    GameRevisionError,
};
use chess::game::tree::GameTreeError;
use chess::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use chess::storage::game_document::{StorageGameDocument, StorageGameDocumentError};

#[derive(Debug, Error)]
pub(super) enum NativeGameCompositionError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Position(#[from] BoardPositionError),
    #[error(transparent)]
    Move(#[from] BoardMoveError),
    #[error(transparent)]
    Square(#[from] BoardSquareError),
    #[error(transparent)]
    Capacity(#[from] CapacityBudgetError),
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
    #[error(transparent)]
    Clock(#[from] ClockStateError),
    #[error(transparent)]
    TimeControl(#[from] ClockTimeControlError),
    #[error(transparent)]
    Desktop(#[from] DesktopError),
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
    #[error(transparent)]
    Bootstrap(#[from] DesktopNativeBootstrapError),
    #[error(transparent)]
    Startup(#[from] DesktopNativeStartupError),
    #[error(transparent)]
    Profile(#[from] DesktopProfileError),
    #[error(transparent)]
    Host(#[from] EnginePackageHostError),
    #[error(transparent)]
    Package(#[from] EnginePackageSelectorError),
    #[error(transparent)]
    Identity(#[from] GameIdentityError),
    #[error(transparent)]
    Revision(#[from] GameRevisionError),
    #[error(transparent)]
    Tree(#[from] GameTreeError),
    #[error(transparent)]
    Storage(#[from] StorageDocumentError),
    #[error(transparent)]
    Document(#[from] StorageGameDocumentError),
    #[error("Production game composition violated acceptance: {0}")]
    Invariant(&'static str),
    #[error("Unexpected production game response: {0:?}")]
    Response(Box<DesktopMessagesEnumResult>),
    #[error("Unexpected production game value: {0:?}")]
    Value(Box<DesktopEnumValue>),
    #[error("Production game lifecycle failed: {0:?}")]
    Lifecycle(DesktopFailure),
    #[error("The real packaged engine is unavailable: {0}")]
    EngineUnavailable(String),
    #[error("Production game composition exceeded its deadline while {stage}: {source}")]
    Deadline {
        stage: &'static str,
        #[source]
        source: tokio::time::error::Elapsed,
    },
    #[error("Game composition failed ({operation}); cleanup also failed ({cleanup})")]
    Cleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
}

type NativeGameCompositionResult<T = ()> = Result<T, NativeGameCompositionError>;

impl NativeGameCompositionError {
    fn finish<T>(
        operation: NativeGameCompositionResult<T>,
        cleanup: NativeGameCompositionResult,
    ) -> NativeGameCompositionResult<T> {
        match (operation, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
            (Err(operation), Err(cleanup)) => Err(Self::Cleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            }),
        }
    }
}

struct NativeGameCompositionCase {
    human: BoardEnumColor,
    initial: BoardPosition,
    movement: BoardMove,
    san: String,
    final_position: BoardPosition,
    rejected_origin: BoardSquare,
    rejected_destination: BoardSquare,
}

impl NativeGameCompositionCase {
    fn new(
        human: BoardEnumColor,
        fen: &str,
        mate: &str,
        rejected: (&str, &str),
    ) -> NativeGameCompositionResult<Self> {
        let initial = BoardPosition::from_fen(fen, BoardEnumVariant::Standard)?;
        let movement = initial.parse_uci(mate)?;
        let final_position = initial.play(movement)?;
        if initial.turn() != human.opposite()
            || initial.is_checkmate()
            || !final_position.is_checkmate()
            || final_position.turn() != human
        {
            return Err(NativeGameCompositionError::Invariant(
                "each custom fixture independently proves the engine's mate in one",
            ));
        }
        Ok(Self {
            human,
            san: initial.san(mate)?,
            initial,
            movement,
            final_position,
            rejected_origin: rejected.0.parse()?,
            rejected_destination: rejected.1.parse()?,
        })
    }

    fn configuration(&self) -> NativeGameCompositionResult<GameConfiguration> {
        Ok(GameConfiguration::new(
            GameConfigurationEnumStart::Custom(self.initial.clone()),
            GameConfigurationEnumOpponent::Reckless { human: self.human },
            ClockTimeControl::sudden_death(NativeGameCompositionHarness::INITIAL_TIME)?,
            GameConfigurationOptions::initial_default(),
        ))
    }

    fn validate(&self, snapshot: &DesktopGameSnapshot) -> NativeGameCompositionResult {
        let outcome = snapshot
            .outcome()
            .ok_or(NativeGameCompositionError::Invariant(
                "the real engine's accepted move has a terminal outcome",
            ))?;
        if snapshot.phase() != GameEnumPhase::Completed
            || snapshot.interruption().is_some()
            || outcome.result() != GameEnumResult::Victory(self.human.opposite())
            || outcome.reason() != GameEnumOutcomeReason::Checkmate
            || outcome.origin() != snapshot.live()
            || snapshot.view() != snapshot.live()
            || snapshot.live() == snapshot.root()
            || snapshot.board().fen() != self.final_position.fen()
            || snapshot.live_turn() != self.human
            || snapshot.moves().len() != 1
            || snapshot.moves()[0].id() != snapshot.live()
            || snapshot.moves()[0].mover() != self.human.opposite()
            || snapshot.moves()[0].san() != self.san
            || snapshot.seats().seat(self.human) != GameEnumSeat::Human
            || snapshot.seats().seat(self.human.opposite()) != GameEnumSeat::Reckless
            || snapshot.is_save_required()
        {
            return Err(NativeGameCompositionError::Invariant(
                "exactly one real-engine move completes the expected checkmate with its live origin",
            ));
        }
        let clock = NativeGameCompositionHarness::frozen(snapshot.clock(), snapshot.captured_at())?;
        if clock.active() != self.human
            || clock.side(self.human).remaining()
                != Some(NativeGameCompositionHarness::INITIAL_TIME)
            || clock.side(self.human).completed_moves() != 0
            || clock.side(self.human.opposite()).completed_moves() != 1
            || !clock
                .side(self.human.opposite())
                .remaining()
                .is_some_and(|remaining| {
                    remaining > ClockDuration::ZERO
                        && remaining <= NativeGameCompositionHarness::INITIAL_TIME
                })
        {
            return Err(NativeGameCompositionError::Invariant(
                "only the engine's accepted move charges its thirty-second clock",
            ));
        }
        Ok(())
    }
}

struct NativeGameCompositionCompleted {
    case: NativeGameCompositionCase,
    snapshot: DesktopGameSnapshot,
}

struct NativeGameCompositionDocument {
    revision: u64,
    body: String,
}

struct NativeGameCompositionStream {
    subscription: DesktopRequestId,
    sequence: DesktopRevision,
}

impl NativeGameCompositionStream {
    async fn subscribe(services: &DesktopNativeHostServices) -> NativeGameCompositionResult<Self> {
        let response =
            NativeGameCompositionHarness::command(services, DesktopEnumCommand::StreamSubscribe)
                .await?;
        let value = NativeGameCompositionHarness::value(response)?;
        let DesktopEnumValue::StreamSubscribe {
            subscription_id,
            sequence,
            snapshot,
        } = value
        else {
            return Err(NativeGameCompositionError::Value(Box::new(value)));
        };
        NativeGameCompositionHarness::engine(snapshot.engine())?;
        Ok(Self {
            subscription: subscription_id,
            sequence,
        })
    }

    async fn games(
        &mut self,
        services: &DesktopNativeHostServices,
    ) -> NativeGameCompositionResult<Vec<DesktopGameSnapshot>> {
        NativeGameCompositionHarness::bounded("waiting for a game event", async {
            loop {
                NativeGameCompositionHarness::acknowledge_wake(services);
                let Some(event) = NativeGameCompositionHarness::application(|| {
                    services.port.try_event()
                })
                .await?
                else {
                    services.wake.notified().await;
                    continue;
                };
                if event.subscription_id() != self.subscription
                    || event.sequence() != self.sequence.next()?
                {
                    return Err(NativeGameCompositionError::Invariant(
                        "native lifecycle events preserve their subscription and continuous sequence",
                    ));
                }
                let games = match event.payload() {
                    DesktopEnumEvent::Games(games) => {
                        if games.iter().any(|game| game.interruption().is_some()) {
                            return Err(NativeGameCompositionError::Invariant(
                                "real engine play never substitutes interruption or Unknown for checkmate",
                            ));
                        }
                        Some(games.clone())
                    }
                    DesktopEnumEvent::Snapshot(snapshot) => {
                        NativeGameCompositionHarness::engine(snapshot.engine())?;
                        None
                    }
                    DesktopEnumEvent::Failure(failure) => {
                        return Err(NativeGameCompositionError::Lifecycle(failure.clone()));
                    }
                };
                let response = NativeGameCompositionHarness::command(
                    services,
                    DesktopEnumCommand::StreamAcknowledge {
                        subscription_id: self.subscription,
                        sequence: event.sequence(),
                    },
                )
                .await?;
                if !matches!(
                    response,
                    DesktopMessagesEnumResult::Value {
                        value: DesktopEnumValue::StreamAcknowledge,
                        ..
                    }
                ) {
                    return Err(NativeGameCompositionError::Response(Box::new(response)));
                }
                self.sequence = event.sequence();
                if let Some(games) = games {
                    return Ok(games);
                }
            }
        })
        .await
    }
}

pub(super) struct NativeGameCompositionHarness;

impl NativeGameCompositionHarness {
    const DEADLINE: Duration = Duration::from_secs(5);
    const EXERCISE_DEADLINE: Duration = Duration::from_secs(30);
    const INITIAL_TIME: ClockDuration = ClockDuration::from_nanos(30_000_000_000);
    const FROZEN_INTERVAL: ClockDuration = ClockDuration::from_nanos(10_000_000_000);
    const PROFILE_ID: u128 = 0xab0e6b7f_9aad_4ab4_8de0_05b0c43d7750;

    async fn bounded<T>(
        stage: &'static str,
        operation: impl Future<Output = NativeGameCompositionResult<T>>,
    ) -> NativeGameCompositionResult<T> {
        timeout(Self::DEADLINE, operation)
            .await
            .map_err(|source| NativeGameCompositionError::Deadline { stage, source })?
    }

    fn acknowledge_wake(services: &DesktopNativeHostServices) {
        services.wake.acknowledge();
        services.port.acknowledge_wake();
    }

    async fn ready(services: &DesktopNativeHostServices) -> NativeGameCompositionResult {
        let generation = services
            .capacity
            .lock()
            .map_err(|_poisoned| {
                NativeGameCompositionError::Invariant("native capacity is not poisoned")
            })?
            .generation()
            .ok_or(NativeGameCompositionError::Invariant(
                "bootstrap supplies the framework preparation generation",
            ))?;
        services
            .events
            .send_replace(DesktopNativeHostEvent::FrameworkReady { generation });
        Self::bounded("entering the native steady capacity phase", async {
            loop {
                Self::acknowledge_wake(services);
                let state = services
                    .capacity
                    .lock()
                    .map_err(|_poisoned| {
                        NativeGameCompositionError::Invariant("native capacity is not poisoned")
                    })?
                    .state();
                if state == CapacityPhaseEnumState::Steady {
                    return Ok(());
                }
                services.wake.notified().await;
            }
        })
        .await
    }

    async fn application<T>(
        mut operation: impl FnMut() -> Result<T, DesktopApplicationError>,
    ) -> NativeGameCompositionResult<T> {
        Self::bounded("waiting for native application access", async {
            loop {
                match operation() {
                    Err(DesktopApplicationError::Busy) => tokio::task::yield_now().await,
                    result => return Ok(result?),
                }
            }
        })
        .await
    }

    async fn command(
        services: &DesktopNativeHostServices,
        command: DesktopEnumCommand,
    ) -> NativeGameCompositionResult<DesktopMessagesEnumResult> {
        let revision = match &command {
            DesktopEnumCommand::Puzzle { operation, .. } => Some(operation.expected_revision()),
            DesktopEnumCommand::Game(operation) => Some(operation.revision()),
            DesktopEnumCommand::Engine(operation) => Some(operation.context().configuration()),
            DesktopEnumCommand::Bootstrap
            | DesktopEnumCommand::FileSelect { .. }
            | DesktopEnumCommand::StreamSubscribe
            | DesktopEnumCommand::StreamAcknowledge { .. }
            | DesktopEnumCommand::StreamResync { .. }
            | DesktopEnumCommand::Stop { .. }
            | DesktopEnumCommand::Pause { .. }
            | DesktopEnumCommand::Quit => None,
        };
        let request = services
            .port
            .request(DesktopCommand::new(command, revision))?;
        Self::bounded("submitting and observing a native command", async {
            let receipt = Self::application(|| services.port.try_submit(&request)).await?;
            loop {
                Self::acknowledge_wake(services);
                if let Some(response) = receipt.response() {
                    return Ok(response.clone());
                }
                services.wake.notified().await;
            }
        })
        .await
    }

    fn game(
        response: DesktopMessagesEnumResult,
    ) -> NativeGameCompositionResult<DesktopGameSnapshot> {
        let value = Self::value(response)?;
        let DesktopEnumValue::Game(snapshot) = value else {
            return Err(NativeGameCompositionError::Value(Box::new(value)));
        };
        Ok(*snapshot)
    }

    fn value(response: DesktopMessagesEnumResult) -> NativeGameCompositionResult<DesktopEnumValue> {
        match response {
            DesktopMessagesEnumResult::Value { value, .. } => Ok(value),
            response @ (DesktopMessagesEnumResult::Failure(_)
            | DesktopMessagesEnumResult::Cancelled
            | DesktopMessagesEnumResult::Stopped
            | DesktopMessagesEnumResult::Paused
            | DesktopMessagesEnumResult::QuitReady
            | DesktopMessagesEnumResult::ReconciliationRequired) => {
                Err(NativeGameCompositionError::Response(Box::new(response)))
            }
        }
    }

    fn engine(snapshot: &DesktopEnumEngineSnapshot) -> NativeGameCompositionResult {
        match snapshot {
            DesktopEnumEngineSnapshot::Ready { identity } => {
                if identity.name().is_empty() || identity.author().is_empty() {
                    return Err(NativeGameCompositionError::Invariant(
                        "production bootstrap reports the real UCI identity",
                    ));
                }
                Ok(())
            }
            DesktopEnumEngineSnapshot::Unavailable { message } => Err(
                NativeGameCompositionError::EngineUnavailable(message.clone()),
            ),
        }
    }

    fn frozen(
        clock: &ClockState,
        now: ClockInstant,
    ) -> NativeGameCompositionResult<ClockStateView> {
        let later = now.checked_add(Self::FROZEN_INTERVAL)?;
        let view = clock.view(now)?;
        if clock.is_running() || view != clock.view(later)? {
            return Err(NativeGameCompositionError::Invariant(
                "a completed game's clock remains frozen across ten controlled seconds",
            ));
        }
        Ok(view)
    }

    async fn complete(
        services: &DesktopNativeHostServices,
        stream: &mut NativeGameCompositionStream,
        case: NativeGameCompositionCase,
    ) -> NativeGameCompositionResult<NativeGameCompositionCompleted> {
        let created = Self::game(
            Self::command(
                services,
                DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Create {
                    owner: DesktopMessagesEntityId::new(),
                    configuration: case.configuration()?,
                }),
            )
            .await?,
        )?;
        if created.phase() != GameEnumPhase::Active
            || !created.moves().is_empty()
            || created.board().fen() != case.initial.fen()
        {
            return Err(NativeGameCompositionError::Invariant(
                "Create starts the real engine game from its validated custom root",
            ));
        }
        let completed = loop {
            let games = stream.games(services).await?;
            if let Some(game) = games.into_iter().find(|game| game.id() == created.id()) {
                match game.phase() {
                    GameEnumPhase::Completed if !game.is_save_required() => break game,
                    GameEnumPhase::Completed => {}
                    GameEnumPhase::Active => {}
                    GameEnumPhase::Setup if game.revision() < created.revision() => {}
                    GameEnumPhase::Setup | GameEnumPhase::Interrupted | GameEnumPhase::Analysis => {
                        return Err(NativeGameCompositionError::Invariant(
                            "a production engine game reaches checkmate without interruption",
                        ));
                    }
                }
            }
        };
        case.validate(&completed)?;
        if completed.root() != created.root()
            || completed.revision() != created.revision().next()?
        {
            return Err(NativeGameCompositionError::Invariant(
                "one accepted engine move advances the original game revision exactly once",
            ));
        }
        let rejected = Self::command(
            services,
            DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session {
                game: completed.id(),
                revision: completed.revision(),
                action: DesktopGameCommandsEnumAction::Play {
                    node: uuid::Uuid::new_v4().to_string().parse()?,
                    origin: case.rejected_origin,
                    destination: case.rejected_destination,
                    promotion: None,
                    color: case.human,
                },
            }),
        )
        .await?;
        if !matches!(rejected, DesktopMessagesEnumResult::Failure(_)) {
            return Err(NativeGameCompositionError::Response(Box::new(rejected)));
        }
        let unchanged = Self::game(
            Self::command(
                services,
                DesktopEnumCommand::Game(DesktopGameBridgeEnumOperation::Session {
                    game: completed.id(),
                    revision: completed.revision(),
                    action: DesktopGameCommandsEnumAction::ReturnToLive,
                }),
            )
            .await?,
        )?;
        case.validate(&unchanged)?;
        if unchanged.revision() != completed.revision()
            || unchanged.live() != completed.live()
            || unchanged.root() != completed.root()
            || unchanged.outcome() != completed.outcome()
            || unchanged.clock() != completed.clock()
        {
            return Err(NativeGameCompositionError::Invariant(
                "a revision-checked post-result Play leaves the tree, outcome and clocks unchanged",
            ));
        }
        Ok(NativeGameCompositionCompleted {
            case,
            snapshot: unchanged,
        })
    }

    fn owner(profile: &Path) -> NativeGameCompositionResult<DesktopNativeStartupOwner> {
        let startup =
            DesktopNativeStartup::new(DesktopProfileSecurity::current()?, DesktopProfileOsLock);
        match startup.admit(
            profile,
            DesktopProfileId::from_uuid(uuid::Uuid::from_u128(Self::PROFILE_ID))?,
            DesktopRequestId::new(),
        )? {
            DesktopNativeStartupEnumAdmission::Owned(owner) => Ok(owner),
            DesktopNativeStartupEnumAdmission::ActivationOnly(_)
            | DesktopNativeStartupEnumAdmission::Busy
            | DesktopNativeStartupEnumAdmission::InstallationBlocked(_) => {
                Err(NativeGameCompositionError::Invariant(
                    "the isolated profile is exclusively reacquired after bootstrap cleanup",
                ))
            }
        }
    }

    fn released(budget: &CapacityBudget) -> NativeGameCompositionResult {
        if budget.available() != budget.workers() {
            return Err(NativeGameCompositionError::Invariant(
                "all production CPU leases are released before bootstrap returns",
            ));
        }
        let lease = budget.try_lease(budget.workers())?;
        drop(lease);
        Ok(())
    }

    fn no_children() -> NativeGameCompositionResult {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        let parent = Pid::from_u32(std::process::id());
        if system.process(parent).is_none() {
            return Err(NativeGameCompositionError::Invariant(
                "the process inventory observes the harness before checking its children",
            ));
        }
        if system
            .processes()
            .values()
            .any(|process| process.parent() == Some(parent))
        {
            return Err(NativeGameCompositionError::Invariant(
                "no direct engine child remains after production bootstrap returns",
            ));
        }
        Ok(())
    }

    fn bootstrap<T>(
        profile: &Path,
        exercise: impl AsyncFnOnce(&DesktopNativeHostServices) -> NativeGameCompositionResult<T>,
    ) -> NativeGameCompositionResult<T> {
        let mut operation = None;
        let mut budget = None;
        let cleanup = DesktopNativeBootstrap::run_with_host(
            [
                OsString::from("--profile-directory"),
                profile.as_os_str().to_owned(),
            ],
            |services| {
                budget = Some(services.budget.clone());
                operation = Some(services.runtime.block_on(async {
                    timeout(Self::EXERCISE_DEADLINE, exercise(&services))
                        .await
                        .map_err(|source| NativeGameCompositionError::Deadline {
                            stage: "exercising the production host services",
                            source,
                        })?
                }));
                Ok(())
            },
        )
        .map_err(NativeGameCompositionError::Bootstrap);
        let operation = operation
            .ok_or(NativeGameCompositionError::Invariant(
                "production bootstrap invokes its host callback",
            ))
            .and_then(|result| result);
        let result = NativeGameCompositionError::finish(operation, cleanup);
        let capacity = budget
            .as_ref()
            .ok_or(NativeGameCompositionError::Invariant(
                "the host callback retains only the independent budget observation",
            ))
            .and_then(Self::released);
        let result = NativeGameCompositionError::finish(result, capacity);
        let result = NativeGameCompositionError::finish(result, Self::no_children());
        let result = NativeGameCompositionError::finish(
            result,
            Self::owner(profile).and_then(|owner| Ok(owner.lease().verify_identity()?)),
        );
        if result.is_ok()
            && let Some(budget) = budget
        {
            println!(
                "Production cleanup observed: {}/{} CPU slots recovered, full-worker lease acquired, zero direct children, exclusive profile reacquired",
                budget.available(),
                budget.workers(),
            );
        }
        result
    }

    fn documents(
        profile: &Path,
        completed: &[NativeGameCompositionCompleted],
    ) -> NativeGameCompositionResult<Vec<NativeGameCompositionDocument>> {
        let owner = Self::owner(profile)?;
        let budget = CapacityBudget::discover()?;
        let runtime = Builder::new_current_thread()
            .max_blocking_threads(budget.workers())
            .enable_all()
            .build()?;
        let documents = runtime.block_on(async {
            let store =
                StorageDocumentStore::open(profile.join("library.sqlite3"), budget.clone()).await?;
            let operation = Self::bounded("reading completed durable game documents", async {
                let client = store.client();
                let mut documents = Vec::with_capacity(completed.len());
                for game in completed {
                    let document = client.read(game.snapshot.id().as_uuid()).await?.ok_or(
                        NativeGameCompositionError::Invariant(
                            "each completed game has an acknowledged durable document",
                        ),
                    )?;
                    documents.push(NativeGameCompositionDocument {
                        revision: document.revision(),
                        body: document.body().to_owned(),
                    });
                }
                Ok(documents)
            })
            .await;
            let cleanup = store
                .close()
                .await
                .map_err(NativeGameCompositionError::Storage);
            NativeGameCompositionError::finish(operation, cleanup)
        });
        drop(runtime);
        let documents = NativeGameCompositionError::finish(
            documents,
            owner.lease().verify_identity().map_err(Into::into),
        );
        NativeGameCompositionError::finish(documents, Self::released(&budget))
    }

    fn restore(
        completed: &[NativeGameCompositionCompleted],
        documents: &[NativeGameCompositionDocument],
    ) -> NativeGameCompositionResult {
        if documents.len() != completed.len() {
            return Err(NativeGameCompositionError::Invariant(
                "the durable document set contains both engine games",
            ));
        }
        for (game, document) in completed.iter().zip(documents) {
            let snapshot = &game.snapshot;
            let restored = StorageGameDocument::restore(&document.body)?;
            let tree = restored.tree();
            let root = tree.node(tree.root())?;
            let live = tree.node(restored.live())?;
            let clock = Self::frozen(restored.clock(), ClockInstant::from_nanos(0))?;
            if document.revision == 0
                || restored.id() != snapshot.id()
                || restored.revision() != snapshot.revision()
                || restored.phase() != GameEnumPhase::Completed
                || restored.outcome() != snapshot.outcome()
                || restored.live() != snapshot.live()
                || restored.view() != snapshot.view()
                || tree.root() != snapshot.root()
                || tree.len() != 2
                || root.children() != [live.id()]
                || live.parent() != Some(root.id())
                || !live.children().is_empty()
                || live.movement() != Some(game.case.movement)
                || root.position() != &game.case.initial
                || live.position() != &game.case.final_position
                || !live.position().is_checkmate()
                || clock != Self::frozen(snapshot.clock(), snapshot.captured_at())?
            {
                return Err(NativeGameCompositionError::Invariant(
                    "durable restoration preserves identities, revisions, the one-move tree, checkmate and frozen balances",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn run() -> NativeGameCompositionResult {
        let cases = [
            NativeGameCompositionCase::new(
                BoardEnumColor::Black,
                "7k/8/5KQ1/8/8/8/8/8 w - - 0 1",
                "g6g7",
                ("h8", "g8"),
            )?,
            NativeGameCompositionCase::new(
                BoardEnumColor::White,
                "8/8/8/8/8/5kq1/8/7K b - - 0 1",
                "g3g2",
                ("h1", "g1"),
            )?,
        ];
        let directory = tempfile::tempdir()?;
        let profile = directory.path().join("profile");
        let operation = (|| {
            let (completed, engine_directory): (Vec<_>, PathBuf) =
                Self::bootstrap(&profile, async |services| {
                    Self::ready(services).await?;
                    let value =
                        Self::value(Self::command(services, DesktopEnumCommand::Bootstrap).await?)?;
                    let DesktopEnumValue::Bootstrap(snapshot) = value else {
                        return Err(NativeGameCompositionError::Value(Box::new(value)));
                    };
                    Self::engine(snapshot.engine())?;
                    if let DesktopEnumEngineSnapshot::Ready { identity } = snapshot.engine() {
                        println!(
                            "Production UCI identity: {} by {}",
                            identity.name(),
                            identity.author()
                        );
                    }
                    let mut stream = NativeGameCompositionStream::subscribe(services).await?;
                    let mut completed = Vec::with_capacity(cases.len());
                    for case in cases {
                        completed.push(Self::complete(services, &mut stream, case).await?);
                    }
                    Ok((completed, services.resources.engine().to_path_buf()))
                })?;
            let package =
                EnginePackageSelector::load(&engine_directory, EnginePackageHost::detect()?)?;
            println!(
                "Verified engine: {} {} source {}; binary {}; embedded model {} ({} bytes), manifest SHA-256 {}",
                EnginePackageManifest::TAG,
                package.target().triple(),
                EnginePackageManifest::COMMIT,
                package.digest(),
                EnginePackageManifest::NETWORK_NAME,
                EnginePackageManifest::NETWORK_BYTES,
                EnginePackageManifest::NETWORK_SHA256,
            );
            let documents = Self::documents(&profile, &completed)?;
            Self::restore(&completed, &documents)?;
            Self::bootstrap(&profile, async |services| {
                Self::ready(services).await?;
                let mut stream = NativeGameCompositionStream::subscribe(services).await?;
                if !stream.games(services).await?.is_empty() {
                    return Err(NativeGameCompositionError::Invariant(
                        "completed library games do not reopen as live tabs on production relaunch",
                    ));
                }
                Ok(())
            })?;
            let reopened = Self::documents(&profile, &completed)?;
            Self::restore(&completed, &reopened)?;
            if documents.iter().zip(&reopened).any(|(before, after)| {
                before.revision != after.revision || before.body != after.body
            }) {
                return Err(NativeGameCompositionError::Invariant(
                    "production relaunch leaves both completed durable documents unchanged",
                ));
            }
            println!(
                "Production game composition passed: {} real-engine mate games, 2 post-result rejections, durable one-move restoration, frozen clocks, completed-tab exclusion on relaunch, full CPU lease recovery, no direct child processes, and exclusive profile reacquisition after both bootstraps",
                completed.len(),
            );
            Ok(())
        })();
        NativeGameCompositionError::finish(operation, directory.close().map_err(Into::into))
    }
}
