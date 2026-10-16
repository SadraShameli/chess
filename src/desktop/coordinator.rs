use std::collections::HashSet;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::ClockDurationError;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationError, DesktopApplicationWakeProvider,
    DesktopApplicationWork,
};
use crate::desktop::dto::{DesktopBootstrapSnapshot, DesktopEnumEvent, DesktopEnumValue};
use crate::desktop::engine_bridge::{
    DesktopEngineBridgeEnumResult, DesktopEngineBridgeFuture, DesktopEngineBridgeProvider,
    DesktopEngineBridgeRequest,
};
use crate::desktop::engine_commands::{
    DesktopEngineCommandsEnumOperation, DesktopEngineCommandsOperation,
};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopError, DesktopFailure};
use crate::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileHandleError, DesktopFileHandleId,
    DesktopFileHandleRegistry, DesktopFileHandleRequest,
};
use crate::desktop::game_bridge::{
    DesktopGameBridgeFuture, DesktopGameBridgeProvider, DesktopGameBridgeRequest,
};
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};
use crate::desktop::messages::DesktopEnumCommand;
use crate::desktop::stream::{DesktopEnumStreamClass, DesktopStream};
use crate::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation;

#[cfg(test)]
#[path = "../../tests/desktop/coordinator_engine_quit.rs"]
mod engine_quit_tests;

