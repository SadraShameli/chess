use std::future::{Future, poll_fn};
use std::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

use crate::board::position::BoardPosition;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationPort, DesktopApplicationReceipt,
    DesktopApplicationWakeError, DesktopApplicationWakeProvider,
};
use crate::desktop::coordinator::{
    DesktopCoordinator, DesktopCoordinatorDialogProvider, DesktopCoordinatorError,
    DesktopCoordinatorLifecycleProvider, DesktopCoordinatorServices,
};
use crate::desktop::dto::{DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot};
use crate::desktop::engine_bridge::{
    DesktopEngineBridgeEnumResult, DesktopEngineBridgeFuture, DesktopEngineBridgeProvider,
    DesktopEngineBridgeRequest,
};
use crate::desktop::engine_commands::{
    DesktopEngineCommandsContext, DesktopEngineCommandsEnumOperation,
    DesktopEngineCommandsOperation,
};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileEnumSelection,
    DesktopFileHandleError, DesktopFileHandleId, DesktopFileHandleRequest,
};
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};
use crate::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
    DesktopRequest,
};
use crate::desktop::puzzle_bridge::DesktopPuzzleBridgeEnumOperation;
use crate::desktop::puzzle_service::{
    DesktopPuzzleServiceEnumActivity, DesktopPuzzleServiceEnumOperation,
    DesktopPuzzleServiceEnumResult, DesktopPuzzleServiceError, DesktopPuzzleServiceOutcome,
    DesktopPuzzleServiceOutcomeSender, DesktopPuzzleServiceProvider, DesktopPuzzleServiceReceipt,
    DesktopPuzzleServiceRequest, DesktopPuzzleServiceState,
};
use crate::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation;
use crate::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchGeneration, EngineSearchRevision,
};
use crate::package::digest::PackageDigest;
use crate::puzzle::attempt::PuzzleAttemptId;
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::session::PuzzleSessionId;
use crate::puzzle::session::{PuzzleSessionContext, PuzzleSessionRevision};
use crate::puzzle::timer::PuzzleTimerEnumPauseReason;
use crate::workbench::enums::WorkbenchEnumParameters;
use crate::workbench::job::{WorkbenchJobDeadline, WorkbenchJobId};

type DesktopCoordinatorEngineQuitTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct DesktopCoordinatorEngineQuitClock(AtomicU64);

impl ClockTimeProvider for DesktopCoordinatorEngineQuitClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.0.load(Ordering::Acquire)))
    }
}

struct DesktopCoordinatorEngineQuitWake;

impl DesktopApplicationWakeProvider for DesktopCoordinatorEngineQuitWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

struct DesktopCoordinatorEngineQuitDialog;

impl DesktopFileDialogProvider for DesktopCoordinatorEngineQuitDialog {
    async fn select(
        &self,
        _purpose: DesktopFileEnumPurpose,
    ) -> Result<Option<PathBuf>, DesktopFileHandleError> {
        Err(DesktopFileHandleError::Io(std::io::Error::other(
            "this fixture registers actual native selections directly",
        )))
    }
}

impl DesktopCoordinatorDialogProvider for DesktopCoordinatorEngineQuitDialog {
    fn request_cancel(&self) -> Result<(), DesktopFileHandleError> {
        Ok(())
    }
}

#[derive(Default)]
struct DesktopCoordinatorEngineQuitLifecycle {
    deadlines: Mutex<Vec<Instant>>,
    required: Mutex<Option<DesktopApplicationReceipt>>,
    fail_next: AtomicBool,
}

impl DesktopCoordinatorLifecycleProvider for Arc<DesktopCoordinatorEngineQuitLifecycle> {
    async fn shutdown(&mut self, deadline: Instant) -> Result<(), DesktopCoordinatorError> {
        let observed = self
            .required
            .lock()
            .map_err(|error| DesktopCoordinatorError::Shutdown {
                source: Box::new(std::io::Error::other(error.to_string())),
            })?
            .as_ref()
            .is_none_or(|receipt| receipt.response().is_some());
        if !observed {
            return Err(DesktopCoordinatorError::Shutdown {
                source: Box::new(std::io::Error::other(
                    "root cleanup preceded observation of the engine receipt",
                )),
            });
        }
        self.deadlines
            .lock()
            .map_err(|error| DesktopCoordinatorError::Shutdown {
                source: Box::new(std::io::Error::other(error.to_string())),
            })?
            .push(deadline);
        if self.fail_next.swap(false, Ordering::AcqRel) {
            return Err(DesktopCoordinatorError::Shutdown {
                source: Box::new(std::io::Error::other(
                    "root cleanup remains owned for explicit retry",
                )),
            });
        }
        Ok(())
    }
}

struct DesktopCoordinatorEngineQuitRequest {
    request: DesktopEngineBridgeRequest,
    response: oneshot::Sender<Result<DesktopEngineBridgeEnumResult, DesktopFailure>>,
}

