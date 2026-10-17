use std::io::Cursor;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::time::Duration;

use diesel::connection::SimpleConnection;
use rand::{SeedableRng, rngs::StdRng};
use tokio::sync::OwnedSemaphorePermit;
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
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationReceipt, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider,
};
use crate::desktop::game_editor::DesktopGameEditorPublication;
use crate::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryFactoryProvider, DesktopGameRegistryOptions,
    DesktopGameRegistryPort,
};
use crate::desktop::game_supervisor::DesktopGameSupervisorEngineFactory;
use crate::desktop::library_service::{
    DesktopLibraryService, DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumResult,
    DesktopLibraryServiceOperation, DesktopLibraryServiceOptions, DesktopLibraryServicePort,
    DesktopLibraryServiceReceipt, DesktopLibraryServiceRequest,
};
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId};
use crate::engine::live_game::EngineLiveGame;
use crate::engine::process_provider::EngineNativeProcess;
use crate::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use crate::game::engine_provider::GameEngineContext;
use crate::game::enums::{GameEnumMode, GameEnumSeat, GameId};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
use crate::game::storage_repository::{
    GameStorageRevision, GameStorageWallTimeError, GameStorageWallTimeProvider,
};
use crate::notation::pgn_import::NotationPgnImport;
use crate::storage::connection::StorageConnection;
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use crate::storage::game_document::StorageGameDocument;
use crate::storage::library_prepared::StorageLibraryEnumContentChange;
use crate::storage::library_repository::{
    StorageLibraryEnumMutationReceipt, StorageLibraryRepository,
};

pub(crate) type DesktopLibraryServiceTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
pub(crate) type DesktopLibraryServiceTestRegistry = DesktopGameRegistry<
    DesktopLibraryServiceTestFactory,
    DesktopLibraryServiceTestClock,
    DesktopLibraryServiceTestProof,
    StdRng,
>;

pub(crate) struct DesktopLibraryServiceTestClock;

impl ClockTimeProvider for DesktopLibraryServiceTestClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(50))
    }
}

struct DesktopLibraryServiceTestWake;

impl DesktopApplicationWakeProvider for DesktopLibraryServiceTestWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

pub(crate) struct DesktopLibraryServiceTestWall {
    pub(crate) millis: AtomicI64,
    pub(crate) calls: AtomicUsize,
}

impl GameStorageWallTimeProvider for DesktopLibraryServiceTestWall {
    fn now(&self) -> Result<GameLibraryOrganizationTimestamp, GameStorageWallTimeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(GameLibraryOrganizationTimestamp::new(
            self.millis.load(Ordering::SeqCst),
        )?)
    }
}

pub(crate) struct DesktopLibraryServiceTestProof;

impl GameAdjudicationMateProofProvider for DesktopLibraryServiceTestProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

pub(crate) struct DesktopLibraryServiceTestFactory {
    pub(crate) factories: Arc<AtomicUsize>,
    pub(crate) shutdowns: Arc<AtomicUsize>,
}

