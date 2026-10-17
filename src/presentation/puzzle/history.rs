use bevy_egui::egui::{self, Ui};

use crate::presentation::analysis::editor_intent::PresentationAnalysisEditorModal;
use crate::puzzle::history::PuzzleHistoryEnumScope;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationPuzzleHistoryEnumChoice {
    Pending,
    Confirm,
    Cancel,
}

pub struct PresentationPuzzleHistory;

impl PresentationPuzzleHistory {
    pub const fn warning(scope: PuzzleHistoryEnumScope) -> &'static str {
        match scope {
            PuzzleHistoryEnumScope::Attempt { .. } => {
                "This attempt is permanently removed from history."
            }
            PuzzleHistoryEnumScope::Session(_) => {
                "This saved run and its attempts are permanently removed. Puzzle datasets are kept."
            }
            PuzzleHistoryEnumScope::All => {
                "All saved puzzle runs are permanently removed. Puzzle datasets are kept."
            }
        }
    }

    pub fn confirm_deletion(
        ui: &Ui,
        scope: PuzzleHistoryEnumScope,
    ) -> PresentationPuzzleHistoryEnumChoice {
        let mut confirm = false;
        let mut cancel = false;
        let modal =
            egui::Modal::new(egui::Id::new("puzzle.history.deletion")).show(ui.ctx(), |ui| {
                PresentationAnalysisEditorModal::label(ui, "Delete puzzle history");
                ui.heading("Delete puzzle history?");
                ui.label(Self::warning(scope));
                ui.label("A run in progress cannot be deleted.");
                cancel = ui.button("Keep History").clicked();
                confirm = ui.button("Confirm Delete").clicked();
            });
        if cancel || modal.should_close() {
            PresentationPuzzleHistoryEnumChoice::Cancel
        } else if confirm {
            PresentationPuzzleHistoryEnumChoice::Confirm
        } else {
            PresentationPuzzleHistoryEnumChoice::Pending
        }
    }
}
