use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use bevy::app::{PostUpdate, PreUpdate};
use bevy::prelude::{App, Entity, IntoScheduleConfigs, Mut, Plugin, Resource, With, World};
use bevy::window::{PrimaryWindow, WindowCloseRequested};
use bevy::winit::WinitSettings;
use bevy_egui::{
    EguiContext, EguiFullOutput, EguiInput, EguiPostUpdateSet, EguiPreUpdateSet,
    PrimaryEguiContext, egui,
};
use thiserror::Error;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::watch;
use uuid::Uuid;

use chess::capacity::budget::{CapacityBudget, CapacityBudgetError};
use chess::clock::time_provider::ClockTimeProviderMonotonic;
use chess::desktop::application::{
    DesktopApplicationError, DesktopApplicationPort, DesktopApplicationReceipt,
};
use chess::desktop::dto::{DesktopEnumEngineSnapshot, DesktopEnumValue};
use chess::desktop::identity::DesktopRequestId;
use chess::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEnumResult};
use chess::desktop::native::bootstrap::{DesktopNativeBootstrap, DesktopNativeBootstrapError};
use chess::desktop::native::event_loop::DesktopNativeEventLoopWake;
use chess::desktop::native::host::{DesktopNativeHost, DesktopNativeHostEvent};
use chess::desktop::native::recovery::{
    DesktopNativeRecoveryEnumAction, DesktopNativeRecoveryEnumStage, DesktopNativeRecoveryError,
    DesktopNativeRecoveryFailure, DesktopNativeRecoveryProvider,
};
use chess::desktop::native::startup::{
    DesktopNativeStartup, DesktopNativeStartupEnumAdmission, DesktopNativeStartupError,
    DesktopNativeStartupOwner,
};
use chess::desktop::profile::owner_record::{
    DesktopProfileEnumPhase, DesktopProfileOwnerRecord, DesktopProfileOwnerRecordError,
};
use chess::desktop::profile::{
    DesktopProfileError, DesktopProfileId, DesktopProfileOsLock, DesktopProfileSecurity,
};
use chess::storage::document_store::{StorageDocumentError, StorageDocumentStore};

