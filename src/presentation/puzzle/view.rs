use bevy_egui::egui::{self, Ui};

use crate::desktop::identity::DesktopRequestId;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::game::play::PresentationGamePlayClock;
use crate::presentation::puzzle::controller::{
    PresentationPuzzleController, PresentationPuzzleControllerEnumAction,
    PresentationPuzzleControllerError, PresentationPuzzleControllerIntent,
};
use crate::presentation::puzzle::summary::PresentationPuzzleSummary;
use crate::presentation::theme::tokens::PresentationThemeTokens;
use crate::puzzle::checkpoint::PuzzleCheckpointEnumStatus;
use crate::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumEvent, PuzzleEnumHint,
    PuzzleEnumPhase, PuzzleEnumPractice, PuzzleEnumPrimaryOutcome,
};
use crate::puzzle::run::{PuzzleRunEnumConfirmation, PuzzleRunEnumStatus};
use crate::puzzle::timer::PuzzleTimerEnumPauseReason;

#[derive(Default)]
pub struct PresentationPuzzleView {
    submitted_frame: Option<u64>,
    error: Option<PresentationPuzzleControllerError>,
}

impl PresentationPuzzleView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        controller: &mut PresentationPuzzleController,
    ) -> Option<PresentationPuzzleControllerIntent> {
        let frame = ui.ctx().cumulative_frame_nr();
        let mut action = None;
        ui.heading("Puzzles");
        Self::show_status(ui, controller);
        if let Some(failure) = controller.failure() {
            ui.colored_label(ui.visuals().error_fg_color, failure.message());
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
        }
        ui.horizontal_wrapped(|ui| {
            if Self::button(
                ui,
                controller,
                "Pause",
                PresentationPuzzleControllerEnumAction::Pause(PuzzleTimerEnumPauseReason::Manual),
            ) {
                action = Some(PresentationPuzzleControllerEnumAction::Pause(
                    PuzzleTimerEnumPauseReason::Manual,
                ));
            }
            if Self::button(
                ui,
                controller,
                "Resume",
                PresentationPuzzleControllerEnumAction::Resume,
            ) {
                action = Some(PresentationPuzzleControllerEnumAction::Resume);
            }
            if Self::button(
                ui,
                controller,
                "Retry Save",
                PresentationPuzzleControllerEnumAction::RetrySave,
            ) {
                action = Some(PresentationPuzzleControllerEnumAction::RetrySave);
            }
        });
        let confirming = controller.is_confirming_end();
        ui.add_enabled_ui(!confirming, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (label, candidate) in [
                    ("Hint", PresentationPuzzleControllerEnumAction::Hint),
                    (
                        "Retry Practice",
                        PresentationPuzzleControllerEnumAction::RetryPractice,
                    ),
                    ("Skip", PresentationPuzzleControllerEnumAction::Skip),
                    ("Next", PresentationPuzzleControllerEnumAction::Next),
                ] {
                    if Self::button(ui, controller, label, candidate) && action.is_none() {
                        action = Some(candidate);
                    }
                }
            });
            if controller.snapshot().status() == PuzzleRunEnumStatus::Exhausted
                && Self::button(
                    ui,
                    controller,
                    "Reshuffle and Continue",
                    PresentationPuzzleControllerEnumAction::Reshuffle,
                )
                && action.is_none()
            {
                action = Some(PresentationPuzzleControllerEnumAction::Reshuffle);
            }
            if Self::button(
                ui,
                controller,
                "End Run",
                PresentationPuzzleControllerEnumAction::End(PuzzleRunEnumConfirmation::Unconfirmed),
            ) && action.is_none()
                && self.submitted_frame != Some(frame)
            {
                match controller.begin_end_confirmation() {
                    Ok(()) => self.error = None,
                    Err(error) => self.error = Some(error),
                }
                self.submitted_frame = Some(frame);
            }
        });
        if controller.board().is_some() {
            Self::show_attempt(ui, controller);
        }
        if let Some(summary) = controller.summary() {
            ui.separator();
            PresentationPuzzleSummary::show(ui, summary);
        } else if matches!(
            controller.snapshot().status(),
            PuzzleRunEnumStatus::Ended(_) | PuzzleRunEnumStatus::Expired(_)
        ) {
            ui.label("Waiting for the run summary");
        }
        if action.is_none() {
            action = self.show_promotion(ui, controller);
        }
        if controller.is_confirming_end() {
            let mut cancel = false;
            let mut confirm = false;
            let modal = egui::Modal::new(egui::Id::new("puzzle.end.confirmation")).show(ui.ctx(), |ui| {
                crate::presentation::analysis::editor_intent::PresentationAnalysisEditorModal::label(ui, "End this run");
                ui.heading("End this run?");
                ui.label("An unfinished puzzle is recorded as skipped. Earlier failures remain failed.");
                cancel = ui.button("Keep Playing").clicked();
                confirm = Self::button(ui, controller, "Confirm End Run", PresentationPuzzleControllerEnumAction::End(PuzzleRunEnumConfirmation::Confirmed));
            });
            if cancel || modal.should_close() {
                controller.cancel_end_confirmation();
            } else if confirm && action.is_none() {
                action = Some(PresentationPuzzleControllerEnumAction::End(
                    PuzzleRunEnumConfirmation::Confirmed,
                ));
            }
        }
        if self.submitted_frame == Some(frame) {
            return None;
        }
        let action = action?;
        match controller.request(action, DesktopRequestId::new()) {
            Ok(intent) => {
                if intent.is_some() {
                    self.submitted_frame = Some(frame);
                }
                self.error = None;
                intent
            }
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }

    fn button(
        ui: &mut Ui,
        controller: &PresentationPuzzleController,
        label: &str,
        action: PresentationPuzzleControllerEnumAction,
    ) -> bool {
        ui.add_enabled(
            controller.can_request(action),
            egui::Button::new(label).min_size(egui::vec2(0.0, PresentationThemeTokens::MIN_TARGET)),
        )
        .clicked()
    }

    fn show_status(ui: &mut Ui, controller: &PresentationPuzzleController) {
        let snapshot = controller.snapshot();
        let mode = match snapshot.config().mode() {
            PuzzleEnumCountdownMode::Session => "Session",
            PuzzleEnumCountdownMode::PerPuzzle => "Per puzzle",
        };
        ui.label(format!(
            "{mode} remaining: {}",
            PresentationGamePlayClock::text(Some(snapshot.timer().remaining()))
        ));
        ui.label(format!(
            "Archived attempts: {}",
            snapshot.archived_attempts()
        ));
        ui.label(match snapshot.status() {
            PuzzleRunEnumStatus::AwaitingReadiness => {
                "Preparing the board. Countdown waits for readiness"
            }
            PuzzleRunEnumStatus::Active => "Puzzle active",
            PuzzleRunEnumStatus::Paused { .. } => {
                "Paused. Board and solution hidden. Select Resume to continue"
            }
            PuzzleRunEnumStatus::Review => "Solution review. Countdown stopped until Next",
            PuzzleRunEnumStatus::Exhausted => {
                "Collection exhausted. Countdown stopped until Reshuffle and Continue"
            }
            PuzzleRunEnumStatus::Expired(_) => "Countdown expired",
            PuzzleRunEnumStatus::Ended(_) => "Run ended",
        });
        match snapshot.checkpoint_status() {
            PuzzleCheckpointEnumStatus::Open => {
                if snapshot.has_retained_write() || snapshot.has_unsaved_state() {
                    ui.small("Saving progress");
                }
            }
            PuzzleCheckpointEnumStatus::Frozen(_) => {
                ui.label("Saving paused this run. Board and solution hidden");
            }
            PuzzleCheckpointEnumStatus::RetryRequired { .. } => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Save failed. Select Retry Save, then explicitly Resume after saving",
                );
            }
            PuzzleCheckpointEnumStatus::AwaitingResume(_) => {
                ui.label("Saving recovered. Resume becomes available after the latest paused state is durable");
            }
        }
        if controller.is_pending() {
            ui.small("Waiting for the puzzle action");
        }
    }

    fn show_attempt(ui: &mut Ui, controller: &PresentationPuzzleController) {
        let Some(attempt) = controller.snapshot().attempt() else {
            return;
        };
        ui.label(match attempt.primary_outcome() {
            None => "Primary outcome: unfinished",
            Some(PuzzleEnumPrimaryOutcome::Solved(PuzzleEnumAssistance::Unassisted)) => {
                "Primary outcome: solved unassisted"
            }
            Some(PuzzleEnumPrimaryOutcome::Solved(PuzzleEnumAssistance::Assisted)) => {
                "Primary outcome: solved assisted"
            }
            Some(PuzzleEnumPrimaryOutcome::Failed) => "Primary outcome: failed",
            Some(PuzzleEnumPrimaryOutcome::Skipped) => "Primary outcome: skipped",
            Some(PuzzleEnumPrimaryOutcome::TimedOut) => "Primary outcome: timed out",
        });
        if matches!(
            attempt.last_event(),
            Some(PuzzleEnumEvent::LegalUnlisted(_))
        ) {
            ui.label("This legal move is not in the listed solution. No alternative solution was verified");
        }
        match attempt.practice() {
            PuzzleEnumPractice::NotStarted => {}
            PuzzleEnumPractice::InProgress => {
                ui.label("Practice in progress. The existing countdown continues");
            }
            PuzzleEnumPractice::Completed => {
                ui.label("Practice completed. The recorded primary failure remains failed");
            }
        }
        if attempt.phase() == PuzzleEnumPhase::OpponentReply {
            ui.label("Waiting for a listed opponent reply. Countdown continues");
        }
        if attempt.assistance() == PuzzleEnumAssistance::Assisted {
            ui.label("Hint assistance recorded for this attempt");
        }
        if let Some(hint) = attempt.hint() {
            let label = match hint {
                PuzzleEnumHint::Source(square) => format!("Hint source: {square}"),
                PuzzleEnumHint::Destination(square) => format!("Hint destination: {square}"),
                PuzzleEnumHint::NextMove(movement) => format!(
                    "Hint next move: {}",
                    movement.uci(attempt.board().variant())
                ),
            };
            ui.label(label);
        }
    }

    fn show_promotion(
        &mut self,
        ui: &mut Ui,
        controller: &mut PresentationPuzzleController,
    ) -> Option<PresentationPuzzleControllerEnumAction> {
        if controller.promotion_choices().is_empty() {
            return None;
        }
        let mut chosen = None;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("puzzle.promotion")).show(ui.ctx(), |ui| {
            crate::presentation::analysis::editor_intent::PresentationAnalysisEditorModal::label(ui, "Choose promotion");
            ui.heading("Choose promotion");
            for movement in controller.promotion_choices() {
                if let Some(kind) = movement.promotion()
                    && ui
                        .add_enabled(
                            controller.can_input(),
                            egui::Button::new(format!(
                                "Promote to {}",
                                PresentationBoardSemanticGrid::piece_name(kind)
                            ))
                            .min_size(egui::vec2(0.0, PresentationThemeTokens::MIN_TARGET)),
                        )
                        .clicked()
                {
                    chosen = Some(PresentationPuzzleControllerEnumAction::Move(*movement));
                }
            }
            cancel = ui.button("Cancel Promotion").clicked();
        });
        if cancel || modal.should_close() {
            controller.cancel_selection();
            None
        } else {
            chosen
        }
    }
}
