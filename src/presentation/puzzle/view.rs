use bevy_egui::egui::{self, Frame, Margin, Ui};

use crate::desktop::identity::DesktopRequestId;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::game::play::PresentationGamePlayClock;
use crate::presentation::puzzle::controller::{
    PresentationPuzzleController, PresentationPuzzleControllerEnumAction,
    PresentationPuzzleControllerError, PresentationPuzzleControllerIntent,
};
use crate::presentation::puzzle::style::{
    PresentationPuzzleStyle, PresentationPuzzleStyleEnumMark,
};
use crate::presentation::puzzle::summary::PresentationPuzzleSummary;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumClock,
    PresentationThemeEnumTone,
};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::puzzle::checkpoint::PuzzleCheckpointEnumStatus;
use crate::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumEvent, PuzzleEnumHint,
    PuzzleEnumHintStage, PuzzleEnumPhase, PuzzleEnumPractice, PuzzleEnumPrimaryOutcome,
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
        PresentationPuzzleStyle::section(ui, |ui| {
            PresentationThemeComponents::subhead(ui, "Puzzles");
            Self::show_run(ui, controller);
        });
        PresentationPuzzleStyle::section(ui, |ui| {
            Self::show_status(ui, controller);
            if let Some(failure) = controller.failure() {
                PresentationPuzzleStyle::error(ui, failure.message());
            }
            if let Some(error) = &self.error {
                PresentationPuzzleStyle::error(ui, error.to_string());
            }
        });
        let confirming = controller.is_confirming_end();
        PresentationPuzzleStyle::section(ui, |ui| {
            ui.add_enabled_ui(!confirming, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = PresentationThemeTokens::SPACE_2;
                    for (label, key, kind, candidate) in [
                        (
                            "Hint",
                            "H",
                            PresentationThemeEnumButton::Secondary,
                            PresentationPuzzleControllerEnumAction::Hint,
                        ),
                        (
                            "Retry Practice",
                            "R",
                            PresentationThemeEnumButton::Secondary,
                            PresentationPuzzleControllerEnumAction::RetryPractice,
                        ),
                        (
                            "Skip",
                            "S",
                            PresentationThemeEnumButton::Secondary,
                            PresentationPuzzleControllerEnumAction::Skip,
                        ),
                        (
                            "Next",
                            "\u{21b5}",
                            PresentationThemeEnumButton::Primary,
                            PresentationPuzzleControllerEnumAction::Next,
                        ),
                    ] {
                        if PresentationPuzzleStyle::action(
                            ui,
                            kind,
                            label,
                            Some(key),
                            controller.can_request(candidate),
                        ) && action.is_none()
                        {
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
            });
            if !confirming && action.is_none() {
                action = Self::shortcut(ui, controller);
            }
        });
        if controller.board().is_some() {
            PresentationPuzzleStyle::section(ui, |ui| {
                PresentationThemeComponents::eyebrow(ui, "This puzzle");
                Self::show_attempt(ui, controller);
            });
        }
        if let Some(summary) = controller.summary() {
            PresentationPuzzleStyle::section(ui, |ui| {
                PresentationPuzzleSummary::show(ui, summary);
            });
        } else if matches!(
            controller.snapshot().status(),
            PuzzleRunEnumStatus::Ended(_) | PuzzleRunEnumStatus::Expired(_)
        ) {
            PresentationPuzzleStyle::note(ui, "Waiting for the run summary");
        }
        let footer = Frame::new()
            .inner_margin(Margin {
                left: 16,
                right: 16,
                top: 12,
                bottom: 16,
            })
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = PresentationThemeTokens::SPACE_2;
                    let mut chosen = None;
                    for (label, kind, candidate) in [
                        (
                            "Pause",
                            PresentationThemeEnumButton::Secondary,
                            PresentationPuzzleControllerEnumAction::Pause(
                                PuzzleTimerEnumPauseReason::Manual,
                            ),
                        ),
                        (
                            "Resume",
                            PresentationThemeEnumButton::Secondary,
                            PresentationPuzzleControllerEnumAction::Resume,
                        ),
                        (
                            "Retry Save",
                            PresentationThemeEnumButton::Secondary,
                            PresentationPuzzleControllerEnumAction::RetrySave,
                        ),
                    ] {
                        if PresentationPuzzleStyle::action(
                            ui,
                            kind,
                            label,
                            None,
                            controller.can_request(candidate),
                        ) && chosen.is_none()
                        {
                            chosen = Some(candidate);
                        }
                    }
                    let end = ui
                        .add_enabled_ui(!confirming, |ui| {
                            PresentationPuzzleStyle::action(
                                ui,
                                PresentationThemeEnumButton::Danger,
                                "End Run",
                                None,
                                controller.can_request(
                                    PresentationPuzzleControllerEnumAction::End(
                                        PuzzleRunEnumConfirmation::Unconfirmed,
                                    ),
                                ),
                            )
                        })
                        .inner;
                    (chosen, end)
                })
                .inner
            })
            .inner;
        let (footer_action, end_clicked) = footer;
        if let Some(candidate) = footer_action {
            action = Some(candidate);
        }
        if end_clicked && action.is_none() && self.submitted_frame != Some(frame) {
            match controller.begin_end_confirmation() {
                Ok(()) => self.error = None,
                Err(error) => self.error = Some(error),
            }
            self.submitted_frame = Some(frame);
        }
        if action.is_none() {
            action = self.show_promotion(ui, controller);
        }
        if controller.is_confirming_end() {
            let mut cancel = false;
            let mut confirm = false;
            let snapshot = controller.snapshot();
            let tiles = [
                ("Attempts", snapshot.archived_attempts().to_string()),
                ("Hints used", Self::hints_used(controller).to_string()),
                (
                    "Time left",
                    PresentationGamePlayClock::text(Some(snapshot.timer().remaining())),
                ),
            ];
            let can_confirm = controller.can_request(PresentationPuzzleControllerEnumAction::End(
                PuzzleRunEnumConfirmation::Confirmed,
            ));
            let modal = PresentationThemeComponents::modal(
                ui.ctx(),
                egui::Id::new("puzzle.end.confirmation"),
            )
            .show(ui.ctx(), |ui| {
                ui.set_width(PresentationThemeTokens::MODAL_WIDTH);
                ui.spacing_mut().item_spacing.y = PresentationThemeTokens::SPACE_4;
                PresentationPuzzleStyle::alert_dialog(ui, "End this run");
                PresentationPuzzleStyle::dialog_heading(
                    ui,
                    "End this run?",
                    "An unfinished puzzle is recorded as skipped. Earlier failures remain failed.",
                );
                let colors = PresentationThemeColors::of(ui.ctx());
                PresentationPuzzleStyle::stats(
                    ui,
                    &tiles,
                    colors.surface_1,
                    PresentationPuzzleStyle::DIALOG_STAT_POINTS,
                );
                PresentationPuzzleStyle::dialog_footer(ui, |ui| {
                    confirm = PresentationPuzzleStyle::action(
                        ui,
                        PresentationThemeEnumButton::DangerSolid,
                        "Confirm End Run",
                        None,
                        can_confirm,
                    );
                    PresentationThemeComponents::kbd(ui, "Esc");
                    cancel = PresentationThemeEnumButton::Ghost
                        .show(ui, "Keep Playing")
                        .clicked();
                });
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
        PresentationPuzzleStyle::action(
            ui,
            PresentationThemeEnumButton::Secondary,
            label,
            None,
            controller.can_request(action),
        )
    }

    fn shortcut(
        ui: &Ui,
        controller: &PresentationPuzzleController,
    ) -> Option<PresentationPuzzleControllerEnumAction> {
        if ui.memory(|memory| memory.focused().is_some()) || ui.ctx().egui_wants_keyboard_input() {
            return None;
        }
        [
            (egui::Key::H, PresentationPuzzleControllerEnumAction::Hint),
            (
                egui::Key::R,
                PresentationPuzzleControllerEnumAction::RetryPractice,
            ),
            (egui::Key::S, PresentationPuzzleControllerEnumAction::Skip),
            (
                egui::Key::Enter,
                PresentationPuzzleControllerEnumAction::Next,
            ),
        ]
        .into_iter()
        .find(|(key, candidate)| {
            controller.can_request(*candidate)
                && ui.input(|input| input.modifiers.is_none() && input.key_pressed(*key))
        })
        .map(|(_, candidate)| candidate)
    }

    fn hints_used(controller: &PresentationPuzzleController) -> usize {
        controller
            .snapshot()
            .attempt()
            .map_or(0, |attempt| match attempt.hint_stage() {
                PuzzleEnumHintStage::Unrequested => 0,
                PuzzleEnumHintStage::Source => 1,
                PuzzleEnumHintStage::Destination => 2,
                PuzzleEnumHintStage::NextMove => 3,
            })
    }

    fn show_run(ui: &mut Ui, controller: &PresentationPuzzleController) {
        let snapshot = controller.snapshot();
        let remaining = snapshot.timer().remaining();
        let total = snapshot.config().duration();
        let state = match snapshot.status() {
            PuzzleRunEnumStatus::Active
                if PresentationGamePlayClock::is_low_time(Some(remaining)) =>
            {
                PresentationThemeEnumClock::Low
            }
            PuzzleRunEnumStatus::Active => PresentationThemeEnumClock::Running,
            PuzzleRunEnumStatus::AwaitingReadiness
            | PuzzleRunEnumStatus::Paused { .. }
            | PuzzleRunEnumStatus::Review
            | PuzzleRunEnumStatus::Exhausted
            | PuzzleRunEnumStatus::Expired(_)
            | PuzzleRunEnumStatus::Ended(_) => PresentationThemeEnumClock::Idle,
        };
        let (pill, tone) = match snapshot.status() {
            PuzzleRunEnumStatus::AwaitingReadiness => {
                ("Preparing", PresentationThemeEnumTone::Neutral)
            }
            PuzzleRunEnumStatus::Active => ("Running", PresentationThemeEnumTone::Run),
            PuzzleRunEnumStatus::Paused { .. } => ("Paused", PresentationThemeEnumTone::Neutral),
            PuzzleRunEnumStatus::Review => ("Review", PresentationThemeEnumTone::Accent),
            PuzzleRunEnumStatus::Exhausted => ("Exhausted", PresentationThemeEnumTone::Low),
            PuzzleRunEnumStatus::Expired(_) => ("Expired", PresentationThemeEnumTone::Error),
            PuzzleRunEnumStatus::Ended(_) => ("Ended", PresentationThemeEnumTone::Neutral),
        };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = PresentationThemeTokens::SPACE_1;
                PresentationThemeComponents::eyebrow(ui, "Time left");
                PresentationPuzzleStyle::countdown(
                    ui,
                    &PresentationGamePlayClock::text(Some(remaining)),
                    state,
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
                PresentationThemeComponents::pill(ui, pill, tone);
            });
        });
        let total_seconds = total.as_duration().as_secs_f32();
        let share = if total_seconds > 0.0 {
            remaining.as_duration().as_secs_f32() / total_seconds
        } else {
            0.0
        };
        PresentationPuzzleStyle::progress(ui, share, state);
        let mode = match snapshot.config().mode() {
            PuzzleEnumCountdownMode::Session => "Session",
            PuzzleEnumCountdownMode::PerPuzzle => "Per puzzle",
        };
        let colors = PresentationThemeColors::of(ui.ctx());
        PresentationPuzzleStyle::stats(
            ui,
            &[
                ("Attempts", snapshot.archived_attempts().to_string()),
                ("Hints used", Self::hints_used(controller).to_string()),
                ("Countdown", mode.to_owned()),
            ],
            colors.surface_2,
            PresentationPuzzleStyle::STAT_POINTS,
        );
        let archived = snapshot.archived_attempts();
        let current = usize::from(snapshot.attempt().is_some());
        if archived + current > 0 {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for number in 1..=archived {
                    PresentationPuzzleStyle::mark(
                        ui,
                        number,
                        PresentationPuzzleStyleEnumMark::Archived,
                    );
                }
                if current > 0 {
                    PresentationPuzzleStyle::mark(
                        ui,
                        archived + 1,
                        PresentationPuzzleStyleEnumMark::Current,
                    );
                }
            });
        }
    }

    fn show_status(ui: &mut Ui, controller: &PresentationPuzzleController) {
        let snapshot = controller.snapshot();
        let mode = match snapshot.config().mode() {
            PuzzleEnumCountdownMode::Session => "Session",
            PuzzleEnumCountdownMode::PerPuzzle => "Per puzzle",
        };
        PresentationThemeComponents::caption(
            ui,
            format!(
                "{mode} remaining: {}",
                PresentationGamePlayClock::text(Some(snapshot.timer().remaining()))
            ),
        );
        PresentationThemeComponents::caption(
            ui,
            format!("Archived attempts: {}", snapshot.archived_attempts()),
        );
        PresentationPuzzleStyle::note(
            ui,
            match snapshot.status() {
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
            },
        );
        match snapshot.checkpoint_status() {
            PuzzleCheckpointEnumStatus::Open => {
                if snapshot.has_retained_write() || snapshot.has_unsaved_state() {
                    PresentationThemeComponents::caption(ui, "Saving progress");
                }
            }
            PuzzleCheckpointEnumStatus::Frozen(_) => {
                PresentationPuzzleStyle::note(
                    ui,
                    "Saving paused this run. Board and solution hidden",
                );
            }
            PuzzleCheckpointEnumStatus::RetryRequired { .. } => {
                PresentationPuzzleStyle::error(
                    ui,
                    "Save failed. Select Retry Save, then explicitly Resume after saving",
                );
            }
            PuzzleCheckpointEnumStatus::AwaitingResume(_) => {
                PresentationPuzzleStyle::note(
                    ui,
                    "Saving recovered. Resume becomes available after the latest paused state is durable",
                );
            }
        }
        if controller.is_pending() {
            PresentationThemeComponents::caption(ui, "Waiting for the puzzle action");
        }
    }

    fn show_attempt(ui: &mut Ui, controller: &PresentationPuzzleController) {
        let Some(attempt) = controller.snapshot().attempt() else {
            return;
        };
        PresentationPuzzleStyle::key_value(
            ui,
            "Hints used",
            &format!("{} of 3", Self::hints_used(controller)),
            false,
        );
        PresentationPuzzleStyle::note(
            ui,
            match attempt.primary_outcome() {
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
            },
        );
        if matches!(
            attempt.last_event(),
            Some(PuzzleEnumEvent::LegalUnlisted(_))
        ) {
            PresentationPuzzleStyle::note(
                ui,
                "This legal move is not in the listed solution. No alternative solution was verified",
            );
        }
        match attempt.practice() {
            PuzzleEnumPractice::NotStarted => {}
            PuzzleEnumPractice::InProgress => {
                PresentationPuzzleStyle::note(
                    ui,
                    "Practice in progress. The existing countdown continues",
                );
            }
            PuzzleEnumPractice::Completed => {
                PresentationPuzzleStyle::note(
                    ui,
                    "Practice completed. The recorded primary failure remains failed",
                );
            }
        }
        if attempt.phase() == PuzzleEnumPhase::OpponentReply {
            PresentationPuzzleStyle::note(
                ui,
                "Waiting for a listed opponent reply. Countdown continues",
            );
        }
        if attempt.assistance() == PuzzleEnumAssistance::Assisted {
            PresentationPuzzleStyle::note(ui, "Hint assistance recorded for this attempt");
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
            PresentationPuzzleStyle::note(ui, label);
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
        let modal = PresentationThemeComponents::modal(
            ui.ctx(),
            egui::Id::new("puzzle.promotion"),
        )
        .show(ui.ctx(), |ui| {
            crate::presentation::analysis::editor_intent::PresentationAnalysisEditorModal::label(
                ui,
                "Choose promotion",
            );
            PresentationThemeComponents::subhead(ui, "Choose promotion");
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
            cancel = PresentationThemeEnumButton::Ghost
                .show(ui, "Cancel Promotion")
                .clicked();
        });
        if cancel || modal.should_close() {
            controller.cancel_selection();
            None
        } else {
            chosen
        }
    }
}
