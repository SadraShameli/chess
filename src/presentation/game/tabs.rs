use bevy_egui::egui::{self, Ui};

use crate::game::enums::GameId;
use crate::presentation::game::overview::{
    PresentationGameOverviewEnumStatus, PresentationGameOverviewRow,
};
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
};
use crate::presentation::theme::tokens::PresentationThemeColors;

pub struct PresentationGameTabs;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PresentationGameTabsAction {
    pub selected: Option<GameId>,
    pub close: Option<GameId>,
}

impl PresentationGameTabs {
    pub const CLOSE_LABEL: &'static str = "Close tab";

    pub fn show(
        ui: &mut Ui,
        rows: &[PresentationGameOverviewRow],
        selected: Option<GameId>,
    ) -> PresentationGameTabsAction {
        let mut action = PresentationGameTabsAction::default();
        for row in rows {
            let response =
                PresentationThemeComponents::tab(ui, row.tab_label(), selected == Some(row.id()))
                    .on_hover_text(row.summary());
            if response.clicked() {
                action.selected = Some(row.id());
            }
            if selected == Some(row.id())
                && PresentationThemeEnumButton::Ghost
                    .show_sized(ui, Self::CLOSE_LABEL, PresentationThemeEnumSize::Small)
                    .clicked()
            {
                action.close = Some(row.id());
            }
        }
        action
    }

    pub fn show_overview(ui: &mut Ui, rows: &[PresentationGameOverviewRow]) {
        if rows.len() < 2 {
            return;
        }
        ui.collapsing("All games", |ui| {
            for row in rows {
                let summary = row.summary();
                let text = match row.status() {
                    PresentationGameOverviewEnumStatus::WaitingForCapacity => {
                        egui::RichText::new(summary)
                            .color(PresentationThemeColors::of(ui.ctx()).low)
                    }
                    PresentationGameOverviewEnumStatus::Running
                    | PresentationGameOverviewEnumStatus::Paused
                    | PresentationGameOverviewEnumStatus::Finished => egui::RichText::new(summary),
                };
                ui.label(text);
            }
        });
    }
}
