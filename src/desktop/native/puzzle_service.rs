use std::future::{Future, pending, poll_fn};
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::task::Poll;
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until, timeout_at};

use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::ClockInstant;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{DesktopApplicationCancellation, DesktopApplicationWakeProvider};
use crate::desktop::native::puzzle_runtime::{
    DesktopNativePuzzleRuntime, DesktopNativePuzzleRuntimePort, DesktopNativePuzzleRuntimeScheduler,
};
use crate::desktop::puzzle_bridge::{
    DesktopPuzzleBridgeEnumHealth, DesktopPuzzleBridgeOutcome, DesktopPuzzleBridgeProvider,
    DesktopPuzzleBridgeReceipt, DesktopPuzzleBridgeRequest,
};
use crate::desktop::puzzle_commands::DesktopPuzzleCommands;
use crate::desktop::puzzle_service::{
    DesktopPuzzleServiceEnumActivity, DesktopPuzzleServiceEnumOperation,
    DesktopPuzzleServiceEnumResult, DesktopPuzzleServiceError, DesktopPuzzleServiceNativeProgress,
    DesktopPuzzleServiceOutcome, DesktopPuzzleServiceOutcomeSender, DesktopPuzzleServiceProvider,
    DesktopPuzzleServiceReceipt, DesktopPuzzleServiceRequest, DesktopPuzzleServiceState,
};
use crate::desktop::puzzle_snapshot::DesktopPuzzleSnapshot;
use crate::puzzle::history::PuzzleHistoryAuthorization;
use crate::puzzle::run::PuzzleRunEnumStatus;
use crate::puzzle::session::PuzzleSessionId;
use crate::puzzle::timer::PuzzleTimerEnumPauseReason;
use crate::storage::puzzle_repository::{StoragePuzzleRecovered, StoragePuzzleRepository};

pub struct DesktopNativePuzzleServiceOptions<C, W, S> {
    pub repository: StoragePuzzleRepository,
    pub executor: Arc<CapacityExecutor>,
    pub clock: Arc<C>,
    pub wake: Arc<W>,
    pub runtime: Handle,
    pub scheduler: S,
    pub replacement_timeout: Duration,
}

struct DesktopNativePuzzleServiceRequest {
    request: DesktopPuzzleServiceRequest,
    response: DesktopPuzzleServiceOutcomeSender<DesktopPuzzleServiceEnumResult>,
}

struct DesktopNativePuzzleServiceInterruption {
    session: PuzzleSessionId,
    reason: PuzzleTimerEnumPauseReason,
    response: DesktopPuzzleServiceOutcomeSender<DesktopPuzzleServiceEnumResult>,
}

struct DesktopNativePuzzleServiceShutdownSlot(Arc<AtomicBool>);

impl Drop for DesktopNativePuzzleServiceShutdownSlot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct DesktopNativePuzzleServiceShutdown {
    deadline: Instant,
    response: DesktopPuzzleServiceOutcomeSender<()>,
    _slot: DesktopNativePuzzleServiceShutdownSlot,
}

#[derive(Clone)]
pub struct DesktopNativePuzzleServicePort {
    ordinary: mpsc::Sender<DesktopNativePuzzleServiceRequest>,
    urgent: mpsc::Sender<DesktopNativePuzzleServiceRequest>,
    interruptions: mpsc::Sender<DesktopNativePuzzleServiceInterruption>,
    shutdowns: mpsc::Sender<DesktopNativePuzzleServiceShutdown>,
    state: watch::Receiver<DesktopPuzzleServiceState>,
    shutdown_pending: Arc<AtomicBool>,
    complete: Arc<AtomicBool>,
}

impl DesktopNativePuzzleServicePort {
    pub const ORDINARY_CAPACITY: usize = 32;
    pub const URGENT_CAPACITY: usize = 2;
    pub const INTERRUPTION_CAPACITY: usize = 1;
    const SHUTDOWN_CAPACITY: usize = 1;

    pub fn interrupt_selected(
        &self,
        reason: PuzzleTimerEnumPauseReason,
    ) -> DesktopPuzzleServiceOutcome<DesktopPuzzleServiceReceipt> {
        let session = self
            .state
            .borrow()
            .selected()
            .ok_or_else(|| Arc::new(DesktopPuzzleServiceError::Incomplete))?;
        self.interrupt(session, reason)
    }

    pub fn request_shutdown(
        &self,
        deadline: Instant,
    ) -> DesktopPuzzleServiceOutcome<DesktopPuzzleServiceReceipt<()>> {
        let (response, receipt) = DesktopPuzzleServiceReceipt::channel();
        if self.is_shutdown_complete() {
            response.send_replace(Some(Ok(())));
            return Ok(receipt);
        }
        self.shutdown_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_pending| Arc::new(DesktopPuzzleServiceError::Busy))?;
        self.shutdowns
            .try_send(DesktopNativePuzzleServiceShutdown {
                deadline,
                response,
                _slot: DesktopNativePuzzleServiceShutdownSlot(Arc::clone(&self.shutdown_pending)),
            })
            .map_err(Self::admission_error)?;
        Ok(receipt)
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.complete.load(Ordering::Acquire)
    }

    fn admission_error<T>(error: mpsc::error::TrySendError<T>) -> Arc<DesktopPuzzleServiceError> {
        Arc::new(match error {
            mpsc::error::TrySendError::Full(_) => DesktopPuzzleServiceError::Busy,
            mpsc::error::TrySendError::Closed(_) => DesktopPuzzleServiceError::Closed,
        })
    }
}

impl DesktopPuzzleServiceProvider for DesktopNativePuzzleServicePort {
    fn submit(
        &self,
        request: DesktopPuzzleServiceRequest,
    ) -> DesktopPuzzleServiceOutcome<DesktopPuzzleServiceReceipt> {
        let (response, receipt) = DesktopPuzzleServiceReceipt::channel();
        let queue = if request.operation.is_urgent() {
            &self.urgent
        } else {
            &self.ordinary
        };
        queue
            .try_send(DesktopNativePuzzleServiceRequest { request, response })
            .map_err(Self::admission_error)?;
        Ok(receipt)
    }

