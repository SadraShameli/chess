use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use thiserror::Error;
use tokio::sync::Notify;

use crate::capacity::lease::CapacityLease;
use crate::clock::duration::ClockDuration;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::game_supervisor::{
    DesktopGameSupervisorEngineFactory, DesktopGameSupervisorEngineProvider,
};
use crate::engine::live_game::{EngineLiveGame, EngineLiveGameError};
use crate::engine::package::selector::EnginePackageSelected;
use crate::engine::process_provider::{EngineProcess, EngineProcessProvider};
use crate::engine::resource_config::{EngineResourceConfig, EngineResourceConfigError};
use crate::engine::session::{EngineSession, EngineSessionError, EngineSessionStartup};
use crate::game::engine_provider::{
    GameEngineContext, GameEngineEnumEvent, GameEngineProvider, GameEngineQuiescenceReceipt,
    GameEngineRequest, GameEngineSearchIdentity, GameEngineStopReceipt,
};
use crate::game::enums::GameId;

#[derive(Debug, Error)]
pub enum DesktopNativeGameEngineAllocationError {
    #[error("The engine thread allocation {threads} does not match its CPU lease of {workers}")]
    LeaseMismatch { threads: u32, workers: usize },
    #[error(transparent)]
    Config(#[from] EngineResourceConfigError),
}

pub struct DesktopNativeGameEngineAllocation {
    config: EngineResourceConfig,
    lease: CapacityLease,
    activity: Option<DesktopNativeGameEngineAllocationActivity>,
}

pub(crate) struct DesktopNativeGameEngineAllocationActivity {
    active: Arc<AtomicBool>,
    changed: Arc<Notify>,
    assigned: Arc<AtomicUsize>,
}

impl DesktopNativeGameEngineAllocationActivity {
    pub(crate) fn new(
        active: Arc<AtomicBool>,
        changed: Arc<Notify>,
        assigned: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            active,
            changed,
            assigned,
        }
    }
}

impl Drop for DesktopNativeGameEngineAllocationActivity {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        self.assigned.store(0, Ordering::SeqCst);
        self.changed.notify_waiters();
    }
}

impl DesktopNativeGameEngineAllocation {
    pub fn new(
        config: EngineResourceConfig,
        lease: CapacityLease,
    ) -> Result<Self, DesktopNativeGameEngineAllocationError> {
        if usize::try_from(config.threads()).ok() != Some(lease.workers()) {
            return Err(DesktopNativeGameEngineAllocationError::LeaseMismatch {
                threads: config.threads(),
                workers: lease.workers(),
            });
        }
        Ok(Self {
            config,
            lease,
            activity: None,
        })
    }

    pub const fn config(&self) -> EngineResourceConfig {
        self.config
    }

    pub fn workers(&self) -> usize {
        self.lease.workers()
    }

    pub(crate) fn with_activity(
        mut self,
        activity: Option<DesktopNativeGameEngineAllocationActivity>,
    ) -> Self {
        self.activity = activity;
        self
    }

    fn with_move_overhead(
        mut self,
        overhead: ClockDuration,
    ) -> Result<Self, DesktopNativeGameEngineAllocationError> {
        self.config = EngineResourceConfig::new(self.workers(), self.config.hash_mib(), overhead)?;
        Ok(self)
    }
}

