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
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;

pub struct PresentationGameNewGame {
    draft: PresentationGameConfiguration,
    submitted_frame: Option<u64>,
    cancelled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationGameNewGameEnumVariant {
    Standard,
    Chess960,
    Dfrc,
    Custom,
}

impl PresentationGameNewGameEnumVariant {
    const fn of(start: &GameConfigurationEnumStart) -> Self {
        match start {
            GameConfigurationEnumStart::Standard => Self::Standard,
            GameConfigurationEnumStart::Chess960 { .. } => Self::Chess960,
            GameConfigurationEnumStart::Dfrc { .. } => Self::Dfrc,
            GameConfigurationEnumStart::Custom(_) => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationGameNewGameEnumStrength {
    Quick,
    Balanced,
    Strong,
    Deep,
}

impl PresentationGameNewGameEnumStrength {
    const QUICK_LIMIT: u64 = 1_000;
    const BALANCED_LIMIT: u64 = 5_000;
    const STRONG_LIMIT: u64 = 15_000;

    const fn of(millis: u64) -> Self {
        if millis < Self::QUICK_LIMIT {
            Self::Quick
        } else if millis < Self::BALANCED_LIMIT {
            Self::Balanced
        } else if millis < Self::STRONG_LIMIT {
            Self::Strong
        } else {
            Self::Deep
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Quick => "Level 1 \u{b7} quick replies",
            Self::Balanced => "Level 2 \u{b7} balanced",
            Self::Strong => "Level 3 \u{b7} strong",
            Self::Deep => "Level 4 \u{b7} deep search",
        }
    }
}

struct PresentationGameNewGameLayout;

impl PresentationGameNewGameLayout {
    const WIDTH: f32 = 560.0;
    const TILE_HEIGHT: f32 = 52.0;
    const TILE_COLUMNS: usize = 4;
    const TILE_GAP: f32 = 8.0;
    const STRENGTH_MIN_MILLIS: u64 = 100;
    const STRENGTH_MAX_MILLIS: u64 = 60_000;
}

impl PresentationGameNewGame {
    pub fn new() -> Self {
        Self {
            draft: PresentationGameConfiguration::new(),
            submitted_frame: None,
            cancelled: false,
        }
    }

    pub fn take_cancel(&mut self) -> bool {
        std::mem::take(&mut self.cancelled)
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
        ui.set_max_width(PresentationGameNewGameLayout::WIDTH);
        ui.heading("New Game");
        PresentationThemeComponents::caption(
            ui,
            "Play offline against Reckless or share the board with another player.",
        );
        ui.add_space(PresentationThemeTokens::SPACE_3);
        let mut start_edit = Ok(());
        egui::ScrollArea::vertical()
            .id_salt("new_game_configuration")
            .show(ui, |ui| {
                self.show_opponent(ui);
                ui.add_space(PresentationThemeTokens::SPACE_4);
                self.show_clock(ui);
                ui.add_space(PresentationThemeTokens::SPACE_4);
                start_edit = self.show_side_and_variant(ui);
                ui.add_space(PresentationThemeTokens::SPACE_4);
                self.show_options(ui);
            });
        let configuration = match start_edit {
            Ok(()) => self.configuration(),
            Err(error) => Err(error.into()),
        };
        ui.add_space(PresentationThemeTokens::SPACE_3);
        PresentationThemeComponents::divider(ui);
        ui.add_space(PresentationThemeTokens::SPACE_3);
        let mut submitted = None;
        ui.horizontal(|ui| {
            match &configuration {
                Ok(_) => {
                    PresentationThemeComponents::caption(ui, "Opens in a new tab.");
                }
                Err(error) => {
                    let colors = PresentationThemeColors::of(ui.ctx());
                    ui.label(
                        egui::RichText::new(error.to_string())
                            .size(PresentationThemeTypography::CAPTION_POINTS)
                            .color(colors.err),
                    );
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                PresentationThemeComponents::kbd(ui, "Enter");
                let start = ui.add_enabled(
                    configuration.is_ok(),
                    PresentationThemeEnumButton::Primary.widget(
                        ui,
                        "Start Game",
                        PresentationThemeEnumSize::Medium,
                    ),
                );
                if PresentationThemeEnumButton::Ghost
                    .show(ui, "Cancel")
                    .clicked()
                {
                    self.cancelled = true;
                }
                let pressed_enter = ui.ctx().memory(|memory| memory.focused().is_none())
                    && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if (start.clicked() || pressed_enter)
                    && self.submitted_frame != Some(ui.ctx().cumulative_frame_nr())
                    && let Ok(configuration) = &configuration
                {
                    self.submitted_frame = Some(ui.ctx().cumulative_frame_nr());
                    submitted = Some(configuration.clone());
                }
            });
        });
        submitted
    }

    fn show_opponent(&mut self, ui: &mut Ui) {
        PresentationThemeComponents::field_label(ui, "Opponent");
        PresentationThemeComponents::segmented(
            ui,
            &mut self.draft.mode,
            &[
                (GameEnumMode::HumanVersusEngine, "Play Reckless"),
                (GameEnumMode::LocalTwoPlayer, "Local two-player"),
            ],
            true,
        );
    }

    fn show_side_and_variant(&mut self, ui: &mut Ui) -> Result<(), BoardStartIndexError> {
        ui.horizontal_wrapped(|ui| {
            if self.draft.mode == GameEnumMode::HumanVersusEngine {
                ui.vertical(|ui| {
                    PresentationThemeComponents::field_label(ui, "Play as");
                    PresentationThemeComponents::segmented(
                        ui,
                        &mut self.draft.human,
                        &[
                            (BoardEnumColor::White, "Play as White"),
                            (BoardEnumColor::Black, "Play as Black"),
                        ],
                        false,
                    );
                });
                ui.add_space(PresentationThemeTokens::SPACE_4);
            }
            ui.vertical(|ui| {
                PresentationThemeComponents::field_label(ui, "Variant");
                self.show_variant(ui);
            });
        });
        self.show_start(ui)
    }

    fn show_variant(&mut self, ui: &mut Ui) {
        let mut options = vec![
            (
                PresentationGameNewGameEnumVariant::Standard,
                "Standard chess",
            ),
            (
                PresentationGameNewGameEnumVariant::Chess960,
                "Chess960 / FRC",
            ),
            (PresentationGameNewGameEnumVariant::Dfrc, "DFRC"),
        ];
        if self.draft.custom_position.is_some() {
            options.push((
                PresentationGameNewGameEnumVariant::Custom,
                "Custom position",
            ));
        }
        let mut variant = PresentationGameNewGameEnumVariant::of(&self.draft.start);
        if !PresentationThemeComponents::segmented(ui, &mut variant, &options, false) {
            return;
        }
        match variant {
            PresentationGameNewGameEnumVariant::Standard => {
                self.draft.start = GameConfigurationEnumStart::Standard;
            }
            PresentationGameNewGameEnumVariant::Chess960 => {
                self.draft.start = GameConfigurationEnumStart::Chess960 {
                    index: GameConfigurationEnumIndex::Random,
                };
            }
            PresentationGameNewGameEnumVariant::Dfrc => {
                self.draft.start = GameConfigurationEnumStart::Dfrc {
                    white: GameConfigurationEnumIndex::Random,
                    black: GameConfigurationEnumIndex::Random,
                };
            }
            PresentationGameNewGameEnumVariant::Custom => {
                if let Some(position) = &self.draft.custom_position {
                    self.draft.start = GameConfigurationEnumStart::Custom(position.clone());
                }
            }
        }
    }

    fn preset_category(preset: GameConfigurationEnumPreset) -> &'static str {
        match preset {
            GameConfigurationEnumPreset::Untimed => "No clock",
            GameConfigurationEnumPreset::OnePlusZero | GameConfigurationEnumPreset::TwoPlusOne => {
                "Bullet"
            }
            GameConfigurationEnumPreset::ThreePlusZero
            | GameConfigurationEnumPreset::ThreePlusTwo
            | GameConfigurationEnumPreset::FivePlusZero
            | GameConfigurationEnumPreset::FivePlusThree => "Blitz",
            GameConfigurationEnumPreset::TenPlusZero
            | GameConfigurationEnumPreset::TenPlusFive
            | GameConfigurationEnumPreset::FifteenPlusTen => "Rapid",
            GameConfigurationEnumPreset::ThirtyPlusZero => "Classical",
        }
    }

    fn show_tiles(&mut self, ui: &mut Ui) {
        let mut tiles: Vec<(
            PresentationGameConfigurationEnumControl,
            &'static str,
            &'static str,
        )> = GameConfigurationEnumPreset::iter()
            .map(|preset| {
                (
                    PresentationGameConfigurationEnumControl::Preset(preset),
                    preset.label(),
                    Self::preset_category(preset),
                )
            })
            .collect();
        tiles.push((
            PresentationGameConfigurationEnumControl::Custom,
            "Custom clock",
            "Your rules",
        ));
        let gaps = PresentationGameNewGameLayout::TILE_GAP * 3.0;
        let width = ((ui.available_width() - gaps) / 4.0).max(PresentationThemeTokens::CONTROL_LG);
        for chunk in tiles.chunks(PresentationGameNewGameLayout::TILE_COLUMNS) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = PresentationGameNewGameLayout::TILE_GAP;
                for (control, label, category) in chunk {
                    if Self::tile(ui, width, label, category, self.draft.control == *control)
                        .clicked()
                    {
                        self.draft.control = *control;
                    }
                }
            });
            ui.add_space(PresentationGameNewGameLayout::TILE_GAP);
        }
    }

