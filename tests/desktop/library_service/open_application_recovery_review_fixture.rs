use std::future::{Future, poll_fn};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::board::position::BoardPosition;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationPort, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider,
};
use crate::desktop::coordinator::{
    DesktopCoordinator, DesktopCoordinatorDialogProvider, DesktopCoordinatorError,
    DesktopCoordinatorLifecycleProvider, DesktopCoordinatorServices,
};
use crate::desktop::dto::{DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot};
use crate::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileHandleError,
};
use crate::desktop::game_bridge::DesktopGameBridgeProvider;
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestClock, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceFuture, DesktopLibraryServicePort, DesktopLibraryServiceProvider,
    DesktopLibraryServiceReceipt, DesktopLibraryServiceRequest,
};
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand};
use crate::game::enums::GameId;
use crate::game::session::GameSession;
use crate::storage::document_store::StorageDocumentStore;

pub(super) struct DesktopLibraryServiceOpenApplicationRecoveryReviewWake;

impl DesktopApplicationWakeProvider for DesktopLibraryServiceOpenApplicationRecoveryReviewWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

struct DesktopLibraryServiceOpenApplicationRecoveryReviewDialog;

impl DesktopFileDialogProvider for DesktopLibraryServiceOpenApplicationRecoveryReviewDialog {
    async fn select(
        &self,
        _purpose: DesktopFileEnumPurpose,
    ) -> Result<Option<PathBuf>, DesktopFileHandleError> {
        Err(DesktopFileHandleError::Io(std::io::Error::other(
            "unexpected open recovery dialog",
        )))
    }
}

impl DesktopCoordinatorDialogProvider for DesktopLibraryServiceOpenApplicationRecoveryReviewDialog {
    fn request_cancel(&self) -> Result<(), DesktopFileHandleError> {
        Ok(())
    }
}

struct DesktopLibraryServiceOpenApplicationRecoveryReviewLifecycle(Arc<AtomicUsize>);

