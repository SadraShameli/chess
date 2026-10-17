use std::num::NonZeroUsize;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::{ClockDurationError, ClockInstant};
use chess::clock::time_control::ClockTimeControl;
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::game_bridge::{DesktopGameBridgeEnumOperation, DesktopGameBridgeRequest};
use chess::desktop::game_commands::DesktopGameCommandsEnumAction;
use chess::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryFactoryProvider, DesktopGameRegistryOptions,
    DesktopGameRegistryPort,
};
use chess::desktop::game_supervisor::DesktopGameSupervisorEngineFactory;
use chess::desktop::identity::DesktopRequestId;
use chess::desktop::messages::DesktopMessagesEntityId;
use chess::engine::live_game::EngineLiveGame;
use chess::engine::process_provider::EngineNativeProcess;
use chess::game::adjudication::{
    GameAdjudicationEnumInterruption, GameAdjudicationEnumMateProof,
    GameAdjudicationEnumMateQuestion, GameAdjudicationMateProofProvider,
};
use chess::game::configuration::{
    GameConfiguration, GameConfigurationEnumOpponent, GameConfigurationEnumStart,
    GameConfigurationOptions,
};
use chess::game::engine_provider::GameEngineContext;
use chess::game::enums::GameId;
use chess::storage::document_store::StorageDocumentStore;
use rand::{SeedableRng, rngs::StdRng};
use tokio::sync::Notify;
use tokio::time::Instant;

pub(crate) type DesktopGameRegistryTestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Default)]
pub(crate) struct DesktopGameRegistryFixtureClock(AtomicU64);

impl DesktopGameRegistryFixtureClock {
    pub(crate) fn advance_to(&self, instant: ClockInstant) -> Result<(), ClockDurationError> {
        self.0.fetch_max(
            instant
                .elapsed_since(ClockInstant::from_nanos(0))?
                .as_nanos(),
            Ordering::SeqCst,
        );
        Ok(())
    }
}

impl ClockTimeProvider for DesktopGameRegistryFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.0.load(Ordering::SeqCst)))
    }
}
#[derive(Default)]
pub(crate) struct DesktopGameRegistryFixtureProof(
    pub(crate) Option<Arc<DesktopGameRegistryFixtureGate>>,
);

#[derive(Default)]
pub(crate) struct DesktopGameRegistryFixtureGate {
    pub(crate) armed: AtomicBool,
    pub(crate) entered: Notify,
    released: Mutex<bool>,
    release: Condvar,
}

impl DesktopGameRegistryFixtureGate {
    fn wait(&self) -> bool {
        if !self.armed.swap(false, Ordering::SeqCst) {
            return true;
        }
        self.entered.notify_one();
        let Ok(mut released) = self.released.lock() else {
            return false;
        };
        while !*released {
            let Ok(next) = self.release.wait(released) else {
                return false;
            };
            released = next;
        }
        true
    }

