use std::future::{Future, pending};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, sleep_until, timeout_at};

use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::DesktopApplicationWakeProvider;
use crate::desktop::puzzle_bridge::{
    DesktopPuzzleBridgeEnumHealth, DesktopPuzzleBridgeEnumOperation, DesktopPuzzleBridgeEnumResult,
    DesktopPuzzleBridgeError, DesktopPuzzleBridgeOutcome, DesktopPuzzleBridgeOutcomeSender,
    DesktopPuzzleBridgeProvider, DesktopPuzzleBridgeReceipt, DesktopPuzzleBridgeRequest,
};
use crate::desktop::puzzle_commands::{DesktopPuzzleCommands, DesktopPuzzleCommandsError};
use crate::desktop::puzzle_snapshot::DesktopPuzzleSnapshot;
use crate::puzzle::checkpoint::PuzzleCheckpointEnumStatus;
use crate::puzzle::persistence::{PuzzlePersistenceEnumCommand, PuzzlePersistenceError};
use crate::puzzle::run::{PuzzleRunEnumStatus, PuzzleRunError};
use crate::puzzle::session::PuzzleSessionId;
use crate::puzzle::storage_repository::{PuzzleStorageDriverError, PuzzleStorageRepository};
use crate::puzzle::summary::PuzzleSummary;
use crate::puzzle::timer::PuzzleTimerEnumPauseReason;

pub trait DesktopNativePuzzleRuntimeScheduler: Send + Sync {
    fn wait_until(
        &self,
        deadline: ClockInstant,
    ) -> impl Future<Output = Result<(), ClockDurationError>> + Send;
}

#[derive(Clone)]
pub struct DesktopNativePuzzleRuntimeSchedulerTokio {
    runtime: Handle,
    clock_epoch: ClockInstant,
    runtime_epoch: Instant,
}

impl DesktopNativePuzzleRuntimeSchedulerTokio {
    pub const fn new(runtime: Handle, clock_epoch: ClockInstant, runtime_epoch: Instant) -> Self {
        Self {
            runtime,
            clock_epoch,
            runtime_epoch,
        }
    }
}

impl DesktopNativePuzzleRuntimeScheduler for DesktopNativePuzzleRuntimeSchedulerTokio {
    async fn wait_until(&self, deadline: ClockInstant) -> Result<(), ClockDurationError> {
        if deadline <= self.clock_epoch {
            return Ok(());
        }
        let deadline = self
            .runtime_epoch
            .checked_add(deadline.elapsed_since(self.clock_epoch)?.as_duration())
            .ok_or(ClockDurationError::Overflow)?;
        let timer = {
            let _entered = self.runtime.enter();
            sleep_until(deadline)
        };
        timer.await;
        Ok(())
    }
}

struct DesktopNativePuzzleRuntimeRequest {
    request: DesktopPuzzleBridgeRequest,
    response: DesktopPuzzleBridgeOutcomeSender<DesktopPuzzleBridgeEnumResult>,
}

struct DesktopNativePuzzleRuntimeInterruption {
    reason: PuzzleTimerEnumPauseReason,
    response: DesktopPuzzleBridgeOutcomeSender<DesktopPuzzleBridgeEnumResult>,
}

struct DesktopNativePuzzleRuntimeShutdownSlot(Arc<AtomicBool>);

impl Drop for DesktopNativePuzzleRuntimeShutdownSlot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct DesktopNativePuzzleRuntimeShutdown {
    deadline: Instant,
    response: DesktopPuzzleBridgeOutcomeSender<()>,
    slot: DesktopNativePuzzleRuntimeShutdownSlot,
}

