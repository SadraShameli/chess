use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chess::board::editor::{BoardEditorDraftError, BoardEditorDraftId};
use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::{ClockDurationError, ClockInstant};
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::application::{
    DesktopApplication, DesktopApplicationPort, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider,
};
use chess::desktop::coordinator::{
    DesktopCoordinator, DesktopCoordinatorDialogProvider, DesktopCoordinatorError,
    DesktopCoordinatorLifecycleProvider, DesktopCoordinatorServices,
};
use chess::desktop::dto::{DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot};
use chess::desktop::editor_commands::DesktopEditorCommands;
use chess::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileHandleError,
};
use chess::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId};
use chess::presentation::app::PresentationApp;
use chess::presentation::app::editor::{
    PresentationAppEditorConnection, PresentationAppEditorDraftIdProvider,
};
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::editor_repository::StorageEditorRepository;
use tokio::sync::Notify;

pub(super) type PresentationAppEditorTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) struct PresentationAppEditorFixtureClock;

impl ClockTimeProvider for PresentationAppEditorFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(100))
    }
}

#[derive(Default)]
pub(super) struct PresentationAppEditorFixtureWake(Notify);

impl DesktopApplicationWakeProvider for PresentationAppEditorFixtureWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        self.0.notify_one();
        Ok(())
    }
}

struct PresentationAppEditorFixtureIds(AtomicU64);

impl PresentationAppEditorDraftIdProvider for PresentationAppEditorFixtureIds {
    fn next_id(&self) -> Result<BoardEditorDraftId, BoardEditorDraftError> {
        BoardEditorDraftId::try_from(uuid::Uuid::from_u128(u128::from(
            self.0.fetch_add(1, Ordering::AcqRel),
        )))
    }
}

struct PresentationAppEditorFixtureDialogs;

impl DesktopFileDialogProvider for PresentationAppEditorFixtureDialogs {
    async fn select(
        &self,
        _purpose: DesktopFileEnumPurpose,
    ) -> Result<Option<PathBuf>, DesktopFileHandleError> {
        Ok(None)
    }
}

impl DesktopCoordinatorDialogProvider for PresentationAppEditorFixtureDialogs {
    fn request_cancel(&self) -> Result<(), DesktopFileHandleError> {
        Ok(())
    }
}

struct PresentationAppEditorFixtureLifecycle;

impl DesktopCoordinatorLifecycleProvider for PresentationAppEditorFixtureLifecycle {
    async fn shutdown(
        &mut self,
        _deadline: tokio::time::Instant,
    ) -> Result<(), DesktopCoordinatorError> {
        Ok(())
    }
}

pub(super) struct PresentationAppEditorFixture {
    pub(super) app: PresentationApp,
    pub(super) port:
        DesktopApplicationPort<PresentationAppEditorFixtureClock, PresentationAppEditorFixtureWake>,
    pub(super) repository: StorageEditorRepository,
    coordinator: Pin<Box<dyn Future<Output = Result<(), DesktopCoordinatorError>> + Send>>,
    wake: Arc<PresentationAppEditorFixtureWake>,
    store: StorageDocumentStore,
    directory: tempfile::TempDir,
}

impl PresentationAppEditorFixture {
    pub(super) async fn new() -> PresentationAppEditorTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::discover()?;
        let store =
            StorageDocumentStore::open(directory.path().join("editor.sqlite3"), budget.clone())
                .await?;
        let repository = StorageEditorRepository::new(store.client());
        let clock = Arc::new(PresentationAppEditorFixtureClock);
        let wake = Arc::new(PresentationAppEditorFixtureWake::default());
        let application = DesktopApplication::new(Arc::clone(&clock), Arc::clone(&wake));
        let port = application.port();
        let owner = DesktopMessagesEntityId::new();
        let mut app = PresentationApp::new(clock.clone());
        app.mount_editor(PresentationAppEditorConnection::new(
            owner,
            Arc::new(PresentationAppEditorFixtureIds(AtomicU64::new(1))),
        ));
        let snapshot = DesktopBootstrapSnapshot::new(
            &BoardPosition::standard(),
            std::num::NonZeroU32::MIN,
            DesktopEnumEngineSnapshot::Unavailable {
                message: "No engine in editor fixture".into(),
            },
        );
        let mut coordinator = DesktopCoordinator::new(
            application,
            clock,
            snapshot,
            DesktopCoordinatorServices::new(
                Arc::new(PresentationAppEditorFixtureDialogs),
                Arc::new(CapacityExecutor::uninitialized(budget)),
                PresentationAppEditorFixtureLifecycle,
            ),
        )
        .with_editor(owner, DesktopEditorCommands::new(repository.clone()));
        Ok(Self {
            app,
            port,
            repository,
            coordinator: Box::pin(async move { coordinator.run().await }),
            wake,
            store,
            directory,
        })
    }

    pub(super) async fn settle(&mut self) -> PresentationAppEditorTestResult {
        for _ in 0..8 {
            self.app.synchronize_editor(&self.port)?;
            let Some(receipt) = self
                .app
                .editor_controller()
                .and_then(|controller| controller.receipt())
                .cloned()
            else {
                return Ok(());
            };
            self.observe(&receipt).await?;
        }
        Err("editor lane did not settle".into())
    }

    pub(super) async fn observe(
        &mut self,
        receipt: &chess::desktop::application::DesktopApplicationReceipt,
    ) -> PresentationAppEditorTestResult {
        let wake = Arc::clone(&self.wake);
        tokio::select! {
            result = &mut self.coordinator => {
                result?;
                return Err("coordinator exited before observation".into());
            }
            () = async {
                loop {
                    self.port.acknowledge_wake();
                    if receipt.response().is_some() { break; }
                    wake.0.notified().await;
                }
            } => {}
        }
        Ok(())
    }

    pub(super) async fn close(self) -> PresentationAppEditorTestResult {
        let quit = self
            .port
            .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
        let receipt = self.port.try_submit(&quit)?;
        self.coordinator.await?;
        let response = receipt.response().cloned();
        drop((self.app, self.repository, self.port));
        self.store.close().await?;
        self.directory.close()?;
        if !matches!(
            response,
            Some(chess::desktop::messages::DesktopMessagesEnumResult::QuitReady)
        ) {
            return Err("coordinator did not complete owned cleanup".into());
        }
        Ok(())
    }
}
