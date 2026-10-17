use crate::storage_sqlite_inspector::StorageSqliteInspector;
use std::future::{Future, poll_fn};
use std::num::{NonZeroU64, NonZeroUsize};
use std::pin::Pin;
use std::sync::{
    Arc, Condvar, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use tokio::sync::Notify;
use tokio::time::Instant;

use crate::board::position::BoardPosition;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::{CapacityExecutor, CapacityExecutorError, CapacityExecutorJob};
use crate::clock::duration::{ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationReceipt, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider,
};
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand};
use crate::desktop::native::puzzle_runtime::DesktopNativePuzzleRuntimeScheduler;
use crate::desktop::native::puzzle_service::{
    DesktopNativePuzzleService, DesktopNativePuzzleServiceEnumCleanup,
    DesktopNativePuzzleServiceOptions, DesktopNativePuzzleServicePort,
    DesktopNativePuzzleServiceShutdown,
};
use crate::desktop::puzzle_bridge::{
    DesktopPuzzleBridgeEnumOperation, DesktopPuzzleBridgeEnumResult, DesktopPuzzleBridgeError,
};
use crate::desktop::puzzle_commands::DesktopPuzzleCommands;
use crate::desktop::puzzle_service::{
    DesktopPuzzleServiceEnumActivity, DesktopPuzzleServiceEnumOperation,
    DesktopPuzzleServiceEnumResult, DesktopPuzzleServiceError, DesktopPuzzleServiceOutcome,
    DesktopPuzzleServiceProvider, DesktopPuzzleServiceReceipt, DesktopPuzzleServiceRequest,
};
use crate::desktop::puzzle_snapshot::DesktopPuzzleSnapshot;
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::PuzzleEnumCountdownMode;
use crate::puzzle::history::{PuzzleHistoryEnumConfirmation, PuzzleHistoryEnumScope};
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::metadata::PuzzleDefinitionMetadata;
use crate::puzzle::persistence::{
    PuzzlePersistenceEnumCommand, PuzzlePersistenceRun, PuzzlePersistenceStorageRevision,
};
use crate::puzzle::random_provider::PuzzleRandomProviderSeeded;
use crate::puzzle::run::{PuzzleRunEnumConfirmation, PuzzleRunEnumStatus};
use crate::puzzle::selection::PuzzleSelectionFilter;
use crate::puzzle::session::{PuzzleSessionConfig, PuzzleSessionContext, PuzzleSessionId};
use crate::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness};
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::puzzle_repository::StoragePuzzleRepository;

type DesktopNativePuzzleServiceTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct DesktopNativePuzzleServiceFixtureClock {
    nanos: AtomicU64,
    changed: Notify,
    failing: AtomicBool,
    preparation_admission:
        Mutex<Option<Arc<DesktopNativePuzzleServiceFixturePreparationAdmission>>>,
}

impl ClockTimeProvider for DesktopNativePuzzleServiceFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        if self.failing.load(Ordering::Acquire) {
            return Err(ClockDurationError::Overflow);
        }
        let admission = self
            .preparation_admission
            .lock()
            .map_err(|_poisoned| ClockDurationError::Overflow)?
            .take();
        if let Some(admission) = admission {
            admission
                .occupy()
                .map_err(|_poisoned| ClockDurationError::Overflow)?;
        }
        Ok(ClockInstant::from_nanos(self.nanos.load(Ordering::Acquire)))
    }
}

#[derive(Default)]
struct DesktopNativePuzzleServiceFixtureWake(AtomicUsize);

impl DesktopApplicationWakeProvider for DesktopNativePuzzleServiceFixtureWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[derive(Clone)]
struct DesktopNativePuzzleServiceFixtureScheduler(Arc<DesktopNativePuzzleServiceFixtureClock>);

impl DesktopNativePuzzleRuntimeScheduler for DesktopNativePuzzleServiceFixtureScheduler {
    async fn wait_until(&self, deadline: ClockInstant) -> Result<(), ClockDurationError> {
        loop {
            let changed = self.0.changed.notified();
            if self.0.now()? >= deadline {
                return Ok(());
            }
            changed.await;
        }
    }
}

struct DesktopNativePuzzleServiceFixture {
    directory: tempfile::TempDir,
    storage: StorageDocumentStore,
    repository: StoragePuzzleRepository,
    budget: CapacityBudget,
    executor: Arc<CapacityExecutor>,
    clock: Arc<DesktopNativePuzzleServiceFixtureClock>,
    wake: Arc<DesktopNativePuzzleServiceFixtureWake>,
    service: DesktopNativePuzzleService<
        DesktopNativePuzzleServiceFixtureClock,
        DesktopNativePuzzleServiceFixtureWake,
        DesktopNativePuzzleServiceFixtureScheduler,
    >,
    port: DesktopNativePuzzleServicePort,
}

impl DesktopNativePuzzleServiceFixture {
    const TIMER_DRIVER_TICK: Duration = Duration::from_millis(1);

    async fn new() -> DesktopNativePuzzleServiceTestResult<Self> {
        Self::with_started_service(true).await
    }

