use semver::Version;

use crate::desktop::profile::installation::{
    DesktopProfileInstallationEnumOutcome, DesktopProfileInstallationEnumState,
    DesktopProfileInstallationIntent,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateRecoveryEnumBlock {
    HelperRunning,
    PreparedUncommitted,
    CommittedPending,
    MigrationFailed,
    FailedOutcome,
    UnknownOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateRecoveryEnumDecision {
    Proceed,
    ClearSuccess,
    Blocked(UpdateRecoveryEnumBlock),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRecoveryIntent {
    state: DesktopProfileInstallationEnumState,
    version: Version,
}

impl UpdateRecoveryIntent {
    pub const fn new(state: DesktopProfileInstallationEnumState, version: Version) -> Self {
        Self { state, version }
    }
}

impl From<&DesktopProfileInstallationIntent> for UpdateRecoveryIntent {
    fn from(intent: &DesktopProfileInstallationIntent) -> Self {
        Self {
            state: intent.state(),
            version: intent.package_version().clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateRecoveryObservation {
    pub installed_version: Version,
    pub helper_quiescent: bool,
    pub migration_valid: Option<bool>,
}

pub struct UpdateRecovery;

impl UpdateRecovery {
    pub fn decide(
        intent: Option<&UpdateRecoveryIntent>,
        observation: &UpdateRecoveryObservation,
    ) -> UpdateRecoveryEnumDecision {
        let Some(intent) = intent else {
            return UpdateRecoveryEnumDecision::Proceed;
        };
        let installed = observation.installed_version == intent.version;
        match intent.state {
            DesktopProfileInstallationEnumState::Prepared => {
                if observation.helper_quiescent {
                    UpdateRecoveryEnumDecision::Blocked(
                        UpdateRecoveryEnumBlock::PreparedUncommitted,
                    )
                } else {
                    UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::HelperRunning)
                }
            }
            DesktopProfileInstallationEnumState::Committed => {
                if !installed {
                    return UpdateRecoveryEnumDecision::Blocked(if observation.helper_quiescent {
                        UpdateRecoveryEnumBlock::CommittedPending
                    } else {
                        UpdateRecoveryEnumBlock::HelperRunning
                    });
                }
                Self::migrated(observation)
            }
            DesktopProfileInstallationEnumState::Outcome { result } => match result {
                DesktopProfileInstallationEnumOutcome::Succeeded if installed => {
                    Self::migrated(observation)
                }
                DesktopProfileInstallationEnumOutcome::Succeeded
                | DesktopProfileInstallationEnumOutcome::Unknown => {
                    UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::UnknownOutcome)
                }
                DesktopProfileInstallationEnumOutcome::Failed => {
                    UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::FailedOutcome)
                }
            },
        }
    }

    fn migrated(observation: &UpdateRecoveryObservation) -> UpdateRecoveryEnumDecision {
        match observation.migration_valid {
            Some(true) => UpdateRecoveryEnumDecision::ClearSuccess,
            Some(false) => {
                UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::MigrationFailed)
            }
            None => UpdateRecoveryEnumDecision::Blocked(UpdateRecoveryEnumBlock::CommittedPending),
        }
    }

    pub const fn retry_allowed(observation: &UpdateRecoveryObservation) -> bool {
        observation.helper_quiescent
    }
}
