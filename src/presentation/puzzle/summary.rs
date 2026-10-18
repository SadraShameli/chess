use bevy_egui::egui::Ui;

use crate::presentation::puzzle::style::PresentationPuzzleStyle;
use crate::presentation::theme::components::PresentationThemeComponents;
use crate::presentation::theme::tokens::PresentationThemeColors;
use crate::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumPrimaryOutcome,
};
use crate::puzzle::summary::{PuzzleSummary, PuzzleSummaryRate};

pub struct PresentationPuzzleSummary;

impl PresentationPuzzleSummary {
    pub fn show(ui: &mut Ui, summary: &PuzzleSummary) {
        let colors = PresentationThemeColors::of(ui.ctx());
        PresentationThemeComponents::subhead(ui, "Run summary");
        let solved = summary
            .primary_count(PuzzleEnumPrimaryOutcome::Solved(
                PuzzleEnumAssistance::Unassisted,
            ))
            .saturating_add(summary.primary_count(PuzzleEnumPrimaryOutcome::Solved(
                PuzzleEnumAssistance::Assisted,
            )));
        let missed = summary
            .primary_count(PuzzleEnumPrimaryOutcome::Failed)
            .saturating_add(summary.primary_count(PuzzleEnumPrimaryOutcome::TimedOut));
        PresentationPuzzleStyle::stats(
            ui,
            &[
                ("Solved", solved.to_string()),
                ("Missed", missed.to_string()),
                (
                    "Skipped",
                    summary
                        .primary_count(PuzzleEnumPrimaryOutcome::Skipped)
                        .to_string(),
                ),
            ],
            colors.surface_2,
            PresentationPuzzleStyle::STAT_POINTS,
        );
        PresentationThemeComponents::eyebrow(ui, "Details");
        PresentationPuzzleStyle::note(ui, format!("Finalized puzzles: {}", summary.finalized()));
        for (label, outcome) in [
            (
                "Unassisted solves",
                PuzzleEnumPrimaryOutcome::Solved(PuzzleEnumAssistance::Unassisted),
            ),
            (
                "Assisted solves",
                PuzzleEnumPrimaryOutcome::Solved(PuzzleEnumAssistance::Assisted),
            ),
            ("Failures", PuzzleEnumPrimaryOutcome::Failed),
            ("Timeouts", PuzzleEnumPrimaryOutcome::TimedOut),
            ("Skips", PuzzleEnumPrimaryOutcome::Skipped),
        ] {
            PresentationPuzzleStyle::note(
                ui,
                format!("{label}: {}", summary.primary_count(outcome)),
            );
        }
        PresentationPuzzleStyle::note(
            ui,
            Self::rate_text(
                "Unassisted solve rate",
                summary.solve_rate(PuzzleEnumAssistance::Unassisted),
            ),
        );
        PresentationPuzzleStyle::note(
            ui,
            Self::rate_text(
                "Assisted solve rate",
                summary.solve_rate(PuzzleEnumAssistance::Assisted),
            ),
        );
        PresentationPuzzleStyle::note(ui, format!("Active time: {:?}", summary.elapsed_active()));
        PresentationPuzzleStyle::note(
            ui,
            format!("Attempts with hints: {}", summary.assisted_attempts()),
        );
        PresentationPuzzleStyle::note(ui, format!("Hints: {}", summary.hints()));
        PresentationPuzzleStyle::note(
            ui,
            format!("Practice retries: {}", summary.practice_retries()),
        );
        PresentationPuzzleStyle::note(
            ui,
            format!("Practice completions: {}", summary.practice_completions()),
        );
        PresentationPuzzleStyle::note(ui, format!("Skip events: {}", summary.skip_events()));
        PresentationPuzzleStyle::note(
            ui,
            format!(
                "Session expiry events: {}",
                summary.timeout_events(PuzzleEnumCountdownMode::Session)
            ),
        );
        PresentationPuzzleStyle::note(
            ui,
            format!(
                "Puzzle expiry events: {}",
                summary.timeout_events(PuzzleEnumCountdownMode::PerPuzzle)
            ),
        );
        PresentationThemeComponents::caption(
            ui,
            "Practice completion does not change a recorded failure. Rates use all finalized puzzles.",
        );
    }

    fn rate_text(label: &str, rate: Option<PuzzleSummaryRate>) -> String {
        match rate {
            Some(rate) => format!("{label}: {}/{}", rate.numerator(), rate.denominator()),
            None => format!("{label}: unavailable"),
        }
    }
}
