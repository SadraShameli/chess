use semver::Version;
use thiserror::Error;
use tokio::sync::watch;

use crate::update::cache::{UpdateCache, UpdateCacheError};
use crate::update::credentials::{
    UpdateCredentialEnumStatus, UpdateCredentialError, UpdateCredentialStore, UpdateCredentials,
};
use crate::update::discovery::{
    UpdateDiscovered, UpdateDiscovery, UpdateDiscoveryError, UpdateOrigins, UpdateRepository,
};
use crate::update::download::{
    UpdateDiskSpace, UpdateDownloadError, UpdateDownloadRequest, UpdateDownloader,
};
use crate::update::http::UpdateHttp;
use crate::update::manifest::UpdateTarget;
use crate::update::network_cost::{UpdateNetworkCostProvider, UpdateNetworkEnumCost};
use crate::update::state::{
    UpdateCacheReceipt, UpdateEnumFailure, UpdateEnumPhase, UpdateState, UpdateStateError,
};
use crate::update::verifier::UpdateSignatureVerifier;

#[derive(Debug, Error)]
pub enum DesktopUpdateError {
    #[error(transparent)]
    State(#[from] UpdateStateError),
    #[error(transparent)]
    Credentials(#[from] UpdateCredentialError),
    #[error(transparent)]
    Cache(#[from] UpdateCacheError),
    #[error(transparent)]
    Discovery(#[from] UpdateDiscoveryError),
    #[error("Retry after {0} more seconds")]
    RetryAfter(u64),
    #[error("No newer release has been found")]
    NoRelease,
    #[error("Installation cannot continue: {0}")]
    Install(String),
}

pub struct DesktopUpdateOptions<H: UpdateHttp + Clone, V: UpdateSignatureVerifier, S: UpdateCredentialStore, N: UpdateNetworkCostProvider> {
    pub http: H,
    pub verifier: V,
    pub repository: UpdateRepository,
    pub origins: UpdateOrigins,
    pub cache: UpdateCache,
    pub credentials: UpdateCredentials<S>,
    pub network: N,
    pub installed: Version,
    pub target: UpdateTarget,
}

pub struct DesktopUpdateCommands<
    H: UpdateHttp + Clone,
    V: UpdateSignatureVerifier,
    S: UpdateCredentialStore,
    N: UpdateNetworkCostProvider,
> {
    discovery: UpdateDiscovery<H, V>,
    downloader: UpdateDownloader<H>,
    repository: UpdateRepository,
    cache: UpdateCache,
    credentials: UpdateCredentials<S>,
    network: N,
    installed: Version,
    target: UpdateTarget,
    state: UpdateState,
    discovered: Option<UpdateDiscovered>,
    retry_not_before_ms: Option<u64>,
}

impl<
    H: UpdateHttp + Clone,
    V: UpdateSignatureVerifier,
    S: UpdateCredentialStore,
    N: UpdateNetworkCostProvider,
> DesktopUpdateCommands<H, V, S, N>
{
    pub fn new(options: DesktopUpdateOptions<H, V, S, N>) -> Result<Self, DesktopUpdateError> {
        let state = match options.cache.completed()? {
            Some((receipt, _package)) => UpdateState::with_cached(receipt),
            None => UpdateState::default(),
        };
        options.cache.cleanup_partials()?;
        Ok(Self {
            discovery: UpdateDiscovery::new(
                options.http.clone(),
                options.verifier,
                options.repository.clone(),
                options.origins.clone(),
            ),
            downloader: UpdateDownloader::new(options.http, options.origins),
            repository: options.repository,
            cache: options.cache,
            credentials: options.credentials,
            network: options.network,
            installed: options.installed,
            target: options.target,
            state,
            discovered: None,
            retry_not_before_ms: None,
        })
    }

    pub const fn state(&self) -> &UpdateState {
        &self.state
    }

    pub const fn credential_status(&self) -> UpdateCredentialEnumStatus {
        self.credentials.status()
    }

    pub fn enter_credential(
        &mut self,
        credential: crate::update::credentials::UpdateCredential,
        persist: bool,
    ) -> Result<(), DesktopUpdateError> {
        if persist {
            self.credentials.enter_persistent(credential)?;
        } else {
            self.credentials.enter_session_only(credential);
        }
        Ok(())
    }

    pub async fn check(&mut self, now_ms: u64) -> Result<(), DesktopUpdateError> {
        if let Some(until) = self.retry_not_before_ms
            && now_ms < until
        {
            return Err(DesktopUpdateError::RetryAfter((until - now_ms).div_ceil(1000)));
        }
        self.state.begin_check()?;
        let outcome = self
            .discovery
            .discover(&self.installed, self.target, self.credentials.current())
            .await;
        match outcome {
            Ok(Some(found)) => {
                self.state
                    .check_found(found.release.clone(), found.manifest_digest)?;
                self.discovered = Some(found);
            }
            Ok(None) => self.state.check_current()?,
            Err(UpdateDiscoveryError::CredentialMissing) => self.state.credentials_missing()?,
            Err(UpdateDiscoveryError::CredentialRejected) => {
                self.credentials.mark_rejected();
                self.state.credentials_rejected()?;
            }
            Err(UpdateDiscoveryError::RateLimited(seconds)) => {
                self.retry_not_before_ms =
                    Some(now_ms.saturating_add(seconds.unwrap_or(60).saturating_mul(1000)));
                self.state.fail(UpdateEnumFailure::RateLimited)?;
            }
            Err(UpdateDiscoveryError::Deadline(_)) => self.state.fail(UpdateEnumFailure::Timeout)?,
            Err(UpdateDiscoveryError::Manifest(_)) => self.state.fail(UpdateEnumFailure::Signature)?,
            Err(_other) => self.state.fail(UpdateEnumFailure::Network)?,
        }
        Ok(())
    }

    pub fn request_download(&mut self) -> Result<(), DesktopUpdateError> {
        self.state.request_download(self.network.current())?;
        Ok(())
    }

    pub fn approve(&mut self) -> Result<(), DesktopUpdateError> {
        self.state.approve(self.network.current())?;
        Ok(())
    }

    pub fn observe_network(&mut self, cost: UpdateNetworkEnumCost) -> Result<bool, DesktopUpdateError> {
        self.state.network_observed(cost)?;
        Ok(matches!(
            self.state.phase(),
            UpdateEnumPhase::Failed(UpdateEnumFailure::NetworkChanged)
        ))
    }

    pub fn cancel(&mut self) -> Result<(), DesktopUpdateError> {
        self.state.cancel()?;
        Ok(())
    }

    pub async fn download(
        &mut self,
        space: &impl UpdateDiskSpace,
        cancel: watch::Receiver<bool>,
    ) -> Result<(), DesktopUpdateError> {
        let found = self.discovered.clone().ok_or(DesktopUpdateError::NoRelease)?;
        let attempt = self
            .state
            .attempt()
            .filter(|_attempt| self.state.phase() == UpdateEnumPhase::Downloading)
            .ok_or(DesktopUpdateError::NoRelease)?;
        let credential = self
            .credentials
            .current()
            .cloned()
            .ok_or(UpdateDiscoveryError::CredentialMissing)?;
        let partial = self.cache.begin_partial(attempt)?;
        let url = self.repository.asset(found.asset.id())?;
        let outcome = self
            .downloader
            .download(
                UpdateDownloadRequest {
                    asset: &found.asset,
                    url,
                    partial: &partial,
                    credential: &credential,
                },
                space,
                cancel,
                |_received| {},
            )
            .await;
        match outcome {
            Ok(()) => {
                let receipt = UpdateCacheReceipt {
                    release: found.release.clone(),
                    manifest_digest: found.manifest_digest,
                    package_sha256: found.asset.sha256(),
                    length: found.asset.length(),
                };
                self.cache.complete(&partial, &found.asset, &receipt)?;
                self.state.download_completed(receipt)?;
            }
            Err(UpdateDownloadError::Cancelled) => self.state.cancel()?,
            Err(UpdateDownloadError::Space { .. }) => self.state.fail(UpdateEnumFailure::Space)?,
            Err(UpdateDownloadError::Stalled(_) | UpdateDownloadError::Deadline(_)) => {
                self.state.fail(UpdateEnumFailure::Timeout)?;
            }
            Err(UpdateDownloadError::Mismatch) => {
                self.state.fail(UpdateEnumFailure::Verification)?;
            }
            Err(UpdateDownloadError::Io(_)) => self.state.fail(UpdateEnumFailure::Storage)?,
            Err(_other) => self.state.fail(UpdateEnumFailure::Network)?,
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopUpdateOperation {
    Snapshot,
    Check,
    RequestDownload,
    Approve,
    Cancel,
    EnterCredential {
        credential: crate::update::credentials::UpdateCredential,
        persist: bool,
    },
    Install {
        prerequisites: crate::desktop::backup_commands::DesktopBackupCommandsPrerequisites,
    },
}

#[derive(Debug, Clone)]
pub struct DesktopUpdateInstallInputs {
    pub asset: crate::update::manifest::UpdateManifestAsset,
    pub package: std::path::PathBuf,
    pub version: Version,
    pub target: UpdateTarget,
}

#[derive(Debug, Clone)]
pub struct DesktopUpdateSnapshot {
    state: UpdateState,
    credential: UpdateCredentialEnumStatus,
    disabled: Option<String>,
    message: Option<String>,
}

impl DesktopUpdateSnapshot {
    pub fn new(
        state: UpdateState,
        credential: UpdateCredentialEnumStatus,
        message: Option<String>,
    ) -> Self {
        Self {
            state,
            credential,
            disabled: None,
            message,
        }
    }

    pub fn disabled(reason: String) -> Self {
        Self {
            state: UpdateState::default(),
            credential: UpdateCredentialEnumStatus::Missing,
            disabled: Some(reason),
            message: None,
        }
    }

    pub const fn state(&self) -> &UpdateState {
        &self.state
    }

    pub const fn credential(&self) -> UpdateCredentialEnumStatus {
        self.credential
    }

    pub fn disabled_reason(&self) -> Option<&str> {
        self.disabled.as_deref()
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

pub trait DesktopUpdateProvider: Send + Sync {
    fn execute(&self, operation: DesktopUpdateOperation) -> DesktopUpdateSnapshot;
}

impl<
    H: UpdateHttp + Clone,
    V: UpdateSignatureVerifier,
    S: UpdateCredentialStore,
    N: UpdateNetworkCostProvider,
> DesktopUpdateCommands<H, V, S, N>
{
    pub fn begin_install(&mut self) -> Result<DesktopUpdateInstallInputs, DesktopUpdateError> {
        self.state.begin_install_prerequisites()?;
        let cached = match self.cache.completed() {
            Ok(Some(cached)) => cached,
            Ok(None) => {
                self.state.abort_install()?;
                return Err(DesktopUpdateError::NoRelease);
            }
            Err(error) => {
                self.state.abort_install()?;
                return Err(error.into());
            }
        };
        let (receipt, package) = cached;
        Ok(DesktopUpdateInstallInputs {
            asset: crate::update::manifest::UpdateManifestAsset::new(
                receipt.release.asset_id,
                receipt.release.asset_name.clone(),
                receipt.release.target,
                receipt.length,
                receipt.package_sha256,
            ),
            package,
            version: receipt.release.version,
            target: receipt.release.target,
        })
    }

    pub fn abort_install(&mut self) -> Result<(), DesktopUpdateError> {
        self.state.abort_install()?;
        Ok(())
    }

    pub fn begin_handoff(&mut self) -> Result<(), DesktopUpdateError> {
        self.state.begin_handoff()?;
        Ok(())
    }

    pub fn snapshot(&self, message: Option<String>) -> DesktopUpdateSnapshot {
        DesktopUpdateSnapshot::new(self.state.clone(), self.credentials.status(), message)
    }
}