    pub(crate) fn release(&self) -> DesktopGameRegistryTestResult {
        *self
            .released
            .lock()
            .map_err(|_poisoned| "proof gate poisoned")? = true;
        self.release.notify_all();
        Ok(())
    }
}
impl GameAdjudicationMateProofProvider for DesktopGameRegistryFixtureProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        if self.0.as_ref().is_none_or(|gate| gate.wait()) {
            GameAdjudicationEnumMateProof::Possible
        } else {
            GameAdjudicationEnumMateProof::Unknown(GameAdjudicationEnumInterruption::Unresolved)
        }
    }
}
pub(crate) struct DesktopGameRegistryFixtureFactory;
impl DesktopGameSupervisorEngineFactory for DesktopGameRegistryFixtureFactory {
    type Engine = EngineLiveGame<EngineNativeProcess, DesktopGameRegistryFixtureClock>;
    type Error = std::io::Error;
    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        Err(std::io::Error::other(
            "Local registry game requested an engine",
        ))
    }
    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        Ok(())
    }
}
#[derive(Default)]
pub(crate) struct DesktopGameRegistryFixtureFactories {
    pub(crate) admitted: Arc<Notify>,
}
impl DesktopGameRegistryFactoryProvider for DesktopGameRegistryFixtureFactories {
    type Factory = DesktopGameRegistryFixtureFactory;
    type Error = std::io::Error;
    fn factory(&mut self, _game: GameId) -> Result<Self::Factory, Self::Error> {
        self.admitted.notify_one();
        Ok(DesktopGameRegistryFixtureFactory)
    }
}
pub(crate) struct DesktopGameRegistryFixture;
impl DesktopGameRegistryFixture {
    pub(crate) fn create() -> DesktopGameBridgeRequest {
        DesktopGameBridgeRequest {
            request_id: DesktopRequestId::new(),
            received_at: ClockInstant::from_nanos(0),
            operation: DesktopGameBridgeEnumOperation::Create {
                owner: DesktopMessagesEntityId::new(),
                configuration: GameConfiguration::new(
                    GameConfigurationEnumStart::Standard,
                    GameConfigurationEnumOpponent::LocalTwoPlayer,
                    ClockTimeControl::initial_default(),
                    GameConfigurationOptions::initial_default(),
                ),
            },
        }
    }
    pub(crate) fn session(
        snapshot: &chess::desktop::game_snapshot::DesktopGameSnapshot,
        action: DesktopGameCommandsEnumAction,
    ) -> DesktopGameBridgeRequest {
        DesktopGameBridgeRequest {
            request_id: DesktopRequestId::new(),
            received_at: ClockInstant::from_nanos(0),
            operation: DesktopGameBridgeEnumOperation::Session {
                game: snapshot.id(),
                revision: snapshot.revision(),
                action,
            },
        }
    }
}

pub(crate) type DesktopGameRegistryFixtureActor = DesktopGameRegistry<
    DesktopGameRegistryFixtureFactories,
    DesktopGameRegistryFixtureClock,
    DesktopGameRegistryFixtureProof,
    StdRng,
>;

pub(crate) struct DesktopGameRegistryFixtureRuntime {
    directory: tempfile::TempDir,
    pub(crate) budget: CapacityBudget,
    pub(crate) executor: Arc<CapacityExecutor>,
    pub(crate) store: StorageDocumentStore,
    pub(crate) clock: Arc<DesktopGameRegistryFixtureClock>,
    proof: Arc<DesktopGameRegistryFixtureProof>,
    pub(crate) admitted: Arc<Notify>,
}

impl DesktopGameRegistryFixtureRuntime {
    pub(crate) async fn new(
        proof: DesktopGameRegistryFixtureProof,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(
            NonZeroUsize::new(chess::capacity::phase::CapacityPhase::MINIMUM_WORKERS)
                .ok_or("invalid capacity")?,
        )?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store =
            StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget.clone())
                .await?;
        Ok(Self {
            directory,
            budget,
            executor,
            store,
            clock: Arc::new(DesktopGameRegistryFixtureClock::default()),
            proof: Arc::new(proof),
            admitted: Arc::new(Notify::new()),
        })
    }

    pub(crate) fn database_path(&self) -> std::path::PathBuf {
        self.directory.path().join("library.sqlite3")
    }

    pub(crate) fn registry(&self) -> (DesktopGameRegistryFixtureActor, DesktopGameRegistryPort) {
        DesktopGameRegistry::new(self.options())
    }

    pub(crate) fn options(
        &self,
    ) -> DesktopGameRegistryOptions<
        DesktopGameRegistryFixtureFactories,
        DesktopGameRegistryFixtureClock,
        DesktopGameRegistryFixtureProof,
        StdRng,
    > {
        DesktopGameRegistryOptions {
            factory_provider: DesktopGameRegistryFixtureFactories {
                admitted: Arc::clone(&self.admitted),
            },
            storage: self.store.client(),
            executor: Arc::clone(&self.executor),
            clock: Arc::clone(&self.clock),
            wall_time: crate::game_storage_wall_time_fixture::GameStorageWallTimeFixture::shared(),
            proof: Arc::clone(&self.proof),
            random: StdRng::seed_from_u64(81),
        }
    }

    pub(crate) async fn close(self) -> DesktopGameRegistryTestResult {
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|_owner| "executor still borrowed")?
            .shutdown()
            .await?;
        self.directory.close()?;
        Ok(())
    }
}
