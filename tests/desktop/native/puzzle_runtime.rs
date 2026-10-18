use std::future::Future;
use std::num::NonZeroU64;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use thiserror::Error;
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::time::Instant;

use crate::board::position::BoardPosition;
use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationReceipt, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider,
};
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand};
use crate::desktop::native::lifecycle::{
    DesktopNativeLifecycle, DesktopNativeLifecycleEnumSignal,
    DesktopNativeLifecycleEnumWindowEvent, DesktopNativeLifecyclePuzzleWindowId,
};
use crate::desktop::native::puzzle_runtime::{
    DesktopNativePuzzleRuntime, DesktopNativePuzzleRuntimePort,
    DesktopNativePuzzleRuntimeScheduler, DesktopNativePuzzleRuntimeSchedulerTokio,
};
use crate::desktop::puzzle_bridge::{
    DesktopPuzzleBridgeEnumHealth, DesktopPuzzleBridgeEnumOperation, DesktopPuzzleBridgeEnumResult,
    DesktopPuzzleBridgeError, DesktopPuzzleBridgeProvider, DesktopPuzzleBridgeReceipt,
    DesktopPuzzleBridgeRequest,
};
use crate::desktop::puzzle_commands::DesktopPuzzleCommands;
use crate::desktop::puzzle_snapshot::DesktopPuzzleSnapshot;
use crate::puzzle::checkpoint::{PuzzleCheckpointEnumFreezeCause, PuzzleCheckpointEnumStatus};
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumPhase, PuzzleEnumPrimaryOutcome,
};
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::metadata::PuzzleDefinitionMetadata;
use crate::puzzle::persistence::{
    PuzzlePersistenceCodec, PuzzlePersistenceEnumCommand, PuzzlePersistenceReceipt,
    PuzzlePersistenceRun, PuzzlePersistenceSaveRequest, PuzzlePersistenceStorageRevision,
};
use crate::puzzle::random_provider::PuzzleRandomProviderSeeded;
use crate::puzzle::run::{PuzzleRunEnumConfirmation, PuzzleRunEnumEndReason, PuzzleRunEnumStatus};
use crate::puzzle::selection::PuzzleSelectionFilter;
use crate::puzzle::session::{PuzzleSessionConfig, PuzzleSessionId};
use crate::puzzle::storage_repository::{PuzzleStorageRepository, PuzzleStorageWrite};
use crate::puzzle::timer::{
    PuzzleTimerEnumPauseReason, PuzzleTimerEnumPhase, PuzzleTimerEnumReadiness,
};

type DesktopNativePuzzleRuntimeTestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Default)]
struct DesktopNativePuzzleRuntimeFixtureClock {
    nanos: AtomicU64,
    changed: Notify,
}

impl DesktopNativePuzzleRuntimeFixtureClock {
    fn set(&self, millis: u64) -> Result<(), ClockDurationError> {
        self.nanos.store(
            ClockDuration::from_millis(millis)?.as_nanos(),
            Ordering::Release,
        );
        self.changed.notify_waiters();
        Ok(())
    }
}

impl ClockTimeProvider for DesktopNativePuzzleRuntimeFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.nanos.load(Ordering::Acquire)))
    }
}

struct DesktopNativePuzzleRuntimeFixtureWake {
    calls: AtomicUsize,
    remaining: AtomicUsize,
    monitor_blocked: AtomicBool,
    observed_ready: AtomicBool,
    health: OnceLock<watch::Receiver<DesktopPuzzleBridgeEnumHealth>>,
}

impl Default for DesktopNativePuzzleRuntimeFixtureWake {
    fn default() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            remaining: AtomicUsize::new(usize::MAX),
            monitor_blocked: AtomicBool::new(false),
            observed_ready: AtomicBool::new(false),
            health: OnceLock::new(),
        }
    }
}

impl DesktopNativePuzzleRuntimeFixtureWake {
    const BLOCKED_PUBLICATION_BUDGET: usize = 8;

    fn probe_blocked(&self) {
        self.observed_ready.store(false, Ordering::Release);
        self.monitor_blocked.store(true, Ordering::Release);
        self.remaining
            .store(Self::BLOCKED_PUBLICATION_BUDGET, Ordering::Release);
    }

    fn finish_probe(&self) {
        self.monitor_blocked.store(false, Ordering::Release);
        self.remaining.store(usize::MAX, Ordering::Release);
    }
}

impl DesktopApplicationWakeProvider for DesktopNativePuzzleRuntimeFixtureWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.monitor_blocked.load(Ordering::Acquire)
            && self.health.get().is_some_and(|health| {
                matches!(*health.borrow(), DesktopPuzzleBridgeEnumHealth::Ready)
            })
        {
            self.observed_ready.store(true, Ordering::Release);
        }
        self.remaining
            .try_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                remaining.checked_sub(1)
            })
            .map_err(|_remaining| DesktopApplicationWakeError::Unavailable)?;
        Ok(())
    }
}

struct DesktopNativePuzzleRuntimeFixtureScheduler {
    clock: Arc<DesktopNativePuzzleRuntimeFixtureClock>,
    deadline: watch::Sender<Option<ClockInstant>>,
}

impl DesktopNativePuzzleRuntimeScheduler for DesktopNativePuzzleRuntimeFixtureScheduler {
    async fn wait_until(&self, deadline: ClockInstant) -> Result<(), ClockDurationError> {
        self.deadline.send_replace(Some(deadline));
        loop {
            let changed = self.clock.changed.notified();
            if self.clock.now()? >= deadline {
                return Ok(());
            }
            changed.await;
        }
    }
}

