use std::ffi::OsString;
use std::future::{Future, poll_fn};
use std::num::{NonZeroU32, TryFromIntError};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, TryLockError};
use std::task::Poll;
use std::time::{Duration, Instant as StandardInstant};

use rand::{
    SeedableRng,
    rngs::{StdRng, SysRng},
};
use thiserror::Error;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{Mutex as AsyncMutex, oneshot, watch};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::{Instant, timeout_at};

use crate::board::position::BoardPosition;
use crate::capacity::budget::{CapacityBudget, CapacityBudgetError};
use crate::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use crate::capacity::phase::{
    CapacityPhase, CapacityPhaseEnumInterruption, CapacityPhaseEnumState, CapacityPhaseError,
    CapacityPhaseGeneration,
};
use crate::clock::duration::ClockDurationError;
use crate::clock::time_provider::{ClockTimeProvider, ClockTimeProviderMonotonic};
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
use crate::desktop::game_bridge::DesktopGameBridgeProvider;
use crate::desktop::game_registry::{DesktopGameRegistryError, DesktopGameRegistryReceipt};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId};
use crate::desktop::native::dialogs_proxy::DesktopNativeDialogsProxy;
use crate::desktop::native::engine_runtime::{
    DesktopNativeEngineRuntime, DesktopNativeEngineRuntimeOptions,
};
use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;
use crate::desktop::native::game_factories::DesktopNativeGameFactories;
use crate::desktop::native::game_resources::{
    DesktopNativeGameResources, DesktopNativeGameResourcesMemory,
    DesktopNativeGameResourcesMemoryProvider, DesktopNativeGameResourcesOptions,
    DesktopNativeGameResourcesPolicy, DesktopNativeGameResourcesPolicyError,
};
use crate::desktop::native::game_runtime::{
    DesktopNativeGameRuntimeError, DesktopNativeGameRuntimePort,
};
use crate::desktop::native::game_service::{
    DesktopNativeGameService, DesktopNativeGameServiceError, DesktopNativeGameServiceOptions,
};
use crate::desktop::native::host::{
    DesktopNativeHost, DesktopNativeHostError, DesktopNativeHostEvent, DesktopNativeHostServices,
};
use crate::desktop::native::puzzle_runtime::DesktopNativePuzzleRuntimeSchedulerTokio;
use crate::desktop::native::puzzle_service::{
    DesktopNativePuzzleService, DesktopNativePuzzleServiceOptions,
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
use crate::desktop::puzzle_service::{DesktopPuzzleServiceError, DesktopPuzzleServiceProvider};
use crate::engine::package::host::{EnginePackageHost, EnginePackageHostError};
use crate::engine::package::selector::{
    EnginePackageSelected, EnginePackageSelector, EnginePackageSelectorError,
};
use crate::engine::process_provider::{EngineNativeProcess, EngineNativeProcessProvider};
use crate::engine::session::{EngineSession, EngineSessionError, EngineSessionStartup};
use crate::presentation::puzzle::panel::PresentationPuzzlePanelConnection;
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};
use crate::storage::puzzle_repository::StoragePuzzleRepository;