    async fn with_started_service(started: bool) -> DesktopNativePuzzleServiceTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("invalid fixture capacity")?)?;
        let storage =
            StorageDocumentStore::open(directory.path().join("puzzles.sqlite3"), budget.clone())
                .await?;
        let repository = StoragePuzzleRepository::new(storage.client());
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let clock = Arc::new(DesktopNativePuzzleServiceFixtureClock::default());
        let wake = Arc::new(DesktopNativePuzzleServiceFixtureWake::default());
        let mut service = DesktopNativePuzzleService::new(DesktopNativePuzzleServiceOptions {
            repository: repository.clone(),
            executor: Arc::clone(&executor),
            clock: Arc::clone(&clock),
            wake: Arc::clone(&wake),
            runtime: tokio::runtime::Handle::current(),
            scheduler: DesktopNativePuzzleServiceFixtureScheduler(Arc::clone(&clock)),
            replacement_timeout: Duration::from_secs(3),
        });
        let port = if started {
            service.start()?
        } else {
            service.port.clone()
        };
        Ok(Self {
            directory,
            storage,
            repository,
            budget,
            executor,
            clock,
            wake,
            service,
            port,
        })
    }

    fn id(value: u64) -> DesktopNativePuzzleServiceTestResult<PuzzleSessionId> {
        Ok(PuzzleSessionId::new(
            NonZeroU64::new(value).ok_or("zero fixture id")?,
        ))
    }

    fn receipt(&self) -> DesktopNativePuzzleServiceTestResult<DesktopApplicationReceipt> {
        let application = DesktopApplication::new(Arc::clone(&self.clock), Arc::clone(&self.wake));
        let port = application.port();
        let request = port.request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
        Ok(port.try_submit(&request)?)
    }

    async fn execute(
        &self,
        operation: DesktopPuzzleServiceEnumOperation,
    ) -> DesktopNativePuzzleServiceTestResult<DesktopPuzzleServiceEnumResult> {
        Ok(self
            .port
            .execute(DesktopPuzzleServiceRequest {
                receipt: self.receipt()?,
                operation,
            })
            .await?)
    }

    async fn seed(
        &self,
        id: PuzzleSessionId,
        ended: bool,
    ) -> DesktopNativePuzzleServiceTestResult<PuzzleSessionContext> {
        let initial = BoardPosition::standard();
        let first = initial.parse_uci("e2e4")?;
        let second = initial.play(first)?.parse_uci("e7e5")?;
        let definitions = ["first", "second"]
            .into_iter()
            .map(|name| {
                Ok(PuzzleDefinition::new(
                    PuzzleDefinitionIdentity::new(
                        "recovery-fixture".into(),
                        "v1".into(),
                        name.into(),
                    )?,
                    initial.clone(),
                    vec![vec![first, second]],
                    PuzzleDefinitionMetadata::default(),
                )?)
            })
            .collect::<DesktopNativePuzzleServiceTestResult<Vec<_>>>()?;
        let mut run = PuzzlePersistenceRun::new(
            id,
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            definitions,
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(81),
        )?;
        if ended {
            run.execute(
                PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Confirmed),
                self.clock.now()?,
            )?;
        }
        let mut owner = DesktopPuzzleCommands::new(
            run,
            self.repository.clone(),
            self.clock.now()?,
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?;
        owner.begin_write(self.clock.now()?)?;
        owner.wait().await?;
        owner.settle(self.clock.now()?)?;
        Ok(owner.context())
    }

    async fn open(
        &self,
        id: PuzzleSessionId,
    ) -> DesktopNativePuzzleServiceTestResult<Arc<DesktopPuzzleSnapshot>> {
        match self
            .execute(DesktopPuzzleServiceEnumOperation::OpenSaved {
                id,
                storage_revision: PuzzlePersistenceStorageRevision::new(1),
            })
            .await?
        {
            DesktopPuzzleServiceEnumResult::Opened(snapshot) => Ok(snapshot),
            DesktopPuzzleServiceEnumResult::Inventory(_)
            | DesktopPuzzleServiceEnumResult::Ended { .. }
            | DesktopPuzzleServiceEnumResult::HistoryDeleted { .. }
            | DesktopPuzzleServiceEnumResult::Session(_) => Err("expected an opened run".into()),
        }
    }

    async fn durable(&self) -> DesktopNativePuzzleServiceTestResult<Arc<DesktopPuzzleSnapshot>> {
        let mut state = self.port.state();
        let state = state
            .wait_for(|state| {
                state.snapshot().is_some_and(|snapshot| {
                    !snapshot.has_retained_write()
                        && !snapshot.has_unsaved_state()
                        && snapshot.durable_context() == Some(snapshot.context())
                })
            })
            .await?;
        state
            .snapshot()
            .cloned()
            .ok_or_else(|| "missing durable snapshot".into())
    }

    async fn session(
        &self,
        expected: PuzzleSessionContext,
        operation: DesktopPuzzleBridgeEnumOperation,
    ) -> DesktopNativePuzzleServiceTestResult<DesktopPuzzleBridgeEnumResult> {
        match self
            .execute(DesktopPuzzleServiceEnumOperation::Session {
                expected,
                operation,
            })
            .await?
        {
            DesktopPuzzleServiceEnumResult::Session(result) => Ok(result),
            DesktopPuzzleServiceEnumResult::Inventory(_)
            | DesktopPuzzleServiceEnumResult::Opened(_)
            | DesktopPuzzleServiceEnumResult::HistoryDeleted { .. }
            | DesktopPuzzleServiceEnumResult::Ended { .. } => {
                Err("expected a session result".into())
            }
        }
    }

    async fn close(mut self) -> DesktopNativePuzzleServiceTestResult {
        self.service
            .shutdown_retained(Instant::now() + Duration::from_secs(3))
            .await?;
        assert!(self.service.is_shutdown_complete());
        self.storage
            .close_retained(Instant::now() + Duration::from_secs(3))
            .await?;
        assert!(self.storage.is_closed());
        drop(self.service);
        Arc::try_unwrap(self.executor)
            .map_err(|_executor| "executor remained retained")?
            .shutdown()
            .await?;
        Ok(())
    }

    async fn assert_pending<F: Future>(mut future: Pin<&mut F>) {
        poll_fn(|context| {
            assert!(future.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
    }
}

#[tokio::test]
async fn idle_inventory_keeps_no_placeholder_session_and_open_requires_explicit_resume()
-> DesktopNativePuzzleServiceTestResult {
    let fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let id = DesktopNativePuzzleServiceFixture::id(9)?;
    fixture.seed(id, false).await?;
    assert_eq!(fixture.port.state().borrow().selected(), None);
    let DesktopPuzzleServiceEnumResult::Inventory(page) = fixture
        .execute(DesktopPuzzleServiceEnumOperation::Inventory { after: None })
        .await?
    else {
        return Err("missing inventory".into());
    };
    assert_eq!(page.entries().len(), 1);
    assert_eq!(page.entries()[0].id(), id);
    assert_eq!(fixture.port.state().borrow().selected(), None);
    let opened = fixture.open(id).await?;
    assert!(matches!(
        opened.status(),
        PuzzleRunEnumStatus::Paused {
            reason: PuzzleTimerEnumPauseReason::Recovery,
            ..
        }
    ));
    assert!(opened.attempt().is_none());
    let durable = fixture.durable().await?;
    assert!(matches!(
        durable.status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    let resumed = fixture
        .session(
            durable.context().clone(),
            DesktopPuzzleBridgeEnumOperation::Command(PuzzlePersistenceEnumCommand::Resume(
                PuzzleTimerEnumReadiness::Ready,
            )),
        )
        .await?;
    assert_eq!(resumed.snapshot().status(), PuzzleRunEnumStatus::Active);
    assert!(resumed.snapshot().attempt().is_some());
    assert!(fixture.wake.0.load(Ordering::Acquire) > 0);
    fixture.close().await
}

#[tokio::test]
async fn history_deletion_needs_confirmation_and_refuses_the_selected_run()
-> DesktopNativePuzzleServiceTestResult {
    let fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let first = DesktopNativePuzzleServiceFixture::id(1)?;
    let second = DesktopNativePuzzleServiceFixture::id(2)?;
    fixture.seed(first, false).await?;
    fixture.seed(second, false).await?;
    let delete = |scope, confirmation| DesktopPuzzleServiceEnumOperation::DeleteHistory {
        scope,
        confirmation,
    };
    let refused = fixture
        .execute(delete(
            PuzzleHistoryEnumScope::Session(second),
            PuzzleHistoryEnumConfirmation::Unconfirmed,
        ))
        .await;
    assert!(refused.is_err());
    fixture.open(first).await?;
    assert!(
        fixture
            .execute(delete(
                PuzzleHistoryEnumScope::Session(first),
                PuzzleHistoryEnumConfirmation::Confirmed,
            ))
            .await
            .is_err()
    );
    assert!(
        fixture
            .execute(delete(
                PuzzleHistoryEnumScope::All,
                PuzzleHistoryEnumConfirmation::Confirmed,
            ))
            .await
            .is_err()
    );
    let DesktopPuzzleServiceEnumResult::HistoryDeleted { removed } = fixture
        .execute(delete(
            PuzzleHistoryEnumScope::Session(second),
            PuzzleHistoryEnumConfirmation::Confirmed,
        ))
        .await?
    else {
        return Err("missing deletion result".into());
    };
    assert_eq!(removed, 1);
    let DesktopPuzzleServiceEnumResult::Inventory(page) = fixture
        .execute(DesktopPuzzleServiceEnumOperation::Inventory { after: None })
        .await?
    else {
        return Err("missing inventory".into());
    };
    assert_eq!(page.entries().len(), 1);
    fixture.close().await
}

#[tokio::test]
async fn stale_revision_and_missing_run_refuse_without_selecting_an_owner()
-> DesktopNativePuzzleServiceTestResult {
    let fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    fixture.seed(id, false).await?;
    let mut receipt = fixture.port.submit(DesktopPuzzleServiceRequest {
        receipt: fixture.receipt()?,
        operation: DesktopPuzzleServiceEnumOperation::OpenSaved {
            id,
            storage_revision: PuzzlePersistenceStorageRevision::new(8),
        },
    })?;
    assert!(matches!(
        &*receipt
            .wait()
            .await
            .err()
            .ok_or("stale revision accepted")?,
        DesktopPuzzleServiceError::RevisionConflict { .. }
    ));
    assert_eq!(fixture.port.state().borrow().selected(), None);
    let absent = DesktopNativePuzzleServiceFixture::id(2)?;
    assert!(fixture.open(absent).await.is_err());
    fixture.close().await
}

#[tokio::test]
async fn ended_run_is_classified_without_a_runtime_or_recovery_write()
-> DesktopNativePuzzleServiceTestResult {
    let fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    fixture.seed(id, true).await?;
    assert!(
        matches!(fixture.execute(DesktopPuzzleServiceEnumOperation::OpenSaved { id, storage_revision: PuzzlePersistenceStorageRevision::new(1) }).await?, DesktopPuzzleServiceEnumResult::Ended { id: ended, .. } if ended == id)
    );
    assert_eq!(fixture.port.state().borrow().selected(), None);
    assert_eq!(
        fixture
            .repository
            .load(id)
            .await?
            .ok_or("missing ended run")?
            .storage_revision()
            .value(),
        1
    );
    fixture.close().await
}

#[tokio::test]
async fn corrupt_checkpoint_is_refused_even_when_metadata_inventory_succeeds()
-> DesktopNativePuzzleServiceTestResult {
    let fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    fixture.seed(id, false).await?;
    let mut connection =
        StorageSqliteInspector::open(fixture.directory.path().join("puzzles.sqlite3"))?;
    connection.execute("UPDATE puzzle_checkpoints SET body = '{}' WHERE run_id = '1'")?;
    assert_eq!(fixture.repository.inventory(None).await?.entries().len(), 1);
    assert!(fixture.open(id).await.is_err());
    assert_eq!(fixture.port.state().borrow().selected(), None);
    fixture.close().await
}

#[tokio::test]
async fn cancelled_load_remains_owned_until_the_real_repository_reply_is_observed()
-> DesktopNativePuzzleServiceTestResult {
    let mut fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    fixture.seed(id, false).await?;
    let lease = fixture.budget.try_lease(fixture.budget.workers())?;
    let application = fixture.receipt()?;
    let mut opened = fixture.port.submit(DesktopPuzzleServiceRequest {
        receipt: application.clone(),
        operation: DesktopPuzzleServiceEnumOperation::OpenSaved {
            id,
            storage_revision: PuzzlePersistenceStorageRevision::new(1),
        },
    })?;
    let mut state = fixture.port.state();
    drop(
        state
            .wait_for(|state| {
                matches!(
                    state.activity(),
                    DesktopPuzzleServiceEnumActivity::Loading { .. }
                )
            })
            .await?,
    );
    application.cancellation().request();
    let mut waiting = Box::pin(opened.wait());
    DesktopNativePuzzleServiceFixture::assert_pending(waiting.as_mut()).await;
    drop(waiting);
    assert!(
        fixture
            .service
            .shutdown_retained(Instant::now())
            .await
            .is_err()
    );
    assert!(!fixture.service.is_shutdown_complete());
    assert!(!fixture.storage.is_closed());
    drop(lease);
    assert!(matches!(
        &*opened
            .wait()
            .await
            .err()
            .ok_or("cancelled load succeeded")?,
        DesktopPuzzleServiceError::Cancelled
    ));
    fixture.close().await
}

#[tokio::test]
async fn replacement_rejects_active_owner_and_old_readonly_commands_never_reach_replacement()
-> DesktopNativePuzzleServiceTestResult {
    let fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let first = DesktopNativePuzzleServiceFixture::id(1)?;
    let second = DesktopNativePuzzleServiceFixture::id(2)?;
    fixture.seed(first, false).await?;
    fixture.seed(second, false).await?;
    fixture.open(first).await?;
    let saved = fixture.durable().await?;
    let active = fixture
        .session(
            saved.context().clone(),
            DesktopPuzzleBridgeEnumOperation::Command(PuzzlePersistenceEnumCommand::Resume(
                PuzzleTimerEnumReadiness::Ready,
            )),
        )
        .await?;
    assert!(fixture.open(second).await.is_err());
    assert_eq!(fixture.port.state().borrow().selected(), Some(first));
    fixture
        .port
        .interrupt(first, PuzzleTimerEnumPauseReason::Sleep)?
        .wait()
        .await?;
    fixture.durable().await?;
    fixture.open(second).await?;
    for operation in [
        DesktopPuzzleBridgeEnumOperation::Snapshot,
        DesktopPuzzleBridgeEnumOperation::Summary,
    ] {
        assert!(
            fixture
                .session(active.snapshot().context().clone(), operation)
                .await
                .is_err()
        );
    }
    assert!(
        fixture
            .port
            .interrupt(first, PuzzleTimerEnumPauseReason::WindowHidden)?
            .wait()
            .await
            .is_err()
    );
    assert_eq!(fixture.port.state().borrow().selected(), Some(second));
    fixture.close().await
}

#[tokio::test]
async fn expired_shutdown_keeps_retry_snapshot_summary_and_bound_interruption_available()
-> DesktopNativePuzzleServiceTestResult {
    let mut fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    fixture.seed(id, false).await?;
    fixture.open(id).await?;
    let snapshot = fixture.durable().await?;
    assert!(
        fixture
            .service
            .shutdown_retained(Instant::now())
            .await
            .is_err()
    );
    assert!(!fixture.service.is_shutdown_complete());
    for operation in [
        DesktopPuzzleBridgeEnumOperation::Snapshot,
        DesktopPuzzleBridgeEnumOperation::Summary,
    ] {
        fixture
            .session(snapshot.context().clone(), operation)
            .await?;
    }
    let mut retry = fixture.port.submit(DesktopPuzzleServiceRequest {
        receipt: fixture.receipt()?,
        operation: DesktopPuzzleServiceEnumOperation::Session {
            expected: snapshot.context().clone(),
            operation: DesktopPuzzleBridgeEnumOperation::RetrySave,
        },
    })?;
    assert!(
        matches!(&*retry.wait().await.err().ok_or("unnecessary retry unexpectedly succeeded")?, DesktopPuzzleServiceError::Runtime(error) if matches!(&**error, DesktopPuzzleBridgeError::Commands(_)))
    );
    fixture
        .port
        .interrupt(id, PuzzleTimerEnumPauseReason::Sleep)?
        .wait()
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn failed_initial_save_retains_owner_and_cancellation_never_discards_admitted_work()
-> DesktopNativePuzzleServiceTestResult {
    let mut fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    let replacement = DesktopNativePuzzleServiceFixture::id(2)?;
    fixture.seed(id, false).await?;
    fixture.seed(replacement, false).await?;
    let mut connection =
        StorageSqliteInspector::open(fixture.directory.path().join("puzzles.sqlite3"))?;
    connection.batch("CREATE TRIGGER reject_recovery BEFORE UPDATE ON puzzle_checkpoints BEGIN SELECT RAISE(ABORT, 'controlled recovery failure'); END;")?;
    let application = fixture.receipt()?;
    let mut opened = fixture.port.submit(DesktopPuzzleServiceRequest {
        receipt: application.clone(),
        operation: DesktopPuzzleServiceEnumOperation::OpenSaved {
            id,
            storage_revision: PuzzlePersistenceStorageRevision::new(1),
        },
    })?;
    let mut observer = opened.clone();
    opened.wait().await?;
    application.cancellation().request();
    drop(opened);
    assert!(matches!(
        observer.wait().await?,
        DesktopPuzzleServiceEnumResult::Opened(_)
    ));
    let mut state = fixture.port.state();
    let blocked = state
        .wait_for(|state| {
            state.snapshot().is_some_and(|snapshot| {
                matches!(
                    snapshot.checkpoint_status(),
                    crate::puzzle::checkpoint::PuzzleCheckpointEnumStatus::RetryRequired { .. }
                )
            })
        })
        .await?
        .snapshot()
        .cloned()
        .ok_or("missing blocked owner")?;
    assert!(
        fixture
            .service
            .shutdown_retained(Instant::now() + Duration::from_secs(3))
            .await
            .is_err()
    );
    assert!(!fixture.service.is_shutdown_complete());
    assert_eq!(fixture.port.state().borrow().selected(), Some(id));
    assert!(fixture.open(replacement).await.is_err());
    assert_eq!(fixture.port.state().borrow().selected(), Some(id));
    connection.batch("DROP TRIGGER reject_recovery;")?;
    fixture
        .session(
            blocked.context().clone(),
            DesktopPuzzleBridgeEnumOperation::RetrySave,
        )
        .await?;
    fixture.durable().await?;
    fixture.close().await
}

#[tokio::test]
async fn cancelled_shutdown_wait_preserves_original_deadline_until_observed()
-> DesktopNativePuzzleServiceTestResult {
    let mut fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    fixture.seed(id, false).await?;
    let lease = fixture.budget.try_lease(fixture.budget.workers())?;
    let mut opened = fixture.port.submit(DesktopPuzzleServiceRequest {
        receipt: fixture.receipt()?,
        operation: DesktopPuzzleServiceEnumOperation::OpenSaved {
            id,
            storage_revision: PuzzlePersistenceStorageRevision::new(1),
        },
    })?;
    let mut state = fixture.port.state();
    drop(
        state
            .wait_for(|state| {
                matches!(
                    state.activity(),
                    DesktopPuzzleServiceEnumActivity::Loading { .. }
                )
            })
            .await?,
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut shutdown = Box::pin(fixture.service.shutdown_retained(deadline));
    DesktopNativePuzzleServiceFixture::assert_pending(shutdown.as_mut()).await;
    drop(shutdown);
    assert_eq!(fixture.service.cleanup_deadline(), Some(deadline));
    let mut resumed = Box::pin(
        fixture
            .service
            .shutdown_retained(deadline + Duration::from_secs(30)),
    );
    DesktopNativePuzzleServiceFixture::assert_pending(resumed.as_mut()).await;
    drop(resumed);
    assert_eq!(fixture.service.cleanup_deadline(), Some(deadline));
    assert!(!fixture.storage.is_closed());
    drop(lease);
    assert!(opened.wait().await.is_err());
    fixture.close().await
}

#[tokio::test]
async fn application_interrupt_uses_current_context_only_within_its_original_session()
-> DesktopNativePuzzleServiceTestResult {
    let fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    fixture.seed(id, false).await?;
    fixture.open(id).await?;
    let saved = fixture.durable().await?;
    fixture
        .session(
            saved.context().clone(),
            DesktopPuzzleBridgeEnumOperation::Command(PuzzlePersistenceEnumCommand::Resume(
                PuzzleTimerEnumReadiness::Ready,
            )),
        )
        .await?;
    let result = fixture
        .execute(DesktopPuzzleServiceEnumOperation::Interrupt {
            session: id,
            reason: PuzzleTimerEnumPauseReason::FocusLost,
        })
        .await?;
    let DesktopPuzzleServiceEnumResult::Session(result) = result else {
        return Err("missing interrupted snapshot".into());
    };
    assert!(matches!(
        result.snapshot().status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    assert!(result.snapshot().attempt().is_none());
    fixture.close().await
}

#[tokio::test]
async fn cancellation_after_write_admission_retains_exact_owner_until_real_commit_is_observed()
-> DesktopNativePuzzleServiceTestResult {
    let mut fixture = DesktopNativePuzzleServiceFixture::with_started_service(false).await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    fixture.seed(id, false).await?;
    let recovered = fixture
        .repository
        .load(id)
        .await?
        .ok_or("missing saved fixture")?;
    let lease = fixture.budget.try_lease(fixture.budget.workers())?;
    let application = fixture.receipt()?;
    let prepared = crate::desktop::native::puzzle_service::DesktopNativePuzzleServiceOwner::<
        DesktopNativePuzzleServiceFixtureClock,
        DesktopNativePuzzleServiceFixtureWake,
        DesktopNativePuzzleServiceFixtureScheduler,
    >::prepare(
        recovered,
        fixture.repository.clone(),
        fixture.clock.now()?,
        &application.cancellation(),
    )?;
    application.cancellation().request();
    let (response, mut receipt) =
        crate::desktop::puzzle_service::DesktopPuzzleServiceReceipt::channel();
    let envelope = crate::desktop::native::puzzle_service::DesktopNativePuzzleServiceRequest {
        request: DesktopPuzzleServiceRequest {
            receipt: application,
            operation: DesktopPuzzleServiceEnumOperation::OpenSaved {
                id,
                storage_revision: PuzzlePersistenceStorageRevision::new(1),
            },
        },
        response,
    };
    let owner = fixture
        .service
        .startup
        .as_mut()
        .ok_or("missing unstarted owner")?;
    owner.recovery = Some(Box::pin(async move { (envelope, Ok(prepared)) }));
    fixture.service.start()?;
    assert!(matches!(
        &*receipt
            .wait()
            .await
            .err()
            .ok_or("cancelled write request succeeded")?,
        DesktopPuzzleServiceError::Cancelled
    ));
    let mut state = fixture.port.state();
    let snapshot = state
        .wait_for(|state| state.snapshot().is_some())
        .await?
        .snapshot()
        .cloned()
        .ok_or("missing retained owner")?;
    assert!(snapshot.has_retained_write());
    assert_eq!(snapshot.context().session_id(), id);
    assert_eq!(fixture.port.state().borrow().selected(), Some(id));
    drop(receipt);
    assert!(
        fixture
            .service
            .shutdown_retained(Instant::now())
            .await
            .is_err()
    );
    assert!(!fixture.service.is_shutdown_complete());
    assert!(!fixture.storage.is_closed());
    drop(lease);
    fixture.durable().await?;
    assert!(
        fixture
            .repository
            .load(id)
            .await?
            .ok_or("lost admitted write")?
            .storage_revision()
            .value()
            >= 2
    );
    fixture.close().await
}

#[tokio::test]
async fn failed_runtime_worker_returns_original_owner_and_later_bound_request_repairs_driving()
-> DesktopNativePuzzleServiceTestResult {
    let fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    fixture.seed(id, false).await?;
    fixture.open(id).await?;
    let before = fixture.durable().await?;
    let application = fixture.receipt()?;
    fixture.clock.failing.store(true, Ordering::Release);
    let mut failed = fixture.port.submit(DesktopPuzzleServiceRequest {
        receipt: application,
        operation: DesktopPuzzleServiceEnumOperation::Session {
            expected: before.context().clone(),
            operation: DesktopPuzzleBridgeEnumOperation::Snapshot,
        },
    })?;
    assert!(failed.wait().await.is_err());
    assert!(!fixture.service.is_shutdown_complete());
    assert_eq!(fixture.port.state().borrow().selected(), Some(id));
    let mut state = fixture.port.state();
    drop(
        state
            .wait_for(|state| {
                matches!(
                    state.health(),
                    Some(crate::desktop::puzzle_bridge::DesktopPuzzleBridgeEnumHealth::Failed(_))
                )
            })
            .await?,
    );
    fixture.clock.failing.store(false, Ordering::Release);
    let repaired = fixture
        .port
        .interrupt(id, PuzzleTimerEnumPauseReason::Sleep)?
        .wait()
        .await?;
    let DesktopPuzzleServiceEnumResult::Session(repaired) = repaired else {
        return Err("missing repaired owner".into());
    };
    assert_eq!(repaired.snapshot().context(), before.context());
    assert_eq!(
        repaired.snapshot().timer().elapsed(),
        before.timer().elapsed()
    );
    assert!(repaired.snapshot().attempt().is_none());
    fixture
        .session(
            before.context().clone(),
            DesktopPuzzleBridgeEnumOperation::Summary,
        )
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn saturated_ordinary_queue_preserves_separate_bounded_urgent_and_native_capacity()
-> DesktopNativePuzzleServiceTestResult {
    let mut fixture = DesktopNativePuzzleServiceFixture::with_started_service(false).await?;
    let application = fixture.receipt()?;
    let mut ordinary = Vec::new();
    for _entry in 0..DesktopNativePuzzleServicePort::ORDINARY_CAPACITY {
        ordinary.push(fixture.port.submit(DesktopPuzzleServiceRequest {
            receipt: application.clone(),
            operation: DesktopPuzzleServiceEnumOperation::Inventory { after: None },
        })?);
    }
    assert!(matches!(
        &*fixture
            .port
            .submit(DesktopPuzzleServiceRequest {
                receipt: application.clone(),
                operation: DesktopPuzzleServiceEnumOperation::Inventory { after: None }
            })
            .err()
            .ok_or("ordinary queue exceeded capacity")?,
        DesktopPuzzleServiceError::Busy
    ));
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    let mut urgent = Vec::new();
    for _entry in 0..DesktopNativePuzzleServicePort::URGENT_CAPACITY {
        urgent.push(fixture.port.submit(DesktopPuzzleServiceRequest {
            receipt: application.clone(),
            operation: DesktopPuzzleServiceEnumOperation::Interrupt {
                session: id,
                reason: PuzzleTimerEnumPauseReason::Sleep,
            },
        })?);
    }
    assert!(matches!(
        &*fixture
            .port
            .submit(DesktopPuzzleServiceRequest {
                receipt: application,
                operation: DesktopPuzzleServiceEnumOperation::Interrupt {
                    session: id,
                    reason: PuzzleTimerEnumPauseReason::Sleep
                }
            })
            .err()
            .ok_or("urgent queue exceeded capacity")?,
        DesktopPuzzleServiceError::Busy
    ));
    let mut native = fixture
        .port
        .interrupt(id, PuzzleTimerEnumPauseReason::Sleep)?;
    assert!(matches!(
        &*fixture
            .port
            .interrupt(id, PuzzleTimerEnumPauseReason::Sleep)
            .err()
            .ok_or("native queue exceeded capacity")?,
        DesktopPuzzleServiceError::Busy
    ));
    fixture.service.start()?;
    assert!(matches!(
        &*native
            .wait()
            .await
            .err()
            .ok_or("idle native interruption succeeded")?,
        DesktopPuzzleServiceError::SessionMismatch { selected: None, .. }
    ));
    for mut receipt in urgent {
        assert!(matches!(
            &*receipt
                .wait()
                .await
                .err()
                .ok_or("idle urgent interruption succeeded")?,
            DesktopPuzzleServiceError::SessionMismatch { selected: None, .. }
        ));
    }
    for mut receipt in ordinary {
        match receipt.wait().await {
            Ok(DesktopPuzzleServiceEnumResult::Inventory(page)) => {
                assert!(page.entries().is_empty())
            }
            Err(error) if matches!(&*error, DesktopPuzzleServiceError::Busy) => {}
            Ok(
                DesktopPuzzleServiceEnumResult::Opened(_)
                | DesktopPuzzleServiceEnumResult::Ended { .. }
                | DesktopPuzzleServiceEnumResult::HistoryDeleted { .. }
                | DesktopPuzzleServiceEnumResult::Session(_),
            )
            | Err(_) => return Err("unexpected inventory admission outcome".into()),
        }
    }
    fixture.close().await
}

#[tokio::test]
async fn cancelled_runtime_cleanup_keeps_original_deadline_and_later_retry_observes_late_commit()
-> DesktopNativePuzzleServiceTestResult {
    let mut fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    fixture.seed(id, false).await?;
    fixture.open(id).await?;
    let before = fixture.durable().await?;
    let lease = fixture.budget.try_lease(fixture.budget.workers())?;
    let active = fixture
        .session(
            before.context().clone(),
            DesktopPuzzleBridgeEnumOperation::Command(PuzzlePersistenceEnumCommand::Resume(
                PuzzleTimerEnumReadiness::Ready,
            )),
        )
        .await?;
    assert!(active.snapshot().has_retained_write());
    let original = Instant::now() + Duration::from_secs(3);
    let later = original + Duration::from_secs(30);
    let mut shutdown = Box::pin(fixture.service.shutdown_retained(original));
    DesktopNativePuzzleServiceFixture::assert_pending(shutdown.as_mut()).await;
    drop(shutdown);
    let mut state = fixture.port.state();
    drop(
        tokio::time::timeout_at(
            original,
            state.wait_for(|state| {
                matches!(
                    state.health(),
                    Some(
                        crate::desktop::puzzle_bridge::DesktopPuzzleBridgeEnumHealth::ShuttingDown
                    )
                )
            }),
        )
        .await
        .map_err(|_elapsed| "runtime cleanup did not publish its closing state")??,
    );
    tokio::time::pause();
    let mut resumed = Box::pin(fixture.service.shutdown_retained(later));
    DesktopNativePuzzleServiceFixture::assert_pending(resumed.as_mut()).await;
    drop(resumed);
    assert_eq!(fixture.service.cleanup_deadline(), Some(original));
    tokio::time::advance(Duration::from_secs(3)).await;
    assert!(Instant::now() >= original);
    assert_eq!(fixture.service.cleanup_deadline(), Some(original));
    tokio::time::advance(DesktopNativePuzzleServiceFixture::TIMER_DRIVER_TICK).await;
    assert!(fixture.service.shutdown_retained(later).await.is_err());
    assert!(!fixture.service.is_shutdown_complete());
    assert!(!fixture.storage.is_closed());
    fixture
        .session(
            active.snapshot().context().clone(),
            DesktopPuzzleBridgeEnumOperation::Snapshot,
        )
        .await?;
    fixture
        .session(
            active.snapshot().context().clone(),
            DesktopPuzzleBridgeEnumOperation::Summary,
        )
        .await?;
    fixture
        .port
        .interrupt(id, PuzzleTimerEnumPauseReason::Sleep)?
        .wait()
        .await?;
    tokio::time::resume();
    drop(lease);
    let durable = fixture.durable().await?;
    assert!(matches!(
        durable.status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    assert_eq!(durable.durable_context(), Some(durable.context()));
    fixture.close().await
}

#[tokio::test]
async fn nonblocking_receipt_observes_retained_success_failure_and_typed_closed_channel()
-> DesktopNativePuzzleServiceTestResult {
    let (success_sender, mut success) =
        crate::desktop::puzzle_service::DesktopPuzzleServiceReceipt::<()>::channel();
    assert!(success.try_result().is_none());
    let mut success_observer = success.clone();
    success_sender.send_replace(Some(Ok(())));
    drop(success_sender);
    assert!(matches!(success.try_result(), Some(Ok(()))));
    assert!(matches!(success.try_result(), Some(Ok(()))));
    success_observer.wait().await?;
    let (failure_sender, mut failure) =
        crate::desktop::puzzle_service::DesktopPuzzleServiceReceipt::<()>::channel();
    let cause = Arc::new(DesktopPuzzleServiceError::Busy);
    failure_sender.send_replace(Some(Err(Arc::clone(&cause))));
    drop(failure_sender);
    let observed = failure
        .try_result()
        .ok_or("failure receipt remained pending")?
        .err()
        .ok_or("failure receipt succeeded")?;
    assert!(Arc::ptr_eq(&observed, &cause));
    assert!(Arc::ptr_eq(
        &failure.wait().await.err().ok_or("failure wait succeeded")?,
        &cause
    ));
    let (closed_sender, mut closed) =
        crate::desktop::puzzle_service::DesktopPuzzleServiceReceipt::<()>::channel();
    let mut closed_observer = closed.clone();
    drop(closed_sender);
    let closed = closed
        .try_result()
        .ok_or("closed receipt remained pending")?
        .err()
        .ok_or("closed receipt succeeded")?;
    assert!(matches!(&*closed, DesktopPuzzleServiceError::Outcome(_)));
    assert!(std::error::Error::source(&*closed).is_some());
    assert!(matches!(
        &*closed_observer
            .wait()
            .await
            .err()
            .ok_or("closed wait succeeded")?,
        DesktopPuzzleServiceError::Outcome(_)
    ));
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum DesktopNativePuzzleServiceFixtureControlError {
    #[error("The controlled puzzle service test synchronization state is poisoned")]
    Poisoned,
}

#[derive(Default)]
struct DesktopNativePuzzleServiceFixtureExecutorGate {
    released: Mutex<bool>,
    changed: Condvar,
}

impl DesktopNativePuzzleServiceFixtureExecutorGate {
    fn wait(&self) -> Result<(), DesktopNativePuzzleServiceFixtureControlError> {
        let mut released = self
            .released
            .lock()
            .map_err(|_poisoned| DesktopNativePuzzleServiceFixtureControlError::Poisoned)?;
        while !*released {
            released = self
                .changed
                .wait(released)
                .map_err(|_poisoned| DesktopNativePuzzleServiceFixtureControlError::Poisoned)?;
        }
        Ok(())
    }

    fn release(&self) -> Result<(), DesktopNativePuzzleServiceFixtureControlError> {
        *self
            .released
            .lock()
            .map_err(|_poisoned| DesktopNativePuzzleServiceFixtureControlError::Poisoned)? = true;
        self.changed.notify_all();
        Ok(())
    }
}

type DesktopNativePuzzleServiceFixtureAdmissionJob = Result<
    CapacityExecutorJob<Result<(), DesktopNativePuzzleServiceFixtureControlError>>,
    CapacityExecutorError,
>;

struct DesktopNativePuzzleServiceFixturePreparationAdmission {
    executor: Arc<CapacityExecutor>,
    workers: usize,
    gate: Arc<DesktopNativePuzzleServiceFixtureExecutorGate>,
    jobs: Mutex<Vec<DesktopNativePuzzleServiceFixtureAdmissionJob>>,
    occupied: AtomicBool,
    changed: Notify,
}

impl DesktopNativePuzzleServiceFixturePreparationAdmission {
    fn new(executor: Arc<CapacityExecutor>, workers: usize) -> Self {
        Self {
            executor,
            workers,
            gate: Arc::new(DesktopNativePuzzleServiceFixtureExecutorGate::default()),
            jobs: Mutex::new(Vec::new()),
            occupied: AtomicBool::new(false),
            changed: Notify::new(),
        }
    }

    fn occupy(&self) -> Result<(), DesktopNativePuzzleServiceFixtureControlError> {
        let mut jobs = self
            .jobs
            .lock()
            .map_err(|_poisoned| DesktopNativePuzzleServiceFixtureControlError::Poisoned)?;
        for _worker in 0..self.workers {
            let gate = Arc::clone(&self.gate);
            jobs.push(self.executor.submit(move |_cancellation| gate.wait()));
        }
        self.occupied.store(true, Ordering::Release);
        self.changed.notify_waiters();
        Ok(())
    }

    async fn wait_until_occupied(&self) {
        loop {
            let changed = self.changed.notified();
            if self.occupied.load(Ordering::Acquire) {
                return;
            }
            changed.await;
        }
    }

    async fn release_and_observe(&self) -> DesktopNativePuzzleServiceTestResult {
        let jobs = {
            let mut jobs = self
                .jobs
                .lock()
                .map_err(|_poisoned| DesktopNativePuzzleServiceFixtureControlError::Poisoned)?;
            std::mem::take(&mut *jobs)
        };
        self.gate.release()?;
        for job in jobs {
            job?.join().await??;
        }
        Ok(())
    }
}

struct DesktopNativePuzzleServiceFixtureShutdownRetryWake {
    port: DesktopNativePuzzleServicePort,
    deadline: Instant,
    receipt: Mutex<DesktopPuzzleServiceReceipt<()>>,
    observed: OnceLock<DesktopPuzzleServiceOutcome<()>>,
    retry: OnceLock<DesktopPuzzleServiceOutcome<DesktopPuzzleServiceReceipt<()>>>,
    attempted: AtomicBool,
    invalid: AtomicBool,
}

impl DesktopNativePuzzleServiceFixtureShutdownRetryWake {
    fn observe_and_retry(&self) {
        let outcome = match self.receipt.lock() {
            Ok(mut receipt) => receipt.try_result(),
            Err(_poisoned) => {
                self.invalid.store(true, Ordering::Release);
                return;
            }
        };
        if let Some(outcome) = outcome
            && outcome.is_err()
            && self
                .attempted
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            if self.observed.set(outcome).is_err() {
                self.invalid.store(true, Ordering::Release);
            }
            if self
                .retry
                .set(self.port.request_shutdown(self.deadline))
                .is_err()
            {
                self.invalid.store(true, Ordering::Release);
            }
        }
    }
}

impl Wake for DesktopNativePuzzleServiceFixtureShutdownRetryWake {
    fn wake(self: Arc<Self>) {
        self.observe_and_retry();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.observe_and_retry();
    }
}

#[tokio::test]
async fn failed_shutdown_notification_releases_reservation_before_synchronous_retry()
-> DesktopNativePuzzleServiceTestResult {
    let mut fixture = DesktopNativePuzzleServiceFixture::with_started_service(false).await?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut receipt = fixture.port.request_shutdown(deadline)?;
    let request = fixture
        .service
        .startup
        .as_mut()
        .ok_or("missing startup owner")?
        .shutdowns
        .try_recv()?;
    let observer = Arc::new(DesktopNativePuzzleServiceFixtureShutdownRetryWake {
        port: fixture.port.clone(),
        deadline,
        receipt: Mutex::new(receipt.clone()),
        observed: OnceLock::new(),
        retry: OnceLock::new(),
        attempted: AtomicBool::new(false),
        invalid: AtomicBool::new(false),
    });
    let waker = Waker::from(Arc::clone(&observer));
    let mut waiting = Box::pin(receipt.wait());
    let pending = waiting
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending();
    let cause = Arc::new(DesktopPuzzleServiceError::ShutdownDeadline);
    DesktopNativePuzzleServiceEnumCleanup::Shutdown(request).refuse(Arc::clone(&cause));
    drop(waiting);
    let mut drained = 0;
    while let Ok(request) = fixture
        .service
        .startup
        .as_mut()
        .ok_or("missing retry owner")?
        .shutdowns
        .try_recv()
    {
        let DesktopNativePuzzleServiceShutdown {
            response, _slot, ..
        } = request;
        drop(_slot);
        response.send_replace(Some(Ok(())));
        drained += 1;
    }
    let reservation_released = !fixture.port.shutdown_pending.load(Ordering::Acquire);
    fixture.close().await?;
    assert!(pending);
    assert!(!observer.invalid.load(Ordering::Acquire));
    assert!(matches!(observer.observed.get(), Some(Err(error)) if Arc::ptr_eq(error, &cause)));
    assert!(
        matches!(observer.retry.get(), Some(Ok(_))),
        "observed failure did not release shutdown retry admission"
    );
    assert_eq!(drained, 1);
    assert!(reservation_released);
    let mut retry = observer
        .retry
        .get()
        .ok_or("synchronous retry was not attempted")?
        .as_ref()
        .map_err(Arc::clone)?
        .clone();
    assert!(matches!(retry.try_result(), Some(Ok(()))));
    Ok(())
}

#[tokio::test]
async fn cancellation_while_recovery_preparation_waits_for_executor_never_admits_write()
-> DesktopNativePuzzleServiceTestResult {
    let fixture = DesktopNativePuzzleServiceFixture::new().await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    fixture.seed(id, false).await?;
    let before = fixture
        .repository
        .load(id)
        .await?
        .ok_or("missing persisted run")?
        .storage_revision();
    let application = fixture.receipt()?;
    let admission = Arc::new(DesktopNativePuzzleServiceFixturePreparationAdmission::new(
        Arc::clone(&fixture.executor),
        fixture.budget.workers(),
    ));
    *fixture
        .clock
        .preparation_admission
        .lock()
        .map_err(|_poisoned| DesktopNativePuzzleServiceFixtureControlError::Poisoned)? =
        Some(Arc::clone(&admission));
    let mut opened = fixture.port.submit(DesktopPuzzleServiceRequest {
        receipt: application.clone(),
        operation: DesktopPuzzleServiceEnumOperation::OpenSaved {
            id,
            storage_revision: before,
        },
    })?;
    admission.wait_until_occupied().await;
    application.cancellation().request();
    admission.release_and_observe().await?;
    let outcome = opened.wait().await;
    let selected = fixture.port.state().borrow().selected();
    let after = fixture
        .repository
        .load(id)
        .await?
        .ok_or("persisted run disappeared")?
        .storage_revision();
    drop(admission);
    fixture.close().await?;
    assert!(
        matches!(outcome, Err(error) if matches!(&*error, DesktopPuzzleServiceError::Cancelled))
    );
    assert_eq!(
        after, before,
        "cancelled executor-waiting recovery admitted a storage write"
    );
    assert_eq!(selected, None);
    Ok(())
}

#[tokio::test]
async fn completed_native_reply_releases_lane_before_newly_queued_interruption()
-> DesktopNativePuzzleServiceTestResult {
    use crate::desktop::native::puzzle_runtime::DesktopNativePuzzleRuntime;
    use crate::desktop::native::puzzle_service::{
        DesktopNativePuzzleServiceEnumReplyLane, DesktopNativePuzzleServiceInterruption,
    };
    use crate::desktop::puzzle_bridge::DesktopPuzzleBridgeProvider;

    let mut fixture = DesktopNativePuzzleServiceFixture::with_started_service(false).await?;
    let id = DesktopNativePuzzleServiceFixture::id(1)?;
    fixture.seed(id, false).await?;
    let recovered = fixture
        .repository
        .load(id)
        .await?
        .ok_or("missing saved fixture")?;
    let storage_revision = recovered.storage_revision();
    let persisted_event_count = recovered.persisted_event_count();
    let mut commands = DesktopPuzzleCommands::new(
        recovered.into_owner(),
        fixture.repository.clone(),
        fixture.clock.now()?,
        storage_revision,
        persisted_event_count,
    )?;
    commands.begin_write(fixture.clock.now()?)?;
    commands.wait().await?;
    commands.settle(fixture.clock.now()?)?;
    let (runtime, runtime_port) = DesktopNativePuzzleRuntime::new(
        commands,
        Arc::clone(&fixture.clock),
        Arc::clone(&fixture.wake),
        DesktopNativePuzzleServiceFixtureScheduler(Arc::clone(&fixture.clock)),
    );
    let mut snapshots = runtime_port.snapshots();
    let owner = fixture
        .service
        .startup
        .as_mut()
        .ok_or("missing unstarted owner")?;
    owner.snapshots = runtime_port.snapshots();
    owner.health = runtime_port.health();
    owner.port = Some(runtime_port.clone());
    owner.retained = Some(Box::new(runtime));
    owner.activity = DesktopPuzzleServiceEnumActivity::Selected;
    owner.ensure_runtime();
    drop(snapshots.wait_for(Option::is_some).await?);

    let admitted = runtime_port.interrupt(PuzzleTimerEnumPauseReason::Sleep)?;
    let mut observer = admitted.clone();
    let (response, mut previous) = DesktopPuzzleServiceReceipt::channel();
    owner.retain_reply(
        DesktopNativePuzzleServiceEnumReplyLane::Native,
        response,
        Ok(admitted),
    );
    let ready = observer.wait().await;
    owner.observe_snapshot();
    let fresh = fixture.port.state().borrow().snapshot().cloned();
    let before_release = fixture
        .port
        .state()
        .borrow()
        .native_admission_progress()
        .cloned()
        .ok_or("missing native admission identity")?;
    let retained_before_interruption = owner.replies.len();
    let (blocked_response, mut blocked) = DesktopPuzzleServiceReceipt::channel();
    owner.dispatch_interruption(DesktopNativePuzzleServiceInterruption {
        session: id,
        reason: PuzzleTimerEnumPauseReason::WindowHidden,
        response: blocked_response,
    });
    let blocked_outcome = blocked.wait().await;
    let busy_progress = fixture
        .port
        .state()
        .borrow()
        .native_admission_progress()
        .cloned()
        .ok_or("missing retained native admission identity")?;
    let mut interruption = fixture
        .port
        .interrupt(id, PuzzleTimerEnumPauseReason::WindowHidden)?;
    let outcome = {
        let mut driving = Box::pin(tokio::task::unconstrained(owner.run()));
        tokio::select! {
            biased;
            outcome = interruption.wait() => Ok(outcome),
            result = &mut driving => Err(result),
        }
    };
    fixture.service.start()?;
    let completion = previous.wait().await;
    let selected = fixture.port.state().borrow().selected();
    let after_release = fixture
        .port
        .state()
        .borrow()
        .native_admission_progress()
        .cloned()
        .ok_or("missing released native admission identity")?;
    fixture.close().await?;

    let ready = ready?;
    let fresh = fresh.ok_or("missing fresh runtime snapshot")?;
    assert!(Arc::ptr_eq(ready.snapshot(), &fresh));
    assert_eq!(retained_before_interruption, 1);
    assert_eq!(selected, Some(id));
    assert!(
        matches!(blocked_outcome, Err(error) if matches!(*error, DesktopPuzzleServiceError::Busy))
    );
    assert!(before_release.matches(&busy_progress));
    assert!(!before_release.matches(&after_release));
    assert!(matches!(
        completion,
        Ok(DesktopPuzzleServiceEnumResult::Session(result))
            if Arc::ptr_eq(result.snapshot(), ready.snapshot())
    ));
    assert!(
        matches!(
            &outcome,
            Ok(Ok(DesktopPuzzleServiceEnumResult::Session(result)))
                if result.snapshot().context().session_id() == id
                    && !Arc::ptr_eq(result.snapshot(), &fresh)
        ),
        "completed native lane refused a newly queued interruption: {outcome:?}"
    );
    Ok(())
}
