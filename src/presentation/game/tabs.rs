use bevy_egui::egui::{self, Ui};

use crate::game::enums::GameId;
use crate::presentation::game::overview::{
    PresentationGameOverviewEnumStatus, PresentationGameOverviewRow,
};

pub struct PresentationGameTabs;

impl PresentationGameTabs {
    pub fn show(
        ui: &mut Ui,
        rows: &[PresentationGameOverviewRow],
        selected: Option<GameId>,
    ) -> Option<GameId> {
        let mut chosen = None;
        for row in rows {
            let response = ui
                .selectable_label(selected == Some(row.id()), row.tab_label())
                .on_hover_text(row.summary());
            if response.clicked() {
                chosen = Some(row.id());
            }
        }
        chosen
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