    fn tile(
        ui: &mut Ui,
        width: f32,
        label: &str,
        category: &str,
        selected: bool,
    ) -> egui::Response {
        let colors = PresentationThemeColors::of(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(width, PresentationGameNewGameLayout::TILE_HEIGHT),
            egui::Sense::click(),
        );
        let (fill, stroke) = if selected {
            (colors.accent_soft, colors.accent)
        } else if response.hovered() {
            (colors.surface_2, colors.line_strong)
        } else {
            (colors.surface_1, colors.line)
        };
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(PresentationThemeTokens::RADIUS_MD),
            fill,
            egui::Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            rect.center() - egui::vec2(0.0, 8.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(PresentationThemeTypography::BODY_POINTS),
            colors.text,
        );
        ui.painter().text(
            rect.center() + egui::vec2(0.0, 10.0),
            egui::Align2::CENTER_CENTER,
            category,
            egui::FontId::proportional(PresentationThemeTypography::MICRO_POINTS),
            colors.text_3,
        );
        let label = label.to_owned();
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, label.clone())
        });
        response
    }

    fn show_start(&mut self, ui: &mut Ui) -> Result<(), BoardStartIndexError> {
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
        PresentationThemeComponents::field_label(ui, "Time control");
        self.show_tiles(ui);
        PresentationThemeComponents::caption(
            ui,
            "Presets show minutes per side + Fischer seconds after each accepted move.",
        );
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
                        && PresentationThemeEnumButton::Danger
                            .show(ui, "Remove stage")
                            .clicked()
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
            && PresentationThemeEnumButton::Secondary
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
        if self.draft.mode == GameEnumMode::HumanVersusEngine {
            PresentationThemeComponents::field_label(ui, "Engine strength");
            ui.horizontal_wrapped(|ui| {
                ui.label("Reckless time per untimed move");
                ui.add(
                    egui::Slider::new(
                        &mut self.draft.untimed_move_millis,
                        PresentationGameNewGameLayout::STRENGTH_MIN_MILLIS
                            ..=PresentationGameNewGameLayout::STRENGTH_MAX_MILLIS,
                    )
                    .logarithmic(true)
                    .clamping(egui::SliderClamping::Never)
                    .suffix(" ms"),
                );
                PresentationThemeComponents::caption(
                    ui,
                    PresentationGameNewGameEnumStrength::of(self.draft.untimed_move_millis).label(),
                );
            });
            PresentationThemeComponents::caption(
                ui,
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
