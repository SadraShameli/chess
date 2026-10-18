use bevy_egui::egui::{self, Ui};

use crate::presentation::puzzle::style::PresentationPuzzleStyle;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton,
};
use crate::presentation::theme::tokens::PresentationThemeTokens;
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
            PresentationThemeComponents::modal(ui.ctx(), egui::Id::new("puzzle.history.deletion"))
                .show(ui.ctx(), |ui| {
                    ui.set_width(PresentationThemeTokens::MODAL_WIDTH);
                    ui.spacing_mut().item_spacing.y = PresentationThemeTokens::SPACE_4;
                    PresentationPuzzleStyle::alert_dialog(ui, "Delete puzzle history");
                    PresentationPuzzleStyle::dialog_heading(
                        ui,
                        "Delete puzzle history?",
                        Self::warning(scope),
                    );
                    PresentationThemeComponents::caption(
                        ui,
                        "A run in progress cannot be deleted.",
                    );
                    PresentationPuzzleStyle::dialog_footer(ui, |ui| {
                        confirm = PresentationThemeEnumButton::DangerSolid
                            .show(ui, "Confirm Delete")
                            .clicked();
                        PresentationThemeComponents::kbd(ui, "Esc");
                        cancel = PresentationThemeEnumButton::Ghost
                            .show(ui, "Keep History")
                            .clicked();
                    });
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