#[derive(Debug, Error)]
enum DesktopNativePuzzleRuntimeFixtureError {
    #[error("The controlled puzzle storage queue refused admission")]
    Admission,
    #[error("The controlled puzzle storage write failed")]
    Write,
    #[error("The controlled puzzle storage receipt was abandoned")]
    Abandoned,
    #[error("The controlled puzzle storage response closed: {0}")]
    Receive(#[from] oneshot::error::RecvError),
    #[error("The controlled puzzle storage revision overflowed")]
    Revision,
}

struct DesktopNativePuzzleRuntimeFixtureSave {
    request: PuzzlePersistenceSaveRequest,
    response:
        oneshot::Sender<Result<PuzzlePersistenceReceipt, DesktopNativePuzzleRuntimeFixtureError>>,
}

impl DesktopNativePuzzleRuntimeFixtureSave {
    fn complete(self, succeeds: bool) -> Result<(), DesktopNativePuzzleRuntimeFixtureError> {
        let Self { request, response } = self;
        let result = if succeeds {
            let revision = request
                .expected_storage_revision()
                .value()
                .checked_add(1)
                .ok_or(DesktopNativePuzzleRuntimeFixtureError::Revision)?;
            Ok(PuzzlePersistenceReceipt::committed(
                request,
                PuzzlePersistenceStorageRevision::new(revision),
            ))
        } else {
            Err(DesktopNativePuzzleRuntimeFixtureError::Write)
        };
        Self::send(response, result)
    }

    fn respond(
        self,
        result: Result<PuzzlePersistenceReceipt, DesktopNativePuzzleRuntimeFixtureError>,
    ) -> Result<(), DesktopNativePuzzleRuntimeFixtureError> {
        Self::send(self.response, result)
    }

    fn send(
        response: oneshot::Sender<
            Result<PuzzlePersistenceReceipt, DesktopNativePuzzleRuntimeFixtureError>,
        >,
        result: Result<PuzzlePersistenceReceipt, DesktopNativePuzzleRuntimeFixtureError>,
    ) -> Result<(), DesktopNativePuzzleRuntimeFixtureError> {
        response
            .send(result)
            .map_err(|_result| DesktopNativePuzzleRuntimeFixtureError::Abandoned)
    }
}

struct DesktopNativePuzzleRuntimeFixtureWrite {
    response:
        oneshot::Receiver<Result<PuzzlePersistenceReceipt, DesktopNativePuzzleRuntimeFixtureError>>,
}

impl PuzzleStorageWrite for DesktopNativePuzzleRuntimeFixtureWrite {
    type Error = DesktopNativePuzzleRuntimeFixtureError;

    async fn wait(&mut self) -> Result<PuzzlePersistenceReceipt, Self::Error> {
        (&mut self.response).await?
    }
}

struct DesktopNativePuzzleRuntimeFixtureRepository {
    saves: mpsc::Sender<DesktopNativePuzzleRuntimeFixtureSave>,
    begun: Arc<AtomicUsize>,
    accepting: Arc<AtomicBool>,
}

impl PuzzleStorageRepository for DesktopNativePuzzleRuntimeFixtureRepository {
    type Error = DesktopNativePuzzleRuntimeFixtureError;
    type Write = DesktopNativePuzzleRuntimeFixtureWrite;

    fn begin_save(
        &self,
        request: PuzzlePersistenceSaveRequest,
    ) -> Result<Self::Write, Self::Error> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err(DesktopNativePuzzleRuntimeFixtureError::Admission);
        }
        let (response, receiver) = oneshot::channel();
        self.saves
            .try_send(DesktopNativePuzzleRuntimeFixtureSave { request, response })
            .map_err(|_error| DesktopNativePuzzleRuntimeFixtureError::Admission)?;
        self.begun.fetch_add(1, Ordering::Relaxed);
        Ok(DesktopNativePuzzleRuntimeFixtureWrite { response: receiver })
    }
}

type DesktopNativePuzzleRuntimeFixtureActor = DesktopNativePuzzleRuntime<
    DesktopNativePuzzleRuntimeFixtureRepository,
    DesktopNativePuzzleRuntimeFixtureClock,
    DesktopNativePuzzleRuntimeFixtureWake,
    DesktopNativePuzzleRuntimeFixtureScheduler,
>;

struct DesktopNativePuzzleRuntimeFixture {
    actor: DesktopNativePuzzleRuntimeFixtureActor,
    port: DesktopNativePuzzleRuntimePort,
    clock: Arc<DesktopNativePuzzleRuntimeFixtureClock>,
    wake: Arc<DesktopNativePuzzleRuntimeFixtureWake>,
    receipt: DesktopApplicationReceipt,
    saves: mpsc::Receiver<DesktopNativePuzzleRuntimeFixtureSave>,
    begun: Arc<AtomicUsize>,
    accepting: Arc<AtomicBool>,
    deadline: watch::Receiver<Option<ClockInstant>>,
}

