use bevy_egui::egui::{self, Ui};

use crate::game::pause::{GamePauseEnumCause, GamePauseEnumFault};
use crate::game::recovery::GameRecovery;
use crate::game::save::GameSaveEnumStatus;
use crate::game::save_coordinator::GameSaveCoordinator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationGameRecoveryEnumIntent {
    RetrySave,
}

pub struct PresentationGameRecovery;

impl PresentationGameRecovery {
    pub fn pause_headline(cause: GamePauseEnumCause, recovered: bool) -> String {
        match (GameRecovery::requires_retry_before_resume(cause), recovered) {
            (Some(fault), false) => format!(
                "Paused after a {} failure. Retry first, then resume explicitly.",
                Self::fault_name(fault)
            ),
            (Some(_), true) => {
                "Recovery succeeded. The game stays paused until you resume it.".to_owned()
            }
            (None, _) => "Paused. Resume when you are ready.".to_owned(),
        }
    }

    pub const fn fault_name(fault: GamePauseEnumFault) -> &'static str {
        match fault {
            GamePauseEnumFault::Engine => "engine",
            GamePauseEnumFault::Graphics => "graphics",
            GamePauseEnumFault::Save => "save",
        }
    }

    pub fn save_headline(coordinator: &GameSaveCoordinator) -> Option<String> {
        match (coordinator.failure(), coordinator.status()) {
            (Some(message), _) => Some(format!(
                "The game could not be saved: {message}. Your moves are kept in memory. Retry Save."
            )),
            (None, GameSaveEnumStatus::Required) => {
                Some("Saving the latest moves. Wait before closing or quitting.".to_owned())
            }
            (None, GameSaveEnumStatus::Current) => None,
        }
    }

    pub fn show(
        ui: &mut Ui,
        coordinator: &GameSaveCoordinator,
    ) -> Option<PresentationGameRecoveryEnumIntent> {
        let headline = Self::save_headline(coordinator)?;
        let failed = coordinator.failure().is_some();
        let mut intent = None;
        ui.group(|ui| {
            let response = ui.label(headline);
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_role(egui::accesskit::Role::Status);
                node.set_live(if failed {
                    egui::accesskit::Live::Assertive
                } else {
                    egui::accesskit::Live::Polite
                });
            });
            if failed && ui.button("Retry Save").clicked() {
                intent = Some(PresentationGameRecoveryEnumIntent::RetrySave);
            }
        });
        intent
    }
}