struct DesktopCoordinatorEngineQuitQuiescence {
    deadline: Instant,
    response: oneshot::Sender<Result<(), DesktopFailure>>,
}

struct DesktopCoordinatorEngineQuitProvider {
    requests: mpsc::Sender<DesktopCoordinatorEngineQuitRequest>,
    quiescences: mpsc::Sender<DesktopCoordinatorEngineQuitQuiescence>,
    calls: AtomicUsize,
}

impl DesktopCoordinatorEngineQuitProvider {
    fn failure(message: impl ToString) -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            message.to_string(),
            true,
        )
    }
}

impl DesktopEngineBridgeProvider for DesktopCoordinatorEngineQuitProvider {
    fn execute(&self, request: DesktopEngineBridgeRequest) -> DesktopEngineBridgeFuture {
        let (response, receiver) = oneshot::channel();
        let admitted = self
            .requests
            .try_send(DesktopCoordinatorEngineQuitRequest { request, response })
            .map_err(Self::failure);
        Box::pin(async move {
            admitted?;
            receiver.await.map_err(Self::failure)?
        })
    }

    fn quiesce(
        &self,
        deadline: Instant,
    ) -> Pin<Box<dyn Future<Output = Result<(), DesktopFailure>> + Send>> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        let (response, receiver) = oneshot::channel();
        let admitted = self
            .quiescences
            .try_send(DesktopCoordinatorEngineQuitQuiescence { deadline, response })
            .map_err(Self::failure);
        Box::pin(async move {
            admitted?;
            receiver.await.map_err(Self::failure)?
        })
    }
}

struct DesktopCoordinatorEngineQuitFixture {
    coordinator: DesktopCoordinator<
        DesktopCoordinatorEngineQuitClock,
        DesktopCoordinatorEngineQuitWake,
        DesktopCoordinatorEngineQuitDialog,
        Arc<DesktopCoordinatorEngineQuitLifecycle>,
    >,
    port:
        DesktopApplicationPort<DesktopCoordinatorEngineQuitClock, DesktopCoordinatorEngineQuitWake>,
    clock: Arc<DesktopCoordinatorEngineQuitClock>,
    engine: Arc<DesktopCoordinatorEngineQuitProvider>,
    requests: mpsc::Receiver<DesktopCoordinatorEngineQuitRequest>,
    quiescences: mpsc::Receiver<DesktopCoordinatorEngineQuitQuiescence>,
    lifecycle: Arc<DesktopCoordinatorEngineQuitLifecycle>,
    executor: Arc<CapacityExecutor>,
    context: DesktopEngineCommandsContext,
}

impl DesktopCoordinatorEngineQuitFixture {
    const WINDOW: Duration = Duration::from_secs(3);

    fn new() -> DesktopCoordinatorEngineQuitTestResult<Self> {
        let clock = Arc::new(DesktopCoordinatorEngineQuitClock::default());
        let application = DesktopApplication::new(
            Arc::clone(&clock),
            Arc::new(DesktopCoordinatorEngineQuitWake),
        );
        let port = application.port();
        let executor = Arc::new(CapacityExecutor::new(CapacityBudget::new(
            NonZeroUsize::MIN,
        )?)?);
        let lifecycle = Arc::new(DesktopCoordinatorEngineQuitLifecycle::default());
        let (request_sender, requests) = mpsc::channel(2);
        let (quiescence_sender, quiescences) = mpsc::channel(2);
        let engine = Arc::new(DesktopCoordinatorEngineQuitProvider {
            requests: request_sender,
            quiescences: quiescence_sender,
            calls: AtomicUsize::new(0),
        });
        let bridge = Arc::clone(&engine);
        let coordinator = DesktopCoordinator::new(
            application,
            Arc::clone(&clock),
            DesktopBootstrapSnapshot::new(
                &BoardPosition::standard(),
                NonZeroU32::MIN,
                DesktopEnumEngineSnapshot::Unavailable {
                    message: "controlled native engine port".into(),
                },
            ),
            DesktopCoordinatorServices::new(
                Arc::new(DesktopCoordinatorEngineQuitDialog),
                Arc::clone(&executor),
                Arc::clone(&lifecycle),
            ),
        )
        .with_engine(bridge);
        let context = DesktopEngineCommandsContext::new(
            DesktopMessagesEntityId::new(),
            EngineSearchGeneration::fresh(),
            DesktopRevision::ZERO,
            EngineSearchContext::new(
                EngineSearchGameId::try_from(uuid::Uuid::from_u128(1))?,
                EngineSearchRevision::new(0),
            ),
        );
        Ok(Self {
            coordinator,
            port,
            clock,
            engine,
            requests,
            quiescences,
            lifecycle,
            executor,
            context,
        })
    }