impl DesktopNativePuzzleRuntimeFixture {
    fn run(
        mode: PuzzleEnumCountdownMode,
        duration: u64,
    ) -> Result<PuzzlePersistenceRun, Box<dyn std::error::Error>> {
        let position = BoardPosition::standard();
        let first = position.parse_uci("e2e4")?;
        let after_first = position.play(first)?;
        let second = after_first.parse_uci("e7e5")?;
        let third = after_first.play(second)?.parse_uci("g1f3")?;
        let definitions = ["first", "second"]
            .into_iter()
            .map(|id| {
                Ok(PuzzleDefinition::new(
                    PuzzleDefinitionIdentity::new("native-runtime".into(), "v1".into(), id.into())?,
                    position.clone(),
                    vec![vec![first, second, third]],
                    PuzzleDefinitionMetadata::default(),
                )?)
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        Ok(PuzzlePersistenceRun::new(
            PuzzleSessionId::new(NonZeroU64::MIN),
            PuzzleSessionConfig::new(mode, Duration::from_millis(duration))?,
            definitions,
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(81),
        )?)
    }

    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Self::from_run(Self::run(PuzzleEnumCountdownMode::Session, 60_000)?)
    }

    fn from_run(run: PuzzlePersistenceRun) -> Result<Self, Box<dyn std::error::Error>> {
        let clock = Arc::new(DesktopNativePuzzleRuntimeFixtureClock::default());
        let wake = Arc::new(DesktopNativePuzzleRuntimeFixtureWake::default());
        let application = DesktopApplication::new(Arc::clone(&clock), Arc::clone(&wake));
        let app_port = application.port();
        let request = app_port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
        let receipt = app_port.try_submit(&request)?;
        let (sender, saves) = mpsc::channel(1);
        let begun = Arc::new(AtomicUsize::new(0));
        let accepting = Arc::new(AtomicBool::new(true));
        let owner = DesktopPuzzleCommands::new(
            run,
            DesktopNativePuzzleRuntimeFixtureRepository {
                saves: sender,
                begun: Arc::clone(&begun),
                accepting: Arc::clone(&accepting),
            },
            clock.now()?,
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?;
        let (deadlines, deadline) = watch::channel(None);
        let (actor, port) = DesktopNativePuzzleRuntime::new(
            owner,
            Arc::clone(&clock),
            Arc::clone(&wake),
            DesktopNativePuzzleRuntimeFixtureScheduler {
                clock: Arc::clone(&clock),
                deadline: deadlines,
            },
        );
        wake.health
            .set(port.health())
            .map_err(|_health| "The fixture health observer was already bound")?;
        Ok(Self {
            actor,
            port,
            clock,
            wake,
            receipt,
            saves,
            begun,
            accepting,
            deadline,
        })
    }

    fn drive(&mut self) -> DesktopNativePuzzleRuntimeTestResult {
        match Self::poll_future(self.actor.run()) {
            Poll::Pending | Poll::Ready(Ok(())) => Ok(()),
            Poll::Ready(Err(error)) => Err(error.into()),
        }
    }

    fn poll_future<F: Future>(future: F) -> Poll<F::Output> {
        let mut future = Box::pin(tokio::task::unconstrained(future));
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    }

    fn completed<T: Clone>(
        receipt: &mut DesktopPuzzleBridgeReceipt<T>,
    ) -> Result<Result<T, Arc<DesktopPuzzleBridgeError>>, Box<dyn std::error::Error>> {
        match Self::poll_future(receipt.wait()) {
            Poll::Ready(outcome) => Ok(outcome),
            Poll::Pending => Err(
                "The actor did not service the retained receipt within the controlled poll".into(),
            ),
        }
    }

    fn assert_save_blocked(&self) -> DesktopNativePuzzleRuntimeTestResult {
        assert!(matches!(
            &*self.port.health().borrow(),
            DesktopPuzzleBridgeEnumHealth::SaveBlocked {
                status: PuzzleCheckpointEnumStatus::RetryRequired { .. },
                failure: Some(_),
            }
        ));
        assert!(self.snapshot()?.attempt().is_none());
        assert!(!self.port.is_shutdown_complete());
        Ok(())
    }

    fn snapshot(&self) -> Result<Arc<DesktopPuzzleSnapshot>, Box<dyn std::error::Error>> {
        self.port
            .snapshots()
            .borrow()
            .clone()
            .ok_or_else(|| "Missing authoritative puzzle snapshot".into())
    }

    fn submit(
        &self,
        operation: DesktopPuzzleBridgeEnumOperation,
    ) -> Result<DesktopPuzzleBridgeReceipt, Box<dyn std::error::Error>> {
        Ok(self.port.submit(DesktopPuzzleBridgeRequest {
            receipt: self.receipt.clone(),
            expected: self.snapshot()?.context().clone(),
            operation,
        })?)
    }

    fn fill_ordinary(&self) -> Result<Vec<DesktopPuzzleBridgeReceipt>, Box<dyn std::error::Error>> {
        (0..DesktopNativePuzzleRuntimePort::ORDINARY_CAPACITY)
            .map(|_index| self.submit(DesktopPuzzleBridgeEnumOperation::Snapshot))
            .collect()
    }

    async fn execute(
        &mut self,
        operation: DesktopPuzzleBridgeEnumOperation,
    ) -> Result<DesktopPuzzleBridgeEnumResult, Box<dyn std::error::Error>> {
        let mut receipt = self.submit(operation)?;
        self.drive()?;
        Ok(receipt.wait().await?)
    }

    async fn command(
        &mut self,
        command: PuzzlePersistenceEnumCommand,
    ) -> Result<Arc<DesktopPuzzleSnapshot>, Box<dyn std::error::Error>> {
        Ok(Arc::clone(
            self.execute(DesktopPuzzleBridgeEnumOperation::Command(command))
                .await?
                .snapshot(),
        ))
    }

    fn commit(&mut self) -> DesktopNativePuzzleRuntimeTestResult {
        self.saves.try_recv()?.complete(true)?;
        self.drive()
    }

    fn drain(&mut self) -> DesktopNativePuzzleRuntimeTestResult {
        for _ in 0..8 {
            self.drive()?;
            if !self.actor.owner.has_retained_write() {
                return Ok(());
            }
            self.commit()?;
        }
        Err("The controlled puzzle actor did not drain retained writes".into())
    }

    async fn start(&mut self) -> DesktopNativePuzzleRuntimeTestResult {
        self.drive()?;
        self.drain()?;
        self.command(PuzzlePersistenceEnumCommand::Readiness(
            PuzzleTimerEnumReadiness::Ready,
        ))
        .await?;
        self.drain()
    }

    async fn shutdown(&mut self) -> DesktopNativePuzzleRuntimeTestResult {
        let mut receipt = self
            .port
            .request_shutdown(Instant::now() + Duration::from_secs(2))?;
        self.drive()?;
        self.drain()?;
        self.drive()?;
        receipt.wait().await?;
        assert!(self.actor.is_shutdown_complete());
        assert!(self.port.is_shutdown_complete());
        Ok(())
    }
}

#[tokio::test]
async fn inactive_initial_save_precedes_countdown_and_requires_fresh_readiness()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    assert!(fixture.port.snapshots().borrow().is_none());
    assert_eq!(
        fixture.port.session_id(),
        PuzzleSessionId::new(NonZeroU64::MIN)
    );
    fixture.drive()?;
    let mut denied = fixture.submit(DesktopPuzzleBridgeEnumOperation::Command(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
    ))?;
    fixture.drive()?;
    assert!(denied.wait().await.is_err());
    fixture.clock.set(9_000)?;
    fixture.drive()?;
    assert_eq!(fixture.snapshot()?.timer().elapsed(), ClockDuration::ZERO);
    fixture.commit()?;
    assert_eq!(
        fixture.snapshot()?.status(),
        PuzzleRunEnumStatus::AwaitingReadiness
    );
    fixture
        .command(PuzzlePersistenceEnumCommand::Readiness(
            PuzzleTimerEnumReadiness::Ready,
        ))
        .await?;
    assert_eq!(fixture.snapshot()?.status(), PuzzleRunEnumStatus::Active);
    assert_eq!(
        *fixture.deadline.borrow(),
        Some(ClockInstant::from_millis(9_250)?)
    );
    fixture.drain()?;
    fixture.shutdown().await
}

#[tokio::test]
async fn scheduler_uses_owner_cadence_with_one_write_and_latest_coalescing()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    let initial_writes = fixture.begun.load(Ordering::Relaxed);
    assert_eq!(
        *fixture.deadline.borrow(),
        Some(ClockInstant::from_millis(250)?)
    );
    fixture.clock.set(249)?;
    fixture.drive()?;
    assert_eq!(fixture.begun.load(Ordering::Relaxed), initial_writes);
    for millis in [250, 500, 750] {
        fixture.clock.set(millis)?;
        fixture.drive()?;
        assert_eq!(
            fixture.snapshot()?.timer().elapsed(),
            ClockDuration::from_millis(millis)?
        );
        assert_eq!(fixture.snapshot()?.status(), PuzzleRunEnumStatus::Active);
        assert!(matches!(
            *fixture.port.health().borrow(),
            DesktopPuzzleBridgeEnumHealth::Ready
        ));
    }
    assert_eq!(fixture.begun.load(Ordering::Relaxed), initial_writes + 1);
    let first = fixture.saves.try_recv()?;
    assert_eq!(
        first.request.token().elapsed_active(),
        ClockDuration::from_millis(250)?
    );
    first.complete(true)?;
    fixture.drive()?;
    let latest = fixture.saves.try_recv()?;
    assert_eq!(
        latest.request.token().elapsed_active(),
        ClockDuration::from_millis(750)?
    );
    latest.complete(true)?;
    fixture.drive()?;
    assert!(!fixture.snapshot()?.has_unsaved_state());
    fixture.shutdown().await
}

