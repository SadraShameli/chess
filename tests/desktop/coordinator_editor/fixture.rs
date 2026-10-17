use std::future::{Future, poll_fn};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::task::Poll;

use chess::board::editor::BoardEditorDraftId;
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
use chess::desktop::editor_commands::{
    DesktopEditorCommands, DesktopEditorCommandsEnumOperation, DesktopEditorCommandsEnumResult,
    DesktopEditorCommandsOpening, DesktopEditorCommandsSnapshot,
};
use chess::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileHandleError,
};
use chess::desktop::identity::DesktopRequestId;
use chess::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
    DesktopRequest,
};
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::editor_repository::StorageEditorRepository;
use tokio::sync::Notify;
use tokio::time::Instant;

pub(super) type DesktopCoordinatorEditorTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) struct DesktopCoordinatorEditorFixtureClock(AtomicU64);

impl DesktopCoordinatorEditorFixtureClock {
    pub(super) fn set(&self, nanos: u64) {
        self.0.store(nanos, Ordering::Release);
    }
}

impl ClockTimeProvider for DesktopCoordinatorEditorFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.0.load(Ordering::Acquire)))
    }
}

#[derive(Default)]
pub(super) struct DesktopCoordinatorEditorFixtureWake(Notify);

impl DesktopApplicationWakeProvider for DesktopCoordinatorEditorFixtureWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        self.0.notify_one();
        Ok(())
    }
}

struct DesktopCoordinatorEditorFixtureDialog;

impl DesktopFileDialogProvider for DesktopCoordinatorEditorFixtureDialog {
    async fn select(
        &self,
        _purpose: DesktopFileEnumPurpose,
    ) -> Result<Option<PathBuf>, DesktopFileHandleError> {
        Ok(None)
    }
}

impl DesktopCoordinatorDialogProvider for DesktopCoordinatorEditorFixtureDialog {
    fn request_cancel(&self) -> Result<(), DesktopFileHandleError> {
        Ok(())
    }
}

struct DesktopCoordinatorEditorFixtureLifecycle(Arc<AtomicUsize>);

