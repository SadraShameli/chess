use bevy_egui::egui::{self, Ui};

use crate::desktop::renderer_commands::{
    DesktopRendererCommands, DesktopRendererCommandsEnumState,
};
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
    PresentationThemeEnumTone,
};
use crate::presentation::theme::tokens::PresentationThemeColors;

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
        let colors = PresentationThemeColors::of(ui.ctx());
        PresentationThemeComponents::card(ui)
            .fill(colors.surface_2)
            .shadow(ui.visuals().popup_shadow)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    PresentationThemeComponents::pill(
                        ui,
                        "Graphics",
                        PresentationThemeEnumTone::Error,
                    );
                    let response = PresentationThemeComponents::subhead(ui, "Graphics paused");
                    ui.ctx().accesskit_node_builder(response.id, |node| {
                        node.set_role(egui::accesskit::Role::Status);
                        node.set_live(egui::accesskit::Live::Assertive);
                    });
                });
                ui.label(egui::RichText::new(headline).color(colors.text_2));
                match commands.state() {
                    DesktopRendererCommandsEnumState::Interrupted(_)
                    | DesktopRendererCommandsEnumState::Unrecoverable(_) => {
                        if PresentationThemeEnumButton::Primary
                            .show_sized(ui, "Retry", PresentationThemeEnumSize::Small)
                            .clicked()
                        {
                            intent = Some(PresentationBoardRecoveryEnumIntent::Retry);
                        }
                    }
                    DesktopRendererCommandsEnumState::Ready => {
                        if PresentationThemeEnumButton::Primary
                            .show_sized(ui, "Resume games", PresentationThemeEnumSize::Small)
                            .clicked()
                        {
                            intent = Some(PresentationBoardRecoveryEnumIntent::ResumeGames);
                        }
                    }
                    DesktopRendererCommandsEnumState::Retrying { .. } => {
                        let button = PresentationThemeEnumButton::Secondary.widget(
                            ui,
                            "Retrying",
                            PresentationThemeEnumSize::Small,
                        );
                        ui.add_enabled(false, button);
                    }
                }
            });
        intent
    }
}