    fn request(
        &self,
        action: DesktopEngineCommandsEnumOperation,
    ) -> DesktopCoordinatorEngineQuitTestResult<DesktopRequest> {
        Ok(self.port.request(DesktopCommand::new(
            DesktopEnumCommand::Engine(DesktopEngineCommandsOperation::new(self.context, action)),
            Some(self.context.configuration()),
        ))?)
    }

    fn submit(
        &self,
        action: DesktopEngineCommandsEnumOperation,
    ) -> DesktopCoordinatorEngineQuitTestResult<DesktopApplicationReceipt> {
        Ok(self.port.try_submit(&self.request(action)?)?)
    }

    fn quit(&self) -> DesktopCoordinatorEngineQuitTestResult<DesktopApplicationReceipt> {
        Ok(self.port.try_submit(
            &self
                .port
                .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
        )?)
    }

    async fn poll(&mut self) -> DesktopCoordinatorEngineQuitTestResult<bool> {
        let running = self.coordinator.run();
        tokio::pin!(running);
        Ok(poll_fn(|context| {
            Poll::Ready(match running.as_mut().poll(context) {
                Poll::Ready(result) => result.map(|()| true),
                Poll::Pending => Ok(false),
            })
        })
        .await?)
    }

    async fn pending(
        &mut self,
    ) -> DesktopCoordinatorEngineQuitTestResult<(
        DesktopApplicationReceipt,
        DesktopCoordinatorEngineQuitRequest,
    )> {
        let receipt = self.submit(DesktopEngineCommandsEnumOperation::Workbench(
            DesktopWorkbenchCommandsEnumOperation::Start {
                parameters: WorkbenchEnumParameters::Compiler,
                deadline: WorkbenchJobDeadline::default(),
            },
        ))?;
        assert!(!self.poll().await?);
        let request = self.requests.try_recv()?;
        *self
            .lifecycle
            .required
            .lock()
            .map_err(|error| error.to_string())? = Some(receipt.clone());
        Ok((receipt, request))
    }

    fn cancel(
        request: DesktopCoordinatorEngineQuitRequest,
    ) -> DesktopCoordinatorEngineQuitTestResult {
        request
            .response
            .send(Err(DesktopFailure::new(
                DesktopEnumFailureCode::Cancelled,
                "native setup cancellation observed".into(),
                false,
            )))
            .map_err(|outcome| format!("Engine response consumer abandoned {outcome:?}"))?;
        Ok(())
    }

    fn quiesced(
        quiescence: DesktopCoordinatorEngineQuitQuiescence,
    ) -> DesktopCoordinatorEngineQuitTestResult {
        quiescence
            .response
            .send(Ok(()))
            .map_err(|outcome| format!("Quiesce response consumer abandoned {outcome:?}"))?;
        Ok(())
    }

    fn lifecycle_deadlines(&self) -> DesktopCoordinatorEngineQuitTestResult<Vec<Instant>> {
        Ok(self
            .lifecycle
            .deadlines
            .lock()
            .map_err(|error| error.to_string())?
            .clone())
    }

    async fn selection(
        &mut self,
        path: &Path,
        owner: DesktopMessagesEntityId,
        purpose: DesktopFileEnumPurpose,
    ) -> DesktopCoordinatorEngineQuitTestResult<DesktopFileHandleId> {
        let files = self
            .coordinator
            .files
            .as_mut()
            .ok_or("missing canonical native selection registry")?;
        Ok(files
            .registry
            .register_selection(
                path.to_path_buf(),
                DesktopFileHandleRequest::new(
                    owner.file_handle_owner(),
                    purpose,
                    self.clock.now()?,
                ),
                &files.executor,
            )
            .await?)
    }

    fn export(
        &self,
        destination: DesktopFileHandleId,
    ) -> DesktopCoordinatorEngineQuitTestResult<DesktopEngineCommandsEnumOperation> {
        Ok(DesktopEngineCommandsEnumOperation::Workbench(
            DesktopWorkbenchCommandsEnumOperation::ApplyExport {
                preview: DesktopRequestId::new(),
                digest: PackageDigest::read(&b"exact native preview"[..])?,
                destination,
            },
        ))
    }

    fn refused(receipt: &DesktopApplicationReceipt) -> DesktopCoordinatorEngineQuitTestResult {
        let Some(DesktopMessagesEnumResult::Failure(failure)) = receipt.response() else {
            return Err("expected native file refusal".into());
        };
        assert!(matches!(
            failure.code(),
            DesktopEnumFailureCode::FileUnavailable
        ));
        Ok(())
    }

    async fn close(self) -> DesktopCoordinatorEngineQuitTestResult {
        drop(self.coordinator);
        Arc::try_unwrap(self.executor)
            .map_err(|executor| {
                format!(
                    "Coordinator retains {} executor owners",
                    Arc::strong_count(&executor)
                )
            })?
            .shutdown()
            .await?;
        Ok(())
    }
}

struct DesktopCoordinatorPuzzleReceiptRequest {
    request: DesktopPuzzleServiceRequest,
    response: DesktopPuzzleServiceOutcomeSender<DesktopPuzzleServiceEnumResult>,
}

