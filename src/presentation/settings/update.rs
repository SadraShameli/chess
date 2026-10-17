use bevy_egui::egui::Ui;

use crate::update::credentials::UpdateCredentialEnumStatus;
use crate::update::state::{UpdateEnumFailure, UpdateEnumPhase, UpdateState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::Display)]
pub enum PresentationSettingsUpdateEnumAction {
    #[strum(serialize = "Check for updates")]
    Check,
    #[strum(serialize = "Enter update credential")]
    EnterCredential,
    #[strum(serialize = "Use credential for this session only")]
    UseSessionCredential,
    #[strum(serialize = "Approve this network for the download")]
    Approve,
    #[strum(serialize = "Download newer version")]
    Download,
    #[strum(serialize = "Cancel")]
    Cancel,
    #[strum(serialize = "Retry")]
    Retry,
    #[strum(serialize = "Install")]
    Install,
}

impl PresentationSettingsUpdateEnumAction {
    pub fn label(self) -> &'static str {
        self.into()
    }
}

pub struct PresentationSettingsUpdate;

impl PresentationSettingsUpdate {
    pub fn status(state: &UpdateState, credential: UpdateCredentialEnumStatus) -> String {
        let credential_text = match credential {
            UpdateCredentialEnumStatus::Missing => "No update credential is stored.",
            UpdateCredentialEnumStatus::Persistent => "A credential is stored in secure storage.",
            UpdateCredentialEnumStatus::SessionOnly => {
                "A credential is held for this session only."
            }
            UpdateCredentialEnumStatus::Rejected => "The update credential was rejected.",
        };
        let phase = match state.phase() {
            UpdateEnumPhase::Idle => "No update check has run.".to_owned(),
            UpdateEnumPhase::Checking => "Checking for updates.".to_owned(),
            UpdateEnumPhase::UpToDate => "This version is current.".to_owned(),
            UpdateEnumPhase::CredentialsMissing => {
                "Updates need a credential. Offline chess is unaffected.".to_owned()
            }
            UpdateEnumPhase::CredentialsRejected => {
                "The credential was rejected. Enter a new one to continue.".to_owned()
            }
            UpdateEnumPhase::Available => state.release().map_or_else(
                || "A newer version is available.".to_owned(),
                |release| format!("Version {} is available.", release.version),
            ),
            UpdateEnumPhase::AwaitingNetworkApproval => {
                "This network is metered or unknown. Approve it for this download.".to_owned()
            }
            UpdateEnumPhase::Downloading => "Downloading the installer.".to_owned(),
            UpdateEnumPhase::Downloaded => {
                "The installer is downloaded and verified. Install when ready.".to_owned()
            }
            UpdateEnumPhase::InstallPrerequisites => "Preparing to install.".to_owned(),
            UpdateEnumPhase::Handoff => "Handing off to the installer helper.".to_owned(),
            UpdateEnumPhase::Failed(failure) => Self::failure(failure).to_owned(),
            UpdateEnumPhase::Cancelled => "The update was cancelled.".to_owned(),
        };
        format!("{phase} {credential_text}")
    }

    const fn failure(failure: UpdateEnumFailure) -> &'static str {
        match failure {
            UpdateEnumFailure::Network => "The update could not reach the server.",
            UpdateEnumFailure::Timeout => "The update timed out.",
            UpdateEnumFailure::Signature => "The release signature did not verify.",
            UpdateEnumFailure::Storage => "The installer could not be stored.",
            UpdateEnumFailure::Space => "There is not enough free disk space.",
            UpdateEnumFailure::NetworkChanged => {
                "The network changed and the download was discarded."
            }
            UpdateEnumFailure::Verification => "The download did not match the signed manifest.",
            UpdateEnumFailure::Install => "The installation failed.",
            UpdateEnumFailure::RateLimited => "GitHub is rate limiting checks. Retry later.",
        }
    }

    pub fn actions(
        state: &UpdateState,
        credential: UpdateCredentialEnumStatus,
    ) -> Vec<PresentationSettingsUpdateEnumAction> {
        use PresentationSettingsUpdateEnumAction as Action;
        let mut actions = Vec::new();
        match state.phase() {
            UpdateEnumPhase::Idle | UpdateEnumPhase::UpToDate => actions.push(Action::Check),
            UpdateEnumPhase::CredentialsMissing | UpdateEnumPhase::CredentialsRejected => {
                actions.push(Action::EnterCredential);
                actions.push(Action::UseSessionCredential);
            }
            UpdateEnumPhase::Available => actions.push(Action::Download),
            UpdateEnumPhase::AwaitingNetworkApproval => {
                actions.push(Action::Approve);
                actions.push(Action::Cancel);
            }
            UpdateEnumPhase::Checking | UpdateEnumPhase::Downloading => actions.push(Action::Cancel),
            UpdateEnumPhase::Downloaded => actions.push(Action::Install),
            UpdateEnumPhase::Failed(_) | UpdateEnumPhase::Cancelled => {
                actions.push(Action::Retry);
            }
            UpdateEnumPhase::InstallPrerequisites | UpdateEnumPhase::Handoff => {}
        }
        if credential == UpdateCredentialEnumStatus::Rejected
            && !actions.contains(&Action::EnterCredential)
        {
            actions.push(Action::EnterCredential);
        }
        actions
    }

    pub fn show(
        ui: &mut Ui,
        state: &UpdateState,
        credential: UpdateCredentialEnumStatus,
    ) -> Option<PresentationSettingsUpdateEnumAction> {
        ui.label(Self::status(state, credential));
        let mut chosen = None;
        ui.horizontal(|ui| {
            for action in Self::actions(state, credential) {
                if ui.button(action.label()).clicked() {
                    chosen = Some(action);
                }
            }
        });
        chosen
    }
}