    fn interrupt(
        &self,
        session: PuzzleSessionId,
        reason: PuzzleTimerEnumPauseReason,
    ) -> DesktopPuzzleServiceOutcome<DesktopPuzzleServiceReceipt> {
        let (response, receipt) = DesktopPuzzleServiceReceipt::channel();
        self.interruptions
            .try_send(DesktopNativePuzzleServiceInterruption {
                session,
                reason,
                response,
            })
            .map_err(Self::admission_error)?;
        Ok(receipt)
    }

    fn state(&self) -> watch::Receiver<DesktopPuzzleServiceState> {
        self.state.clone()
    }
}

type DesktopNativePuzzleServiceRuntime<C, W, S> =
    DesktopNativePuzzleRuntime<StoragePuzzleRepository, C, W, S>;
type DesktopNativePuzzleServiceRuntimeOutcome<C, W, S> = (
    Box<DesktopNativePuzzleServiceRuntime<C, W, S>>,
    DesktopPuzzleBridgeOutcome<()>,
);
type DesktopNativePuzzleServiceFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
type DesktopNativePuzzleServiceOperationOutcome = (
    DesktopNativePuzzleServiceRequest,
    DesktopPuzzleServiceOutcome<DesktopPuzzleServiceEnumResult>,
);

enum DesktopNativePuzzleServiceEnumPrepared {
    Owner {
        commands: Box<DesktopPuzzleCommands<StoragePuzzleRepository>>,
        failure: Option<Arc<DesktopPuzzleServiceError>>,
    },
    Ended(DesktopPuzzleServiceEnumResult),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopNativePuzzleServiceEnumReplyLane {
    Ordinary,
    Urgent,
    Native,
}

struct DesktopNativePuzzleServiceReply {
    lane: DesktopNativePuzzleServiceEnumReplyLane,
    future: DesktopNativePuzzleServiceFuture<(
        DesktopPuzzleServiceOutcomeSender<DesktopPuzzleServiceEnumResult>,
        DesktopPuzzleServiceOutcome<DesktopPuzzleServiceEnumResult>,
    )>,
}

enum DesktopNativePuzzleServiceEnumCleanup {
    Replace {
        envelope: DesktopNativePuzzleServiceRequest,
        deadline: Instant,
    },
    Shutdown(DesktopNativePuzzleServiceShutdown),
}

impl DesktopNativePuzzleServiceEnumCleanup {
    fn deadline(&self) -> Instant {
        match self {
            Self::Replace { deadline, .. } => *deadline,
            Self::Shutdown(request) => request.deadline,
        }
    }