#[tokio::test]
async fn stalled_wake_freezes_at_historical_durable_boundary_and_never_auto_resumes()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    fixture.clock.set(9_000)?;
    fixture.drive()?;
    let snapshot = fixture.snapshot()?;
    assert!(snapshot.attempt().is_none());
    assert_eq!(
        snapshot.timer().elapsed(),
        ClockDuration::from_millis(1_000)?
    );
    assert!(
        matches!(snapshot.checkpoint_status(), PuzzleCheckpointEnumStatus::Frozen(freeze) if freeze.cause() == PuzzleCheckpointEnumFreezeCause::DurableLag && freeze.pause_at() == ClockInstant::from_millis(1_000)?)
    );
    fixture.drain()?;
    fixture.clock.set(19_000)?;
    fixture.drive()?;
    assert!(fixture.snapshot()?.attempt().is_none());
    assert_eq!(
        fixture.snapshot()?.timer().elapsed(),
        snapshot.timer().elapsed()
    );
    fixture
        .command(PuzzlePersistenceEnumCommand::Resume(
            PuzzleTimerEnumReadiness::Ready,
        ))
        .await?;
    assert_eq!(fixture.snapshot()?.status(), PuzzleRunEnumStatus::Active);
    fixture.drain()?;
    fixture.shutdown().await
}

#[tokio::test]
async fn late_committed_write_is_settled_with_fresh_time_before_it_can_erase_freeze()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    fixture.clock.set(250)?;
    fixture.drive()?;
    let save = fixture.saves.try_recv()?;
    save.complete(true)?;
    fixture.clock.set(9_000)?;
    fixture.drive()?;
    let snapshot = fixture.snapshot()?;
    assert_eq!(snapshot.captured_at(), ClockInstant::from_millis(9_000)?);
    assert_eq!(
        snapshot.timer().elapsed(),
        ClockDuration::from_millis(1_000)?
    );
    assert!(snapshot.attempt().is_none());
    assert_eq!(
        snapshot.durable_elapsed(),
        Some(ClockDuration::from_millis(250)?)
    );
    assert!(snapshot.has_retained_write());
    fixture.drain()?;
    assert!(fixture.snapshot()?.attempt().is_none());
    fixture.shutdown().await
}

