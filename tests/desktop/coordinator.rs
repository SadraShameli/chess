use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::{ClockDurationError, ClockInstant};
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::application::{
    DesktopApplication, DesktopApplicationPort, DesktopApplicationReceipt,
    DesktopApplicationWakeError, DesktopApplicationWakeProvider,
};
use chess::desktop::coordinator::{
    DesktopCoordinator, DesktopCoordinatorDialogProvider, DesktopCoordinatorError,
    DesktopCoordinatorLifecycleProvider, DesktopCoordinatorServices,
};
use chess::desktop::dto::{DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot, DesktopEnumValue};
use chess::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileHandleError,
};
use chess::desktop::identity::DesktopRevision;
use chess::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
};
use tokio::sync::{Notify, Semaphore};
use tokio::time::Instant;

type DesktopCoordinatorTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[path = "coordinator_games.rs"]
mod games;

struct DesktopCoordinatorClock;

impl ClockTimeProvider for DesktopCoordinatorClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(0))
    }
}

#[derive(Default)]
struct DesktopCoordinatorWake(Notify);

impl DesktopApplicationWakeProvider for DesktopCoordinatorWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        self.0.notify_one();
        Ok(())
    }
}

struct DesktopCoordinatorDialog {
    opened: Notify,
    cancelled: Notify,
    closed: Semaphore,
    selections: AtomicUsize,
    cancellations: AtomicUsize,
    path: PathBuf,
}

impl DesktopFileDialogProvider for DesktopCoordinatorDialog {
    async fn select(
        &self,
        _purpose: DesktopFileEnumPurpose,
    ) -> Result<Option<PathBuf>, DesktopFileHandleError> {
        self.selections.fetch_add(1, Ordering::AcqRel);
        self.opened.notify_one();
        self.closed
            .acquire()
            .await
            .map_err(|source| DesktopFileHandleError::Io(std::io::Error::other(source)))?
            .forget();
        Ok(Some(self.path.clone()))
    }
}

impl DesktopCoordinatorDialogProvider for DesktopCoordinatorDialog {
    fn request_cancel(&self) -> Result<(), DesktopFileHandleError> {
        self.cancellations.fetch_add(1, Ordering::AcqRel);
        self.cancelled.notify_one();
        Ok(())
    }
}

#[derive(Default)]
struct DesktopCoordinatorShutdownState {
    started: Notify,
    allow: Notify,
    finished: AtomicBool,
    fail_next: AtomicBool,
    attempts: AtomicUsize,
}

struct DesktopCoordinatorLifecycle(Arc<DesktopCoordinatorShutdownState>);

impl DesktopCoordinatorLifecycleProvider for DesktopCoordinatorLifecycle {
    async fn shutdown(&mut self, deadline: Instant) -> Result<(), DesktopCoordinatorError> {
        assert!(deadline > Instant::now());
        self.0.attempts.fetch_add(1, Ordering::AcqRel);
        self.0.started.notify_one();
        self.0.allow.notified().await;
        if self.0.fail_next.swap(false, Ordering::AcqRel) {
            return Err(DesktopCoordinatorError::Shutdown {
                source: Box::new(std::io::Error::other("retained cleanup requires retry")),
            });
        }
        self.0.finished.store(true, Ordering::Release);
        Ok(())
    }
}

