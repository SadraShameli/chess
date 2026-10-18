use std::io::{BufRead, BufReader};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use std::time::Instant;

use semver::Version;
use tokio::sync::{mpsc, watch};

use uuid::Uuid;

use crate::desktop::backup_bridge::DesktopBackupService;
use crate::desktop::backup_commands::DesktopBackupCommandsPrerequisites;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::native::credentials::DesktopNativeCredentials;
use crate::desktop::native::update::space::DesktopNativeUpdateSpace;
use crate::desktop::profile::DesktopProfileLease;
use crate::desktop::profile::installation::DesktopProfileInstallationSpec;
use crate::desktop::update_commands::{
    DesktopUpdateCommands, DesktopUpdateError, DesktopUpdateInstallInputs, DesktopUpdateOperation,
    DesktopUpdateOptions, DesktopUpdateProvider, DesktopUpdateSnapshot,
};
use crate::update::cache::UpdateCache;
use crate::update::credentials::UpdateCredentials;
use crate::update::discovery::{UpdateOrigins, UpdateRepository};
use crate::update::http::UpdateReqwest;
use crate::update::install::{
    UpdateHelperEnumReadiness, UpdateInstall, UpdateInstallConfirmation, UpdateInstallEnvironment,
    UpdateInstallRequest, UpdateIntentLeaseWriter,
};
use crate::update::manifest::UpdateTarget;
use crate::update::network_cost::UpdateNetworkCostUnsupported;
use crate::update::state::UpdateEnumPhase;
use crate::update::verifier::UpdateVerifier;

type DesktopNativeUpdateCommands = DesktopUpdateCommands<
    UpdateReqwest,
    UpdateVerifier,
    DesktopNativeCredentials,
    UpdateNetworkCostUnsupported,
>;

pub struct DesktopNativeUpdateInstallContext {
    pub backup: Arc<DesktopBackupService>,
    pub lease: Arc<DesktopProfileLease>,
    pub quit: Arc<dyn Fn() + Send + Sync>,
    pub data_schema: NonZeroU32,
}

pub struct DesktopNativeUpdateServiceOptions {
    pub cache_directory: PathBuf,
    pub installed: Version,
    pub runtime: tokio::runtime::Handle,
    pub install: Option<DesktopNativeUpdateInstallContext>,
}

struct DesktopNativeUpdateEnvironment {
    prerequisites: DesktopBackupCommandsPrerequisites,
    backup: Uuid,
    command: Command,
    child: Option<Child>,
    ready_timeout: Duration,
}

impl DesktopNativeUpdateEnvironment {
    const READY_TIMEOUT: Duration = Duration::from_secs(10);

    fn stop_helper(&mut self) {
        if let Some(child) = &mut self.child {
            let _killed = child.kill();
            let _reaped = child.wait();
        }
    }
}

impl UpdateInstallEnvironment for DesktopNativeUpdateEnvironment {
    fn resolve_unsaved_edits(&mut self) -> Result<(), String> {
        if self.prerequisites.editors_have_no_unsaved_edits {
            Ok(())
        } else {
            Err("Unsaved editor changes must be saved or discarded first".to_owned())
        }
    }

    fn pause_and_save_games(&mut self) -> Result<(), String> {
        if self.prerequisites.games_paused_and_saved {
            Ok(())
        } else {
            Err("Every game must be paused and saved first".to_owned())
        }
    }

    fn safety_backup(&mut self) -> Result<DesktopRequestId, String> {
        DesktopRequestId::from_str(&self.backup.to_string()).map_err(|error| error.to_string())
    }