#[derive(Debug, Error)]
pub(super) enum NativeHostLifecycleError {
    #[error("Native host lifecycle I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Capacity(#[from] CapacityBudgetError),
    #[error(transparent)]
    Profile(#[from] DesktopProfileError),
    #[error(transparent)]
    Startup(#[from] DesktopNativeStartupError),
    #[error(transparent)]
    Bootstrap(#[from] DesktopNativeBootstrapError),
    #[error(transparent)]
    OwnerRecord(#[from] DesktopProfileOwnerRecordError),
    #[error(transparent)]
    Storage(#[from] StorageDocumentError),
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
    #[error("Native host lifecycle invariant failed: {0}")]
    Invariant(&'static str),
    #[error("Native host lifecycle did not finish within {0:?}")]
    Deadline(Duration),
    #[error("Production graphics entered recovery at {stage:?}: {message}")]
    Graphics {
        stage: DesktopNativeRecoveryEnumStage,
        message: String,
    },
    #[error("The native lifecycle driver failed: {0}")]
    Driver(#[source] Arc<Self>),
}

#[derive(Default)]
struct NativeHostLifecycleObservation {
    ready: AtomicBool,
    engine_ready: AtomicBool,
    first_confirmation: AtomicBool,
    cancellation: AtomicBool,
    second_confirmation: AtomicBool,
    confirmation: AtomicBool,
    failure: OnceLock<Arc<NativeHostLifecycleError>>,
}

impl NativeHostLifecycleObservation {
    fn fail(&self, error: NativeHostLifecycleError) {
        self.failure.get_or_init(|| Arc::new(error));
    }

    fn validate(&self) -> Result<(), NativeHostLifecycleError> {
        if let Some(error) = self.failure.get() {
            return Err(NativeHostLifecycleError::Driver(Arc::clone(error)));
        }
        for (observed, requirement) in [
            (&self.ready, "production graphics reached FrameworkReady"),
            (
                &self.engine_ready,
                "the actual bundled Reckless engine initialized",
            ),
            (
                &self.first_confirmation,
                "native Close opened the first quit confirmation",
            ),
            (
                &self.cancellation,
                "Keep open preserved a responsive application",
            ),
            (
                &self.second_confirmation,
                "native Close reopened quit confirmation",
            ),
            (
                &self.confirmation,
                "Confirm quit was submitted through real egui semantics",
            ),
        ] {
            if !observed.load(Ordering::Acquire) {
                return Err(NativeHostLifecycleError::Invariant(requirement));
            }
        }
        Ok(())
    }
}

struct NativeHostLifecycleRecovery(Arc<NativeHostLifecycleObservation>);

impl DesktopNativeRecoveryProvider for NativeHostLifecycleRecovery {
    fn choose(
        &self,
        failure: &DesktopNativeRecoveryFailure,
    ) -> Result<DesktopNativeRecoveryEnumAction, DesktopNativeRecoveryError> {
        self.0.fail(NativeHostLifecycleError::Graphics {
            stage: failure.stage(),
            message: failure.message().to_owned(),
        });
        Ok(DesktopNativeRecoveryEnumAction::SafeQuit)
    }
}

enum NativeHostLifecycleEnumPhase {
    AwaitReady,
    AwaitFirstConfirmation,
    AwaitCancellation,
    AwaitSecondConfirmation,
    AwaitShutdown,
    Failing,
}

#[derive(Resource)]
struct NativeHostLifecycleDriver {
    observation: Arc<NativeHostLifecycleObservation>,
    events: watch::Receiver<DesktopNativeHostEvent>,
    port: DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
    bootstrap: Option<DesktopApplicationReceipt>,
    main_thread: ThreadId,
    started: Instant,
    phase: NativeHostLifecycleEnumPhase,
    input: Option<egui::accesskit::NodeId>,
    close_pending: bool,
}

impl NativeHostLifecycleDriver {
    const DEADLINE: Duration = Duration::from_secs(15);

    fn system(
        world: &mut World,
        operation: fn(&mut Self, &mut World) -> Result<(), NativeHostLifecycleError>,
    ) {
        world.resource_scope(|world, mut driver: Mut<'_, Self>| {
            if let Err(error) = operation(&mut driver, world) {
                driver.observation.fail(error);
                driver.phase = NativeHostLifecycleEnumPhase::Failing;
                driver.close_pending = true;
            }
        });
    }

    fn input_system(world: &mut World) {
        Self::system(world, Self::inject_input);
    }

    fn output_system(world: &mut World) {
        Self::system(world, Self::observe_output);
    }

    fn inject_input(&mut self, world: &mut World) -> Result<(), NativeHostLifecycleError> {
        if thread::current().id() != self.main_thread {
            return Err(NativeHostLifecycleError::Invariant(
                "the production host stayed on the OS event-loop thread",
            ));
        }
        if !matches!(self.phase, NativeHostLifecycleEnumPhase::Failing)
            && self.started.elapsed() >= Self::DEADLINE
        {
            return Err(NativeHostLifecycleError::Deadline(Self::DEADLINE));
        }
        let mut contexts =
            world.query_filtered::<(&mut EguiContext, &mut EguiInput), With<PrimaryEguiContext>>();
        if let Ok((mut context, mut input)) = contexts.single_mut(world) {
            context.get_mut().enable_accesskit();
            if let Some(target_node) = self.input.take() {
                input.0.events.push(egui::Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Click,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node,
                        data: None,
                    },
                ));
                if matches!(self.phase, NativeHostLifecycleEnumPhase::AwaitShutdown) {
                    self.observation.confirmation.store(true, Ordering::Release);
                }
            }
        }
        if self.close_pending {
            let mut windows = world.query_filtered::<Entity, With<PrimaryWindow>>();
            if let Ok(window) = windows.single(world) {
                world.write_message(WindowCloseRequested { window });
                self.close_pending = false;
            }
        }
        Ok(())
    }

    fn request_bootstrap(&mut self) -> Result<(), NativeHostLifecycleError> {
        if self.bootstrap.is_none() {
            let request = self
                .port
                .request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?;
            match self.port.try_submit(&request) {
                Ok(receipt) => self.bootstrap = Some(receipt),
                Err(DesktopApplicationError::Busy) => self.port.rearm_wake()?,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn bootstrap_ready(&self) -> Result<bool, NativeHostLifecycleError> {
        match self
            .bootstrap
            .as_ref()
            .and_then(DesktopApplicationReceipt::response)
        {
            Some(DesktopMessagesEnumResult::Value {
                value: DesktopEnumValue::Bootstrap(snapshot),
                ..
            }) => match snapshot.engine() {
                DesktopEnumEngineSnapshot::Ready { identity } => {
                    if identity.name().is_empty() || identity.supported_options().is_empty() {
                        return Err(NativeHostLifecycleError::Invariant(
                            "Reckless returned its real identity and UCI capabilities",
                        ));
                    }
                    Ok(true)
                }
                DesktopEnumEngineSnapshot::Unavailable { .. } => {
                    Err(NativeHostLifecycleError::Invariant(
                        "the bundled Reckless engine must initialize for lifecycle acceptance",
                    ))
                }
            },
            Some(
                DesktopMessagesEnumResult::Value { .. }
                | DesktopMessagesEnumResult::Failure(_)
                | DesktopMessagesEnumResult::LibraryFailure(_)
                | DesktopMessagesEnumResult::EditorFailure(_)
                | DesktopMessagesEnumResult::Cancelled
                | DesktopMessagesEnumResult::Stopped
                | DesktopMessagesEnumResult::Paused
                | DesktopMessagesEnumResult::QuitReady
                | DesktopMessagesEnumResult::ReconciliationRequired,
            ) => Err(NativeHostLifecycleError::Invariant(
                "bootstrap returned an authoritative snapshot",
            )),
            None => Ok(false),
        }
    }

    fn button(output: &egui::FullOutput, label: &str) -> Option<egui::accesskit::NodeId> {
        output
            .platform_output
            .accesskit_update
            .as_ref()?
            .nodes
            .iter()
            .find(|(_, node)| {
                node.role() == egui::accesskit::Role::Button && node.label() == Some(label)
            })
            .map(|(id, _node)| *id)
    }

    fn observe_output(&mut self, world: &mut World) -> Result<(), NativeHostLifecycleError> {
        world.insert_resource(WinitSettings::continuous());
        let mut outputs = world.query_filtered::<&EguiFullOutput, With<PrimaryEguiContext>>();
        let Ok(output) = outputs.single(world) else {
            return Ok(());
        };
        let Some(output) = output.0.as_ref() else {
            return Ok(());
        };
        let keep_open = Self::button(output, "Keep open");
        let confirm = Self::button(output, "Confirm quit");
        match self.phase {
            NativeHostLifecycleEnumPhase::AwaitReady => {
                self.request_bootstrap()?;
                if matches!(
                    *self.events.borrow(),
                    DesktopNativeHostEvent::FrameworkReady { .. }
                ) && self.bootstrap_ready()?
                    && Self::button(output, "Quit").is_some()
                {
                    self.observation.ready.store(true, Ordering::Release);
                    self.observation.engine_ready.store(true, Ordering::Release);
                    self.phase = NativeHostLifecycleEnumPhase::AwaitFirstConfirmation;
                    self.close_pending = true;
                }
            }
            NativeHostLifecycleEnumPhase::AwaitFirstConfirmation => {
                if let (Some(keep_open), Some(_confirm)) = (keep_open, confirm) {
                    self.observation
                        .first_confirmation
                        .store(true, Ordering::Release);
                    self.bootstrap = None;
                    self.input = Some(keep_open);
                    self.phase = NativeHostLifecycleEnumPhase::AwaitCancellation;
                }
            }
            NativeHostLifecycleEnumPhase::AwaitCancellation => {
                if keep_open.is_none() && confirm.is_none() && self.input.is_none() {
                    self.request_bootstrap()?;
                    if self.bootstrap_ready()? {
                        self.observation.cancellation.store(true, Ordering::Release);
                        self.phase = NativeHostLifecycleEnumPhase::AwaitSecondConfirmation;
                        self.close_pending = true;
                    }
                }
            }
            NativeHostLifecycleEnumPhase::AwaitSecondConfirmation => {
                if let (Some(_keep_open), Some(confirm)) = (keep_open, confirm) {
                    self.observation
                        .second_confirmation
                        .store(true, Ordering::Release);
                    self.input = Some(confirm);
                    self.phase = NativeHostLifecycleEnumPhase::AwaitShutdown;
                }
            }
            NativeHostLifecycleEnumPhase::AwaitShutdown => {}
            NativeHostLifecycleEnumPhase::Failing => {
                if let Some(confirm) = confirm {
                    self.input = Some(confirm);
                } else {
                    self.close_pending = true;
                }
            }
        }
        Ok(())
    }
}

struct NativeHostLifecyclePlugin {
    observation: Arc<NativeHostLifecycleObservation>,
    events: watch::Receiver<DesktopNativeHostEvent>,
    port: DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
    main_thread: ThreadId,
}

impl Plugin for NativeHostLifecyclePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NativeHostLifecycleDriver {
            observation: Arc::clone(&self.observation),
            events: self.events.clone(),
            port: self.port.clone(),
            bootstrap: None,
            main_thread: self.main_thread,
            started: Instant::now(),
            phase: NativeHostLifecycleEnumPhase::AwaitReady,
            input: None,
            close_pending: false,
        })
        .add_systems(
            PreUpdate,
            NativeHostLifecycleDriver::input_system
                .after(EguiPreUpdateSet::ProcessInput)
                .before(EguiPreUpdateSet::BeginPass),
        )
        .add_systems(
            PostUpdate,
            NativeHostLifecycleDriver::output_system
                .after(EguiPostUpdateSet::EndPass)
                .before(EguiPostUpdateSet::ProcessOutput),
        );
    }
}

pub(super) struct NativeHostLifecycleHarness;

impl NativeHostLifecycleHarness {
    const PROFILE_ID: u128 = 0x8a7c6693_3c20_4fa8_8c98_0d7319cbeeb7;
    const DOCUMENT_ID: Uuid = Uuid::from_u128(0xb07042b2_f344_4263_94ae_2217b746b9d1);
    const DOCUMENT: &str = r#"{"fixture":"native-host-lifecycle"}"#;

    fn owner(
        profile: &Path,
        security: &DesktopProfileSecurity,
    ) -> Result<DesktopNativeStartupOwner, NativeHostLifecycleError> {
        let startup = DesktopNativeStartup::new(security.clone(), DesktopProfileOsLock);
        match startup.admit(
            profile,
            DesktopProfileId::from_uuid(Uuid::from_u128(Self::PROFILE_ID))?,
            DesktopRequestId::new(),
        )? {
            DesktopNativeStartupEnumAdmission::Owned(owner) => Ok(owner),
            DesktopNativeStartupEnumAdmission::Busy
            | DesktopNativeStartupEnumAdmission::ActivationOnly(_)
            | DesktopNativeStartupEnumAdmission::InstallationBlocked(_) => {
                Err(NativeHostLifecycleError::Invariant(
                    "the isolated profile is exclusively owned and can be reacquired after shutdown",
                ))
            }
        }
    }

    fn runtime(budget: &CapacityBudget) -> Result<Runtime, NativeHostLifecycleError> {
        Ok(Builder::new_current_thread()
            .max_blocking_threads(budget.workers())
            .enable_all()
            .build()?)
    }

    pub(super) fn run() -> Result<(), NativeHostLifecycleError> {
        let directory = tempfile::tempdir()?;
        let profile = directory.path().join("profile");
        let security = DesktopProfileSecurity::current()?;
        let budget = CapacityBudget::discover()?;
        {
            let owner = Self::owner(&profile, &security)?;
            let runtime = Self::runtime(&budget)?;
            runtime.block_on(async {
                let storage =
                    StorageDocumentStore::open(profile.join("library.sqlite3"), budget.clone())
                        .await?;
                let receipt = storage
                    .client()
                    .save(Self::DOCUMENT_ID, 0, Self::DOCUMENT.to_owned())
                    .await?;
                if receipt.revision() != 1 {
                    return Err(NativeHostLifecycleError::Invariant(
                        "the durable seed document has its initial revision",
                    ));
                }
                storage.close().await?;
                Ok::<(), NativeHostLifecycleError>(())
            })?;
            owner.lease().verify_identity()?;
        }
        let observation = Arc::new(NativeHostLifecycleObservation::default());
        DesktopNativeBootstrap::run_with_host(
            [
                OsString::from("--profile-directory"),
                profile.as_os_str().to_owned(),
            ],
            |mut services| {
                services.recovery = Arc::new(NativeHostLifecycleRecovery(Arc::clone(&observation)));
                let plugin = NativeHostLifecyclePlugin {
                    observation: Arc::clone(&observation),
                    events: services.events.subscribe(),
                    port: services.port.clone(),
                    main_thread: thread::current().id(),
                };
                DesktopNativeHost::run_with_plugin(services, plugin)
            },
        )?;
        observation.validate()?;
        let record = DesktopProfileOwnerRecord::read(&profile, &security)?.ok_or(
            NativeHostLifecycleError::Invariant(
                "the production bootstrap published its owner record",
            ),
        )?;
        if record.phase() != DesktopProfileEnumPhase::ShuttingDown {
            return Err(NativeHostLifecycleError::Invariant(
                "the production activation owner reached ShuttingDown",
            ));
        }
        #[cfg(unix)]
        {
            let socket =
                record
                    .endpoint()
                    .unix_path()
                    .ok_or(NativeHostLifecycleError::Invariant(
                        "the production Unix owner published a Unix activation endpoint",
                    ))?;
            if socket.exists() {
                return Err(NativeHostLifecycleError::Invariant(
                    "the production activation listener removed its socket",
                ));
            }
        }
        let owner = Self::owner(&profile, &security)?;
        if owner.lease().identity().profile_id()
            != DesktopProfileId::from_uuid(Uuid::from_u128(Self::PROFILE_ID))?
        {
            return Err(NativeHostLifecycleError::Invariant(
                "the durable profile identity survived production shutdown",
            ));
        }
        let runtime = Self::runtime(&budget)?;
        runtime.block_on(async {
            let storage =
                StorageDocumentStore::open(profile.join("library.sqlite3"), budget).await?;
            let document = storage.client().read(Self::DOCUMENT_ID).await?.ok_or(
                NativeHostLifecycleError::Invariant(
                    "the durable seed document survived native startup and shutdown",
                ),
            )?;
            if document.revision() != 1 || document.body() != Self::DOCUMENT {
                return Err(NativeHostLifecycleError::Invariant(
                    "native startup preserved the durable seed document and revision",
                ));
            }
            drop(document);
            storage.close().await?;
            Ok::<(), NativeHostLifecycleError>(())
        })?;
        owner.lease().verify_identity()?;
        println!(
            "Native host lifecycle passed: production startup, Reckless identity, graphics readiness, quit cancellation, explicit quit confirmation, owned cleanup, profile reacquisition, and durable SQLite reopen"
        );
        Ok(())
    }
}
