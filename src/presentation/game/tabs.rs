use bevy_egui::egui::{self, Ui};

use crate::game::enums::GameId;
use crate::presentation::game::overview::{
    PresentationGameOverviewEnumStatus, PresentationGameOverviewRow,
};

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
            let response = ui
                .selectable_label(selected == Some(row.id()), row.tab_label())
                .on_hover_text(row.summary());
            if response.clicked() {
                action.selected = Some(row.id());
            }
            if selected == Some(row.id()) && ui.small_button(Self::CLOSE_LABEL).clicked() {
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
                        egui::RichText::new(summary).color(ui.visuals().warn_fg_color)
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
