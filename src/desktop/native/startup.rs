use std::path::Path;

use thiserror::Error;

use crate::desktop::identity::DesktopRequestId;
use crate::desktop::profile::activation::DesktopProfileActivationBinding;
use crate::desktop::profile::installation::{
    DesktopProfileInstallationEnumStartup, DesktopProfileInstallationError,
    DesktopProfileInstallationIntent,
};
use crate::desktop::profile::owner_record::{
    DesktopProfileOwnerRecord, DesktopProfileOwnerRecordError,
};
use crate::update::recovery::{
    UpdateRecovery, UpdateRecoveryEnumDecision, UpdateRecoveryIntent, UpdateRecoveryObservation,
};
use crate::desktop::profile::{
    DesktopProfileAdmission, DesktopProfileEnumAdmission, DesktopProfileError, DesktopProfileId,
    DesktopProfileIdentity, DesktopProfileLease, DesktopProfileLockProvider,
    DesktopProfileSecurity,
};

#[derive(Debug, Error)]
pub enum DesktopNativeStartupError {
    #[error("The profile cannot be admitted: {0}")]
    Profile(#[from] DesktopProfileError),
    #[error("The existing owner's activation record is invalid: {0}")]
    OwnerRecord(#[from] DesktopProfileOwnerRecordError),
    #[error("The profile installation barrier cannot be inspected: {0}")]
    Installation(#[from] DesktopProfileInstallationError),
    #[error("The existing owner's record has no persisted profile identity")]
    MissingProfileIdentity,
    #[error("The existing owner's profile {owner} does not match the stored profile {stored}")]
    ProfileBindingMismatch {
        owner: DesktopProfileId,
        stored: DesktopProfileId,
    },
}

#[derive(Debug)]
pub struct DesktopNativeStartupOwner {
    lease: Box<DesktopProfileLease>,
    binding: DesktopProfileActivationBinding,
}

impl DesktopNativeStartupOwner {
    pub fn lease(&self) -> &DesktopProfileLease {
        &self.lease
    }

    pub fn binding(&self) -> DesktopProfileActivationBinding {
        self.binding
    }

    pub fn into_parts(self) -> (Box<DesktopProfileLease>, DesktopProfileActivationBinding) {
        (self.lease, self.binding)
    }
}

#[derive(Debug)]
pub struct DesktopNativeStartupInstallationBlocked {
    binding: DesktopProfileActivationBinding,
    intent: DesktopProfileInstallationIntent,
}

impl DesktopNativeStartupInstallationBlocked {
    pub fn binding(&self) -> DesktopProfileActivationBinding {
        self.binding
    }

    pub fn intent(&self) -> &DesktopProfileInstallationIntent {
        &self.intent
    }
}

#[derive(Debug)]
pub enum DesktopNativeStartupEnumAdmission {
    Owned(DesktopNativeStartupOwner),
    InstallationBlocked(DesktopNativeStartupInstallationBlocked),
    ActivationOnly(DesktopProfileOwnerRecord),
    Busy,
}

pub struct DesktopNativeStartup<L> {
    admission: DesktopProfileAdmission<L>,
    security: DesktopProfileSecurity,
}

impl<L: DesktopProfileLockProvider> DesktopNativeStartup<L> {
    pub fn new(security: DesktopProfileSecurity, lock_provider: L) -> Self {
        Self {
            admission: DesktopProfileAdmission::new(security.clone(), lock_provider),
            security,
        }
    }

    pub fn admit(
        &self,
        data_directory: &Path,
        proposed_profile_id: DesktopProfileId,
        owner_generation: DesktopRequestId,
    ) -> Result<DesktopNativeStartupEnumAdmission, DesktopNativeStartupError> {
        match self
            .admission
            .acquire(data_directory, proposed_profile_id)?
        {
            DesktopProfileEnumAdmission::Owned(lease) => {
                let binding = DesktopProfileActivationBinding::new(
                    lease.identity().profile_id(),
                    owner_generation,
                );
                match DesktopProfileInstallationIntent::inspect(&lease)? {
                    DesktopProfileInstallationEnumStartup::Normal => {
                        Ok(DesktopNativeStartupEnumAdmission::Owned(
                            DesktopNativeStartupOwner { lease, binding },
                        ))
                    }
                    DesktopProfileInstallationEnumStartup::Blocked(intent) => {
                        if Self::clearable(&intent) {
                            return Ok(DesktopNativeStartupEnumAdmission::Owned(
                                DesktopNativeStartupOwner { lease, binding },
                            ));
                        }
                        drop(lease);
                        Ok(DesktopNativeStartupEnumAdmission::InstallationBlocked(
                            DesktopNativeStartupInstallationBlocked { binding, intent },
                        ))
                    }
                }
            }
            DesktopProfileEnumAdmission::Busy => self.inspect_existing_owner(data_directory),
        }
    }

    pub fn clearable(intent: &DesktopProfileInstallationIntent) -> bool {
        let Ok(installed_version) = semver::Version::parse(env!("CARGO_PKG_VERSION")) else {
            return false;
        };
        let observation = UpdateRecoveryObservation {
            installed_version,
            helper_quiescent: !Self::helper_running(),
            migration_valid: Some(true),
        };
        UpdateRecovery::decide(Some(&UpdateRecoveryIntent::from(intent)), &observation)
            == UpdateRecoveryEnumDecision::ClearSuccess
    }

    fn helper_running() -> bool {
        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        system
            .processes()
            .values()
            .any(|process| process.name().to_string_lossy().contains("chess-update-helper"))
    }

    pub fn inspect_existing_owner(
        &self,
        data_directory: &Path,
    ) -> Result<DesktopNativeStartupEnumAdmission, DesktopNativeStartupError> {
        let Some(record) = DesktopProfileOwnerRecord::read(data_directory, &self.security)? else {
            return Ok(DesktopNativeStartupEnumAdmission::Busy);
        };
        let stored = DesktopProfileIdentity::read_existing(data_directory, &self.security)?
            .ok_or(DesktopNativeStartupError::MissingProfileIdentity)?;
        let owner = record.binding().profile();
        if stored != owner {
            return Err(DesktopNativeStartupError::ProfileBindingMismatch { owner, stored });
        }
        Ok(DesktopNativeStartupEnumAdmission::ActivationOnly(record))
    }
}
