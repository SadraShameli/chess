use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use semver::Version;
use tokio::sync::{mpsc, watch};

use crate::desktop::native::credentials::DesktopNativeCredentials;
use crate::desktop::native::update::space::DesktopNativeUpdateSpace;
use crate::desktop::update_commands::{
    DesktopUpdateCommands, DesktopUpdateOperation, DesktopUpdateOptions, DesktopUpdateProvider,
    DesktopUpdateSnapshot,
};
use crate::update::cache::UpdateCache;
use crate::update::credentials::UpdateCredentials;
use crate::update::discovery::{UpdateOrigins, UpdateRepository};
use crate::update::http::UpdateReqwest;
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

pub struct DesktopNativeUpdateServiceOptions {
    pub cache_directory: PathBuf,
    pub installed: Version,
    pub runtime: tokio::runtime::Handle,
}

pub struct DesktopNativeUpdateService {
    shared: Arc<Mutex<DesktopUpdateSnapshot>>,
    commands: Option<mpsc::UnboundedSender<DesktopUpdateOperation>>,
}

impl DesktopNativeUpdateService {
    pub const REPOSITORY: Option<&'static str> = option_env!("CHESS_UPDATE_REPOSITORY");

    pub fn disabled(reason: &str) -> Self {
        Self {
            shared: Arc::new(Mutex::new(DesktopUpdateSnapshot::disabled(reason.to_owned()))),
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
        let runtime = options.runtime.clone();
        runtime.spawn(Self::run(
            Self::build(http, verifier, repository, target, options),
            task_shared,
            receiver,
        ));
        if sender.send(DesktopUpdateOperation::Check).is_err() {
            return Self::disabled("The update service stopped immediately");
        }
        Self {
            shared,
            commands: Some(sender),
        }
    }

    async fn build(
        http: UpdateReqwest,
        verifier: UpdateVerifier,
        repository: UpdateRepository,
        target: UpdateTarget,
        options: DesktopNativeUpdateServiceOptions,
    ) -> Option<DesktopNativeUpdateCommands> {
        tokio::task::spawn_blocking(move || {
            DesktopUpdateCommands::new(DesktopUpdateOptions {
                http,
                verifier,
                repository,
                origins: UpdateOrigins::github(),
                cache: UpdateCache::new(options.cache_directory),
                credentials: UpdateCredentials::open(DesktopNativeCredentials::new()),
                network: UpdateNetworkCostUnsupported,
                installed: options.installed,
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
                DesktopUpdateOperation::EnterCredential { credential, persist } => {
                    commands.enter_credential(credential, persist)
                }
                DesktopUpdateOperation::Install => {
                    Self::publish(
                        &shared,
                        commands.snapshot(Some(
                            "Installing needs a verified safety backup, which is not available yet"
                                .to_owned(),
                        )),
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