impl DesktopCoordinatorLifecycleProvider for DesktopCoordinatorEditorFixtureLifecycle {
    async fn shutdown(&mut self, _deadline: Instant) -> Result<(), DesktopCoordinatorError> {
        self.0.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

pub(super) struct DesktopCoordinatorEditorFixtureDriver {
    future: Pin<Box<dyn Future<Output = Result<(), DesktopCoordinatorError>> + Send>>,
}

impl DesktopCoordinatorEditorFixtureDriver {
    pub(super) async fn retire_after_storage_barrier(
        mut self,
    ) -> DesktopCoordinatorEditorTestResult<bool> {
        poll_fn(|context| match self.future.as_mut().poll(context) {
            Poll::Pending => Poll::Ready(Ok(false)),
            Poll::Ready(result) => Poll::Ready(result.map(|()| true).map_err(Into::into)),
        })
        .await
    }

    pub(super) async fn poll_pending(&mut self) -> DesktopCoordinatorEditorTestResult {
        poll_fn(|context| match self.future.as_mut().poll(context) {
            Poll::Pending => Poll::Ready(Ok(())),
            Poll::Ready(result) => Poll::Ready(match result {
                Ok(()) => Err("coordinator exited before Quit".into()),
                Err(error) => Err(error.into()),
            }),
        })
        .await
    }

    pub(super) async fn response(
        &mut self,
        fixture: &DesktopCoordinatorEditorFixture,
        receipt: &DesktopApplicationReceipt,
    ) -> DesktopCoordinatorEditorTestResult<DesktopMessagesEnumResult> {
        tokio::select! {
            biased;
            result = &mut self.future => {
                result?;
                Err("coordinator exited before the requested observation".into())
            }
            response = fixture.response(receipt) => Ok(response),
        }
    }
}

pub(super) struct DesktopCoordinatorEditorFixture {
    pub(super) port: DesktopApplicationPort<
        DesktopCoordinatorEditorFixtureClock,
        DesktopCoordinatorEditorFixtureWake,
    >,
    pub(super) clock: Arc<DesktopCoordinatorEditorFixtureClock>,
    pub(super) owner: DesktopMessagesEntityId,
    pub(super) repository: StorageEditorRepository,
    pub(super) path: PathBuf,
    pub(super) shutdown: Arc<AtomicUsize>,
    wake: Arc<DesktopCoordinatorEditorFixtureWake>,
    store: StorageDocumentStore,
    directory: tempfile::TempDir,
}

impl DesktopCoordinatorEditorFixture {
    pub(super) async fn new()
    -> DesktopCoordinatorEditorTestResult<(Self, DesktopCoordinatorEditorFixtureDriver)> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("editor.sqlite3");
        let budget = CapacityBudget::discover()?;
        let store = StorageDocumentStore::open(path.clone(), budget.clone()).await?;
        let repository = StorageEditorRepository::new(store.client());
        let clock = Arc::new(DesktopCoordinatorEditorFixtureClock(AtomicU64::new(100)));
        let wake = Arc::new(DesktopCoordinatorEditorFixtureWake::default());
        let application = DesktopApplication::new(Arc::clone(&clock), Arc::clone(&wake));
        let port = application.port();
        let owner = DesktopMessagesEntityId::new();
        let shutdown = Arc::new(AtomicUsize::new(0));
        let cpu_threads = std::num::NonZeroU32::new(u32::try_from(budget.workers())?)
            .ok_or("the shared budget has no CPU capacity")?;
        let snapshot = DesktopBootstrapSnapshot::new(
            &BoardPosition::standard(),
            cpu_threads,
            DesktopEnumEngineSnapshot::Unavailable {
                message: "No engine in editor route fixture".to_owned(),
            },
        );
        let mut coordinator = DesktopCoordinator::new(
            application,
            Arc::clone(&clock),
            snapshot,
            DesktopCoordinatorServices::new(
                Arc::new(DesktopCoordinatorEditorFixtureDialog),
                Arc::new(CapacityExecutor::uninitialized(budget)),
                DesktopCoordinatorEditorFixtureLifecycle(Arc::clone(&shutdown)),
            ),
        )
        .with_editor(owner, DesktopEditorCommands::new(repository.clone()));
        let driver = DesktopCoordinatorEditorFixtureDriver {
            future: Box::pin(async move { coordinator.run().await }),
        };
        Ok((
            Self {
                port,
                clock,
                owner,
                repository,
                path,
                shutdown,
                wake,
                store,
                directory,
            },
            driver,
        ))
    }

    pub(super) fn id(value: u128) -> DesktopCoordinatorEditorTestResult<BoardEditorDraftId> {
        Ok(BoardEditorDraftId::try_from(uuid::Uuid::from_u128(value))?)
    }

    pub(super) fn opening(
        value: u128,
    ) -> DesktopCoordinatorEditorTestResult<DesktopEditorCommandsOpening> {
        let session: DesktopRequestId = uuid::Uuid::from_u128(value + 1000).to_string().parse()?;
        Ok(DesktopEditorCommandsOpening::new(Self::id(value)?, session))
    }

    pub(super) fn request(
        &self,
        operation: DesktopEditorCommandsEnumOperation,
    ) -> DesktopCoordinatorEditorTestResult<DesktopRequest> {
        Ok(self
            .port
            .prepare_action()?
            .bind(DesktopCommand::editor(self.owner, operation))?)
    }

    pub(super) fn submit(
        &self,
        command: DesktopCommand,
    ) -> DesktopCoordinatorEditorTestResult<DesktopApplicationReceipt> {
        Ok(self.port.try_submit(&self.port.request(command)?)?)
    }

    pub(super) async fn response(
        &self,
        receipt: &DesktopApplicationReceipt,
    ) -> DesktopMessagesEnumResult {
        loop {
            self.port.acknowledge_wake();
            if let Some(response) = receipt.response() {
                return response.clone();
            }
            self.wake.0.notified().await;
        }
    }

    pub(super) fn snapshot(
        response: DesktopMessagesEnumResult,
    ) -> DesktopCoordinatorEditorTestResult<DesktopEditorCommandsSnapshot> {
        if let DesktopMessagesEnumResult::Value {
            value: DesktopEnumValue::Editor(value),
            ..
        } = response
            && let DesktopEditorCommandsEnumResult::Snapshot(Some(snapshot)) = *value
        {
            Ok(snapshot)
        } else {
            Err("expected the actual editor snapshot".into())
        }
    }

    pub(super) async fn create(
        &self,
        driver: &mut DesktopCoordinatorEditorFixtureDriver,
    ) -> DesktopCoordinatorEditorTestResult<DesktopEditorCommandsSnapshot> {
        let request = self.request(DesktopEditorCommandsEnumOperation::Create {
            opening: Self::opening(1)?,
            selected: None,
            expected: None,
        })?;
        let receipt = self.port.try_submit(&request)?;
        Self::snapshot(driver.response(self, &receipt).await?)
    }

    pub(super) async fn close(
        self,
        driver: DesktopCoordinatorEditorFixtureDriver,
        quit: Option<DesktopApplicationReceipt>,
    ) -> DesktopCoordinatorEditorTestResult<DesktopMessagesEnumResult> {
        let quit = match quit {
            Some(quit) => quit,
            None => self.submit(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
        };
        driver.future.await?;
        let response = quit
            .response()
            .cloned()
            .ok_or("Quit did not publish its result")?;
        self.store.close().await?;
        self.directory.close()?;
        Ok(response)
    }

    pub(super) async fn close_storage(self) -> DesktopCoordinatorEditorTestResult {
        self.store.close().await?;
        self.directory.close()?;
        Ok(())
    }
}