    fn refuse(self, failure: Arc<DesktopPuzzleServiceError>) {
        match self {
            Self::Replace { envelope, .. } => {
                envelope.response.send_replace(Some(Err(failure)));
            }
            Self::Shutdown(request) => {
                let DesktopNativePuzzleServiceShutdown {
                    response, _slot, ..
                } = request;
                drop(_slot);
                response.send_replace(Some(Err(failure)));
            }
        }
    }
}

struct DesktopNativePuzzleServiceOwner<C, W, S> {
    options: DesktopNativePuzzleServiceOptions<C, W, S>,
    ordinary: mpsc::Receiver<DesktopNativePuzzleServiceRequest>,
    urgent: mpsc::Receiver<DesktopNativePuzzleServiceRequest>,
    interruptions: mpsc::Receiver<DesktopNativePuzzleServiceInterruption>,
    shutdowns: mpsc::Receiver<DesktopNativePuzzleServiceShutdown>,
    state: watch::Sender<DesktopPuzzleServiceState>,
    native_admission_progress: DesktopPuzzleServiceNativeProgress,
    complete: Arc<AtomicBool>,
    port: Option<DesktopNativePuzzleRuntimePort>,
    worker: Option<JoinHandle<DesktopNativePuzzleServiceRuntimeOutcome<C, W, S>>>,
    retained: Option<Box<DesktopNativePuzzleServiceRuntime<C, W, S>>>,
    worker_failure: Option<Arc<DesktopPuzzleServiceError>>,
    snapshots: watch::Receiver<Option<Arc<DesktopPuzzleSnapshot>>>,
    health: watch::Receiver<DesktopPuzzleBridgeEnumHealth>,
    storage_task:
        Option<DesktopNativePuzzleServiceFuture<DesktopNativePuzzleServiceOperationOutcome>>,
    recovery: Option<
        DesktopNativePuzzleServiceFuture<(
            DesktopNativePuzzleServiceRequest,
            DesktopPuzzleServiceOutcome<DesktopNativePuzzleServiceEnumPrepared>,
        )>,
    >,
    opening: Option<DesktopNativePuzzleServiceRequest>,
    replies: Vec<DesktopNativePuzzleServiceReply>,
    cleanup: Option<DesktopNativePuzzleServiceEnumCleanup>,
    runtime_cleanup: Option<DesktopPuzzleBridgeReceipt<()>>,
    failure: Option<Arc<DesktopPuzzleServiceError>>,
    activity: DesktopPuzzleServiceEnumActivity,
    closing: bool,
}

impl<C, W, S> DesktopNativePuzzleServiceOwner<C, W, S>
where
    C: ClockTimeProvider + 'static,
    W: DesktopApplicationWakeProvider + 'static,
    S: DesktopNativePuzzleRuntimeScheduler + Clone + 'static,
{
    fn new(
        options: DesktopNativePuzzleServiceOptions<C, W, S>,
    ) -> (Self, DesktopNativePuzzleServicePort) {
        let (ordinary, ordinary_receiver) =
            mpsc::channel(DesktopNativePuzzleServicePort::ORDINARY_CAPACITY);
        let (urgent, urgent_receiver) =
            mpsc::channel(DesktopNativePuzzleServicePort::URGENT_CAPACITY);
        let (interruptions, interruption_receiver) =
            mpsc::channel(DesktopNativePuzzleServicePort::INTERRUPTION_CAPACITY);
        let (shutdowns, shutdown_receiver) =
            mpsc::channel(DesktopNativePuzzleServicePort::SHUTDOWN_CAPACITY);
        let (state, state_receiver) = watch::channel(DesktopPuzzleServiceState::new(
            None,
            None,
            DesktopPuzzleServiceEnumActivity::Idle,
            None,
            None,
        ));
        let complete = Arc::new(AtomicBool::new(false));
        let (_, snapshots) = watch::channel(None);
        let (_, health) = watch::channel(DesktopPuzzleBridgeEnumHealth::Starting);
        (
            Self {
                options,
                ordinary: ordinary_receiver,
                urgent: urgent_receiver,
                interruptions: interruption_receiver,
                shutdowns: shutdown_receiver,
                state,
                native_admission_progress: DesktopPuzzleServiceNativeProgress::new(),
                complete: Arc::clone(&complete),
                port: None,
                worker: None,
                retained: None,
                worker_failure: None,
                snapshots,
                health,
                storage_task: None,
                recovery: None,
                opening: None,
                replies: Vec::new(),
                cleanup: None,
                runtime_cleanup: None,
                failure: None,
                activity: DesktopPuzzleServiceEnumActivity::Idle,
                closing: false,
            },
            DesktopNativePuzzleServicePort {
                ordinary,
                urgent,
                interruptions,
                shutdowns,
                state: state_receiver,
                shutdown_pending: Arc::new(AtomicBool::new(false)),
                complete,
            },
        )
    }

    async fn run(&mut self) -> DesktopPuzzleServiceOutcome<()> {
        loop {
            self.advance_cleanup();
            if self.complete.load(Ordering::Acquire) {
                return Ok(());
            }
            let has_selected = self.port.is_some() && self.worker_failure.is_none();
            let cleanup_deadline = self.cleanup.as_ref().and_then(|cleanup| {
                if self
                    .port
                    .as_ref()
                    .is_some_and(DesktopNativePuzzleRuntimePort::is_shutdown_complete)
                {
                    None
                } else {
                    Some(cleanup.deadline())
                }
            });
            tokio::select! {
                biased;
                result = Self::wait_runtime_cleanup(&mut self.runtime_cleanup) => { self.observe_runtime_cleanup(result); }
                joined = Self::wait_worker(&mut self.worker) => { self.observe_worker(joined); }
                shutdown = self.shutdowns.recv(), if self.cleanup.is_none() && self.runtime_cleanup.is_none() => {
                    if let Some(shutdown) = shutdown { self.begin_shutdown(shutdown); }
                }
                (index, response, result) = Self::wait_replies(&mut self.replies) => {
                    let reply = self.replies.swap_remove(index);
                    if reply.lane == DesktopNativePuzzleServiceEnumReplyLane::Native {
                        self.record_native_admission_progress();
                    }
                    drop(reply);
                    self.respond(response, result);
                }
                interruption = self.interruptions.recv() => {
                    if let Some(interruption) = interruption { self.dispatch_interruption(interruption); }
                }
                request = self.urgent.recv() => {
                    if let Some(request) = request { self.dispatch(request, DesktopNativePuzzleServiceEnumReplyLane::Urgent); }
                }
                result = Self::wait_future(&mut self.recovery) => {
                    self.recovery = None;
                    self.observe_recovery(result);
                }
                result = Self::wait_future(&mut self.storage_task) => {
                    self.storage_task = None;
                    self.finish_operation(result);
                }
                changed = self.snapshots.changed(), if has_selected => {
                    match changed { Ok(()) => self.observe_snapshot(), Err(error) => self.record_failure(Arc::new(DesktopPuzzleServiceError::Outcome(error))) }
                }
                changed = self.health.changed(), if has_selected => {
                    match changed { Ok(()) => self.publish(), Err(error) => self.record_failure(Arc::new(DesktopPuzzleServiceError::Outcome(error))) }
                }
                () = Self::wait_deadline(cleanup_deadline) => { self.fail_cleanup(Arc::new(DesktopPuzzleServiceError::ShutdownDeadline)); }
                request = self.ordinary.recv() => {
                    if let Some(request) = request { self.dispatch(request, DesktopNativePuzzleServiceEnumReplyLane::Ordinary); }
                }
            }
        }
    }

    async fn wait_future<T>(future: &mut Option<DesktopNativePuzzleServiceFuture<T>>) -> T {
        match future {
            Some(future) => future.await,
            None => pending().await,
        }
    }

    async fn wait_worker(
        worker: &mut Option<JoinHandle<DesktopNativePuzzleServiceRuntimeOutcome<C, W, S>>>,
    ) -> Result<DesktopNativePuzzleServiceRuntimeOutcome<C, W, S>, tokio::task::JoinError> {
        match worker {
            Some(worker) => worker.await,
            None => pending().await,
        }
    }

    async fn wait_runtime_cleanup(
        cleanup: &mut Option<DesktopPuzzleBridgeReceipt<()>>,
    ) -> DesktopPuzzleBridgeOutcome<()> {
        match cleanup {
            Some(cleanup) => cleanup.wait().await,
            None => pending().await,
        }
    }

    async fn wait_deadline(deadline: Option<Instant>) {
        match deadline {
            Some(deadline) => sleep_until(deadline).await,
            None => pending().await,
        }
    }

    async fn wait_replies(
        replies: &mut [DesktopNativePuzzleServiceReply],
    ) -> (
        usize,
        DesktopPuzzleServiceOutcomeSender<DesktopPuzzleServiceEnumResult>,
        DesktopPuzzleServiceOutcome<DesktopPuzzleServiceEnumResult>,
    ) {
        poll_fn(|context| {
            for (index, reply) in replies.iter_mut().enumerate() {
                if let Poll::Ready((response, result)) = reply.future.as_mut().poll(context) {
                    return Poll::Ready((index, response, result));
                }
            }
            Poll::Pending
        })
        .await
    }

    fn dispatch(
        &mut self,
        envelope: DesktopNativePuzzleServiceRequest,
        lane: DesktopNativePuzzleServiceEnumReplyLane,
    ) {
        let refusal = if envelope.request.receipt.is_cancellation_requested() {
            Some(DesktopPuzzleServiceError::Cancelled)
        } else if self.closing && !envelope.request.operation.is_available_while_closing() {
            Some(DesktopPuzzleServiceError::Closing)
        } else {
            None
        };
        if let Some(refusal) = refusal {
            self.respond(envelope.response, Err(Arc::new(refusal)));
            return;
        }
        match &envelope.request.operation {
            DesktopPuzzleServiceEnumOperation::Inventory { after } => {
                if self.storage_task.is_some() {
                    self.respond(
                        envelope.response,
                        Err(Arc::new(DesktopPuzzleServiceError::Busy)),
                    );
                    return;
                }
                let repository = self.options.repository.clone();
                let after = *after;
                self.storage_task = Some(Box::pin(async move {
                    let result = repository
                        .inventory(after)
                        .await
                        .map(|page| DesktopPuzzleServiceEnumResult::Inventory(Arc::new(page)))
                        .map_err(|error| Arc::new(DesktopPuzzleServiceError::Storage(error)));
                    (envelope, result)
                }));
            }
            DesktopPuzzleServiceEnumOperation::DeleteHistory {
                scope,
                confirmation,
            } => {
                if self.storage_task.is_some() {
                    self.respond(
                        envelope.response,
                        Err(Arc::new(DesktopPuzzleServiceError::Busy)),
                    );
                    return;
                }
                let active = {
                    let state = self.state.borrow();
                    state.selected().filter(|_| {
                        state.snapshot().is_none_or(|snapshot| {
                            !matches!(
                                snapshot.status(),
                                PuzzleRunEnumStatus::Ended(_)
                                    | PuzzleRunEnumStatus::Exhausted
                                    | PuzzleRunEnumStatus::Expired(_)
                            )
                        })
                    })
                };
                let authorization =
                    match PuzzleHistoryAuthorization::new(*scope, *confirmation, active) {
                        Ok(authorization) => authorization,
                        Err(error) => {
                            self.respond(
                                envelope.response,
                                Err(Arc::new(DesktopPuzzleServiceError::History(error))),
                            );
                            return;
                        }
                    };
                let repository = self.options.repository.clone();
                self.storage_task = Some(Box::pin(async move {
                    let result = repository
                        .delete_history(authorization)
                        .await
                        .map(|removed| DesktopPuzzleServiceEnumResult::HistoryDeleted { removed })
                        .map_err(|error| Arc::new(DesktopPuzzleServiceError::Storage(error)));
                    (envelope, result)
                }));
            }
            DesktopPuzzleServiceEnumOperation::OpenSaved { .. } => self.open(envelope),
            DesktopPuzzleServiceEnumOperation::Session { .. }
            | DesktopPuzzleServiceEnumOperation::Interrupt { .. } => self.forward(envelope, lane),
        }
    }

    fn open(&mut self, envelope: DesktopNativePuzzleServiceRequest) {
        if self.recovery.is_some()
            || self.opening.is_some()
            || self.cleanup.is_some()
            || self.runtime_cleanup.is_some()
            || !self.replies.is_empty()
        {
            self.respond(
                envelope.response,
                Err(Arc::new(DesktopPuzzleServiceError::Busy)),
            );
            return;
        }
        if self.port.is_some() {
            let safe = self
                .snapshots
                .borrow()
                .as_ref()
                .is_some_and(|snapshot| Self::is_durable_and_hidden(snapshot));
            if !safe || self.worker_failure.is_some() {
                self.respond(
                    envelope.response,
                    Err(Arc::new(DesktopPuzzleServiceError::ReplacementBlocked)),
                );
                return;
            }
            let id = match &envelope.request.operation {
                DesktopPuzzleServiceEnumOperation::OpenSaved { id, .. } => *id,
                DesktopPuzzleServiceEnumOperation::Inventory { .. }
                | DesktopPuzzleServiceEnumOperation::Session { .. }
                | DesktopPuzzleServiceEnumOperation::Interrupt { .. }
                | DesktopPuzzleServiceEnumOperation::DeleteHistory { .. } => {
                    self.respond(
                        envelope.response,
                        Err(Arc::new(DesktopPuzzleServiceError::Incomplete)),
                    );
                    return;
                }
            };
            let Some(deadline) = Instant::now().checked_add(self.options.replacement_timeout)
            else {
                self.respond(
                    envelope.response,
                    Err(Arc::new(DesktopPuzzleServiceError::ShutdownDeadline)),
                );
                return;
            };
            self.activity = DesktopPuzzleServiceEnumActivity::Replacing { id };
            self.cleanup =
                Some(DesktopNativePuzzleServiceEnumCleanup::Replace { envelope, deadline });
            self.publish();
        } else {
            self.begin_load(envelope);
        }
    }

    fn begin_load(&mut self, envelope: DesktopNativePuzzleServiceRequest) {
        let (id, expected) = match &envelope.request.operation {
            DesktopPuzzleServiceEnumOperation::OpenSaved {
                id,
                storage_revision,
            } => (*id, *storage_revision),
            DesktopPuzzleServiceEnumOperation::Inventory { .. }
            | DesktopPuzzleServiceEnumOperation::Session { .. }
            | DesktopPuzzleServiceEnumOperation::Interrupt { .. }
            | DesktopPuzzleServiceEnumOperation::DeleteHistory { .. } => {
                self.respond(
                    envelope.response,
                    Err(Arc::new(DesktopPuzzleServiceError::Incomplete)),
                );
                return;
            }
        };
        let repository = self.options.repository.clone();
        let executor = Arc::clone(&self.options.executor);
        let clock = Arc::clone(&self.options.clock);
        self.activity = DesktopPuzzleServiceEnumActivity::Loading { id };
        self.recovery = Some(Box::pin(async move {
            let result = async {
                let recovered = repository
                    .load(id)
                    .await
                    .map_err(DesktopPuzzleServiceError::Storage)?
                    .ok_or(DesktopPuzzleServiceError::Missing(id))?;
                if envelope.request.receipt.is_cancellation_requested() {
                    return Err(DesktopPuzzleServiceError::Cancelled);
                }
                let actual = recovered.storage_revision();
                if expected != actual {
                    return Err(DesktopPuzzleServiceError::RevisionConflict {
                        id,
                        expected,
                        actual,
                    });
                }
                let now = clock.now()?;
                let cancellation = envelope.request.receipt.cancellation();
                let job = executor
                    .submit_waiting(move |_cancellation| {
                        Self::prepare(recovered, repository, now, &cancellation)
                    })
                    .await?;
                job.join().await?
            }
            .await
            .map_err(Arc::new);
            (envelope, result)
        }));
        self.publish();
    }

    fn prepare(
        recovered: StoragePuzzleRecovered,
        repository: StoragePuzzleRepository,
        now: ClockInstant,
        cancellation: &DesktopApplicationCancellation,
    ) -> Result<DesktopNativePuzzleServiceEnumPrepared, DesktopPuzzleServiceError> {
        if cancellation.is_requested() {
            return Err(DesktopPuzzleServiceError::Cancelled);
        }
        let storage_revision = recovered.storage_revision();
        let persisted_event_count = recovered.persisted_event_count();
        let owner = recovered.into_owner();
        let preparation = |error| DesktopPuzzleServiceError::Preparation(Box::new(error));
        if matches!(
            owner.run().view(now).map_err(preparation)?.status(),
            PuzzleRunEnumStatus::Ended(_)
        ) {
            return Ok(DesktopNativePuzzleServiceEnumPrepared::Ended(
                DesktopPuzzleServiceEnumResult::Ended {
                    id: owner.run().context().session_id(),
                    storage_revision,
                    summary: owner.run().summary(now).map_err(preparation)?,
                },
            ));
        }
        let mut commands = DesktopPuzzleCommands::new(
            owner,
            repository,
            now,
            storage_revision,
            persisted_event_count,
        )
        .map_err(|error| DesktopPuzzleServiceError::Preparation(Box::new(error)))?;
        if cancellation.is_requested() {
            return Err(DesktopPuzzleServiceError::Cancelled);
        }
        let failure = commands
            .begin_write(now)
            .err()
            .map(|error| Arc::new(DesktopPuzzleServiceError::Preparation(Box::new(error))));
        Ok(DesktopNativePuzzleServiceEnumPrepared::Owner {
            commands: Box::new(commands),
            failure,
        })
    }

    fn observe_recovery(
        &mut self,
        (envelope, result): (
            DesktopNativePuzzleServiceRequest,
            DesktopPuzzleServiceOutcome<DesktopNativePuzzleServiceEnumPrepared>,
        ),
    ) {
        match result {
            Ok(DesktopNativePuzzleServiceEnumPrepared::Owner { commands, failure }) => {
                let (runtime, port) = DesktopNativePuzzleRuntime::new(
                    *commands,
                    Arc::clone(&self.options.clock),
                    Arc::clone(&self.options.wake),
                    self.options.scheduler.clone(),
                );
                self.snapshots = port.snapshots();
                self.health = port.health();
                self.port = Some(port);
                self.retained = Some(Box::new(runtime));
                self.failure = failure;
                self.opening = Some(envelope);
                self.activity = DesktopPuzzleServiceEnumActivity::Selected;
                self.ensure_runtime();
            }
            Ok(DesktopNativePuzzleServiceEnumPrepared::Ended(result)) => {
                self.finish_operation((envelope, Ok(result)));
                self.activity = DesktopPuzzleServiceEnumActivity::Idle;
            }
            Err(failure) => {
                self.respond(envelope.response, Err(failure));
                self.activity = DesktopPuzzleServiceEnumActivity::Idle;
            }
        }
        self.publish();
    }

    fn ensure_runtime(&mut self) {
        if self.worker.is_none()
            && let Some(mut runtime) = self.retained.take()
        {
            if runtime.is_shutdown_complete() {
                self.retained = Some(runtime);
                return;
            }
            self.worker = Some(self.options.runtime.spawn(async move {
                let result = runtime.run().await;
                (runtime, result)
            }));
        }
    }

    fn forward(
        &mut self,
        envelope: DesktopNativePuzzleServiceRequest,
        lane: DesktopNativePuzzleServiceEnumReplyLane,
    ) {
        let DesktopNativePuzzleServiceRequest { request, response } = envelope;
        let session = match &request.operation {
            DesktopPuzzleServiceEnumOperation::Session { expected, .. } => expected.session_id(),
            DesktopPuzzleServiceEnumOperation::Interrupt { session, .. } => *session,
            DesktopPuzzleServiceEnumOperation::Inventory { .. }
            | DesktopPuzzleServiceEnumOperation::OpenSaved { .. }
            | DesktopPuzzleServiceEnumOperation::DeleteHistory { .. } => {
                self.respond(
                    response,
                    Err(Arc::new(DesktopPuzzleServiceError::Incomplete)),
                );
                return;
            }
        };
        if let Err(error) = self.validate_session(session) {
            self.respond(response, Err(error));
            return;
        }
        self.ensure_runtime();
        if self.replies.iter().any(|reply| reply.lane == lane) {
            self.respond(response, Err(Arc::new(DesktopPuzzleServiceError::Busy)));
            return;
        }
        let Some(port) = &self.port else {
            self.respond(
                response,
                Err(Arc::new(DesktopPuzzleServiceError::Incomplete)),
            );
            return;
        };
        let admitted = match request.operation {
            DesktopPuzzleServiceEnumOperation::Session {
                expected,
                operation,
            } => port.submit(DesktopPuzzleBridgeRequest {
                receipt: request.receipt,
                expected,
                operation,
            }),
            DesktopPuzzleServiceEnumOperation::Interrupt { reason, .. } => port.interrupt(reason),
            DesktopPuzzleServiceEnumOperation::Inventory { .. }
            | DesktopPuzzleServiceEnumOperation::OpenSaved { .. }
            | DesktopPuzzleServiceEnumOperation::DeleteHistory { .. } => {
                self.respond(
                    response,
                    Err(Arc::new(DesktopPuzzleServiceError::Incomplete)),
                );
                return;
            }
        };
        self.retain_reply(lane, response, admitted);
    }

    fn dispatch_interruption(&mut self, interruption: DesktopNativePuzzleServiceInterruption) {
        if !self
            .replies
            .iter()
            .any(|reply| reply.lane == DesktopNativePuzzleServiceEnumReplyLane::Native)
        {
            self.record_native_admission_progress();
        }
        if let Err(error) = self.validate_session(interruption.session) {
            self.respond(interruption.response, Err(error));
            return;
        }
        self.ensure_runtime();
        if self
            .replies
            .iter()
            .any(|reply| reply.lane == DesktopNativePuzzleServiceEnumReplyLane::Native)
        {
            self.respond(
                interruption.response,
                Err(Arc::new(DesktopPuzzleServiceError::Busy)),
            );
            return;
        }
        let Some(port) = &self.port else {
            self.respond(
                interruption.response,
                Err(Arc::new(DesktopPuzzleServiceError::Incomplete)),
            );
            return;
        };
        let admitted = port.interrupt(interruption.reason);
        self.retain_reply(
            DesktopNativePuzzleServiceEnumReplyLane::Native,
            interruption.response,
            admitted,
        );
    }

    fn record_native_admission_progress(&mut self) {
        self.native_admission_progress = DesktopPuzzleServiceNativeProgress::new();
    }

    fn validate_session(&self, expected: PuzzleSessionId) -> DesktopPuzzleServiceOutcome<()> {
        let selected = self
            .port
            .as_ref()
            .map(DesktopNativePuzzleRuntimePort::session_id);
        if selected != Some(expected) {
            return Err(Arc::new(DesktopPuzzleServiceError::SessionMismatch {
                expected,
                selected,
            }));
        }
        Ok(())
    }

    fn retain_reply(
        &mut self,
        lane: DesktopNativePuzzleServiceEnumReplyLane,
        response: DesktopPuzzleServiceOutcomeSender<DesktopPuzzleServiceEnumResult>,
        admitted: DesktopPuzzleBridgeOutcome<DesktopPuzzleBridgeReceipt>,
    ) {
        match admitted {
            Ok(mut receipt) => self.replies.push(DesktopNativePuzzleServiceReply {
                lane,
                future: Box::pin(async move {
                    let result = receipt
                        .wait()
                        .await
                        .map(DesktopPuzzleServiceEnumResult::Session)
                        .map_err(|error| Arc::new(DesktopPuzzleServiceError::Runtime(error)));
                    (response, result)
                }),
            }),
            Err(error) => self.respond(
                response,
                Err(Arc::new(DesktopPuzzleServiceError::Runtime(error))),
            ),
        }
    }

    fn observe_worker(
        &mut self,
        joined: Result<DesktopNativePuzzleServiceRuntimeOutcome<C, W, S>, tokio::task::JoinError>,
    ) {
        self.worker = None;
        match joined {
            Ok((runtime, result)) => {
                self.retained = Some(runtime);
                if let Err(error) = result {
                    let failure = Arc::new(DesktopPuzzleServiceError::Runtime(error));
                    if let Some(envelope) = self.opening.take() {
                        self.respond(envelope.response, Err(Arc::clone(&failure)));
                    }
                    self.record_failure(failure);
                }
            }
            Err(error) => {
                let failure = Arc::new(DesktopPuzzleServiceError::Worker(Arc::new(error)));
                self.worker_failure = Some(Arc::clone(&failure));
                self.record_failure(failure);
            }
        }
        self.observe_snapshot();
    }

    fn observe_snapshot(&mut self) {
        let snapshot = self.snapshots.borrow_and_update().clone();
        if let Some(snapshot) = snapshot
            && let Some(envelope) = self.opening.take()
        {
            self.finish_operation((
                envelope,
                Ok(DesktopPuzzleServiceEnumResult::Opened(snapshot)),
            ));
        }
        self.publish();
    }

    fn finish_operation(&mut self, (envelope, result): DesktopNativePuzzleServiceOperationOutcome) {
        let result = if envelope.request.receipt.is_cancellation_requested() {
            Err(Arc::new(DesktopPuzzleServiceError::Cancelled))
        } else if self.closing && !envelope.request.operation.is_available_while_closing() {
            Err(Arc::new(DesktopPuzzleServiceError::Closing))
        } else {
            result
        };
        self.respond(envelope.response, result);
    }

    fn respond(
        &mut self,
        response: DesktopPuzzleServiceOutcomeSender<DesktopPuzzleServiceEnumResult>,
        result: DesktopPuzzleServiceOutcome<DesktopPuzzleServiceEnumResult>,
    ) {
        if let Err(failure) = &result {
            self.failure = Some(Arc::clone(failure));
        }
        response.send_replace(Some(result));
        self.publish();
    }

    fn begin_shutdown(&mut self, shutdown: DesktopNativePuzzleServiceShutdown) {
        self.closing = true;
        self.activity = DesktopPuzzleServiceEnumActivity::Closing;
        self.cleanup = Some(DesktopNativePuzzleServiceEnumCleanup::Shutdown(shutdown));
        self.publish();
    }

    fn advance_cleanup(&mut self) {
        let Some(cleanup) = &self.cleanup else {
            return;
        };
        let deadline = cleanup.deadline();
        let runtime_complete = self
            .port
            .as_ref()
            .is_some_and(DesktopNativePuzzleRuntimePort::is_shutdown_complete);
        if !runtime_complete && Instant::now() >= deadline {
            self.fail_cleanup(Arc::new(DesktopPuzzleServiceError::ShutdownDeadline));
            return;
        }
        if self.recovery.is_some()
            || self.storage_task.is_some()
            || self.opening.is_some()
            || !self.replies.is_empty()
        {
            return;
        }
        if let Some(failure) = self.worker_failure.clone() {
            self.fail_cleanup(failure);
            return;
        }
        if self.runtime_cleanup.is_some() {
            return;
        }
        if let Some(port) = &self.port {
            if port.is_shutdown_complete() {
                if self.worker.is_some() {
                    return;
                }
                if self
                    .retained
                    .as_ref()
                    .is_none_or(|runtime| !runtime.is_shutdown_complete())
                {
                    self.fail_cleanup(Arc::new(DesktopPuzzleServiceError::Incomplete));
                    return;
                }
            } else {
                self.ensure_runtime();
                let result = self
                    .port
                    .as_ref()
                    .ok_or_else(|| Arc::new(DesktopPuzzleServiceError::Incomplete))
                    .and_then(|port| {
                        port.request_shutdown(deadline)
                            .map_err(|error| Arc::new(DesktopPuzzleServiceError::Runtime(error)))
                    });
                match result {
                    Ok(receipt) => self.runtime_cleanup = Some(receipt),
                    Err(error) => self.fail_cleanup(error),
                }
                return;
            }
        }
        let Some(cleanup) = self.cleanup.take() else {
            return;
        };
        match cleanup {
            DesktopNativePuzzleServiceEnumCleanup::Replace { envelope, .. } => {
                if envelope.request.receipt.is_cancellation_requested() {
                    self.respond(
                        envelope.response,
                        Err(Arc::new(DesktopPuzzleServiceError::Cancelled)),
                    );
                    return;
                }
                self.clear_selected();
                self.begin_load(envelope);
            }
            DesktopNativePuzzleServiceEnumCleanup::Shutdown(shutdown) => {
                self.clear_selected();
                self.finish_shutdown();
                shutdown.response.send_replace(Some(Ok(())));
            }
        }
    }

    fn observe_runtime_cleanup(&mut self, result: DesktopPuzzleBridgeOutcome<()>) {
        self.runtime_cleanup = None;
        if let Err(error) = result {
            let failure = Arc::new(DesktopPuzzleServiceError::Runtime(error));
            self.record_failure(Arc::clone(&failure));
            self.fail_cleanup(failure);
        }
    }

    fn fail_cleanup(&mut self, failure: Arc<DesktopPuzzleServiceError>) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup.refuse(Arc::clone(&failure));
        }
        self.activity = if self.closing {
            DesktopPuzzleServiceEnumActivity::Closing
        } else if self.port.is_some() {
            DesktopPuzzleServiceEnumActivity::Selected
        } else {
            DesktopPuzzleServiceEnumActivity::Idle
        };
        self.record_failure(failure);
    }

    fn clear_selected(&mut self) {
        self.retained = None;
        self.port = None;
        let (_, snapshots) = watch::channel(None);
        let (_, health) = watch::channel(DesktopPuzzleBridgeEnumHealth::Starting);
        self.snapshots = snapshots;
        self.health = health;
    }

    fn finish_shutdown(&mut self) {
        self.ordinary.close();
        self.urgent.close();
        self.interruptions.close();
        self.shutdowns.close();
        while let Ok(envelope) = self.ordinary.try_recv() {
            envelope
                .response
                .send_replace(Some(Err(Arc::new(DesktopPuzzleServiceError::Closing))));
        }
        while let Ok(envelope) = self.urgent.try_recv() {
            envelope
                .response
                .send_replace(Some(Err(Arc::new(DesktopPuzzleServiceError::Closing))));
        }
        while let Ok(interruption) = self.interruptions.try_recv() {
            interruption
                .response
                .send_replace(Some(Err(Arc::new(DesktopPuzzleServiceError::Closing))));
        }
        while let Ok(shutdown) = self.shutdowns.try_recv() {
            shutdown
                .response
                .send_replace(Some(Err(Arc::new(DesktopPuzzleServiceError::Closing))));
        }
        self.activity = DesktopPuzzleServiceEnumActivity::ShutdownComplete;
        self.complete.store(true, Ordering::Release);
        self.publish();
    }

    fn is_durable_and_hidden(snapshot: &DesktopPuzzleSnapshot) -> bool {
        matches!(
            snapshot.status(),
            PuzzleRunEnumStatus::Paused { .. } | PuzzleRunEnumStatus::Ended(_)
        ) && snapshot.attempt().is_none()
            && !snapshot.has_retained_write()
            && !snapshot.has_unsaved_state()
            && snapshot.durable_context() == Some(snapshot.context())
            && snapshot.durable_elapsed() == Some(snapshot.timer().elapsed())
    }

    fn record_failure(&mut self, failure: Arc<DesktopPuzzleServiceError>) {
        self.failure = Some(failure);
        self.publish();
    }

    fn publish(&mut self) {
        let health = self
            .port
            .as_ref()
            .map(|_port| self.health.borrow_and_update().clone());
        self.state.send_replace(
            DesktopPuzzleServiceState::new(
                self.port
                    .as_ref()
                    .map(DesktopNativePuzzleRuntimePort::session_id),
                self.snapshots.borrow().clone(),
                if self.closing
                    && self.activity != DesktopPuzzleServiceEnumActivity::ShutdownComplete
                {
                    DesktopPuzzleServiceEnumActivity::Closing
                } else {
                    self.activity
                },
                health,
                self.failure.clone(),
            )
            .with_native_admission_progress(self.native_admission_progress.clone()),
        );
        if let Err(error) = self.options.wake.wake() {
            self.failure = Some(Arc::new(DesktopPuzzleServiceError::Wake(error)));
            let previous = self.state.borrow().clone();
            self.state.send_replace(
                DesktopPuzzleServiceState::new(
                    previous.selected(),
                    previous.snapshot().cloned(),
                    previous.activity(),
                    previous.health().cloned(),
                    self.failure.clone(),
                )
                .with_native_admission_progress(self.native_admission_progress.clone()),
            );
        }
    }
}

