use std::collections::HashMap;
use std::num::{NonZeroU32, TryFromIntError};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tauri::ipc::{Channel, JavaScriptChannelId};
use tauri::{AppHandle, Manager};
use thiserror::Error;
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::board::position::BoardPosition;
use crate::capacity::budget::{CapacityBudget, CapacityBudgetError};
use crate::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use crate::clock::duration::ClockInstant;
use crate::clock::time_provider::{ClockTimeProvider, ClockTimeProviderMonotonic};
use crate::desktop::dto::{
    DesktopBootstrapSnapshot, DesktopEngineIdentity, DesktopEnumEngineSnapshot, DesktopEnumEvent,
    DesktopEnumRequest, DesktopEnumResponseResult, DesktopEnumValue, DesktopResponse,
};
use crate::desktop::envelope::DesktopEnvelope;
use crate::desktop::error::{DesktopEnumFailureCode, DesktopError, DesktopFailure};
use crate::desktop::file_handle::{
    DesktopFileDialogNative, DesktopFileDialogProvider, DesktopFileHandleOwner,
    DesktopFileHandleRegistry, DesktopFileHandleRequest,
};
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};
use crate::desktop::request_cache::DesktopRequestCache;
use crate::desktop::stream::{DesktopStream, DesktopStreamEvent};
use crate::engine::package::host::EnginePackageHost;
use crate::engine::package::host::EnginePackageHostError;
use crate::engine::package::selector::{EnginePackageSelector, EnginePackageSelectorError};
use crate::engine::process_provider::{EngineNativeProcess, EngineNativeProcessProvider};
use crate::engine::session::EngineSession;
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};