#[tokio::test]
async fn exact_deadline_urgent_actions_execute_before_timer_or_ready_storage_receipt()
-> DesktopNativePuzzleRuntimeTestResult {
    for command in [
        PuzzlePersistenceEnumCommand::Pause(PuzzleTimerEnumPauseReason::FocusLost),
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Hidden),
        PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Confirmed),
    ] {
        let is_end = matches!(command, PuzzlePersistenceEnumCommand::End(_));
        let mut fixture = DesktopNativePuzzleRuntimeFixture::from_run(
            DesktopNativePuzzleRuntimeFixture::run(PuzzleEnumCountdownMode::PerPuzzle, 100)?,
        )?;
        fixture.drive()?;
        fixture.drain()?;
        fixture
            .command(PuzzlePersistenceEnumCommand::Readiness(
                PuzzleTimerEnumReadiness::Ready,
            ))
            .await?;
        fixture.saves.try_recv()?.complete(true)?;
        let mut receipt = fixture.submit(DesktopPuzzleBridgeEnumOperation::Command(command))?;
        fixture.clock.set(100)?;
        fixture.drive()?;
        let result = receipt.wait().await?;
        let snapshot = result.snapshot();
        assert!(snapshot.attempt().is_none());
        assert_eq!(snapshot.timer().elapsed(), ClockDuration::from_millis(100)?);
        if is_end {
            assert_eq!(
                snapshot.status(),
                PuzzleRunEnumStatus::Ended(PuzzleRunEnumEndReason::Confirmed)
            );
            assert_eq!(snapshot.archived_attempts(), 0);
        } else {
            assert!(matches!(
                snapshot.status(),
                PuzzleRunEnumStatus::Paused {
                    phase: PuzzleTimerEnumPhase::AwaitingReadiness,
                    ..
                }
            ));
            assert_eq!(snapshot.archived_attempts(), 1);
        }
        fixture.drain()?;
        fixture.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn native_interruption_uses_current_context_after_accepted_ordinary_move()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    let initial = fixture.snapshot()?;
    fixture
        .command(PuzzlePersistenceEnumCommand::Move(
            BoardPosition::standard().parse_uci("e2e4")?,
        ))
        .await?;
    assert_ne!(fixture.snapshot()?.context(), initial.context());
    let mut interrupted = fixture.port.interrupt(PuzzleTimerEnumPauseReason::Sleep)?;
    fixture.drive()?;
    assert!(interrupted.wait().await?.snapshot().attempt().is_none());
    fixture.drain()?;
    assert!(fixture.snapshot()?.attempt().is_none());
    fixture.shutdown().await
}

#[tokio::test]
async fn native_hidden_signal_at_exact_expiry_hides_the_next_board_without_resuming_on_focus()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::from_run(
        DesktopNativePuzzleRuntimeFixture::run(PuzzleEnumCountdownMode::PerPuzzle, 100)?,
    )?;
    fixture.start().await?;
    let window = DesktopNativeLifecyclePuzzleWindowId::new(NonZeroU64::MIN);
    let lifecycle = DesktopNativeLifecycle::new(window);
    let reason = lifecycle
        .puzzle_interruption(DesktopNativeLifecycleEnumSignal::Window {
            window,
            event: DesktopNativeLifecycleEnumWindowEvent::Hidden,
        })
        .ok_or("Missing hidden interruption")?;
    let mut receipt = fixture.port.interrupt(reason)?;
    fixture.clock.set(100)?;
    fixture.drive()?;
    assert!(receipt.wait().await?.snapshot().attempt().is_none());
    assert_eq!(fixture.snapshot()?.archived_attempts(), 1);
    fixture.drain()?;
    assert_eq!(
        lifecycle.puzzle_interruption(DesktopNativeLifecycleEnumSignal::Window {
            window,
            event: DesktopNativeLifecycleEnumWindowEvent::FocusGained
        }),
        None
    );
    fixture
        .command(PuzzlePersistenceEnumCommand::Readiness(
            PuzzleTimerEnumReadiness::Ready,
        ))
        .await?;
    fixture.clock.set(9_000)?;
    fixture.drive()?;
    assert!(fixture.snapshot()?.attempt().is_none());
    assert_eq!(
        fixture.snapshot()?.timer().elapsed(),
        ClockDuration::from_millis(100)?
    );
    fixture.shutdown().await
}

#[tokio::test]
async fn cancelled_waits_and_dropped_execute_future_keep_accepted_write_and_receipt()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    let mut receipt = fixture.submit(DesktopPuzzleBridgeEnumOperation::Command(
        PuzzlePersistenceEnumCommand::Hint,
    ))?;
    let mut wait = Box::pin(receipt.wait());
    assert!(
        wait.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(wait);
    fixture.drive()?;
    let first = receipt.wait().await?;
    assert_eq!(
        first
            .snapshot()
            .attempt()
            .ok_or("Missing hinted puzzle")?
            .hint_stage(),
        crate::puzzle::enums::PuzzleEnumHintStage::Source
    );
    assert_eq!(
        receipt.wait().await?.snapshot().context(),
        first.snapshot().context()
    );
    fixture.clock.set(500)?;
    fixture.drive()?;
    let mut retained = fixture.port.execute(DesktopPuzzleBridgeRequest {
        receipt: fixture.receipt.clone(),
        expected: fixture.snapshot()?.context().clone(),
        operation: DesktopPuzzleBridgeEnumOperation::Command(PuzzlePersistenceEnumCommand::Pause(
            PuzzleTimerEnumPauseReason::Manual,
        )),
    });
    assert!(
        retained
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(retained);
    fixture.drive()?;
    assert!(fixture.snapshot()?.attempt().is_none());
    fixture.drain()?;
    assert_eq!(
        fixture.snapshot()?.durable_context(),
        Some(fixture.snapshot()?.context())
    );
    assert!(fixture.wake.calls.load(Ordering::Relaxed) > 0);
    fixture.shutdown().await
}

#[tokio::test]
async fn actual_application_cancellation_prevents_unaccepted_mutation()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    let before = fixture.snapshot()?;
    let mut receipt = fixture.submit(DesktopPuzzleBridgeEnumOperation::Command(
        PuzzlePersistenceEnumCommand::Hint,
    ))?;
    fixture.receipt.cancellation().request();
    fixture.drive()?;
    assert!(
        matches!(receipt.wait().await, Err(error) if matches!(error.as_ref(),DesktopPuzzleBridgeError::Cancelled))
    );
    assert_eq!(fixture.snapshot()?.context(), before.context());
    fixture.shutdown().await
}

