use bevy_egui::egui::{self, Ui};

use crate::desktop::opening_commands::{DesktopOpeningExplorerView, DesktopOpeningRating};
use crate::presentation::cursor::PresentationStudyCursor;
use crate::presentation::opening::book::PresentationOpeningBook;
use crate::presentation::opening::controller::{
    PresentationOpeningController, PresentationOpeningEnumOrigin, PresentationOpeningEnumTab,
    PresentationOpeningIntent,
};
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
};
use crate::presentation::theme::tokens::PresentationThemeTokens;

#[cfg(test)]
#[path = "../../../tests/presentation/opening.rs"]
mod tests;

pub struct PresentationOpeningExplorer;

impl PresentationOpeningExplorer {
    const BAR_WIDTH: f32 = 96.0;

    pub fn show_tabs(
        ui: &mut Ui,
        controller: &mut PresentationOpeningController,
        cursor: PresentationStudyCursor,
    ) -> Option<PresentationOpeningIntent> {
        ui.horizontal(|ui| {
            for (label, tab) in [
                ("Explorer", PresentationOpeningEnumTab::Explorer),
                ("Book", PresentationOpeningEnumTab::Book),
            ] {
                if PresentationThemeComponents::tab(ui, label, controller.tab() == tab).clicked() {
                    controller.select_tab(tab);
                }
            }
        });
        ui.label(if cursor.selected().is_some() {
            "Selected position is shared with the board and report"
        } else {
            "No position is selected. The starting position is shown"
        });
        match controller.tab() {
            PresentationOpeningEnumTab::Explorer => Self::show(ui, controller),
            PresentationOpeningEnumTab::Book => PresentationOpeningBook::show(ui, controller),
        }
    }

    pub fn show(
        ui: &mut Ui,
        controller: &PresentationOpeningController,
    ) -> Option<PresentationOpeningIntent> {
        PresentationThemeComponents::eyebrow(ui, "Bundled games");
        ui.heading("Opening explorer");
        ui.small(
            "Explorer moves are study navigation only and never play a move in a running game",
        );
        let Some(view) = controller.explorer() else {
            ui.label("No opening data is available for this position");
            return None;
        };
        let mut intent = None;
        Self::show_totals(ui, view);
        ui.heading("Continuations");
        if view.continuations.is_empty() {
            ui.label("No continuations are recorded for this position");
        }
        for continuation in &view.continuations {
            let text = format!(
                "{}: {} games, {} of {} next moves",
                continuation.uci,
                continuation.games,
                continuation.frequency.numerator,
                continuation.frequency.denominator
            );
            let outcomes = &continuation.outcomes;
            let clicked = ui
                .horizontal(|ui| {
                    let clicked = ui
                        .add(
                            PresentationThemeEnumButton::Ghost
                                .widget(ui, text, PresentationThemeEnumSize::Small)
                                .min_size(egui::vec2(0.0, PresentationThemeTokens::MIN_TARGET)),
                        )
                        .clicked();
                    PresentationThemeComponents::outcome_bar(
                        ui,
                        outcomes.white,
                        outcomes.draws,
                        outcomes.black,
                        Self::BAR_WIDTH,
                    );
                    let share = PresentationThemeComponents::share(
                        continuation.frequency.numerator,
                        continuation.frequency.denominator,
                    );
                    PresentationThemeComponents::caption(ui, format!("{:.0}%", share * 100.0));
                    clicked
                })
                .inner;
            if clicked {
                intent = Some(PresentationOpeningIntent::FollowContinuation {
                    origin: PresentationOpeningEnumOrigin::Explorer,
                    movement: continuation.movement,
                });
            }
        }
        ui.heading("Sample games");
        if view.samples.is_empty() {
            ui.label("No sample games match the current filters");
        }
        for sample in &view.samples {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Game {}: {} vs {}",
                    sample.game,
                    sample.white.as_deref().unwrap_or("unknown"),
                    sample.black.as_deref().unwrap_or("unknown")
                ));
                if sample.partial_date {
                    ui.small("Partial date");
                }
                if sample.library
                    && PresentationThemeEnumButton::Secondary
                        .show_sized(
                            ui,
                            format!("Open Sample {}", sample.game),
                            PresentationThemeEnumSize::Small,
                        )
                        .clicked()
                {
                    intent = Some(PresentationOpeningIntent::OpenSample(sample.game));
                }
            });
        }
        if let Some(offset) = view.next_sample_offset
            && PresentationThemeEnumButton::Ghost
                .show_sized(ui, "Next Samples", PresentationThemeEnumSize::Small)
                .clicked()
        {
            intent = Some(PresentationOpeningIntent::SamplePage(offset));
        }
        intent
    }

    fn rating_text(label: &str, rating: Option<DesktopOpeningRating>) -> String {
        rating.map_or_else(
            || format!("{label}: unavailable"),
            |rating| format!("{label}: {}", rating.sum / rating.samples),
        )
    }

    fn show_totals(ui: &mut Ui, view: &DesktopOpeningExplorerView) {
        ui.label(format!("Games at this position: {}", view.games));
        let known = view.outcomes.white + view.outcomes.draws + view.outcomes.black;
        if known == 0 {
            ui.label("Result counts: unavailable");
        } else {
            ui.label(format!("White wins: {} of {known}", view.outcomes.white));
            ui.label(format!("Draws: {} of {known}", view.outcomes.draws));
            ui.label(format!("Black wins: {} of {known}", view.outcomes.black));
        }
        ui.label(format!("Unknown results: {}", view.outcomes.unknown));
        ui.label(format!("Games ending here: {}", view.ending_games));
        ui.label(Self::rating_text("Average White rating", view.white_rating));
        ui.label(Self::rating_text("Average Black rating", view.black_rating));
        if view.partial_date_matches > 0 {
            ui.label(format!(
                "Includes {} games with partial dates",
                view.partial_date_matches
            ));
        }
        if view.potential_duplicate_pairs > 0 {
            ui.label(format!(
                "Possible duplicate games: {}",
                view.potential_duplicate_pairs
            ));
        }
    }
}