struct DesktopCoordinatorPuzzleReceiptProvider {
    requests: mpsc::Sender<DesktopCoordinatorPuzzleReceiptRequest>,
    state: watch::Sender<DesktopPuzzleServiceState>,
}

impl DesktopCoordinatorPuzzleReceiptProvider {
    fn new() -> (Self, mpsc::Receiver<DesktopCoordinatorPuzzleReceiptRequest>) {
        let (requests, receiver) = mpsc::channel(2);
        let (state, _receiver) = watch::channel(DesktopPuzzleServiceState::new(
            None,
            None,
            DesktopPuzzleServiceEnumActivity::Idle,
            None,
            None,
        ));
        (Self { requests, state }, receiver)
    }
}

impl DesktopPuzzleServiceProvider for DesktopCoordinatorPuzzleReceiptProvider {
    fn submit(
        &self,
        request: DesktopPuzzleServiceRequest,
    ) -> DesktopPuzzleServiceOutcome<DesktopPuzzleServiceReceipt> {
        let (response, receipt) = DesktopPuzzleServiceReceipt::channel();
        self.requests
            .try_send(DesktopCoordinatorPuzzleReceiptRequest { request, response })
            .map_err(|error| {
                Arc::new(match error {
                    mpsc::error::TrySendError::Full(_) => DesktopPuzzleServiceError::Busy,
                    mpsc::error::TrySendError::Closed(_) => DesktopPuzzleServiceError::Closed,
                })
            })?;
        Ok(receipt)
    }

    fn interrupt(
        &self,
        session: PuzzleSessionId,
        _reason: PuzzleTimerEnumPauseReason,
    ) -> DesktopPuzzleServiceOutcome<DesktopPuzzleServiceReceipt> {
        Err(Arc::new(DesktopPuzzleServiceError::SessionMismatch {
            expected: session,
            selected: self.state.borrow().selected(),
        }))
    }

    fn state(&self) -> watch::Receiver<DesktopPuzzleServiceState> {
        self.state.subscribe()
    }
}