#[tokio::test]
async fn failed_cleanup_keeps_quit_pending_and_the_actor_owned() -> DesktopCoordinatorTestResult {
    let fixture = DesktopCoordinatorFixture::new(PathBuf::new())?;
    fixture.shutdown.fail_next.store(true, Ordering::Release);
    let quit = fixture.submit(DesktopEnumCommand::Quit)?;
    let mut coordinator = fixture.coordinator;
    let mut task = tokio::spawn(async move {
        let result = coordinator.run().await;
        (coordinator, result)
    });
    fixture.shutdown.started.notified().await;
    fixture.shutdown.allow.notify_one();
    loop {
        tokio::select! {
            outcome = &mut task => {
                let (_coordinator, result) = outcome?;
                result?;
                return Err("the actor exited with unobserved cleanup".into());
            }
            () = fixture.wake.0.notified() => {
                fixture.port.acknowledge_wake();
                if quit.pending_notice()?.is_some() {
                    break;
                }
            }
        }
    }
    assert!(quit.response().is_none());
    assert_eq!(fixture.shutdown.attempts.load(Ordering::Acquire), 1);
    fixture.port.retry_quit()?;
    fixture.shutdown.started.notified().await;
    assert_eq!(fixture.shutdown.attempts.load(Ordering::Acquire), 2);
    fixture.shutdown.allow.notify_one();
    let (coordinator, result) = task.await?;
    result?;
    assert!(coordinator.shutdown_failure().is_none());
    assert!(matches!(
        quit.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    Ok(())
}

struct DesktopCoordinatorFixture {
    port: DesktopApplicationPort<DesktopCoordinatorClock, DesktopCoordinatorWake>,
    coordinator: DesktopCoordinator<
        DesktopCoordinatorClock,
        DesktopCoordinatorWake,
        DesktopCoordinatorDialog,
        DesktopCoordinatorLifecycle,
    >,
    provider: Arc<DesktopCoordinatorDialog>,
    wake: Arc<DesktopCoordinatorWake>,
    shutdown: Arc<DesktopCoordinatorShutdownState>,
}

impl DesktopCoordinatorFixture {
    fn new(path: PathBuf) -> DesktopCoordinatorTestResult<Self> {
        let clock = Arc::new(DesktopCoordinatorClock);
        let wake = Arc::new(DesktopCoordinatorWake::default());
        let application = DesktopApplication::new(Arc::clone(&clock), Arc::clone(&wake));
        let provider = Arc::new(DesktopCoordinatorDialog {
            opened: Notify::new(),
            cancelled: Notify::new(),
            closed: Semaphore::new(0),
            selections: AtomicUsize::new(0),
            cancellations: AtomicUsize::new(0),
            path,
        });
        let shutdown = Arc::new(DesktopCoordinatorShutdownState::default());
        let executor = Arc::new(CapacityExecutor::new(CapacityBudget::new(
            NonZeroUsize::MIN,
        )?)?);
        let snapshot = DesktopBootstrapSnapshot::new(
            &BoardPosition::standard(),
            std::num::NonZeroU32::MIN,
            DesktopEnumEngineSnapshot::Unavailable {
                message: "No engine in actor fixture".to_owned(),
            },
        );
        let port = application.port();
        let coordinator = DesktopCoordinator::new(
            application,
            clock,
            snapshot,
            DesktopCoordinatorServices::new(
                Arc::clone(&provider),
                executor,
                DesktopCoordinatorLifecycle(Arc::clone(&shutdown)),
            ),
        );
        Ok(Self {
            port,
            coordinator,
            provider,
            wake,
            shutdown,
        })
    }

    fn submit(
        &self,
        operation: DesktopEnumCommand,
    ) -> DesktopCoordinatorTestResult<DesktopApplicationReceipt> {
        let revision = operation
            .mutation_entity()
            .map(|_owner| DesktopRevision::ZERO);
        Ok(self.port.try_submit(
            &self
                .port
                .request(DesktopCommand::new(operation, revision))?,
        )?)
    }

    async fn response(
        port: &DesktopApplicationPort<DesktopCoordinatorClock, DesktopCoordinatorWake>,
        wake: &DesktopCoordinatorWake,
        receipt: &DesktopApplicationReceipt,
    ) -> DesktopMessagesEnumResult {
        loop {
            port.acknowledge_wake();
            if let Some(result) = receipt.response() {
                return result.clone();
            }
            wake.0.notified().await;
        }
    }
}

#[tokio::test]
async fn a_pending_native_selection_does_not_block_bootstrap_or_shutdown_intent()
-> DesktopCoordinatorTestResult {
    let file = tempfile::NamedTempFile::new()?;
    let fixture = DesktopCoordinatorFixture::new(file.path().to_path_buf())?;
    let owner = DesktopMessagesEntityId::new();
    let selection = fixture.submit(DesktopEnumCommand::FileSelect {
        owner,
        purpose: DesktopFileEnumPurpose::ReadFile,
    })?;
    let mut coordinator = fixture.coordinator;
    let task = tokio::spawn(async move {
        let result = coordinator.run().await;
        (coordinator, result)
    });
    fixture.provider.opened.notified().await;
    let request = fixture
        .port
        .request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
    let bootstrap = fixture.port.try_submit(&request)?;
    assert!(matches!(
        DesktopCoordinatorFixture::response(&fixture.port, &fixture.wake, &bootstrap).await,
        DesktopMessagesEnumResult::Value {
            value: DesktopEnumValue::Bootstrap(_),
            ..
        }
    ));
    let quit = fixture.port.try_submit(
        &fixture
            .port
            .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
    )?;
    let marker = fixture.port.try_submit(
        &fixture
            .port
            .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
    )?;
    assert!(quit.shares_outcome(&marker));
    fixture.provider.cancelled.notified().await;
    assert!(quit.response().is_none());
    assert!(!fixture.shutdown.finished.load(Ordering::Acquire));
    fixture.provider.closed.add_permits(1);
    fixture.shutdown.started.notified().await;
    assert!(quit.response().is_none());
    assert!(matches!(
        selection.response(),
        Some(DesktopMessagesEnumResult::Cancelled)
    ));
    fixture.shutdown.allow.notify_one();
    let (_coordinator, result) = task.await?;
    result?;
    assert!(matches!(
        quit.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    assert_eq!(fixture.provider.selections.load(Ordering::Acquire), 1);
    assert_eq!(fixture.provider.cancellations.load(Ordering::Acquire), 1);
    Ok(())
}

#[tokio::test]
async fn a_selected_file_has_one_retained_action_and_stale_revisions_never_open_a_dialog()
-> DesktopCoordinatorTestResult {
    let file = tempfile::NamedTempFile::new()?;
    let fixture = DesktopCoordinatorFixture::new(file.path().to_path_buf())?;
    let owner = DesktopMessagesEntityId::new();
    let stale = fixture
        .port
        .try_submit(&fixture.port.request(DesktopCommand::new(
            DesktopEnumCommand::FileSelect {
                owner,
                purpose: DesktopFileEnumPurpose::ReadFile,
            },
            Some(DesktopRevision::ZERO.next()?),
        ))?)?;
    let mut coordinator = fixture.coordinator;
    let task = tokio::spawn(async move {
        let result = coordinator.run().await;
        (coordinator, result)
    });
    assert!(matches!(
        DesktopCoordinatorFixture::response(&fixture.port, &fixture.wake, &stale).await,
        DesktopMessagesEnumResult::Failure(_)
    ));
    assert_eq!(fixture.provider.selections.load(Ordering::Acquire), 0);
    let request = fixture.port.request(DesktopCommand::new(
        DesktopEnumCommand::FileSelect {
            owner,
            purpose: DesktopFileEnumPurpose::ReadFile,
        },
        Some(DesktopRevision::ZERO),
    ))?;
    let selection = fixture.port.try_submit(&request)?;
    fixture.provider.opened.notified().await;
    for _ in 0..100 {
        assert!(selection.shares_outcome(&fixture.port.try_submit(&request)?));
    }
    fixture.provider.closed.add_permits(1);
    assert!(matches!(
        DesktopCoordinatorFixture::response(&fixture.port, &fixture.wake, &selection).await,
        DesktopMessagesEnumResult::Value {
            value: DesktopEnumValue::FileSelect(Some(_)),
            ..
        }
    ));
    let quit = fixture.port.try_submit(
        &fixture
            .port
            .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
    )?;
    fixture.shutdown.started.notified().await;
    fixture.shutdown.allow.notify_one();
    let (_coordinator, result) = task.await?;
    result?;
    assert!(matches!(
        quit.response(),
        Some(DesktopMessagesEnumResult::QuitReady)
    ));
    assert_eq!(fixture.provider.selections.load(Ordering::Acquire), 1);
    Ok(())
}

#[tokio::test]
async fn subscriptions_bind_acknowledgments_and_deliver_owned_native_events()
-> DesktopCoordinatorTestResult {
    let fixture = DesktopCoordinatorFixture::new(PathBuf::new())?;
    let subscribed = fixture.submit(DesktopEnumCommand::StreamSubscribe)?;
    let mut coordinator = fixture.coordinator;
    let task = tokio::spawn(async move {
        let result = coordinator.run().await;
        (coordinator, result)
    });
    let DesktopMessagesEnumResult::Value {
        value: DesktopEnumValue::StreamSubscribe {
            subscription_id, ..
        },
        ..
    } = DesktopCoordinatorFixture::response(&fixture.port, &fixture.wake, &subscribed).await
    else {
        return Err("missing native subscription".into());
    };
    let resync = fixture
        .port
        .try_submit(&fixture.port.request(DesktopCommand::new(
            DesktopEnumCommand::StreamResync { subscription_id },
            None,
        ))?)?;
    let DesktopMessagesEnumResult::Value {
        value: DesktopEnumValue::StreamResync { sequence, .. },
        ..
    } = DesktopCoordinatorFixture::response(&fixture.port, &fixture.wake, &resync).await
    else {
        return Err("missing resynchronization".into());
    };
    let event = fixture.port.try_event()?.ok_or("missing native event")?;
    assert_eq!(event.sequence(), sequence);
    assert_eq!(event.subscription_id(), subscription_id);
    assert!(fixture.port.try_event()?.is_none());
    let invalid = fixture
        .port
        .try_submit(&fixture.port.request(DesktopCommand::new(
            DesktopEnumCommand::StreamAcknowledge {
                subscription_id,
                sequence: sequence.next()?,
            },
            None,
        ))?)?;
    assert!(matches!(
        DesktopCoordinatorFixture::response(&fixture.port, &fixture.wake, &invalid).await,
        DesktopMessagesEnumResult::Failure(_)
    ));
    let ack = fixture
        .port
        .try_submit(&fixture.port.request(DesktopCommand::new(
            DesktopEnumCommand::StreamAcknowledge {
                subscription_id,
                sequence,
            },
            None,
        ))?)?;
    assert!(matches!(
        DesktopCoordinatorFixture::response(&fixture.port, &fixture.wake, &ack).await,
        DesktopMessagesEnumResult::Value {
            value: DesktopEnumValue::StreamAcknowledge,
            ..
        }
    ));
    fixture.port.try_submit(
        &fixture
            .port
            .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
    )?;
    fixture.shutdown.started.notified().await;
    fixture.shutdown.allow.notify_one();
    let (_coordinator, result) = task.await?;
    result?;
    Ok(())
}