#[cfg(test)]
#[path = "../../../tests/desktop/native/bootstrap_games.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../tests/desktop/native/bootstrap_puzzles.rs"]
mod puzzle_tests;

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
    #[error("Native initialization or cleanup exceeded its owned deadline")]
    CleanupDeadline,
    #[error("Application cleanup is already in progress; wait before retrying")]
    CleanupBusy,
    #[error("Native engine quiescence exceeded its preparation deadline: {0}")]
    EngineQuiescenceDeadline(#[source] tokio::time::error::Elapsed),
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
    Clock(#[from] ClockDurationError),
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
    Analysis(#[from] crate::desktop::error::DesktopFailure),
    #[error(transparent)]
    Storage(#[from] StorageDocumentError),
    #[error(transparent)]
    GameResources(#[from] DesktopNativeGameResourcesPolicyError),
    #[error(transparent)]
    GameService(#[from] DesktopNativeGameServiceError),
    #[error("The saved puzzle service failed: {0}")]
    PuzzleService(#[from] Arc<DesktopPuzzleServiceError>),
    #[error("The native game lifecycle operation failed: {0}")]
    GameRuntime(#[source] Arc<DesktopNativeGameRuntimeError>),
    #[error("The native game registry operation failed: {0}")]
    GameRegistry(#[source] Arc<crate::desktop::game_registry::DesktopGameRegistryError>),
    #[error("The operating system could not seed randomized chess starts: {0}")]
    Random(#[from] rand::rngs::SysError),
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

type DesktopNativeBootstrapPuzzleService = DesktopNativePuzzleService<
    ClockTimeProviderMonotonic,
    DesktopNativeEventLoopWake,
    DesktopNativePuzzleRuntimeSchedulerTokio,
>;
type DesktopNativeBootstrapPuzzleCleanupFuture = Pin<
    Box<
        dyn Future<
                Output = (
                    DesktopNativeBootstrapPuzzleService,
                    Result<(), Arc<DesktopPuzzleServiceError>>,
                ),
            > + Send,
    >,
>;

struct DesktopNativeBootstrapPuzzleCleanup {
    deadline: Instant,
    future: DesktopNativeBootstrapPuzzleCleanupFuture,
}

#[derive(Default)]
struct DesktopNativeBootstrapServices {
    storage: Option<StorageDocumentStore>,
    engine: Option<EngineSession<EngineNativeProcess>>,
    startup: Option<EngineSessionStartup<EngineNativeProcess>>,
    executor_startup: Option<CapacityExecutor>,
    executor: Option<Arc<CapacityExecutor>>,
    games: Option<DesktopNativeGameService>,
    puzzles: Option<DesktopNativeBootstrapPuzzleService>,
    puzzle_cleanup: Option<DesktopNativeBootstrapPuzzleCleanup>,
    analysis: Option<
        DesktopNativeEngineRuntime<EngineNativeProcessProvider, DesktopNativeGameResourcesMemory>,
    >,
    terminal: Option<DesktopNativeBootstrapError>,
}

impl DesktopNativeBootstrapServices {
    fn is_shutdown_complete(&self) -> bool {
        self.puzzle_cleanup.is_none()
            && self
                .puzzles
                .as_ref()
                .is_none_or(DesktopNativePuzzleService::is_shutdown_complete)
            && self
                .analysis
                .as_ref()
                .is_none_or(DesktopNativeEngineRuntime::is_shutdown_complete)
            && self
                .games
                .as_ref()
                .is_none_or(DesktopNativeGameService::is_shutdown_complete)
            && self
                .startup
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
        self.cleanup_puzzles(deadline).await?;
        let mut services = timeout_at(deadline, self.services.lock())
            .await
            .map_err(|_elapsed| DesktopNativeBootstrapError::CleanupDeadline)?;
        if let Some(analysis) = &mut services.analysis {
            let result = analysis.shutdown_retained(deadline).await;
            let complete = analysis.is_shutdown_complete();
            match result {
                Ok(()) if complete => {}
                Ok(()) => return Err(DesktopNativeBootstrapError::CleanupDeadline),
                Err(error) if complete => services.retain_terminal(error.into()),
                Err(error) => return Err(error.into()),
            }
            services.analysis = None;
        }
        if let Some(games) = &mut services.games {
            let result = games.shutdown_retained(deadline).await;
            let complete = games.is_shutdown_complete();
            match result {
                Ok(()) if complete => {}
                Ok(()) => return Err(DesktopNativeBootstrapError::CleanupDeadline),
                Err(error) if complete => services.retain_terminal(error.into()),
                Err(error) => return Err(error.into()),
            }
            services.games = None;
        }
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

    async fn cleanup_puzzles(&self, deadline: Instant) -> Result<(), DesktopNativeBootstrapError> {
        let effective_deadline = {
            let services = self
                .services
                .try_lock()
                .map_err(|_busy| DesktopNativeBootstrapError::CleanupBusy)?;
            services
                .puzzle_cleanup
                .as_ref()
                .map_or(deadline, |cleanup| cleanup.deadline.min(deadline))
        };
        timeout_at(
            effective_deadline,
            poll_fn(|context| {
                let mut services = match self.services.try_lock() {
                    Ok(services) => services,
                    Err(_busy) => {
                        return Poll::Ready(Err(DesktopNativeBootstrapError::CleanupBusy));
                    }
                };
                if services.puzzle_cleanup.is_none() {
                    let Some(mut puzzles) = services.puzzles.take() else {
                        return Poll::Ready(Ok(()));
                    };
                    services.puzzle_cleanup = Some(DesktopNativeBootstrapPuzzleCleanup {
                        deadline,
                        future: Box::pin(async move {
                            let outcome = puzzles.shutdown_retained(deadline).await;
                            (puzzles, outcome)
                        }),
                    });
                }
                let Some(cleanup) = services.puzzle_cleanup.as_mut() else {
                    return Poll::Ready(Err(DesktopNativeBootstrapError::HostServices));
                };
                let Poll::Ready((puzzles, outcome)) = cleanup.future.as_mut().poll(context) else {
                    return Poll::Pending;
                };
                services.puzzle_cleanup = None;
                let complete = puzzles.is_shutdown_complete();
                if !complete {
                    services.puzzles = Some(puzzles);
                }
                Poll::Ready(match outcome {
                    Ok(()) if complete => Ok(()),
                    Ok(()) => Err(DesktopNativeBootstrapError::CleanupDeadline),
                    Err(error) if complete => {
                        services.retain_terminal(error.into());
                        Ok(())
                    }
                    Err(error) => Err(error.into()),
                })
            }),
        )
        .await
        .map_err(|_elapsed| DesktopNativeBootstrapError::CleanupDeadline)?
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
    games: DesktopNativeGameRuntimePort,
    analysis: Option<crate::desktop::engine_bridge::DesktopEngineBridgePort>,
    publication: &'a mut DesktopProfileOwnerRecordPublisher<'lease>,
    wake: &'a DesktopNativeEventLoopWake,
}

impl DesktopNativeBootstrapSupervisor {
    const CONTROL_TIMEOUT: Duration = Duration::from_secs(3);

    fn control_deadline() -> Instant {
        Instant::now() + Self::CONTROL_TIMEOUT
    }

    async fn game_control(
        control: Result<DesktopGameRegistryReceipt, DesktopGameRegistryError>,
    ) -> Result<(), DesktopNativeBootstrapError> {
        control
            .map_err(|error| DesktopNativeBootstrapError::GameRegistry(Arc::new(error)))?
            .wait()
            .await
            .map_err(DesktopNativeBootstrapError::GameRegistry)
    }

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
        let (mut engine_snapshot, package) = Self::engine(
            resources.engine().to_path_buf(),
            &executor,
            &self.budget,
            &self.lifecycle,
        )
        .await?;
        let cpu_threads = NonZeroU32::new(u32::try_from(self.budget.workers())?)
            .ok_or(CapacityBudgetError::InvalidAllocation)?;
        let clock = Arc::new(ClockTimeProviderMonotonic::new());
        let memory = Arc::new(DesktopNativeGameResourcesMemory::new(Arc::clone(&executor)));
        let available_memory = memory.available_bytes().await?;
        let workload_workers = self
            .budget
            .workers()
            .checked_sub(CapacityPhase::STEADY_RESERVED_WORKERS)
            .ok_or(CapacityBudgetError::InvalidAllocation)?;
        let policy =
            DesktopNativeGameResourcesPolicy::automatic(workload_workers, available_memory)?;
        let engine_resources = Arc::new(DesktopNativeGameResources::new(
            DesktopNativeGameResourcesOptions {
                budget: self.budget.clone(),
                policy,
                memory,
            },
        )?);
        let random = executor
            .submit(|_cancellation| StdRng::try_from_rng(&mut SysRng))?
            .join()
            .await??;
        let storage_client = self
            .lifecycle
            .services
            .lock()
            .await
            .storage
            .as_ref()
            .ok_or(DesktopNativeBootstrapError::HostServices)?
            .client();
        let puzzles = DesktopNativePuzzleService::new(DesktopNativePuzzleServiceOptions {
            repository: StoragePuzzleRepository::new(storage_client.clone()),
            executor: Arc::clone(&executor),
            clock: Arc::clone(&clock),
            wake: Arc::clone(&self.wake),
            runtime: tokio::runtime::Handle::current(),
            scheduler: DesktopNativePuzzleRuntimeSchedulerTokio::new(
                tokio::runtime::Handle::current(),
                clock.now()?,
                Instant::now(),
            ),
            replacement_timeout: Self::CONTROL_TIMEOUT,
        });
        let puzzle_port = {
            let mut retained = self.lifecycle.services.lock().await;
            retained.puzzles = Some(puzzles);
            retained
                .puzzles
                .as_mut()
                .ok_or(DesktopNativeBootstrapError::HostServices)?
                .start()?
        };
        let puzzle_owner = DesktopMessagesEntityId::new();
        let games = DesktopNativeGameService::new(DesktopNativeGameServiceOptions {
            factory_provider: DesktopNativeGameFactories::new(
                package.clone(),
                Arc::clone(&clock),
                Arc::clone(&engine_resources),
            ),
            storage: storage_client,
            executor: Arc::clone(&executor),
            clock: Arc::clone(&clock),
            proof: Arc::new(DesktopNativeGameService::proof()),
            random,
        });
        let game_port = {
            let mut retained = self.lifecycle.services.lock().await;
            retained.games = Some(games);
            let games = retained
                .games
                .as_mut()
                .ok_or(DesktopNativeBootstrapError::HostServices)?;
            games.restore().await?;
            games.start()?
        };
        let mut engine_port = None;
        let mut engine_panels = None;
        if let Some(package) = package {
            let analysis = DesktopNativeEngineRuntime::new(DesktopNativeEngineRuntimeOptions {
                provider: EngineNativeProcessProvider,
                package,
                resources: engine_resources,
                executor: Arc::clone(&executor),
                clock: Arc::clone(&clock),
                wake: Arc::clone(&self.wake),
                snapshots: game_port.snapshots(),
                budget: self.budget.clone(),
                tablebase_parent: std::env::temp_dir(),
            });
            let mut retained = self.lifecycle.services.lock().await;
            retained.analysis = Some(analysis);
            let analysis = retained
                .analysis
                .as_mut()
                .ok_or(DesktopNativeBootstrapError::HostServices)?;
            match analysis.initialize().await {
                Ok((port, panels)) => {
                    engine_port = Some(port);
                    engine_panels = Some(panels);
                }
                Err(failure) => {
                    engine_snapshot = DesktopEnumEngineSnapshot::Unavailable {
                        message: failure.to_string(),
                    };
                    let cleanup = analysis.shutdown_retained(Self::control_deadline()).await;
                    let complete = analysis.is_shutdown_complete();
                    if !complete {
                        return Err(cleanup.err().map_or(
                            DesktopNativeBootstrapError::CleanupDeadline,
                            DesktopNativeBootstrapError::Analysis,
                        ));
                    }
                    if let Err(failure) = cleanup {
                        retained.retain_terminal(failure.into());
                    }
                    retained.analysis = None;
                }
            }
        }
        let snapshot =
            DesktopBootstrapSnapshot::new(&BoardPosition::standard(), cpu_threads, engine_snapshot);
        let application = DesktopApplication::new(Arc::clone(&clock), Arc::clone(&self.wake));
        let port = application.port();
        let (dialog_ready, dialogs) = watch::channel(None);
        let services = DesktopCoordinatorServices::new(
            Arc::new(DesktopNativeDialogsProxy::new(dialogs)),
            executor,
            self.lifecycle.clone(),
        );
        let mut coordinator = DesktopCoordinator::new(application, clock, snapshot, services)
            .with_games(Arc::new(game_port.clone()))
            .with_puzzles(puzzle_owner, Arc::new(puzzle_port.clone()));
        if let Some(engine_port) = &engine_port {
            coordinator = coordinator.with_engine(Arc::new(engine_port.clone()));
        }
        let mut phase = CapacityPhase::new(self.budget.clone())?;
        let generation = phase.request_preparation()?;
        let capacity = Arc::new(Mutex::new(phase));
        Self::game_control(game_port.quiesce_all(generation, Self::control_deadline())).await?;
        if !Self::prepare(&capacity, generation).await? {
            return Err(DesktopNativeBootstrapError::HostServices);
        }
        let (events, receiver) = DesktopNativeHost::event_channel();
        let host = DesktopNativeHostServices {
            engine_panels,
            puzzle_panel: Some(PresentationPuzzlePanelConnection {
                owner: puzzle_owner,
                state: puzzle_port.state(),
            }),
            puzzles: puzzle_port,
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
            games: game_port,
            analysis: engine_port,
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
    ) -> Result<
        (DesktopEnumEngineSnapshot, Option<EnginePackageSelected>),
        DesktopNativeBootstrapError,
    > {
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
                package.clone(),
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
            let engine = services
                .engine
                .as_mut()
                .ok_or(DesktopNativeBootstrapError::HostServices)?;
            engine.shutdown_retained(Self::control_deadline()).await?;
            if !engine.is_shutdown_complete() {
                return Err(DesktopNativeBootstrapError::CleanupDeadline);
            }
            services.engine = None;
            Ok::<_, DesktopNativeBootstrapError>((snapshot, Some(package)))
        }
        .await;
        match outcome {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                eprintln!("Reckless engine startup failed: {error}");
                let mut services = lifecycle.services.lock().await;
                if let Some(startup) = &mut services.startup {
                    let cleanup = startup.shutdown_retained(Self::control_deadline()).await;
                    if startup.is_shutdown_complete() {
                        services.startup = None;
                    }
                    if let Err(cleanup) = cleanup {
                        services.retain_terminal(DesktopNativeBootstrapError::Engine(cleanup));
                    }
                }
                if let Some(engine) = &mut services.engine {
                    let cleanup = engine.shutdown_retained(Self::control_deadline()).await;
                    if engine.is_shutdown_complete() {
                        services.engine = None;
                    }
                    if let Err(cleanup) = cleanup {
                        services.retain_terminal(DesktopNativeBootstrapError::Engine(cleanup));
                    }
                }
                if services.engine.is_some() || services.startup.is_some() {
                    return Err(DesktopNativeBootstrapError::CleanupDeadline);
                }
                Ok((
                    DesktopEnumEngineSnapshot::Unavailable {
                        message: format!(
                            "The packaged Reckless engine is unavailable: {error}. Repair the engine package and restart Chess."
                        ),
                    },
                    None,
                ))
            }
        }
    }

    async fn observe_host(
        mut observation: DesktopNativeBootstrapHostObservation<'_, '_>,
    ) -> Result<(), DesktopNativeBootstrapError> {
        let mut graphics_preparation = None;
        loop {
            if *observation.host_done.borrow() {
                Self::phase(&observation.capacity, |phase| {
                    phase.request_teardown()?;
                    if let Some(generation) = phase.generation()
                        && phase.state() == CapacityPhaseEnumState::Draining
                    {
                        phase.acknowledge_preparation_stopped(generation)?;
                    }
                    phase.begin_teardown()
                })
                .await?;
                return Ok(());
            }
            tokio::select! {
                biased;
                changed = observation.host_done.changed() => {
                    changed.map_err(|_closed| DesktopNativeBootstrapError::HostEvents)?;
                }
                changed = observation.events.changed() => {
                    if changed.is_err() {
                        if !*observation.host_done.borrow() {
                            observation.host_done.changed().await.map_err(|_closed| DesktopNativeBootstrapError::HostEvents)?;
                        }
                        continue;
                    }
                    let event = *observation.events.borrow_and_update();
                    Self::observe_event(&mut observation, event, &mut graphics_preparation).await?;
                    observation.wake.wake().map_err(DesktopApplicationError::Wake)?;
                }
            }
        }
    }

    async fn observe_event(
        observation: &mut DesktopNativeBootstrapHostObservation<'_, '_>,
        event: DesktopNativeHostEvent,
        graphics_preparation: &mut Option<CapacityPhaseGeneration>,
    ) -> Result<(), DesktopNativeBootstrapError> {
        let capacity = &observation.capacity;
        let games = &observation.games;
        let generation = match event {
            DesktopNativeHostEvent::Idle => return Ok(()),
            DesktopNativeHostEvent::PreparationRequested { generation }
            | DesktopNativeHostEvent::GraphicsPreparationStopped { generation, .. }
            | DesktopNativeHostEvent::FrameworkReady { generation }
            | DesktopNativeHostEvent::Interrupted { generation } => generation,
        };
        if Self::preparation_event(capacity, generation, |_phase| Ok(()))
            .await?
            .is_none()
        {
            return Ok(());
        }
        if matches!(
            event,
            DesktopNativeHostEvent::PreparationRequested { .. }
                | DesktopNativeHostEvent::GraphicsPreparationStopped { .. }
                | DesktopNativeHostEvent::Interrupted { .. }
        ) && let Some(analysis) = &observation.analysis
        {
            let deadline = Self::control_deadline();
            timeout_at(deadline, analysis.quiesce(deadline))
                .await
                .map_err(DesktopNativeBootstrapError::EngineQuiescenceDeadline)??;
        }
        match event {
            DesktopNativeHostEvent::Idle => {}
            DesktopNativeHostEvent::PreparationRequested { generation } => {
                if !Self::quiesce_preparation(capacity, games, generation).await? {
                    return Ok(());
                }
                if Self::prepare(capacity, generation).await? {
                    observation
                        .publication
                        .transition(DesktopProfileEnumPhase::Recovery)?;
                }
            }
            DesktopNativeHostEvent::GraphicsPreparationStopped {
                generation,
                received_at,
            } => {
                if !Self::quiesce_preparation(capacity, games, generation).await? {
                    return Ok(());
                }
                if Self::preparation_event(capacity, generation, |phase| {
                    phase
                        .complete_interruption(generation, CapacityPhaseEnumInterruption::Failed)?;
                    phase.open_interrupted_admission()
                })
                .await?
                .is_none()
                {
                    return Ok(());
                }
                let Some(control) = Self::preparation_event(capacity, generation, |_phase| {
                    Ok(games
                        .registry_port()
                        .pause_graphics_all(received_at, Self::control_deadline()))
                })
                .await?
                else {
                    return Ok(());
                };
                Self::game_control(control).await?;
                let Some(fresh) = Self::preparation_event(
                    capacity,
                    generation,
                    CapacityPhase::request_preparation,
                )
                .await?
                else {
                    return Ok(());
                };
                if !Self::quiesce_preparation(capacity, games, fresh).await? {
                    return Ok(());
                }
                if Self::prepare(capacity, fresh).await? {
                    *graphics_preparation = Some(fresh);
                    observation
                        .publication
                        .transition(DesktopProfileEnumPhase::Recovery)?;
                }
            }
            DesktopNativeHostEvent::FrameworkReady { generation } => {
                if Self::preparation_event(capacity, generation, |phase| {
                    phase.complete_preparation(generation)
                })
                .await?
                .is_none()
                {
                    return Ok(());
                }
                if *graphics_preparation == Some(generation) {
                    let Some(control) = Self::preparation_event(capacity, generation, |_phase| {
                        Ok(games
                            .registry_port()
                            .recover_graphics_all(Self::control_deadline()))
                    })
                    .await?
                    else {
                        return Ok(());
                    };
                    Self::game_control(control).await?;
                    *graphics_preparation = None;
                }
                let Some(control) = Self::preparation_event(capacity, generation, |_phase| {
                    Ok(games.resume_all(generation, Self::control_deadline()))
                })
                .await?
                else {
                    return Ok(());
                };
                Self::game_control(control).await?;
                observation
                    .publication
                    .transition(DesktopProfileEnumPhase::Ready)?;
            }
            DesktopNativeHostEvent::Interrupted { generation } => {
                if Self::preparation_event(capacity, generation, |phase| {
                    phase.complete_interruption(generation, CapacityPhaseEnumInterruption::Failed)
                })
                .await?
                .is_some()
                {
                    observation
                        .publication
                        .transition(DesktopProfileEnumPhase::Recovery)?;
                }
            }
        }
        Ok(())
    }

    async fn quiesce_preparation(
        capacity: &Mutex<CapacityPhase>,
        games: &DesktopNativeGameRuntimePort,
        generation: CapacityPhaseGeneration,
    ) -> Result<bool, DesktopNativeBootstrapError> {
        let Some(control) = Self::preparation_event(capacity, generation, |_phase| {
            Ok(games.quiesce_all(generation, Self::control_deadline()))
        })
        .await?
        else {
            return Ok(false);
        };
        Self::game_control(control).await?;
        Ok(true)
    }

    async fn preparation_event<T>(
        capacity: &Mutex<CapacityPhase>,
        generation: CapacityPhaseGeneration,
        mut operation: impl FnMut(&mut CapacityPhase) -> Result<T, CapacityPhaseError>,
    ) -> Result<Option<T>, DesktopNativeBootstrapError> {
        Self::phase(capacity, |phase| {
            if phase.accepts_preparation_event(generation)? {
                operation(phase).map(Some)
            } else {
                Ok(None)
            }
        })
        .await
    }

    async fn prepare(
        capacity: &Mutex<CapacityPhase>,
        generation: CapacityPhaseGeneration,
    ) -> Result<bool, DesktopNativeBootstrapError> {
        timeout_at(
            Self::control_deadline(),
            Self::prepare_retained(capacity, generation),
        )
        .await
        .map_err(|_elapsed| DesktopNativeBootstrapError::CleanupDeadline)?
    }

    async fn prepare_retained(
        capacity: &Mutex<CapacityPhase>,
        generation: CapacityPhaseGeneration,
    ) -> Result<bool, DesktopNativeBootstrapError> {
        if Self::preparation_event(capacity, generation, |phase| {
            if phase.state() == CapacityPhaseEnumState::Preparing
                && phase.generation() == Some(generation)
            {
                Ok(())
            } else {
                phase.acknowledge_searches_stopped(generation)
            }
        })
        .await?
        .is_none()
        {
            return Ok(false);
        }
        loop {
            match Self::preparation_event(capacity, generation, |phase| {
                if phase.state() == CapacityPhaseEnumState::Preparing
                    && phase.generation() == Some(generation)
                {
                    Ok(())
                } else {
                    phase.try_enter_preparation(generation)
                }
            })
            .await
            {
                Err(DesktopNativeBootstrapError::Phase(CapacityPhaseError::QuiescencePending)) => {
                    tokio::task::yield_now().await
                }
                result => return result.map(|outcome| outcome.is_some()),
            }
        }
    }

    async fn phase<T>(
        capacity: &Mutex<CapacityPhase>,
        mut operation: impl FnMut(&mut CapacityPhase) -> Result<T, CapacityPhaseError>,
    ) -> Result<T, DesktopNativeBootstrapError> {
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
            DesktopNativeStartupEnumAdmission::Owned(owner) => {
                Self::owned(owner, security, CapacityBudget::discover()?, host)
            }
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
        budget: CapacityBudget,
        host: impl FnOnce(DesktopNativeHostServices) -> Result<(), DesktopNativeHostError>,
    ) -> Result<(), DesktopNativeBootstrapError> {
        let (lease, binding) = owner.into_parts();
        let lease: Arc<DesktopProfileLease> = Arc::from(lease);
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
