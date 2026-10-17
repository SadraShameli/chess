use std::future::{Future, poll_fn};
use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::Poll;

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::{ClockDurationError, ClockInstant};
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::clock::time_provider::ClockTimeProvider;
use chess::desktop::application::{
    DesktopApplication, DesktopApplicationPort, DesktopApplicationReceipt,
    DesktopApplicationWakeError, DesktopApplicationWakeProvider,
};
use chess::desktop::coordinator::{
    DesktopCoordinator, DesktopCoordinatorDialogProvider, DesktopCoordinatorError,
    DesktopCoordinatorLifecycleProvider, DesktopCoordinatorServices,
};
use chess::desktop::dto::{DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot};
use chess::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileHandleError,
};
use chess::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorBinding, DesktopGameEditorContext, DesktopGameEditorEpoch,
    DesktopGameEditorGeneration, DesktopGameEditorPublication,
};
use chess::desktop::identity::DesktopRequestId;
use chess::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumResult, DesktopLibraryServiceError,
    DesktopLibraryServiceFuture, DesktopLibraryServiceOperation, DesktopLibraryServiceProvider,
    DesktopLibraryServiceRequest,
};
use chess::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId};
use chess::game::enums::{GameEnumMode, GameEnumSeat, GameId};
use chess::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
use chess::game::storage_repository::GameStorageRevision;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::library_query::{StorageLibraryQuery, StorageLibraryQueryOptions};
use chess::storage::library_repository::StorageLibraryRepository;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

pub(super) type DesktopLibraryRoutingTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) struct DesktopLibraryRoutingClock;

impl ClockTimeProvider for DesktopLibraryRoutingClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(17))
    }
}

#[derive(Default)]
pub(super) struct DesktopLibraryRoutingWake(pub(super) AtomicBool);

impl DesktopApplicationWakeProvider for DesktopLibraryRoutingWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        if self.0.load(Ordering::Acquire) {
            Err(DesktopApplicationWakeError::Unavailable)
        } else {
            Ok(())
        }
    }
}

pub(super) struct DesktopLibraryRoutingDialog;

impl DesktopFileDialogProvider for DesktopLibraryRoutingDialog {
    async fn select(
        &self,
        _purpose: DesktopFileEnumPurpose,
    ) -> Result<Option<PathBuf>, DesktopFileHandleError> {
        Err(DesktopFileHandleError::Io(std::io::Error::other(
            "unexpected native dialog",
        )))
    }
}

impl DesktopCoordinatorDialogProvider for DesktopLibraryRoutingDialog {
    fn request_cancel(&self) -> Result<(), DesktopFileHandleError> {
        Ok(())
    }
}

pub(super) struct DesktopLibraryRoutingLifecycle(pub(super) Arc<AtomicUsize>);

impl DesktopCoordinatorLifecycleProvider for DesktopLibraryRoutingLifecycle {
    async fn shutdown(&mut self, _deadline: Instant) -> Result<(), DesktopCoordinatorError> {
        self.0.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

pub(super) struct DesktopLibraryRoutingRequest {
    pub(super) request: DesktopLibraryServiceRequest,
    response:
        oneshot::Sender<Result<DesktopLibraryServiceEnumResult, Arc<DesktopLibraryServiceError>>>,
}

impl DesktopLibraryRoutingRequest {
    pub(super) fn finish(
        self,
        response: Result<DesktopLibraryServiceEnumResult, Arc<DesktopLibraryServiceError>>,
    ) -> DesktopLibraryRoutingTestResult {
        self.response
            .send(response)
            .map_err(|result| format!("library result abandoned: {result:?}"))?;
        Ok(())
    }
}

struct DesktopLibraryRoutingProvider(mpsc::Sender<DesktopLibraryRoutingRequest>);

impl DesktopLibraryServiceProvider for DesktopLibraryRoutingProvider {
    fn execute(&self, request: DesktopLibraryServiceRequest) -> DesktopLibraryServiceFuture {
        let (response, receiver) = oneshot::channel();
        let submitted = self
            .0
            .try_send(DesktopLibraryRoutingRequest { request, response });
        Box::pin(async move {
            submitted.map_err(|_full| Arc::new(DesktopLibraryServiceError::Busy))?;
            receiver
                .await
                .map_err(|_closed| Arc::new(DesktopLibraryServiceError::Closed))?
        })
    }
}

pub(super) struct DesktopLibraryRoutingFixture {
    pub(super) coordinator: DesktopCoordinator<
        DesktopLibraryRoutingClock,
        DesktopLibraryRoutingWake,
        DesktopLibraryRoutingDialog,
        DesktopLibraryRoutingLifecycle,
    >,
    pub(super) port: DesktopApplicationPort<DesktopLibraryRoutingClock, DesktopLibraryRoutingWake>,
    pub(super) workspace: DesktopMessagesEntityId,
    pub(super) requests: mpsc::Receiver<DesktopLibraryRoutingRequest>,
    pub(super) shutdowns: Arc<AtomicUsize>,
    pub(super) wake: Arc<DesktopLibraryRoutingWake>,
    executor: Arc<CapacityExecutor>,
    store: StorageDocumentStore,
    _directory: tempfile::TempDir,
}

impl DesktopLibraryRoutingFixture {
    pub(super) fn application()
    -> DesktopApplication<DesktopLibraryRoutingClock, DesktopLibraryRoutingWake> {
        DesktopApplication::new(
            Arc::new(DesktopLibraryRoutingClock),
            Arc::new(DesktopLibraryRoutingWake::default()),
        )
    }

    pub(super) async fn new() -> DesktopLibraryRoutingTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store =
            StorageDocumentStore::open(directory.path().join("routing.sqlite"), budget).await?;
        let wake = Arc::new(DesktopLibraryRoutingWake::default());
        let clock = Arc::new(DesktopLibraryRoutingClock);
        let application = DesktopApplication::new(Arc::clone(&clock), Arc::clone(&wake));
        let port = application.port();
        let workspace = DesktopMessagesEntityId::new();
        let (sender, requests) = mpsc::channel(DesktopApplication::<DesktopLibraryRoutingClock, DesktopLibraryRoutingWake>::ORDINARY_CAPACITY);
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let coordinator = DesktopCoordinator::new(
            application,
            clock,
            DesktopBootstrapSnapshot::new(
                &BoardPosition::standard(),
                NonZeroU32::MIN,
                DesktopEnumEngineSnapshot::Unavailable {
                    message: "library routing fixture".into(),
                },
            ),
            DesktopCoordinatorServices::new(
                Arc::new(DesktopLibraryRoutingDialog),
                Arc::clone(&executor),
                DesktopLibraryRoutingLifecycle(Arc::clone(&shutdowns)),
            ),
        )
        .with_library(workspace, Arc::new(DesktopLibraryRoutingProvider(sender)));
        Ok(Self {
            coordinator,
            port,
            workspace,
            requests,
            shutdowns,
            wake,
            executor,
            store,
            _directory: directory,
        })
    }

    pub(super) fn game(value: u128) -> DesktopLibraryRoutingTestResult<GameId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    pub(super) fn session(value: u128) -> DesktopLibraryRoutingTestResult<GameSession> {
        Ok(GameSession::new(GameSessionOptions {
            id: Self::game(value)?,
            root: uuid::Uuid::from_u128(1).to_string().parse()?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            clock: ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White),
            metadata: GameMetadata::default(),
        })?)
    }

