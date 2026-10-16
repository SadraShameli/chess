use std::collections::{HashMap, VecDeque};
use std::future::{Future, pending, poll_fn};
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::pin::Pin;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use std::task::{Context, Poll};
use std::time::Duration;

use rand::Rng;
use thiserror::Error;
use tokio::sync::{RwLock, mpsc, oneshot, watch};
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout_at};

use crate::board::start::BoardStartError;
use crate::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use crate::capacity::phase::CapacityPhaseGeneration;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::state::ClockState;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::game_bridge::{
    DesktopGameBridgeEnumOperation, DesktopGameBridgeFuture, DesktopGameBridgeProvider,
    DesktopGameBridgeRequest,
};
use crate::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::game_supervisor::{
    DesktopGameSupervisor, DesktopGameSupervisorEngineFactory, DesktopGameSupervisorError,
    DesktopGameSupervisorEvent, DesktopGameSupervisorOptions, DesktopGameSupervisorPort,
    DesktopGameSupervisorReceipt,
};
use crate::desktop::identity::DesktopRequestId;
use crate::game::adjudication::GameAdjudicationMateProofProvider;
use crate::game::configuration::GameConfiguration;
use crate::game::engine_provider::GameEngineProvider;
use crate::game::enums::{GameId, GameIdentityError, GameMoveId, GameRevisionError};
use crate::game::pause::{GamePauseEnumCause, GamePauseEnumFault};
use crate::game::session::{GameMetadata, GameSession, GameSessionError, GameSessionOptions};
use crate::game::storage_repository::GameStorageRevision;
use crate::game::takeback::GameTakebackEnumConsent;
use crate::storage::document_store::StorageDocumentClient;

