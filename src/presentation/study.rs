use bevy_egui::egui::{CollapsingHeader, Ui};

use crate::desktop::dataset_commands::{DesktopDatasetRow, DesktopDatasetStarter};
use crate::presentation::cursor::PresentationStudyCursor;
use crate::presentation::opening::controller::{
    PresentationOpeningController, PresentationOpeningIntent,
};
use crate::presentation::opening::explorer::PresentationOpeningExplorer;
use crate::presentation::report::controller::{
    PresentationReportController, PresentationReportIntent,
};
use crate::presentation::report::view::PresentationReportView;
use crate::presentation::settings::datasets::{
    PresentationSettingsDatasets, PresentationSettingsDatasetsIntent,
};

#[derive(Default)]
pub struct PresentationStudy {
    cursor: PresentationStudyCursor,
    report: PresentationReportController,
    opening: PresentationOpeningController,
    datasets: PresentationSettingsDatasets,
    starters: Vec<DesktopDatasetStarter>,
    rows: Vec<DesktopDatasetRow>,
    report_intents: Vec<PresentationReportIntent>,
    opening_intents: Vec<PresentationOpeningIntent>,
    dataset_intents: Vec<PresentationSettingsDatasetsIntent>,
}

impl PresentationStudy {
    pub const fn new() -> Self {
        Self {
            cursor: PresentationStudyCursor::new(),
            report: PresentationReportController::new(),
            opening: PresentationOpeningController::new(),
            datasets: PresentationSettingsDatasets::new(),
            starters: Vec::new(),
            rows: Vec::new(),
            report_intents: Vec::new(),
            opening_intents: Vec::new(),
            dataset_intents: Vec::new(),
        }
    }

    pub const fn cursor(&self) -> PresentationStudyCursor {
        self.cursor
    }

    pub const fn report_mut(&mut self) -> &mut PresentationReportController {
        &mut self.report
    }

    pub const fn opening_mut(&mut self) -> &mut PresentationOpeningController {
        &mut self.opening
    }

    pub fn show_report(&mut self, ui: &mut Ui) -> Option<PresentationReportIntent> {
        PresentationReportView::show(ui, &self.report, &mut self.cursor)
    }

    pub fn show_opening(&mut self, ui: &mut Ui) -> Option<PresentationOpeningIntent> {
        PresentationOpeningExplorer::show_tabs(ui, &mut self.opening, self.cursor)
    }

    pub fn show_datasets(
        &mut self,
        ui: &mut Ui,
        starters: &[DesktopDatasetStarter],
        rows: &[DesktopDatasetRow],
    ) -> Option<PresentationSettingsDatasetsIntent> {
        self.datasets.show(ui, starters, rows)
    }

    pub fn set_datasets(
        &mut self,
        starters: Vec<DesktopDatasetStarter>,
        rows: Vec<DesktopDatasetRow>,
    ) {
        self.starters = starters;
        self.rows = rows;
    }

    pub fn show_panels(&mut self, ui: &mut Ui) {
        CollapsingHeader::new("Game report")
            .default_open(false)
            .show(ui, |ui| {
                if let Some(intent) = self.show_report(ui) {
                    self.report_intents.push(intent);
                }
            });
        CollapsingHeader::new("Opening explorer")
            .default_open(false)
            .show(ui, |ui| {
                if let Some(intent) = self.show_opening(ui) {
                    self.opening_intents.push(intent);
                }
            });
        CollapsingHeader::new("Data")
            .default_open(false)
            .show(ui, |ui| {
                if let Some(intent) = self.datasets.show(ui, &self.starters, &self.rows) {
                    self.dataset_intents.push(intent);
                }
            });
    }

    pub fn take_report_intents(&mut self) -> Vec<PresentationReportIntent> {
        std::mem::take(&mut self.report_intents)
    }

    pub fn take_opening_intents(&mut self) -> Vec<PresentationOpeningIntent> {
        std::mem::take(&mut self.opening_intents)
    }

    pub fn take_dataset_intents(&mut self) -> Vec<PresentationSettingsDatasetsIntent> {
        std::mem::take(&mut self.dataset_intents)
    }
}