type DesktopNativePuzzleServiceOwnerOutcome<C, W, S> = (
    Box<DesktopNativePuzzleServiceOwner<C, W, S>>,
    DesktopPuzzleServiceOutcome<()>,
);

struct DesktopNativePuzzleServiceCleanup {
    deadline: Instant,
    receipt: DesktopPuzzleServiceReceipt<()>,
    expired: bool,
}

pub struct DesktopNativePuzzleService<C, W, S> {
    runtime: Handle,
    startup: Option<Box<DesktopNativePuzzleServiceOwner<C, W, S>>>,
    worker: Option<JoinHandle<DesktopNativePuzzleServiceOwnerOutcome<C, W, S>>>,
    retained: Option<Box<DesktopNativePuzzleServiceOwner<C, W, S>>>,
    port: DesktopNativePuzzleServicePort,
    cleanup: Option<DesktopNativePuzzleServiceCleanup>,
    worker_failure: Option<Arc<DesktopPuzzleServiceError>>,
    closing: bool,
}

impl<C, W, S> DesktopNativePuzzleService<C, W, S>
where
    C: ClockTimeProvider + 'static,
    W: DesktopApplicationWakeProvider + 'static,
    S: DesktopNativePuzzleRuntimeScheduler + Clone + 'static,
{
    pub fn new(options: DesktopNativePuzzleServiceOptions<C, W, S>) -> Self {
        let runtime = options.runtime.clone();
        let (owner, port) = DesktopNativePuzzleServiceOwner::new(options);
        Self {
            runtime,
            startup: Some(Box::new(owner)),
            worker: None,
            retained: None,
            port,
            cleanup: None,
            worker_failure: None,
            closing: false,
        }
    }

    pub fn start(&mut self) -> DesktopPuzzleServiceOutcome<DesktopNativePuzzleServicePort> {
        if self.closing {
            return Err(Arc::new(DesktopPuzzleServiceError::Closing));
        }
        let owner = self
            .startup
            .take()
            .ok_or_else(|| Arc::new(DesktopPuzzleServiceError::Started))?;
        self.spawn(owner);
        Ok(self.port.clone())
    }

    fn spawn(&mut self, mut owner: Box<DesktopNativePuzzleServiceOwner<C, W, S>>) {
        self.worker = Some(self.runtime.spawn(async move {
            let result = owner.run().await;
            (owner, result)
        }));
    }

    pub async fn run(&mut self) -> DesktopPuzzleServiceOutcome<()> {
        if self.startup.is_some() {
            self.start()?;
        }
        self.observe_worker(None).await
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.closing
            && self.startup.is_none()
            && self.worker.is_none()
            && self.worker_failure.is_none()
            && self.cleanup.is_none()
            && self.port.is_shutdown_complete()
            && self.retained.as_ref().is_some_and(|owner| {
                owner.complete.load(Ordering::Acquire)
                    && owner.worker.is_none()
                    && owner.retained.is_none()
                    && owner.recovery.is_none()
                    && owner.storage_task.is_none()
                    && owner.replies.is_empty()
                    && owner.runtime_cleanup.is_none()
            })
    }

    pub fn cleanup_deadline(&self) -> Option<Instant> {
        self.cleanup.as_ref().map(|cleanup| cleanup.deadline)
    }

    pub async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> DesktopPuzzleServiceOutcome<()> {
        if self.is_shutdown_complete() {
            return Ok(());
        }
        self.closing = true;
        if let Some(owner) = self.startup.take() {
            self.spawn(owner);
        }
        if let Some(failure) = &self.worker_failure {
            return Err(Arc::clone(failure));
        }
        if let Some(cleanup) = &mut self.cleanup
            && cleanup.expired
        {
            let observed = timeout_at(deadline, cleanup.receipt.wait())
                .await
                .map_err(|_elapsed| Arc::new(DesktopPuzzleServiceError::ShutdownDeadline))?;
            if let Err(failure) = observed {
                let previous = self.port.state.borrow().clone();
                if previous.failure().is_none() {
                    return Err(failure);
                }
            }
            self.cleanup = None;
        }
        if self.cleanup.is_none() {
            let receipt = self.port.request_shutdown(deadline)?;
            self.cleanup = Some(DesktopNativePuzzleServiceCleanup {
                deadline,
                receipt,
                expired: false,
            });
        }
        let cleanup = self
            .cleanup
            .as_mut()
            .ok_or_else(|| Arc::new(DesktopPuzzleServiceError::Incomplete))?;
        let original_deadline = cleanup.deadline;
        let result = timeout_at(original_deadline, cleanup.receipt.wait()).await;
        match result {
            Ok(result) => {
                self.cleanup = None;
                result?;
            }
            Err(_elapsed) => {
                if let Some(cleanup) = &mut self.cleanup {
                    cleanup.expired = true;
                }
                return Err(Arc::new(DesktopPuzzleServiceError::ShutdownDeadline));
            }
        }
        self.observe_worker(Some(original_deadline)).await?;
        if !self.is_shutdown_complete() {
            return Err(Arc::new(DesktopPuzzleServiceError::Incomplete));
        }
        Ok(())
    }

    async fn observe_worker(
        &mut self,
        deadline: Option<Instant>,
    ) -> DesktopPuzzleServiceOutcome<()> {
        let Some(worker) = &mut self.worker else {
            return self
                .worker_failure
                .as_ref()
                .map_or(Ok(()), |failure| Err(Arc::clone(failure)));
        };
        let joined = match deadline {
            Some(deadline) => timeout_at(deadline, worker)
                .await
                .map_err(|_elapsed| Arc::new(DesktopPuzzleServiceError::ShutdownDeadline))?,
            None => worker.await,
        };
        self.worker = None;
        match joined {
            Ok((owner, result)) => {
                self.retained = Some(owner);
                result
            }
            Err(error) => {
                let failure = Arc::new(DesktopPuzzleServiceError::Worker(Arc::new(error)));
                self.worker_failure = Some(Arc::clone(&failure));
                Err(failure)
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/desktop/native/puzzle_service.rs"]
mod tests;