impl DesktopCoordinatorLifecycleProvider
    for DesktopLibraryServiceOpenApplicationRecoveryReviewLifecycle
{
    async fn shutdown(&mut self, _deadline: Instant) -> Result<(), DesktopCoordinatorError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct DesktopLibraryServiceOpenApplicationRecoveryReviewProvider {
    port: DesktopLibraryServicePort,
    observed: mpsc::Sender<DesktopLibraryServiceReceipt>,
    dispatches: Arc<AtomicUsize>,
}

impl DesktopLibraryServiceProvider for DesktopLibraryServiceOpenApplicationRecoveryReviewProvider {
    fn execute(&self, request: DesktopLibraryServiceRequest) -> DesktopLibraryServiceFuture {
        self.dispatches.fetch_add(1, Ordering::SeqCst);
        let submitted = self.port.try_submit(request);
        let observed = self.observed.clone();
        Box::pin(async move {
            let mut receipt = submitted.map_err(Arc::new)?;
            if let Err(abandoned) = observed.send(receipt.clone()).await {
                drop(abandoned);
            }
            receipt.wait().await
        })
    }
}

pub(super) struct DesktopLibraryServiceOpenApplicationRecoveryReviewFixture {
    pub(super) library: DesktopLibraryServiceFixture,
    pub(super) application: DesktopApplicationPort<
        DesktopLibraryServiceTestClock,
        DesktopLibraryServiceOpenApplicationRecoveryReviewWake,
    >,
    pub(super) observed: mpsc::Receiver<DesktopLibraryServiceReceipt>,
    pub(super) dispatches: Arc<AtomicUsize>,
    coordinator: DesktopCoordinator<
        DesktopLibraryServiceTestClock,
        DesktopLibraryServiceOpenApplicationRecoveryReviewWake,
        DesktopLibraryServiceOpenApplicationRecoveryReviewDialog,
        DesktopLibraryServiceOpenApplicationRecoveryReviewLifecycle,
    >,
    shutdowns: Arc<AtomicUsize>,
}

impl DesktopLibraryServiceOpenApplicationRecoveryReviewFixture {
    pub(super) const DEADLINE: Duration = Duration::from_secs(3);

    pub(super) fn played() -> DesktopLibraryServiceTestResult<GameSession> {
        let mut session = DesktopLibraryServiceFixture::played(100)?;
        session.begin_analysis(
            session.revision(),
            crate::clock::duration::ClockInstant::from_nanos(50),
        )?;
        Ok(session)
    }

    pub(super) async fn new() -> DesktopLibraryServiceTestResult<Self> {
        let library = DesktopLibraryServiceFixture::new().await?;
        let clock = Arc::new(DesktopLibraryServiceTestClock);
        let application = DesktopApplication::new(
            Arc::clone(&clock),
            Arc::new(DesktopLibraryServiceOpenApplicationRecoveryReviewWake),
        );
        let port = application.port();
        let (observed, receiver) = mpsc::channel(DesktopLibraryServicePort::CAPACITY);
        let dispatches = Arc::new(AtomicUsize::new(0));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let coordinator = DesktopCoordinator::new(
            application,
            clock,
            DesktopBootstrapSnapshot::new(
                &BoardPosition::standard(),
                NonZeroU32::MIN,
                DesktopEnumEngineSnapshot::Unavailable {
                    message: "open application recovery fixture".into(),
                },
            ),
            DesktopCoordinatorServices::new(
                Arc::new(DesktopLibraryServiceOpenApplicationRecoveryReviewDialog),
                Arc::clone(&library.executor),
                DesktopLibraryServiceOpenApplicationRecoveryReviewLifecycle(Arc::clone(&shutdowns)),
            ),
        )
        .with_library(
            library.workspace,
            Arc::new(DesktopLibraryServiceOpenApplicationRecoveryReviewProvider {
                port: library.port.clone(),
                observed,
                dispatches: Arc::clone(&dispatches),
            }),
        );
        Ok(Self {
            library,
            application: port,
            observed: receiver,
            dispatches,
            coordinator,
            shutdowns,
        })
    }

    pub(super) async fn drive_until(
        &mut self,
        mut ready: impl FnMut() -> bool,
    ) -> DesktopLibraryServiceTestResult {
        let mut coordinator = Box::pin(self.coordinator.run());
        let mut service = Box::pin(self.library.service.run());
        let mut registry = Box::pin(self.library.registry.run());
        tokio::time::timeout(
            Self::DEADLINE,
            poll_fn(|context| {
                if let Poll::Ready(result) = coordinator.as_mut().poll(context) {
                    return Poll::Ready(Err(format!("coordinator ended: {result:?}").into()));
                }
                if let Poll::Ready(result) = service.as_mut().poll(context) {
                    return Poll::Ready(Err(format!("service ended: {result:?}").into()));
                }
                if let Poll::Ready(result) = registry.as_mut().poll(context) {
                    return Poll::Ready(Err(format!("registry ended: {result:?}").into()));
                }
                if ready() {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                }
            }),
        )
        .await?
    }

    pub(super) fn snapshot(
        &self,
        game: GameId,
    ) -> DesktopLibraryServiceTestResult<DesktopGameSnapshot> {
        self.library
            .games
            .snapshots()
            .borrow()
            .iter()
            .find(|snapshot| snapshot.id() == game)
            .cloned()
            .ok_or_else(|| "missing actual registry snapshot".into())
    }

    pub(super) async fn close(mut self) -> DesktopLibraryServiceTestResult<(usize, usize, usize)> {
        let quit = self.application.try_submit(
            &self
                .application
                .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
        )?;
        tokio::time::timeout(Self::DEADLINE, Box::pin(self.coordinator.run())).await??;
        let quit_completed = quit.response().is_some();
        let shutdowns = self.shutdowns.load(Ordering::SeqCst);
        let memory = self.library.store.client();
        let budget = self.library.budget.clone();
        drop((
            self.coordinator,
            self.application,
            self.observed,
            self.dispatches,
            self.shutdowns,
            quit,
        ));
        self.library.close().await?;
        let released = memory
            .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?
            .num_permits();
        let capacity = budget.available();
        drop((memory, budget));
        if !quit_completed {
            return Err("coordinator exited without completing Quit".into());
        }
        Ok((shutdowns, released, capacity))
    }
}
