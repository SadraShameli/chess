use bevy_egui::egui::{self, Ui};

use crate::desktop::renderer_commands::{
    DesktopRendererCommands, DesktopRendererCommandsEnumState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationBoardRecoveryEnumIntent {
    Retry,
    ResumeGames,
}

pub struct PresentationBoardRecovery;

impl PresentationBoardRecovery {
    pub fn headline(commands: &DesktopRendererCommands) -> Option<String> {
        match commands.state() {
            DesktopRendererCommandsEnumState::Ready => commands.games_paused().then(|| {
                "Graphics recovered. Live games remain paused until you resume them".to_owned()
            }),
            DesktopRendererCommandsEnumState::Interrupted(cause) => Some(format!(
                "{}. Live games are paused. Retry to restart graphics",
                cause.label()
            )),
            DesktopRendererCommandsEnumState::Retrying { cause, .. } => Some(format!(
                "Retrying graphics after: {}",
                cause.label().to_lowercase()
            )),
            DesktopRendererCommandsEnumState::Unrecoverable(cause) => Some(format!(
                "{}. Graphics did not restart within 10 seconds. Live games stay paused",
                cause.label()
            )),
        }
    }

    pub fn show(
        ui: &mut Ui,
        commands: &DesktopRendererCommands,
    ) -> Option<PresentationBoardRecoveryEnumIntent> {
        let headline = Self::headline(commands)?;
        let mut intent = None;
        ui.group(|ui| {
            let response = ui.heading("Graphics paused");
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_role(egui::accesskit::Role::Status);
                node.set_live(egui::accesskit::Live::Assertive);
            });
            ui.label(headline);
            match commands.state() {
                DesktopRendererCommandsEnumState::Interrupted(_)
                | DesktopRendererCommandsEnumState::Unrecoverable(_) => {
                    if crate::presentation::theme::components::PresentationThemeEnumButton::Secondary.show(ui, "Retry").clicked() {
                        intent = Some(PresentationBoardRecoveryEnumIntent::Retry);
                    }
                }
                DesktopRendererCommandsEnumState::Ready => {
                    if crate::presentation::theme::components::PresentationThemeEnumButton::Secondary.show(ui, "Resume games").clicked() {
                        intent = Some(PresentationBoardRecoveryEnumIntent::ResumeGames);
                    }
                }
                DesktopRendererCommandsEnumState::Retrying { .. } => {
                    ui.add_enabled(false, egui::Button::new("Retrying"));
                }
            }
        });
        intent
    }
}