pub trait DesktopNativeGameEngineResources: Send + Sync + 'static {
    type Registration: Send + 'static;
    type Error: std::error::Error + Send + Sync + 'static;

    fn register(
        &self,
        context: GameEngineContext,
    ) -> impl Future<Output = Result<Self::Registration, Self::Error>> + Send;

    fn allocate(
        &self,
        context: GameEngineContext,
    ) -> impl Future<Output = Result<DesktopNativeGameEngineAllocation, Self::Error>> + Send;

    fn next_update(
        &self,
        context: GameEngineContext,
    ) -> impl Future<Output = Result<DesktopNativeGameEngineAllocation, Self::Error>> + Send;

    fn acknowledge(
        &self,
        context: GameEngineContext,
        config: EngineResourceConfig,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

#[derive(Debug, Error)]
pub enum DesktopNativeGameEngineError<E: std::error::Error + Send + Sync + 'static> {
    #[error("The native engine still owns an unfinished operation or unobserved resources")]
    OperationPending,
    #[error("The native engine operation was cancelled for retained cleanup")]
    Cancelled,
    #[error("The native engine resource ownership is incomplete")]
    MissingState,
    #[error("The engine belongs to game {expected:?}, received {received:?}")]
    WrongGame { expected: GameId, received: GameId },
    #[error("Cannot allocate native engine resources: {0}")]
    Resources(#[source] E),
    #[error(transparent)]
    Allocation(#[from] DesktopNativeGameEngineAllocationError),
    #[error(transparent)]
    Startup(#[from] EngineSessionError),
    #[error(transparent)]
    Engine(#[from] EngineLiveGameError),
    #[error("The retained native engine operation exceeded its deadline: {0}")]
    Deadline(#[source] tokio::time::error::Elapsed),
    #[error("Native engine factory cleanup previously failed: {0}")]
    ShutdownFailed(#[source] Arc<Self>),
    #[error("Native engine cleanup failed ({operation}); startup cleanup also failed ({startup})")]
    Cleanup {
        #[source]
        operation: Box<Self>,
        startup: Box<Self>,
    },
}

type DesktopNativeGameEngineFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

type DesktopNativeGameEngineInitialization<P> = DesktopNativeGameEngineFuture<(
    EngineSessionStartup<P>,
    Option<Result<EngineSession<P>, EngineSessionError>>,
)>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopNativeGameEngineEnumOperation {
    Start(GameEngineContext),
    Resources(GameEngineContext),
}

struct DesktopNativeGameEngineCompletion<P: EngineProcess, C: ClockTimeProvider, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    engine: EngineLiveGame<P, C>,
    allocation: DesktopNativeGameEngineAllocation,
    result: Result<Option<GameEngineSearchIdentity>, DesktopNativeGameEngineError<E>>,
}

struct DesktopNativeGameEngineState<P: EngineProcess, C: ClockTimeProvider, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    engine: Option<EngineLiveGame<P, C>>,
    allocation: Option<DesktopNativeGameEngineAllocation>,
    operation: Option<DesktopNativeGameEngineFuture<DesktopNativeGameEngineCompletion<P, C, E>>>,
    intent: Option<DesktopNativeGameEngineEnumOperation>,
    cancelled: Option<Arc<Notify>>,
}

impl<P, C, E> DesktopNativeGameEngineState<P, C, E>
where
    P: EngineProcess + 'static,
    C: ClockTimeProvider + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    const fn new() -> Self {
        Self {
            engine: None,
            allocation: None,
            operation: None,
            intent: None,
            cancelled: None,
        }
    }

    fn is_shutdown_complete(&self) -> bool {
        self.operation.is_none()
            && self
                .engine
                .as_ref()
                .is_none_or(EngineLiveGame::is_shutdown_complete)
    }

    fn release_observed_capacity(&mut self) {
        if self
            .engine
            .as_ref()
            .is_some_and(|engine| engine.is_idle() || engine.is_shutdown_complete())
        {
            self.allocation = None;
        }
    }

    fn configure<R: DesktopNativeGameEngineResources<Error = E>>(
        &mut self,
        allocation: DesktopNativeGameEngineAllocation,
        context: GameEngineContext,
        resources: Arc<R>,
        request: Option<GameEngineRequest>,
    ) -> Result<(), DesktopNativeGameEngineError<E>> {
        if self.operation.is_some() || self.allocation.is_some() {
            return Err(DesktopNativeGameEngineError::OperationPending);
        }
        let mut engine = self
            .engine
            .take()
            .ok_or(DesktopNativeGameEngineError::MissingState)?;
        self.intent = Some(match &request {
            Some(request) => DesktopNativeGameEngineEnumOperation::Start(request.context()),
            None => DesktopNativeGameEngineEnumOperation::Resources(context),
        });
        let cancelled = Arc::new(Notify::new());
        self.cancelled = Some(Arc::clone(&cancelled));
        self.operation = Some(Box::pin(async move {
            let config = allocation.config();
            let configuring = async {
                if engine.resources() != Some(config) {
                    engine.configure_resources(config).await?;
                }
                resources
                    .acknowledge(context, config)
                    .await
                    .map_err(DesktopNativeGameEngineError::Resources)?;
                match request {
                    Some(request) => engine
                        .start(request)
                        .await
                        .map(Some)
                        .map_err(DesktopNativeGameEngineError::Engine),
                    None => Ok(None),
                }
            };
            let result = tokio::select! {
                biased;
                () = cancelled.notified() => Err(DesktopNativeGameEngineError::Cancelled),
                result = configuring => result,
            };
            DesktopNativeGameEngineCompletion {
                engine,
                allocation,
                result,
            }
        }));
        Ok(())
    }

    async fn complete_operation(
        &mut self,
    ) -> Result<Option<GameEngineSearchIdentity>, DesktopNativeGameEngineError<E>> {
        let completion = self
            .operation
            .as_mut()
            .ok_or(DesktopNativeGameEngineError::MissingState)?
            .as_mut()
            .await;
        self.operation = None;
        self.intent = None;
        self.cancelled = None;
        self.engine = Some(completion.engine);
        self.allocation = Some(completion.allocation);
        self.release_observed_capacity();
        completion.result
    }

    async fn settle(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<(), DesktopNativeGameEngineError<E>> {
        if self.operation.is_some() {
            if let Some(cancelled) = &self.cancelled {
                cancelled.notify_one();
            }
            match tokio::time::timeout_at(deadline, self.complete_operation())
                .await
                .map_err(DesktopNativeGameEngineError::Deadline)?
            {
                Ok(_) | Err(DesktopNativeGameEngineError::Cancelled) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

pub struct DesktopNativeGameEngineFactoryOptions<P, C, R> {
    pub processes: P,
    pub package: EnginePackageSelected,
    pub clock: Arc<C>,
    pub resources: Arc<R>,
}

pub struct DesktopNativeGameEngineFactory<
    P: EngineProcessProvider,
    C: ClockTimeProvider,
    R: DesktopNativeGameEngineResources,
> {
    processes: P,
    package: EnginePackageSelected,
    clock: Arc<C>,
    resources: Arc<R>,
    context: Option<GameEngineContext>,
    registration: Option<R::Registration>,
    registration_wait: Option<DesktopNativeGameEngineFuture<Result<R::Registration, R::Error>>>,
    allocation: Option<DesktopNativeGameEngineAllocation>,
    allocation_wait:
        Option<DesktopNativeGameEngineFuture<Result<DesktopNativeGameEngineAllocation, R::Error>>>,
    startup: Option<EngineSessionStartup<P::Process>>,
    initializing: Option<DesktopNativeGameEngineInitialization<P::Process>>,
    initialization_cancelled: Option<Arc<Notify>>,
    pending: DesktopNativeGameEngineState<P::Process, C, R::Error>,
    shutdown_failure: Option<Arc<DesktopNativeGameEngineError<R::Error>>>,
}

impl<P: EngineProcessProvider, C: ClockTimeProvider + 'static, R: DesktopNativeGameEngineResources>
    DesktopNativeGameEngineFactory<P, C, R>
where
    P::Process: 'static,
{
    pub fn new(options: DesktopNativeGameEngineFactoryOptions<P, C, R>) -> Self {
        Self {
            processes: options.processes,
            package: options.package,
            clock: options.clock,
            resources: options.resources,
            context: None,
            registration: None,
            registration_wait: None,
            allocation: None,
            allocation_wait: None,
            startup: None,
            initializing: None,
            initialization_cancelled: None,
            pending: DesktopNativeGameEngineState::new(),
            shutdown_failure: None,
        }
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.initializing.is_none()
            && self
                .startup
                .as_ref()
                .is_none_or(EngineSessionStartup::is_shutdown_complete)
            && self.pending.is_shutdown_complete()
    }

    async fn finish_initialization(
        &mut self,
    ) -> Result<(), DesktopNativeGameEngineError<R::Error>> {
        let (startup, result) = self
            .initializing
            .as_mut()
            .ok_or(DesktopNativeGameEngineError::MissingState)?
            .as_mut()
            .await;
        self.initializing = None;
        self.initialization_cancelled = None;
        self.startup = Some(startup);
        let session = result.ok_or(DesktopNativeGameEngineError::Cancelled)??;
        self.pending.engine = Some(EngineLiveGame::new(session, Arc::clone(&self.clock)));
        Ok(())
    }

    fn retained_shutdown_result(&self) -> Result<(), DesktopNativeGameEngineError<R::Error>> {
        match &self.shutdown_failure {
            Some(error) => Err(DesktopNativeGameEngineError::ShutdownFailed(Arc::clone(
                error,
            ))),
            None => Ok(()),
        }
    }
}

impl<P, C, R> DesktopGameSupervisorEngineFactory for DesktopNativeGameEngineFactory<P, C, R>
where
    P: EngineProcessProvider + Send,
    C: ClockTimeProvider + 'static,
    R: DesktopNativeGameEngineResources,
    P::Process: 'static,
{
    type Engine = DesktopNativeGameEngine<P::Process, C, R>;
    type Error = DesktopNativeGameEngineError<R::Error>;

    async fn create(&mut self, context: GameEngineContext) -> Result<Self::Engine, Self::Error> {
        if self.context.is_some_and(|existing| existing != context) {
            return Err(DesktopNativeGameEngineError::OperationPending);
        }
        self.context = Some(context);
        self.shutdown_failure = None;
        if self.registration.is_none() {
            if self.registration_wait.is_none() {
                let resources = Arc::clone(&self.resources);
                self.registration_wait =
                    Some(Box::pin(async move { resources.register(context).await }));
            }
            let result = self
                .registration_wait
                .as_mut()
                .ok_or(Self::Error::MissingState)?
                .as_mut()
                .await;
            self.registration_wait = None;
            self.registration = Some(result.map_err(Self::Error::Resources)?);
        }
        if self.allocation.is_none() && self.pending.operation.is_none() {
            if self.allocation_wait.is_none() {
                let resources = Arc::clone(&self.resources);
                self.allocation_wait =
                    Some(Box::pin(async move { resources.allocate(context).await }));
            }
            let result = self
                .allocation_wait
                .as_mut()
                .ok_or(Self::Error::MissingState)?
                .as_mut()
                .await;
            self.allocation_wait = None;
            self.allocation = Some(result.map_err(Self::Error::Resources)?);
        }
        if self.pending.engine.is_none() && self.pending.operation.is_none() {
            if self.initializing.is_none() {
                if self.startup.is_some() {
                    return Err(Self::Error::OperationPending);
                }
                let mut startup = EngineSessionStartup::new(&self.processes, self.package.clone())?;
                let cancelled = Arc::new(Notify::new());
                self.initialization_cancelled = Some(Arc::clone(&cancelled));
                self.initializing = Some(Box::pin(async move {
                    let result = tokio::select! {
                        biased;
                        () = cancelled.notified() => None,
                        result = startup.initialize() => Some(result),
                    };
                    (startup, result)
                }));
            }
            self.finish_initialization().await?;
        }
        if self.pending.operation.is_none() {
            let allocation = self.allocation.take().ok_or(Self::Error::MissingState)?;
            self.pending
                .configure(allocation, context, Arc::clone(&self.resources), None)?;
        }
        self.pending.complete_operation().await?;
        let Some(registration) = self.registration.take() else {
            return Err(DesktopNativeGameEngineError::MissingState);
        };
        self.startup = None;
        self.allocation = None;
        self.context = None;
        let state = std::mem::replace(&mut self.pending, DesktopNativeGameEngineState::new());
        Ok(DesktopNativeGameEngine {
            state,
            game: context.game(),
            context,
            resources: Arc::clone(&self.resources),
            registration: Some(registration),
            allocation_wait: None,
            waiting: None,
        })
    }

    async fn shutdown_retained(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<(), Self::Error> {
        self.registration_wait = None;
        self.allocation_wait = None;
        if self.initializing.is_some() {
            if let Some(cancelled) = &self.initialization_cancelled {
                cancelled.notify_one();
            }
            match tokio::time::timeout_at(deadline, self.finish_initialization())
                .await
                .map_err(Self::Error::Deadline)?
            {
                Ok(()) | Err(Self::Error::Cancelled) => {}
                Err(error) => return Err(error),
            }
        }
        self.pending.settle(deadline).await?;
        let engine_result = match &mut self.pending.engine {
            Some(engine) => engine
                .shutdown_retained(deadline.into_std())
                .await
                .map_err(Self::Error::Engine),
            None => Ok(()),
        };
        let startup_result = match &mut self.startup {
            Some(startup) => startup
                .shutdown_retained(deadline)
                .await
                .map_err(Self::Error::Startup),
            None => Ok(()),
        };
        let result = match (engine_result, startup_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(operation), Err(startup)) => Err(Self::Error::Cleanup {
                operation: Box::new(operation),
                startup: Box::new(startup),
            }),
        };
        if self.is_shutdown_complete() {
            self.pending = DesktopNativeGameEngineState::new();
            self.startup = None;
            self.allocation = None;
            self.registration = None;
            self.context = None;
            if let Err(error) = result {
                self.shutdown_failure = Some(Arc::new(error));
            }
            self.retained_shutdown_result()
        } else {
            result
        }
    }
}

pub struct DesktopNativeGameEngine<
    P: EngineProcess,
    C: ClockTimeProvider,
    R: DesktopNativeGameEngineResources,
> {
    state: DesktopNativeGameEngineState<P, C, R::Error>,
    game: GameId,
    context: GameEngineContext,
    resources: Arc<R>,
    registration: Option<R::Registration>,
    allocation_wait:
        Option<DesktopNativeGameEngineFuture<Result<DesktopNativeGameEngineAllocation, R::Error>>>,
    waiting: Option<DesktopNativeGameEngineEnumOperation>,
}

impl<
    P: EngineProcess + 'static,
    C: ClockTimeProvider + 'static,
    R: DesktopNativeGameEngineResources,
> DesktopNativeGameEngine<P, C, R>
{
    pub fn resources(&self) -> Option<EngineResourceConfig> {
        self.state
            .engine
            .as_ref()
            .and_then(EngineLiveGame::resources)
    }

    fn validate_game(
        &self,
        context: GameEngineContext,
    ) -> Result<(), DesktopNativeGameEngineError<R::Error>> {
        if context.game() != self.game {
            return Err(DesktopNativeGameEngineError::WrongGame {
                expected: self.game,
                received: context.game(),
            });
        }
        Ok(())
    }

    fn release_observed_capacity(&mut self) {
        self.state.release_observed_capacity();
        if self.state.is_shutdown_complete() {
            self.registration = None;
        }
    }

    fn cancel_allocation(&mut self) {
        self.allocation_wait = None;
        self.waiting = None;
    }

    fn begin_allocation(
        &mut self,
        intent: DesktopNativeGameEngineEnumOperation,
    ) -> Result<(), DesktopNativeGameEngineError<R::Error>> {
        if let Some(waiting) = self.waiting {
            if waiting == intent {
                return Ok(());
            }
            return Err(DesktopNativeGameEngineError::OperationPending);
        }
        let resources = Arc::clone(&self.resources);
        self.allocation_wait = Some(Box::pin(async move {
            match intent {
                DesktopNativeGameEngineEnumOperation::Start(context) => {
                    resources.allocate(context).await
                }
                DesktopNativeGameEngineEnumOperation::Resources(context) => {
                    resources.next_update(context).await
                }
            }
        }));
        self.waiting = Some(intent);
        Ok(())
    }

    async fn receive_allocation(
        &mut self,
    ) -> Result<DesktopNativeGameEngineAllocation, DesktopNativeGameEngineError<R::Error>> {
        let result = self
            .allocation_wait
            .as_mut()
            .ok_or(DesktopNativeGameEngineError::MissingState)?
            .as_mut()
            .await;
        self.cancel_allocation();
        result.map_err(DesktopNativeGameEngineError::Resources)
    }

    async fn settle(&mut self) -> Result<(), DesktopNativeGameEngineError<R::Error>> {
        self.cancel_allocation();
        if self.state.operation.is_some() {
            self.state.complete_operation().await?;
        }
        self.release_observed_capacity();
        Ok(())
    }
}

impl<
    P: EngineProcess + 'static,
    C: ClockTimeProvider + 'static,
    R: DesktopNativeGameEngineResources,
> GameEngineProvider for DesktopNativeGameEngine<P, C, R>
{
    type Error = DesktopNativeGameEngineError<R::Error>;

    fn is_idle(&self) -> bool {
        self.registration.is_some()
            && self.allocation_wait.is_none()
            && self.state.operation.is_none()
            && self.state.allocation.is_none()
            && self
                .state
                .engine
                .as_ref()
                .is_some_and(GameEngineProvider::is_idle)
    }

    async fn start(
        &mut self,
        request: GameEngineRequest,
    ) -> Result<GameEngineSearchIdentity, Self::Error> {
        self.validate_game(request.context())?;
        let intent = DesktopNativeGameEngineEnumOperation::Start(request.context());
        if matches!(
            self.waiting,
            Some(DesktopNativeGameEngineEnumOperation::Resources(_))
        ) {
            self.cancel_allocation();
        }
        if matches!(
            self.state.intent,
            Some(DesktopNativeGameEngineEnumOperation::Resources(_))
        ) {
            self.state.complete_operation().await?;
        }
        if self.state.operation.is_some() {
            if self.state.intent != Some(intent) {
                return Err(Self::Error::OperationPending);
            }
            return self
                .state
                .complete_operation()
                .await?
                .ok_or(Self::Error::MissingState);
        }
        if self.waiting.is_none() && !self.is_idle() {
            return Err(Self::Error::OperationPending);
        }
        self.context = request.context();
        self.begin_allocation(intent)?;
        let allocation = self
            .receive_allocation()
            .await?
            .with_move_overhead(request.config().move_overhead())?;
        self.state.configure(
            allocation,
            self.context,
            Arc::clone(&self.resources),
            Some(request),
        )?;
        let result = self
            .state
            .complete_operation()
            .await?
            .ok_or(Self::Error::MissingState);
        self.release_observed_capacity();
        result
    }

    async fn next_event(
        &mut self,
        expected: GameEngineContext,
    ) -> Result<GameEngineEnumEvent, Self::Error> {
        self.validate_game(expected)?;
        let result = self
            .state
            .engine
            .as_mut()
            .ok_or(Self::Error::MissingState)?
            .next_event(expected)
            .await
            .map_err(Self::Error::Engine);
        self.release_observed_capacity();
        result
    }

    async fn quiesce(
        &mut self,
        expected: GameEngineContext,
        received_at: Instant,
    ) -> Result<GameEngineQuiescenceReceipt, Self::Error> {
        self.validate_game(expected)?;
        self.settle().await?;
        let result = self
            .state
            .engine
            .as_mut()
            .ok_or(Self::Error::MissingState)?
            .quiesce(expected, received_at)
            .await
            .map_err(Self::Error::Engine);
        self.release_observed_capacity();
        result
    }

    async fn stop(
        &mut self,
        identity: GameEngineSearchIdentity,
        received_at: Instant,
    ) -> Result<GameEngineStopReceipt, Self::Error> {
        self.validate_game(identity.context())?;
        self.settle().await?;
        let result = self
            .state
            .engine
            .as_mut()
            .ok_or(Self::Error::MissingState)?
            .stop(identity, received_at)
            .await
            .map_err(Self::Error::Engine);
        self.release_observed_capacity();
        result
    }
}

impl<
    P: EngineProcess + 'static,
    C: ClockTimeProvider + 'static,
    R: DesktopNativeGameEngineResources,
> DesktopGameSupervisorEngineProvider for DesktopNativeGameEngine<P, C, R>
{
    fn is_shutdown_complete(&self) -> bool {
        self.state.is_shutdown_complete() && self.registration.is_none()
    }

    async fn refresh_resources(&mut self) -> Result<(), Self::Error> {
        let intent = DesktopNativeGameEngineEnumOperation::Resources(self.context);
        if let Some(existing) = self.state.intent {
            if existing != intent {
                return Err(Self::Error::OperationPending);
            }
        } else {
            if self.waiting.is_none() && !self.is_idle() {
                return Err(Self::Error::OperationPending);
            }
            self.begin_allocation(intent)?;
            let overhead = self
                .resources()
                .ok_or(Self::Error::MissingState)?
                .move_overhead();
            let allocation = self
                .receive_allocation()
                .await?
                .with_move_overhead(overhead)?;
            self.state
                .configure(allocation, self.context, Arc::clone(&self.resources), None)?;
        }
        let result = self.state.complete_operation().await.map(|_identity| ());
        self.release_observed_capacity();
        result
    }

    async fn shutdown_retained(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<(), Self::Error> {
        self.cancel_allocation();
        self.state.settle(deadline).await?;
        let result = self
            .state
            .engine
            .as_mut()
            .ok_or(Self::Error::MissingState)?
            .shutdown_retained(deadline.into_std())
            .await
            .map_err(Self::Error::Engine);
        self.release_observed_capacity();
        result
    }
}
