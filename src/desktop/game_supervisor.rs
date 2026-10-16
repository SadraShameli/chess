use std::collections::VecDeque;
use std::future::{Future, pending};
use std::panic::RefUnwindSafe;
use std::pin::Pin;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use std::time::{Duration, Instant as StandardInstant};

use thiserror::Error;
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::time::{Instant, timeout_at};

#[cfg(test)]
#[path = "../../tests/desktop/game_save_cancellation.rs"]
mod cancellation_tests;

use crate::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use crate::capacity::phase::CapacityPhaseGeneration;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use crate::desktop::game_snapshot::{
    DesktopGameSnapshot, DesktopGameSnapshotEnumNotice, DesktopGameSnapshotError,
};
use crate::desktop::game_transition::{
    DesktopGameTransition, DesktopGameTransitionEnumEffect, DesktopGameTransitionError,
    DesktopGameTransitionOptions,
};
use crate::desktop::identity::DesktopRequestId;
use crate::engine::live_game::{EngineLiveGame, EngineLiveGameError};
use crate::engine::process_provider::EngineProcess;
use crate::game::adjudication::GameAdjudicationMateProofProvider;
use crate::game::engine_provider::{
    GameEngineContext, GameEngineEnumEvent, GameEngineMove, GameEngineProvider, GameEngineRequest,
    GameEngineSearchConfig, GameEngineSearchIdentity,
};
use crate::game::enums::{
    GameEnumEngineReadiness, GameEnumMode, GameEnumPhase, GameEnumSeat, GameIdentityError,
    GameRevisionError,
};
use crate::game::pause::{
    GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseEnumFault, GamePauseRequest,
};
use crate::game::premove::GamePremoveEnumResolution;
use crate::game::save::{GameSaveEnumStatus, GameSaveFailureRequest};
use crate::game::session::{GameSession, GameSessionError, GameSessionMoveRequest};
use crate::game::storage_repository::{
    GameStorageReceipt, GameStorageRepository, GameStorageRevision, GameStorageSaveRequest,
};
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::game_document::{StorageGameDocumentError, StorageGameDocumentRepository};

