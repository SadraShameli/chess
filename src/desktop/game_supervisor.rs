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

#[cfg(test)]
#[path = "../../tests/desktop/game_content.rs"]
mod content_tests;

mod content;
mod editor;

use crate::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use crate::capacity::phase::CapacityPhaseGeneration;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumPending, DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use crate::desktop::game_editor::{
    DesktopGameEditorContext, DesktopGameEditorError, DesktopGameEditorPublicationError,
};
use crate::desktop::game_snapshot::{
    DesktopGameSnapshot, DesktopGameSnapshotAllocation, DesktopGameSnapshotEnumAdmission,
    DesktopGameSnapshotEnumNotice, DesktopGameSnapshotError,
};
use crate::desktop::game_supervisor::content::DesktopGameSupervisorContent;
pub use crate::desktop::game_supervisor::editor::DesktopGameSupervisorEditorReceipt;
use crate::desktop::game_supervisor::editor::{
    DesktopGameSupervisorEditorApplication, DesktopGameSupervisorEditorComputeContext,
    DesktopGameSupervisorEditorEnumOperation, DesktopGameSupervisorEditorEnvelope,
    DesktopGameSupervisorEditorProjection, DesktopGameSupervisorEditorReplies,
    DesktopGameSupervisorEditorReply, DesktopGameSupervisorEnumReply,
};
use crate::desktop::game_transition::{
    DesktopGameTransitionEnumEffect, DesktopGameTransitionError, DesktopGameTransitionOptions,
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
use crate::game::session::lifecycle::GameSessionLifecycleEnumRequest;
use crate::game::session::{GameSession, GameSessionError, GameSessionMoveRequest};
use crate::game::storage_repository::{
    GameStorageEnumEditCause, GameStorageReceipt, GameStorageRepository, GameStorageRevision,
    GameStorageSaveRequest, GameStorageSaveRequestError, GameStorageWallTimeError,
    GameStorageWallTimeProvider,
};
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::game_document::StorageGameDocumentError;
use crate::storage::library_played::StorageLibraryPlayedRepository;
use crate::storage::library_repository::StorageLibraryError;

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
    #[error(transparent)]
    Editor(#[from] DesktopGameEditorError),
    #[error("Open the played game editor before submitting a bound editor action")]
    EditorNotOpen,
    #[error("The application receipt does not own this editor outcome")]
    EditorReceipt,
    #[error("The editor action was cancelled before acceptance")]
    EditorCancelled,
    #[error("The variation token does not identify this source's retained proof")]
    EditorVariationToken,
    #[error("The variation scope offset is outside its retained proof")]
    EditorScopeOffset,
    #[error("The accepted played editor action at {context:?} awaits publication: {source}")]
    EditorPublicationPending {
        context: DesktopGameEditorContext,
        #[source]
        source: Arc<DesktopGameEditorPublicationError>,
    },
    #[error("The queued premove at {context:?} could not complete: {source}")]
    Premove {
        context: GameEngineContext,
        #[source]
        source: Box<Self>,
    },
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
    Storage(#[from] StorageLibraryError),
    #[error(transparent)]
    WallTime(#[from] GameStorageWallTimeError),
    #[error(transparent)]
    Capture(#[from] GameStorageSaveRequestError),
    #[error("The game retains a storage failure ({storage}) and wall-time failure ({wall_time})")]
    ContentSave {
        #[source]
        storage: Arc<Self>,
        wall_time: Arc<Self>,
    },
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

enum DesktopGameSupervisorEnumOrdinary {
    Game(DesktopGameSupervisorEnvelope),
    Editor(DesktopGameSupervisorEditorEnvelope),
}

impl DesktopGameSupervisorEnumOrdinary {
    fn close(self) {
        let failure = Arc::new(DesktopGameSupervisorError::Closed);
        match self {
            Self::Game(envelope) => envelope.complete(Err(failure)),
            Self::Editor(envelope) => envelope.complete(Err(failure)),
        }
    }
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
    ordinary: mpsc::Sender<DesktopGameSupervisorEnumOrdinary>,
    controls: mpsc::Sender<DesktopGameSupervisorEnvelope>,
    held_controls: Arc<AtomicU8>,
    controls_available: Arc<Notify>,
    closed: Arc<AtomicBool>,
    events: watch::Receiver<DesktopGameSupervisorEvent>,
    editor_replies: Arc<DesktopGameSupervisorEditorReplies>,
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
        let envelope = DesktopGameSupervisorEnvelope {
            request,
            response,
            _control: control,
        };
        if envelope._control.is_some() {
            self.controls
                .try_send(envelope)
                .map_err(Self::admission_error)?;
        } else {
            self.ordinary
                .try_send(DesktopGameSupervisorEnumOrdinary::Game(envelope))
                .map_err(Self::admission_error)?;
        }
        Ok(DesktopGameSupervisorReceipt {
            request_id,
            response: receiver,
        })
    }

    fn admission_error<T>(error: mpsc::error::TrySendError<T>) -> DesktopGameSupervisorError {
        match error {
            mpsc::error::TrySendError::Full(_) => DesktopGameSupervisorError::Busy,
            mpsc::error::TrySendError::Closed(_) => DesktopGameSupervisorError::Closed,
        }
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
    pub wall_time: Arc<dyn GameStorageWallTimeProvider>,
    pub proof: Arc<P>,
    pub factory: F,
    pub search_config: GameEngineSearchConfig,
}

struct DesktopGameSupervisorSession(Mutex<Option<DesktopGameSupervisorContent>>);

impl DesktopGameSupervisorSession {
    fn take(&self) -> Result<DesktopGameSupervisorContent, DesktopGameSupervisorError> {
        self.0
            .lock()
            .map_err(|_poisoned| DesktopGameSupervisorError::Poisoned)?
            .take()
            .ok_or(DesktopGameSupervisorError::StateUnavailable)
    }
    fn replace(
        &self,
        session: DesktopGameSupervisorContent,
    ) -> Result<(), DesktopGameSupervisorError> {
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
    fn is_current(&self) -> bool {
        self.0.lock().is_ok_and(|owner| {
            owner
                .as_ref()
                .is_some_and(DesktopGameSupervisorContent::is_current)
        })
    }

    fn acknowledge(
        &self,
        receipt: GameStorageReceipt,
    ) -> Result<GameSaveEnumStatus, DesktopGameSupervisorError> {
        let mut session = self.take()?;
        let result = session.acknowledge(receipt);
        self.replace(session)?;
        result
    }
}

enum DesktopGameSupervisorEnumComputation {
    Initialize,
    Action(DesktopGameTransitionOptions),
    EngineMove(GameEngineMove),
    Premove(DesktopGameSupervisorPremove),
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
    Editor {
        application: DesktopGameSupervisorEditorApplication,
        operation: DesktopGameSupervisorEditorEnumOperation,
        storage: StorageDocumentClient,
    },
}

#[derive(Clone, Copy)]
struct DesktopGameSupervisorPremove {
    expected: GameEngineContext,
    checkpoint_at: ClockInstant,
}

impl DesktopGameSupervisorPremove {
    fn validate(&self, state: &GameSession) -> Result<(), DesktopGameSupervisorError> {
        if self.expected.game() != state.id() {
            return Err(DesktopGameSupervisorError::StateUnavailable);
        }
        state.revision().check_expected(self.expected.revision())?;
        Ok(())
    }
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
    wall_time: Arc<dyn GameStorageWallTimeProvider>,
    proof: Arc<P>,
    config: GameEngineSearchConfig,
    storage_revision: GameStorageRevision,
}

struct DesktopGameSupervisorComputeContext<'a, C, P> {
    clock: &'a C,
    wall_time: &'a dyn GameStorageWallTimeProvider,
    proof: &'a P,
    config: GameEngineSearchConfig,
    storage_revision: GameStorageRevision,
}

struct DesktopGameSupervisorComputed {
    snapshot: DesktopGameSnapshot,
    save: Option<GameStorageSaveRequest>,
    search: Option<GameEngineRequest>,
    premove: Option<DesktopGameSupervisorPremove>,
    quit: bool,
    failure: Option<Arc<DesktopGameSupervisorError>>,
    editor: Option<Result<DesktopAnnotationCommandsEnumResult, Arc<DesktopGameSupervisorError>>>,
    publishing: bool,
}

type DesktopGameSupervisorComputeFuture = Pin<
    Box<
        dyn Future<Output = Result<DesktopGameSupervisorComputed, DesktopGameSupervisorError>>
            + Send,
    >,
>;
type DesktopGameSupervisorSaveFuture =
    Pin<Box<dyn Future<Output = Result<GameStorageReceipt, StorageLibraryError>> + Send>>;

struct DesktopGameSupervisorWrite {
    request: GameStorageSaveRequest,
    response: Option<(DesktopGameSnapshot, DesktopGameSupervisorEnumReply)>,
    quit: bool,
}

struct DesktopGameSupervisorWriteContext {
    request: GameStorageSaveRequest,
    response: Option<(DesktopGameSnapshot, DesktopGameSupervisorEnumReply)>,
    quit: bool,
}

pub struct DesktopGameSupervisor<F: DesktopGameSupervisorEngineFactory, C, P> {
    session: Arc<DesktopGameSupervisorSession>,
    storage: StorageDocumentClient,
    storage_revision: GameStorageRevision,
    executor: Arc<CapacityExecutor>,
    clock: Arc<C>,
    wall_time: Arc<dyn GameStorageWallTimeProvider>,
    proof: Arc<P>,
    factory: F,
    factory_shutdown_complete: bool,
    engine: Option<F::Engine>,
    active_search: Option<GameEngineSearchIdentity>,
    search_request: Option<GameEngineRequest>,
    search_config: GameEngineSearchConfig,
    preparation: Option<CapacityPhaseGeneration>,
    ordinary: mpsc::Receiver<DesktopGameSupervisorEnumOrdinary>,
    deferred_ordinary: Option<DesktopGameSupervisorEnumOrdinary>,
    controls: mpsc::Receiver<DesktopGameSupervisorEnvelope>,
    deferred_controls: VecDeque<DesktopGameSupervisorEnvelope>,
    events: watch::Sender<DesktopGameSupervisorEvent>,
    closed: Arc<AtomicBool>,
    computing: Option<DesktopGameSupervisorComputeFuture>,
    computing_response: Option<DesktopGameSupervisorEnvelope>,
    computing_editor: Option<DesktopGameSupervisorEditorEnvelope>,
    editor_projections: VecDeque<DesktopGameSupervisorEditorProjection>,
    editor_publication: Option<Box<DesktopGameSupervisorEditorEnvelope>>,
    factory_request: Option<DesktopGameSupervisorEnvelope>,
    pending_engine_move: Option<GameEngineMove>,
    pending_premove: Option<DesktopGameSupervisorPremove>,
    pending_engine_failure: bool,
    controls_closed: bool,
    ordinary_closed: bool,
    writes: VecDeque<DesktopGameSupervisorWrite>,
    saving: Option<DesktopGameSupervisorSaveFuture>,
    completed_save: Option<Result<GameStorageReceipt, StorageLibraryError>>,
    saving_context: Option<DesktopGameSupervisorWriteContext>,
    save_failure: Option<Arc<DesktopGameSupervisorError>>,
    save_failure_at: Option<ClockInstant>,
    content_failure: Option<Arc<DesktopGameSupervisorError>>,
    quit_ready: bool,
    last_storage_acknowledgment: Option<GameStorageReceipt>,
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
            DesktopGameSupervisorContent::new(options.session),
        ))));
        let computed = Self::computation(
            DesktopGameSupervisorComputeOptions {
                session: Arc::clone(&session),
                executor: Arc::clone(&options.executor),
                clock: Arc::clone(&options.clock),
                wall_time: Arc::clone(&options.wall_time),
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
            editor_replies: Arc::new(DesktopGameSupervisorEditorReplies::default()),
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
                wall_time: options.wall_time,
                proof: options.proof,
                factory: options.factory,
                factory_shutdown_complete: false,
                last_storage_acknowledgment: None,
                engine: None,
                active_search: None,
                search_request: computed.search,
                search_config: options.search_config,
                preparation: None,
                ordinary: ordinary_receiver,
                deferred_ordinary: None,
                controls: control_receiver,
                deferred_controls: VecDeque::with_capacity(
                    DesktopGameSupervisorPort::CONTROL_CAPACITY,
                ),
                events,
                closed,
                computing: None,
                computing_response: None,
                computing_editor: None,
                editor_projections: VecDeque::new(),
                editor_publication: None,
                factory_request: None,
                pending_engine_move: None,
                pending_premove: None,
                pending_engine_failure: false,
                controls_closed: false,
                ordinary_closed: false,
                writes,
                saving: None,
                completed_save: None,
                saving_context: None,
                save_failure: None,
                save_failure_at: None,
                content_failure: computed.failure,
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
                    let continuation = match &operation {
                        DesktopGameSupervisorEnumComputation::Premove(pending) => {
                            Some(pending.expected)
                        }
                        DesktopGameSupervisorEnumComputation::Initialize
                        | DesktopGameSupervisorEnumComputation::Action(_)
                        | DesktopGameSupervisorEnumComputation::EngineMove(_)
                        | DesktopGameSupervisorEnumComputation::Clock
                        | DesktopGameSupervisorEnumComputation::Search
                        | DesktopGameSupervisorEnumComputation::EngineFailure
                        | DesktopGameSupervisorEnumComputation::SaveFailure { .. }
                        | DesktopGameSupervisorEnumComputation::Resave { .. }
                        | DesktopGameSupervisorEnumComputation::Editor { .. } => None,
                    };
                    let result = Self::compute(
                        &mut state,
                        operation,
                        DesktopGameSupervisorComputeContext {
                            clock: &options.clock,
                            wall_time: options.wall_time.as_ref(),
                            proof: &options.proof,
                            config: options.config,
                            storage_revision: options.storage_revision,
                        },
                    );
                    options.session.replace(state)?;
                    result.map_err(|source| match continuation {
                        Some(context) => DesktopGameSupervisorError::Premove {
                            context,
                            source: Box::new(source),
                        },
                        None => source,
                    })
                })
                .await?
                .join()
                .await?
        })
    }

    fn compute(
        state: &mut DesktopGameSupervisorContent,
        operation: DesktopGameSupervisorEnumComputation,
        context: DesktopGameSupervisorComputeContext<'_, C, P>,
    ) -> Result<DesktopGameSupervisorComputed, DesktopGameSupervisorError> {
        let DesktopGameSupervisorComputeContext {
            clock,
            wall_time,
            proof,
            config,
            storage_revision,
        } = context;
        if let DesktopGameSupervisorEnumComputation::Editor {
            application,
            operation,
            storage,
        } = operation
        {
            let now = clock.now()?;
            let computed = state.editor_operation(
                operation,
                DesktopGameSupervisorEditorComputeContext {
                    application: &application,
                    storage_revision,
                    storage: &storage,
                    wall_time,
                    proof,
                },
            );
            let (editor, should_save) = match computed {
                Ok(computed) => (Ok(computed.result), computed.save),
                Err(error @ DesktopGameSupervisorError::EditorPublicationPending { .. }) => {
                    (Err(Arc::new(error)), false)
                }
                Err(error) => return Err(error),
            };
            let snapshot = DesktopGameSnapshot::capture(&state.session, now)?;
            let save = if should_save && state.pending_result.is_none() {
                Some(GameStorageSaveRequest::capture_with_change(
                    &state.session,
                    now,
                    storage_revision,
                    state.change(),
                )?)
            } else {
                None
            };
            return Ok(DesktopGameSupervisorComputed {
                snapshot,
                save,
                search: None,
                premove: None,
                quit: false,
                failure: state.failure(),
                editor: Some(editor),
                publishing: state.editor.is_publishing(),
            });
        }
        if let DesktopGameSupervisorEnumComputation::Premove(pending) = &operation {
            pending.validate(&state.session)?;
        } else if let DesktopGameSupervisorEnumComputation::Action(options) = &operation {
            state
                .session
                .revision()
                .check_expected(options.request.expected_revision)?;
        } else if let DesktopGameSupervisorEnumComputation::EngineMove(result) = &operation {
            let expected = result.identity().context();
            if expected.game() != state.session.id() {
                return Err(DesktopGameSupervisorError::StateUnavailable);
            }
            state
                .session
                .revision()
                .check_expected(expected.revision())?;
        }
        let mut now = clock.now()?;
        let editor_before = (state.session.revision(), state.session.view());
        let editor_generation = state.editor.next_generation()?;
        let mut notice = None;
        let mut premove = None;
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
                    let timestamp = wall_time.now()?;
                    state.record(GameStorageEnumEditCause::Created, timestamp);
                    DesktopGameTransitionEnumEffect::Autosave
                } else {
                    DesktopGameTransitionEnumEffect::SnapshotOnly
                }
            }
            DesktopGameSupervisorEnumComputation::Action(options) => {
                now = options.request.ordered_received_at(state.session.clock());
                state.apply(options, proof, wall_time)?
            }
            DesktopGameSupervisorEnumComputation::EngineMove(result) => {
                now = result.received_at();
                let color = state
                    .session
                    .tree()
                    .node(state.session.live())
                    .map_err(GameSessionError::from)?
                    .position()
                    .turn();
                let timestamp = wall_time.now()?;
                let before = state.observe();
                state.session.accept_move(
                    GameSessionMoveRequest {
                        expected_revision: state.session.revision(),
                        seat: GameEnumSeat::Reckless,
                        color,
                        node: uuid::Uuid::new_v4().to_string().parse()?,
                        movement: result.movement(),
                        received_at: now,
                    },
                    proof,
                )?;
                state.record_effect(before, GameStorageEnumEditCause::AcceptedMove, timestamp);
                if state.session.phase() == GameEnumPhase::Active
                    && state.session.premove().pending().is_some()
                {
                    premove = Some(DesktopGameSupervisorPremove {
                        expected: GameEngineContext::new(
                            state.session.id(),
                            state.session.revision(),
                        ),
                        checkpoint_at: now,
                    });
                }
                DesktopGameTransitionEnumEffect::Autosave
            }
            DesktopGameSupervisorEnumComputation::Premove(pending) => {
                let position = state
                    .session
                    .tree()
                    .node(state.session.live())
                    .map_err(GameSessionError::from)?
                    .position();
                let timestamp = state
                    .session
                    .premove()
                    .pending()
                    .filter(|intent| {
                        position
                            .move_intent(intent.origin(), intent.destination(), intent.promotion())
                            .is_ok()
                    })
                    .map(|_intent| wall_time.now())
                    .transpose()?;
                match state.session.resolve_premove(state.session.revision())? {
                    GamePremoveEnumResolution::Execute(movement) => {
                        let color = state
                            .session
                            .tree()
                            .node(state.session.live())
                            .map_err(GameSessionError::from)?
                            .position()
                            .turn();
                        let timestamp =
                            timestamp.ok_or(DesktopGameSupervisorError::StateUnavailable)?;
                        let before = state.observe();
                        state.session.accept_move(
                            GameSessionMoveRequest {
                                expected_revision: state.session.revision(),
                                seat: GameEnumSeat::Human,
                                color,
                                node: uuid::Uuid::new_v4().to_string().parse()?,
                                movement,
                                received_at: now,
                            },
                            proof,
                        )?;
                        state.record_effect(
                            before,
                            GameStorageEnumEditCause::AcceptedMove,
                            timestamp,
                        );
                        DesktopGameTransitionEnumEffect::Autosave
                    }
                    GamePremoveEnumResolution::Empty => {
                        now = pending.checkpoint_at;
                        DesktopGameTransitionEnumEffect::SnapshotOnly
                    }
                    GamePremoveEnumResolution::ClearedIllegal { intent, .. } => {
                        now = pending.checkpoint_at;
                        notice = Some(DesktopGameSnapshotEnumNotice::IllegalPremove {
                            revision: state.session.revision(),
                            intent,
                        });
                        DesktopGameTransitionEnumEffect::SnapshotOnly
                    }
                }
            }
            DesktopGameSupervisorEnumComputation::Clock => {
                if state.lifecycle(
                    GameSessionLifecycleEnumRequest::PollDeadline {
                        expected_revision: state.session.revision(),
                        received_at: now,
                    },
                    proof,
                    wall_time,
                )? {
                    DesktopGameTransitionEnumEffect::Autosave
                } else {
                    DesktopGameTransitionEnumEffect::SnapshotOnly
                }
            }
            DesktopGameSupervisorEnumComputation::Search => {
                DesktopGameTransitionEnumEffect::SnapshotOnly
            }
            DesktopGameSupervisorEnumComputation::EngineFailure => {
                if state.pending_result.is_some() {
                    state.lifecycle(
                        GameSessionLifecycleEnumRequest::PollDeadline {
                            expected_revision: state.session.revision(),
                            received_at: now,
                        },
                        proof,
                        wall_time,
                    )?;
                    DesktopGameTransitionEnumEffect::SnapshotOnly
                } else if state.session.phase() == GameEnumPhase::Active {
                    let engine = if state.session.seats().mode() == GameEnumMode::LocalTwoPlayer {
                        GamePauseEnumEngineBarrier::NoEngine
                    } else {
                        GamePauseEnumEngineBarrier::FailedEngine
                    };
                    state.lifecycle(
                        GameSessionLifecycleEnumRequest::Pause(GamePauseRequest {
                            expected_revision: state.session.revision(),
                            cause: GamePauseEnumCause::Fault(GamePauseEnumFault::Engine),
                            received_at: now,
                            engine,
                        }),
                        proof,
                        wall_time,
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
                if state.session.phase() == GameEnumPhase::Active {
                    state.lifecycle(
                        GameSessionLifecycleEnumRequest::Pause(GamePauseRequest {
                            expected_revision: state.session.revision(),
                            cause: if matches!(engine, GamePauseEnumEngineBarrier::FailedEngine) {
                                GamePauseEnumCause::Fault(GamePauseEnumFault::Engine)
                            } else {
                                GamePauseEnumCause::AutosaveFailure
                            },
                            received_at: now,
                            engine,
                        }),
                        proof,
                        wall_time,
                    )?;
                }
                if state.pending_result.is_none() {
                    state.session.interrupt_for_save_failure(
                        GameSaveFailureRequest {
                            expected_revision: state.session.revision(),
                            received_at: now,
                            engine: GamePauseEnumEngineBarrier::NoEngine,
                        },
                        proof,
                    )?;
                }
                DesktopGameTransitionEnumEffect::SnapshotOnly
            }
            DesktopGameSupervisorEnumComputation::Editor { .. } => {
                return Err(DesktopGameSupervisorError::StateUnavailable);
            }
        };
        state
            .editor
            .observe(editor_before, &state.session, editor_generation);
        let mut snapshot = DesktopGameSnapshot::capture(&state.session, now)?;
        snapshot.retain_notice(notice);
        let save = if state.pending_result.is_none()
            && matches!(
                effect,
                DesktopGameTransitionEnumEffect::Autosave | DesktopGameTransitionEnumEffect::Quit
            ) {
            Some(GameStorageSaveRequest::capture_with_change(
                &state.session,
                now,
                storage_revision,
                state.change(),
            )?)
        } else {
            None
        };
        let search = if state.session.phase() == GameEnumPhase::Active
            && state.session.seats().seat(snapshot.live_turn()) == GameEnumSeat::Reckless
        {
            Some(state.session.engine_request(config)?)
        } else {
            None
        };
        Ok(DesktopGameSupervisorComputed {
            snapshot,
            save,
            search,
            premove,
            quit: state.pending_result.is_none()
                && matches!(effect, DesktopGameTransitionEnumEffect::Quit),
            failure: state.failure(),
            editor: None,
            publishing: state.editor.is_publishing(),
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
                wall_time: Arc::clone(&self.wall_time),
                proof: Arc::clone(&self.proof),
                config: self.search_config,
                storage_revision: self.storage_revision,
            },
            operation,
        ));
        self.computing_response = response;
        self.set_saving_computing(true);
    }

    fn set_saving_computing(&self, computing: bool) {
        for (_, response) in self
            .writes
            .iter()
            .filter_map(|write| write.response.as_ref())
            .chain(
                self.saving_context
                    .as_ref()
                    .and_then(|context| context.response.as_ref()),
            )
        {
            if let DesktopGameSupervisorEnumReply::Editor(reply) = response {
                reply.envelope.phase.send_replace(Some(
                    DesktopAnnotationCommandsEnumPending::Saving { computing },
                ));
            }
        }
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
            if self.computing.is_none()
                && let Some(result) = self.completed_save.take()
            {
                self.finish_save(result)?;
            }
            if self.quit_ready
                && self.computing.is_none()
                && self.saving.is_none()
                && self.writes.is_empty()
                && self.editor_projections.is_empty()
                && self.editor_publication.is_none()
                && self.save_failure.is_none()
            {
                return Ok(());
            }
            if self.computing.is_none() {
                let control = match self.deferred_controls.pop_front() {
                    Some(control) => Some(control),
                    None => match self.controls.try_recv() {
                        Ok(control) => Some(control),
                        Err(mpsc::error::TryRecvError::Empty) => None,
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            self.controls_closed = true;
                            None
                        }
                    },
                };
                if let Some(control) = control {
                    self.handle(control).await?;
                } else if self.save_failure.is_some()
                    && !self.events.borrow().snapshot.is_save_required()
                {
                    let engine = match self.barrier(Instant::now().into_std()).await {
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
                } else if let Some(pending) = self.pending_premove.take()
                    && self.context() == pending.expected
                    && self.events.borrow().snapshot.phase() == GameEnumPhase::Active
                {
                    self.schedule(DesktopGameSupervisorEnumComputation::Premove(pending), None);
                } else if self
                    .deadline()?
                    .is_some_and(|deadline| Instant::now() >= deadline)
                {
                    self.adjudicate_clock().await?;
                } else if self.saving.is_none()
                    && self.writes.is_empty()
                    && self.factory_request.is_none()
                    && let Some(response) = self.deferred_ordinary.take()
                {
                    match response {
                        DesktopGameSupervisorEnumOrdinary::Game(response) => {
                            self.handle(response).await?
                        }
                        DesktopGameSupervisorEnumOrdinary::Editor(response) => {
                            self.handle_editor(response)
                        }
                    }
                }
            }
            if self.controls_closed
                && self.ordinary_closed
                && self.computing.is_none()
                && self.saving.is_none()
                && self.writes.is_empty()
                && self.editor_projections.is_empty()
                && self.deferred_ordinary.is_none()
                && !self.quit_ready
            {
                if self.content_failure.is_some() {
                    return Err(DesktopGameSupervisorError::SaveRequired);
                }
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
            self.refresh_resources_view(matches!(
                &engine_operation,
                Some(DesktopGameSupervisorEnumEngineOperation::Start(_))
            ));
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
                result = Self::save_ready(&mut self.saving), if self.saving.is_some() => {
                    self.saving = None;
                    if self.computing.is_some() { self.completed_save = Some(result); }
                    else { self.finish_save(result)?; }
                }
                result = Self::factory_ready(&mut self.factory, factory_context), if factory_context.is_some() && self.preparation.is_none() && self.computing.is_none() => {
                    let response = self.factory_request.take().ok_or(DesktopGameSupervisorError::StateUnavailable)?;
                    match result {
                        Ok(engine) => {
                            self.engine = Some(engine);
                            let mut response = response;
                            response.request.received_at = self.clock.now()?;
                            self.handle(response).await?;
                        }
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
                    self.adjudicate_clock().await?;
                }
                () = std::future::ready(()), if !self.editor_projections.is_empty() && self.computing.is_none() && self.editor_publication.is_none() => {
                    self.project_editor()?;
                }
                ordinary = self.ordinary.recv(), if !self.ordinary_closed && self.computing.is_none() && self.deferred_ordinary.is_none() && self.factory_request.is_none() => {
                    match ordinary {
                        Some(DesktopGameSupervisorEnumOrdinary::Game(request)) if self.saving.is_some() || !self.writes.is_empty() => self.deferred_ordinary = Some(DesktopGameSupervisorEnumOrdinary::Game(request)),
                        Some(DesktopGameSupervisorEnumOrdinary::Editor(request)) if matches!(request.operation, DesktopGameSupervisorEditorEnumOperation::Open) && (self.saving.is_some() || !self.writes.is_empty()) => self.deferred_ordinary = Some(DesktopGameSupervisorEnumOrdinary::Editor(request)),
                        Some(DesktopGameSupervisorEnumOrdinary::Game(request)) => self.handle(request).await?,
                        Some(DesktopGameSupervisorEnumOrdinary::Editor(request)) => self.handle_editor(request),
                        None => self.ordinary_closed = true,
                    }
                }
            }
        }
    }

    fn context(&self) -> GameEngineContext {
        let snapshot = &self.events.borrow().snapshot;
        GameEngineContext::new(snapshot.id(), snapshot.revision())
    }

    async fn adjudicate_clock(&mut self) -> Result<(), DesktopGameSupervisorError> {
        self.search_request = None;
        if self.active_search.is_some() {
            let _barrier = self.barrier(Instant::now().into_std()).await?;
        }
        self.schedule(DesktopGameSupervisorEnumComputation::Clock, None);
        Ok(())
    }

    fn handle_editor(&mut self, mut envelope: DesktopGameSupervisorEditorEnvelope) {
        let operation = std::mem::replace(
            &mut envelope.operation,
            DesktopGameSupervisorEditorEnumOperation::Open,
        );
        if matches!(operation, DesktopGameSupervisorEditorEnumOperation::Publish)
            && self
                .editor_publication
                .as_ref()
                .is_some_and(|pending| pending.application.shares_outcome(&envelope.application))
        {
            self.editor_publication = None;
        }
        self.schedule(
            DesktopGameSupervisorEnumComputation::Editor {
                application: DesktopGameSupervisorEditorApplication::capture(&envelope.application),
                operation,
                storage: self.storage.clone(),
            },
            None,
        );
        self.computing_editor = Some(envelope);
    }

    fn project_editor(&mut self) -> Result<(), DesktopGameSupervisorError> {
        let pending = self
            .editor_projections
            .pop_front()
            .ok_or(DesktopGameSupervisorError::StateUnavailable)?;
        let mut envelope = pending.envelope;
        envelope.operation = DesktopGameSupervisorEditorEnumOperation::Project(pending.result);
        self.handle_editor(envelope);
        Ok(())
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
                    let _barrier = self.barrier(Instant::now().into_std()).await?;
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
                DesktopGameCommandsEnumAction::RetryEngine | DesktopGameCommandsEnumAction::Resume
            )
        {
            let state = self.session.take()?;
            let validation = if matches!(
                response.request.action,
                DesktopGameCommandsEnumAction::RetryEngine
            ) {
                state.session.validate_recovery(
                    response.request.expected_revision,
                    GamePauseEnumFault::Engine,
                )
            } else {
                state
                    .session
                    .validate_resume(response.request.expected_revision)
            };
            self.session.replace(state)?;
            if let Err(error) = validation {
                response.complete(Err(Arc::new(DesktopGameSupervisorError::Transition(
                    error.into(),
                ))));
                return Ok(());
            }
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
            let now = Instant::now();
            let elapsed = self
                .clock
                .now()?
                .elapsed_since(response.request.received_at)?;
            let received_at = now
                .checked_sub(elapsed.as_duration())
                .ok_or(DesktopGameSupervisorError::StateUnavailable)?;
            match self.barrier(received_at.into_std()).await {
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

    async fn barrier(
        &mut self,
        received_at: StandardInstant,
    ) -> Result<GamePauseEnumEngineBarrier, DesktopGameSupervisorError> {
        let context = self.context();
        match &mut self.engine {
            Some(engine) => {
                let receipt = engine
                    .quiesce(context, received_at)
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
        self.set_saving_computing(false);
        let editor = self.computing_editor.take();
        let is_editor = editor.is_some();
        match result {
            Err(error) => {
                if let Some(editor) = editor {
                    editor.complete(Err(Arc::new(error)));
                    Ok(())
                } else if let Some(response) = response {
                    response.complete(Err(Arc::new(error)));
                    Ok(())
                } else if matches!(error, DesktopGameSupervisorError::Premove { .. }) {
                    self.publish_notice(Arc::new(error));
                    Ok(())
                } else {
                    Err(error)
                }
            }
            Ok(computed) => {
                self.content_failure = computed.failure;
                if !is_editor {
                    self.search_request = computed.search;
                }
                if let Some(expected) = computed.premove {
                    self.pending_premove = Some(expected);
                }
                let mut event = self.events.borrow().clone();
                let notice = event.snapshot.notice();
                event.snapshot = computed.snapshot;
                event.snapshot.retain_notice(notice);
                if let Some(request) = computed.save {
                    let response = match (response, editor, computed.editor) {
                        (Some(response), None, None) => {
                            Some(DesktopGameSupervisorEnumReply::Game(response))
                        }
                        (None, Some(envelope), Some(Ok(result))) => {
                            envelope.phase.send_replace(Some(
                                DesktopAnnotationCommandsEnumPending::Saving { computing: false },
                            ));
                            Some(DesktopGameSupervisorEnumReply::Editor(Box::new(
                                DesktopGameSupervisorEditorReply { envelope, result },
                            )))
                        }
                        (None, None, None) => None,
                        _ => return Err(DesktopGameSupervisorError::StateUnavailable),
                    };
                    self.writes.push_back(DesktopGameSupervisorWrite {
                        request,
                        response: response.map(|response| (event.snapshot.clone(), response)),
                        quit: computed.quit,
                    });
                } else if let Some(editor) = editor {
                    let result = match computed.editor {
                        Some(result) => result,
                        None => Err(Arc::new(DesktopGameSupervisorError::StateUnavailable)),
                    };
                    let publishing = matches!(
                        &result,
                        Err(error) if matches!(error.as_ref(), DesktopGameSupervisorError::EditorPublicationPending { .. })
                    );
                    editor.complete(result);
                    if publishing {
                        if self.editor_publication.as_ref().is_some_and(|pending| {
                            !pending.application.shares_outcome(&editor.application)
                        }) {
                            return Err(DesktopGameSupervisorError::EditorReceipt);
                        }
                        self.editor_publication = Some(Box::new(editor));
                    }
                } else if let Some(response) = response {
                    let result =
                        if matches!(response.request.action, DesktopGameCommandsEnumAction::Quit)
                            && event.snapshot.is_save_required()
                        {
                            Err(Arc::new(DesktopGameSupervisorError::SaveRequired))
                        } else {
                            Ok(event.snapshot.clone())
                        };
                    response.complete(result);
                }
                if computed.publishing != self.editor_publication.is_some() {
                    return Err(DesktopGameSupervisorError::StateUnavailable);
                }
                if let Some(failure) = &self.content_failure {
                    event.notice = Some(Arc::clone(failure));
                }
                event.persistence = self.persistence();
                self.events.send_replace(event);
                Ok(())
            }
        }
    }

    fn begin_save(&mut self, retry_failure: bool) {
        if self.saving.is_some()
            || self.completed_save.is_some()
            || !retry_failure
                && self.save_failure.is_some()
                && !self.writes.iter().any(|write| {
                    write
                        .response
                        .as_ref()
                        .is_some_and(|(_, response)| response.is_retry_save())
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
            StorageLibraryPlayedRepository::new(client, &executor)
                .save(request)
                .await
        }));
    }

    fn finish_save(
        &mut self,
        result: Result<GameStorageReceipt, StorageLibraryError>,
    ) -> Result<(), DesktopGameSupervisorError> {
        let context = self
            .saving_context
            .take()
            .ok_or(DesktopGameSupervisorError::StateUnavailable)?;
        match result {
            Ok(receipt) => {
                self.storage_revision = receipt.storage_revision();
                let status = self.session.acknowledge(receipt)?;
                self.last_storage_acknowledgment = Some(receipt);
                if receipt.game_revision() == self.context().revision() {
                    self.save_failure = None;
                    self.save_failure_at = None;
                }
                let mut event = self.events.borrow().clone();
                event.storage_revision = self.storage_revision;
                event.snapshot.observe_save(status, receipt.game_revision());
                let owner = self.session.take()?;
                self.content_failure = owner.failure();
                self.session.replace(owner)?;
                event.notice = self.content_failure.as_ref().map(Arc::clone);
                event.persistence = self.persistence();
                self.quit_ready |= context.quit;
                self.events.send_replace(event);
                if let Some((mut snapshot, response)) = context.response {
                    if receipt.game_revision() == snapshot.revision() {
                        snapshot.observe_save(GameSaveEnumStatus::Current, receipt.game_revision());
                    }
                    match response {
                        DesktopGameSupervisorEnumReply::Game(response) => {
                            response.complete(Ok(snapshot))
                        }
                        DesktopGameSupervisorEnumReply::Editor(reply) => {
                            let DesktopGameSupervisorEditorReply { envelope, result } = *reply;
                            envelope
                                .storage_receipt
                                .set(receipt)
                                .map_err(|_duplicate| {
                                    DesktopGameSupervisorError::StateUnavailable
                                })?;
                            envelope.phase.send_replace(Some(
                                DesktopAnnotationCommandsEnumPending::Projecting {
                                    storage_revision: receipt.storage_revision(),
                                    covered: receipt.game_revision(),
                                },
                            ));
                            envelope.response.send_replace(Some(Ok(
                                DesktopAnnotationCommandsEnumResult::PendingProjection {
                                    result: Box::new(result.clone()),
                                    storage_revision: receipt.storage_revision(),
                                    covered: receipt.game_revision(),
                                },
                            )));
                            self.editor_projections.push_back(
                                DesktopGameSupervisorEditorProjection { envelope, result },
                            );
                        }
                    }
                }
            }
            Err(
                StorageLibraryError::Cancelled
                | StorageLibraryError::Codec(StorageGameDocumentError::Cancelled),
            ) => {
                self.writes.push_front(DesktopGameSupervisorWrite {
                    request: context.request,
                    response: context.response,
                    quit: context.quit,
                });
            }
            Err(error) => {
                let failure = Arc::new(DesktopGameSupervisorError::Storage(error));
                if let Some((_, response)) = context.response {
                    response.fail(Arc::clone(&failure));
                }
                for write in self.writes.drain(..) {
                    if let Some((_, response)) = write.response {
                        response.fail(Arc::clone(&failure));
                    }
                }
                self.search_request = None;
                self.pending_engine_move = None;
                self.save_failure_at = Some(self.clock.now()?);
                self.save_failure = Some(Arc::clone(&failure));
                let mut owner = self.session.take()?;
                owner.storage_failure = Some(Arc::clone(&failure));
                self.content_failure = owner.failure();
                self.session.replace(owner)?;
                self.publish_notice(self.content_failure.as_ref().map_or(failure, Arc::clone));
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
            || self.completed_save.is_some()
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

    fn refresh_resources_view(&self, start_pending: bool) {
        let admission = if start_pending {
            DesktopGameSnapshotEnumAdmission::WaitingForCapacity
        } else {
            DesktopGameSnapshotEnumAdmission::Admitted
        };
        let allocation = self
            .engine
            .as_ref()
            .and_then(|engine| engine.allocation_view())
            .map(|view| DesktopGameSnapshotAllocation::new(view.workers(), view.hash_mib()));
        self.events.send_if_modified(|event| {
            if event.snapshot.admission() == admission && event.snapshot.allocation() == allocation
            {
                return false;
            }
            event.snapshot.set_resources(admission, allocation);
            true
        });
    }

    fn publish_notice(&self, failure: Arc<DesktopGameSupervisorError>) {
        let mut event = self.events.borrow().clone();
        event.notice = Some(failure);
        event.persistence = self.persistence();
        self.events.send_replace(event);
    }

    fn persistence(&self) -> DesktopGameSupervisorEnumPersistence {
        match self.content_failure.as_ref().or(self.save_failure.as_ref()) {
            Some(failure) => DesktopGameSupervisorEnumPersistence::Failed(Arc::clone(failure)),
            None if self.saving.is_some()
                || self.completed_save.is_some()
                || !self.writes.is_empty()
                || !self.session.is_current() =>
            {
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

    pub const fn last_storage_acknowledgment(&self) -> Option<GameStorageReceipt> {
        self.last_storage_acknowledgment
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.closed.load(Ordering::Acquire)
            && self.computing.is_none()
            && self.saving.is_none()
            && self.completed_save.is_none()
            && self.writes.is_empty()
            && self.editor_projections.is_empty()
            && self.editor_publication.is_none()
            && self.factory_request.is_none()
            && self.factory_shutdown_complete
            && self.save_failure.is_none()
            && self.content_failure.is_none()
            && self.session.is_current()
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
        self.pending_premove = None;
        while let Ok(response) = self.ordinary.try_recv() {
            response.close();
        }
        if let Some(response) = self.deferred_ordinary.take() {
            response.close();
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
        let preparation = self.prepare_shutdown(deadline).await;
        let engine = Self::shutdown_engine(&mut self.engine, deadline);
        let factory = Self::shutdown_factory(&mut self.factory, deadline);
        let (engine, factory) = tokio::join!(engine, factory);
        self.factory_shutdown_complete = factory.is_ok();
        let resources = DesktopGameSupervisorError::combine(engine, factory);
        let work = self.observe_work(deadline).await;
        DesktopGameSupervisorError::combine(
            DesktopGameSupervisorError::combine(preparation, resources),
            work,
        )
    }

    async fn prepare_shutdown(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopGameSupervisorError> {
        self.observe_computation(deadline).await?;
        if self.events.borrow().snapshot.phase() != GameEnumPhase::Active {
            return Ok(());
        }
        let received_at = self.clock.now()?;
        let barrier = timeout_at(deadline, self.barrier(Instant::now().into_std()))
            .await
            .map_err(DesktopGameSupervisorError::Deadline)?;
        match barrier {
            Ok(engine) => {
                self.schedule(
                    DesktopGameSupervisorEnumComputation::Action(DesktopGameTransitionOptions {
                        request: DesktopGameCommandsRequest {
                            request_id: DesktopRequestId::new(),
                            expected_revision: self.context().revision(),
                            received_at,
                            action: DesktopGameCommandsEnumAction::Quit,
                        },
                        engine,
                        readiness: GameEnumEngineReadiness::Unavailable,
                    }),
                    None,
                );
                self.observe_computation(deadline).await
            }
            Err(error) => {
                self.schedule(DesktopGameSupervisorEnumComputation::EngineFailure, None);
                DesktopGameSupervisorError::combine(
                    Err(error),
                    self.observe_computation(deadline).await,
                )
            }
        }
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
        if let Some(result) = self.completed_save.take() {
            self.finish_save(result)?;
        }
        self.observe_editor_projections(deadline).await?;
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
                self.observe_editor_projections(deadline).await?;
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
                return if self.writes.is_empty()
                    && self.save_failure.is_none()
                    && self.session.is_current()
                {
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

    async fn observe_editor_projections(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopGameSupervisorError> {
        self.observe_computation(deadline).await?;
        if let Some(mut publication) = self.editor_publication.take() {
            publication.operation = DesktopGameSupervisorEditorEnumOperation::Publish;
            publication
                .phase
                .send_replace(Some(DesktopAnnotationCommandsEnumPending::Computing));
            publication.response.send_replace(None);
            self.handle_editor(*publication);
            self.observe_computation(deadline).await?;
            if self.editor_publication.is_some() {
                return Err(DesktopGameSupervisorError::SaveRequired);
            }
        }
        while !self.editor_projections.is_empty() {
            self.project_editor()?;
            self.observe_computation(deadline).await?;
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
    ) -> Result<GameStorageReceipt, StorageLibraryError> {
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
