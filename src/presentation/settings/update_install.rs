use bevy_egui::egui::Ui;

use crate::update::recovery::UpdateRecoveryEnumBlock;

pub struct PresentationSettingsUpdateInstall;

impl PresentationSettingsUpdateInstall {
    pub const CONFIRMATION: &'static str = "Installing will resolve unsaved edits, pause and save every game, take a safety backup, close the app and relaunch it with games paused. Continue?";

    pub fn recovery_guidance(block: UpdateRecoveryEnumBlock) -> &'static str {
        match block {
            UpdateRecoveryEnumBlock::HelperRunning => {
                "An installer helper may still be running. Wait for it to finish, then reopen the app."
            }
            UpdateRecoveryEnumBlock::PreparedUncommitted => {
                "An update was prepared but never started. Cancel it to continue; no data was changed."
            }
            UpdateRecoveryEnumBlock::CommittedPending => {
                "An update was started but its result is unknown. Reopen the app after the installer finishes."
            }
            UpdateRecoveryEnumBlock::MigrationFailed => {
                "The new version could not migrate your data. Your data and the safety backup are untouched; preview a recovery or install a compatible version manually."
            }
            UpdateRecoveryEnumBlock::FailedOutcome => {
                "The installation failed. Your data and the safety backup are untouched. You may retry."
            }
            UpdateRecoveryEnumBlock::UnknownOutcome => {
                "The installation outcome is unknown. Do not retry until the installer is confirmed stopped."
            }
        }
    }

    pub fn show_confirmation(ui: &mut Ui, confirmed: &mut bool) -> bool {
        ui.label(Self::CONFIRMATION);
        ui.checkbox(confirmed, "I understand");
        ui.add_enabled(*confirmed, bevy_egui::egui::Button::new("Install and relaunch"))
            .clicked()
    }

    pub fn show_recovery(ui: &mut Ui, block: UpdateRecoveryEnumBlock) {
        ui.label(Self::recovery_guidance(block));
    }
}