    fn start_helper(&mut self, spec: &DesktopProfileInstallationSpec) -> UpdateHelperEnumReadiness {
        let generation = String::from(spec.helper_generation);
        let spawned = self
            .command
            .arg("--generation")
            .arg(&generation)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = spawned else {
            return UpdateHelperEnumReadiness::Lost;
        };
        let Some(stdout) = child.stdout.take() else {
            self.child = Some(child);
            self.stop_helper();
            return UpdateHelperEnumReadiness::Lost;
        };
        self.child = Some(child);
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _read = BufReader::new(stdout).read_line(&mut line);
            let _sent = sender.send(line);
        });
        match receiver.recv_timeout(self.ready_timeout) {
            Ok(line) if line.trim() == format!("ready {generation}") => {
                UpdateHelperEnumReadiness::Ready(spec.helper_generation)
            }
            Ok(_) | Err(_) => {
                self.stop_helper();
                UpdateHelperEnumReadiness::Lost
            }
        }
    }

    fn confirm_helper_stopped(&mut self, _spec: &DesktopProfileInstallationSpec) -> bool {
        match &mut self.child {
            Some(child) => child.try_wait().is_ok_and(|status| status.is_some()),
            None => true,
        }
    }
}

struct DesktopNativeUpdateBuild {
    http: UpdateReqwest,
    verifier: UpdateVerifier,
    repository: UpdateRepository,
    target: UpdateTarget,
    cache_directory: PathBuf,
    installed: Version,
}

pub struct DesktopNativeUpdateService {
    shared: Arc<Mutex<DesktopUpdateSnapshot>>,
    commands: Option<mpsc::UnboundedSender<DesktopUpdateOperation>>,
}

impl DesktopNativeUpdateService {
    pub const REPOSITORY: Option<&'static str> = None;

    pub fn disabled(reason: &str) -> Self {
        Self {
            shared: Arc::new(Mutex::new(DesktopUpdateSnapshot::disabled(
                reason.to_owned(),
            ))),
            commands: None,
        }
    }

    pub fn start(options: DesktopNativeUpdateServiceOptions) -> Self {
        let Some((owner, name)) = Self::REPOSITORY.and_then(|text| text.split_once('/')) else {
            return Self::disabled("No update repository is configured for this build");
        };
        let repository = match UpdateRepository::new(owner, name) {
            Ok(repository) => repository,
            Err(error) => return Self::disabled(&error.to_string()),
        };
        let verifier = match UpdateVerifier::embedded() {
            Ok(verifier) => verifier,
            Err(error) => return Self::disabled(&error.to_string()),
        };
        let Some(target) = UpdateTarget::detect() else {
            return Self::disabled("Updates are not available for this platform");
        };
        let http = match UpdateReqwest::new() {
            Ok(http) => http,
            Err(error) => return Self::disabled(&error.to_string()),
        };
        let shared = Arc::new(Mutex::new(DesktopUpdateSnapshot::new(
            crate::update::state::UpdateState::default(),
            crate::update::credentials::UpdateCredentialEnumStatus::Missing,
            None,
        )));
        let (sender, receiver) = mpsc::unbounded_channel();
        let task_shared = Arc::clone(&shared);
        let DesktopNativeUpdateServiceOptions {
            cache_directory,
            installed,
            runtime,
            install,
        } = options;
        runtime.spawn(Self::run(
            Self::build(DesktopNativeUpdateBuild {
                http,
                verifier,
                repository,
                target,
                cache_directory,
                installed,
            }),
            task_shared,
            receiver,
            install,
        ));
        if sender.send(DesktopUpdateOperation::Check).is_err() {
            return Self::disabled("The update service stopped immediately");
        }
        Self {
            shared,
            commands: Some(sender),
        }
    }

    async fn build(build: DesktopNativeUpdateBuild) -> Option<DesktopNativeUpdateCommands> {
        let DesktopNativeUpdateBuild {
            http,
            verifier,
            repository,
            target,
            cache_directory,
            installed,
        } = build;
        tokio::task::spawn_blocking(move || {
            DesktopUpdateCommands::new(DesktopUpdateOptions {
                http,
                verifier,
                repository,
                origins: UpdateOrigins::github(),
                cache: UpdateCache::new(cache_directory),
                credentials: UpdateCredentials::open(DesktopNativeCredentials::new()),
                network: UpdateNetworkCostUnsupported,
                installed,
                target,
            })
            .ok()
        })
        .await
        .ok()
        .flatten()
    }