#[derive(Clone)]
pub struct DesktopNativePuzzleRuntimePort {
    session: PuzzleSessionId,
    ordinary: mpsc::Sender<DesktopNativePuzzleRuntimeRequest>,
    urgent: mpsc::Sender<DesktopNativePuzzleRuntimeRequest>,
    interruptions: mpsc::Sender<DesktopNativePuzzleRuntimeInterruption>,
    shutdowns: mpsc::Sender<DesktopNativePuzzleRuntimeShutdown>,
    snapshots: watch::Receiver<Option<Arc<DesktopPuzzleSnapshot>>>,
    health: watch::Receiver<DesktopPuzzleBridgeEnumHealth>,
    shutdown_pending: Arc<AtomicBool>,
    complete: Arc<AtomicBool>,
}

impl DesktopNativePuzzleRuntimePort {
    pub const ORDINARY_CAPACITY: usize = 256;
    pub const URGENT_CAPACITY: usize = 2;
    const INTERRUPTION_CAPACITY: usize = 1;
    const SHUTDOWN_CAPACITY: usize = 1;

    pub const fn session_id(&self) -> PuzzleSessionId {
        self.session
    }

    pub fn interrupt(
        &self,
        reason: PuzzleTimerEnumPauseReason,
    ) -> DesktopPuzzleBridgeOutcome<DesktopPuzzleBridgeReceipt> {
        let (response, receipt) = DesktopPuzzleBridgeReceipt::channel();
        self.interruptions
            .try_send(DesktopNativePuzzleRuntimeInterruption { reason, response })
            .map_err(Self::admission_error)?;
        Ok(receipt)
    }

    pub fn request_shutdown(
        &self,
        deadline: Instant,
    ) -> DesktopPuzzleBridgeOutcome<DesktopPuzzleBridgeReceipt<()>> {
        let (response, receipt) = DesktopPuzzleBridgeReceipt::channel();
        if self.is_shutdown_complete() {
            response.send_replace(Some(Ok(())));
            return Ok(receipt);
        }
        self.shutdown_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_pending| Arc::new(DesktopPuzzleBridgeError::Busy))?;
        let slot = DesktopNativePuzzleRuntimeShutdownSlot(Arc::clone(&self.shutdown_pending));
        self.shutdowns
            .try_send(DesktopNativePuzzleRuntimeShutdown {
                deadline,
                response,
                slot,
            })
            .map_err(Self::admission_error)?;
        Ok(receipt)
    }

    pub async fn shutdown(&self, deadline: Instant) -> DesktopPuzzleBridgeOutcome<()> {
        let mut receipt = self.request_shutdown(deadline)?;
        timeout_at(deadline, receipt.wait())
            .await
            .map_err(|_elapsed| Arc::new(DesktopPuzzleBridgeError::ShutdownDeadline))?
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.complete.load(Ordering::Acquire)
    }

    fn admission_error<T>(error: mpsc::error::TrySendError<T>) -> Arc<DesktopPuzzleBridgeError> {
        Arc::new(match error {
            mpsc::error::TrySendError::Full(_) => DesktopPuzzleBridgeError::Busy,
            mpsc::error::TrySendError::Closed(_) => DesktopPuzzleBridgeError::Closed,
        })
    }
}

impl DesktopPuzzleBridgeProvider for DesktopNativePuzzleRuntimePort {
    fn session_id(&self) -> PuzzleSessionId {
        self.session_id()
    }

    fn submit(
        &self,
        request: DesktopPuzzleBridgeRequest,
    ) -> DesktopPuzzleBridgeOutcome<DesktopPuzzleBridgeReceipt> {
        let (response, receipt) = DesktopPuzzleBridgeReceipt::channel();
        let queue = if request.operation.is_urgent() {
            &self.urgent
        } else {
            &self.ordinary
        };
        queue
            .try_send(DesktopNativePuzzleRuntimeRequest { request, response })
            .map_err(Self::admission_error)?;
        Ok(receipt)
    }

    fn snapshots(&self) -> watch::Receiver<Option<Arc<DesktopPuzzleSnapshot>>> {
        self.snapshots.clone()
    }