#[tokio::test]
async fn ordinary_saturation_does_not_consume_pause_hidden_native_or_shutdown_slots()
-> DesktopNativePuzzleRuntimeTestResult {
    for command in [
        PuzzlePersistenceEnumCommand::Pause(PuzzleTimerEnumPauseReason::Manual),
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Hidden),
    ] {
        let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
        fixture.start().await?;
        let mut ordinary = fixture.fill_ordinary()?;
        assert!(
            fixture
                .submit(DesktopPuzzleBridgeEnumOperation::Snapshot)
                .is_err()
        );
        let mut urgent = fixture.submit(DesktopPuzzleBridgeEnumOperation::Command(command))?;
        fixture.drive()?;
        assert!(urgent.wait().await?.snapshot().attempt().is_none());
        for receipt in &mut ordinary {
            assert!(receipt.wait().await?.snapshot().attempt().is_none());
        }
        fixture.drain()?;
        fixture.shutdown().await?;
    }
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    let mut ordinary = fixture.fill_ordinary()?;
    let mut native = fixture
        .port
        .interrupt(PuzzleTimerEnumPauseReason::WindowHidden)?;
    fixture.drive()?;
    assert!(native.wait().await?.snapshot().attempt().is_none());
    for receipt in &mut ordinary {
        assert!(receipt.wait().await?.snapshot().attempt().is_none());
    }
    fixture.drain()?;
    let mut refused = fixture.fill_ordinary()?;
    fixture.shutdown().await?;
    for receipt in &mut refused {
        assert!(
            matches!(receipt.wait().await, Err(error) if matches!(error.as_ref(),DesktopPuzzleBridgeError::Closing))
        );
    }
    Ok(())
}

#[tokio::test]
async fn recovery_save_and_readiness_never_replace_explicit_resume()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut run = DesktopNativePuzzleRuntimeFixture::run(PuzzleEnumCountdownMode::Session, 60_000)?;
    run.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        ClockInstant::from_nanos(0),
    )?;
    let bytes = PuzzlePersistenceCodec::encode(&run.capture(ClockInstant::from_millis(500)?, 0)?)?;
    let mut fixture =
        DesktopNativePuzzleRuntimeFixture::from_run(PuzzlePersistenceCodec::decode(&bytes)?)?;
    fixture.drive()?;
    fixture.drain()?;
    assert!(fixture.snapshot()?.attempt().is_none());
    let mut readiness = fixture.submit(DesktopPuzzleBridgeEnumOperation::Command(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
    ))?;
    fixture.drive()?;
    assert!(readiness.wait().await.is_err());
    fixture
        .command(PuzzlePersistenceEnumCommand::Resume(
            PuzzleTimerEnumReadiness::Ready,
        ))
        .await?;
    assert_eq!(fixture.snapshot()?.status(), PuzzleRunEnumStatus::Active);
    assert_eq!(
        fixture.snapshot()?.timer().elapsed(),
        ClockDuration::from_millis(500)?
    );
    fixture.drain()?;
    fixture.shutdown().await
}

#[tokio::test]
async fn shutdown_failure_retains_newest_state_until_explicit_retry_drains_it()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    fixture.command(PuzzlePersistenceEnumCommand::Hint).await?;
    let mut shutdown = fixture
        .port
        .request_shutdown(Instant::now() + Duration::from_secs(2))?;
    fixture.drive()?;
    assert!(fixture.snapshot()?.attempt().is_none());
    let newest = fixture.snapshot()?.context().clone();
    fixture.saves.try_recv()?.complete(false)?;
    fixture.drive()?;
    assert!(shutdown.wait().await.is_err());
    assert!(!fixture.actor.is_shutdown_complete());
    assert!(!fixture.port.is_shutdown_complete());
    assert_eq!(fixture.snapshot()?.context(), &newest);
    assert!(fixture.snapshot()?.has_unsaved_state());
    let mut resume = fixture.submit(DesktopPuzzleBridgeEnumOperation::Command(
        PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::Ready),
    ))?;
    fixture.drive()?;
    assert!(
        matches!(resume.wait().await, Err(error) if matches!(error.as_ref(),DesktopPuzzleBridgeError::Closing))
    );
    fixture
        .execute(DesktopPuzzleBridgeEnumOperation::RetrySave)
        .await?;
    fixture.drain()?;
    assert_eq!(fixture.snapshot()?.durable_context(), Some(&newest));
    assert!(fixture.snapshot()?.attempt().is_none());
    fixture.shutdown().await
}

#[tokio::test(start_paused = true)]
async fn cancelled_shutdown_wait_keeps_original_absolute_deadline_and_retained_owner()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut receipt = fixture.port.request_shutdown(deadline)?;
    let mut wait = Box::pin(receipt.wait());
    assert!(
        wait.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(wait);
    fixture.drive()?;
    assert_eq!(fixture.actor.shutdown_deadline, Some(deadline));
    let mut cleanup_retry = Box::pin(
        fixture
            .actor
            .shutdown_retained(deadline + Duration::from_secs(10)),
    );
    assert!(
        cleanup_retry
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(cleanup_retry);
    assert_eq!(fixture.actor.shutdown_deadline, Some(deadline));
    assert_eq!(
        fixture
            .actor
            .pending_shutdown
            .as_ref()
            .ok_or("Missing retained shutdown")?
            .deadline,
        deadline
    );
    tokio::time::advance(Duration::from_secs(2)).await;
    fixture.drive()?;
    assert!(
        matches!(receipt.wait().await, Err(error) if matches!(error.as_ref(),DesktopPuzzleBridgeError::ShutdownDeadline))
    );
    assert!(!fixture.actor.is_shutdown_complete());
    assert!(fixture.actor.owner.has_retained_write());
    fixture.drain()?;
    assert!(fixture.snapshot()?.attempt().is_none());
    assert!(
        matches!(fixture.actor.shutdown_retained(deadline).await, Err(error) if matches!(error.as_ref(),DesktopPuzzleBridgeError::ShutdownDeadline))
    );
    assert!(!fixture.actor.is_shutdown_complete());
    fixture.shutdown().await
}

#[tokio::test]
async fn replies_remain_explicit_and_charge_all_waiting_time_while_summary_is_explicit()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    fixture
        .command(PuzzlePersistenceEnumCommand::Move(
            BoardPosition::standard().parse_uci("e2e4")?,
        ))
        .await?;
    fixture.drain()?;
    fixture.clock.set(250)?;
    fixture.drive()?;
    assert_eq!(
        fixture
            .snapshot()?
            .attempt()
            .ok_or("Missing reply position")?
            .phase(),
        PuzzleEnumPhase::OpponentReply
    );
    assert_eq!(
        fixture.snapshot()?.timer().elapsed(),
        ClockDuration::from_millis(250)?
    );
    fixture.drain()?;
    fixture
        .command(PuzzlePersistenceEnumCommand::OpponentReply)
        .await?;
    fixture.drain()?;
    let reply = fixture.snapshot()?;
    assert_eq!(
        reply.attempt().ok_or("Missing solver position")?.phase(),
        PuzzleEnumPhase::SolverMove
    );
    let board = BoardPosition::standard();
    let after_first = board.play(board.parse_uci("e2e4")?)?;
    let after_reply = after_first.play(after_first.parse_uci("e7e5")?)?;
    fixture
        .command(PuzzlePersistenceEnumCommand::Move(
            after_reply.parse_uci("g1f3")?,
        ))
        .await?;
    let summary = fixture
        .execute(DesktopPuzzleBridgeEnumOperation::Summary)
        .await?;
    match summary {
        DesktopPuzzleBridgeEnumResult::Summary { summary, .. } => assert_eq!(
            summary.primary_count(PuzzleEnumPrimaryOutcome::Solved(
                PuzzleEnumAssistance::Unassisted
            )),
            1
        ),
        DesktopPuzzleBridgeEnumResult::Snapshot(_) => {
            return Err("Summary returned a cadence snapshot".into());
        }
    }
    fixture.drain()?;
    fixture.shutdown().await
}

