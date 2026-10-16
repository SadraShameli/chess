use std::ffi::OsString;
use std::num::{NonZeroU32, TryFromIntError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant as StandardInstant};

use thiserror::Error;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{Mutex as AsyncMutex, oneshot, watch};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::{Instant, timeout_at};

use crate::board::position::BoardPosition;
use crate::capacity::budget::{CapacityBudget, CapacityBudgetError};
use crate::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use crate::capacity::phase::{
    CapacityPhase, CapacityPhaseEnumInterruption, CapacityPhaseError, CapacityPhaseGeneration,
};
use crate::clock::time_provider::ClockTimeProviderMonotonic;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationError, DesktopApplicationPort,
    DesktopApplicationWakeProvider,
};
use crate::desktop::coordinator::{
    DesktopCoordinator, DesktopCoordinatorError, DesktopCoordinatorLifecycleProvider,
    DesktopCoordinatorServices,
};
use crate::desktop::dto::{
    DesktopBootstrapSnapshot, DesktopEngineIdentity, DesktopEnumEngineSnapshot,
};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand};
use crate::desktop::native::dialogs_proxy::DesktopNativeDialogsProxy;
use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;
use crate::desktop::native::host::{
    DesktopNativeHost, DesktopNativeHostError, DesktopNativeHostEvent, DesktopNativeHostServices,
};
use crate::desktop::native::recovery::{
    DesktopNativeRecoveryEnumAction, DesktopNativeRecoveryError, DesktopNativeRecoveryFailure,
    DesktopNativeRecoveryProvider,
};
use crate::desktop::native::resources::{DesktopNativeResources, DesktopNativeResourcesError};
use crate::desktop::native::startup::{
    DesktopNativeStartup, DesktopNativeStartupEnumAdmission, DesktopNativeStartupError,
    DesktopNativeStartupOwner,
};
use crate::desktop::profile::activation::server::{
    DesktopProfileActivationServer, DesktopProfileActivationServerError,
};
use crate::desktop::profile::activation::{
    DesktopProfileActivationBinding, DesktopProfileActivationEnumResult,
    DesktopProfileActivationError, DesktopProfileActivationExchange,
    DesktopProfileActivationRequest,
};
use crate::desktop::profile::owner_record::{
    DesktopProfileEnumPhase, DesktopProfileOwnerRecord, DesktopProfileOwnerRecordError,
    DesktopProfileOwnerRecordPublisher,
};
use crate::desktop::profile::{
    DesktopProfileError, DesktopProfileId, DesktopProfileIdentity, DesktopProfileLease,
    DesktopProfileOsLock, DesktopProfileSecurity,
};
use crate::engine::package::host::{EnginePackageHost, EnginePackageHostError};
use crate::engine::package::selector::{EnginePackageSelector, EnginePackageSelectorError};
use crate::engine::process_provider::{EngineNativeProcess, EngineNativeProcessProvider};
use crate::engine::search::enums::EngineSearchEnumState;
use crate::engine::session::{EngineSession, EngineSessionError, EngineSessionStartup};
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};

#[cfg(unix)]
use crate::desktop::native::activation::unix::{
    DesktopNativeActivationUnixConnectionTarget, DesktopNativeActivationUnixListener,
    DesktopNativeActivationUnixStream,
};
#[cfg(windows)]
use crate::desktop::native::activation::windows::{
    DesktopNativeActivationWindowsListener, DesktopNativeActivationWindowsStream,
};

