use bevy_egui::egui::{self, Ui};
use thiserror::Error;

use crate::board::r#move::BoardMoveError;
use crate::desktop::identity::DesktopRequestId;
use crate::engine::analysis::{EngineAnalysisHashOccupancy, EngineAnalysisLine};
use crate::engine::search::session::EngineSearchSessionEnumCompletion;
use crate::presentation::board::figurines::PresentationBoardFigurines;
use crate::presentation::analysis::configuration::{
    PresentationAnalysisConfiguration, PresentationAnalysisConfigurationEnumMode,
};
use crate::presentation::analysis::controller::{
    PresentationAnalysisController, PresentationAnalysisControllerEnumState,
    PresentationAnalysisControllerError, PresentationAnalysisControllerIntent,
};
use crate::presentation::analysis::line::PresentationAnalysisLineScore;
use crate::presentation::theme::tokens::PresentationThemeTokens;
use crate::presentation::workbench::status::PresentationWorkbenchStatusEnumState;

#[derive(Debug, Error)]
pub enum PresentationAnalysisViewError {
    #[error(transparent)]
    Controller(#[from] PresentationAnalysisControllerError),
    #[error("The completed analysis move could not be displayed: {0}")]
    Move(#[from] BoardMoveError),
    #[error("The evaluation bar value could not be represented: {0}")]
    Evaluation(#[from] std::num::TryFromIntError),
}

#[derive(Default)]
pub struct PresentationAnalysisView {
    configuration: PresentationAnalysisConfiguration,
    submitted_frame: Option<u64>,
}

impl PresentationAnalysisView {
    pub fn new() -> Self {
        Self::default()
    }
    pub const fn configuration(&self) -> &PresentationAnalysisConfiguration {
        &self.configuration
    }
    pub fn set_configuration(&mut self, configuration: PresentationAnalysisConfiguration) {
        self.configuration = configuration;
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        controller: &mut PresentationAnalysisController,
    ) -> Result<Option<PresentationAnalysisControllerIntent>, PresentationAnalysisViewError> {
        egui::ScrollArea::vertical()
            .id_salt(("analysis.panel", controller.selection().context()))
            .show(ui, |ui| self.show_contents(ui, controller))
            .inner
    }

    fn show_contents(
        &mut self,
        ui: &mut Ui,
        controller: &mut PresentationAnalysisController,
    ) -> Result<Option<PresentationAnalysisControllerIntent>, PresentationAnalysisViewError> {
        ui.heading("Analysis");
        Self::show_state(ui, controller)?;
        let mut intent = None;
        ui.add_enabled_ui(controller.can_start(), |ui| self.show_configuration(ui));
        let limit = self.configuration.validate();
        if let Err(error) = &limit {
            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
        }
        let start_label = if controller.state() == PresentationAnalysisControllerEnumState::Failed {
            "Retry Analysis"
        } else {
            "Start Analysis"
        };
        let frame = ui.ctx().cumulative_frame_nr();
        let mut start = false;
        let mut stop = false;
        ui.horizontal_wrapped(|ui| {
            let size = egui::vec2(0.0, PresentationThemeTokens::MIN_TARGET);
            start = ui
                .add_enabled(
                    controller.can_start() && limit.is_ok(),
                    egui::Button::new(start_label).min_size(size),
                )
                .clicked();
            stop = ui
                .add_enabled(
                    controller.can_stop(),
                    egui::Button::new("Stop Analysis").min_size(size),
                )
                .clicked();
        });
        if self.submitted_frame != Some(frame) {
            if start && let Ok(limit) = limit {
                intent = controller.start(limit, DesktopRequestId::new())?;
            } else if stop {
                intent = controller.stop(DesktopRequestId::new())?;
            }
            if intent.is_some() {
                self.submitted_frame = Some(frame);
            }
        }
        if controller.selection().workspace().has_evaluation_bar() {
            Self::show_evaluation(ui, controller)?;
        } else {
            ui.label("Switch to Analysis or Review to start analysis");
        }
        if controller.lines().is_empty() {
            ui.label("No candidates reported for this search.");
        }
        for candidate in controller.lines() {
            ui.separator();
            ui.strong(format!("Candidate {}", candidate.rank()));
            if let Some(score) = candidate.source().score() {
                let score = PresentationAnalysisLineScore::new(score);
                ui.label(score.white_label());
                ui.small(score.reported_label());
            } else {
                ui.label("White score: unavailable");
            }
            ui.label(PresentationBoardFigurines::display_line(
                ui.ctx(),
                candidate.variation_text(),
            ));
            egui::CollapsingHeader::new(format!("Candidate {} statistics", candidate.rank()))
                .id_salt((
                    "analysis.candidate",
                    controller.metadata().map(|metadata| metadata.tag()),
                    candidate.rank(),
                ))
                .show(ui, |ui| Self::show_statistics(ui, candidate.source()));
        }
        if let Some(latest) = controller
            .snapshot()
            .and_then(|snapshot| snapshot.latest_update())
        {
            egui::CollapsingHeader::new("Latest telemetry (separate update)")
                .id_salt("analysis.latest")
                .show(ui, |ui| {
                    ui.small("These statistics belong only to the latest engine update.");
                    Self::show_statistics(ui, latest);
                });
        }
        Ok(intent)
    }

    fn show_configuration(&mut self, ui: &mut Ui) {
        let mut mode = self.configuration.mode();
        ui.label("Search mode");
        ui.horizontal_wrapped(|ui| {
            for choice in PresentationAnalysisConfigurationEnumMode::ALL {
                ui.radio_value(&mut mode, choice, choice.label());
            }
        });
        if mode != self.configuration.mode() {
            self.configuration = PresentationAnalysisConfiguration::for_mode(mode);
        }
        match &mut self.configuration {
            PresentationAnalysisConfiguration::Infinite => {
                ui.small("Search until you explicitly Stop.");
            }
            PresentationAnalysisConfiguration::Depth(value) => {
                Self::field(ui, "Depth (plies)", value)
            }
            PresentationAnalysisConfiguration::Nodes(value) => {
                Self::field(ui, "Node budget", value)
            }
            PresentationAnalysisConfiguration::MoveTime(value) => {
                Self::field(ui, "Move time (ms)", value)
            }
            PresentationAnalysisConfiguration::Clock(clock) => {
                Self::field(ui, "White time (ms)", &mut clock.white_time);
                Self::field(ui, "Black time (ms)", &mut clock.black_time);
                Self::field(ui, "White increment (ms)", &mut clock.white_increment);
                Self::field(ui, "Black increment (ms)", &mut clock.black_increment);
                Self::field(ui, "Moves to go (optional)", &mut clock.moves_to_go);
            }
        }
    }

    fn field(ui: &mut Ui, label: &'static str, value: &mut String) {
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(label);
            ui.add(
                egui::TextEdit::singleline(value)
                    .id_salt(label.id)
                    .desired_width(PresentationThemeTokens::PANEL_WIDTH)
                    .char_limit(32),
            )
            .labelled_by(label.id);
        });
    }

    fn show_state(
        ui: &mut Ui,
        controller: &PresentationAnalysisController,
    ) -> Result<(), PresentationAnalysisViewError> {
        let label = match controller.state() {
            PresentationAnalysisControllerEnumState::Idle => "Analysis idle",
            PresentationAnalysisControllerEnumState::Starting => "Starting analysis",
            PresentationAnalysisControllerEnumState::Searching => "Analysis searching",
            PresentationAnalysisControllerEnumState::Stopping => {
                "Stopping analysis: waiting for engine completion"
            }
            PresentationAnalysisControllerEnumState::Completed => "Analysis complete",
            PresentationAnalysisControllerEnumState::Stopped => {
                "Analysis stopped. Start again when ready."
            }
            PresentationAnalysisControllerEnumState::Cancelled => {
                "Analysis cancelled. Start again when ready."
            }
            PresentationAnalysisControllerEnumState::Failed => {
                "Analysis failed. Review the error and Retry Analysis."
            }
        };
        if let Some(terminal) = controller.terminal() {
            match terminal.completion() {
                EngineSearchSessionEnumCompletion::BestMove(movement) => {
                    let position = terminal.snapshot().metadata().position().current();
                    ui.label(format!(
                        "Analysis complete: {}",
                        PresentationBoardFigurines::display(
                            ui.ctx(),
                            &position.san(&movement.uci(position.variant()))?,
                            Some(position.turn())
                        )
                    ));
                }
                EngineSearchSessionEnumCompletion::Failed(error) => {
                    PresentationWorkbenchStatusEnumState::Error(label.to_owned()).show(ui);
                    ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                }
                EngineSearchSessionEnumCompletion::Stopped
                | EngineSearchSessionEnumCompletion::Cancelled => {
                    PresentationWorkbenchStatusEnumState::Empty(label.to_owned()).show(ui);
                }
            }
        } else {
            let status = match controller.state() {
                PresentationAnalysisControllerEnumState::Starting
                | PresentationAnalysisControllerEnumState::Searching
                | PresentationAnalysisControllerEnumState::Stopping => {
                    PresentationWorkbenchStatusEnumState::Loading(label.to_owned())
                }
                PresentationAnalysisControllerEnumState::Completed => {
                    PresentationWorkbenchStatusEnumState::Result(label.to_owned())
                }
                PresentationAnalysisControllerEnumState::Failed => {
                    PresentationWorkbenchStatusEnumState::Error(label.to_owned())
                }
                PresentationAnalysisControllerEnumState::Idle
                | PresentationAnalysisControllerEnumState::Stopped
                | PresentationAnalysisControllerEnumState::Cancelled => {
                    PresentationWorkbenchStatusEnumState::Empty(label.to_owned())
                }
            };
            status.show(ui);
        }
        if let Some(failure) = controller.failure() {
            ui.colored_label(ui.visuals().error_fg_color, failure.message());
        }
        Ok(())
    }

    fn show_statistics(ui: &mut Ui, line: &EngineAnalysisLine) {
        ui.label(format!("Depth: {}", Self::optional(line.depth())));
        ui.label(format!(
            "Selective depth: {}",
            Self::optional(line.selective_depth())
        ));
        ui.label(format!("Nodes: {}", Self::optional(line.nodes())));
        ui.label(format!(
            "Elapsed: {}",
            line.elapsed().map_or_else(
                || "unavailable".to_owned(),
                |elapsed| format!("{} ms", elapsed.as_millis())
            )
        ));
        ui.label(format!("NPS: {}", Self::optional(line.nodes_per_second())));
        ui.label(format!(
            "Hash occupancy: {}",
            line.hash_occupancy().map_or_else(
                || "unavailable".to_owned(),
                |occupancy| format!(
                    "{}/{}",
                    occupancy.per_mille(),
                    EngineAnalysisHashOccupancy::FULL
                )
            )
        ));
        ui.label(format!(
            "Tablebase hits: {}",
            Self::optional(line.tablebase_hits())
        ));
    }

    fn optional(value: Option<impl std::fmt::Display>) -> String {
        value.map_or_else(|| "unavailable".to_owned(), |value| value.to_string())
    }

    fn show_evaluation(
        ui: &mut Ui,
        controller: &PresentationAnalysisController,
    ) -> Result<(), PresentationAnalysisViewError> {
        let score = controller
            .lines()
            .first()
            .filter(|candidate| candidate.rank() == 1)
            .and_then(|candidate| candidate.source().score())
            .map(PresentationAnalysisLineScore::new);
        let (fraction, label, is_uncertain) = match score {
            Some(score) => (
                score.fraction()?,
                format!("White evaluation: {}", score.white_label()),
                score.is_bound(),
            ),
            None => (0.5, "White evaluation: unavailable".to_owned(), true),
        };
        let response = ui.add(
            egui::ProgressBar::new(fraction)
                .desired_height(PresentationThemeTokens::MIN_TARGET)
                .text(label.clone()),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, &label)
        });
        ui.ctx()
            .accesskit_node_builder(response.id, |node| node.clear_numeric_value());
        if is_uncertain {
            let painter = ui.painter().with_clip_rect(response.rect);
            let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
            let height = response.rect.height();
            let mut x = response.rect.left() - height;
            while x < response.rect.right() {
                painter.line_segment(
                    [
                        egui::pos2(x, response.rect.bottom()),
                        egui::pos2(x + height, response.rect.top()),
                    ],
                    stroke,
                );
                x += PresentationThemeTokens::MIN_TARGET / 2.0;
            }
        }
        Ok(())
    }
}
