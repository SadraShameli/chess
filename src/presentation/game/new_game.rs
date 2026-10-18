use bevy_egui::egui::{self, Ui};
use strum::IntoEnumIterator;

use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::board::start::{BoardStartIndex, BoardStartIndexError};
use crate::game::configuration::{
    GameConfiguration, GameConfigurationEnumIndex, GameConfigurationEnumPreset,
    GameConfigurationEnumStart,
};
use crate::game::enums::GameEnumMode;
use crate::presentation::game::configuration::{
    PresentationGameConfiguration, PresentationGameConfigurationEnumBonus,
    PresentationGameConfigurationEnumClockFormat, PresentationGameConfigurationEnumControl,
    PresentationGameConfigurationEnumFinalQuota, PresentationGameConfigurationError,
    PresentationGameConfigurationStage,
};
use crate::presentation::theme::tokens::PresentationThemeTokens;

pub struct PresentationGameNewGame {
    draft: PresentationGameConfiguration,
    submitted_frame: Option<u64>,
}

impl PresentationGameNewGame {
    pub fn new() -> Self {
        Self {
            draft: PresentationGameConfiguration::new(),
            submitted_frame: None,
        }
    }

    pub fn from_position(position: BoardPosition) -> Self {
        let mut view = Self::new();
        view.draft
            .set_start(GameConfigurationEnumStart::Custom(position));
        view
    }

    pub fn draft(&self) -> &PresentationGameConfiguration {
        &self.draft
    }
    pub fn draft_mut(&mut self) -> &mut PresentationGameConfiguration {
        &mut self.draft
    }

    pub fn configuration(&self) -> Result<GameConfiguration, PresentationGameConfigurationError> {
        self.draft.validate()
    }

    pub fn show(&mut self, ui: &mut Ui) -> Option<GameConfiguration> {
        ui.heading("New Game");
        ui.label("Play offline against Reckless or share the board with another player.");
        let mut submitted = None;
        egui::ScrollArea::vertical()
            .id_salt("new_game_configuration")
            .show(ui, |ui| {
                self.show_opponent(ui);
                ui.separator();
                let start_edit = self.show_start(ui);
                ui.separator();
                self.show_clock(ui);
                ui.separator();
                self.show_options(ui);
                let configuration = match start_edit {
                    Ok(()) => self.configuration(),
                    Err(error) => Err(error.into()),
                };
                if let Err(error) = &configuration {
                    ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                }
                if ui
                    .add_enabled(
                        configuration.is_ok(),
                        egui::Button::new("Start Game").min_size(egui::vec2(
                            PresentationThemeTokens::PANEL_WIDTH,
                            PresentationThemeTokens::MIN_TARGET,
                        )),
                    )
                    .clicked()
                    && self.submitted_frame != Some(ui.ctx().cumulative_frame_nr())
                    && let Ok(configuration) = configuration
                {
                    self.submitted_frame = Some(ui.ctx().cumulative_frame_nr());
                    submitted = Some(configuration);
                }
            });
        submitted
    }

