use bevy_egui::egui::{self, Sense, Ui};

use crate::board::enums::BoardEnumColor;
use crate::desktop::report_commands::{DesktopReportEnumEvaluation, DesktopReportView};
use crate::presentation::cursor::PresentationStudyCursor;
use crate::presentation::report::controller::{
    PresentationReportController, PresentationReportIntent,
};
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
    PresentationThemeEnumTone,
};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;
use crate::report::job::{ReportJobEnumFreshness, ReportJobEnumState};
use crate::report::scoring::{ReportScoringEnumCoverage, ReportScoringEnumLabel};

#[cfg(test)]
#[path = "../../../tests/presentation/report.rs"]
mod tests;

pub struct PresentationReportView;

impl PresentationReportView {
    const GRAPH_HEIGHT: f32 = 120.0;
    const GRAPH_LIMIT: f32 = 1_000.0;

    pub fn show(
        ui: &mut Ui,
        controller: &PresentationReportController,
        cursor: &mut PresentationStudyCursor,
    ) -> Option<PresentationReportIntent> {
        PresentationThemeComponents::eyebrow(ui, "Report");
        ui.heading("Game report");
        for failure in controller.failures() {
            ui.colored_label(ui.visuals().warn_fg_color, failure);
        }
        let Some(view) = controller.view() else {
            ui.label("No report is available for this game");
            return None;
        };
        let mut intent = None;
        Self::show_status(ui, view);
        Self::show_accuracy(ui, view);
        Self::show_tally(ui, view);
        Self::show_graph(ui, view, cursor);
        Self::show_findings(ui, view, cursor);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(controller.can_cancel(), Self::button(ui, "Cancel Report"))
                .clicked()
            {
                intent = Some(PresentationReportIntent::Cancel(view.id));
            }
            if ui
                .add_enabled(
                    controller.can_reanalyze(),
                    Self::button(ui, "Reanalyze Game"),
                )
                .clicked()
            {
                intent = Some(PresentationReportIntent::PreviewReanalysis(view.id));
            }
        });
        if controller.has_replacement_preview() {
            let mut confirm = false;
            let mut discard = false;
            let modal = PresentationThemeComponents::modal(ui.ctx(), egui::Id::new("report.replacement")).show(ui.ctx(), |ui| {
                crate::presentation::analysis::editor_intent::PresentationAnalysisEditorModal::label(ui, "Replace this report");
                ui.heading("Replace this report?");
                ui.label("The new analysis starts from scratch. The existing report is kept until you confirm.");
                discard = PresentationThemeEnumButton::Ghost.show(ui, "Keep Existing Report").clicked();
                confirm = PresentationThemeEnumButton::Primary.show(ui, "Confirm Replacement").clicked();
            });
            if discard || modal.should_close() {
                intent = Some(PresentationReportIntent::DiscardReplacement(view.id));
            } else if confirm {
                intent = Some(PresentationReportIntent::ConfirmReplacement(view.id));
            }
        }
        intent
    }

    fn button(ui: &Ui, label: &str) -> egui::Button<'static> {
        PresentationThemeEnumButton::Secondary
            .widget(ui, label, PresentationThemeEnumSize::Small)
            .min_size(egui::vec2(0.0, PresentationThemeTokens::MIN_TARGET))
    }

    const fn tone(label: Option<ReportScoringEnumLabel>) -> PresentationThemeEnumTone {
        match label {
            Some(ReportScoringEnumLabel::Blunder) => PresentationThemeEnumTone::Error,
            Some(ReportScoringEnumLabel::Mistake) => PresentationThemeEnumTone::Low,
            Some(ReportScoringEnumLabel::Inaccuracy) => PresentationThemeEnumTone::Unsaved,
            Some(ReportScoringEnumLabel::Acceptable) | None => PresentationThemeEnumTone::Neutral,
        }
    }

    fn show_tally(ui: &mut Ui, view: &DesktopReportView) {
        ui.horizontal_wrapped(|ui| {
            for label in [
                ReportScoringEnumLabel::Inaccuracy,
                ReportScoringEnumLabel::Mistake,
                ReportScoringEnumLabel::Blunder,
            ] {
                let count = view
                    .findings
                    .iter()
                    .filter(|finding| !finding.forced && finding.label == Some(label))
                    .count();
                PresentationThemeComponents::pill(
                    ui,
                    format!("{count} {}", Self::label_text(label)),
                    Self::tone(Some(label)),
                );
            }
        });
    }

    const fn state_text(state: ReportJobEnumState) -> &'static str {
        match state {
            ReportJobEnumState::Queued => "Queued",
            ReportJobEnumState::Running => "Analyzing",
            ReportJobEnumState::Pausing => "Pausing",
            ReportJobEnumState::Paused => "Paused",
            ReportJobEnumState::Cancelling => "Cancelling",
            ReportJobEnumState::Cancelled => "Cancelled",
            ReportJobEnumState::Completed => "Complete",
        }
    }

    pub const fn label_text(label: ReportScoringEnumLabel) -> &'static str {
        match label {
            ReportScoringEnumLabel::Acceptable => "Acceptable",
            ReportScoringEnumLabel::Inaccuracy => "Inaccuracy",
            ReportScoringEnumLabel::Mistake => "Mistake",
            ReportScoringEnumLabel::Blunder => "Blunder",
        }
    }

    fn show_status(ui: &mut Ui, view: &DesktopReportView) {
        ui.label(format!("Report status: {}", Self::state_text(view.state)));
        ui.label(format!(
            "Analyzed positions: {} of {}",
            view.completed, view.total
        ));
        if view.freshness == ReportJobEnumFreshness::Outdated {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Outdated: the game line changed after this report",
            );
        }
        if view.coverage == ReportScoringEnumCoverage::Partial {
            ui.label("Partial report: accuracy covers analyzed moves only");
        }
        ui.small(format!("Scoring policy version {}", view.policy.version()));
    }

    fn show_accuracy(ui: &mut Ui, view: &DesktopReportView) {
        let colors = PresentationThemeColors::of(ui.ctx());
        ui.horizontal(|ui| {
            for player in &view.accuracy {
                let side = match player.color {
                    BoardEnumColor::White => "White",
                    BoardEnumColor::Black => "Black",
                };
                PresentationThemeComponents::card(ui).show(ui, |ui| {
                    PresentationThemeComponents::eyebrow(ui, side);
                    PresentationThemeComponents::mono(
                        ui,
                        player
                            .display
                            .as_deref()
                            .map_or_else(|| "?".to_owned(), |value| format!("{value}%")),
                        colors.text,
                        PresentationThemeTypography::TITLE_POINTS,
                    );
                });
            }
        });
        for player in &view.accuracy {
            let side = match player.color {
                BoardEnumColor::White => "White",
                BoardEnumColor::Black => "Black",
            };
            let text = player
                .display
                .as_deref()
                .map_or_else(|| "unavailable".to_owned(), |value| format!("{value}%"));
            ui.label(format!(
                "{side} accuracy: {text} ({} scored, {} forced, {} not scored)",
                player.scored_moves, player.forced_moves, player.unscored_moves
            ));
        }
    }

    fn show_graph(ui: &mut Ui, view: &DesktopReportView, cursor: &mut PresentationStudyCursor) {
        PresentationThemeComponents::subhead(ui, "Evaluation graph");
        let width = ui
            .available_width()
            .max(PresentationThemeTokens::MIN_TARGET);
        let (response, painter) =
            ui.allocate_painter(egui::vec2(width, Self::GRAPH_HEIGHT), Sense::click());
        let rect = response.rect;
        let count = view.findings.len();
        let colors = PresentationThemeColors::of(ui.ctx());
        painter.rect_filled(
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.center().y)),
            0.0,
            colors.eval_white.gamma_multiply(0.18),
        );
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(rect.left(), rect.center().y), rect.max),
            0.0,
            colors.eval_black.gamma_multiply(0.5),
        );
        let stroke = egui::Stroke::new(1.5, colors.accent);
        painter.line_segment(
            [rect.left_center(), rect.right_center()],
            egui::Stroke::new(1.0, colors.line),
        );
        let point = |index: usize, value: DesktopReportEnumEvaluation| {
            let share = if count > 1 {
                f32::from(u16::try_from(index).unwrap_or(u16::MAX))
                    / f32::from(u16::try_from(count - 1).unwrap_or(u16::MAX).max(1))
            } else {
                0.5
            };
            let clamped = match value {
                DesktopReportEnumEvaluation::Centipawns(value) => {
                    f32::from(i16::try_from(value.clamp(-1_000, 1_000)).unwrap_or(0))
                }
                DesktopReportEnumEvaluation::Mate(value) => {
                    if value >= 0 {
                        Self::GRAPH_LIMIT
                    } else {
                        -Self::GRAPH_LIMIT
                    }
                }
            };
            egui::pos2(
                rect.left() + rect.width() * share,
                rect.center().y - clamped / Self::GRAPH_LIMIT * (rect.height() / 2.0),
            )
        };
        let mut previous = None;
        for (index, finding) in view.findings.iter().enumerate() {
            let Some(value) = finding.after_evaluation else {
                previous = None;
                continue;
            };
            let current = point(index, value);
            if let Some(last) = previous {
                painter.line_segment([last, current], stroke);
            }
            let selected = cursor.selected() == Some(finding.after_node);
            painter.circle_filled(
                current,
                if selected { 5.0 } else { 3.0 },
                if selected {
                    colors.accent
                } else {
                    colors.text_2
                },
            );
            previous = Some(current);
        }
        if response.clicked()
            && let Some(position) = response.interact_pointer_pos()
            && count > 0
        {
            let share = ((position.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let last = f32::from(u16::try_from(count - 1).unwrap_or(u16::MAX));
            let index = (share * last).round();
            let index = (0..count)
                .min_by(|left, right| {
                    let left = (f32::from(u16::try_from(*left).unwrap_or(u16::MAX)) - index).abs();
                    let right =
                        (f32::from(u16::try_from(*right).unwrap_or(u16::MAX)) - index).abs();
                    left.total_cmp(&right)
                })
                .unwrap_or(0);
            if let Some(finding) = view.findings.get(index) {
                cursor.select(finding.after_node);
            }
        }
    }

    fn show_findings(ui: &mut Ui, view: &DesktopReportView, cursor: &mut PresentationStudyCursor) {
        PresentationThemeComponents::subhead(ui, "Move findings");
        if view.findings.is_empty() {
            ui.label("This game has no moves to analyze");
        }
        for finding in &view.findings {
            let label = if finding.forced {
                "Forced"
            } else if finding.missed_mate {
                "Missed mate"
            } else {
                finding.label.map_or("Not scored", Self::label_text)
            };
            let text = format!("Move {}: {} {label}", finding.index + 1, finding.played_uci);
            let tone = if finding.forced {
                PresentationThemeEnumTone::Neutral
            } else if finding.missed_mate {
                PresentationThemeEnumTone::Error
            } else {
                Self::tone(finding.label)
            };
            let clicked = ui
                .horizontal(|ui| {
                    let clicked = ui
                        .add(
                            Self::button(ui, &text)
                                .selected(cursor.selected() == Some(finding.after_node)),
                        )
                        .clicked();
                    if tone != PresentationThemeEnumTone::Neutral {
                        PresentationThemeComponents::pill(ui, label, tone);
                    }
                    clicked
                })
                .inner;
            if clicked {
                cursor.select(finding.after_node);
            }
        }
        match cursor.selected() {
            Some(_) => ui.label("A report position is selected. Other study tabs follow it"),
            None => ui.label("Select a finding or graph point to follow it on the board"),
        };
    }
}