#[derive(Debug, Error)]
pub enum DesktopCoordinatorError {
    #[error("The live-game snapshot service closed before application shutdown")]
    GamesClosed,
    #[error("The Quit deadline exceeds the native runtime range")]
    QuitDeadlineRange,
    #[error("The Quit cleanup window expired; unfinished operations remain owned until completion")]
    QuitDeadline,
    #[error(
        "Previous engine quiescence responses remain unresolved; wait for observation before another Retry quit"
    )]
    EngineQuiescenceCapacity,
    #[error("Engine quiescence failed: {0}")]
    EngineQuiescence(#[source] DesktopFailure),
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
    #[error(transparent)]
    Time(#[from] ClockDurationError),
    #[error(transparent)]
    Stream(#[from] DesktopError),
    #[error(transparent)]
    Dialog(#[from] DesktopFileHandleError),
    #[error("Application shutdown is incomplete: {source}")]
    Shutdown {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

pub trait DesktopCoordinatorDialogProvider: DesktopFileDialogProvider {
    fn request_cancel(&self) -> Result<(), DesktopFileHandleError>;
}

pub trait DesktopCoordinatorLifecycleProvider: Send {
    fn shutdown(
        &mut self,
        deadline: Instant,
    ) -> impl Future<Output = Result<(), DesktopCoordinatorError>> + Send;
}

pub struct DesktopCoordinatorServices<P, L> {
    provider: Arc<P>,
    executor: Arc<CapacityExecutor>,
    lifecycle: L,
}

impl<P, L> DesktopCoordinatorServices<P, L> {
    pub fn new(provider: Arc<P>, executor: Arc<CapacityExecutor>, lifecycle: L) -> Self {
        Self {
            provider,
            executor,
            lifecycle,
        }
    }
}

struct DesktopCoordinatorFiles<P> {
    provider: Arc<P>,
    registry: DesktopFileHandleRegistry,
    executor: Arc<CapacityExecutor>,
}

type DesktopCoordinatorSelectionFuture<P> = Pin<
    Box<
        dyn Future<
                Output = (
                    DesktopCoordinatorFiles<P>,
                    Result<Option<DesktopFileHandleId>, DesktopFileHandleError>,
                ),
            > + Send,
    >,
>;

struct DesktopCoordinatorSelection<P> {
    work: DesktopApplicationWork,
    selection: DesktopCoordinatorSelectionFuture<P>,
}

struct DesktopCoordinatorGamePending {
    work: DesktopApplicationWork,
    future: Option<DesktopGameBridgeFuture>,
    response: Option<Result<DesktopGameSnapshot, DesktopFailure>>,
}

struct DesktopCoordinatorEnginePending {
    work: DesktopApplicationWork,
    future: Option<DesktopEngineBridgeFuture>,
    response: Option<Result<DesktopEngineBridgeEnumResult, DesktopFailure>>,
}

type DesktopCoordinatorEngineQuiescenceFuture =
    Pin<Box<dyn Future<Output = Result<(), DesktopFailure>> + Send>>;

struct DesktopCoordinatorEngineQuiescence {
    future: DesktopCoordinatorEngineQuiescenceFuture,
    outcome: Option<Result<(), DesktopFailure>>,
}

impl DesktopCoordinatorEngineQuiescence {
    fn new(engine: &dyn DesktopEngineBridgeProvider, deadline: Instant) -> Self {
        Self {
            future: engine.quiesce(deadline),
            outcome: None,
        }
    }
}

enum DesktopCoordinatorEngineQuitEnumPhase {
    Quiescing(Vec<DesktopCoordinatorEngineQuiescence>),
    Cleanup,
}

impl DesktopCoordinatorEngineQuitEnumPhase {
    const MAX_RETAINED: usize = 2;

    fn new(engine: &dyn DesktopEngineBridgeProvider, deadline: Instant) -> Self {
        Self::Quiescing(vec![DesktopCoordinatorEngineQuiescence::new(
            engine, deadline,
        )])
    }

    fn retry(
        &mut self,
        engine: &dyn DesktopEngineBridgeProvider,
        deadline: Instant,
    ) -> Result<(), DesktopCoordinatorError> {
        match self {
            Self::Quiescing(attempts) => {
                attempts.retain(|attempt| attempt.outcome.is_none());
                if attempts.len() == Self::MAX_RETAINED {
                    return Err(DesktopCoordinatorError::EngineQuiescenceCapacity);
                }
                attempts.push(DesktopCoordinatorEngineQuiescence::new(engine, deadline));
            }
            Self::Cleanup => {}
        }
        Ok(())
    }

    fn failure(&self) -> Option<DesktopCoordinatorError> {
        match self {
            Self::Quiescing(attempts) => {
                attempts.last().and_then(|attempt| match &attempt.outcome {
                    Some(Err(failure)) => {
                        Some(DesktopCoordinatorError::EngineQuiescence(failure.clone()))
                    }
                    Some(Ok(())) | None => None,
                })
            }
            Self::Cleanup => None,
        }
    }

    fn is_ready(&self) -> bool {
        match self {
            Self::Quiescing(attempts) => {
                !attempts.is_empty()
                    && attempts.iter().all(|attempt| attempt.outcome.is_some())
                    && attempts
                        .last()
                        .is_some_and(|attempt| matches!(attempt.outcome.as_ref(), Some(Ok(()))))
            }
            Self::Cleanup => true,
        }
    }

    fn begin_cleanup(&mut self) {
        *self = Self::Cleanup;
    }
}

pub struct DesktopCoordinator<C, W, P, L>
where
    C: ClockTimeProvider,
    W: DesktopApplicationWakeProvider,
{
    application: DesktopApplication<C, W>,
    clock: Arc<C>,
    snapshot: DesktopBootstrapSnapshot,
    files: Option<DesktopCoordinatorFiles<P>>,
    pending_selection: Option<DesktopCoordinatorSelection<P>>,
    provider: Arc<P>,
    controls: Vec<DesktopApplicationWork>,
    stream: Option<DesktopStream<DesktopEnumEvent>>,
    subscription: Option<DesktopRequestId>,
    lifecycle: L,
    cancellation_failure: Option<DesktopFileHandleError>,
    shutdown_failure: Option<DesktopCoordinatorError>,
    reported_deadlines: HashSet<DesktopRequestId>,
    games: Option<Arc<dyn DesktopGameBridgeProvider>>,
    game_snapshots: Option<watch::Receiver<Vec<DesktopGameSnapshot>>>,
    latest_games: Vec<DesktopGameSnapshot>,
    pending_games: Vec<DesktopCoordinatorGamePending>,
    engine: Option<Arc<dyn DesktopEngineBridgeProvider>>,
    pending_engine: Vec<DesktopCoordinatorEnginePending>,
    engine_quit: Option<DesktopCoordinatorEngineQuitEnumPhase>,
    shutdown_notice: Option<(DesktopRequestId, DesktopFailure)>,
    quit_retry_deadline: Option<Instant>,
    quit_deadline: Option<Instant>,
}

impl<C, W, P, L> DesktopCoordinator<C, W, P, L>
where
    C: ClockTimeProvider + 'static,
    W: DesktopApplicationWakeProvider + 'static,
    P: DesktopCoordinatorDialogProvider + 'static,
    L: DesktopCoordinatorLifecycleProvider,
{
    pub const SHUTDOWN: Duration = Duration::from_secs(3);

    pub fn new(
        application: DesktopApplication<C, W>,
        clock: Arc<C>,
        snapshot: DesktopBootstrapSnapshot,
        services: DesktopCoordinatorServices<P, L>,
    ) -> Self {
        let DesktopCoordinatorServices {
            provider,
            executor,
            lifecycle,
        } = services;
        Self {
            application,
            clock,
            snapshot,
            files: Some(DesktopCoordinatorFiles {
                provider: Arc::clone(&provider),
                registry: DesktopFileHandleRegistry::new(),
                executor,
            }),
            pending_selection: None,
            provider,
            controls: Vec::new(),
            stream: None,
            subscription: None,
            lifecycle,
            cancellation_failure: None,
            shutdown_failure: None,
            reported_deadlines: HashSet::new(),
            games: None,
            game_snapshots: None,
            latest_games: Vec::new(),
            pending_games: Vec::new(),
            engine: None,
            pending_engine: Vec::new(),
            engine_quit: None,
            shutdown_notice: None,
            quit_retry_deadline: None,
            quit_deadline: None,
        }
    }

    pub fn snapshot(&self) -> &DesktopBootstrapSnapshot {
        &self.snapshot
    }

    pub fn with_games(mut self, games: Arc<dyn DesktopGameBridgeProvider>) -> Self {
        let receiver = games.snapshots();
        self.latest_games = receiver.borrow().clone();
        self.game_snapshots = Some(receiver);
        self.games = Some(games);
        self
    }

    pub fn with_engine(mut self, engine: Arc<dyn DesktopEngineBridgeProvider>) -> Self {
        self.engine = Some(engine);
        self
    }

    pub async fn run(&mut self) -> Result<(), DesktopCoordinatorError> {
        loop {
            self.complete_games().await?;
            self.complete_engine().await?;
            if self.complete_controls().await? {
                return Ok(());
            }
            let mut processed = 0;
            while processed < Self::COMMAND_BATCH {
                let Some(work) = Self::retry(|| self.application.try_next()).await? else {
                    break;
                };
                self.dispatch(work).await?;
                processed += 1;
            }
            if self.complete_controls().await? {
                return Ok(());
            }
            self.report_pending_controls().await?;
            let deadline = self.next_deadline()?;
            tokio::select! {
                biased;
                (index, outcome) = Self::engine_quiescence_ready(&mut self.engine_quit) => {
                    if let Some(DesktopCoordinatorEngineQuitEnumPhase::Quiescing(attempts)) = &mut self.engine_quit {
                        attempts[index].outcome = Some(outcome);
                    }
                }
                (index, response) = Self::engine_ready(&mut self.pending_engine) => {
                    self.pending_engine[index].response = Some(response);
                }
                (index, response) = Self::game_ready(&mut self.pending_games) => {
                    self.pending_games[index].response = Some(response);
                }
                snapshots = Self::game_snapshots_ready(&mut self.game_snapshots) => {
                    self.publish_games(snapshots?).await?;
                }
                selected = Self::selection_ready(&mut self.pending_selection), if self.pending_selection.is_some() => {
                    self.finish_selection(selected).await?;
                }
                () = Self::wait_deadline(deadline) => self.check_stream()?,
                () = self.application.ready() => {}
                () = tokio::task::yield_now(), if processed == Self::COMMAND_BATCH => {}
            }
        }
    }

    const COMMAND_BATCH: usize = 32;

    pub fn cancellation_failure(&self) -> Option<&DesktopFileHandleError> {
        self.cancellation_failure.as_ref()
    }

    pub fn shutdown_failure(&self) -> Option<&DesktopCoordinatorError> {
        self.shutdown_failure.as_ref()
    }

    fn uses_quit_deadline(&self, control: &DesktopApplicationWork) -> bool {
        self.quit_deadline.is_some()
            && self.pending_selection.is_none()
            && matches!(
                control.request().command().operation(),
                DesktopEnumCommand::Quit
            )
    }

    async fn report_pending_controls(&mut self) -> Result<(), DesktopCoordinatorError> {
        let now = self.clock.now()?;
        for control in &self.controls {
            if self.uses_quit_deadline(control) {
                continue;
            }
            if !self
                .reported_deadlines
                .contains(&control.receipt().request_id())
                && control
                    .receipt()
                    .deadline()
                    .is_some_and(|deadline| now >= deadline)
            {
                Self::retry(|| self.application.report_pending_control(control, DesktopFailure::new(
                    DesktopEnumFailureCode::ResourceRefusal,
                    "Close the native file dialog to finish this action. Its operation remains owned until the dialog closes.".to_owned(),
                    true,
                ))).await?;
                self.reported_deadlines
                    .insert(control.receipt().request_id());
            }
        }
        Ok(())
    }

    async fn retry<T>(
        mut operation: impl FnMut() -> Result<T, DesktopApplicationError>,
    ) -> Result<T, DesktopApplicationError> {
        loop {
            match operation() {
                Err(DesktopApplicationError::Busy) => tokio::task::yield_now().await,
                result => return result,
            }
        }
    }

    async fn dispatch(
        &mut self,
        work: DesktopApplicationWork,
    ) -> Result<(), DesktopCoordinatorError> {
        match work.request().command().operation().clone() {
            DesktopEnumCommand::Engine(operation) => self.dispatch_engine(work, operation).await?,
            DesktopEnumCommand::Game(operation) => {
                let response =
                    if work.request().command().expected_revision() != Some(operation.revision()) {
                        Some(Err(DesktopFailure::new(
                            DesktopEnumFailureCode::StaleRevision,
                            "The game command has a mismatched native revision".to_owned(),
                            true,
                        )))
                    } else if self.games.is_none() {
                        Some(Err(DesktopFailure::new(
                            DesktopEnumFailureCode::ResourceRefusal,
                            "The live-game service is unavailable".to_owned(),
                            true,
                        )))
                    } else {
                        None
                    };
                let is_control = work.request().command().operation().is_control();
                if !is_control {
                    match Self::retry(|| self.application.begin_commit(&work)).await {
                        Ok(()) => {}
                        Err(DesktopApplicationError::CancellationRequested) => {
                            Self::retry(|| self.application.acknowledge_stopped(&work)).await?;
                            return Ok(());
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                let future = match (&self.games, &response) {
                    (Some(games), None) => Some(games.execute(DesktopGameBridgeRequest {
                        request_id: work.receipt().request_id(),
                        received_at: work.receipt().received_at(),
                        operation,
                    })),
                    (Some(_), Some(_)) | (None, _) => None,
                };
                self.pending_games.push(DesktopCoordinatorGamePending {
                    work,
                    future,
                    response,
                });
            }
            DesktopEnumCommand::Bootstrap => {
                self.complete(&work, DesktopEnumValue::Bootstrap(self.snapshot.clone()))
                    .await?;
            }
            DesktopEnumCommand::FileSelect { owner, purpose } => {
                if work.request().command().expected_revision() != Some(DesktopRevision::ZERO) {
                    self.fail(
                        &work,
                        DesktopFailure::new(
                            DesktopEnumFailureCode::StaleRevision,
                            "The file-selection revision is stale".to_owned(),
                            true,
                        ),
                    )
                    .await?;
                } else if let Some(mut files) = self.files.take() {
                    let receipt = work.receipt().clone();
                    let clock = Arc::clone(&self.clock);
                    let selection = Box::pin(async move {
                        let outcome = async {
                            if receipt.is_cancellation_requested() {
                                return Ok(None);
                            }
                            let Some(path) = files.provider.select(purpose).await? else {
                                return Ok(None);
                            };
                            if receipt.is_cancellation_requested() {
                                return Ok(None);
                            }
                            let request = DesktopFileHandleRequest::new(
                                owner.file_handle_owner(),
                                purpose,
                                clock.now()?,
                            );
                            files
                                .registry
                                .register_selection(path, request, &files.executor)
                                .await
                                .map(Some)
                        }
                        .await;
                        (files, outcome)
                    });
                    self.pending_selection = Some(DesktopCoordinatorSelection { work, selection });
                } else {
                    self.fail(
                        &work,
                        DesktopFailure::new(
                            DesktopEnumFailureCode::ResourceRefusal,
                            "Close the existing native file dialog before opening another"
                                .to_owned(),
                            true,
                        ),
                    )
                    .await?;
                }
            }
            DesktopEnumCommand::StreamSubscribe => {
                Self::retry(|| self.application.clear_events()).await?;
                let subscription = DesktopRequestId::new();
                self.stream = Some(DesktopStream::new(subscription, self.clock.now()?));
                self.subscription = Some(subscription);
                self.complete(
                    &work,
                    DesktopEnumValue::StreamSubscribe {
                        subscription_id: subscription,
                        sequence: DesktopRevision::ZERO,
                        snapshot: self.snapshot.clone(),
                    },
                )
                .await?;
                if self.games.is_some() {
                    self.publish_games(self.latest_games.clone()).await?;
                }
            }
            DesktopEnumCommand::StreamAcknowledge {
                subscription_id,
                sequence,
            } => {
                let result = self.acknowledge(subscription_id, sequence);
                self.complete_stream(&work, result).await?;
            }
            DesktopEnumCommand::StreamResync { subscription_id } => {
                let result = self.resynchronize(subscription_id).await;
                self.complete_stream(&work, result).await?;
            }
            DesktopEnumCommand::Stop { .. }
            | DesktopEnumCommand::Pause { .. }
            | DesktopEnumCommand::Quit => {
                if self
                    .pending_selection
                    .as_ref()
                    .is_some_and(|selection| selection.work.is_cancellation_requested())
                    && let Err(error) = self.provider.request_cancel()
                {
                    self.cancellation_failure = Some(error);
                }
                self.controls.push(work);
            }
        }
        Ok(())
    }

    fn record_shutdown_failure(&mut self, control: usize, error: DesktopCoordinatorError) {
        let notice = DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            format!("{error}. Services remain owned. Choose Retry quit to try cleanup again."),
            true,
        );
        self.shutdown_failure = Some(error);
        self.shutdown_notice = Some((self.controls[control].receipt().request_id(), notice));
    }

    async fn publish_shutdown_notice(&mut self) -> Result<(), DesktopCoordinatorError> {
        if let Some((request, notice)) = &self.shutdown_notice
            && let Some(control) = self
                .controls
                .iter()
                .find(|control| control.receipt().request_id() == *request)
        {
            Self::retry(|| {
                self.application
                    .report_pending_control(control, notice.clone())
            })
            .await?;
            self.reported_deadlines.insert(*request);
            self.shutdown_notice = None;
        }
        Ok(())
    }

    async fn complete_controls(&mut self) -> Result<bool, DesktopCoordinatorError> {
        if self.application.take_quit_retry() {
            self.shutdown_failure = None;
            self.shutdown_notice = None;
            self.quit_retry_deadline = Some(
                Instant::now()
                    .checked_add(Self::SHUTDOWN)
                    .ok_or(DesktopCoordinatorError::QuitDeadlineRange)?,
            );
        }
        self.controls.sort_by_key(|work| {
            matches!(
                work.request().command().operation(),
                DesktopEnumCommand::Quit
            )
        });
        let mut index = 0;
        while index < self.controls.len() {
            let is_quit = matches!(
                self.controls[index].request().command().operation(),
                DesktopEnumCommand::Quit
            );
            if is_quit {
                let retry = self.quit_retry_deadline.take();
                let deadline = match retry.or(self.quit_deadline) {
                    Some(deadline) => deadline,
                    None => self.controls[index]
                        .receipt()
                        .runtime_received_at()
                        .checked_add(Self::SHUTDOWN)
                        .ok_or(DesktopCoordinatorError::QuitDeadlineRange)?,
                };
                self.quit_deadline = Some(deadline);
                if let Some(deadline) = retry
                    && let (Some(quit), Some(engine)) = (&mut self.engine_quit, &self.engine)
                    && let Err(error) = quit.retry(engine.as_ref(), deadline)
                {
                    self.record_shutdown_failure(index, error);
                }
                if self.engine_quit.is_none()
                    && !self.pending_engine.is_empty()
                    && let Some(engine) = &self.engine
                {
                    self.engine_quit = Some(DesktopCoordinatorEngineQuitEnumPhase::new(
                        engine.as_ref(),
                        deadline,
                    ));
                }
                if self.shutdown_failure.is_none() {
                    let failure = if Instant::now() >= deadline {
                        Some(DesktopCoordinatorError::QuitDeadline)
                    } else {
                        self.engine_quit
                            .as_ref()
                            .and_then(DesktopCoordinatorEngineQuitEnumPhase::failure)
                    };
                    if let Some(error) = failure {
                        self.record_shutdown_failure(index, error);
                    }
                }
                self.publish_shutdown_notice().await?;
                if self.pending_selection.is_some()
                    || !self.pending_games.is_empty()
                    || !self.pending_engine.is_empty()
                    || self.shutdown_failure.is_some()
                    || self
                        .engine_quit
                        .as_ref()
                        .is_some_and(|quit| !quit.is_ready())
                {
                    index += 1;
                    continue;
                }
                if let Some(quit) = &mut self.engine_quit {
                    quit.begin_cleanup();
                }
                if let Err(error) = self.lifecycle.shutdown(deadline).await {
                    self.record_shutdown_failure(index, error);
                    self.publish_shutdown_notice().await?;
                    index += 1;
                    continue;
                }
            }
            match Self::retry(|| self.application.complete_control(&self.controls[index])).await {
                Ok(()) => {
                    self.reported_deadlines
                        .remove(&self.controls[index].receipt().request_id());
                    self.controls.remove(index);
                    if is_quit {
                        self.engine_quit = None;
                        self.quit_deadline = None;
                        return Ok(true);
                    }
                }
                Err(DesktopApplicationError::CancellationUnacknowledged) => index += 1,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(false)
    }

    async fn engine_quiescence_ready(
        quit: &mut Option<DesktopCoordinatorEngineQuitEnumPhase>,
    ) -> (usize, Result<(), DesktopFailure>) {
        poll_fn(|context| {
            if let Some(DesktopCoordinatorEngineQuitEnumPhase::Quiescing(attempts)) = quit {
                for (index, attempt) in attempts.iter_mut().enumerate() {
                    if attempt.outcome.is_none()
                        && let Poll::Ready(outcome) = attempt.future.as_mut().poll(context)
                    {
                        return Poll::Ready((index, outcome));
                    }
                }
            }
            Poll::Pending
        })
        .await
    }

    async fn complete_games(&mut self) -> Result<(), DesktopCoordinatorError> {
        let mut index = 0;
        while index < self.pending_games.len() {
            let Some(response) = self.pending_games[index].response.clone() else {
                index += 1;
                continue;
            };
            let work = &self.pending_games[index].work;
            let result = if work.request().command().operation().is_control() {
                Self::retry(|| {
                    self.application
                        .complete_game_control(work, response.clone())
                })
                .await
            } else {
                match response {
                    Ok(snapshot) => {
                        Self::retry(|| {
                            self.application.complete_value(
                                work,
                                DesktopEnumValue::Game(Box::new(snapshot.clone())),
                                Some(DesktopRevision::for_game(snapshot.revision())),
                            )
                        })
                        .await
                    }
                    Err(failure) => {
                        Self::retry(|| self.application.complete_failure(work, failure.clone()))
                            .await
                    }
                }
            };
            match result {
                Ok(()) => {
                    self.pending_games.swap_remove(index);
                }
                Err(DesktopApplicationError::CancellationUnacknowledged) => index += 1,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    async fn dispatch_engine(
        &mut self,
        work: DesktopApplicationWork,
        operation: crate::desktop::engine_commands::DesktopEngineCommandsOperation,
    ) -> Result<(), DesktopCoordinatorError> {
        let failure = if work.request().command().expected_revision()
            != Some(operation.context().configuration())
        {
            Some(DesktopFailure::new(
                DesktopEnumFailureCode::StaleRevision,
                "The engine command has a mismatched configuration revision".to_owned(),
                true,
            ))
        } else if self.engine.is_none() {
            Some(DesktopFailure::new(
                DesktopEnumFailureCode::ResourceRefusal,
                "The native analysis and workbench service is unavailable".to_owned(),
                true,
            ))
        } else {
            None
        };
        let mut selection = None;
        let failure = if let Some(failure) = failure {
            Some(failure)
        } else if let Some((handle, purpose)) = Self::engine_file_selection(&operation) {
            let now = self.clock.now()?;
            let selected = self
                .files
                .as_mut()
                .ok_or(DesktopFileHandleError::Saturated)
                .and_then(|files| {
                    files.registry.consume(
                        handle,
                        operation.context().owner().file_handle_owner(),
                        purpose,
                        now,
                    )
                });
            match selected {
                Ok(selected) => {
                    selection = Some(selected);
                    None
                }
                Err(error) => Some(DesktopFailure::new(
                    DesktopEnumFailureCode::FileUnavailable,
                    error.to_string(),
                    true,
                )),
            }
        } else {
            None
        };
        if !work.request().command().operation().is_control() {
            match Self::retry(|| self.application.begin_commit(&work)).await {
                Ok(()) => {}
                Err(DesktopApplicationError::CancellationRequested) => {
                    Self::retry(|| self.application.acknowledge_stopped(&work)).await?;
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            }
        }
        let future = match (&self.engine, &failure) {
            (Some(engine), None) => Some(engine.execute(DesktopEngineBridgeRequest {
                operation,
                receipt: work.receipt().clone(),
                selection,
            })),
            (Some(_), Some(_)) | (None, _) => None,
        };
        self.pending_engine.push(DesktopCoordinatorEnginePending {
            work,
            future,
            response: failure.map(Err),
        });
        Ok(())
    }

    fn engine_file_selection(
        operation: &DesktopEngineCommandsOperation,
    ) -> Option<(DesktopFileHandleId, DesktopFileEnumPurpose)> {
        match operation.action() {
            DesktopEngineCommandsEnumOperation::PrepareTablebases { directory } => {
                Some((*directory, DesktopFileEnumPurpose::Directory))
            }
            DesktopEngineCommandsEnumOperation::Workbench(
                DesktopWorkbenchCommandsEnumOperation::ApplyExport { destination, .. },
            ) => Some((*destination, DesktopFileEnumPurpose::WriteFile)),
            DesktopEngineCommandsEnumOperation::Snapshot
            | DesktopEngineCommandsEnumOperation::Start { .. }
            | DesktopEngineCommandsEnumOperation::Stop { .. }
            | DesktopEngineCommandsEnumOperation::Configure(_)
            | DesktopEngineCommandsEnumOperation::Retry
            | DesktopEngineCommandsEnumOperation::LoadTablebases
            | DesktopEngineCommandsEnumOperation::Workbench(
                DesktopWorkbenchCommandsEnumOperation::Start { .. }
                | DesktopWorkbenchCommandsEnumOperation::Cancel { .. }
                | DesktopWorkbenchCommandsEnumOperation::Result { .. }
                | DesktopWorkbenchCommandsEnumOperation::PreviewExport { .. },
            ) => None,
        }
    }

    async fn engine_ready(
        pending: &mut [DesktopCoordinatorEnginePending],
    ) -> (usize, Result<DesktopEngineBridgeEnumResult, DesktopFailure>) {
        std::future::poll_fn(|context| {
            for (index, pending) in pending.iter_mut().enumerate() {
                if let Some(future) = &mut pending.future
                    && let Poll::Ready(response) = future.as_mut().poll(context)
                {
                    pending.future = None;
                    return Poll::Ready((index, response));
                }
            }
            Poll::Pending
        })
        .await
    }

    async fn complete_engine(&mut self) -> Result<(), DesktopCoordinatorError> {
        let mut index = 0;
        while index < self.pending_engine.len() {
            let Some(response) = self.pending_engine[index].response.clone() else {
                index += 1;
                continue;
            };
            let work = &self.pending_engine[index].work;
            let result = if work.request().command().operation().is_control() {
                Self::retry(|| {
                    self.application
                        .complete_engine_control(work, response.clone())
                })
                .await
            } else {
                match response {
                    Ok(value) => {
                        Self::retry(|| {
                            self.application.complete_value(
                                work,
                                DesktopEnumValue::Engine(Box::new(value.clone())),
                                None,
                            )
                        })
                        .await
                    }
                    Err(failure) => {
                        Self::retry(|| self.application.complete_failure(work, failure.clone()))
                            .await
                    }
                }
            };
            match result {
                Ok(()) => {
                    self.pending_engine.swap_remove(index);
                }
                Err(DesktopApplicationError::CancellationUnacknowledged) => index += 1,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    async fn game_ready(
        pending: &mut [DesktopCoordinatorGamePending],
    ) -> (usize, Result<DesktopGameSnapshot, DesktopFailure>) {
        std::future::poll_fn(|context| {
            for (index, pending) in pending.iter_mut().enumerate() {
                if let Some(future) = &mut pending.future
                    && let Poll::Ready(response) = future.as_mut().poll(context)
                {
                    pending.future = None;
                    return Poll::Ready((index, response));
                }
            }
            Poll::Pending
        })
        .await
    }

    async fn game_snapshots_ready(
        receiver: &mut Option<watch::Receiver<Vec<DesktopGameSnapshot>>>,
    ) -> Result<Vec<DesktopGameSnapshot>, DesktopCoordinatorError> {
        match receiver {
            Some(receiver) => {
                receiver
                    .changed()
                    .await
                    .map_err(|_closed| DesktopCoordinatorError::GamesClosed)?;
                Ok(receiver.borrow_and_update().clone())
            }
            None => std::future::pending().await,
        }
    }

    async fn publish_games(
        &mut self,
        snapshots: Vec<DesktopGameSnapshot>,
    ) -> Result<(), DesktopCoordinatorError> {
        self.latest_games = snapshots;
        if let Some(stream) = &mut self.stream
            && let Some(event) = stream.publish(
                DesktopEnumEvent::Games(self.latest_games.clone()),
                DesktopEnumStreamClass::Snapshot,
                self.clock.now()?,
            )?
        {
            Self::retry(|| self.application.publish_event(event.clone())).await?;
        }
        Ok(())
    }

    async fn selection_ready(
        pending: &mut Option<DesktopCoordinatorSelection<P>>,
    ) -> (
        DesktopCoordinatorFiles<P>,
        Result<Option<DesktopFileHandleId>, DesktopFileHandleError>,
    ) {
        match pending {
            Some(selection) => selection.selection.as_mut().await,
            None => std::future::pending().await,
        }
    }

    async fn finish_selection(
        &mut self,
        (mut files, outcome): (
            DesktopCoordinatorFiles<P>,
            Result<Option<DesktopFileHandleId>, DesktopFileHandleError>,
        ),
    ) -> Result<(), DesktopCoordinatorError> {
        let Some(selection) = self.pending_selection.take() else {
            return Err(DesktopApplicationError::UnknownWork.into());
        };
        let work = selection.work;
        if work.is_cancellation_requested() {
            match outcome {
                Ok(Some(handle)) => {
                    files.registry.revoke(handle);
                }
                Ok(None) => {}
                Err(error) => self.cancellation_failure = Some(error),
            }
            self.files = Some(files);
            Self::retry(|| self.application.acknowledge_stopped(&work)).await?;
            return Ok(());
        }
        self.files = Some(files);
        match outcome {
            Ok(handle) => match Self::retry(|| self.application.begin_commit(&work)).await {
                Ok(()) => {
                    self.complete(&work, DesktopEnumValue::FileSelect(handle))
                        .await?
                }
                Err(DesktopApplicationError::CancellationRequested) => {
                    if let Some(handle) = handle
                        && let Some(files) = &mut self.files
                    {
                        files.registry.revoke(handle);
                    }
                    Self::retry(|| self.application.acknowledge_stopped(&work)).await?;
                }
                Err(error) => return Err(error.into()),
            },
            Err(error) => {
                self.fail(
                    &work,
                    DesktopFailure::new(
                        DesktopEnumFailureCode::FileUnavailable,
                        error.to_string(),
                        true,
                    ),
                )
                .await?
            }
        }
        Ok(())
    }

    async fn complete(
        &mut self,
        work: &DesktopApplicationWork,
        value: DesktopEnumValue,
    ) -> Result<(), DesktopCoordinatorError> {
        match Self::retry(|| self.application.complete_value(work, value.clone(), None)).await {
            Ok(()) => Ok(()),
            Err(DesktopApplicationError::CancellationRequested) => {
                Self::retry(|| self.application.acknowledge_stopped(work)).await?;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn fail(
        &mut self,
        work: &DesktopApplicationWork,
        failure: DesktopFailure,
    ) -> Result<(), DesktopCoordinatorError> {
        match Self::retry(|| self.application.complete_failure(work, failure.clone())).await {
            Ok(()) => Ok(()),
            Err(DesktopApplicationError::CancellationRequested) => {
                Self::retry(|| self.application.acknowledge_stopped(work)).await?;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    fn acknowledge(
        &mut self,
        subscription: DesktopRequestId,
        sequence: DesktopRevision,
    ) -> Result<DesktopEnumValue, DesktopCoordinatorError> {
        let now = self.clock.now()?;
        self.stream_for(subscription)?.acknowledge(sequence, now)?;
        Ok(DesktopEnumValue::StreamAcknowledge)
    }

    async fn resynchronize(
        &mut self,
        subscription: DesktopRequestId,
    ) -> Result<DesktopEnumValue, DesktopCoordinatorError> {
        let now = self.clock.now()?;
        let snapshot = self.snapshot.clone();
        let event = self
            .stream_for(subscription)?
            .resynchronize(DesktopEnumEvent::Snapshot(snapshot.clone()), now)?;
        let sequence = event.sequence();
        Self::retry(|| self.application.clear_events()).await?;
        Self::retry(|| self.application.publish_event(event.clone())).await?;
        if self.games.is_some() {
            self.publish_games(self.latest_games.clone()).await?;
        }
        Ok(DesktopEnumValue::StreamResync {
            subscription_id: subscription,
            sequence,
            snapshot,
        })
    }

    async fn complete_stream(
        &mut self,
        work: &DesktopApplicationWork,
        result: Result<DesktopEnumValue, DesktopCoordinatorError>,
    ) -> Result<(), DesktopCoordinatorError> {
        match result {
            Ok(value) => self.complete(work, value).await,
            Err(DesktopCoordinatorError::Stream(error)) => self.fail(work, error.into()).await,
            Err(error) => Err(error),
        }
    }

    fn stream_for(
        &mut self,
        subscription: DesktopRequestId,
    ) -> Result<&mut DesktopStream<DesktopEnumEvent>, DesktopCoordinatorError> {
        if self.subscription != Some(subscription) {
            return Err(DesktopError::ResyncRequired.into());
        }
        self.stream
            .as_mut()
            .ok_or(DesktopError::ResyncRequired.into())
    }

    fn next_deadline(&self) -> Result<Option<Instant>, DesktopCoordinatorError> {
        let stream_deadline = self
            .stream
            .as_ref()
            .map(DesktopStream::acknowledgement_deadline)
            .transpose()?
            .flatten();
        let control_deadline = self
            .controls
            .iter()
            .filter(|control| {
                !self.uses_quit_deadline(control)
                    && !self
                        .reported_deadlines
                        .contains(&control.receipt().request_id())
            })
            .filter_map(|control| control.receipt().deadline())
            .min();
        let application_deadline = match stream_deadline.into_iter().chain(control_deadline).min() {
            Some(deadline) => {
                let now = self.clock.now()?;
                Some(
                    Instant::now()
                        + if now >= deadline {
                            Duration::ZERO
                        } else {
                            Duration::from_nanos(deadline.elapsed_since(now)?.as_nanos())
                        },
                )
            }
            None => None,
        };
        let quit_deadline = if self.shutdown_failure.is_none() {
            self.quit_deadline
        } else {
            None
        };
        Ok(application_deadline.into_iter().chain(quit_deadline).min())
    }

    async fn wait_deadline(deadline: Option<Instant>) {
        match deadline {
            Some(deadline) => tokio::time::sleep_until(deadline).await,
            None => std::future::pending().await,
        }
    }

    fn check_stream(&mut self) -> Result<(), DesktopCoordinatorError> {
        let now = self.clock.now()?;
        if let Some(stream) = &mut self.stream {
            match stream.check_health(now) {
                Ok(()) | Err(DesktopError::ResyncRequired) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    pub fn publish_snapshot(
        &mut self,
        snapshot: DesktopBootstrapSnapshot,
    ) -> Result<(), DesktopCoordinatorError> {
        self.snapshot = snapshot;
        let now = self.clock.now()?;
        if let Some(stream) = &mut self.stream
            && let Some(event) = stream.publish(
                DesktopEnumEvent::Snapshot(self.snapshot.clone()),
                DesktopEnumStreamClass::Snapshot,
                now,
            )?
        {
            self.application.publish_event(event)?;
        }
        Ok(())
    }
}