#[derive(Debug, Error)]
pub enum DesktopGameRegistryError {
    #[error("The game registry cannot admit another request")]
    Busy,
    #[error("The game registry is closed")]
    Closed,
    #[error("The game is absent from the running registry")]
    UnknownGame,
    #[error("A game with this identity is already registered")]
    DuplicateGame,
    #[error("The game registry received a session request through its creation queue")]
    InvalidCreate,
    #[error("The game registry has no matching preparation generation")]
    PreparationGeneration,
    #[error("The game registry random source is poisoned")]
    RandomPoisoned,
    #[error("The game registry creation response is unavailable")]
    MissingCreation,
    #[error("The game factory could not be constructed: {source}")]
    Factory {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("The game operation failed: {0}")]
    Supervisor(#[source] Arc<DesktopGameSupervisorError>),
    #[error("The retained registry control failed: {0}")]
    Control(#[source] Arc<Self>),
    #[error("The game registry response channel closed: {0}")]
    Response(#[from] oneshot::error::RecvError),
    #[error("The game registry retained shutdown exceeded its deadline: {0}")]
    Deadline(#[source] tokio::time::error::Elapsed),
    #[error("The retained game task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error("The game registry operation failed ({operation}); cleanup also failed ({cleanup})")]
    Cleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
    #[error(transparent)]
    Executor(#[from] CapacityExecutorError),
    #[error(transparent)]
    Identity(#[from] GameIdentityError),
    #[error(transparent)]
    Session(#[from] GameSessionError),
    #[error(transparent)]
    Start(#[from] BoardStartError),
    #[error(transparent)]
    Time(#[from] ClockDurationError),
}

impl DesktopGameRegistryError {
    fn record(failure: &mut Option<Self>, error: Self) {
        *failure = Some(match failure.take() {
            Some(operation) => Self::Cleanup {
                operation: Box::new(operation),
                cleanup: Box::new(error),
            },
            None => error,
        });
    }

    fn failure(&self) -> DesktopFailure {
        let code = match self {
            Self::Busy => DesktopEnumFailureCode::ResourceRefusal,
            Self::Closed => DesktopEnumFailureCode::Cancelled,
            Self::UnknownGame | Self::DuplicateGame | Self::InvalidCreate => {
                DesktopEnumFailureCode::InvalidInput
            }
            Self::Supervisor(error) => {
                if matches!(error.as_ref(), DesktopGameSupervisorError::Revision(_)) {
                    DesktopEnumFailureCode::StaleRevision
                } else if matches!(error.as_ref(), DesktopGameSupervisorError::Busy) {
                    DesktopEnumFailureCode::ResourceRefusal
                } else if matches!(error.as_ref(), DesktopGameSupervisorError::Closed) {
                    DesktopEnumFailureCode::Cancelled
                } else {
                    DesktopEnumFailureCode::InternalFailure
                }
            }
            Self::PreparationGeneration
            | Self::RandomPoisoned
            | Self::MissingCreation
            | Self::Factory { .. }
            | Self::Control(_)
            | Self::Response(_)
            | Self::Deadline(_)
            | Self::Task(_)
            | Self::Cleanup { .. }
            | Self::Executor(_)
            | Self::Identity(_)
            | Self::Session(_)
            | Self::Start(_)
            | Self::Time(_) => DesktopEnumFailureCode::InternalFailure,
        };
        DesktopFailure::new(code, self.to_string(), true)
    }
}

pub trait DesktopGameRegistryFactoryProvider: Send {
    type Factory: DesktopGameSupervisorEngineFactory;
    type Error: std::error::Error + Send + Sync + 'static;

    fn factory(&mut self, game: GameId) -> Result<Self::Factory, Self::Error>;
}

pub struct DesktopGameRegistryOptions<F, C, P, R> {
    pub factory_provider: F,
    pub storage: StorageDocumentClient,
    pub executor: Arc<CapacityExecutor>,
    pub clock: Arc<C>,
    pub proof: Arc<P>,
    pub random: R,
}

pub struct DesktopGameRegistryRestored {
    pub session: GameSession,
    pub storage_revision: GameStorageRevision,
}

struct DesktopGameRegistryCreate {
    request: DesktopGameBridgeRequest,
    response: oneshot::Sender<Result<DesktopGameSnapshot, DesktopFailure>>,
}

struct DesktopGameRegistryBroadcastOptions {
    port: DesktopGameSupervisorPort,
    pending_receipt: Option<DesktopGameSupervisorReceipt>,
    action: DesktopGameCommandsEnumAction,
    received_at: ClockInstant,
    deadline: Instant,
}

#[derive(Clone, Copy)]
enum DesktopGameRegistryEnumControl {
    Quiesce {
        generation: CapacityPhaseGeneration,
        deadline: Instant,
    },
    Resume {
        generation: CapacityPhaseGeneration,
        deadline: Instant,
    },
    Shutdown {
        deadline: Instant,
    },
    PauseGraphics {
        received_at: ClockInstant,
        deadline: Instant,
    },
    RecoverGraphics {
        deadline: Instant,
    },
}

impl DesktopGameRegistryEnumControl {
    const fn index(self) -> u32 {
        match self {
            Self::Quiesce { .. } => 0,
            Self::Resume { .. } => 1,
            Self::Shutdown { .. } => 2,
            Self::PauseGraphics { .. } => 3,
            Self::RecoverGraphics { .. } => 4,
        }
    }
}

struct DesktopGameRegistryControl {
    operation: DesktopGameRegistryEnumControl,
    response: oneshot::Sender<Result<(), Arc<DesktopGameRegistryError>>>,
    _slot: DesktopGameRegistryControlSlot,
}

struct DesktopGameRegistryControlSlot {
    held: Arc<AtomicU8>,
    mask: u8,
}

impl Drop for DesktopGameRegistryControlSlot {
    fn drop(&mut self) {
        self.held.fetch_and(!self.mask, Ordering::AcqRel);
    }
}

pub struct DesktopGameRegistryReceipt {
    response: oneshot::Receiver<Result<(), Arc<DesktopGameRegistryError>>>,
}

impl DesktopGameRegistryReceipt {
    pub async fn wait(self) -> Result<(), Arc<DesktopGameRegistryError>> {
        self.response
            .await
            .map_err(|error| Arc::new(DesktopGameRegistryError::Response(error)))?
    }
}

#[derive(Clone)]
pub struct DesktopGameRegistryPort {
    creates: mpsc::Sender<DesktopGameRegistryCreate>,
    controls: mpsc::Sender<DesktopGameRegistryControl>,
    held_controls: Arc<AtomicU8>,
    sessions: Arc<RwLock<HashMap<GameId, DesktopGameSupervisorPort>>>,
    snapshots: watch::Receiver<Vec<DesktopGameSnapshot>>,
    closed: Arc<AtomicBool>,
}

impl DesktopGameRegistryPort {
    const CREATE_CAPACITY: usize = 32;
    const CONTROL_CAPACITY: usize = 5;

    pub fn quiesce_all(
        &self,
        generation: CapacityPhaseGeneration,
        deadline: Instant,
    ) -> Result<DesktopGameRegistryReceipt, DesktopGameRegistryError> {
        self.control(DesktopGameRegistryEnumControl::Quiesce {
            generation,
            deadline,
        })
    }

    pub fn resume_all(
        &self,
        generation: CapacityPhaseGeneration,
        deadline: Instant,
    ) -> Result<DesktopGameRegistryReceipt, DesktopGameRegistryError> {
        self.control(DesktopGameRegistryEnumControl::Resume {
            generation,
            deadline,
        })
    }

    pub fn shutdown_all(
        &self,
        deadline: Instant,
    ) -> Result<DesktopGameRegistryReceipt, DesktopGameRegistryError> {
        self.control(DesktopGameRegistryEnumControl::Shutdown { deadline })
    }

    pub fn pause_graphics_all(
        &self,
        received_at: ClockInstant,
        deadline: Instant,
    ) -> Result<DesktopGameRegistryReceipt, DesktopGameRegistryError> {
        self.control(DesktopGameRegistryEnumControl::PauseGraphics {
            received_at,
            deadline,
        })
    }

    pub fn recover_graphics_all(
        &self,
        deadline: Instant,
    ) -> Result<DesktopGameRegistryReceipt, DesktopGameRegistryError> {
        self.control(DesktopGameRegistryEnumControl::RecoverGraphics { deadline })
    }

    fn control(
        &self,
        operation: DesktopGameRegistryEnumControl,
    ) -> Result<DesktopGameRegistryReceipt, DesktopGameRegistryError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DesktopGameRegistryError::Closed);
        }
        let mask = 1_u8
            .checked_shl(operation.index())
            .ok_or(DesktopGameRegistryError::Busy)?;
        self.held_controls
            .try_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                (held & mask == 0).then_some(held | mask)
            })
            .map_err(|_held| DesktopGameRegistryError::Busy)?;
        let (response, receiver) = oneshot::channel();
        self.controls
            .try_send(DesktopGameRegistryControl {
                operation,
                response,
                _slot: DesktopGameRegistryControlSlot {
                    held: Arc::clone(&self.held_controls),
                    mask,
                },
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => DesktopGameRegistryError::Busy,
                mpsc::error::TrySendError::Closed(_) => DesktopGameRegistryError::Closed,
            })?;
        Ok(DesktopGameRegistryReceipt { response: receiver })
    }
}

impl DesktopGameBridgeProvider for DesktopGameRegistryPort {
    fn execute(&self, request: DesktopGameBridgeRequest) -> DesktopGameBridgeFuture {
        let port = self.clone();
        Box::pin(async move {
            if port.closed.load(Ordering::Acquire)
                && !matches!(
                    &request.operation,
                    DesktopGameBridgeEnumOperation::Session {
                        action: DesktopGameCommandsEnumAction::RetrySave,
                        ..
                    }
                )
            {
                return Err(DesktopGameRegistryError::Closed.failure());
            }
            match request.operation {
                DesktopGameBridgeEnumOperation::Session {
                    game,
                    revision,
                    action,
                } => {
                    let supervisor = port
                        .sessions
                        .read()
                        .await
                        .get(&game)
                        .cloned()
                        .ok_or_else(|| DesktopGameRegistryError::UnknownGame.failure())?;
                    let receipt = supervisor
                        .try_submit(DesktopGameCommandsRequest {
                            request_id: request.request_id,
                            expected_revision: revision,
                            received_at: request.received_at,
                            action,
                        })
                        .map_err(|error| {
                            DesktopGameRegistryError::Supervisor(Arc::new(error)).failure()
                        })?;
                    receipt
                        .wait()
                        .await
                        .map_err(|error| DesktopGameRegistryError::Supervisor(error).failure())
                }
                DesktopGameBridgeEnumOperation::Create {
                    owner,
                    configuration,
                } => {
                    let (response, receiver) = oneshot::channel();
                    port.creates
                        .try_send(DesktopGameRegistryCreate {
                            request: DesktopGameBridgeRequest {
                                request_id: request.request_id,
                                received_at: request.received_at,
                                operation: DesktopGameBridgeEnumOperation::Create {
                                    owner,
                                    configuration,
                                },
                            },
                            response,
                        })
                        .map_err(|error| match error {
                            mpsc::error::TrySendError::Full(_) => {
                                DesktopGameRegistryError::Busy.failure()
                            }
                            mpsc::error::TrySendError::Closed(_) => {
                                DesktopGameRegistryError::Closed.failure()
                            }
                        })?;
                    receiver
                        .await
                        .map_err(|error| DesktopGameRegistryError::Response(error).failure())?
                }
            }
        })
    }

    fn snapshots(&self) -> watch::Receiver<Vec<DesktopGameSnapshot>> {
        self.snapshots.clone()
    }
}

type DesktopGameRegistryActor<F, C, P> =
    DesktopGameSupervisor<<F as DesktopGameRegistryFactoryProvider>::Factory, C, P>;
type DesktopGameRegistryActorOutcome<F, C, P> = (
    DesktopGameRegistryActor<F, C, P>,
    Result<(), DesktopGameSupervisorError>,
);
type DesktopGameRegistryCreateFuture<F, C, P> = Pin<
    Box<
        dyn Future<
                Output = Result<
                    (DesktopGameRegistryActor<F, C, P>, DesktopGameSupervisorPort),
                    DesktopGameRegistryError,
                >,
            > + Send,
    >,
>;
type DesktopGameRegistryActivationFuture =
    Pin<Box<dyn Future<Output = Result<DesktopGameSnapshot, DesktopGameRegistryError>> + Send>>;
type DesktopGameRegistryWatchFuture = Pin<
    Box<
        dyn Future<
                Output = (
                    GameId,
                    watch::Receiver<DesktopGameSupervisorEvent>,
                    Result<(), watch::error::RecvError>,
                ),
            > + Send,
    >,
>;

pub struct DesktopGameRegistry<F: DesktopGameRegistryFactoryProvider, C, P, R> {
    factory_provider: F,
    storage: StorageDocumentClient,
    executor: Arc<CapacityExecutor>,
    clock: Arc<C>,
    proof: Arc<P>,
    random: Arc<Mutex<R>>,
    sessions: Arc<RwLock<HashMap<GameId, DesktopGameSupervisorPort>>>,
    creates: mpsc::Receiver<DesktopGameRegistryCreate>,
    controls: mpsc::Receiver<DesktopGameRegistryControl>,
    creating: Option<DesktopGameRegistryCreateFuture<F, C, P>>,
    installing: VecDeque<(DesktopGameRegistryActor<F, C, P>, DesktopGameSupervisorPort)>,
    activating: Option<DesktopGameRegistryActivationFuture>,
    creation_response: Option<oneshot::Sender<Result<DesktopGameSnapshot, DesktopFailure>>>,
    tasks: JoinSet<DesktopGameRegistryActorOutcome<F, C, P>>,
    retained: Vec<DesktopGameRegistryActor<F, C, P>>,
    watches: Vec<DesktopGameRegistryWatchFuture>,
    snapshots: watch::Sender<Vec<DesktopGameSnapshot>>,
    closed: Arc<AtomicBool>,
    preparation: Option<CapacityPhaseGeneration>,
    creates_closed: bool,
    controls_closed: bool,
    stopping: bool,
}

impl<F, C, P, R> DesktopGameRegistry<F, C, P, R>
where
    F: DesktopGameRegistryFactoryProvider,
    F::Factory: 'static,
    <F::Factory as DesktopGameSupervisorEngineFactory>::Engine: 'static,
    <<F::Factory as DesktopGameSupervisorEngineFactory>::Engine as GameEngineProvider>::Error:
        'static,
    C: ClockTimeProvider + RefUnwindSafe + 'static,
    P: GameAdjudicationMateProofProvider + Send + Sync + RefUnwindSafe + 'static,
    R: Rng + Send + UnwindSafe + 'static,
{
    const CONTROL_TIMEOUT: Duration = Duration::from_secs(3);

    pub fn new(options: DesktopGameRegistryOptions<F, C, P, R>) -> (Self, DesktopGameRegistryPort) {
        let sessions = Arc::new(RwLock::new(HashMap::new()));
        let (creates, create_receiver) = mpsc::channel(DesktopGameRegistryPort::CREATE_CAPACITY);
        let (controls, control_receiver) = mpsc::channel(DesktopGameRegistryPort::CONTROL_CAPACITY);
        let (snapshots, snapshot_receiver) = watch::channel(Vec::new());
        let closed = Arc::new(AtomicBool::new(false));
        let port = DesktopGameRegistryPort {
            creates,
            controls,
            held_controls: Arc::new(AtomicU8::new(0)),
            sessions: Arc::clone(&sessions),
            snapshots: snapshot_receiver,
            closed: Arc::clone(&closed),
        };
        (
            Self {
                factory_provider: options.factory_provider,
                storage: options.storage,
                executor: options.executor,
                clock: options.clock,
                proof: options.proof,
                random: Arc::new(Mutex::new(options.random)),
                sessions,
                creates: create_receiver,
                controls: control_receiver,
                creating: None,
                installing: VecDeque::new(),
                activating: None,
                creation_response: None,
                tasks: JoinSet::new(),
                retained: Vec::new(),
                watches: Vec::new(),
                snapshots,
                closed,
                preparation: None,
                creates_closed: false,
                controls_closed: false,
                stopping: false,
            },
            port,
        )
    }

    pub async fn restore(
        &mut self,
        restored: DesktopGameRegistryRestored,
    ) -> Result<(), DesktopGameRegistryError> {
        let game = restored.session.id();
        if self.sessions.read().await.contains_key(&game) {
            return Err(DesktopGameRegistryError::DuplicateGame);
        }
        let factory = self.factory_provider.factory(game).map_err(|source| {
            DesktopGameRegistryError::Factory {
                source: Box::new(source),
            }
        })?;
        let search_config = restored.session.engine_search_config();
        let (actor, port) = DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
            session: restored.session,
            storage_revision: restored.storage_revision,
            storage: self.storage.clone(),
            executor: Arc::clone(&self.executor),
            clock: Arc::clone(&self.clock),
            proof: Arc::clone(&self.proof),
            factory,
            search_config,
        })
        .await
        .map_err(|error| DesktopGameRegistryError::Supervisor(Arc::new(error)))?;
        self.install(actor, port, Instant::now() + Self::CONTROL_TIMEOUT)
            .await
    }

    pub async fn run(&mut self) -> Result<(), DesktopGameRegistryError> {
        let operation = self.run_loop().await;
        if operation.is_err() {
            self.closed.store(true, Ordering::Release);
        }
        operation
    }

    async fn run_loop(&mut self) -> Result<(), DesktopGameRegistryError> {
        loop {
            if self.stopping
                && self.tasks.is_empty()
                && self.creating.is_none()
                && self.activating.is_none()
            {
                return if self.retained.is_empty() {
                    Ok(())
                } else {
                    self.cleanup_retained(Instant::now() + Self::CONTROL_TIMEOUT)
                        .await
                };
            }
            if self.creates_closed && self.controls_closed && !self.stopping {
                self.shutdown_retained(Instant::now() + Self::CONTROL_TIMEOUT)
                    .await?;
                return Ok(());
            }
            tokio::select! {
                biased;
                control = self.controls.recv(), if !self.controls_closed && !self.stopping => {
                    match control {
                        Some(control) => self.handle_control(control).await?,
                        None => self.controls_closed = true,
                    }
                }
                result = Self::creation_ready(&mut self.creating), if self.creating.is_some() => {
                    self.creating = None;
                    self.finish_creation(result, Instant::now() + Self::CONTROL_TIMEOUT).await?;
                }
                result = Self::activation_ready(&mut self.activating), if self.activating.is_some() => {
                    self.activating = None;
                    self.finish_activation(result)?;
                }
                task = self.tasks.join_next(), if !self.tasks.is_empty() => {
                    if let Some(result) = task { self.observe_actor(result)?; }
                }
                watched = poll_fn(|context| Self::poll_watches(&mut self.watches, context)), if !self.watches.is_empty() => {
                    let (game, receiver, result) = watched;
                    self.observe_snapshot(game, receiver, result);
                }
                creation = self.creates.recv(), if !self.creates_closed && self.creating.is_none() && self.activating.is_none() && !self.stopping => {
                    match creation {
                        Some(creation) => self.begin_creation(creation),
                        None => self.creates_closed = true,
                    }
                }
            }
        }
    }

    fn begin_creation(&mut self, creation: DesktopGameRegistryCreate) {
        let result = self.prepare_creation(creation.request);
        match result {
            Ok(future) => {
                self.creating = Some(future);
                self.creation_response = Some(creation.response);
            }
            Err(error) => {
                if let Err(abandoned) = creation.response.send(Err(error.failure())) {
                    drop(abandoned);
                }
            }
        }
    }

    fn prepare_creation(
        &mut self,
        request: DesktopGameBridgeRequest,
    ) -> Result<DesktopGameRegistryCreateFuture<F, C, P>, DesktopGameRegistryError> {
        let configuration = match request.operation {
            DesktopGameBridgeEnumOperation::Create {
                configuration,
                owner: _,
            } => configuration,
            DesktopGameBridgeEnumOperation::Session { .. } => {
                return Err(DesktopGameRegistryError::InvalidCreate);
            }
        };
        let game = uuid::Uuid::new_v4().to_string().parse()?;
        let root = uuid::Uuid::new_v4().to_string().parse()?;
        let factory = self.factory_provider.factory(game).map_err(|source| {
            DesktopGameRegistryError::Factory {
                source: Box::new(source),
            }
        })?;
        let random = Arc::clone(&self.random);
        let executor = Arc::clone(&self.executor);
        let storage = self.storage.clone();
        let clock = Arc::clone(&self.clock);
        let proof = Arc::clone(&self.proof);
        Ok(Box::pin(async move {
            let session = executor
                .submit_waiting(move |_cancellation| {
                    let mut random = random
                        .lock()
                        .map_err(|_poisoned| DesktopGameRegistryError::RandomPoisoned)?;
                    Self::session(game, root, configuration, &mut random)
                })
                .await?
                .join()
                .await??;
            let search_config = session.engine_search_config();
            DesktopGameSupervisor::new(DesktopGameSupervisorOptions {
                session,
                storage_revision: GameStorageRevision::INITIAL,
                storage,
                executor,
                clock,
                proof,
                factory,
                search_config,
            })
            .await
            .map_err(|error| DesktopGameRegistryError::Supervisor(Arc::new(error)))
        }))
    }

    fn session(
        game: GameId,
        root: GameMoveId,
        configuration: GameConfiguration,
        random: &mut R,
    ) -> Result<GameSession, DesktopGameRegistryError> {
        let start = configuration.start().resolve(random)?;
        let clock = ClockState::new(
            configuration.time_control().clone(),
            start.position().turn(),
        );
        let mut session = GameSession::new(GameSessionOptions {
            id: game,
            root,
            start,
            seats: configuration.seats()?,
            clock,
            metadata: GameMetadata::default(),
        })?;
        session.configure_play_options(
            session.revision(),
            configuration.options().engine_search(),
            configuration.options().is_auto_flip_enabled(),
        )?;
        if !configuration.options().requires_local_takeback_consent() {
            session.configure_takeback_consent(
                session.revision(),
                GameTakebackEnumConsent::NotRequired,
            )?;
        }
        if configuration.options().is_premove_enabled() {
            session.set_premoves_enabled(session.revision(), true)?;
        }
        Ok(session)
    }

    async fn finish_creation(
        &mut self,
        result: Result<
            (DesktopGameRegistryActor<F, C, P>, DesktopGameSupervisorPort),
            DesktopGameRegistryError,
        >,
        deadline: Instant,
    ) -> Result<(), DesktopGameRegistryError> {
        let activation = match result {
            Ok((actor, port)) => {
                let snapshot = port.snapshot().snapshot;
                let installed = self.install(actor, port.clone(), deadline).await;
                match installed {
                    Ok(()) if self.closed.load(Ordering::Acquire) => {
                        Err(DesktopGameRegistryError::Closed)
                    }
                    Ok(()) => self
                        .clock
                        .now()
                        .map_err(DesktopGameRegistryError::Time)
                        .and_then(|received_at| {
                            port.try_submit(DesktopGameCommandsRequest {
                                request_id: DesktopRequestId::new(),
                                expected_revision: snapshot.revision(),
                                received_at,
                                action: DesktopGameCommandsEnumAction::Start,
                            })
                            .map_err(|error| DesktopGameRegistryError::Supervisor(Arc::new(error)))
                        }),
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        };
        match activation {
            Ok(receipt) => {
                self.activating = Some(Box::pin(async move {
                    receipt
                        .wait()
                        .await
                        .map_err(DesktopGameRegistryError::Supervisor)
                }));
                Ok(())
            }
            Err(error) => self.finish_activation(Err(error)),
        }
    }

    fn finish_activation(
        &mut self,
        result: Result<DesktopGameSnapshot, DesktopGameRegistryError>,
    ) -> Result<(), DesktopGameRegistryError> {
        let response = self
            .creation_response
            .take()
            .ok_or(DesktopGameRegistryError::MissingCreation)?;
        if let Ok(snapshot) = &result {
            self.publish(snapshot.clone());
        }
        if let Err(abandoned) = response.send(result.map_err(|error| error.failure())) {
            drop(abandoned);
        }
        Ok(())
    }

    async fn install(
        &mut self,
        actor: DesktopGameRegistryActor<F, C, P>,
        port: DesktopGameSupervisorPort,
        deadline: Instant,
    ) -> Result<(), DesktopGameRegistryError> {
        self.installing.push_back((actor, port));
        self.finish_installations(deadline).await
    }

    async fn finish_installations(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopGameRegistryError> {
        while !self.installing.is_empty() {
            let mut sessions = timeout_at(deadline, self.sessions.write())
                .await
                .map_err(DesktopGameRegistryError::Deadline)?;
            let Some((mut actor, port)) = self.installing.pop_front() else {
                break;
            };
            let snapshot = port.snapshot().snapshot;
            let game = snapshot.id();
            let receiver = port.events();
            sessions.insert(game, port.clone());
            drop(sessions);
            self.tasks.spawn(async move {
                let result = actor.run().await;
                (actor, result)
            });
            self.watch(game, receiver);
            self.publish(snapshot);
            if let Some(generation) = self.preparation {
                let receipt = port
                    .try_submit(DesktopGameCommandsRequest {
                        request_id: DesktopRequestId::new(),
                        expected_revision: port.snapshot().snapshot.revision(),
                        received_at: self.clock.now()?,
                        action: DesktopGameCommandsEnumAction::QuiesceForPreparation { generation },
                    })
                    .map_err(|error| DesktopGameRegistryError::Supervisor(Arc::new(error)))?;
                timeout_at(deadline, receipt.wait())
                    .await
                    .map_err(DesktopGameRegistryError::Deadline)?
                    .map_err(DesktopGameRegistryError::Supervisor)?;
            }
        }
        Ok(())
    }

    pub(crate) async fn forward_next_snapshot(&mut self) {
        let (game, receiver, result) =
            poll_fn(|context| Self::poll_watches(&mut self.watches, context)).await;
        self.observe_snapshot(game, receiver, result);
    }

    fn observe_snapshot(
        &mut self,
        game: GameId,
        receiver: watch::Receiver<DesktopGameSupervisorEvent>,
        result: Result<(), watch::error::RecvError>,
    ) {
        if result.is_ok() {
            self.publish(receiver.borrow().snapshot.clone());
            self.watch(game, receiver);
        }
    }

    fn watch(&mut self, game: GameId, mut receiver: watch::Receiver<DesktopGameSupervisorEvent>) {
        self.watches.push(Box::pin(async move {
            let result = receiver.changed().await;
            (game, receiver, result)
        }));
    }

    fn poll_watches(
        watches: &mut Vec<DesktopGameRegistryWatchFuture>,
        context: &mut Context<'_>,
    ) -> Poll<(
        GameId,
        watch::Receiver<DesktopGameSupervisorEvent>,
        Result<(), watch::error::RecvError>,
    )> {
        for index in 0..watches.len() {
            if let Poll::Ready(result) = watches[index].as_mut().poll(context) {
                drop(watches.swap_remove(index));
                return Poll::Ready(result);
            }
        }
        Poll::Pending
    }

    fn publish(&self, snapshot: DesktopGameSnapshot) {
        let mut snapshots = self.snapshots.borrow().clone();
        match snapshots
            .iter_mut()
            .find(|current| current.id() == snapshot.id())
        {
            Some(current) if current.revision() <= snapshot.revision() => *current = snapshot,
            Some(_) => return,
            None => snapshots.push(snapshot),
        }
        self.snapshots.send_replace(snapshots);
    }

    async fn handle_control(
        &mut self,
        control: DesktopGameRegistryControl,
    ) -> Result<(), DesktopGameRegistryError> {
        let result = match control.operation {
            DesktopGameRegistryEnumControl::Quiesce {
                generation,
                deadline,
            } => {
                self.preparation = Some(generation);
                self.broadcast(
                    DesktopGameCommandsEnumAction::QuiesceForPreparation { generation },
                    deadline,
                )
                .await
            }
            DesktopGameRegistryEnumControl::Resume {
                generation,
                deadline,
            } => {
                if self.preparation != Some(generation) {
                    Err(DesktopGameRegistryError::PreparationGeneration)
                } else {
                    let result = self
                        .broadcast(
                            DesktopGameCommandsEnumAction::ResumeSearches { generation },
                            deadline,
                        )
                        .await;
                    if result.is_ok() {
                        self.preparation = None;
                    }
                    result
                }
            }
            DesktopGameRegistryEnumControl::Shutdown { deadline } => {
                self.stopping = true;
                self.shutdown_retained(deadline).await
            }
            DesktopGameRegistryEnumControl::PauseGraphics {
                received_at,
                deadline,
            } => {
                self.broadcast_at(
                    DesktopGameCommandsEnumAction::Pause {
                        cause: GamePauseEnumCause::Fault(GamePauseEnumFault::Graphics),
                    },
                    received_at,
                    deadline,
                )
                .await
            }
            DesktopGameRegistryEnumControl::RecoverGraphics { deadline } => {
                self.broadcast(DesktopGameCommandsEnumAction::RecoverGraphics, deadline)
                    .await
            }
        };
        let result = result.map_err(Arc::new);
        let shutdown_failure = if self.stopping {
            result.as_ref().err().cloned()
        } else {
            None
        };
        if let Err(abandoned) = control.response.send(result) {
            drop(abandoned);
        }
        match shutdown_failure {
            Some(error) => Err(DesktopGameRegistryError::Control(error)),
            None => Ok(()),
        }
    }

    async fn broadcast(
        &mut self,
        action: DesktopGameCommandsEnumAction,
        deadline: Instant,
    ) -> Result<(), DesktopGameRegistryError> {
        self.broadcast_at(action, self.clock.now()?, deadline).await
    }

    async fn broadcast_at(
        &mut self,
        action: DesktopGameCommandsEnumAction,
        received_at: ClockInstant,
        deadline: Instant,
    ) -> Result<(), DesktopGameRegistryError> {
        let ports: Vec<_> = self.sessions.read().await.values().cloned().collect();
        let mut receipts = Vec::with_capacity(ports.len());
        let mut failure = None;
        for port in ports {
            let request = Self::broadcast_request(&port, action.clone(), received_at);
            match port.try_submit(request) {
                Ok(receipt) => receipts.push((port, Some(receipt))),
                Err(DesktopGameSupervisorError::Busy) if action.is_rebasable_control() => {
                    receipts.push((port, None));
                }
                Err(DesktopGameSupervisorError::Closed) => {}
                Err(error) => DesktopGameRegistryError::record(
                    &mut failure,
                    DesktopGameRegistryError::Supervisor(Arc::new(error)),
                ),
            }
        }
        for (port, receipt) in receipts {
            if let Err(error) = self
                .observe_broadcast(DesktopGameRegistryBroadcastOptions {
                    port,
                    pending_receipt: receipt,
                    action: action.clone(),
                    received_at,
                    deadline,
                })
                .await
            {
                DesktopGameRegistryError::record(&mut failure, error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    fn broadcast_request(
        port: &DesktopGameSupervisorPort,
        action: DesktopGameCommandsEnumAction,
        received_at: crate::clock::duration::ClockInstant,
    ) -> DesktopGameCommandsRequest {
        DesktopGameCommandsRequest {
            request_id: DesktopRequestId::new(),
            expected_revision: port.snapshot().snapshot.revision(),
            received_at,
            action,
        }
    }

    async fn observe_broadcast(
        &mut self,
        options: DesktopGameRegistryBroadcastOptions,
    ) -> Result<(), DesktopGameRegistryError> {
        let DesktopGameRegistryBroadcastOptions {
            port,
            mut pending_receipt,
            action,
            received_at,
            deadline,
        } = options;
        loop {
            let receipt = match pending_receipt.take() {
                Some(receipt) => receipt,
                None => {
                    let retry_at = if action.is_graphics_lifecycle() {
                        received_at
                    } else {
                        self.clock.now()?
                    };
                    let request = Self::broadcast_request(&port, action.clone(), retry_at);
                    match port.submit_reserved_waiting(request, deadline).await {
                        Ok(receipt) => receipt,
                        Err(DesktopGameSupervisorError::Closed) => return Ok(()),
                        Err(error) => {
                            return Err(DesktopGameRegistryError::Supervisor(Arc::new(error)));
                        }
                    }
                }
            };
            match timeout_at(deadline, receipt.wait()).await {
                Ok(Ok(snapshot)) => {
                    self.publish(snapshot);
                    return Ok(());
                }
                Ok(Err(error)) => {
                    if matches!(action, DesktopGameCommandsEnumAction::Quit)
                        && matches!(error.as_ref(), DesktopGameSupervisorError::Closed)
                    {
                        return Ok(());
                    }
                    if !action.is_rebasable_control()
                        || !matches!(
                            error.as_ref(),
                            DesktopGameSupervisorError::Revision(GameRevisionError::Stale { .. })
                        )
                        || Instant::now() >= deadline
                    {
                        return Err(DesktopGameRegistryError::Supervisor(error));
                    }
                }
                Err(error) => return Err(DesktopGameRegistryError::Deadline(error)),
            }
        }
    }

    fn observe_actor(
        &mut self,
        result: Result<DesktopGameRegistryActorOutcome<F, C, P>, tokio::task::JoinError>,
    ) -> Result<(), DesktopGameRegistryError> {
        let (actor, result) = result?;
        if !actor.is_shutdown_complete() {
            self.retained.push(actor);
        }
        result.map_err(|error| DesktopGameRegistryError::Supervisor(Arc::new(error)))
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.closed.load(Ordering::Acquire)
            && self.tasks.is_empty()
            && self.retained.is_empty()
            && self.creating.is_none()
            && self.installing.is_empty()
            && self.activating.is_none()
            && self.creation_response.is_none()
    }

    pub async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopGameRegistryError> {
        self.closed.store(true, Ordering::Release);
        self.creates.close();
        self.controls.close();
        while let Ok(creation) = self.creates.try_recv() {
            if let Err(abandoned) = creation
                .response
                .send(Err(DesktopGameRegistryError::Closed.failure()))
            {
                drop(abandoned);
            }
        }
        while let Ok(control) = self.controls.try_recv() {
            if let Err(abandoned) = control
                .response
                .send(Err(Arc::new(DesktopGameRegistryError::Closed)))
            {
                drop(abandoned);
            }
        }
        self.creates_closed = true;
        self.controls_closed = true;
        let mut failure = None;
        if let Some(creating) = &mut self.creating {
            match timeout_at(deadline, creating).await {
                Ok(result) => {
                    self.creating = None;
                    if let Err(error) = self.finish_creation(result, deadline).await {
                        DesktopGameRegistryError::record(&mut failure, error);
                    }
                }
                Err(error) => DesktopGameRegistryError::record(
                    &mut failure,
                    DesktopGameRegistryError::Deadline(error),
                ),
            }
        }
        if let Err(error) = self.finish_installations(deadline).await {
            DesktopGameRegistryError::record(&mut failure, error);
        }
        if self.creating.is_none()
            && self.activating.is_none()
            && self.creation_response.is_some()
            && let Err(error) = self.finish_activation(Err(DesktopGameRegistryError::Closed))
        {
            DesktopGameRegistryError::record(&mut failure, error);
        }
        let quit = self
            .broadcast(DesktopGameCommandsEnumAction::Quit, deadline)
            .await;
        if let Some(activating) = &mut self.activating {
            match timeout_at(deadline, activating).await {
                Ok(result) => {
                    self.activating = None;
                    if let Err(error) = self.finish_activation(result) {
                        DesktopGameRegistryError::record(&mut failure, error);
                    }
                }
                Err(error) => DesktopGameRegistryError::record(
                    &mut failure,
                    DesktopGameRegistryError::Deadline(error),
                ),
            }
        }
        if let Err(error) = quit {
            DesktopGameRegistryError::record(&mut failure, error);
            return failure.map_or(Ok(()), Err);
        }
        while !self.tasks.is_empty() {
            match timeout_at(deadline, self.tasks.join_next()).await {
                Ok(Some(result)) => {
                    if let Err(error) = self.observe_actor(result) {
                        DesktopGameRegistryError::record(&mut failure, error);
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    DesktopGameRegistryError::record(
                        &mut failure,
                        DesktopGameRegistryError::Deadline(error),
                    );
                    break;
                }
            }
        }
        if let Err(error) = self.cleanup_retained(deadline).await {
            DesktopGameRegistryError::record(&mut failure, error);
        }
        if self.is_shutdown_complete() {
            self.stopping = true;
            self.watches.clear();
        }
        failure.map_or(Ok(()), Err)
    }

    async fn cleanup_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopGameRegistryError> {
        let mut failure = None;
        let mut index = 0;
        while index < self.retained.len() {
            let result = self.retained[index].shutdown_retained(deadline).await;
            if let Err(error) = result {
                DesktopGameRegistryError::record(
                    &mut failure,
                    DesktopGameRegistryError::Supervisor(Arc::new(error)),
                );
            }
            if self.retained[index].is_shutdown_complete() {
                self.retained.swap_remove(index);
            } else {
                index += 1;
            }
        }
        failure.map_or(Ok(()), Err)
    }

    async fn activation_ready(
        activating: &mut Option<DesktopGameRegistryActivationFuture>,
    ) -> Result<DesktopGameSnapshot, DesktopGameRegistryError> {
        match activating {
            Some(activating) => activating.await,
            None => pending().await,
        }
    }

    async fn creation_ready(
        creating: &mut Option<DesktopGameRegistryCreateFuture<F, C, P>>,
    ) -> Result<
        (DesktopGameRegistryActor<F, C, P>, DesktopGameSupervisorPort),
        DesktopGameRegistryError,
    > {
        match creating {
            Some(creating) => creating.await,
            None => pending().await,
        }
    }
}