    fn health(&self) -> watch::Receiver<DesktopPuzzleBridgeEnumHealth> {
        self.health.clone()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopNativePuzzleRuntimeEnumPhase {
    Accepting,
    Closing,
    Stopped,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopNativePuzzleRuntimeEnumStoragePhase {
    Idle,
    Waiting,
    SettlementBlocked,
}

struct DesktopNativePuzzleRuntimePublication {
    snapshot: Arc<DesktopPuzzleSnapshot>,
    refusal: Option<Arc<DesktopPuzzleBridgeError>>,
}

impl DesktopNativePuzzleRuntimePublication {
    fn into_snapshot(self) -> DesktopPuzzleBridgeOutcome<Arc<DesktopPuzzleSnapshot>> {
        self.refusal.map_or(Ok(self.snapshot), Err)
    }

    fn into_result(
        self,
        summary: Option<PuzzleSummary>,
    ) -> DesktopPuzzleBridgeOutcome<DesktopPuzzleBridgeEnumResult> {
        let snapshot = self.into_snapshot()?;
        Ok(match summary {
            Some(summary) => DesktopPuzzleBridgeEnumResult::Summary { snapshot, summary },
            None => DesktopPuzzleBridgeEnumResult::Snapshot(snapshot),
        })
    }
}

pub struct DesktopNativePuzzleRuntime<R: PuzzleStorageRepository, C, W, S> {
    owner: DesktopPuzzleCommands<R>,
    clock: Arc<C>,
    wake: Arc<W>,
    scheduler: S,
    ordinary: mpsc::Receiver<DesktopNativePuzzleRuntimeRequest>,
    urgent: mpsc::Receiver<DesktopNativePuzzleRuntimeRequest>,
    interruptions: mpsc::Receiver<DesktopNativePuzzleRuntimeInterruption>,
    shutdowns: mpsc::Receiver<DesktopNativePuzzleRuntimeShutdown>,
    pending_shutdown: Option<DesktopNativePuzzleRuntimeShutdown>,
    shutdown_deadline: Option<Instant>,
    snapshots: watch::Sender<Option<Arc<DesktopPuzzleSnapshot>>>,
    health: watch::Sender<DesktopPuzzleBridgeEnumHealth>,
    failure: Option<Arc<DesktopPuzzleBridgeError>>,
    phase: DesktopNativePuzzleRuntimeEnumPhase,
    storage_phase: DesktopNativePuzzleRuntimeEnumStoragePhase,
    complete: Arc<AtomicBool>,
    started: bool,
}

impl<R, C, W, S> DesktopNativePuzzleRuntime<R, C, W, S>
where
    R: PuzzleStorageRepository,
    C: ClockTimeProvider,
    W: DesktopApplicationWakeProvider,
    S: DesktopNativePuzzleRuntimeScheduler,
{
    pub fn new(
        owner: DesktopPuzzleCommands<R>,
        clock: Arc<C>,
        wake: Arc<W>,
        scheduler: S,
    ) -> (Self, DesktopNativePuzzleRuntimePort) {
        let session = owner.context().session_id();
        let storage_phase = if owner.has_retained_write() {
            DesktopNativePuzzleRuntimeEnumStoragePhase::Waiting
        } else {
            DesktopNativePuzzleRuntimeEnumStoragePhase::Idle
        };
        let (ordinary, ordinary_receiver) =
            mpsc::channel(DesktopNativePuzzleRuntimePort::ORDINARY_CAPACITY);
        let (urgent, urgent_receiver) =
            mpsc::channel(DesktopNativePuzzleRuntimePort::URGENT_CAPACITY);
        let (interruptions, interruption_receiver) =
            mpsc::channel(DesktopNativePuzzleRuntimePort::INTERRUPTION_CAPACITY);
        let (shutdowns, shutdown_receiver) =
            mpsc::channel(DesktopNativePuzzleRuntimePort::SHUTDOWN_CAPACITY);
        let (snapshots, snapshot_receiver) = watch::channel(None);
        let (health, health_receiver) = watch::channel(DesktopPuzzleBridgeEnumHealth::Starting);
        let complete = Arc::new(AtomicBool::new(false));
        (
            Self {
                owner,
                clock,
                wake,
                scheduler,
                ordinary: ordinary_receiver,
                urgent: urgent_receiver,
                interruptions: interruption_receiver,
                shutdowns: shutdown_receiver,
                pending_shutdown: None,
                shutdown_deadline: None,
                snapshots,
                health,
                failure: None,
                phase: DesktopNativePuzzleRuntimeEnumPhase::Accepting,
                storage_phase,
                complete: Arc::clone(&complete),
                started: false,
            },
            DesktopNativePuzzleRuntimePort {
                session,
                ordinary,
                urgent,
                interruptions,
                shutdowns,
                snapshots: snapshot_receiver,
                health: health_receiver,
                shutdown_pending: Arc::new(AtomicBool::new(false)),
                complete,
            },
        )
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.phase == DesktopNativePuzzleRuntimeEnumPhase::Stopped
            && self.complete.load(Ordering::Acquire)
            && !self.owner.has_retained_write()
    }

    pub async fn run(&mut self) -> DesktopPuzzleBridgeOutcome<()> {
        let result = self.run_retained().await;
        if let Err(failure) = &result {
            self.record_failure(Arc::clone(failure));
        }
        result
    }

    async fn run_retained(&mut self) -> DesktopPuzzleBridgeOutcome<()> {
        loop {
            if self.is_shutdown_complete() {
                return Ok(());
            }
            if let Some(shutdown) = &self.pending_shutdown {
                let deadline = shutdown.deadline;
                if self.shutdown_retained(deadline).await.is_ok() {
                    return Ok(());
                }
                continue;
            }
            let deadline = self.owner.next_deadline().map_err(Self::command_error)?;
            let writing = self.storage_phase == DesktopNativePuzzleRuntimeEnumStoragePhase::Waiting;
            tokio::select! {
                biased;
                shutdown = self.shutdowns.recv() => {
                    self.pending_shutdown = Some(shutdown.ok_or_else(Self::closed)?);
                }
                interruption = self.interruptions.recv() => {
                    let interruption = interruption.ok_or_else(Self::closed)?;
                    let now = self.now()?;
                    let result = self.apply_interruption(interruption.reason, now);
                    let publication = self.publish_snapshot(now);
                    interruption.response.send_replace(Some(result.and_then(|()| publication?.into_result(None))));
                }
                request = self.urgent.recv() => {
                    self.dispatch(request.ok_or_else(Self::closed)?)?;
                }
                result = self.owner.wait(), if writing => {
                    result.map_err(Self::command_error)?;
                    let now = self.now()?;
                    if let Err(failure) = self.settle_observed(now) {
                        self.failure = Some(failure);
                    }
                    self.publish_snapshot(now)?;
                }
                request = self.ordinary.recv() => {
                    self.dispatch(request.ok_or_else(Self::closed)?)?;
                }
                () = async {}, if !self.started => {
                    self.started = true;
                    self.publish_snapshot(self.now()?)?;
                }
                result = Self::wait_deadline(&self.scheduler, deadline) => {
                    result.map_err(|error| Arc::new(DesktopPuzzleBridgeError::Clock(error)))?;
                    self.publish_snapshot(self.now()?)?;
                }
            }
        }
    }

    async fn wait_deadline(
        scheduler: &S,
        deadline: Option<ClockInstant>,
    ) -> Result<(), ClockDurationError> {
        match deadline {
            Some(deadline) => scheduler.wait_until(deadline).await,
            None => pending().await,
        }
    }

    fn dispatch(
        &mut self,
        envelope: DesktopNativePuzzleRuntimeRequest,
    ) -> DesktopPuzzleBridgeOutcome<()> {
        let request = envelope.request;
        let now = self.now()?;
        let result = if request.receipt.is_cancellation_requested() {
            Err(Arc::new(DesktopPuzzleBridgeError::Cancelled))
        } else if self.phase != DesktopNativePuzzleRuntimeEnumPhase::Accepting
            && !request.operation.is_available_while_closing()
        {
            Err(Arc::new(DesktopPuzzleBridgeError::Closing))
        } else if self.storage_phase
            == DesktopNativePuzzleRuntimeEnumStoragePhase::SettlementBlocked
            && !request.operation.is_available_while_closing()
        {
            Err(self.settlement_failure())
        } else {
            match request.operation {
                DesktopPuzzleBridgeEnumOperation::Command(command) => {
                    let result = self
                        .owner
                        .execute(&request.expected, command, now)
                        .map(|()| None);
                    self.observe_owner_result(result)
                }
                DesktopPuzzleBridgeEnumOperation::RetrySave => {
                    let result = self.owner.retry_save(&request.expected, now).map(|()| None);
                    self.observe_owner_result(result)
                }
                DesktopPuzzleBridgeEnumOperation::Snapshot => Ok(None),
                DesktopPuzzleBridgeEnumOperation::Summary => {
                    let result = self.owner.summary(now).map(Some);
                    self.observe_owner_result(result)
                }
            }
        };
        let publication = self.publish_snapshot(now);
        let publication_failure = publication.as_ref().err().map(Arc::clone);
        let result = result.and_then(|summary| publication?.into_result(summary));
        envelope.response.send_replace(Some(result));
        publication_failure.map_or(Ok(()), Err)
    }

    fn apply_interruption(
        &mut self,
        reason: PuzzleTimerEnumPauseReason,
        now: ClockInstant,
    ) -> DesktopPuzzleBridgeOutcome<()> {
        let context = self.owner.context();
        let result = self
            .owner
            .execute(&context, PuzzlePersistenceEnumCommand::Pause(reason), now);
        let result = match result {
            Err(DesktopPuzzleCommandsError::StaleContext { .. }) => self.owner.execute(
                &self.owner.context(),
                PuzzlePersistenceEnumCommand::Pause(reason),
                now,
            ),
            result => result,
        };
        match result {
            Ok(())
            | Err(DesktopPuzzleCommandsError::Persistence(PuzzlePersistenceError::Run(
                PuzzleRunError::Ended(_),
            ))) => Ok(()),
            Err(error) => self.observe_owner_result(Err(error)),
        }
    }

    pub async fn shutdown_retained(&mut self, deadline: Instant) -> DesktopPuzzleBridgeOutcome<()> {
        if self.is_shutdown_complete() {
            return Ok(());
        }
        if self.pending_shutdown.is_none() {
            match self.shutdowns.try_recv() {
                Ok(shutdown) => self.pending_shutdown = Some(shutdown),
                Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {
                }
            }
        }
        self.phase = DesktopNativePuzzleRuntimeEnumPhase::Closing;
        let requested = self
            .pending_shutdown
            .as_ref()
            .map_or(deadline, |request| request.deadline.min(deadline));
        let deadline = self.shutdown_deadline.get_or_insert(requested);
        *deadline = (*deadline).min(requested);
        let deadline = *deadline;
        self.health
            .send_replace(DesktopPuzzleBridgeEnumHealth::ShuttingDown);
        let result = self.drain_retained(deadline).await;
        self.shutdown_deadline = None;
        if let Err(failure) = &result {
            self.record_failure(Arc::clone(failure));
        }
        if let Some(shutdown) = self.pending_shutdown.take() {
            drop(shutdown.slot);
            shutdown.response.send_replace(Some(result.clone()));
        }
        result
    }

    async fn drain_retained(&mut self, deadline: Instant) -> DesktopPuzzleBridgeOutcome<()> {
        let now = self.now()?;
        let interruption = self.apply_interruption(PuzzleTimerEnumPauseReason::WindowHidden, now);
        let snapshot = self.publish_snapshot(now)?.into_snapshot()?;
        interruption?;
        let mut snapshot = snapshot;
        loop {
            if Instant::now() >= deadline {
                return Err(Arc::new(DesktopPuzzleBridgeError::ShutdownDeadline));
            }
            if matches!(
                snapshot.checkpoint_status(),
                PuzzleCheckpointEnumStatus::RetryRequired { .. }
            ) {
                return Err(Arc::new(DesktopPuzzleBridgeError::SaveRequired(
                    snapshot.checkpoint_status(),
                )));
            }
            if self.storage_phase == DesktopNativePuzzleRuntimeEnumStoragePhase::SettlementBlocked {
                return Err(self.settlement_failure());
            }
            if Self::is_durable_and_hidden(&snapshot) {
                self.finish_shutdown();
                self.notify()?;
                return Ok(());
            }
            if self.storage_phase != DesktopNativePuzzleRuntimeEnumStoragePhase::Waiting {
                return Err(Arc::new(DesktopPuzzleBridgeError::Incomplete));
            }
            timeout_at(deadline, self.owner.wait())
                .await
                .map_err(|_elapsed| Arc::new(DesktopPuzzleBridgeError::ShutdownDeadline))?
                .map_err(Self::command_error)?;
            let now = self.now()?;
            let settlement = self.settle_observed(now);
            snapshot = self.publish_snapshot(now)?.into_snapshot()?;
            settlement?;
        }
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

    fn finish_shutdown(&mut self) {
        self.phase = DesktopNativePuzzleRuntimeEnumPhase::Stopped;
        self.complete.store(true, Ordering::Release);
        self.ordinary.close();
        self.urgent.close();
        self.interruptions.close();
        self.shutdowns.close();
        while let Ok(request) = self.ordinary.try_recv() {
            request
                .response
                .send_replace(Some(Err(Arc::new(DesktopPuzzleBridgeError::Closing))));
        }
        while let Ok(request) = self.urgent.try_recv() {
            request
                .response
                .send_replace(Some(Err(Arc::new(DesktopPuzzleBridgeError::Closing))));
        }
        while let Ok(interruption) = self.interruptions.try_recv() {
            interruption
                .response
                .send_replace(Some(Err(Arc::new(DesktopPuzzleBridgeError::Closing))));
        }
        while let Ok(shutdown) = self.shutdowns.try_recv() {
            drop(shutdown.slot);
            let result = if Instant::now() >= shutdown.deadline {
                Err(Arc::new(DesktopPuzzleBridgeError::ShutdownDeadline))
            } else {
                Ok(())
            };
            shutdown.response.send_replace(Some(result));
        }
        self.health
            .send_replace(DesktopPuzzleBridgeEnumHealth::ShutdownComplete);
    }

    fn publish_snapshot(
        &mut self,
        now: ClockInstant,
    ) -> DesktopPuzzleBridgeOutcome<DesktopNativePuzzleRuntimePublication> {
        let (snapshot, refusal) = match self.owner.snapshot(now) {
            Ok(snapshot) => (snapshot, None),
            Err(error) if Self::is_repository_failure(&error) => {
                let failure = Self::command_error(error);
                self.failure = Some(Arc::clone(&failure));
                (
                    self.owner.snapshot(now).map_err(Self::command_error)?,
                    Some(failure),
                )
            }
            Err(error) => return Err(Self::command_error(error)),
        };
        self.synchronize_storage_phase();
        let snapshot = Arc::new(snapshot);
        let status = snapshot.checkpoint_status();
        let settlement_blocked =
            self.storage_phase == DesktopNativePuzzleRuntimeEnumStoragePhase::SettlementBlocked;
        if status == PuzzleCheckpointEnumStatus::Open && !settlement_blocked {
            self.failure = None;
        }
        self.snapshots.send_replace(Some(Arc::clone(&snapshot)));
        let health = if settlement_blocked {
            DesktopPuzzleBridgeEnumHealth::Failed(self.settlement_failure())
        } else if status != PuzzleCheckpointEnumStatus::Open {
            DesktopPuzzleBridgeEnumHealth::SaveBlocked {
                status,
                failure: self.failure.clone(),
            }
        } else if self.phase == DesktopNativePuzzleRuntimeEnumPhase::Closing {
            DesktopPuzzleBridgeEnumHealth::ShuttingDown
        } else {
            DesktopPuzzleBridgeEnumHealth::Ready
        };
        self.health.send_replace(health);
        if let Err(failure) = self.notify() {
            self.record_failure(Arc::clone(&failure));
            return Err(failure);
        }
        Ok(DesktopNativePuzzleRuntimePublication { snapshot, refusal })
    }

    fn settle_observed(&mut self, now: ClockInstant) -> DesktopPuzzleBridgeOutcome<()> {
        let result = self.owner.settle(now).map_err(Self::command_error);
        self.storage_phase = DesktopNativePuzzleRuntimeEnumStoragePhase::Idle;
        if let Err(failure) = &result {
            self.failure = Some(Arc::clone(failure));
            if self.owner.has_retained_write() {
                self.storage_phase = DesktopNativePuzzleRuntimeEnumStoragePhase::SettlementBlocked;
                self.apply_interruption(PuzzleTimerEnumPauseReason::SaveBackpressure, now)?;
            }
        }
        self.synchronize_storage_phase();
        result
    }

    fn synchronize_storage_phase(&mut self) {
        if self.storage_phase != DesktopNativePuzzleRuntimeEnumStoragePhase::SettlementBlocked {
            self.storage_phase = if self.owner.has_retained_write() {
                DesktopNativePuzzleRuntimeEnumStoragePhase::Waiting
            } else {
                DesktopNativePuzzleRuntimeEnumStoragePhase::Idle
            };
        }
    }

    fn observe_owner_result<T>(
        &mut self,
        result: Result<T, DesktopPuzzleCommandsError<R::Error>>,
    ) -> DesktopPuzzleBridgeOutcome<T> {
        result.map_err(|error| {
            let repository_failure = Self::is_repository_failure(&error);
            let failure = Self::command_error(error);
            if repository_failure {
                self.failure = Some(Arc::clone(&failure));
            }
            failure
        })
    }

    fn is_repository_failure(error: &DesktopPuzzleCommandsError<R::Error>) -> bool {
        matches!(
            error,
            DesktopPuzzleCommandsError::Storage(PuzzleStorageDriverError::Repository(_))
        )
    }

    fn settlement_failure(&self) -> Arc<DesktopPuzzleBridgeError> {
        self.failure.as_ref().map_or_else(
            || Arc::new(DesktopPuzzleBridgeError::Incomplete),
            Arc::clone,
        )
    }

    fn record_failure(&mut self, failure: Arc<DesktopPuzzleBridgeError>) {
        self.failure = Some(Arc::clone(&failure));
        self.health
            .send_replace(DesktopPuzzleBridgeEnumHealth::Failed(failure));
    }

    fn notify(&self) -> DesktopPuzzleBridgeOutcome<()> {
        self.wake
            .wake()
            .map_err(|error| Arc::new(DesktopPuzzleBridgeError::Wake(error)))
    }

    fn now(&self) -> DesktopPuzzleBridgeOutcome<ClockInstant> {
        self.clock
            .now()
            .map_err(|error| Arc::new(DesktopPuzzleBridgeError::Clock(error)))
    }

    fn closed() -> Arc<DesktopPuzzleBridgeError> {
        Arc::new(DesktopPuzzleBridgeError::Closed)
    }

    fn command_error(error: DesktopPuzzleCommandsError<R::Error>) -> Arc<DesktopPuzzleBridgeError> {
        Arc::new(DesktopPuzzleBridgeError::Commands(Box::new(error)))
    }
}

#[cfg(test)]
#[path = "../../../tests/desktop/native/puzzle_runtime.rs"]
mod tests;