    fn show_opponent(&mut self, ui: &mut Ui) {
        ui.strong("Opponent");
        ui.horizontal_wrapped(|ui| {
            ui.radio_value(
                &mut self.draft.mode,
                GameEnumMode::HumanVersusEngine,
                "Play Reckless",
            );
            ui.radio_value(
                &mut self.draft.mode,
                GameEnumMode::LocalTwoPlayer,
                "Local two-player",
            );
        });
        if self.draft.mode == GameEnumMode::HumanVersusEngine {
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(
                    &mut self.draft.human,
                    BoardEnumColor::White,
                    "Play as White",
                );
                ui.radio_value(
                    &mut self.draft.human,
                    BoardEnumColor::Black,
                    "Play as Black",
                );
            });
        }
    }

    fn show_start(&mut self, ui: &mut Ui) -> Result<(), BoardStartIndexError> {
        ui.strong("Starting position");
        ui.horizontal_wrapped(|ui| {
            if ui
                .radio(
                    matches!(self.draft.start, GameConfigurationEnumStart::Standard),
                    "Standard chess",
                )
                .clicked()
            {
                self.draft.start = GameConfigurationEnumStart::Standard;
            }
            if ui
                .radio(
                    matches!(
                        self.draft.start,
                        GameConfigurationEnumStart::Chess960 { .. }
                    ),
                    "Chess960 / FRC",
                )
                .clicked()
                && !matches!(
                    self.draft.start,
                    GameConfigurationEnumStart::Chess960 { .. }
                )
            {
                self.draft.start = GameConfigurationEnumStart::Chess960 {
                    index: GameConfigurationEnumIndex::Random,
                };
            }
            if ui
                .radio(
                    matches!(self.draft.start, GameConfigurationEnumStart::Dfrc { .. }),
                    "DFRC",
                )
                .clicked()
                && !matches!(self.draft.start, GameConfigurationEnumStart::Dfrc { .. })
            {
                self.draft.start = GameConfigurationEnumStart::Dfrc {
                    white: GameConfigurationEnumIndex::Random,
                    black: GameConfigurationEnumIndex::Random,
                };
            }
            if let Some(position) = &self.draft.custom_position
                && ui
                    .radio(
                        matches!(self.draft.start, GameConfigurationEnumStart::Custom(_)),
                        "Custom position",
                    )
                    .clicked()
            {
                self.draft.start = GameConfigurationEnumStart::Custom(position.clone());
            }
        });
        match &mut self.draft.start {
            GameConfigurationEnumStart::Standard => {}
            GameConfigurationEnumStart::Chess960 { index } => {
                Self::show_index(ui, "Both sides", index)?;
            }
            GameConfigurationEnumStart::Dfrc { white, black } => {
                Self::show_index(ui, "White", white)?;
                Self::show_index(ui, "Black", black)?;
                ui.small("Random back ranks are chosen independently for each side after Start.");
            }
            GameConfigurationEnumStart::Custom(position) => {
                ui.label(position.fen());
                ui.small("Your editor position is preserved. Start creates a separate game.");
            }
        }
        Ok(())
    }

    fn show_index(
        ui: &mut Ui,
        label: &str,
        choice: &mut GameConfigurationEnumIndex,
    ) -> Result<(), BoardStartIndexError> {
        ui.push_id(label, |ui| {
            let mut result = Ok(());
            ui.horizontal_wrapped(|ui| {
                ui.label(label);
                if ui
                    .radio(
                        matches!(choice, GameConfigurationEnumIndex::Random),
                        "Random",
                    )
                    .clicked()
                {
                    *choice = GameConfigurationEnumIndex::Random;
                }
                if ui
                    .radio(
                        matches!(choice, GameConfigurationEnumIndex::Numbered(_)),
                        "Numbered (zero-based 0..959)",
                    )
                    .clicked()
                    && matches!(choice, GameConfigurationEnumIndex::Random)
                {
                    *choice = GameConfigurationEnumIndex::Numbered(BoardStartIndex::STANDARD);
                }
                if let GameConfigurationEnumIndex::Numbered(index) = choice {
                    let mut value = index.value();
                    if ui
                        .add(
                            egui::DragValue::new(&mut value)
                                .range(BoardStartIndex::MIN..=BoardStartIndex::MAX),
                        )
                        .changed()
                    {
                        match BoardStartIndex::try_from(value) {
                            Ok(selected) => *index = selected,
                            Err(error) => result = Err(error),
                        }
                    }
                }
            });
            result
        })
        .inner
    }

    fn show_clock(&mut self, ui: &mut Ui) {
        ui.strong("Time control");
        ui.horizontal_wrapped(|ui| {
            for preset in GameConfigurationEnumPreset::iter() {
                ui.radio_value(
                    &mut self.draft.control,
                    PresentationGameConfigurationEnumControl::Preset(preset),
                    preset.label(),
                );
            }
            ui.radio_value(
                &mut self.draft.control,
                PresentationGameConfigurationEnumControl::Custom,
                "Custom clock",
            );
        });
        ui.small("Presets show minutes per side + Fischer seconds after each accepted move.");
        if self.draft.control != PresentationGameConfigurationEnumControl::Custom {
            return;
        }
        let old_format = self.draft.clock_format;
        egui::ComboBox::from_id_salt("new_game_clock_format")
            .selected_text(self.draft.clock_format.label())
            .show_ui(ui, |ui| {
                for format in PresentationGameConfigurationEnumClockFormat::iter() {
                    ui.selectable_value(&mut self.draft.clock_format, format, format.label());
                }
            });
        if self.draft.clock_format != old_format
            && self.draft.clock_format != PresentationGameConfigurationEnumClockFormat::Staged
        {
            self.draft.stages.truncate(1);
        }
        if self.draft.clock_format == PresentationGameConfigurationEnumClockFormat::Staged {
            ui.horizontal_wrapped(|ui| {
                ui.label("Final stage");
                ui.radio_value(
                    &mut self.draft.final_quota,
                    PresentationGameConfigurationEnumFinalQuota::Unlimited,
                    "To the finish",
                );
                ui.radio_value(
                    &mut self.draft.final_quota,
                    PresentationGameConfigurationEnumFinalQuota::Cyclic,
                    "Repeat final stage",
                );
            });
        }
        let count = self.draft.stages.len();
        let mut remove = None;
        for (index, stage) in self.draft.stages.iter_mut().enumerate() {
            let is_final = index.checked_add(1) == Some(count);
            let needs_moves = match self.draft.clock_format {
                PresentationGameConfigurationEnumClockFormat::Staged => {
                    !is_final
                        || self.draft.final_quota
                            == PresentationGameConfigurationEnumFinalQuota::Cyclic
                }
                PresentationGameConfigurationEnumClockFormat::Cyclic => true,
                PresentationGameConfigurationEnumClockFormat::SuddenDeath
                | PresentationGameConfigurationEnumClockFormat::Fischer
                | PresentationGameConfigurationEnumClockFormat::SimpleDelay
                | PresentationGameConfigurationEnumClockFormat::Bronstein => false,
            };
            ui.push_id(index, |ui| {
                ui.group(|ui| {
                    Self::show_stage(ui, stage, self.draft.clock_format, needs_moves);
                    if self.draft.clock_format
                        == PresentationGameConfigurationEnumClockFormat::Staged
                        && count > 1
                        && crate::presentation::theme::components::PresentationThemeEnumButton::Danger.show(ui, "Remove stage").clicked()
                    {
                        remove = Some(index);
                    }
                });
            });
        }
        if let Some(index) = remove {
            self.draft.stages.remove(index);
        }
        if (self.draft.clock_format == PresentationGameConfigurationEnumClockFormat::Staged
            || self.draft.stages.is_empty())
            && crate::presentation::theme::components::PresentationThemeEnumButton::Secondary
                .show(ui, "Add stage")
                .clicked()
        {
            self.draft.add_stage();
        }
    }

    fn show_stage(
        ui: &mut Ui,
        stage: &mut PresentationGameConfigurationStage,
        format: PresentationGameConfigurationEnumClockFormat,
        needs_moves: bool,
    ) {
        Self::show_millis(ui, "Starting time", &mut stage.base_millis);
        if needs_moves {
            ui.horizontal_wrapped(|ui| {
                ui.label("Moves in this stage");
                ui.add(egui::DragValue::new(&mut stage.moves).range(0..=u32::MAX));
            });
        }
        if matches!(
            format,
            PresentationGameConfigurationEnumClockFormat::Staged
                | PresentationGameConfigurationEnumClockFormat::Cyclic
        ) {
            egui::ComboBox::from_id_salt("stage_bonus")
                .selected_text(stage.bonus.label())
                .show_ui(ui, |ui| {
                    for bonus in PresentationGameConfigurationEnumBonus::iter() {
                        ui.selectable_value(&mut stage.bonus, bonus, bonus.label());
                    }
                });
        }
        let bonus = format.bonus(stage.bonus);
        if bonus != PresentationGameConfigurationEnumBonus::None {
            Self::show_millis(ui, bonus.label(), &mut stage.bonus_millis);
        }
    }

    fn show_options(&mut self, ui: &mut Ui) {
        ui.strong("Game options");
        if self.draft.mode == GameEnumMode::HumanVersusEngine {
            Self::show_millis(
                ui,
                "Reckless time per untimed move",
                &mut self.draft.untimed_move_millis,
            );
            ui.small(
                "This budget applies only to untimed games. Timed games use their chess clocks.",
            );
            ui.checkbox(
                &mut self.draft.is_premove_enabled,
                "Enable one queued premove",
            );
        } else {
            ui.checkbox(
                &mut self.draft.requires_local_takeback_consent,
                "Require the other player's confirmation for takebacks",
            );
            ui.checkbox(
                &mut self.draft.is_auto_flip_enabled,
                "Auto-flip after each move",
            );
        }
    }

    fn show_millis(ui: &mut Ui, label: &str, millis: &mut u64) {
        ui.horizontal_wrapped(|ui| {
            ui.label(label);
            ui.add(
                egui::DragValue::new(millis)
                    .speed(100.0)
                    .suffix(" ms")
                    .range(0..=u64::MAX),
            );
        });
    }
}

impl Default for PresentationGameNewGame {
    fn default() -> Self {
        Self::new()
    }
}