#[derive(Debug, Error)]
pub enum DesktopGameSupervisorError {
    #[error("The game supervisor cannot admit another request")]
    Busy,
    #[error("The game supervisor has closed")]
    Closed,
    #[error("Waiting admission is available only for a reserved game control")]
    ReservedControlRequired,
    #[error("The owned game state is unavailable")]
    StateUnavailable,
    #[error("The game ownership-transfer state is poisoned")]
    Poisoned,
    #[error("The game preparation generation is stale")]
    PreparationGeneration,
    #[error("The game cannot complete shutdown before its pending save is retried")]
    SaveRequired,
    #[error("The game engine is unavailable until explicit Retry")]
    EngineUnavailable,
    #[error("The game engine operation failed: {source}")]
    Engine {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("The game engine factory failed: {source}")]
    Factory {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("Game cleanup exceeded its retained deadline: {0}")]
    Deadline(#[source] tokio::time::error::Elapsed),
    #[error("The game response channel closed: {0}")]
    Response(#[from] oneshot::error::RecvError),
    #[error("The game retains incomplete cleanup after failure: {0}")]
    Retained(#[source] Arc<Self>),
    #[error("The game operation failed ({operation}); cleanup also failed ({cleanup})")]
    Cleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
    #[error(transparent)]
    Executor(#[from] CapacityExecutorError),
    #[error(transparent)]
    Storage(#[from] StorageGameDocumentError),
    #[error(transparent)]
    Session(#[from] GameSessionError),
    #[error(transparent)]
    Transition(#[from] DesktopGameTransitionError),
    #[error(transparent)]
    Snapshot(#[from] DesktopGameSnapshotError),
    #[error(transparent)]
    Time(#[from] ClockDurationError),
    #[error(transparent)]
    Revision(#[from] GameRevisionError),
    #[error(transparent)]
    Identity(#[from] GameIdentityError),
}

impl DesktopGameSupervisorError {
    fn combine(operation: Result<(), Self>, cleanup: Result<(), Self>) -> Result<(), Self> {
        match (operation, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(operation), Err(cleanup)) => Err(Self::Cleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            }),
        }
    }
}

pub trait DesktopGameSupervisorEngineProvider: GameEngineProvider {
    fn is_shutdown_complete(&self) -> bool;
    fn refresh_resources(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

impl<P: EngineProcess, C: ClockTimeProvider> DesktopGameSupervisorEngineProvider
    for EngineLiveGame<P, C>
{
    fn is_shutdown_complete(&self) -> bool {
        EngineLiveGame::is_shutdown_complete(self)
    }
    async fn refresh_resources(&mut self) -> Result<(), EngineLiveGameError> {
        pending().await
    }
    async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), EngineLiveGameError> {
        EngineLiveGame::shutdown_retained(self, deadline.into_std()).await
    }
}

pub trait DesktopGameSupervisorEngineFactory: Send {
    type Engine: DesktopGameSupervisorEngineProvider;
    type Error: std::error::Error + Send + Sync + 'static;
    fn create(
        &mut self,
        context: GameEngineContext,
    ) -> impl Future<Output = Result<Self::Engine, Self::Error>> + Send;
    fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

#[derive(Debug, Clone)]
pub enum DesktopGameSupervisorEnumPersistence {
    Current,
    Pending,
    Failed(Arc<DesktopGameSupervisorError>),
}

#[derive(Debug, Clone)]
pub struct DesktopGameSupervisorEvent {
    pub snapshot: DesktopGameSnapshot,
    pub storage_revision: GameStorageRevision,
    pub persistence: DesktopGameSupervisorEnumPersistence,
    pub notice: Option<Arc<DesktopGameSupervisorError>>,
}

type DesktopGameSupervisorResponse = Result<DesktopGameSnapshot, Arc<DesktopGameSupervisorError>>;

pub struct DesktopGameSupervisorReceipt {
    request_id: DesktopRequestId,
    response: oneshot::Receiver<DesktopGameSupervisorResponse>,
}

impl DesktopGameSupervisorReceipt {
    pub const fn request_id(&self) -> DesktopRequestId {
        self.request_id
    }
    pub async fn wait(self) -> DesktopGameSupervisorResponse {
        self.response
            .await
            .map_err(|error| Arc::new(DesktopGameSupervisorError::Response(error)))?
    }
    pub fn try_result(
        &mut self,
    ) -> Result<Option<DesktopGameSupervisorResponse>, DesktopGameSupervisorError> {
        match self.response.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(oneshot::error::TryRecvError::Empty) => Ok(None),
            Err(oneshot::error::TryRecvError::Closed) => Err(DesktopGameSupervisorError::Closed),
        }
    }
}

struct DesktopGameSupervisorControl {
    held: Arc<AtomicU8>,
    mask: u8,
    available: Arc<Notify>,
}

impl Drop for DesktopGameSupervisorControl {
    fn drop(&mut self) {
        self.held.fetch_and(!self.mask, Ordering::AcqRel);
        self.available.notify_waiters();
    }
}

struct DesktopGameSupervisorEnvelope {
    request: DesktopGameCommandsRequest,
    response: oneshot::Sender<DesktopGameSupervisorResponse>,
    _control: Option<DesktopGameSupervisorControl>,
}

impl DesktopGameSupervisorEnvelope {
    fn complete(self, response: DesktopGameSupervisorResponse) {
        if let Err(abandoned) = self.response.send(response) {
            drop(abandoned);
        }
    }
}

#[derive(Clone)]
pub struct DesktopGameSupervisorPort {
    ordinary: mpsc::Sender<DesktopGameSupervisorEnvelope>,
    controls: mpsc::Sender<DesktopGameSupervisorEnvelope>,
    held_controls: Arc<AtomicU8>,
    controls_available: Arc<Notify>,
    closed: Arc<AtomicBool>,
    events: watch::Receiver<DesktopGameSupervisorEvent>,
}

impl DesktopGameSupervisorPort {
    pub const ORDINARY_CAPACITY: usize = 32;
    const CONTROL_CAPACITY: usize = 6;
    pub fn snapshot(&self) -> DesktopGameSupervisorEvent {
        self.events.borrow().clone()
    }
    pub fn events(&self) -> watch::Receiver<DesktopGameSupervisorEvent> {
        self.events.clone()
    }
    pub fn try_submit(
        &self,
        request: DesktopGameCommandsRequest,
    ) -> Result<DesktopGameSupervisorReceipt, DesktopGameSupervisorError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DesktopGameSupervisorError::Closed);
        }
        let control = request
            .action
            .control_index()
            .map(|index| {
                let mask = 1_u8
                    .checked_shl(
                        u32::try_from(index).map_err(|_held| DesktopGameSupervisorError::Busy)?,
                    )
                    .ok_or(DesktopGameSupervisorError::Busy)?;
                self.held_controls
                    .try_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                        (held & mask == 0).then_some(held | mask)
                    })
                    .map_err(|_held| DesktopGameSupervisorError::Busy)?;
                Ok::<_, DesktopGameSupervisorError>(DesktopGameSupervisorControl {
                    held: Arc::clone(&self.held_controls),
                    mask,
                    available: Arc::clone(&self.controls_available),
                })
            })
            .transpose()?;
        let request_id = request.request_id;
        let (response, receiver) = oneshot::channel();
        let sender = if control.is_some() {
            &self.controls
        } else {
            &self.ordinary
        };
        sender
            .try_send(DesktopGameSupervisorEnvelope {
                request,
                response,
                _control: control,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => DesktopGameSupervisorError::Busy,
                mpsc::error::TrySendError::Closed(_) => DesktopGameSupervisorError::Closed,
            })?;
        Ok(DesktopGameSupervisorReceipt {
            request_id,
            response: receiver,
        })
    }

    pub(crate) async fn submit_reserved_waiting(
        &self,
        request: DesktopGameCommandsRequest,
        deadline: Instant,
    ) -> Result<DesktopGameSupervisorReceipt, DesktopGameSupervisorError> {
        if request.action.control_index().is_none() {
            return Err(DesktopGameSupervisorError::ReservedControlRequired);
        }
        loop {
            let available = self.controls_available.notified();
            tokio::pin!(available);
            available.as_mut().enable();
            match self.try_submit(request.clone()) {
                Ok(receipt) => return Ok(receipt),
                Err(DesktopGameSupervisorError::Busy) => {
                    timeout_at(deadline, available)
                        .await
                        .map_err(DesktopGameSupervisorError::Deadline)?;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

pub struct DesktopGameSupervisorOptions<F, C, P> {
    pub session: GameSession,
    pub storage_revision: GameStorageRevision,
    pub storage: StorageDocumentClient,
    pub executor: Arc<CapacityExecutor>,
    pub clock: Arc<C>,
    pub proof: Arc<P>,
    pub factory: F,
    pub search_config: GameEngineSearchConfig,
}

struct DesktopGameSupervisorSession(Mutex<Option<GameSession>>);

impl DesktopGameSupervisorSession {
    fn take(&self) -> Result<GameSession, DesktopGameSupervisorError> {
        self.0
            .lock()
            .map_err(|_poisoned| DesktopGameSupervisorError::Poisoned)?
            .take()
            .ok_or(DesktopGameSupervisorError::StateUnavailable)
    }
    fn replace(&self, session: GameSession) -> Result<(), DesktopGameSupervisorError> {
        let mut state = self
            .0
            .lock()
            .map_err(|_poisoned| DesktopGameSupervisorError::Poisoned)?;
        if state.is_some() {
            return Err(DesktopGameSupervisorError::StateUnavailable);
        }
        *state = Some(session);
        Ok(())
    }
    fn acknowledge(
        &self,
        receipt: GameStorageReceipt,
    ) -> Result<GameSaveEnumStatus, DesktopGameSupervisorError> {
        let mut session = self.take()?;
        let result = session
            .acknowledge_save(receipt)
            .map(|_| session.save_status())
            .map_err(DesktopGameSupervisorError::Session);
        self.replace(session)?;
        result
    }
}

enum DesktopGameSupervisorEnumComputation {
    Initialize,
    Action(DesktopGameTransitionOptions),
    EngineMove(GameEngineMove),
    Clock,
    Search,
    EngineFailure,
    SaveFailure {
        engine: GamePauseEnumEngineBarrier,
        received_at: ClockInstant,
    },
    Resave {
        quit: bool,
    },
}

enum DesktopGameSupervisorEnumEngineOperation {
    Start(Box<GameEngineRequest>),
    Events,
    Resources,
}

enum DesktopGameSupervisorEnumEngineActivity {
    Started(GameEngineSearchIdentity),
    Event(GameEngineEnumEvent),
    ResourcesUpdated,
}

struct DesktopGameSupervisorComputeOptions<C, P> {
    session: Arc<DesktopGameSupervisorSession>,
    executor: Arc<CapacityExecutor>,
    clock: Arc<C>,
    proof: Arc<P>,
    config: GameEngineSearchConfig,
    storage_revision: GameStorageRevision,
}

struct DesktopGameSupervisorComputeContext<'a, C, P> {
    clock: &'a C,
    proof: &'a P,
    config: GameEngineSearchConfig,
    storage_revision: GameStorageRevision,
}

struct DesktopGameSupervisorComputed {
    snapshot: DesktopGameSnapshot,
    checkpoint: Option<GameStorageSaveRequest>,
    save: Option<GameStorageSaveRequest>,
    search: Option<GameEngineRequest>,
    quit: bool,
}

type DesktopGameSupervisorComputeFuture = Pin<
    Box<
        dyn Future<Output = Result<DesktopGameSupervisorComputed, DesktopGameSupervisorError>>
            + Send,
    >,
>;
type DesktopGameSupervisorSaveFuture =
    Pin<Box<dyn Future<Output = Result<GameStorageReceipt, StorageGameDocumentError>> + Send>>;

struct DesktopGameSupervisorWrite {
    request: GameStorageSaveRequest,
    response: Option<(DesktopGameSnapshot, DesktopGameSupervisorEnvelope)>,
    quit: bool,
}

struct DesktopGameSupervisorWriteContext {
    request: GameStorageSaveRequest,
    response: Option<(DesktopGameSnapshot, DesktopGameSupervisorEnvelope)>,
    quit: bool,
}

pub struct DesktopGameSupervisor<F: DesktopGameSupervisorEngineFactory, C, P> {
    session: Arc<DesktopGameSupervisorSession>,
    storage: StorageDocumentClient,
    storage_revision: GameStorageRevision,
    executor: Arc<CapacityExecutor>,
    clock: Arc<C>,
    proof: Arc<P>,
    factory: F,
    factory_shutdown_complete: bool,
    engine: Option<F::Engine>,
    active_search: Option<GameEngineSearchIdentity>,
    search_request: Option<GameEngineRequest>,
    search_config: GameEngineSearchConfig,
    preparation: Option<CapacityPhaseGeneration>,
    ordinary: mpsc::Receiver<DesktopGameSupervisorEnvelope>,
    controls: mpsc::Receiver<DesktopGameSupervisorEnvelope>,
    deferred_controls: VecDeque<DesktopGameSupervisorEnvelope>,
    events: watch::Sender<DesktopGameSupervisorEvent>,
    closed: Arc<AtomicBool>,
    computing: Option<DesktopGameSupervisorComputeFuture>,
    computing_response: Option<DesktopGameSupervisorEnvelope>,
    factory_request: Option<DesktopGameSupervisorEnvelope>,
    pending_engine_move: Option<GameEngineMove>,
    pending_engine_failure: bool,
    controls_closed: bool,
    ordinary_closed: bool,
    writes: VecDeque<DesktopGameSupervisorWrite>,
    saving: Option<DesktopGameSupervisorSaveFuture>,
    saving_context: Option<DesktopGameSupervisorWriteContext>,
    save_failure: Option<Arc<DesktopGameSupervisorError>>,
    save_failure_at: Option<ClockInstant>,
    quit_ready: bool,
}

impl<F, C, P> DesktopGameSupervisor<F, C, P>
where
    F: DesktopGameSupervisorEngineFactory,
    <F::Engine as GameEngineProvider>::Error: 'static,
    C: ClockTimeProvider + RefUnwindSafe + 'static,
    P: GameAdjudicationMateProofProvider + Send + Sync + RefUnwindSafe + 'static,
{
    pub async fn new(
        options: DesktopGameSupervisorOptions<F, C, P>,
    ) -> Result<(Self, DesktopGameSupervisorPort), DesktopGameSupervisorError> {
        let session = Arc::new(DesktopGameSupervisorSession(Mutex::new(Some(
            options.session,
        ))));
        let computed = Self::computation(
            DesktopGameSupervisorComputeOptions {
                session: Arc::clone(&session),
                executor: Arc::clone(&options.executor),
                clock: Arc::clone(&options.clock),
                proof: Arc::clone(&options.proof),
                config: options.search_config,
                storage_revision: options.storage_revision,
            },
            DesktopGameSupervisorEnumComputation::Initialize,
        )
        .await?;
        let persistence = if computed.save.is_some() {
            DesktopGameSupervisorEnumPersistence::Pending
        } else {
            DesktopGameSupervisorEnumPersistence::Current
        };
        let (events, receiver) = watch::channel(DesktopGameSupervisorEvent {
            snapshot: computed.snapshot,
            storage_revision: options.storage_revision,
            persistence,
            notice: None,
        });
        let (ordinary, ordinary_receiver) =
            mpsc::channel(DesktopGameSupervisorPort::ORDINARY_CAPACITY);
        let (controls, control_receiver) =
            mpsc::channel(DesktopGameSupervisorPort::CONTROL_CAPACITY);
        let closed = Arc::new(AtomicBool::new(false));
        let port = DesktopGameSupervisorPort {
            ordinary,
            controls,
            held_controls: Arc::new(AtomicU8::new(0)),
            controls_available: Arc::new(Notify::new()),
            closed: Arc::clone(&closed),
            events: receiver,
        };
        let mut writes = VecDeque::new();
        if let Some(request) = computed.save {
            writes.push_back(DesktopGameSupervisorWrite {
                request,
                response: None,
                quit: false,
            });
        }
        Ok((
            Self {
                session,
                storage: options.storage,
                storage_revision: options.storage_revision,
                executor: options.executor,
                clock: options.clock,
                proof: options.proof,
                factory: options.factory,
                factory_shutdown_complete: false,
                engine: None,
                active_search: None,
                search_request: computed.search,
                search_config: options.search_config,
                preparation: None,
                ordinary: ordinary_receiver,
                controls: control_receiver,
                deferred_controls: VecDeque::with_capacity(
                    DesktopGameSupervisorPort::CONTROL_CAPACITY,
                ),
                events,
                closed,
                computing: None,
                computing_response: None,
                factory_request: None,
                pending_engine_move: None,
                pending_engine_failure: false,
                controls_closed: false,
                ordinary_closed: false,
                writes,
                saving: None,
                saving_context: None,
                save_failure: None,
                save_failure_at: None,
                quit_ready: false,
            },
            port,
        ))
    }

    fn computation(
        options: DesktopGameSupervisorComputeOptions<C, P>,
        operation: DesktopGameSupervisorEnumComputation,
    ) -> DesktopGameSupervisorComputeFuture {
        Box::pin(async move {
            options
                .executor
                .submit_waiting(move |_cancellation| {
                    let mut state = options.session.take()?;
                    let result = Self::compute(
                        &mut state,
                        operation,
                        DesktopGameSupervisorComputeContext {
                            clock: &options.clock,
                            proof: &options.proof,
                            config: options.config,
                            storage_revision: options.storage_revision,
                        },
                    );
                    options.session.replace(state)?;
                    result
                })
                .await?
                .join()
                .await?
        })
    }

    fn compute(
        state: &mut GameSession,
        operation: DesktopGameSupervisorEnumComputation,
        context: DesktopGameSupervisorComputeContext<'_, C, P>,
    ) -> Result<DesktopGameSupervisorComputed, DesktopGameSupervisorError> {
        let DesktopGameSupervisorComputeContext {
            clock,
            proof,
            config,
            storage_revision,
        } = context;
        let mut now = clock.now()?;
        let mut notice = None;
        let mut checkpoint = None;
        let effect = match operation {
            DesktopGameSupervisorEnumComputation::Resave { quit } => {
                if quit {
                    DesktopGameTransitionEnumEffect::Quit
                } else {
                    DesktopGameTransitionEnumEffect::Autosave
                }
            }
            DesktopGameSupervisorEnumComputation::Initialize => {
                if storage_revision == GameStorageRevision::INITIAL {
                    DesktopGameTransitionEnumEffect::Autosave
                } else {
                    DesktopGameTransitionEnumEffect::SnapshotOnly
                }
            }
            DesktopGameSupervisorEnumComputation::Action(options) => {
                now = options.request.ordered_received_at(state.clock());
                DesktopGameTransition::apply(state, options, proof)?
            }
            DesktopGameSupervisorEnumComputation::EngineMove(result) => {
                let expected = result.identity().context();
                if expected.game() != state.id() {
                    return Err(DesktopGameSupervisorError::StateUnavailable);
                }
                state.revision().check_expected(expected.revision())?;
                now = result.received_at();
                let color = state
                    .tree()
                    .node(state.live())
                    .map_err(GameSessionError::from)?
                    .position()
                    .turn();
                state.accept_move(
                    GameSessionMoveRequest {
                        expected_revision: state.revision(),
                        seat: GameEnumSeat::Reckless,
                        color,
                        node: uuid::Uuid::new_v4().to_string().parse()?,
                        movement: result.movement(),
                        received_at: now,
                    },
                    proof,
                )?;
                if state.phase() == GameEnumPhase::Active {
                    match state.resolve_premove(state.revision()) {
                        Ok(GamePremoveEnumResolution::Execute(movement)) => {
                            checkpoint = Some(GameStorageSaveRequest::capture(
                                state,
                                now,
                                storage_revision,
                            )?);
                            now = clock.now()?;
                            let color = state
                                .tree()
                                .node(state.live())
                                .map_err(GameSessionError::from)?
                                .position()
                                .turn();
                            state.accept_move(
                                GameSessionMoveRequest {
                                    expected_revision: state.revision(),
                                    seat: GameEnumSeat::Human,
                                    color,
                                    node: uuid::Uuid::new_v4().to_string().parse()?,
                                    movement,
                                    received_at: now,
                                },
                                proof,
                            )?;
                        }
                        Ok(GamePremoveEnumResolution::Empty) => {}
                        Ok(GamePremoveEnumResolution::ClearedIllegal { intent, .. }) => {
                            notice = Some(DesktopGameSnapshotEnumNotice::IllegalPremove {
                                revision: state.revision(),
                                intent,
                            });
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                DesktopGameTransitionEnumEffect::Autosave
            }
            DesktopGameSupervisorEnumComputation::Clock => {
                let update = state.poll_deadline(state.revision(), now, proof)?;
                if update.transition().is_some() {
                    DesktopGameTransitionEnumEffect::Autosave
                } else {
                    DesktopGameTransitionEnumEffect::SnapshotOnly
                }
            }
            DesktopGameSupervisorEnumComputation::Search => {
                DesktopGameTransitionEnumEffect::SnapshotOnly
            }
            DesktopGameSupervisorEnumComputation::EngineFailure => {
                if state.phase() == GameEnumPhase::Active {
                    state.pause(
                        GamePauseRequest {
                            expected_revision: state.revision(),
                            cause: GamePauseEnumCause::Fault(GamePauseEnumFault::Engine),
                            received_at: now,
                            engine: GamePauseEnumEngineBarrier::FailedEngine,
                        },
                        proof,
                    )?;
                    DesktopGameTransitionEnumEffect::Autosave
                } else {
                    DesktopGameTransitionEnumEffect::SnapshotOnly
                }
            }
            DesktopGameSupervisorEnumComputation::SaveFailure {
                engine,
                received_at,
            } => {
                now = received_at;
                if state.phase() == GameEnumPhase::Active
                    && matches!(engine, GamePauseEnumEngineBarrier::FailedEngine)
                {
                    state.pause(
                        GamePauseRequest {
                            expected_revision: state.revision(),
                            cause: GamePauseEnumCause::Fault(GamePauseEnumFault::Engine),
                            received_at: now,
                            engine: GamePauseEnumEngineBarrier::FailedEngine,
                        },
                        proof,
                    )?;
                }
                state.interrupt_for_save_failure(
                    GameSaveFailureRequest {
                        expected_revision: state.revision(),
                        received_at: now,
                        engine,
                    },
                    proof,
                )?;
                DesktopGameTransitionEnumEffect::SnapshotOnly
            }
        };
        let mut snapshot = DesktopGameSnapshot::capture(state, now)?;
        snapshot.retain_notice(notice);
        let save = match effect {
            DesktopGameTransitionEnumEffect::Autosave | DesktopGameTransitionEnumEffect::Quit => {
                Some(GameStorageSaveRequest::capture(
                    state,
                    now,
                    storage_revision,
                )?)
            }
            DesktopGameTransitionEnumEffect::SnapshotOnly => None,
        };
        let search = if state.phase() == GameEnumPhase::Active
            && state.seats().seat(snapshot.live_turn()) == GameEnumSeat::Reckless
        {
            Some(state.engine_request(config)?)
        } else {
            None
        };
        Ok(DesktopGameSupervisorComputed {
            snapshot,
            checkpoint,
            save,
            search,
            quit: matches!(effect, DesktopGameTransitionEnumEffect::Quit),
        })
    }

    fn schedule(
        &mut self,
        operation: DesktopGameSupervisorEnumComputation,
        response: Option<DesktopGameSupervisorEnvelope>,
    ) {
        self.computing = Some(Self::computation(
            DesktopGameSupervisorComputeOptions {
                session: Arc::clone(&self.session),
                executor: Arc::clone(&self.executor),
                clock: Arc::clone(&self.clock),
                proof: Arc::clone(&self.proof),
                config: self.search_config,
                storage_revision: self.storage_revision,
            },
            operation,
        ));
        self.computing_response = response;
    }

    const ENGINE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
    const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

    pub async fn run(&mut self) -> Result<(), DesktopGameSupervisorError> {
        let operation = self.run_loop().await;
        self.closed.store(true, Ordering::Release);
        let cleanup = self
            .shutdown_retained(Instant::now() + Self::SHUTDOWN_TIMEOUT)
            .await;
        DesktopGameSupervisorError::combine(operation, cleanup)
    }

    async fn run_loop(&mut self) -> Result<(), DesktopGameSupervisorError> {
        loop {
            if self.quit_ready
                && self.computing.is_none()
                && self.saving.is_none()
                && self.writes.is_empty()
                && self.save_failure.is_none()
            {
                return Ok(());
            }
            if self.computing.is_none() {
                if let Some(control) = self.deferred_controls.pop_front() {
                    self.handle(control).await?;
                } else if self.save_failure.is_some()
                    && !self.events.borrow().snapshot.is_save_required()
                {
                    let engine = match self.barrier().await {
                        Ok(engine) => engine,
                        Err(error) => {
                            self.publish_notice(Arc::new(error));
                            self.retire_engine().await?;
                            GamePauseEnumEngineBarrier::FailedEngine
                        }
                    };
                    self.schedule(
                        DesktopGameSupervisorEnumComputation::SaveFailure {
                            engine,
                            received_at: self
                                .save_failure_at
                                .ok_or(DesktopGameSupervisorError::StateUnavailable)?,
                        },
                        None,
                    );
                } else if self.pending_engine_failure {
                    self.retire_engine().await?;
                    self.pending_engine_failure = false;
                    self.schedule(DesktopGameSupervisorEnumComputation::EngineFailure, None);
                } else if let Some(movement) = self.pending_engine_move.take() {
                    self.schedule(
                        DesktopGameSupervisorEnumComputation::EngineMove(movement),
                        None,
                    );
                }
            }
            if self.controls_closed
                && self.ordinary_closed
                && self.computing.is_none()
                && self.saving.is_none()
                && self.writes.is_empty()
                && !self.quit_ready
            {
                let (response, receiver) = oneshot::channel();
                drop(receiver);
                self.handle(DesktopGameSupervisorEnvelope {
                    request: DesktopGameCommandsRequest {
                        request_id: DesktopRequestId::new(),
                        expected_revision: self.context().revision(),
                        received_at: self.clock.now()?,
                        action: DesktopGameCommandsEnumAction::Quit,
                    },
                    response,
                    _control: None,
                })
                .await?;
            }
            if self.computing.is_none()
                && self.events.borrow().snapshot.phase() != GameEnumPhase::Active
                && self.engine.is_some()
                && self.factory_request.is_none()
            {
                self.retire_engine().await?;
            }
            self.begin_save(false);
            let engine_operation = self.engine_operation();
            let has_engine_operation = engine_operation.is_some();
            let context = self.context();
            let factory_context = self.factory_request.as_ref().map(|_| context);
            let deadline = self.deadline()?;
            tokio::select! {
                biased;
                control = self.controls.recv(), if !self.controls_closed && self.deferred_controls.len() < DesktopGameSupervisorPort::CONTROL_CAPACITY => {
                    if let Some(control) = control {
                        if self.computing.is_none() || matches!(control.request.action, DesktopGameCommandsEnumAction::QuiesceForPreparation { .. } | DesktopGameCommandsEnumAction::ResumeSearches { .. }) { self.handle(control).await?; }
                        else { self.deferred_controls.push_back(control); }
                    } else { self.controls_closed = true; }
                }
                result = Self::compute_ready(&mut self.computing), if self.computing.is_some() => { self.computing = None; self.finish_compute(result)?; }
                result = Self::save_ready(&mut self.saving), if self.saving.is_some() && self.computing.is_none() => { self.saving = None; self.finish_save(result)?; }
                result = Self::factory_ready(&mut self.factory, factory_context), if factory_context.is_some() && self.preparation.is_none() && self.computing.is_none() => {
                    let response = self.factory_request.take().ok_or(DesktopGameSupervisorError::StateUnavailable)?;
                    match result {
                        Ok(engine) => { self.engine = Some(engine); self.handle(response).await?; }
                        Err(source) => {
                            let operation = DesktopGameSupervisorError::Factory { source: Box::new(source) };
                            let cleanup = Self::shutdown_factory(
                                &mut self.factory,
                                Instant::now() + Self::ENGINE_SHUTDOWN_TIMEOUT,
                            ).await;
                            self.factory_shutdown_complete = cleanup.is_ok();
                            let failure = Arc::new(match cleanup {
                                Ok(()) => operation,
                                Err(cleanup) => DesktopGameSupervisorError::Cleanup {
                                    operation: Box::new(operation),
                                    cleanup: Box::new(cleanup),
                                },
                            });
                            response.complete(Err(Arc::clone(&failure)));
                            if !self.factory_shutdown_complete {
                                return Err(DesktopGameSupervisorError::Retained(failure));
                            }
                        }
                    }
                }
                result = Self::engine_ready(&mut self.engine, context, engine_operation), if has_engine_operation => {
                    match result {
                        Ok(DesktopGameSupervisorEnumEngineActivity::Started(identity)) => { self.search_request = None; self.active_search = Some(identity); }
                        Ok(DesktopGameSupervisorEnumEngineActivity::ResourcesUpdated) => {}
                        Ok(DesktopGameSupervisorEnumEngineActivity::Event(GameEngineEnumEvent::Move(movement))) => { self.active_search = None; self.pending_engine_move = Some(movement); }
                        Ok(DesktopGameSupervisorEnumEngineActivity::Event(GameEngineEnumEvent::Info { .. } | GameEngineEnumEvent::Diagnostic { .. } | GameEngineEnumEvent::Readiness { .. } | GameEngineEnumEvent::Discarded { .. } | GameEngineEnumEvent::Stopped(_))) => {}
                        Err(source) => {
                            self.active_search = None;
                            self.search_request = None;
                            let error = DesktopGameSupervisorError::Engine { source: Box::new(source) };
                            self.publish_notice(Arc::new(error));
                            self.retire_engine().await?;
                            self.pending_engine_failure = true;
                        }
                    }
                }
                () = Self::wait_deadline(deadline), if deadline.is_some() && self.computing.is_none() => {
                    self.search_request = None;
                    if self.active_search.is_some() { let _barrier = self.barrier().await?; }
                    self.schedule(DesktopGameSupervisorEnumComputation::Clock, None);
                }
                ordinary = self.ordinary.recv(), if !self.ordinary_closed && self.computing.is_none() && self.saving.is_none() && self.writes.is_empty() && self.factory_request.is_none() => {
                    match ordinary { Some(request) => self.handle(request).await?, None => self.ordinary_closed = true }
                }
            }
        }
    }

    fn context(&self) -> GameEngineContext {
        let snapshot = &self.events.borrow().snapshot;
        GameEngineContext::new(snapshot.id(), snapshot.revision())
    }

    async fn handle(
        &mut self,
        response: DesktopGameSupervisorEnvelope,
    ) -> Result<(), DesktopGameSupervisorError> {
        match response.request.action {
            DesktopGameCommandsEnumAction::QuiesceForPreparation { generation } => {
                self.preparation = Some(generation);
                self.search_request = None;
                if self.engine.is_some() {
                    let _barrier = self.barrier().await?;
                }
                self.cancel_factory_request().await?;
                response.complete(Ok(self.events.borrow().snapshot.clone()));
                return Ok(());
            }
            DesktopGameCommandsEnumAction::ResumeSearches { generation } => {
                if self.preparation != Some(generation) {
                    response.complete(Err(Arc::new(
                        DesktopGameSupervisorError::PreparationGeneration,
                    )));
                    return Ok(());
                }
                self.preparation = None;
                response.complete(Ok(self.events.borrow().snapshot.clone()));
                if self.computing.is_none() {
                    self.schedule(DesktopGameSupervisorEnumComputation::Search, None);
                }
                return Ok(());
            }
            DesktopGameCommandsEnumAction::Start
            | DesktopGameCommandsEnumAction::Play { .. }
            | DesktopGameCommandsEnumAction::Navigate { .. }
            | DesktopGameCommandsEnumAction::Branch { .. }
            | DesktopGameCommandsEnumAction::ReturnToLive
            | DesktopGameCommandsEnumAction::BeginAnalysis
            | DesktopGameCommandsEnumAction::Pause { .. }
            | DesktopGameCommandsEnumAction::Resume
            | DesktopGameCommandsEnumAction::RequestTakeback { .. }
            | DesktopGameCommandsEnumAction::RespondTakeback { .. }
            | DesktopGameCommandsEnumAction::ConfigureTakeback { .. }
            | DesktopGameCommandsEnumAction::Resign { .. }
            | DesktopGameCommandsEnumAction::FinishLocal { .. }
            | DesktopGameCommandsEnumAction::ClaimDraw { .. }
            | DesktopGameCommandsEnumAction::ConfigurePremove { .. }
            | DesktopGameCommandsEnumAction::QueuePremove { .. }
            | DesktopGameCommandsEnumAction::CancelPremove
            | DesktopGameCommandsEnumAction::RetryEngine
            | DesktopGameCommandsEnumAction::RetrySave
            | DesktopGameCommandsEnumAction::RecoverGraphics
            | DesktopGameCommandsEnumAction::Stop
            | DesktopGameCommandsEnumAction::Quit => {}
        }
        if response.request.action.is_control() {
            self.cancel_factory_request().await?;
        }
        if let Err(error) = self
            .context()
            .revision()
            .check_expected(response.request.expected_revision)
        {
            response.complete(Err(Arc::new(error.into())));
            return Ok(());
        }
        let engine_mode =
            self.events.borrow().snapshot.seats().mode() == GameEnumMode::HumanVersusEngine;
        if engine_mode
            && matches!(
                response.request.action,
                DesktopGameCommandsEnumAction::RetryEngine
            )
        {
            self.retire_engine().await?;
        }
        if engine_mode
            && self.engine.is_none()
            && matches!(
                response.request.action,
                DesktopGameCommandsEnumAction::Start
                    | DesktopGameCommandsEnumAction::Resume
                    | DesktopGameCommandsEnumAction::RetryEngine
            )
        {
            if self.preparation.is_some() {
                response.complete(Err(Arc::new(DesktopGameSupervisorError::Busy)));
                return Ok(());
            }
            self.factory_shutdown_complete = false;
            self.factory_request = Some(response);
            return Ok(());
        }
        let engine = if self.events.borrow().snapshot.phase() == GameEnumPhase::Active
            && matches!(
                response.request.action,
                DesktopGameCommandsEnumAction::Pause { .. }
                    | DesktopGameCommandsEnumAction::Stop
                    | DesktopGameCommandsEnumAction::Quit
                    | DesktopGameCommandsEnumAction::RequestTakeback { .. }
                    | DesktopGameCommandsEnumAction::Resign { .. }
                    | DesktopGameCommandsEnumAction::ConfigurePremove { .. }
                    | DesktopGameCommandsEnumAction::ConfigureTakeback { .. }
                    | DesktopGameCommandsEnumAction::ClaimDraw { .. }
            ) {
            self.search_request = None;
            self.pending_engine_move = None;
            match self.barrier().await {
                Ok(engine) => engine,
                Err(error) => {
                    response.complete(Err(Arc::new(error)));
                    self.pending_engine_move = None;
                    self.pending_engine_failure = true;
                    self.active_search = None;
                    return Ok(());
                }
            }
        } else {
            GamePauseEnumEngineBarrier::NoEngine
        };
        let readiness = if engine_mode && self.engine.is_none() {
            GameEnumEngineReadiness::Unavailable
        } else if engine_mode {
            GameEnumEngineReadiness::Ready
        } else {
            GameEnumEngineReadiness::NotRequired
        };
        let operation =
            DesktopGameSupervisorEnumComputation::Action(DesktopGameTransitionOptions {
                request: response.request.clone(),
                engine,
                readiness,
            });
        self.schedule(operation, Some(response));
        Ok(())
    }

    async fn barrier(&mut self) -> Result<GamePauseEnumEngineBarrier, DesktopGameSupervisorError> {
        let context = self.context();
        match &mut self.engine {
            Some(engine) => {
                let receipt = engine
                    .quiesce(context, StandardInstant::now())
                    .await
                    .map_err(|source| DesktopGameSupervisorError::Engine {
                        source: Box::new(source),
                    })?;
                self.active_search = None;
                Ok(GamePauseEnumEngineBarrier::Quiescent(receipt))
            }
            None if self.events.borrow().snapshot.seats().mode()
                == GameEnumMode::LocalTwoPlayer =>
            {
                Ok(GamePauseEnumEngineBarrier::NoEngine)
            }
            None => Err(DesktopGameSupervisorError::EngineUnavailable),
        }
    }

    fn finish_compute(
        &mut self,
        result: Result<DesktopGameSupervisorComputed, DesktopGameSupervisorError>,
    ) -> Result<(), DesktopGameSupervisorError> {
        let response = self.computing_response.take();
        match result {
            Err(error) => {
                if let Some(response) = response {
                    response.complete(Err(Arc::new(error)));
                    Ok(())
                } else {
                    Err(error)
                }
            }
            Ok(computed) => {
                self.search_request = computed.search;
                let mut event = self.events.borrow().clone();
                let notice = event.snapshot.notice();
                event.snapshot = computed.snapshot;
                event.snapshot.retain_notice(notice);
                if let Some(request) = computed.checkpoint {
                    self.writes.push_back(DesktopGameSupervisorWrite {
                        request,
                        response: None,
                        quit: false,
                    });
                }
                if let Some(request) = computed.save {
                    self.writes.push_back(DesktopGameSupervisorWrite {
                        request,
                        response: response.map(|response| (event.snapshot.clone(), response)),
                        quit: computed.quit,
                    });
                } else if let Some(response) = response {
                    response.complete(Ok(event.snapshot.clone()));
                }
                event.persistence = self.persistence();
                self.events.send_replace(event);
                Ok(())
            }
        }
    }

    fn begin_save(&mut self, retry_failure: bool) {
        if self.saving.is_some()
            || !retry_failure
                && self.save_failure.is_some()
                && !self.writes.iter().any(|write| {
                    write.response.as_ref().is_some_and(|(_, response)| {
                        matches!(
                            response.request.action,
                            DesktopGameCommandsEnumAction::RetrySave
                                | DesktopGameCommandsEnumAction::Quit
                        )
                    })
                })
        {
            return;
        }
        let Some(write) = self.writes.pop_front() else {
            return;
        };
        let request = write.request.expect_storage_revision(self.storage_revision);
        let client = self.storage.clone();
        let executor = Arc::clone(&self.executor);
        self.saving_context = Some(DesktopGameSupervisorWriteContext {
            request: request.clone(),
            response: write.response,
            quit: write.quit,
        });
        self.saving = Some(Box::pin(async move {
            StorageGameDocumentRepository::new(client, &executor)
                .save(request)
                .await
        }));
    }

    fn finish_save(
        &mut self,
        result: Result<GameStorageReceipt, StorageGameDocumentError>,
    ) -> Result<(), DesktopGameSupervisorError> {
        let context = self
            .saving_context
            .take()
            .ok_or(DesktopGameSupervisorError::StateUnavailable)?;
        match result {
            Ok(receipt) => {
                self.storage_revision = receipt.storage_revision();
                let status = self.session.acknowledge(receipt)?;
                if receipt.game_revision() == self.context().revision() {
                    self.save_failure = None;
                    self.save_failure_at = None;
                }
                let mut event = self.events.borrow().clone();
                event.storage_revision = self.storage_revision;
                event.snapshot.refresh_save_status(status);
                if self.save_failure.is_none() {
                    event.notice = None;
                }
                event.persistence = self.persistence();
                self.quit_ready |= context.quit;
                self.events.send_replace(event);
                if let Some((mut snapshot, response)) = context.response {
                    if receipt.game_revision() == snapshot.revision() {
                        snapshot.refresh_save_status(GameSaveEnumStatus::Current);
                    }
                    response.complete(Ok(snapshot));
                }
            }
            Err(StorageGameDocumentError::Cancelled) => {
                self.writes.push_front(DesktopGameSupervisorWrite {
                    request: context.request,
                    response: context.response,
                    quit: context.quit,
                });
            }
            Err(error) => {
                let failure = Arc::new(DesktopGameSupervisorError::Storage(error));
                if let Some((_, response)) = context.response {
                    response.complete(Err(Arc::clone(&failure)));
                }
                for write in self.writes.drain(..) {
                    if let Some((_, response)) = write.response {
                        response.complete(Err(Arc::clone(&failure)));
                    }
                }
                self.search_request = None;
                self.pending_engine_move = None;
                self.save_failure_at = Some(self.clock.now()?);
                self.save_failure = Some(Arc::clone(&failure));
                self.publish_notice(failure);
            }
        }
        Ok(())
    }

    fn engine_operation(&mut self) -> Option<DesktopGameSupervisorEnumEngineOperation> {
        if self.engine.is_none() || self.pending_engine_move.is_some() {
            return None;
        }
        if self.active_search.is_some() {
            return Some(DesktopGameSupervisorEnumEngineOperation::Events);
        }
        if self.quit_ready
            || self.computing.is_some()
            || self.saving.is_some()
            || !self.writes.is_empty()
            || self.preparation.is_some()
            || self.save_failure.is_some()
            || self.factory_request.is_some()
        {
            return None;
        }
        if self
            .search_request
            .as_ref()
            .is_some_and(|request| request.context() != self.context())
        {
            self.search_request = None;
        }
        match &self.search_request {
            Some(request) => Some(DesktopGameSupervisorEnumEngineOperation::Start(Box::new(
                request.clone(),
            ))),
            None => Some(DesktopGameSupervisorEnumEngineOperation::Resources),
        }
    }

    fn publish_notice(&self, failure: Arc<DesktopGameSupervisorError>) {
        let mut event = self.events.borrow().clone();
        event.notice = Some(failure);
        event.persistence = self.persistence();
        self.events.send_replace(event);
    }

    fn persistence(&self) -> DesktopGameSupervisorEnumPersistence {
        match &self.save_failure {
            Some(failure) => DesktopGameSupervisorEnumPersistence::Failed(Arc::clone(failure)),
            None if self.saving.is_some() || !self.writes.is_empty() => {
                DesktopGameSupervisorEnumPersistence::Pending
            }
            None => DesktopGameSupervisorEnumPersistence::Current,
        }
    }

    async fn cancel_factory_request(&mut self) -> Result<(), DesktopGameSupervisorError> {
        if self.factory_request.is_some() {
            let result = Self::shutdown_factory(
                &mut self.factory,
                Instant::now() + Self::ENGINE_SHUTDOWN_TIMEOUT,
            )
            .await;
            self.factory_shutdown_complete = result.is_ok();
            result?;
            if let Some(cancelled) = self.factory_request.take() {
                cancelled.complete(Err(Arc::new(DesktopGameSupervisorError::Closed)));
            }
        }
        Ok(())
    }

    fn deadline(&self) -> Result<Option<Instant>, DesktopGameSupervisorError> {
        if self.events.borrow().snapshot.phase() != GameEnumPhase::Active {
            return Ok(None);
        }
        let now = self.clock.now()?;
        let view = self
            .events
            .borrow()
            .snapshot
            .clock()
            .view(now)
            .map_err(GameSessionError::from)?;
        view.deadline()
            .map(|deadline| {
                let duration = if now.has_reached(deadline) {
                    Duration::ZERO
                } else {
                    deadline.elapsed_since(now)?.as_duration()
                };
                Instant::now()
                    .checked_add(duration)
                    .ok_or(DesktopGameSupervisorError::StateUnavailable)
            })
            .transpose()
    }

    async fn retire_engine(&mut self) -> Result<(), DesktopGameSupervisorError> {
        if let Some(engine) = &mut self.engine {
            engine
                .shutdown_retained(Instant::now() + Self::ENGINE_SHUTDOWN_TIMEOUT)
                .await
                .map_err(|source| DesktopGameSupervisorError::Engine {
                    source: Box::new(source),
                })?;
        }
        self.engine = None;
        self.active_search = None;
        Ok(())
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.closed.load(Ordering::Acquire)
            && self.computing.is_none()
            && self.saving.is_none()
            && self.writes.is_empty()
            && self.factory_request.is_none()
            && self.factory_shutdown_complete
            && self.save_failure.is_none()
            && self
                .engine
                .as_ref()
                .is_none_or(DesktopGameSupervisorEngineProvider::is_shutdown_complete)
    }

    pub async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopGameSupervisorError> {
        self.closed.store(true, Ordering::Release);
        self.ordinary.close();
        self.controls.close();
        self.search_request = None;
        self.active_search = None;
        while let Ok(response) = self.ordinary.try_recv() {
            response.complete(Err(Arc::new(DesktopGameSupervisorError::Closed)));
        }
        while let Ok(response) = self.controls.try_recv() {
            response.complete(Err(Arc::new(DesktopGameSupervisorError::Closed)));
        }
        for response in self.deferred_controls.drain(..) {
            response.complete(Err(Arc::new(DesktopGameSupervisorError::Closed)));
        }
        if let Some(response) = self.factory_request.take() {
            response.complete(Err(Arc::new(DesktopGameSupervisorError::Closed)));
        }
        let engine = Self::shutdown_engine(&mut self.engine, deadline);
        let factory = Self::shutdown_factory(&mut self.factory, deadline);
        let (engine, factory) = tokio::join!(engine, factory);
        self.factory_shutdown_complete = factory.is_ok();
        let resources = DesktopGameSupervisorError::combine(engine, factory);
        let work = self.observe_work(deadline).await;
        DesktopGameSupervisorError::combine(resources, work)
    }

    async fn shutdown_factory(
        factory: &mut F,
        deadline: Instant,
    ) -> Result<(), DesktopGameSupervisorError> {
        match timeout_at(deadline, factory.shutdown_retained(deadline)).await {
            Ok(result) => result.map_err(|source| DesktopGameSupervisorError::Factory {
                source: Box::new(source),
            }),
            Err(error) => Err(DesktopGameSupervisorError::Deadline(error)),
        }
    }

    async fn shutdown_engine(
        engine: &mut Option<F::Engine>,
        deadline: Instant,
    ) -> Result<(), DesktopGameSupervisorError> {
        let Some(engine) = engine
            .as_mut()
            .filter(|engine| !engine.is_shutdown_complete())
        else {
            return Ok(());
        };
        match timeout_at(deadline, engine.shutdown_retained(deadline)).await {
            Ok(result) => result.map_err(|source| DesktopGameSupervisorError::Engine {
                source: Box::new(source),
            }),
            Err(error) => Err(DesktopGameSupervisorError::Deadline(error)),
        }
    }

    async fn observe_work(&mut self, deadline: Instant) -> Result<(), DesktopGameSupervisorError> {
        let mut retry_failure = self.save_failure.is_some() && self.saving.is_none();
        let mut resave_required = retry_failure;
        self.observe_computation(deadline).await?;
        loop {
            if let Some(saving) = &mut self.saving {
                let result = timeout_at(deadline, saving)
                    .await
                    .map_err(DesktopGameSupervisorError::Deadline)?;
                self.saving = None;
                if result.is_err() {
                    retry_failure = false;
                }
                self.finish_save(result)?;
            }
            if self.save_failure.is_some() && !self.events.borrow().snapshot.is_save_required() {
                let engine = match self.events.borrow().snapshot.seats().mode() {
                    GameEnumMode::LocalTwoPlayer => GamePauseEnumEngineBarrier::NoEngine,
                    GameEnumMode::HumanVersusEngine => GamePauseEnumEngineBarrier::FailedEngine,
                };
                self.schedule(
                    DesktopGameSupervisorEnumComputation::SaveFailure {
                        engine,
                        received_at: self
                            .save_failure_at
                            .ok_or(DesktopGameSupervisorError::StateUnavailable)?,
                    },
                    None,
                );
                self.observe_computation(deadline).await?;
            }
            if resave_required {
                resave_required = false;
                self.schedule(
                    DesktopGameSupervisorEnumComputation::Resave { quit: false },
                    None,
                );
                self.observe_computation(deadline).await?;
            }
            self.begin_save(retry_failure);
            if self.saving.is_none() {
                return if self.writes.is_empty() && self.save_failure.is_none() {
                    Ok(())
                } else {
                    Err(DesktopGameSupervisorError::SaveRequired)
                };
            }
        }
    }

    async fn observe_computation(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopGameSupervisorError> {
        if let Some(computing) = &mut self.computing {
            let result = timeout_at(deadline, computing)
                .await
                .map_err(DesktopGameSupervisorError::Deadline)?;
            self.computing = None;
            self.finish_compute(result)?;
        }
        Ok(())
    }

    async fn compute_ready(
        waiting: &mut Option<DesktopGameSupervisorComputeFuture>,
    ) -> Result<DesktopGameSupervisorComputed, DesktopGameSupervisorError> {
        match waiting {
            Some(future) => future.await,
            None => pending().await,
        }
    }
    async fn save_ready(
        waiting: &mut Option<DesktopGameSupervisorSaveFuture>,
    ) -> Result<GameStorageReceipt, StorageGameDocumentError> {
        match waiting {
            Some(future) => future.await,
            None => pending().await,
        }
    }
    async fn factory_ready(
        factory: &mut F,
        context: Option<GameEngineContext>,
    ) -> Result<F::Engine, F::Error> {
        match context {
            Some(context) => factory.create(context).await,
            None => pending().await,
        }
    }
    async fn engine_ready(
        engine: &mut Option<F::Engine>,
        context: GameEngineContext,
        operation: Option<DesktopGameSupervisorEnumEngineOperation>,
    ) -> Result<DesktopGameSupervisorEnumEngineActivity, <F::Engine as GameEngineProvider>::Error>
    {
        match (engine, operation) {
            (Some(engine), Some(DesktopGameSupervisorEnumEngineOperation::Start(request))) => {
                engine
                    .start(*request)
                    .await
                    .map(DesktopGameSupervisorEnumEngineActivity::Started)
            }
            (Some(engine), Some(DesktopGameSupervisorEnumEngineOperation::Events)) => engine
                .next_event(context)
                .await
                .map(DesktopGameSupervisorEnumEngineActivity::Event),
            (Some(engine), Some(DesktopGameSupervisorEnumEngineOperation::Resources)) => engine
                .refresh_resources()
                .await
                .map(|()| DesktopGameSupervisorEnumEngineActivity::ResourcesUpdated),
            (Some(_), None) | (None, Some(_) | None) => pending().await,
        }
    }

    async fn wait_deadline(deadline: Option<Instant>) {
        match deadline {
            Some(deadline) => tokio::time::sleep_until(deadline).await,
            None => pending().await,
        }
    }
}