    fn binding() -> DesktopGameEditorBinding {
        DesktopGameEditorBinding::new(
            GameStorageRevision::new(9),
            DesktopGameEditorEpoch::from_request_id(DesktopRequestId::new()),
            DesktopGameEditorGeneration::new(4),
        )
    }

    pub(super) fn context(
        value: u128,
    ) -> DesktopLibraryRoutingTestResult<DesktopGameEditorContext> {
        let session = Self::session(value)?;
        Ok(DesktopGameEditor::played(&session, Self::binding())?.context())
    }

    pub(super) fn publication(
        &self,
        value: u128,
    ) -> DesktopLibraryRoutingTestResult<DesktopGameEditorPublication> {
        let session = Self::session(value)?;
        let editor = DesktopGameEditor::played(&session, Self::binding())?;
        Ok(editor.publish(editor.context(), &self.store.client())?)
    }

    pub(super) fn query(workspace: DesktopMessagesEntityId) -> DesktopLibraryServiceOperation {
        DesktopLibraryServiceOperation::new(
            workspace,
            DesktopLibraryServiceEnumAction::Query {
                options: StorageLibraryQueryOptions::default(),
                cursor: None,
            },
        )
    }

    pub(super) fn open(
        workspace: DesktopMessagesEntityId,
        game: GameId,
    ) -> DesktopLibraryServiceOperation {
        DesktopLibraryServiceOperation::new(
            workspace,
            DesktopLibraryServiceEnumAction::Open {
                game,
                storage_revision: GameStorageRevision::new(9),
            },
        )
    }

    pub(super) fn submit(
        &self,
        operation: DesktopLibraryServiceOperation,
    ) -> DesktopLibraryRoutingTestResult<DesktopApplicationReceipt> {
        Ok(self
            .port
            .try_submit(&self.port.request(DesktopCommand::library(operation))?)?)
    }

    pub(super) fn quit(&self) -> DesktopLibraryRoutingTestResult<DesktopApplicationReceipt> {
        Ok(self.port.try_submit(
            &self
                .port
                .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?,
        )?)
    }

    pub(super) async fn poll(&mut self) -> DesktopLibraryRoutingTestResult<bool> {
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

    pub(super) async fn query_result(
        &self,
    ) -> DesktopLibraryRoutingTestResult<DesktopLibraryServiceEnumResult> {
        Ok(DesktopLibraryServiceEnumResult::Query(Arc::new(
            StorageLibraryRepository::new(self.store.client(), &self.executor)
                .query(StorageLibraryQuery::new(
                    StorageLibraryQueryOptions::default(),
                )?)
                .await?,
        )))
    }

    pub(super) async fn close(self) -> DesktopLibraryRoutingTestResult {
        drop(self.coordinator);
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|executor| {
                format!("retained executor owners: {}", Arc::strong_count(&executor))
            })?
            .shutdown()
            .await?;
        Ok(())
    }
}