#[tokio::test(start_paused = true)]
async fn production_scheduler_uses_supplied_runtime_and_shared_epoch_pair()
-> DesktopNativePuzzleRuntimeTestResult {
    let clock_epoch = ClockInstant::from_millis(5_000)?;
    let runtime_epoch = Instant::now();
    let scheduler = DesktopNativePuzzleRuntimeSchedulerTokio::new(
        tokio::runtime::Handle::current(),
        clock_epoch,
        runtime_epoch,
    );
    let mut wait = Box::pin(scheduler.wait_until(ClockInstant::from_millis(5_250)?));
    assert!(
        wait.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    tokio::time::advance(Duration::from_millis(249)).await;
    assert!(
        wait.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    tokio::time::advance(Duration::from_millis(1)).await;
    wait.await?;
    scheduler.wait_until(clock_epoch).await?;
    Ok(())
}

#[tokio::test]
async fn first_repository_admission_refusal_publishes_frozen_state_and_keeps_retry_serviced()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.accepting.store(false, Ordering::Release);
    let running = fixture.drive();
    assert!(
        running.is_ok(),
        "A recoverable initial save refusal stopped the actor: {running:?}"
    );
    fixture.assert_save_blocked()?;
    assert_eq!(fixture.snapshot()?.timer().elapsed(), ClockDuration::ZERO);
    assert_eq!(fixture.begun.load(Ordering::Relaxed), 0);
    let mut snapshot = fixture.submit(DesktopPuzzleBridgeEnumOperation::Snapshot)?;
    fixture.drive()?;
    let observed = DesktopNativePuzzleRuntimeFixture::completed(&mut snapshot)??;
    assert!(observed.snapshot().attempt().is_none());
    fixture.accepting.store(true, Ordering::Release);
    fixture
        .execute(DesktopPuzzleBridgeEnumOperation::RetrySave)
        .await?;
    fixture.drain()?;
    assert!(fixture.snapshot()?.attempt().is_none());
    assert!(!fixture.snapshot()?.has_unsaved_state());
    fixture
        .command(PuzzlePersistenceEnumCommand::Resume(
            PuzzleTimerEnumReadiness::Ready,
        ))
        .await?;
    assert_eq!(fixture.snapshot()?.status(), PuzzleRunEnumStatus::Active);
    fixture.drain()?;
    fixture.shutdown().await
}

#[tokio::test]
async fn snapshot_started_save_admission_refusal_keeps_actor_and_authoritative_pause_alive()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    fixture.accepting.store(false, Ordering::Release);
    fixture.clock.set(250)?;
    let mut request = fixture.submit(DesktopPuzzleBridgeEnumOperation::Snapshot)?;
    let running = fixture.drive();
    assert!(
        running.is_ok(),
        "A snapshot save refusal stopped the actor: {running:?}"
    );
    let outcome = DesktopNativePuzzleRuntimeFixture::completed(&mut request)?;
    assert!(
        matches!(outcome, Err(error) if matches!(error.as_ref(), DesktopPuzzleBridgeError::Commands(_)))
    );
    fixture.assert_save_blocked()?;
    assert_eq!(
        fixture.snapshot()?.timer().elapsed(),
        ClockDuration::from_millis(250)?
    );
    fixture.accepting.store(true, Ordering::Release);
    fixture
        .execute(DesktopPuzzleBridgeEnumOperation::RetrySave)
        .await?;
    fixture.drain()?;
    assert!(fixture.snapshot()?.attempt().is_none());
    fixture.shutdown().await
}

