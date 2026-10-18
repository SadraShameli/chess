use bevy_egui::egui::Ui;

use crate::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumPrimaryOutcome,
};
use crate::puzzle::summary::{PuzzleSummary, PuzzleSummaryRate};

pub struct PresentationPuzzleSummary;

impl PresentationPuzzleSummary {
    pub fn show(ui: &mut Ui, summary: &PuzzleSummary) {
        ui.heading("Run summary");
        ui.label(format!("Finalized puzzles: {}", summary.finalized()));
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
            ui.label(format!("{label}: {}", summary.primary_count(outcome)));
        }
        ui.label(Self::rate_text(
            "Unassisted solve rate",
            summary.solve_rate(PuzzleEnumAssistance::Unassisted),
        ));
        ui.label(Self::rate_text(
            "Assisted solve rate",
            summary.solve_rate(PuzzleEnumAssistance::Assisted),
        ));
        ui.label(format!("Active time: {:?}", summary.elapsed_active()));
        ui.label(format!(
            "Attempts with hints: {}",
            summary.assisted_attempts()
        ));
        ui.label(format!("Hints: {}", summary.hints()));
        ui.label(format!("Practice retries: {}", summary.practice_retries()));
        ui.label(format!(
            "Practice completions: {}",
            summary.practice_completions()
        ));
        ui.label(format!("Skip events: {}", summary.skip_events()));
        ui.label(format!(
            "Session expiry events: {}",
            summary.timeout_events(PuzzleEnumCountdownMode::Session)
        ));
        ui.label(format!(
            "Puzzle expiry events: {}",
            summary.timeout_events(PuzzleEnumCountdownMode::PerPuzzle)
        ));
        ui.small("Practice completion does not change a recorded failure. Rates use all finalized puzzles.");
    }

    fn rate_text(label: &str, rate: Option<PuzzleSummaryRate>) -> String {
        match rate {
            Some(rate) => format!("{label}: {}/{}", rate.numerator(), rate.denominator()),
            None => format!("{label}: unavailable"),
        }
    }
}
