use std::num::NonZeroU32;
use std::path::Path;
use std::str::FromStr;

use semver::Version;
use thiserror::Error;

use crate::desktop::identity::DesktopRequestId;
use crate::desktop::profile::DesktopProfileLease;
use crate::desktop::profile::installation::{
    DesktopProfileInstallationEnumOutcome, DesktopProfileInstallationEnumState,
    DesktopProfileInstallationError, DesktopProfileInstallationIntent,
    DesktopProfileInstallationSpec,
};
use crate::package::digest::{PackageDigest, PackageDigestError};
use crate::update::manifest::{UpdateManifest, UpdateManifestAsset, UpdateManifestError};

#[derive(Debug, Error)]
pub enum UpdateInstallError {
    #[error("Installation needs an explicit confirmation")]
    Unconfirmed,
    #[error("An installation prerequisite failed: {0}")]
    Prerequisite(String),
    #[error(transparent)]
    Package(#[from] UpdateManifestError),
    #[error("The package digest is invalid: {0}")]
    Digest(#[from] PackageDigestError),
    #[error(transparent)]
    Intent(#[from] DesktopProfileInstallationError),
    #[error(
        "The helper did not acknowledge readiness for this generation; the prepared intent is retained"
    )]
    HelperNotReady,
    #[error("The helper has not been confirmed stopped; the prepared intent is retained")]
    HelperStillRunning,
}

#[derive(Debug, Clone, Copy)]
pub struct UpdateInstallConfirmation(());

impl UpdateInstallConfirmation {
    pub const fn explicit() -> Self {
        Self(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateHelperEnumReadiness {
    Ready(DesktopRequestId),
    Lost,
}

pub trait UpdateInstallEnvironment {
    fn resolve_unsaved_edits(&mut self) -> Result<(), String>;
    fn pause_and_save_games(&mut self) -> Result<(), String>;
    fn safety_backup(&mut self) -> Result<DesktopRequestId, String>;
    fn start_helper(&mut self, spec: &DesktopProfileInstallationSpec) -> UpdateHelperEnumReadiness;
    fn confirm_helper_stopped(&mut self, spec: &DesktopProfileInstallationSpec) -> bool;
}

pub trait UpdateIntentWriter {
    fn write(
        &self,
        spec: &DesktopProfileInstallationSpec,
        state: DesktopProfileInstallationEnumState,
    ) -> Result<(), DesktopProfileInstallationError>;
}

pub struct UpdateIntentLeaseWriter<'a>(pub &'a DesktopProfileLease);

impl UpdateIntentWriter for UpdateIntentLeaseWriter<'_> {
    fn write(
        &self,
        spec: &DesktopProfileInstallationSpec,
        state: DesktopProfileInstallationEnumState,
    ) -> Result<(), DesktopProfileInstallationError> {
        DesktopProfileInstallationIntent::write(self.0, spec, state)
    }
}

pub struct UpdateInstallRequest<'a> {
    pub asset: &'a UpdateManifestAsset,
    pub package: &'a Path,
    pub version: Version,
    pub data_schema: NonZeroU32,
}

pub struct UpdateInstall;

impl UpdateInstall {
    pub fn run(
        _confirmation: UpdateInstallConfirmation,
        request: &UpdateInstallRequest<'_>,
        environment: &mut impl UpdateInstallEnvironment,
        writer: &impl UpdateIntentWriter,
    ) -> Result<DesktopProfileInstallationSpec, UpdateInstallError> {
        environment
            .resolve_unsaved_edits()
            .map_err(UpdateInstallError::Prerequisite)?;
        environment
            .pause_and_save_games()
            .map_err(UpdateInstallError::Prerequisite)?;
        let backup = environment
            .safety_backup()
            .map_err(UpdateInstallError::Prerequisite)?;
        request.asset.revalidate(request.package)?;
        let spec = DesktopProfileInstallationSpec {
            owner_generation: DesktopRequestId::new(),
            helper_generation: DesktopRequestId::new(),
            package_digest: PackageDigest::from_str(&UpdateManifest::encode_hex(
                &request.asset.sha256(),
            ))?,
            package_version: request.version.clone(),
            safety_backup_id: backup,
            data_schema: request.data_schema,
        };
        writer.write(&spec, DesktopProfileInstallationEnumState::Prepared)?;
        match environment.start_helper(&spec) {
            UpdateHelperEnumReadiness::Ready(generation)
                if generation == spec.helper_generation => {}
            UpdateHelperEnumReadiness::Ready(_) | UpdateHelperEnumReadiness::Lost => {
                return Err(UpdateInstallError::HelperNotReady);
            }
        }
        writer.write(&spec, DesktopProfileInstallationEnumState::Committed)?;
        Ok(spec)
    }

    pub fn cancel_precommit(
        spec: &DesktopProfileInstallationSpec,
        environment: &mut impl UpdateInstallEnvironment,
        writer: &impl UpdateIntentWriter,
    ) -> Result<(), UpdateInstallError> {
        if !environment.confirm_helper_stopped(spec) {
            return Err(UpdateInstallError::HelperStillRunning);
        }
        writer.write(
            spec,
            DesktopProfileInstallationEnumState::Outcome {
                result: DesktopProfileInstallationEnumOutcome::Failed,
            },
        )?;
        Ok(())
    }
}