#[tokio::test(start_paused = true)]
async fn puzzle_receipt_is_observed_before_quit_cleanup_at_the_original_deadline()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let (provider, mut requests) = DesktopCoordinatorPuzzleReceiptProvider::new();
    let owner = DesktopMessagesEntityId::new();
    fixture.coordinator = fixture.coordinator.with_puzzles(owner, Arc::new(provider));
    let request = fixture.port.request(DesktopCommand::puzzle(
        owner,
        DesktopPuzzleServiceEnumOperation::Inventory { after: None },
    ))?;
    let receipt = fixture.port.try_submit(&request)?;
    assert!(!fixture.poll().await?);
    let forwarded = requests.try_recv()?;
    assert!(forwarded.request.receipt.shares_outcome(&receipt));
    *fixture
        .lifecycle
        .required
        .lock()
        .map_err(|error| error.to_string())? = Some(receipt.clone());
    let quit = fixture.quit()?;
    let deadline = quit.runtime_received_at() + DesktopCoordinatorEngineQuitFixture::WINDOW;
    assert!(!fixture.poll().await?);
    assert!(fixture.lifecycle_deadlines()?.is_empty());
    assert!(forwarded.request.receipt.is_cancellation_requested());
    tokio::time::advance(Duration::from_secs(2)).await;
    forwarded
        .response
        .send_replace(Some(Err(Arc::new(DesktopPuzzleServiceError::Cancelled))));
    assert!(fixture.poll().await?);
    assert_eq!(fixture.lifecycle_deadlines()?, [deadline]);
    assert!(matches!(
        receipt.response(),
        Some(DesktopMessagesEnumResult::Failure(failure))
            if matches!(failure.code(), DesktopEnumFailureCode::Cancelled)
    ));
    assert!(matches!(
        quit.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    fixture.close().await
}

#[tokio::test]
async fn puzzle_revision_mismatch_is_rejected_before_service_dispatch()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let (provider, mut requests) = DesktopCoordinatorPuzzleReceiptProvider::new();
    let owner = DesktopMessagesEntityId::new();
    fixture.coordinator = fixture.coordinator.with_puzzles(owner, Arc::new(provider));
    let request = fixture.port.request(DesktopCommand::new(
        DesktopEnumCommand::Puzzle {
            owner,
            operation: DesktopPuzzleServiceEnumOperation::Inventory { after: None },
        },
        Some(DesktopRevision::ZERO.next()?),
    ))?;
    let receipt = fixture.port.try_submit(&request)?;
    assert!(!fixture.poll().await?);
    assert!(matches!(
        receipt.response(),
        Some(DesktopMessagesEnumResult::Failure(failure))
            if matches!(failure.code(), DesktopEnumFailureCode::StaleRevision)
    ));
    assert!(matches!(
        requests.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    fixture.quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await
}

#[tokio::test]
async fn ready_puzzle_control_is_revisited_after_its_mutation_completes()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let (provider, mut requests) = DesktopCoordinatorPuzzleReceiptProvider::new();
    let owner = DesktopMessagesEntityId::new();
    fixture.coordinator = fixture.coordinator.with_puzzles(owner, Arc::new(provider));
    let context = PuzzleSessionContext::new(
        PuzzleSessionId::new(NonZeroU64::MIN),
        PuzzleAttemptId::new(NonZeroU64::MIN),
        PuzzleDefinitionIdentity::new("coordinator".into(), "1".into(), "one".into())?,
        PuzzleSessionRevision::ZERO,
    );
    let inventory = fixture.port.request(DesktopCommand::puzzle(
        owner,
        DesktopPuzzleServiceEnumOperation::Inventory { after: None },
    ))?;
    fixture.port.try_submit(&inventory)?;
    assert!(!fixture.poll().await?);
    let inventory = requests.try_recv()?;
    let mutation = fixture.port.request(DesktopCommand::puzzle(
        owner,
        DesktopPuzzleServiceEnumOperation::Session {
            expected: context.clone(),
            operation: DesktopPuzzleBridgeEnumOperation::Command(
                crate::puzzle::persistence::PuzzlePersistenceEnumCommand::Hint,
            ),
        },
    ))?;
    let mutation_receipt = fixture.port.try_submit(&mutation)?;
    assert!(!fixture.poll().await?);
    let mutation = requests.try_recv()?;
    let pause = fixture.port.request(DesktopCommand::puzzle(
        owner,
        DesktopPuzzleServiceEnumOperation::Interrupt {
            session: context.session_id(),
            reason: PuzzleTimerEnumPauseReason::Manual,
        },
    ))?;
    let pause_receipt = fixture.port.try_submit(&pause)?;
    assert!(!fixture.poll().await?);
    let pause = requests.try_recv()?;
    inventory
        .response
        .send_replace(Some(Err(Arc::new(DesktopPuzzleServiceError::Missing(
            context.session_id(),
        )))));
    assert!(!fixture.poll().await?);
    pause
        .response
        .send_replace(Some(Err(Arc::new(DesktopPuzzleServiceError::Cancelled))));
    assert!(!fixture.poll().await?);
    mutation
        .response
        .send_replace(Some(Err(Arc::new(DesktopPuzzleServiceError::Cancelled))));
    assert!(!fixture.poll().await?);
    let completed_without_another_event =
        mutation_receipt.response().is_some() && pause_receipt.response().is_some();
    fixture.quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await?;
    assert!(
        completed_without_another_event,
        "the ready puzzle control was not revisited after its mutation completed"
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn confirmed_quit_preempts_pending_setup_and_observes_its_receipt_before_shared_cleanup()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let (setup, request) = fixture.pending().await?;
    let bootstrap = fixture.port.try_submit(
        &fixture
            .port
            .request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?,
    )?;
    let quit = fixture.quit()?;
    assert!(quit.shares_outcome(&fixture.quit()?));
    assert!(!fixture.poll().await?);
    let quiescence = fixture.quiescences.try_recv()?;
    let deadline = quit.runtime_received_at() + DesktopCoordinatorEngineQuitFixture::WINDOW;
    assert_eq!(quiescence.deadline, deadline);
    assert!(bootstrap.response().is_some());
    assert!(fixture.lifecycle_deadlines()?.is_empty());
    DesktopCoordinatorEngineQuitFixture::quiesced(quiescence)?;
    assert!(!fixture.poll().await?);
    assert!(quit.response().is_none());
    assert!(fixture.lifecycle_deadlines()?.is_empty());
    DesktopCoordinatorEngineQuitFixture::cancel(request)?;
    assert!(fixture.poll().await?);
    assert!(matches!(
        setup.response(),
        Some(DesktopMessagesEnumResult::Failure(_))
    ));
    assert!(matches!(
        quit.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    assert_eq!(fixture.lifecycle_deadlines()?, [deadline]);
    assert_eq!(fixture.engine.calls.load(Ordering::Acquire), 1);
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn dropped_run_wait_retains_one_actual_quiescence_and_does_not_remint_it()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let (_, request) = fixture.pending().await?;
    let quit = fixture.quit()?;
    assert!(!fixture.poll().await?);
    let quiescence = fixture.quiescences.try_recv()?;
    for _ in 0..4 {
        assert!(!fixture.poll().await?);
    }
    assert_eq!(fixture.engine.calls.load(Ordering::Acquire), 1);
    assert!(fixture.quiescences.try_recv().is_err());
    DesktopCoordinatorEngineQuitFixture::quiesced(quiescence)?;
    DesktopCoordinatorEngineQuitFixture::cancel(request)?;
    assert!(fixture.poll().await?);
    assert!(matches!(
        quit.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn queued_quit_keeps_its_first_deadline_and_retry_never_reuses_the_late_old_outcome()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let (_, request) = fixture.pending().await?;
    let quit = fixture.quit()?;
    let original = quit.runtime_received_at() + DesktopCoordinatorEngineQuitFixture::WINDOW;
    tokio::time::advance(Duration::from_secs(2)).await;
    assert!(!fixture.poll().await?);
    let first = fixture.quiescences.try_recv()?;
    assert_eq!(first.deadline, original);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(!fixture.poll().await?);
    assert!(quit.response().is_none());
    assert!(quit.pending_notice()?.is_some());
    assert!(fixture.lifecycle_deadlines()?.is_empty());
    fixture.port.retry_quit()?;
    let fresh = Instant::now() + DesktopCoordinatorEngineQuitFixture::WINDOW;
    assert!(!fixture.poll().await?);
    let second = fixture.quiescences.try_recv()?;
    assert_eq!(second.deadline, fresh);
    assert!(second.deadline > first.deadline);
    DesktopCoordinatorEngineQuitFixture::quiesced(first)?;
    DesktopCoordinatorEngineQuitFixture::cancel(request)?;
    assert!(!fixture.poll().await?);
    assert!(quit.response().is_none());
    assert!(fixture.lifecycle_deadlines()?.is_empty());
    DesktopCoordinatorEngineQuitFixture::quiesced(second)?;
    assert!(fixture.poll().await?);
    assert_eq!(fixture.lifecycle_deadlines()?, [fresh]);
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn quiescence_refusal_waits_for_explicit_retry_even_after_engine_receipts_finish()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let (_, request) = fixture.pending().await?;
    let quit = fixture.quit()?;
    assert!(!fixture.poll().await?);
    let first = fixture.quiescences.try_recv()?;
    first
        .response
        .send(Err(DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            "retained child needs another cleanup attempt".into(),
            true,
        )))
        .map_err(|outcome| format!("Quiesce consumer abandoned {outcome:?}"))?;
    DesktopCoordinatorEngineQuitFixture::cancel(request)?;
    for _ in 0..3 {
        assert!(!fixture.poll().await?);
    }
    assert!(quit.pending_notice()?.is_some());
    assert!(fixture.lifecycle_deadlines()?.is_empty());
    assert_eq!(fixture.engine.calls.load(Ordering::Acquire), 1);
    tokio::time::advance(Duration::from_millis(100)).await;
    fixture.port.retry_quit()?;
    let fresh = Instant::now() + DesktopCoordinatorEngineQuitFixture::WINDOW;
    assert!(!fixture.poll().await?);
    let second = fixture.quiescences.try_recv()?;
    assert_eq!(second.deadline, fresh);
    assert!(quit.response().is_none());
    DesktopCoordinatorEngineQuitFixture::quiesced(second)?;
    assert!(fixture.poll().await?);
    assert_eq!(fixture.lifecycle_deadlines()?, [fresh]);
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn repeated_retry_is_bounded_without_dropping_prior_quiescence_responses()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let (_, request) = fixture.pending().await?;
    let quit = fixture.quit()?;
    assert!(!fixture.poll().await?);
    let first = fixture.quiescences.try_recv()?;
    tokio::time::advance(DesktopCoordinatorEngineQuitFixture::WINDOW).await;
    assert!(!fixture.poll().await?);
    fixture.port.retry_quit()?;
    assert!(!fixture.poll().await?);
    let second = fixture.quiescences.try_recv()?;
    fixture.port.retry_quit()?;
    assert!(!fixture.poll().await?);
    assert_eq!(fixture.engine.calls.load(Ordering::Acquire), 2);
    assert!(fixture.quiescences.try_recv().is_err());
    DesktopCoordinatorEngineQuitFixture::quiesced(first)?;
    DesktopCoordinatorEngineQuitFixture::quiesced(second)?;
    DesktopCoordinatorEngineQuitFixture::cancel(request)?;
    assert!(!fixture.poll().await?);
    assert!(quit.response().is_none());
    fixture.port.retry_quit()?;
    assert!(!fixture.poll().await?);
    let third = fixture.quiescences.try_recv()?;
    DesktopCoordinatorEngineQuitFixture::quiesced(third)?;
    assert!(fixture.poll().await?);
    assert_eq!(fixture.engine.calls.load(Ordering::Acquire), 3);
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn apply_export_consumes_the_opaque_destination_once_and_replay_keeps_its_receipt()
-> DesktopCoordinatorEngineQuitTestResult {
    let directory = tempfile::tempdir()?;
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let handle = fixture
        .selection(
            &directory.path().join("diagnostic.json"),
            fixture.context.owner(),
            DesktopFileEnumPurpose::WriteFile,
        )
        .await?;
    let native = fixture.request(fixture.export(handle)?)?;
    let receipt = fixture.port.try_submit(&native)?;
    assert!(!fixture.poll().await?);
    let actual = fixture.requests.try_recv()?;
    assert!(matches!(
        &actual.request.selection,
        Some(DesktopFileEnumSelection::Destination(_))
    ));
    DesktopCoordinatorEngineQuitFixture::cancel(actual)?;
    assert!(!fixture.poll().await?);
    assert!(receipt.shares_outcome(&fixture.port.try_submit(&native)?));
    assert!(!fixture.poll().await?);
    assert!(fixture.requests.try_recv().is_err());
    let reused = fixture.submit(fixture.export(handle)?)?;
    assert!(!fixture.poll().await?);
    DesktopCoordinatorEngineQuitFixture::refused(&reused)?;
    assert!(fixture.requests.try_recv().is_err());
    let quit = fixture.quit()?;
    assert!(fixture.poll().await?);
    assert!(matches!(
        quit.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn export_wrong_owner_and_purpose_do_not_consume_the_authoritative_selection()
-> DesktopCoordinatorEngineQuitTestResult {
    let directory = tempfile::tempdir()?;
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let owner = fixture.context.owner();
    let handle = fixture
        .selection(
            &directory.path().join("diagnostic.json"),
            owner,
            DesktopFileEnumPurpose::WriteFile,
        )
        .await?;
    let foreign = DesktopEngineCommandsContext::new(
        DesktopMessagesEntityId::new(),
        fixture.context.generation(),
        fixture.context.configuration(),
        fixture.context.selection(),
    );
    let request = fixture.port.request(DesktopCommand::new(
        DesktopEnumCommand::Engine(DesktopEngineCommandsOperation::new(
            foreign,
            fixture.export(handle)?,
        )),
        Some(DesktopRevision::ZERO),
    ))?;
    let refused = fixture.port.try_submit(&request)?;
    assert!(!fixture.poll().await?);
    DesktopCoordinatorEngineQuitFixture::refused(&refused)?;
    assert!(fixture.requests.try_recv().is_err());
    let accepted = fixture.submit(fixture.export(handle)?)?;
    assert!(!fixture.poll().await?);
    let actual = fixture.requests.try_recv()?;
    assert!(matches!(
        &actual.request.selection,
        Some(DesktopFileEnumSelection::Destination(_))
    ));
    DesktopCoordinatorEngineQuitFixture::cancel(actual)?;
    assert!(!fixture.poll().await?);
    assert!(accepted.response().is_some());
    let source = fixture
        .selection(directory.path(), owner, DesktopFileEnumPurpose::Directory)
        .await?;
    let wrong = fixture.submit(fixture.export(source)?)?;
    assert!(!fixture.poll().await?);
    DesktopCoordinatorEngineQuitFixture::refused(&wrong)?;
    assert!(fixture.requests.try_recv().is_err());
    let prepared = fixture
        .submit(DesktopEngineCommandsEnumOperation::PrepareTablebases { directory: source })?;
    assert!(!fixture.poll().await?);
    let actual = fixture.requests.try_recv()?;
    assert!(matches!(
        &actual.request.selection,
        Some(DesktopFileEnumSelection::Directory { .. })
    ));
    DesktopCoordinatorEngineQuitFixture::cancel(actual)?;
    assert!(!fixture.poll().await?);
    assert!(prepared.response().is_some());
    fixture.quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn expired_export_destination_is_refused_before_any_engine_admission()
-> DesktopCoordinatorEngineQuitTestResult {
    let directory = tempfile::tempdir()?;
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let handle = fixture
        .selection(
            &directory.path().join("diagnostic.json"),
            fixture.context.owner(),
            DesktopFileEnumPurpose::WriteFile,
        )
        .await?;
    fixture.clock.0.store(600_000_000_000, Ordering::Release);
    let refused = fixture.submit(fixture.export(handle)?)?;
    assert!(!fixture.poll().await?);
    DesktopCoordinatorEngineQuitFixture::refused(&refused)?;
    assert!(fixture.requests.try_recv().is_err());
    fixture.quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn preview_export_needs_no_selection_even_while_the_registry_is_owned_elsewhere()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let files = fixture
        .coordinator
        .files
        .take()
        .ok_or("missing native files owner")?;
    let preview = fixture.submit(DesktopEngineCommandsEnumOperation::Workbench(
        DesktopWorkbenchCommandsEnumOperation::PreviewExport {
            job: WorkbenchJobId::try_from(uuid::Uuid::from_u128(2))?,
        },
    ))?;
    assert!(!fixture.poll().await?);
    let actual = fixture.requests.try_recv()?;
    assert!(actual.request.selection.is_none());
    DesktopCoordinatorEngineQuitFixture::cancel(actual)?;
    assert!(!fixture.poll().await?);
    assert!(preview.response().is_some());
    fixture.coordinator.files = Some(files);
    fixture.quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn root_cleanup_retry_gets_a_fresh_shared_deadline_after_engine_quiescence_was_observed()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let (_, request) = fixture.pending().await?;
    fixture.lifecycle.fail_next.store(true, Ordering::Release);
    let quit = fixture.quit()?;
    assert!(!fixture.poll().await?);
    let quiescence = fixture.quiescences.try_recv()?;
    let first = quiescence.deadline;
    DesktopCoordinatorEngineQuitFixture::quiesced(quiescence)?;
    DesktopCoordinatorEngineQuitFixture::cancel(request)?;
    assert!(!fixture.poll().await?);
    assert_eq!(fixture.lifecycle_deadlines()?, [first]);
    assert!(quit.pending_notice()?.is_some());
    assert!(quit.response().is_none());
    tokio::time::advance(Duration::from_millis(100)).await;
    fixture.port.retry_quit()?;
    let fresh = Instant::now() + DesktopCoordinatorEngineQuitFixture::WINDOW;
    assert!(fixture.poll().await?);
    assert_eq!(fixture.lifecycle_deadlines()?, [first, fresh]);
    assert_eq!(fixture.engine.calls.load(Ordering::Acquire), 1);
    assert!(matches!(
        quit.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn queued_quit_without_pending_engine_uses_its_original_shared_deadline()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let quit = fixture.quit()?;
    let deadline = quit.runtime_received_at() + DesktopCoordinatorEngineQuitFixture::WINDOW;
    tokio::time::advance(Duration::from_secs(2)).await;
    assert!(fixture.poll().await?);
    assert_eq!(fixture.lifecycle_deadlines()?, [deadline]);
    assert_eq!(fixture.engine.calls.load(Ordering::Acquire), 0);
    assert!(matches!(
        quit.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn queued_quit_without_an_engine_provider_preserves_its_original_deadline()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    fixture.coordinator.engine = None;
    let quit = fixture.quit()?;
    let deadline = quit.runtime_received_at() + DesktopCoordinatorEngineQuitFixture::WINDOW;
    tokio::time::advance(Duration::from_secs(2)).await;
    assert!(fixture.poll().await?);
    assert_eq!(fixture.lifecycle_deadlines()?, [deadline]);
    assert_eq!(fixture.engine.calls.load(Ordering::Acquire), 0);
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn expired_quit_without_engine_work_requires_a_fresh_explicit_retry()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    fixture.coordinator.engine = None;
    let quit = fixture.quit()?;
    tokio::time::advance(DesktopCoordinatorEngineQuitFixture::WINDOW).await;
    assert!(!fixture.poll().await?);
    assert!(fixture.lifecycle_deadlines()?.is_empty());
    assert!(quit.response().is_none());
    assert!(quit.pending_notice()?.is_some());
    fixture.port.retry_quit()?;
    let fresh = Instant::now() + DesktopCoordinatorEngineQuitFixture::WINDOW;
    assert!(fixture.poll().await?);
    assert_eq!(fixture.lifecycle_deadlines()?, [fresh]);
    assert_eq!(fixture.engine.calls.load(Ordering::Acquire), 0);
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn waiting_for_a_native_dialog_does_not_restart_the_quit_window()
-> DesktopCoordinatorEngineQuitTestResult {
    let mut fixture = DesktopCoordinatorEngineQuitFixture::new()?;
    let selected = fixture
        .port
        .try_submit(&fixture.port.request(DesktopCommand::new(
            DesktopEnumCommand::FileSelect {
                owner: DesktopMessagesEntityId::new(),
                purpose: DesktopFileEnumPurpose::ReadFile,
            },
            Some(DesktopRevision::ZERO),
        ))?)?;
    let work = fixture
        .coordinator
        .application
        .try_next()?
        .ok_or("missing native selection request")?;
    let files = fixture
        .coordinator
        .files
        .take()
        .ok_or("missing native file owner")?;
    let (closed, closing) = oneshot::channel::<()>();
    fixture.coordinator.pending_selection =
        Some(crate::desktop::coordinator::DesktopCoordinatorSelection {
            work,
            selection: Box::pin(async move {
                let outcome = closing
                    .await
                    .map(|()| None)
                    .map_err(|error| DesktopFileHandleError::Io(std::io::Error::other(error)));
                (files, outcome)
            }),
        });
    let quit = fixture.quit()?;
    let deadline = quit.runtime_received_at() + DesktopCoordinatorEngineQuitFixture::WINDOW;
    assert!(!fixture.poll().await?);
    assert!(fixture.lifecycle_deadlines()?.is_empty());
    tokio::time::advance(Duration::from_secs(2)).await;
    closed
        .send(())
        .map_err(|()| "native dialog completion was abandoned")?;
    assert!(fixture.poll().await?);
    assert_eq!(fixture.lifecycle_deadlines()?, [deadline]);
    assert!(matches!(
        selected.response(),
        Some(DesktopMessagesEnumResult::Cancelled)
    ));
    assert_eq!(fixture.engine.calls.load(Ordering::Acquire), 0);
    fixture.close().await
}