#[derive(Debug, Error)]
pub enum DesktopBootstrapError {
    #[error(transparent)]
    Budget(#[from] CapacityBudgetError),
    #[error(transparent)]
    Executor(#[from] CapacityExecutorError),
    #[error(transparent)]
    Storage(#[from] StorageDocumentError),
    #[error("Application startup or shutdown failed: {0}")]
    Native(#[from] tauri::Error),
    #[error("Application CPU capacity is outside the supported range: {0}")]
    Range(#[from] TryFromIntError),
    #[error("Application startup or shutdown failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("The native application worker is unavailable")]
    Closed,
    #[error("The engine could not shut down: {0}")]
    Engine(#[from] crate::engine::session::EngineSessionError),
    #[error(transparent)]
    EnginePackage(#[from] EnginePackageSelectorError),
    #[error(transparent)]
    EngineHost(#[from] EnginePackageHostError),
}

struct DesktopBootstrapCommand {
    window: String,
    request: DesktopEnvelope<DesktopEnumRequest>,
    channel: Option<Channel<DesktopStreamEvent<DesktopEnumEvent>>>,
    result: oneshot::Sender<DesktopResponse>,
}

struct DesktopBootstrapLifecycle {
    shutdown: oneshot::Sender<()>,
    worker: tauri::async_runtime::JoinHandle<Result<(), DesktopBootstrapError>>,
}

pub struct DesktopBootstrap {
    commands: mpsc::Sender<DesktopBootstrapCommand>,
    lifecycle: Mutex<Option<DesktopBootstrapLifecycle>>,
}

impl DesktopBootstrap {
    const QUEUE_CAPACITY: usize = 256;
    const WINDOW: &str = "main";

    pub async fn start(app: AppHandle) -> Result<Self, DesktopBootstrapError> {
        let clock = Arc::new(ClockTimeProviderMonotonic::new());
        let budget = CapacityBudget::discover()?;
        let cpu_threads = NonZeroU32::new(u32::try_from(budget.workers())?)
            .ok_or(CapacityBudgetError::InvalidAllocation)?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        #[cfg(not(feature = "native-test"))]
        let data = app.path().app_data_dir()?;
        #[cfg(feature = "native-test")]
        let test_data = executor
            .submit(|_cancellation| tempfile::tempdir())?
            .join()
            .await??;
        #[cfg(feature = "native-test")]
        let data = test_data.path().to_path_buf();
        tokio::fs::create_dir_all(&data).await?;
        let resources = if tauri::is_dev() || cfg!(feature = "native-test") {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/engine")
        } else {
            app.path().resource_dir()?.join("resources/engine")
        };
        let storage = StorageDocumentStore::open(data.join("library.sqlite3"), budget).await?;
        let (engine, engine_snapshot) = Self::start_engine(resources, &executor).await;
        let snapshot =
            DesktopBootstrapSnapshot::new(&BoardPosition::standard(), cpu_threads, engine_snapshot);
        let files = DesktopBootstrapFiles {
            file_handles: DesktopFileHandleRegistry::new(),
            files: DesktopFileDialogNative::new(app),
            executor: Arc::clone(&executor),
            requests: DesktopRequestCache::new(),
            owners: HashMap::new(),
            clock: Arc::clone(&clock),
        };
        let (file_commands, file_receiver) = mpsc::channel(
            DesktopBootstrapFiles::<DesktopFileDialogNative<tauri::Wry>>::QUEUE_CAPACITY,
        );
        let (file_shutdown, file_shutdown_receiver) = oneshot::channel();
        let file_worker =
            tauri::async_runtime::spawn(files.run(file_receiver, file_shutdown_receiver));
        let coordinator = DesktopBootstrapCoordinator {
            control: DesktopBootstrapControl::new(snapshot),
            file_commands,
            file_shutdown,
            file_worker,
            executor,
            storage,
            engine,
            #[cfg(feature = "native-test")]
            _test_data: test_data,
            clock,
        };
        let (commands, receiver) = mpsc::channel(Self::QUEUE_CAPACITY);
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let worker = tauri::async_runtime::spawn(coordinator.run(receiver, shutdown_receiver));
        Ok(Self {
            commands,
            lifecycle: Mutex::new(Some(DesktopBootstrapLifecycle { shutdown, worker })),
        })
    }

    async fn start_engine(
        resources: PathBuf,
        executor: &CapacityExecutor,
    ) -> (
        Option<EngineSession<EngineNativeProcess>>,
        DesktopEnumEngineSnapshot,
    ) {
        let outcome = async {
            let selected = executor
                .submit(move |_cancellation| -> Result<_, DesktopBootstrapError> {
                    Ok(EnginePackageSelector::load(
                        &resources,
                        EnginePackageHost::detect()?,
                    )?)
                })?
                .join()
                .await??;
            EngineSession::start(&EngineNativeProcessProvider, selected)
                .await
                .map_err(DesktopBootstrapError::Engine)
        }
        .await;
        match outcome {
            Ok(engine) => {
                let identity = engine.identity();
                let snapshot = DesktopEnumEngineSnapshot::Ready {
                    identity: Box::new(DesktopEngineIdentity::new(
                        identity.name().to_owned(),
                        identity.author().to_owned(),
                        identity
                            .capabilities()
                            .options()
                            .map(|option| option.name().to_owned())
                            .collect(),
                    )),
                };
                (Some(engine), snapshot)
            }
            Err(error) => {
                eprintln!("Engine startup failed: {error}");
                (None, DesktopEnumEngineSnapshot::Unavailable {
                    message: "The bundled Reckless engine is unavailable. The board is preserved. Restart the app after checking its engine package.".to_owned(),
                })
            }
        }
    }

    pub async fn shutdown(&self) -> Result<(), DesktopBootstrapError> {
        let lifecycle = self.lifecycle.lock().await.take();
        if let Some(lifecycle) = lifecycle {
            let signalled = lifecycle
                .shutdown
                .send(())
                .map_err(|_error| DesktopBootstrapError::Closed);
            lifecycle.worker.await??;
            signalled?;
        }
        Ok(())
    }

    async fn request(
        &self,
        window: String,
        request: DesktopEnvelope<DesktopEnumRequest>,
        channel: Option<Channel<DesktopStreamEvent<DesktopEnumEvent>>>,
    ) -> DesktopResponse {
        let request_id = request.request_id();
        if window != Self::WINDOW {
            return DesktopResponse::new(
                request_id,
                DesktopEnumResponseResult::Failure {
                    error: DesktopFailure::new(
                        DesktopEnumFailureCode::InvalidInput,
                        "This window cannot access application state.".to_owned(),
                        false,
                    ),
                },
            );
        }
        let (result, response) = oneshot::channel();
        let command = DesktopBootstrapCommand {
            window,
            request,
            channel,
            result,
        };
        if let Err(error) = self.commands.try_send(command) {
            let code = match error {
                mpsc::error::TrySendError::Full(_command) => {
                    DesktopEnumFailureCode::ResourceRefusal
                }
                mpsc::error::TrySendError::Closed(_command) => {
                    DesktopEnumFailureCode::InternalFailure
                }
            };
            return DesktopResponse::new(
                request_id,
                DesktopEnumResponseResult::Failure {
                    error: DesktopFailure::new(
                        code,
                        "The native application cannot accept another request.".to_owned(),
                        true,
                    ),
                },
            );
        }
        match response.await {
            Ok(response) => response,
            Err(_error) => DesktopResponse::new(
                request_id,
                DesktopEnumResponseResult::Failure {
                    error: DesktopFailure::new(
                        DesktopEnumFailureCode::Cancelled,
                        "The application is shutting down.".to_owned(),
                        false,
                    ),
                },
            ),
        }
    }
}

struct DesktopBootstrapSubscription {
    window: String,
    stream: DesktopStream<DesktopEnumEvent>,
    channel: Channel<DesktopStreamEvent<DesktopEnumEvent>>,
}

struct DesktopBootstrapControl {
    snapshot: DesktopBootstrapSnapshot,
    subscriptions: HashMap<DesktopRequestId, DesktopBootstrapSubscription>,
}

impl DesktopBootstrapControl {
    fn new(snapshot: DesktopBootstrapSnapshot) -> Self {
        Self {
            snapshot,
            subscriptions: HashMap::new(),
        }
    }

    fn subscribe(
        &mut self,
        window: String,
        channel: Option<Channel<DesktopStreamEvent<DesktopEnumEvent>>>,
        now: ClockInstant,
    ) -> Result<DesktopEnumValue, DesktopError> {
        let channel = channel.ok_or(DesktopError::ResyncRequired)?;
        self.subscriptions
            .retain(|_id, subscription| subscription.window != window);
        let subscription_id = DesktopRequestId::new();
        self.subscriptions.insert(
            subscription_id,
            DesktopBootstrapSubscription {
                window,
                stream: DesktopStream::new(subscription_id, now),
                channel,
            },
        );
        Ok(DesktopEnumValue::StreamSubscribe {
            subscription_id,
            sequence: DesktopRevision::ZERO,
            snapshot: self.snapshot.clone(),
        })
    }

    fn tick(&mut self, now: ClockInstant) -> Result<(), DesktopError> {
        for subscription in self.subscriptions.values_mut() {
            if subscription.stream.is_resync_required() {
                continue;
            }
            match subscription.stream.flush_pending(now) {
                Ok(events) => {
                    for event in events {
                        if subscription.channel.send(event).is_err() {
                            subscription.stream.require_resync();
                            break;
                        }
                    }
                }
                Err(DesktopError::ResyncRequired) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn subscription(
        &mut self,
        window: &str,
        subscription_id: DesktopRequestId,
    ) -> Result<&mut DesktopBootstrapSubscription, DesktopError> {
        self.subscriptions
            .get_mut(&subscription_id)
            .filter(|subscription| subscription.window == window)
            .ok_or(DesktopError::ResyncRequired)
    }

    fn acknowledge(
        &mut self,
        window: &str,
        subscription_id: DesktopRequestId,
        sequence: DesktopRevision,
        now: ClockInstant,
    ) -> Result<DesktopEnumValue, DesktopError> {
        self.subscription(window, subscription_id)?
            .stream
            .acknowledge(sequence, now)?;
        Ok(DesktopEnumValue::StreamAcknowledge)
    }

    fn resynchronize(
        &mut self,
        window: &str,
        subscription_id: DesktopRequestId,
        now: ClockInstant,
    ) -> Result<DesktopEnumValue, DesktopError> {
        let snapshot = self.snapshot.clone();
        let subscription = self.subscription(window, subscription_id)?;
        let event = subscription
            .stream
            .resynchronize(DesktopEnumEvent::Snapshot(snapshot.clone()), now)?;
        let sequence = event.sequence();
        subscription.channel.send(event).map_err(|_error| {
            subscription.stream.require_resync();
            DesktopError::ResyncRequired
        })?;
        Ok(DesktopEnumValue::StreamResync {
            subscription_id,
            sequence,
            snapshot,
        })
    }
}

struct DesktopBootstrapCoordinator {
    control: DesktopBootstrapControl,
    file_commands: mpsc::Sender<DesktopBootstrapCommand>,
    file_shutdown: oneshot::Sender<()>,
    file_worker: tauri::async_runtime::JoinHandle<()>,
    executor: Arc<CapacityExecutor>,
    storage: StorageDocumentStore,
    engine: Option<EngineSession<EngineNativeProcess>>,
    #[cfg(feature = "native-test")]
    _test_data: tempfile::TempDir,
    clock: Arc<ClockTimeProviderMonotonic>,
}

impl DesktopBootstrapCoordinator {
    const TICK_INTERVAL: Duration = Duration::from_millis(200);

    async fn run(
        mut self,
        mut commands: mpsc::Receiver<DesktopBootstrapCommand>,
        mut shutdown: oneshot::Receiver<()>,
    ) -> Result<(), DesktopBootstrapError> {
        let mut interval = tokio::time::interval(Self::TICK_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let command = tokio::select! {
                biased;
                _ = &mut shutdown => break,
                _ = interval.tick() => {
                    match self.clock.now().map_err(DesktopFailure::from).and_then(|now| self.control.tick(now).map_err(Into::into)) {
                        Ok(()) => {},
                        Err(error) => eprintln!("Desktop stream maintenance failed: {error:?}"),
                    }
                    continue;
                },
                command = commands.recv() => match command { Some(command) => command, None => break },
            };
            let request_id = command.request.request_id();
            if matches!(
                command.request.payload(),
                DesktopEnumRequest::FileSelect { .. }
            ) {
                if let Err(error) = self.file_commands.try_send(command) {
                    let (command, error) = match error {
                        mpsc::error::TrySendError::Full(command) => (command, DesktopFailure::new(DesktopEnumFailureCode::ResourceRefusal, "A native file selection is already pending. Try again after it closes.".to_owned(), true)),
                        mpsc::error::TrySendError::Closed(command) => (command, DesktopFailure::new(DesktopEnumFailureCode::InternalFailure, "The native file selection worker is unavailable.".to_owned(), false)),
                    };
                    let response = DesktopResponse::new(
                        request_id,
                        DesktopEnumResponseResult::Failure { error },
                    );
                    if let Err(abandoned) = command.result.send(response) {
                        drop(abandoned);
                    }
                }
                continue;
            }
            let outcome = self.handle(command.window, command.request, command.channel);
            if let Err(abandoned) = command
                .result
                .send(DesktopResponse::new(request_id, outcome))
            {
                drop(abandoned);
            }
        }
        drop(commands);
        let file_signal = self
            .file_shutdown
            .send(())
            .map_err(|_error| DesktopBootstrapError::Closed);
        drop(self.file_commands);
        let files = self
            .file_worker
            .await
            .map_err(DesktopBootstrapError::Native);
        let engine = match self.engine {
            Some(engine) => engine
                .shutdown()
                .await
                .map_err(DesktopBootstrapError::Engine),
            None => Ok(()),
        };
        let storage = self
            .storage
            .close()
            .await
            .map_err(DesktopBootstrapError::Storage);
        #[cfg(feature = "native-test")]
        let test_cleanup: Result<(), DesktopBootstrapError> = async {
            let test_data = self._test_data;
            self.executor
                .submit(move |_cancellation| test_data.close())?
                .join()
                .await??;
            Ok(())
        }
        .await;
        let executor = match Arc::try_unwrap(self.executor) {
            Ok(executor) => executor
                .shutdown()
                .await
                .map_err(DesktopBootstrapError::Executor),
            Err(_executor) => Err(DesktopBootstrapError::Closed),
        };
        files?;
        file_signal?;
        engine?;
        storage?;
        #[cfg(feature = "native-test")]
        test_cleanup?;
        executor
    }

    fn handle(
        &mut self,
        window: String,
        request: DesktopEnvelope<DesktopEnumRequest>,
        channel: Option<Channel<DesktopStreamEvent<DesktopEnumEvent>>>,
    ) -> DesktopEnumResponseResult {
        let outcome = self.dispatch(window, request, channel);
        match outcome {
            Ok(value) => DesktopEnumResponseResult::Success {
                value,
                revision: None,
            },
            Err(error) => DesktopEnumResponseResult::Failure { error },
        }
    }

    fn dispatch(
        &mut self,
        window: String,
        request: DesktopEnvelope<DesktopEnumRequest>,
        channel: Option<Channel<DesktopStreamEvent<DesktopEnumEvent>>>,
    ) -> Result<DesktopEnumValue, DesktopFailure> {
        let now = self.clock.now().map_err(DesktopFailure::from)?;
        match request.payload() {
            DesktopEnumRequest::Bootstrap => {
                Ok(DesktopEnumValue::Bootstrap(self.control.snapshot.clone()))
            }
            DesktopEnumRequest::StreamSubscribe => self
                .control
                .subscribe(window, channel, now)
                .map_err(Into::into),
            DesktopEnumRequest::StreamAcknowledge {
                subscription_id,
                sequence,
            } => self
                .control
                .acknowledge(&window, *subscription_id, *sequence, now)
                .map_err(Into::into),
            DesktopEnumRequest::StreamResync { subscription_id } => self
                .control
                .resynchronize(&window, *subscription_id, now)
                .map_err(Into::into),
            DesktopEnumRequest::FileSelect { purpose: _ } => Err(DesktopFailure::new(
                DesktopEnumFailureCode::InvalidInput,
                "File selections use the native selection queue.".to_owned(),
                false,
            )),
        }
    }
}

struct DesktopBootstrapFiles<P: DesktopFileDialogProvider> {
    file_handles: DesktopFileHandleRegistry,
    files: P,
    executor: Arc<CapacityExecutor>,
    requests: DesktopRequestCache<DesktopEnumResponseResult>,
    owners: HashMap<String, DesktopFileHandleOwner>,
    clock: Arc<ClockTimeProviderMonotonic>,
}

impl<P: DesktopFileDialogProvider> DesktopBootstrapFiles<P> {
    const QUEUE_CAPACITY: usize = 1;

    async fn run(
        mut self,
        mut commands: mpsc::Receiver<DesktopBootstrapCommand>,
        mut shutdown: oneshot::Receiver<()>,
    ) {
        loop {
            let command = tokio::select! {
                biased;
                _ = &mut shutdown => break,
                command = commands.recv() => match command { Some(command) => command, None => break },
            };
            let request_id = command.request.request_id();
            let outcome = tokio::select! {
                biased;
                _ = &mut shutdown => break,
                result = self.select(command.window, command.request) => result,
            };
            let result = match outcome {
                Ok(value) => DesktopEnumResponseResult::Success {
                    value,
                    revision: None,
                },
                Err(error) => DesktopEnumResponseResult::Failure { error },
            };
            if let Err(abandoned) = command
                .result
                .send(DesktopResponse::new(request_id, result))
            {
                drop(abandoned);
            }
        }
    }

    async fn select(
        &mut self,
        window: String,
        request: DesktopEnvelope<DesktopEnumRequest>,
    ) -> Result<DesktopEnumValue, DesktopFailure> {
        let DesktopEnumRequest::FileSelect { purpose } = request.payload() else {
            return Err(DesktopFailure::new(
                DesktopEnumFailureCode::InvalidInput,
                "The native selection queue requires a file selection request.".to_owned(),
                false,
            ));
        };
        let request_id = request.request_id();
        let now = self.clock.now().map_err(DesktopFailure::from)?;
        let fingerprint: [u8; 32] =
            Sha256::digest(serde_json::to_vec(&request).map_err(|_error| {
                DesktopFailure::new(
                    DesktopEnumFailureCode::InvalidInput,
                    "The file selection request is invalid.".to_owned(),
                    false,
                )
            })?)
            .into();
        if let Some(cached) = self
            .requests
            .lookup(request_id, &fingerprint, now)
            .map_err(DesktopFailure::from)?
        {
            return match cached.clone() {
                DesktopEnumResponseResult::Success { value, revision: _ } => Ok(value),
                DesktopEnumResponseResult::Failure { error } => Err(error),
            };
        }
        let owner = *self.owners.entry(window).or_default();
        let outcome = self
            .file_handles
            .select(
                &self.files,
                DesktopFileHandleRequest::new(owner, *purpose, now),
                &self.executor,
            )
            .await
            .map(DesktopEnumValue::FileSelect)
            .map_err(|_error| {
                DesktopFailure::new(
                    DesktopEnumFailureCode::FileUnavailable,
                    "The selected location is unavailable. Select it again.".to_owned(),
                    true,
                )
            });
        let result = match &outcome {
            Ok(value) => DesktopEnumResponseResult::Success {
                value: value.clone(),
                revision: None,
            },
            Err(error) => DesktopEnumResponseResult::Failure {
                error: error.clone(),
            },
        };
        self.requests
            .remember(
                request_id,
                fingerprint,
                result,
                self.clock.now().map_err(DesktopFailure::from)?,
            )
            .map_err(DesktopFailure::from)?;
        outcome
    }
}

#[tauri::command]
pub async fn desktop_command(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, DesktopBootstrap>,
    request: DesktopEnvelope<DesktopEnumRequest>,
    on_event: Option<JavaScriptChannelId>,
) -> Result<DesktopResponse, DesktopFailure> {
    Ok(state
        .request(
            window.label().to_owned(),
            request,
            on_event.map(|id| id.channel_on(window.as_ref().clone())),
        )
        .await)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use tauri::ipc::Channel;

    use crate::board::position::BoardPosition;
    use crate::clock::duration::ClockInstant;
    use crate::desktop::bootstrap::DesktopBootstrapControl;
    use crate::desktop::dto::{
        DesktopBootstrapSnapshot, DesktopEnumEngineSnapshot, DesktopEnumValue,
    };
    use crate::desktop::identity::DesktopRevision;

    struct DesktopBootstrapFixture;

    impl DesktopBootstrapFixture {
        fn control() -> DesktopBootstrapControl {
            let snapshot = DesktopBootstrapSnapshot::new(
                &BoardPosition::standard(),
                NonZeroU32::MIN,
                DesktopEnumEngineSnapshot::Unavailable {
                    message: "Engine not started in this fixture".to_owned(),
                },
            );
            DesktopBootstrapControl::new(snapshot)
        }
    }

    struct DesktopBootstrapPendingDialog {
        opened: std::sync::Arc<tokio::sync::Notify>,
    }

    impl crate::desktop::file_handle::DesktopFileDialogProvider for DesktopBootstrapPendingDialog {
        async fn select(
            &self,
            _purpose: crate::desktop::file_handle::DesktopFileEnumPurpose,
        ) -> Result<Option<std::path::PathBuf>, crate::desktop::file_handle::DesktopFileHandleError>
        {
            self.opened.notify_one();
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn pending_file_dialog_preserves_control_dispatch_and_owned_shutdown()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::collections::HashMap;
        use std::num::NonZeroUsize;
        use std::sync::Arc;
        use std::time::Duration;

        use tokio::sync::{Notify, mpsc, oneshot};

        use crate::capacity::budget::CapacityBudget;
        use crate::capacity::executor::CapacityExecutor;
        use crate::desktop::bootstrap::{
            DesktopBootstrapCommand, DesktopBootstrapCoordinator, DesktopBootstrapFiles,
        };
        use crate::desktop::dto::DesktopEnumRequest;
        use crate::desktop::envelope::DesktopEnvelope;
        use crate::desktop::file_handle::{DesktopFileEnumPurpose, DesktopFileHandleRegistry};
        use crate::desktop::identity::DesktopRequestId;
        use crate::desktop::request_cache::DesktopRequestCache;
        use crate::storage::document_store::StorageDocumentStore;

        let directory = tempfile::tempdir()?;
        let clock = Arc::new(crate::clock::time_provider::ClockTimeProviderMonotonic::new());
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let storage =
            StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget.clone())
                .await?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let opened = Arc::new(Notify::new());
        let files = DesktopBootstrapFiles {
            file_handles: DesktopFileHandleRegistry::new(),
            files: DesktopBootstrapPendingDialog {
                opened: Arc::clone(&opened),
            },
            executor: Arc::clone(&executor),
            requests: DesktopRequestCache::new(),
            owners: HashMap::new(),
            clock: Arc::clone(&clock),
        };
        let (file_commands, file_receiver) = mpsc::channel(1);
        let (file_shutdown, file_shutdown_receiver) = oneshot::channel();
        let file_worker = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
            files.run(file_receiver, file_shutdown_receiver),
        ));
        let coordinator = DesktopBootstrapCoordinator {
            control: DesktopBootstrapFixture::control(),
            file_commands,
            file_shutdown,
            file_worker,
            executor,
            storage,
            engine: None,
            #[cfg(feature = "native-test")]
            _test_data: directory,
            clock: Arc::clone(&clock),
        };
        let (commands, receiver) = mpsc::channel(8);
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let worker = tokio::spawn(coordinator.run(receiver, shutdown_receiver));
        let (file_result, file_response) = oneshot::channel();
        commands
            .send(DesktopBootstrapCommand {
                window: "main".to_owned(),
                request: DesktopEnvelope::new(
                    DesktopRequestId::new(),
                    None,
                    DesktopEnumRequest::FileSelect {
                        purpose: DesktopFileEnumPurpose::ReadFile,
                    },
                ),
                channel: None,
                result: file_result,
            })
            .await?;
        opened.notified().await;
        let (result, response) = oneshot::channel();
        commands
            .send(DesktopBootstrapCommand {
                window: "main".to_owned(),
                request: DesktopEnvelope::new(
                    DesktopRequestId::new(),
                    None,
                    DesktopEnumRequest::Bootstrap,
                ),
                channel: None,
                result,
            })
            .await?;
        let _response = tokio::time::timeout(Duration::from_secs(2), response).await??;
        shutdown.send(()).map_err(|()| "shutdown receiver closed")?;
        tokio::time::timeout(Duration::from_secs(2), worker).await???;
        assert!(file_response.await.is_err());
        assert_eq!(budget.available(), 1);
        Ok(())
    }

    #[test]
    fn subscriptions_bind_to_the_native_window_and_replace_old_channels()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut control = DesktopBootstrapFixture::control();
        let now = ClockInstant::from_nanos(0);
        let first =
            control.subscribe("main".to_owned(), Some(Channel::new(|_body| Ok(()))), now)?;
        let DesktopEnumValue::StreamSubscribe {
            subscription_id,
            sequence,
            snapshot: _,
        } = first
        else {
            return Err("missing subscription receipt".into());
        };
        assert_eq!(sequence, DesktopRevision::ZERO);
        assert!(
            control
                .acknowledge("foreign", subscription_id, sequence, now)
                .is_err()
        );
        assert!(
            control
                .resynchronize("foreign", subscription_id, now)
                .is_err()
        );
        control.acknowledge("main", subscription_id, sequence, now)?;
        control.subscribe("main".to_owned(), Some(Channel::new(|_body| Ok(()))), now)?;
        assert_eq!(control.subscriptions.len(), 1);
        assert!(control.resynchronize("main", subscription_id, now).is_err());
        assert!(control.subscribe("main".to_owned(), None, now).is_err());
        Ok(())
    }

    #[test]
    fn idle_ticks_enforce_acknowledgement_deadlines_and_resync_recovers()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut control = DesktopBootstrapFixture::control();
        let now = ClockInstant::from_nanos(0);
        let subscribed =
            control.subscribe("main".to_owned(), Some(Channel::new(|_body| Ok(()))), now)?;
        let DesktopEnumValue::StreamSubscribe {
            subscription_id,
            sequence: _,
            snapshot: _,
        } = subscribed
        else {
            return Err("missing subscription receipt".into());
        };
        let receipt = control.resynchronize("main", subscription_id, now)?;
        let DesktopEnumValue::StreamResync {
            sequence,
            subscription_id: _,
            snapshot: _,
        } = receipt
        else {
            return Err("missing resync receipt".into());
        };
        let deadline = ClockInstant::from_millis(10_000)?;
        control.tick(deadline)?;
        assert!(
            control
                .acknowledge("main", subscription_id, sequence, deadline)
                .is_err()
        );
        let receipt = control.resynchronize("main", subscription_id, deadline)?;
        let DesktopEnumValue::StreamResync {
            sequence: new_sequence,
            subscription_id: _,
            snapshot: _,
        } = receipt
        else {
            return Err("missing recovery receipt".into());
        };
        assert!(new_sequence > sequence);
        control.acknowledge("main", subscription_id, new_sequence, deadline)?;
        Ok(())
    }
}