impl DesktopGameSupervisorEngineFactory for DesktopLibraryServiceTestFactory {
    type Engine = EngineLiveGame<EngineNativeProcess, DesktopLibraryServiceTestClock>;
    type Error = std::io::Error;
    async fn create(&mut self, _context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        Err(std::io::Error::other(
            "Local saved opening requested an engine",
        ))
    }
    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), Self::Error> {
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl DesktopGameRegistryFactoryProvider for DesktopLibraryServiceTestFactory {
    type Factory = Self;
    type Error = std::io::Error;
    fn factory(&mut self, _game: GameId) -> Result<Self::Factory, Self::Error> {
        self.factories.fetch_add(1, Ordering::SeqCst);
        Ok(Self {
            factories: Arc::clone(&self.factories),
            shutdowns: Arc::clone(&self.shutdowns),
        })
    }
}

pub(crate) struct DesktopLibraryServiceFixture {
    pub(crate) directory: tempfile::TempDir,
    pub(crate) budget: CapacityBudget,
    pub(crate) executor: Arc<CapacityExecutor>,
    pub(crate) store: StorageDocumentStore,
    pub(crate) wall: Arc<DesktopLibraryServiceTestWall>,
    pub(crate) registry: DesktopLibraryServiceTestRegistry,
    pub(crate) games: DesktopGameRegistryPort,
    pub(crate) service: DesktopLibraryService,
    pub(crate) port: DesktopLibraryServicePort,
    pub(crate) workspace: DesktopMessagesEntityId,
    pub(crate) factories: Arc<AtomicUsize>,
    pub(crate) shutdowns: Arc<AtomicUsize>,
    application: DesktopApplication<DesktopLibraryServiceTestClock, DesktopLibraryServiceTestWake>,
}

impl DesktopLibraryServiceFixture {
    pub(crate) async fn new() -> DesktopLibraryServiceTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store =
            StorageDocumentStore::open(directory.path().join("library.sqlite"), budget.clone())
                .await?;
        let wall = Arc::new(DesktopLibraryServiceTestWall {
            millis: AtomicI64::new(1000),
            calls: AtomicUsize::new(0),
        });
        let factories = Arc::new(AtomicUsize::new(0));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let (registry, games) = DesktopGameRegistry::new(DesktopGameRegistryOptions {
            factory_provider: DesktopLibraryServiceTestFactory {
                factories: Arc::clone(&factories),
                shutdowns: Arc::clone(&shutdowns),
            },
            storage: store.client(),
            executor: Arc::clone(&executor),
            clock: Arc::new(DesktopLibraryServiceTestClock),
            wall_time: Arc::<DesktopLibraryServiceTestWall>::clone(&wall),
            proof: Arc::new(DesktopLibraryServiceTestProof),
            random: StdRng::seed_from_u64(7),
        });
        let workspace = DesktopMessagesEntityId::new();
        let (service, port) = DesktopLibraryService::new(DesktopLibraryServiceOptions {
            workspace,
            storage: store.client(),
            executor: Arc::clone(&executor),
            games: games.clone(),
            wall_time: Arc::<DesktopLibraryServiceTestWall>::clone(&wall),
            imports: None,
        });
        Ok(Self {
            directory,
            budget,
            executor,
            store,
            wall,
            registry,
            games,
            service,
            port,
            workspace,
            factories,
            shutdowns,
            application: DesktopApplication::new(
                Arc::new(DesktopLibraryServiceTestClock),
                Arc::new(DesktopLibraryServiceTestWake),
            ),
        })
    }

    pub(crate) fn id(value: u128) -> DesktopLibraryServiceTestResult<GameId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    pub(crate) fn receipt(&self) -> DesktopLibraryServiceTestResult<DesktopApplicationReceipt> {
        let port = self.application.port();
        let request = port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
        let receipt = port.try_submit(&request)?;
        let work = self
            .application
            .try_next()?
            .ok_or("missing admitted application work")?;
        assert!(receipt.shares_outcome(work.receipt()));
        Ok(receipt)
    }

    pub(crate) fn submit(
        &self,
        action: DesktopLibraryServiceEnumAction,
    ) -> DesktopLibraryServiceTestResult<DesktopLibraryServiceReceipt> {
        Ok(self.port.try_submit(DesktopLibraryServiceRequest::new(
            self.receipt()?,
            DesktopLibraryServiceOperation::new(self.workspace, action),
        ))?)
    }

    pub(crate) async fn drive(
        &mut self,
        reply: &mut DesktopLibraryServiceReceipt,
    ) -> DesktopLibraryServiceTestResult<DesktopLibraryServiceEnumResult> {
        tokio::select! {
            result = reply.wait() => Ok(result?),
            result = Box::pin(self.service.run()) => Err(format!("service ended before reply: {result:?}").into()),
            result = Box::pin(self.registry.run()) => Err(format!("registry ended before reply: {result:?}").into()),
        }
    }

    pub(crate) async fn execute(
        &mut self,
        action: DesktopLibraryServiceEnumAction,
    ) -> DesktopLibraryServiceTestResult<DesktopLibraryServiceEnumResult> {
        let mut reply = self.submit(action)?;
        self.drive(&mut reply).await
    }

    pub(crate) async fn open(
        &mut self,
        id: u128,
        revision: GameStorageRevision,
    ) -> DesktopLibraryServiceTestResult<DesktopGameEditorPublication> {
        let DesktopLibraryServiceEnumResult::Opened(snapshot) = self
            .execute(DesktopLibraryServiceEnumAction::Open {
                game: Self::id(id)?,
                storage_revision: revision,
            })
            .await?
        else {
            return Err("missing editor opening".into());
        };
        Ok(snapshot)
    }

    pub(crate) fn played(id: u128) -> DesktopLibraryServiceTestResult<GameSession> {
        Ok(GameSession::new(GameSessionOptions {
            id: Self::id(id)?,
            root: uuid::Uuid::from_u128(1).to_string().parse()?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            clock: ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White),
            metadata: GameMetadata::default(),
        })?)
    }

    pub(crate) async fn seed(
        &self,
        id: u128,
        notation: bool,
    ) -> DesktopLibraryServiceTestResult<GameStorageRevision> {
        let game = Self::id(id)?;
        let body = if notation {
            let mut node = 0_u128;
            let mut parser = NotationPgnImport::new(
                Cursor::new(
                    "[TimeControl \"600+5\"]\n[WhiteElo \"unrated\"]\n\n{original} 1. e4 {[%clk 0:09:58]} (1. d4 d5) e5 *",
                ),
                move || {
                    node += 1;
                    uuid::Uuid::from_u128(node).to_string().parse()
                },
            );
            let game_record = parser
                .next_game(&|| false)?
                .ok_or("missing parsed notation")?;
            StorageGameDocument::capture_notation(game, &game_record)?.json()?
        } else {
            StorageGameDocument::capture(&Self::played(id)?, ClockInstant::from_nanos(0))?.json()?
        };
        self.seed_body(game, body).await
    }

    pub(crate) async fn seed_played(
        &self,
        session: &GameSession,
    ) -> DesktopLibraryServiceTestResult<GameStorageRevision> {
        self.seed_body(
            session.id(),
            StorageGameDocument::capture(session, ClockInstant::from_nanos(50))?.json()?,
        )
        .await
    }

    async fn seed_body(
        &self,
        game: GameId,
        body: String,
    ) -> DesktopLibraryServiceTestResult<GameStorageRevision> {
        let repository = StorageLibraryRepository::new(self.store.client(), &self.executor);
        let prepared = repository
            .prepare_write(
                game,
                GameStorageRevision::INITIAL,
                body,
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(
                    10,
                )?),
            )
            .await?;
        let StorageLibraryEnumMutationReceipt::Saved(receipt) =
            repository.begin_save(prepared)?.wait().await?
        else {
            return Err("missing seeded receipt".into());
        };
        Ok(receipt.content_revision())
    }

    pub(crate) async fn sql(&self, statement: &'static str) -> DesktopLibraryServiceTestResult {
        let path = self.directory.path().join("library.sqlite");
        self.executor
            .submit_waiting(
                move |_cancellation| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                    StorageConnection::open(&path)?
                        .diesel()
                        .batch_execute(statement)?;
                    Ok(())
                },
            )
            .await?
            .join()
            .await?
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(crate) fn fill_memory(&self) -> DesktopLibraryServiceTestResult<Vec<OwnedSemaphorePermit>> {
        let mut permits = Vec::new();
        let mut bytes = StorageDocumentStore::MEMORY_BUDGET;
        while bytes != 0 {
            match self.store.client().reserve_library_memory(bytes) {
                Ok(permit) => permits.push(permit),
                Err(StorageDocumentError::Saturated) => bytes /= 2,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(permits)
    }

    pub(crate) fn available_memory(&self) -> DesktopLibraryServiceTestResult<usize> {
        let permits = self.fill_memory()?;
        Ok(permits.iter().map(OwnedSemaphorePermit::num_permits).sum())
    }

    pub(crate) async fn close(mut self) -> DesktopLibraryServiceTestResult {
        self.service
            .shutdown_retained(Instant::now() + Duration::from_secs(3))
            .await?;
        assert!(self.service.is_shutdown_complete());
        Box::pin(
            self.registry
                .shutdown_retained(Instant::now() + Duration::from_secs(3)),
        )
        .await?;
        assert!(self.registry.is_shutdown_complete());
        assert_eq!(
            self.factories.load(Ordering::SeqCst),
            self.shutdowns.load(Ordering::SeqCst)
        );
        drop((self.service, self.registry, self.port, self.games));
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|_executor| "executor ownership remains")?
            .shutdown()
            .await?;
        Ok(())
    }
}
