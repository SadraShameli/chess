use bevy_egui::egui::{self, Ui};

use crate::presentation::opening::controller::{
    PresentationOpeningController, PresentationOpeningEnumOrigin, PresentationOpeningIntent,
};
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};

pub struct PresentationOpeningBook;

impl PresentationOpeningBook {
    const BAR_WIDTH: f32 = 72.0;

    pub fn show(
        ui: &mut Ui,
        controller: &PresentationOpeningController,
    ) -> Option<PresentationOpeningIntent> {
        PresentationThemeComponents::eyebrow(ui, "Opening book");
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
            let share_value = book_move.share().map_or(0, |share| share.tenths());
            let clicked = ui
                .horizontal(|ui| {
                    let clicked = ui
                        .add(
                            PresentationThemeEnumButton::Ghost
                                .widget(ui, text, PresentationThemeEnumSize::Small)
                                .min_size(egui::vec2(0.0, PresentationThemeTokens::MIN_TARGET)),
                        )
                        .clicked();
                    let colors = PresentationThemeColors::of(ui.ctx());
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(Self::BAR_WIDTH, 6.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(rect, 2.0, colors.surface_3);
                    let fill = PresentationThemeComponents::share(u64::from(share_value), 1000);
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            rect.min,
                            egui::vec2(rect.width() * fill, rect.height()),
                        ),
                        2.0,
                        colors.accent,
                    );
                    clicked
                })
                .inner;
            if clicked {
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
