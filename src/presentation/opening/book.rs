use bevy_egui::egui::{self, Ui};

use crate::presentation::opening::controller::{
    PresentationOpeningController, PresentationOpeningEnumOrigin, PresentationOpeningIntent,
};
use crate::presentation::theme::tokens::PresentationThemeTokens;

pub struct PresentationOpeningBook;

impl PresentationOpeningBook {
    pub fn show(
        ui: &mut Ui,
        controller: &PresentationOpeningController,
    ) -> Option<PresentationOpeningIntent> {
        ui.heading("Reference book");
        ui.small("Study only. Book moves are never played for an opponent");
        let Some(study) = controller.book() else {
            ui.label("No reference book is available for this position");
            return None;
        };
        let mut intent = None;
        if study.moves().is_empty() {
            ui.label("The book has no legal moves for this position");
        }
        for book_move in study.moves() {
            let share = book_move.share().map_or_else(
                || "share unavailable".to_owned(),
                |share| format!("{}.{}%", share.tenths() / 10, share.tenths() % 10),
            );
            let text = format!(
                "{}: weight {}, {share}",
                book_move.uci(),
                book_move.raw_weight()
            );
            if ui
                .add(
                    egui::Button::new(text)
                        .min_size(egui::vec2(0.0, PresentationThemeTokens::MIN_TARGET)),
                )
                .clicked()
            {
                intent = Some(PresentationOpeningIntent::FollowContinuation {
                    origin: PresentationOpeningEnumOrigin::Book,
                    movement: book_move.movement(),
                });
            }
        }
        if study.rejected_illegal_records() > 0 {
            ui.label(format!(
                "Ignored illegal book records: {}",
                study.rejected_illegal_records()
            ));
        }
        intent
    }
}
