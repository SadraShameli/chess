use std::time::Duration;

use bevy_egui::egui::{self, Ui};
use thiserror::Error;

use crate::desktop::error::DesktopFailure;
use crate::desktop::identity::DesktopRequestId;
use crate::presentation::theme::tokens::PresentationThemeTokens;
use crate::puzzle::enums::PuzzleEnumCountdownMode;
use crate::puzzle::metadata::{PuzzleDefinitionMetadataError, PuzzleDefinitionTheme};
use crate::puzzle::selection::{
    PuzzleSelectionDataset, PuzzleSelectionDifficulty, PuzzleSelectionError, PuzzleSelectionFilter,
};
use crate::puzzle::session::{PuzzleSessionConfig, PuzzleSessionConfigError};

#[derive(Debug, Error)]
pub enum PresentationPuzzleSetupError {
    #[error("Choose an available local puzzle collection")]
    CollectionRequired,
    #[error("This collection contains no available puzzles")]
    EmptyCollection,
    #[error("Enter both minimum and maximum difficulty, or leave both empty")]
    DifficultyBounds,
    #[error("A selected theme is no longer available in this collection")]
    UnavailableTheme,
    #[error("Countdown seconds must contain unsigned decimal digits")]
    DurationSyntax,
    #[error("Countdown seconds exceed the supported range: {0}")]
    DurationRange(#[from] std::num::ParseIntError),
    #[error("The setup result does not match the pending Start Run request")]
    StaleResult,
    #[error(transparent)]
    Metadata(#[from] PuzzleDefinitionMetadataError),
    #[error(transparent)]
    Filter(#[from] PuzzleSelectionError),
    #[error(transparent)]
    Config(#[from] PuzzleSessionConfigError),
}

pub struct PresentationPuzzleSetupCollection {
    dataset: PuzzleSelectionDataset,
    label: String,
    available_puzzles: usize,
    themes: Vec<PuzzleDefinitionTheme>,
}

impl PresentationPuzzleSetupCollection {
    pub fn new(
        dataset: PuzzleSelectionDataset,
        label: String,
        available_puzzles: usize,
        themes: Vec<PuzzleDefinitionTheme>,
    ) -> Result<Self, PresentationPuzzleSetupError> {
        PuzzleSelectionFilter::new(None, themes.clone())?;
        Ok(Self {
            dataset,
            label,
            available_puzzles,
            themes,
        })
    }

    pub const fn dataset(&self) -> &PuzzleSelectionDataset {
        &self.dataset
    }
}

#[derive(Debug, Clone)]
pub struct PresentationPuzzleSetupIntent {
    request_id: DesktopRequestId,
    dataset: PuzzleSelectionDataset,
    filter: PuzzleSelectionFilter,
    config: PuzzleSessionConfig,
}

impl PresentationPuzzleSetupIntent {
    pub const fn request_id(&self) -> DesktopRequestId {
        self.request_id
    }

    pub const fn dataset(&self) -> &PuzzleSelectionDataset {
        &self.dataset
    }

    pub const fn filter(&self) -> &PuzzleSelectionFilter {
        &self.filter
    }

    pub const fn config(&self) -> PuzzleSessionConfig {
        self.config
    }
}

pub struct PresentationPuzzleSetup {
    dataset: Option<PuzzleSelectionDataset>,
    minimum: String,
    maximum: String,
    themes: Vec<PuzzleDefinitionTheme>,
    mode: PuzzleEnumCountdownMode,
    seconds: String,
    pending: Option<PresentationPuzzleSetupIntent>,
    submitted_frame: Option<u64>,
    failure: Option<DesktopFailure>,
}

impl Default for PresentationPuzzleSetup {
    fn default() -> Self {
        let mode = PuzzleEnumCountdownMode::Session;
        Self {
            dataset: None,
            minimum: String::new(),
            maximum: String::new(),
            themes: Vec::new(),
            mode,
            seconds: Self::default_seconds(mode),
            pending: None,
            submitted_frame: None,
            failure: None,
        }
    }
}

impl PresentationPuzzleSetup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn select_dataset(&mut self, dataset: PuzzleSelectionDataset) {
        if self.dataset.as_ref() != Some(&dataset) {
            self.dataset = Some(dataset);
            self.themes.clear();
        }
    }

    pub fn set_difficulty(&mut self, minimum: String, maximum: String) {
        self.minimum = minimum;
        self.maximum = maximum;
    }

    pub fn set_themes(&mut self, themes: Vec<PuzzleDefinitionTheme>) {
        self.themes = themes;
    }

    pub fn set_countdown(&mut self, mode: PuzzleEnumCountdownMode, seconds: String) {
        self.mode = mode;
        self.seconds = seconds;
    }

    pub fn validate(
        &self,
        collections: &[PresentationPuzzleSetupCollection],
    ) -> Result<
        (
            PuzzleSelectionDataset,
            PuzzleSelectionFilter,
            PuzzleSessionConfig,
        ),
        PresentationPuzzleSetupError,
    > {
        let collection = collections
            .iter()
            .find(|collection| self.dataset.as_ref() == Some(&collection.dataset))
            .ok_or(PresentationPuzzleSetupError::CollectionRequired)?;
        if collection.available_puzzles == 0 {
            return Err(PresentationPuzzleSetupError::EmptyCollection);
        }
        if self
            .themes
            .iter()
            .any(|theme| !collection.themes.contains(theme))
        {
            return Err(PresentationPuzzleSetupError::UnavailableTheme);
        }
        let difficulty = match (self.minimum.is_empty(), self.maximum.is_empty()) {
            (true, true) => None,
            (false, false) => Some(PuzzleSelectionDifficulty::new(
                self.minimum.parse()?,
                self.maximum.parse()?,
            )?),
            (true, false) | (false, true) => {
                return Err(PresentationPuzzleSetupError::DifficultyBounds);
            }
        };
        let filter = PuzzleSelectionFilter::new(difficulty, self.themes.clone())?;
        if self.seconds.is_empty() || !self.seconds.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(PresentationPuzzleSetupError::DurationSyntax);
        }
        let config =
            PuzzleSessionConfig::new(self.mode, Duration::from_secs(self.seconds.parse()?))?;
        Ok((collection.dataset.clone(), filter, config))
    }

    pub fn observe_result(
        &mut self,
        request_id: DesktopRequestId,
        failure: Option<DesktopFailure>,
    ) -> Result<(), PresentationPuzzleSetupError> {
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| pending.request_id != request_id)
        {
            return Err(PresentationPuzzleSetupError::StaleResult);
        }
        self.pending = None;
        self.failure = failure;
        Ok(())
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        collections: &[PresentationPuzzleSetupCollection],
        can_start: bool,
    ) -> Option<PresentationPuzzleSetupIntent> {
        ui.heading("Puzzle setup");
        ui.label("Filters and countdown settings apply to the next run");
        if let Some(failure) = &self.failure {
            ui.colored_label(ui.visuals().error_fg_color, failure.message());
        }
        if collections.is_empty() {
            ui.label("No local puzzle collection is available");
        }
        let enabled = can_start && self.pending.is_none();
        ui.add_enabled_ui(enabled, |ui| {
            ui.strong("Collection");
            for collection in collections {
                if ui.radio(self.dataset.as_ref() == Some(&collection.dataset), &collection.label).clicked() {
                    self.select_dataset(collection.dataset.clone());
                }
            }
            let label = ui.label("Minimum difficulty");
            ui.add(egui::TextEdit::singleline(&mut self.minimum).hint_text("Any")).labelled_by(label.id);
            let label = ui.label("Maximum difficulty");
            ui.add(egui::TextEdit::singleline(&mut self.maximum).hint_text("Any")).labelled_by(label.id);
            ui.small("Leave both empty for any difficulty. Unknown ratings are unavailable to a difficulty filter");
            if let Some(collection) = collections.iter().find(|collection| self.dataset.as_ref() == Some(&collection.dataset)) {
                ui.strong("Themes (any selected theme)");
                for theme in &collection.themes {
                    let mut selected = self.themes.contains(theme);
                    if ui.checkbox(&mut selected, theme.as_str()).changed() {
                        if selected {
                            self.themes.push(theme.clone());
                        } else {
                            self.themes.retain(|existing| existing != theme);
                        }
                    }
                }
                ui.small("Select no themes to include all themes, including unavailable theme metadata");
            }
            ui.strong("Countdown");
            for (mode, label) in [(PuzzleEnumCountdownMode::Session, "Whole session"), (PuzzleEnumCountdownMode::PerPuzzle, "Per puzzle")] {
                if ui.radio_value(&mut self.mode, mode, label).changed() {
                    self.seconds = Self::default_seconds(mode);
                }
            }
            let label = ui.label("Countdown seconds");
            ui.add(egui::TextEdit::singleline(&mut self.seconds)).labelled_by(label.id);
        });
        let validated = self.validate(collections);
        if let Err(error) = &validated {
            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
        }
        let clicked = ui
            .add_enabled(
                enabled && validated.is_ok(),
                egui::Button::new("Start Run").min_size(egui::vec2(
                    PresentationThemeTokens::PANEL_WIDTH,
                    PresentationThemeTokens::MIN_TARGET,
                )),
            )
            .clicked();
        let frame = ui.ctx().cumulative_frame_nr();
        if clicked
            && self.submitted_frame != Some(frame)
            && let Ok((dataset, filter, config)) = validated
        {
            let intent = PresentationPuzzleSetupIntent {
                request_id: DesktopRequestId::new(),
                dataset,
                filter,
                config,
            };
            self.pending = Some(intent.clone());
            self.submitted_frame = Some(frame);
            self.failure = None;
            Some(intent)
        } else {
            None
        }
    }

    fn default_seconds(mode: PuzzleEnumCountdownMode) -> String {
        PuzzleSessionConfig::defaults(mode)
            .duration()
            .as_duration()
            .as_secs()
            .to_string()
    }
}