    fn publish(shared: &Mutex<DesktopUpdateSnapshot>, snapshot: DesktopUpdateSnapshot) {
        *shared.lock().unwrap_or_else(PoisonError::into_inner) = snapshot;
    }

    async fn run(
        build: impl std::future::Future<Output = Option<DesktopNativeUpdateCommands>>,
        shared: Arc<Mutex<DesktopUpdateSnapshot>>,
        mut receiver: mpsc::UnboundedReceiver<DesktopUpdateOperation>,
        install: Option<DesktopNativeUpdateInstallContext>,
    ) {
        let Some(mut commands) = build.await else {
            Self::publish(
                &shared,
                DesktopUpdateSnapshot::disabled("The update service could not start".to_owned()),
            );
            return;
        };
        Self::publish(&shared, commands.snapshot(None));
        let epoch = Instant::now();
        while let Some(operation) = receiver.recv().await {
            let now_ms = u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
            let outcome = match operation {
                DesktopUpdateOperation::Snapshot => Ok(()),
                DesktopUpdateOperation::Check => commands.check(now_ms).await,
                DesktopUpdateOperation::RequestDownload => commands.request_download(),
                DesktopUpdateOperation::Approve => commands.approve(),
                DesktopUpdateOperation::Cancel => commands.cancel(),
                DesktopUpdateOperation::EnterCredential {
                    credential,
                    persist,
                } => commands.enter_credential(credential, persist),
                DesktopUpdateOperation::Install { prerequisites } => {
                    let outcome = match &install {
                        Some(context) => Self::install(&mut commands, context, prerequisites).await,
                        None => Err(DesktopUpdateError::Install(
                            "Installing is not available in this session".to_owned(),
                        )),
                    };
                    Self::publish(
                        &shared,
                        commands.snapshot(outcome.err().map(|error| error.to_string())),
                    );
                    continue;
                }
            };
            let message = outcome.err().map(|error| error.to_string());
            Self::publish(&shared, commands.snapshot(message));
            if commands.state().phase() == UpdateEnumPhase::Downloading {
                let result = Self::download(&mut commands, &mut receiver).await;
                let message = result.err().map(|error| error.to_string());
                Self::publish(&shared, commands.snapshot(message));
            }
        }
    }

    fn destination(target: UpdateTarget, executable: &Path) -> Option<PathBuf> {
        match target {
            UpdateTarget::MacosArm64 => executable.ancestors().nth(3).map(Path::to_path_buf),
            UpdateTarget::WindowsX86_64 => executable.parent().map(Path::to_path_buf),
            UpdateTarget::LinuxAppImage => UpdateTarget::appimage_path(),
            UpdateTarget::LinuxDeb => Some(executable.to_path_buf()),
        }
    }

    fn helper_command(
        context: &DesktopNativeUpdateInstallContext,
        inputs: &DesktopUpdateInstallInputs,
    ) -> Result<Command, DesktopUpdateError> {
        let failure = |what: &str| DesktopUpdateError::Install(what.to_owned());
        let executable = std::env::current_exe()
            .map_err(|error| DesktopUpdateError::Install(error.to_string()))?;
        let helper = executable
            .parent()
            .map(|directory| {
                directory.join(if cfg!(windows) {
                    "chess-update-helper.exe"
                } else {
                    "chess-update-helper"
                })
            })
            .filter(|path| path.is_file())
            .ok_or_else(|| failure("The update helper is missing from this installation"))?;
        let destination = Self::destination(inputs.target, &executable)
            .ok_or_else(|| failure("The installation location cannot be determined"))?;
        let relaunch = match inputs.target {
            UpdateTarget::LinuxAppImage => destination.clone(),
            UpdateTarget::MacosArm64 | UpdateTarget::WindowsX86_64 | UpdateTarget::LinuxDeb => {
                executable
            }
        };
        let mut command = Command::new(helper);
        command
            .arg("--data-dir")
            .arg(context.lease.identity().data_directory())
            .arg("--package")
            .arg(&inputs.package)
            .arg("--target")
            .arg(inputs.target.to_string())
            .arg("--version")
            .arg(inputs.version.to_string())
            .arg("--destination")
            .arg(destination)
            .arg("--parent-pid")
            .arg(std::process::id().to_string())
            .arg("--relaunch")
            .arg(relaunch);
        Ok(command)
    }