#[tokio::test]
async fn coalesced_save_admission_refusal_retains_newest_pause_and_services_explicit_retry()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    fixture.clock.set(250)?;
    fixture.drive()?;
    let first = fixture.saves.try_recv()?;
    fixture.clock.set(500)?;
    fixture.drive()?;
    fixture.accepting.store(false, Ordering::Release);
    first.complete(true)?;
    let running = fixture.drive();
    assert!(
        running.is_ok(),
        "A coalesced save refusal stopped the actor: {running:?}"
    );
    fixture.assert_save_blocked()?;
    let paused = fixture.snapshot()?;
    assert_eq!(paused.timer().elapsed(), ClockDuration::from_millis(500)?);
    assert_eq!(
        paused.durable_elapsed(),
        Some(ClockDuration::from_millis(250)?)
    );
    assert!(paused.has_unsaved_state());
    let mut observed = fixture.submit(DesktopPuzzleBridgeEnumOperation::Snapshot)?;
    fixture.drive()?;
    assert_eq!(
        DesktopNativePuzzleRuntimeFixture::completed(&mut observed)??
            .snapshot()
            .context(),
        paused.context()
    );
    fixture.accepting.store(true, Ordering::Release);
    fixture
        .execute(DesktopPuzzleBridgeEnumOperation::RetrySave)
        .await?;
    fixture.drain()?;
    let saved = fixture.snapshot()?;
    assert_eq!(saved.context(), paused.context());
    assert_eq!(saved.durable_context(), Some(paused.context()));
    assert_eq!(saved.durable_elapsed(), Some(paused.timer().elapsed()));
    assert!(saved.attempt().is_none());
    assert!(!saved.has_unsaved_state());
    fixture.shutdown().await
}

#[tokio::test]
async fn duplicate_committed_receipt_keeps_blocked_health_and_services_requests_without_spinning()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    fixture.start().await?;
    fixture.clock.set(250)?;
    fixture.drive()?;
    let old = fixture.saves.try_recv()?;
    let old_request = old.request.clone();
    let old_revision = old_request
        .expected_storage_revision()
        .value()
        .checked_add(1)
        .ok_or(DesktopNativePuzzleRuntimeFixtureError::Revision)?;
    fixture.command(PuzzlePersistenceEnumCommand::Hint).await?;
    let newest = fixture.snapshot()?.context().clone();
    old.complete(true)?;
    fixture.drive()?;
    let current = fixture.saves.try_recv()?;
    current.respond(Ok(PuzzlePersistenceReceipt::committed(
        old_request,
        PuzzlePersistenceStorageRevision::new(old_revision),
    )))?;
    let mut snapshot = fixture.submit(DesktopPuzzleBridgeEnumOperation::Snapshot)?;
    fixture.wake.probe_blocked();
    let running = fixture.drive();
    let observed_ready = fixture.wake.observed_ready.load(Ordering::Acquire);
    assert!(
        running.is_ok() && !observed_ready,
        "Blocked receipt progress: {running:?}; published_ready: {observed_ready}; remaining_publications: {}",
        fixture.wake.remaining.load(Ordering::Acquire)
    );
    fixture.wake.finish_probe();
    let observed = DesktopNativePuzzleRuntimeFixture::completed(&mut snapshot)??;
    assert_eq!(
        observed.snapshot().context().session_id(),
        newest.session_id()
    );
    assert!(observed.snapshot().context().revision() >= newest.revision());
    assert!(observed.snapshot().attempt().is_none());
    assert!(observed.snapshot().has_retained_write());
    assert!(!matches!(
        *fixture.port.health().borrow(),
        DesktopPuzzleBridgeEnumHealth::Ready
    ));
    let mut retry = fixture.submit(DesktopPuzzleBridgeEnumOperation::RetrySave)?;
    fixture.drive()?;
    assert!(
        matches!(DesktopNativePuzzleRuntimeFixture::completed(&mut retry)?, Err(error) if matches!(error.as_ref(), DesktopPuzzleBridgeError::Commands(_)))
    );
    let mut interruption = fixture
        .port
        .interrupt(PuzzleTimerEnumPauseReason::WindowHidden)?;
    fixture.drive()?;
    assert!(
        DesktopNativePuzzleRuntimeFixture::completed(&mut interruption)??
            .snapshot()
            .attempt()
            .is_none()
    );
    assert!(fixture.actor.owner.has_retained_write());
    assert!(!fixture.actor.is_shutdown_complete());
    assert!(!matches!(
        *fixture.port.health().borrow(),
        DesktopPuzzleBridgeEnumHealth::Ready
    ));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn queued_shutdown_before_first_actor_poll_keeps_original_deadline_during_direct_cleanup()
-> DesktopNativePuzzleRuntimeTestResult {
    let mut fixture = DesktopNativePuzzleRuntimeFixture::new()?;
    let original_deadline = Instant::now() + Duration::from_secs(2);
    let later_deadline = original_deadline + Duration::from_secs(20);
    let mut receipt = fixture.port.request_shutdown(original_deadline)?;
    assert!(!fixture.actor.started);
    assert!(
        DesktopNativePuzzleRuntimeFixture::poll_future(
            fixture.actor.shutdown_retained(later_deadline)
        )
        .is_pending()
    );
    assert!(!fixture.actor.started);
    tokio::time::advance(Duration::from_secs(2)).await;
    let mut outcome = None;
    for _ in 0..8 {
        if let Ok(save) = fixture.saves.try_recv() {
            save.complete(true)?;
        }
        match DesktopNativePuzzleRuntimeFixture::poll_future(
            fixture.actor.shutdown_retained(later_deadline),
        ) {
            Poll::Ready(result) => {
                outcome = Some(result);
                break;
            }
            Poll::Pending => {}
        }
    }
    let outcome =
        outcome.ok_or("Direct cleanup did not settle within the bounded controlled polls")?;
    assert!(
        matches!(outcome, Err(error) if matches!(error.as_ref(), DesktopPuzzleBridgeError::ShutdownDeadline)),
        "A later direct cleanup deadline renewed the queued shutdown"
    );
    assert!(
        matches!(DesktopNativePuzzleRuntimeFixture::completed(&mut receipt)?, Err(error) if matches!(error.as_ref(), DesktopPuzzleBridgeError::ShutdownDeadline)),
        "The expired queued shutdown was acknowledged as complete"
    );
    assert!(!fixture.actor.is_shutdown_complete());
    assert!(!fixture.port.is_shutdown_complete());
    assert!(fixture.actor.owner.has_retained_write());
    fixture.drain()?;
    fixture.shutdown().await
}
