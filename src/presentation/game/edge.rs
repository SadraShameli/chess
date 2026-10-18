use bevy_egui::egui::{self, Ui};

use crate::board::enums::BoardEnumColor;
use crate::clock::duration::ClockInstant;
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::game::play::PresentationGamePlayClock;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumClock,
};
use crate::presentation::theme::tokens::PresentationThemeTokens;

pub struct PresentationGameEdge;

impl PresentationGameEdge {
    pub fn show(
        ui: &mut Ui,
        color: BoardEnumColor,
        game: Option<&DesktopGameSnapshot>,
        now: ClockInstant,
        extra: impl FnOnce(&mut Ui),
    ) {
        ui.horizontal(|ui| {
            ui.set_min_height(PresentationThemeTokens::CONTROL_LG);
            PresentationThemeComponents::subhead(
                ui,
                PresentationBoardSemanticGrid::color_name(color),
            );
            extra(ui);
            let Some(view) = game.and_then(|game| game.clock().view(now).ok()) else {
                return;
            };
            let displayed = view.side(color).displayed();
            let state =
                if view.active() != color || !game.is_some_and(|game| game.clock().is_running()) {
                    PresentationThemeEnumClock::Idle
                } else if PresentationGamePlayClock::is_low_time(displayed) {
                    PresentationThemeEnumClock::Low
                } else {
                    PresentationThemeEnumClock::Running
                };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                PresentationThemeComponents::clock(
                    ui,
                    &PresentationGamePlayClock::text(displayed),
                    state,
                    None,
                );
            });
        });
    }
}