    async fn install(
        commands: &mut DesktopNativeUpdateCommands,
        context: &DesktopNativeUpdateInstallContext,
        prerequisites: DesktopBackupCommandsPrerequisites,
    ) -> Result<(), DesktopUpdateError> {
        let inputs = commands.begin_install()?;
        let handoff = Self::handoff(context, &inputs, prerequisites).await;
        match handoff {
            Ok(()) => {
                commands.begin_handoff()?;
                (context.quit)();
                Ok(())
            }
            Err(error) => {
                commands.abort_install()?;
                Err(error)
            }
        }
    }

    async fn handoff(
        context: &DesktopNativeUpdateInstallContext,
        inputs: &DesktopUpdateInstallInputs,
        prerequisites: DesktopBackupCommandsPrerequisites,
    ) -> Result<(), DesktopUpdateError> {
        if let Some(unmet) = prerequisites.first_unmet() {
            return Err(DesktopUpdateError::Install(format!(
                "{unmet:?} must be satisfied first"
            )));
        }
        let backup = context
            .backup
            .pre_update_backup(prerequisites)
            .await
            .map_err(|failure| DesktopUpdateError::Install(failure.to_string()))?;
        let command = Self::helper_command(context, inputs)?;
        let lease = Arc::clone(&context.lease);
        let schema = context.data_schema;
        let inputs = inputs.clone();
        tokio::task::spawn_blocking(move || {
            let mut environment = DesktopNativeUpdateEnvironment {
                prerequisites,
                backup,
                command,
                child: None,
                ready_timeout: DesktopNativeUpdateEnvironment::READY_TIMEOUT,
            };
            UpdateInstall::run(
                UpdateInstallConfirmation::explicit(),
                &UpdateInstallRequest {
                    asset: &inputs.asset,
                    package: &inputs.package,
                    version: inputs.version.clone(),
                    data_schema: schema,
                },
                &mut environment,
                &UpdateIntentLeaseWriter(&lease),
            )
            .map(|_spec| ())
            .map_err(|error| DesktopUpdateError::Install(error.to_string()))
        })
        .await
        .map_err(|error| DesktopUpdateError::Install(error.to_string()))?
    }

    async fn download(
        commands: &mut DesktopNativeUpdateCommands,
        receiver: &mut mpsc::UnboundedReceiver<DesktopUpdateOperation>,
    ) -> Result<(), crate::desktop::update_commands::DesktopUpdateError> {
        let (cancel, signal) = watch::channel(false);
        let download = commands.download(&DesktopNativeUpdateSpace, signal);
        tokio::pin!(download);
        loop {
            tokio::select! {
                result = &mut download => return result,
                next = receiver.recv() => match next {
                    Some(DesktopUpdateOperation::Cancel) | None => {
                        let _ignored = cancel.send(true);
                    }
                    Some(_other) => {}
                },
            }
        }
    }
}

impl DesktopUpdateProvider for DesktopNativeUpdateService {
    fn execute(&self, operation: DesktopUpdateOperation) -> DesktopUpdateSnapshot {
        if operation != DesktopUpdateOperation::Snapshot
            && let Some(commands) = &self.commands
        {
            let _ignored = commands.send(operation);
        }
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
#[path = "../../../../tests/desktop/native/update_helper_deadline.rs"]
mod tests;