#[derive(Debug, Error)]
pub enum DesktopNativeBootstrapError {
    #[error("Unknown application argument: {0:?}. Use --help for supported arguments")]
    Argument(OsString),
    #[error("The --profile-directory argument requires exactly one nonempty path")]
    ProfileArgument,
    #[error("The installed resource location cannot be resolved from {0}")]
    ResourceLocation(PathBuf),
    #[error("The existing profile is busy. Select its open Chess window or retry after it exits")]
    Busy,
    #[error("The existing owner declined window activation")]
    ActivationRejected,
    #[error("The existing owner's activation attempt failed: {0}")]
    Activation(#[from] DesktopProfileActivationError),
    #[error("Application initialization did not deliver native host services")]
    HostServices,
    #[error("The native host event channel closed before shutdown")]
    HostEvents,
    #[error("The native capacity phase is poisoned")]
    CapacityPoisoned,
    #[error("The idle engine has not acknowledged preparation: {0:?}")]
    EngineNotIdle(EngineSearchEnumState),
    #[error("Native initialization or cleanup exceeded its owned deadline")]
    CleanupDeadline,
    #[error("Application operation failed ({operation}); cleanup also failed ({cleanup})")]
    Cleanup {
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
    #[error("Native startup I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Range(#[from] TryFromIntError),
    #[error(transparent)]
    Startup(#[from] DesktopNativeStartupError),
    #[error(transparent)]
    Profile(#[from] DesktopProfileError),
    #[error(transparent)]
    OwnerRecord(#[from] DesktopProfileOwnerRecordError),
    #[error(transparent)]
    Capacity(#[from] CapacityBudgetError),
    #[error(transparent)]
    Phase(#[from] CapacityPhaseError),
    #[error(transparent)]
    Executor(#[from] CapacityExecutorError),
    #[error(transparent)]
    Resources(#[from] DesktopNativeResourcesError),
    #[error(transparent)]
    EngineHost(#[from] EnginePackageHostError),
    #[error(transparent)]
    EnginePackage(#[from] EnginePackageSelectorError),
    #[error(transparent)]
    Engine(#[from] EngineSessionError),
    #[error(transparent)]
    Storage(#[from] StorageDocumentError),
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
    #[error(transparent)]
    Coordinator(#[from] DesktopCoordinatorError),
    #[error(transparent)]
    Host(#[from] DesktopNativeHostError),
    #[error(transparent)]
    ActivationServer(#[from] DesktopProfileActivationServerError),
    #[error("The owned native supervisor failed: {0}")]
    Supervisor(#[from] JoinError),
}

impl DesktopNativeBootstrapError {
    fn combine(operation: Result<(), Self>, cleanup: Result<(), Self>) -> Result<(), Self> {
        match (operation, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(operation), Err(cleanup)) => Err(Self::Cleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            }),
        }
    }
}

#[derive(Debug)]
enum DesktopNativeBootstrapEnumArguments {
    Help,
    Run { profile: Option<PathBuf> },
}

impl DesktopNativeBootstrapEnumArguments {
    const PROFILE_DIRECTORY: &str = "--profile-directory";
    const HELP: &str = "--help";

    fn parse(
        arguments: impl IntoIterator<Item = OsString>,
    ) -> Result<Self, DesktopNativeBootstrapError> {
        let mut arguments = arguments.into_iter();
        let mut profile = None;
        let mut help = false;
        while let Some(argument) = arguments.next() {
            if argument == Self::HELP || argument == "-h" {
                help = true;
            } else if argument == Self::PROFILE_DIRECTORY {
                if profile.is_some() {
                    return Err(DesktopNativeBootstrapError::ProfileArgument);
                }
                let path = arguments
                    .next()
                    .filter(|value| {
                        !value.is_empty() && value != Self::HELP && value != Self::PROFILE_DIRECTORY
                    })
                    .ok_or(DesktopNativeBootstrapError::ProfileArgument)?;
                profile = Some(PathBuf::from(path));
            } else {
                return Err(DesktopNativeBootstrapError::Argument(argument));
            }
        }
        if help {
            Ok(Self::Help)
        } else {
            Ok(Self::Run { profile })
        }
    }
}

#[derive(Default)]
struct DesktopNativeBootstrapServices {
    storage: Option<StorageDocumentStore>,
    engine: Option<EngineSession<EngineNativeProcess>>,
    startup: Option<EngineSessionStartup<EngineNativeProcess>>,
    executor_startup: Option<CapacityExecutor>,
    executor: Option<Arc<CapacityExecutor>>,
    terminal: Option<DesktopNativeBootstrapError>,
}

impl DesktopNativeBootstrapServices {
    fn is_shutdown_complete(&self) -> bool {
        self.startup
            .as_ref()
            .is_none_or(EngineSessionStartup::is_shutdown_complete)
            && self
                .engine
                .as_ref()
                .is_none_or(EngineSession::is_shutdown_complete)
            && self
                .storage
                .as_ref()
                .is_none_or(StorageDocumentStore::is_closed)
            && self
                .executor_startup
                .as_ref()
                .is_none_or(CapacityExecutor::is_shutdown_complete)
    }

    fn retain_terminal(&mut self, error: DesktopNativeBootstrapError) {
        self.terminal = Some(match self.terminal.take() {
            Some(operation) => DesktopNativeBootstrapError::Cleanup {
                operation: Box::new(operation),
                cleanup: Box::new(error),
            },
            None => error,
        });
    }
}

#[derive(Clone)]
struct DesktopNativeBootstrapLifecycle {
    services: Arc<AsyncMutex<DesktopNativeBootstrapServices>>,
}

impl DesktopNativeBootstrapLifecycle {
    fn new() -> Self {
        Self {
            services: Arc::new(AsyncMutex::new(DesktopNativeBootstrapServices::default())),
        }
    }

    async fn cleanup(&self, deadline: Instant) -> Result<(), DesktopNativeBootstrapError> {
        let mut services = timeout_at(deadline, self.services.lock())
            .await
            .map_err(|_elapsed| DesktopNativeBootstrapError::CleanupDeadline)?;
        let DesktopNativeBootstrapServices {
            engine,
            storage,
            startup,
            executor_startup,
            ..
        } = &mut *services;
        let (engine_result, storage_result, startup_result, executor_startup_result) = tokio::join!(
            async {
                match engine.as_mut() {
                    Some(engine) if !engine.is_shutdown_complete() => engine
                        .shutdown_retained(deadline)
                        .await
                        .map_err(DesktopNativeBootstrapError::Engine),
                    Some(_) | None => Ok(()),
                }
            },
            async {
                match storage.as_mut() {
                    Some(storage) if !storage.is_closed() => storage
                        .close_retained(deadline)
                        .await
                        .map_err(DesktopNativeBootstrapError::Storage),
                    Some(_) | None => Ok(()),
                }
            },
            async {
                match startup.as_mut() {
                    Some(startup) if !startup.is_shutdown_complete() => startup
                        .shutdown_retained(deadline)
                        .await
                        .map_err(DesktopNativeBootstrapError::Engine),
                    Some(_) | None => Ok(()),
                }
            },
            async {
                match executor_startup.as_mut() {
                    Some(executor) if !executor.is_shutdown_complete() => executor
                        .shutdown_retained(deadline)
                        .await
                        .map_err(DesktopNativeBootstrapError::Executor),
                    Some(_) | None => Ok(()),
                }
            },
        );
        let engine_observed = services
            .engine
            .as_ref()
            .is_none_or(EngineSession::is_shutdown_complete);
        let storage_observed = services
            .storage
            .as_ref()
            .is_none_or(StorageDocumentStore::is_closed);
        let mut pending = Ok(());
        for (outcome, observed) in [
            (engine_result, engine_observed),
            (storage_result, storage_observed),
            (
                startup_result,
                services
                    .startup
                    .as_ref()
                    .is_none_or(EngineSessionStartup::is_shutdown_complete),
            ),
            (
                executor_startup_result,
                services
                    .executor_startup
                    .as_ref()
                    .is_none_or(CapacityExecutor::is_shutdown_complete),
            ),
        ] {
            if let Err(error) = outcome {
                if observed {
                    services.retain_terminal(error);
                } else {
                    pending = DesktopNativeBootstrapError::combine(pending, Err(error));
                }
            }
        }
        if pending.is_ok() && !services.is_shutdown_complete() {
            return Err(DesktopNativeBootstrapError::CleanupDeadline);
        }
        pending
    }

    async fn verify_idle(&self) -> Result<(), DesktopNativeBootstrapError> {
        let services = self.services.lock().await;
        if let Some(engine) = &services.engine
            && engine.state() != EngineSearchEnumState::Idle
        {
            return Err(DesktopNativeBootstrapError::EngineNotIdle(engine.state()));
        }
        Ok(())
    }
}

impl DesktopCoordinatorLifecycleProvider for DesktopNativeBootstrapLifecycle {
    async fn shutdown(&mut self, deadline: Instant) -> Result<(), DesktopCoordinatorError> {
        self.cleanup(deadline)
            .await
            .map_err(|source| DesktopCoordinatorError::Shutdown {
                source: Box::new(source),
            })
    }
}

struct DesktopNativeBootstrapRecovery;

impl DesktopNativeRecoveryProvider for DesktopNativeBootstrapRecovery {
    fn choose(
        &self,
        failure: &DesktopNativeRecoveryFailure,
    ) -> Result<DesktopNativeRecoveryEnumAction, DesktopNativeRecoveryError> {
        let result = rfd::MessageDialog::new()
            .set_title("Chess graphics recovery")
            .set_level(rfd::MessageLevel::Error)
            .set_description(format!(
                "{}\n\nYes: Retry graphics.\nNo: Save and quit safely.\nCancel: Keep the application paused.",
                failure.message(),
            ))
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show();
        match result {
            rfd::MessageDialogResult::Yes => Ok(DesktopNativeRecoveryEnumAction::Retry),
            rfd::MessageDialogResult::No => Ok(DesktopNativeRecoveryEnumAction::SafeQuit),
            rfd::MessageDialogResult::Cancel => Ok(DesktopNativeRecoveryEnumAction::KeepPaused),
            rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Custom(_) => {
                Err(DesktopNativeRecoveryError::Dialog {
                    source: Box::new(std::io::Error::other(
                        "The native recovery dialog returned an unavailable action",
                    )),
                })
            }
        }
    }
}

type DesktopNativeBootstrapPort =
    DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>;

struct DesktopNativeBootstrapSupervisor {
    lease: Arc<DesktopProfileLease>,
    binding: DesktopProfileActivationBinding,
    security: DesktopProfileSecurity,
    budget: CapacityBudget,
    lifecycle: DesktopNativeBootstrapLifecycle,
    wake: Arc<DesktopNativeEventLoopWake>,
    ready: oneshot::Sender<DesktopNativeHostServices>,
    host_done: watch::Receiver<bool>,
}

struct DesktopNativeBootstrapHostObservation<'a, 'lease> {
    events: watch::Receiver<DesktopNativeHostEvent>,
    host_done: watch::Receiver<bool>,
    capacity: Arc<Mutex<CapacityPhase>>,
    lifecycle: &'a DesktopNativeBootstrapLifecycle,
    publication: &'a mut DesktopProfileOwnerRecordPublisher<'lease>,
    wake: &'a DesktopNativeEventLoopWake,
}

impl DesktopNativeBootstrapSupervisor {
    async fn run(self) -> Result<(), DesktopNativeBootstrapError> {
        #[cfg(unix)]
        let mut listener =
            DesktopNativeActivationUnixListener::bind(&self.lease, self.binding, &self.security)?;
        #[cfg(windows)]
        let mut listener = DesktopNativeActivationWindowsListener::bind(
            &self.lease,
            self.binding,
            &self.security,
        )?;
        let pid = NonZeroU32::new(std::process::id())
            .ok_or_else(|| std::io::Error::other("The native process identifier is zero"))?;
        let retained_lease = Arc::clone(&self.lease);
        let binding = self.binding;
        let wake = Arc::clone(&self.wake);
        let published =
            DesktopProfileOwnerRecordPublisher::publish(&retained_lease, binding, &listener, pid);
        let operation = match published {
            Ok(mut publication) => {
                let (shutdown, stopping) = watch::channel(false);
                let outcome = {
                    let server =
                        DesktopProfileActivationServer::run(&mut listener, binding, wake, stopping);
                    let application = self.application(&mut publication);
                    tokio::pin!(server, application);
                    tokio::select! {
                        result = server.as_mut() => {
                            let server_result = result.map(|_report| ()).map_err(DesktopNativeBootstrapError::ActivationServer);
                            if let Err(error) = &server_result {
                                eprintln!("Native window activation stopped: {error}");
                            }
                            DesktopNativeBootstrapError::combine(application.await, server_result)
                        }
                        result = application.as_mut() => {
                            shutdown.send_replace(true);
                            let drained = server.await.map(|_report| ()).map_err(DesktopNativeBootstrapError::ActivationServer);
                            DesktopNativeBootstrapError::combine(result, drained)
                        }
                    }
                };
                DesktopNativeBootstrapError::combine(
                    outcome,
                    publication
                        .transition(DesktopProfileEnumPhase::ShuttingDown)
                        .map_err(DesktopNativeBootstrapError::OwnerRecord),
                )
            }
            Err(error) => Err(error.into()),
        };
        #[cfg(unix)]
        let cleanup = listener
            .shutdown()
            .map_err(DesktopNativeBootstrapError::Activation);
        #[cfg(windows)]
        let cleanup = {
            drop(listener);
            Ok(())
        };
        DesktopNativeBootstrapError::combine(operation, cleanup)
    }

    async fn application(
        self,
        publication: &mut DesktopProfileOwnerRecordPublisher<'_>,
    ) -> Result<(), DesktopNativeBootstrapError> {
        let executor = {
            let mut services = self.lifecycle.services.lock().await;
            services.executor_startup = Some(CapacityExecutor::uninitialized(self.budget.clone()));
            services
                .executor_startup
                .as_mut()
                .ok_or(DesktopNativeBootstrapError::HostServices)?
                .initialize()?;
            let executor = Arc::new(
                services
                    .executor_startup
                    .take()
                    .ok_or(DesktopNativeBootstrapError::HostServices)?,
            );
            services.executor = Some(Arc::clone(&executor));
            executor
        };
        let directory = DesktopNativeBootstrap::resource_directory()?;
        let resources = executor
            .submit(move |_cancellation| DesktopNativeResources::from_directory(&directory))?
            .join()
            .await??;
        let storage = StorageDocumentStore::open(
            self.lease
                .identity()
                .data_directory()
                .join("library.sqlite3"),
            self.budget.clone(),
        )
        .await?;
        self.lifecycle.services.lock().await.storage = Some(storage);
        let engine_snapshot = Self::engine(
            resources.engine().to_path_buf(),
            &executor,
            &self.budget,
            &self.lifecycle,
        )
        .await;
        let cpu_threads = NonZeroU32::new(u32::try_from(self.budget.workers())?)
            .ok_or(CapacityBudgetError::InvalidAllocation)?;
        let snapshot =
            DesktopBootstrapSnapshot::new(&BoardPosition::standard(), cpu_threads, engine_snapshot);
        let clock = Arc::new(ClockTimeProviderMonotonic::new());
        let application = DesktopApplication::new(Arc::clone(&clock), Arc::clone(&self.wake));
        let port = application.port();
        let (dialog_ready, dialogs) = watch::channel(None);
        let services = DesktopCoordinatorServices::new(
            Arc::new(DesktopNativeDialogsProxy::new(dialogs)),
            executor,
            self.lifecycle.clone(),
        );
        let mut coordinator = DesktopCoordinator::new(application, clock, snapshot, services);
        self.lifecycle.verify_idle().await?;
        let mut phase = CapacityPhase::new(self.budget.clone())?;
        let generation = phase.request_preparation()?;
        phase.acknowledge_searches_stopped(generation)?;
        phase.try_enter_preparation(generation)?;
        let capacity = Arc::new(Mutex::new(phase));
        let (events, receiver) = DesktopNativeHost::event_channel();
        let host = DesktopNativeHostServices {
            port,
            wake: Arc::clone(&self.wake),
            resources,
            capacity: Arc::clone(&capacity),
            runtime: tokio::runtime::Handle::current(),
            recovery: Arc::new(DesktopNativeBootstrapRecovery),
            events,
            budget: self.budget,
            dialog_ready,
        };
        self.ready
            .send(host)
            .map_err(|_host| DesktopNativeBootstrapError::HostServices)?;
        let observations = Self::observe_host(DesktopNativeBootstrapHostObservation {
            events: receiver,
            host_done: self.host_done,
            capacity,
            lifecycle: &self.lifecycle,
            publication,
            wake: &self.wake,
        });
        let (coordinator, observations) = tokio::join!(coordinator.run(), observations);
        DesktopNativeBootstrapError::combine(
            coordinator.map_err(DesktopNativeBootstrapError::Coordinator),
            observations,
        )
    }

    async fn engine(
        directory: PathBuf,
        executor: &CapacityExecutor,
        budget: &CapacityBudget,
        lifecycle: &DesktopNativeBootstrapLifecycle,
    ) -> DesktopEnumEngineSnapshot {
        let outcome = async {
            let package = executor
                .submit(move |_cancellation| {
                    Ok::<_, DesktopNativeBootstrapError>(EnginePackageSelector::load(
                        &directory,
                        EnginePackageHost::detect()?,
                    )?)
                })?
                .join()
                .await??;
            let _initialization = budget.lease(1).await?;
            let mut services = lifecycle.services.lock().await;
            services.startup = Some(EngineSessionStartup::new(
                &EngineNativeProcessProvider,
                package,
            )?);
            let engine = services
                .startup
                .as_mut()
                .ok_or(DesktopNativeBootstrapError::HostServices)?
                .initialize()
                .await?;
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
            services.engine = Some(engine);
            services.startup = None;
            Ok::<_, DesktopNativeBootstrapError>(snapshot)
        }
        .await;
        match outcome {
            Ok(snapshot) => snapshot,
            Err(error) => {
                eprintln!("Reckless engine startup failed: {error}");
                let mut services = lifecycle.services.lock().await;
                if let Some(startup) = &mut services.startup {
                    let cleanup = startup
                        .shutdown_retained(Instant::now() + Duration::from_secs(3))
                        .await;
                    if startup.is_shutdown_complete() {
                        services.startup = None;
                    }
                    if let Err(cleanup) = cleanup {
                        services.retain_terminal(DesktopNativeBootstrapError::Engine(cleanup));
                    }
                }
                DesktopEnumEngineSnapshot::Unavailable {
                    message: format!(
                        "The packaged Reckless engine is unavailable: {error}. Repair the engine package and restart Chess."
                    ),
                }
            }
        }
    }

    async fn observe_host(
        observation: DesktopNativeBootstrapHostObservation<'_, '_>,
    ) -> Result<(), DesktopNativeBootstrapError> {
        let DesktopNativeBootstrapHostObservation {
            mut events,
            mut host_done,
            capacity,
            lifecycle,
            publication,
            wake,
        } = observation;
        loop {
            if *host_done.borrow() {
                return Ok(());
            }
            tokio::select! {
                biased;
                changed = host_done.changed() => {
                    changed.map_err(|_closed| DesktopNativeBootstrapError::HostEvents)?;
                }
                changed = events.changed() => {
                    if changed.is_err() {
                        if !*host_done.borrow() {
                            host_done.changed().await.map_err(|_closed| DesktopNativeBootstrapError::HostEvents)?;
                        }
                        return Ok(());
                    }
                    let event = *events.borrow_and_update();
                    match event {
                        DesktopNativeHostEvent::Idle => {}
                        DesktopNativeHostEvent::PreparationRequested { generation } => {
                            lifecycle.verify_idle().await?;
                            Self::prepare(&capacity, generation).await?;
                            publication.transition(DesktopProfileEnumPhase::Recovery)?;
                        }
                        DesktopNativeHostEvent::FrameworkReady { generation } => {
                            Self::phase(&capacity, |phase| phase.complete_preparation(generation)).await?;
                            publication.transition(DesktopProfileEnumPhase::Ready)?;
                        }
                        DesktopNativeHostEvent::Interrupted { generation } => {
                            Self::phase(&capacity, |phase| {
                                phase.interrupt_preparation(generation, CapacityPhaseEnumInterruption::Failed)?;
                                phase.acknowledge_preparation_stopped(generation)
                            }).await?;
                            publication.transition(DesktopProfileEnumPhase::Recovery)?;
                        }
                    }
                    wake.wake().map_err(DesktopApplicationError::Wake)?;
                }
            }
        }
    }

    async fn prepare(
        capacity: &Mutex<CapacityPhase>,
        generation: CapacityPhaseGeneration,
    ) -> Result<(), DesktopNativeBootstrapError> {
        Self::phase(capacity, |phase| {
            phase.acknowledge_searches_stopped(generation)
        })
        .await?;
        loop {
            match Self::phase(capacity, |phase| phase.try_enter_preparation(generation)).await {
                Err(DesktopNativeBootstrapError::Phase(CapacityPhaseError::QuiescencePending)) => {
                    tokio::task::yield_now().await
                }
                result => return result,
            }
        }
    }

    async fn phase(
        capacity: &Mutex<CapacityPhase>,
        mut operation: impl FnMut(&mut CapacityPhase) -> Result<(), CapacityPhaseError>,
    ) -> Result<(), DesktopNativeBootstrapError> {
        loop {
            {
                match capacity.try_lock() {
                    Ok(mut phase) => {
                        return operation(&mut phase).map_err(DesktopNativeBootstrapError::Phase);
                    }
                    Err(TryLockError::WouldBlock) => {}
                    Err(TryLockError::Poisoned(_source)) => {
                        return Err(DesktopNativeBootstrapError::CapacityPoisoned);
                    }
                }
            }
            tokio::task::yield_now().await;
        }
    }
}

pub struct DesktopNativeBootstrap;

impl DesktopNativeBootstrap {
    const HELP: &str = "Chess\n\nUsage: chess [--profile-directory PATH]\n\nOptions:\n  -h, --help                 Show this help and exit\n  --profile-directory PATH   Use a specific local profile directory";
    const RESOURCE_DIRECTORY: &str = "resources";

    pub fn run() -> Result<(), DesktopNativeBootstrapError> {
        Self::run_with_arguments(std::env::args_os().skip(1))
    }

    pub fn run_with_arguments(
        arguments: impl IntoIterator<Item = OsString>,
    ) -> Result<(), DesktopNativeBootstrapError> {
        Self::run_with_host(arguments, DesktopNativeHost::run)
    }

    pub fn run_with_host(
        arguments: impl IntoIterator<Item = OsString>,
        host: impl FnOnce(DesktopNativeHostServices) -> Result<(), DesktopNativeHostError>,
    ) -> Result<(), DesktopNativeBootstrapError> {
        let profile = match DesktopNativeBootstrapEnumArguments::parse(arguments)? {
            DesktopNativeBootstrapEnumArguments::Help => {
                println!("{}", Self::HELP);
                return Ok(());
            }
            DesktopNativeBootstrapEnumArguments::Run { profile } => match profile {
                Some(path) => path,
                None => DesktopProfileIdentity::discover_data_directory()?,
            },
        };
        let started = StandardInstant::now();
        let security = DesktopProfileSecurity::current()?;
        let startup = DesktopNativeStartup::new(security.clone(), DesktopProfileOsLock);
        let admission = startup.admit(
            &profile,
            DesktopProfileId::from_uuid(uuid::Uuid::new_v4())?,
            DesktopRequestId::new(),
        )?;
        match admission {
            DesktopNativeStartupEnumAdmission::Owned(owner) => Self::owned(owner, security, host),
            DesktopNativeStartupEnumAdmission::ActivationOnly(record) => {
                Self::activate(&startup, &profile, &security, Some(record), started)
            }
            DesktopNativeStartupEnumAdmission::Busy => {
                Self::activate(&startup, &profile, &security, None, started)
            }
            DesktopNativeStartupEnumAdmission::InstallationBlocked(status) => {
                eprintln!(
                    "Chess installation requires recovery ({:?}) for profile {}. Ordinary startup remains blocked; no services were started.",
                    status.intent().state(),
                    status.binding().profile()
                );
                Ok(())
            }
        }
    }

    fn owned(
        owner: DesktopNativeStartupOwner,
        security: DesktopProfileSecurity,
        host: impl FnOnce(DesktopNativeHostServices) -> Result<(), DesktopNativeHostError>,
    ) -> Result<(), DesktopNativeBootstrapError> {
        let (lease, binding) = owner.into_parts();
        let lease: Arc<DesktopProfileLease> = Arc::from(lease);
        let budget = CapacityBudget::discover()?;
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(budget.workers())
            .enable_all()
            .thread_name("chess-runtime")
            .build()?;
        let lifecycle = DesktopNativeBootstrapLifecycle::new();
        let (ready, receiver) = oneshot::channel();
        let (host_done, completion) = watch::channel(false);
        let supervisor = DesktopNativeBootstrapSupervisor {
            lease: Arc::clone(&lease),
            binding,
            security,
            budget,
            lifecycle: lifecycle.clone(),
            wake: Arc::new(DesktopNativeEventLoopWake::new()),
            ready,
            host_done: completion,
        };
        let mut worker = runtime.spawn(supervisor.run());
        let (host_result, port) = match runtime.block_on(receiver) {
            Ok(services) => {
                let port = services.port.clone();
                (
                    host(services).map_err(DesktopNativeBootstrapError::Host),
                    Some(port),
                )
            }
            Err(_closed) => (Ok(()), None),
        };
        host_done.send_replace(true);
        let submitted = match &port {
            Some(port) => runtime.block_on(Self::request_quit(port)),
            None => Ok(()),
        };
        let observed = Self::observe_supervisor(&runtime, &mut worker, port.as_ref());
        let mut result = DesktopNativeBootstrapError::combine(host_result, submitted);
        result = DesktopNativeBootstrapError::combine(result, observed);
        loop {
            match runtime.block_on(lifecycle.cleanup(
                Instant::now()
                    + DesktopCoordinator::<
                        ClockTimeProviderMonotonic,
                        DesktopNativeEventLoopWake,
                        DesktopNativeDialogsProxy,
                        DesktopNativeBootstrapLifecycle,
                    >::SHUTDOWN,
            )) {
                Ok(()) => break,
                Err(error) => Self::retry_cleanup(&error.to_string()),
            }
        }
        let (mut executor, terminal) = runtime.block_on(async {
            let mut services = lifecycle.services.lock().await;
            (services.executor.take(), services.terminal.take())
        });
        if let Some(terminal) = terminal {
            result = DesktopNativeBootstrapError::combine(result, Err(terminal));
        }
        while let Some(owner) = executor {
            match Arc::try_unwrap(owner) {
                Ok(mut drained) => {
                    loop {
                        let outcome = runtime.block_on(
                            drained.shutdown_retained(Instant::now() + Duration::from_secs(3)),
                        );
                        if drained.is_shutdown_complete() {
                            result = DesktopNativeBootstrapError::combine(
                                result,
                                outcome.map_err(DesktopNativeBootstrapError::Executor),
                            );
                            break;
                        }
                        if let Err(error) = outcome {
                            Self::retry_cleanup(&error.to_string());
                        }
                    }
                    executor = None;
                }
                Err(owner) => {
                    Self::retry_cleanup(
                        "Application work still owns the CPU executor. Close outstanding work before retrying cleanup.",
                    );
                    executor = Some(owner);
                }
            }
        }
        drop(lifecycle);
        drop(runtime);
        result = DesktopNativeBootstrapError::combine(
            result,
            lease
                .verify_identity()
                .map_err(DesktopNativeBootstrapError::Profile),
        );
        drop(lease);
        result
    }

    fn observe_supervisor(
        runtime: &Runtime,
        worker: &mut JoinHandle<Result<(), DesktopNativeBootstrapError>>,
        port: Option<&DesktopNativeBootstrapPort>,
    ) -> Result<(), DesktopNativeBootstrapError> {
        let mut retried = Ok(());
        loop {
            match runtime.block_on(async {
                timeout_at(Instant::now() + Duration::from_secs(3), &mut *worker).await
            }) {
                Ok(result) => return DesktopNativeBootstrapError::combine(result?, retried),
                Err(_elapsed) => {
                    Self::retry_cleanup(
                        "Application shutdown has not completed. Its profile, database and engine remain owned. Retry cleanup to continue safely.",
                    );
                    if let Some(port) = port
                        && let Err(error) = port.retry_quit()
                    {
                        retried = DesktopNativeBootstrapError::combine(retried, Err(error.into()));
                    }
                }
            }
        }
    }

    async fn request_quit(
        port: &DesktopNativeBootstrapPort,
    ) -> Result<(), DesktopNativeBootstrapError> {
        let request = port.request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
        loop {
            match port.try_submit(&request) {
                Ok(_receipt) => return Ok(()),
                Err(DesktopApplicationError::Busy) => tokio::task::yield_now().await,
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn retry_cleanup(message: &str) {
        drop(
            rfd::MessageDialog::new()
                .set_title("Chess shutdown is still pending")
                .set_level(rfd::MessageLevel::Error)
                .set_description(message)
                .set_buttons(rfd::MessageButtons::OkCustom("Retry Cleanup".to_owned()))
                .show(),
        );
    }

    fn resource_directory() -> Result<PathBuf, DesktopNativeBootstrapError> {
        if cfg!(debug_assertions) {
            Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join(Self::RESOURCE_DIRECTORY))
        } else {
            Self::installed_resource_directory(&std::env::current_exe()?)
        }
    }

    fn installed_resource_directory(
        executable: &Path,
    ) -> Result<PathBuf, DesktopNativeBootstrapError> {
        let parent = executable.parent().ok_or_else(|| {
            DesktopNativeBootstrapError::ResourceLocation(executable.to_path_buf())
        })?;
        #[cfg(target_os = "macos")]
        {
            if parent.file_name() != Some(std::ffi::OsStr::new("MacOS")) {
                return Err(DesktopNativeBootstrapError::ResourceLocation(
                    executable.to_path_buf(),
                ));
            }
            Ok(parent
                .parent()
                .ok_or_else(|| {
                    DesktopNativeBootstrapError::ResourceLocation(executable.to_path_buf())
                })?
                .join("Resources")
                .join(Self::RESOURCE_DIRECTORY))
        }
        #[cfg(target_os = "windows")]
        {
            Ok(parent.join(Self::RESOURCE_DIRECTORY))
        }
        #[cfg(target_os = "linux")]
        {
            let prefix = parent.parent().ok_or_else(|| {
                DesktopNativeBootstrapError::ResourceLocation(executable.to_path_buf())
            })?;
            Ok(
                DesktopProfileIdentity::data_directory_in(&prefix.join("share"))
                    .join(Self::RESOURCE_DIRECTORY),
            )
        }
    }

    fn activate(
        startup: &DesktopNativeStartup<DesktopProfileOsLock>,
        profile: &Path,
        security: &DesktopProfileSecurity,
        mut initial: Option<DesktopProfileOwnerRecord>,
        started: StandardInstant,
    ) -> Result<(), DesktopNativeBootstrapError> {
        let runtime = Builder::new_current_thread().enable_all().build()?;
        let deadline = Instant::from_std(started)
            .checked_add(DesktopProfileActivationExchange::CLIENT_DEADLINE)
            .ok_or(DesktopProfileActivationError::DeadlineOverflow)?;
        let trials = u32::try_from(DesktopProfileActivationExchange::CLIENT_TRIALS)?;
        let interval = DesktopProfileActivationExchange::CLIENT_DEADLINE / trials;
        let request_id = DesktopRequestId::new();
        let mut last = None;
        for trial in 0..DesktopProfileActivationExchange::CLIENT_TRIALS {
            if Instant::now() >= deadline {
                break;
            }
            let record = if trial == 0 {
                initial.take()
            } else {
                match startup.inspect_existing_owner(profile)? {
                    DesktopNativeStartupEnumAdmission::ActivationOnly(record) => Some(record),
                    DesktopNativeStartupEnumAdmission::Busy => None,
                    DesktopNativeStartupEnumAdmission::Owned(_)
                    | DesktopNativeStartupEnumAdmission::InstallationBlocked(_) => {
                        return Err(DesktopNativeBootstrapError::Busy);
                    }
                }
            };
            let offset = interval * u32::try_from(trial + 1)?;
            let attempt_deadline = Instant::from_std(started)
                .checked_add(offset)
                .ok_or(DesktopProfileActivationError::DeadlineOverflow)?
                .min(deadline);
            if let Some(record) = record {
                let request = DesktopProfileActivationRequest::new(record.binding(), request_id);
                let outcome = Self::activation_exchange(
                    &runtime,
                    &record,
                    security,
                    request,
                    attempt_deadline,
                );
                match outcome {
                    Ok(DesktopProfileActivationEnumResult::Focused) => return Ok(()),
                    Ok(DesktopProfileActivationEnumResult::OwnerStarting) => {
                        println!(
                            "Chess is already starting. Its existing window will be shown when ready."
                        );
                        return Ok(());
                    }
                    Ok(DesktopProfileActivationEnumResult::ActivationRequested) => {
                        println!(
                            "Activation requested. Select the existing Chess window if your desktop does not focus it."
                        );
                        return Ok(());
                    }
                    Ok(DesktopProfileActivationEnumResult::Busy) => {}
                    Ok(DesktopProfileActivationEnumResult::Rejected) => {
                        return Err(DesktopNativeBootstrapError::ActivationRejected);
                    }
                    Err(error) => last = Some(error),
                }
            }
            if trial + 1 < DesktopProfileActivationExchange::CLIENT_TRIALS
                && Instant::now() < attempt_deadline
            {
                runtime.block_on(async {
                    tokio::time::sleep_until(attempt_deadline).await;
                });
            }
        }
        match last {
            Some(error) => Err(error.into()),
            None => Err(DesktopNativeBootstrapError::Busy),
        }
    }

    fn activation_exchange(
        runtime: &Runtime,
        record: &DesktopProfileOwnerRecord,
        security: &DesktopProfileSecurity,
        request: DesktopProfileActivationRequest,
        deadline: Instant,
    ) -> Result<DesktopProfileActivationEnumResult, DesktopProfileActivationError> {
        #[cfg(unix)]
        {
            let target =
                DesktopNativeActivationUnixConnectionTarget::prepare(record.endpoint(), security)?;
            runtime.block_on(async {
                let stream = DesktopNativeActivationUnixStream::connect(&target, deadline).await?;
                DesktopProfileActivationExchange::activate(stream, request, deadline).await
            })
        }
        #[cfg(windows)]
        {
            runtime.block_on(async {
                let stream = timeout_at(
                    deadline,
                    DesktopNativeActivationWindowsStream::connect(record.endpoint(), security),
                )
                .await
                .map_err(|_elapsed| DesktopProfileActivationError::Deadline)??;
                DesktopProfileActivationExchange::activate(stream, request, deadline).await
            })
        }
    }
}
