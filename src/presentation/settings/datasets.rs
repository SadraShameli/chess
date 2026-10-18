use bevy_egui::egui::{self, Ui};

use crate::desktop::dataset_commands::{DesktopDatasetRow, DesktopDatasetStarter};
use crate::presentation::analysis::editor_intent::PresentationAnalysisEditorModal;
use crate::presentation::theme::tokens::PresentationThemeTokens;
use crate::storage::dataset_registry::{
    StorageDatasetId, StorageDatasetRegistryEnumKind, StorageDatasetRegistryEnumOrigin,
};

#[cfg(test)]
#[path = "../../../tests/presentation/datasets.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationSettingsDatasetsIntent {
    Import(StorageDatasetRegistryEnumKind),
    Replace(StorageDatasetId),
    Delete(StorageDatasetId),
}

#[derive(Default)]
pub struct PresentationSettingsDatasets {
    pending_delete: Option<StorageDatasetId>,
}

impl PresentationSettingsDatasets {
    pub const fn new() -> Self {
        Self {
            pending_delete: None,
        }
    }

    pub const fn pending_delete(&self) -> Option<StorageDatasetId> {
        self.pending_delete
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        starters: &[DesktopDatasetStarter],
        rows: &[DesktopDatasetRow],
    ) -> Option<PresentationSettingsDatasetsIntent> {
        let mut intent = None;
        ui.heading("Data");
        ui.small("Datasets are never downloaded or updated automatically");
        ui.heading("Packaged starter sources");
        if starters.is_empty() {
            ui.label("No packaged starter sources are available");
        }
        for starter in starters {
            ui.label(format!(
                "{} {} ({}), from {}",
                starter.source, starter.version, starter.license, starter.origin
            ));
        }
        ui.heading("Installed datasets");
        if rows.is_empty() {
            ui.label("No datasets are installed");
        }
        for row in rows {
            if let Some(chosen) = self.show_row(ui, row) {
                intent = Some(chosen);
            }
        }
        ui.horizontal_wrapped(|ui| {
            for (label, kind) in [
                ("Import Games", StorageDatasetRegistryEnumKind::Corpus),
                ("Import Opening Book", StorageDatasetRegistryEnumKind::Book),
                ("Import Puzzles", StorageDatasetRegistryEnumKind::Puzzles),
            ] {
                if ui
                    .add(
                        egui::Button::new(label)
                            .min_size(egui::vec2(0.0, PresentationThemeTokens::MIN_TARGET)),
                    )
                    .clicked()
                {
                    intent = Some(PresentationSettingsDatasetsIntent::Import(kind));
                }
            }
        });
        if let Some(pending) = self.pending_delete {
            let mut confirm = false;
            let mut cancel = false;
            let modal = crate::presentation::theme::components::PresentationThemeComponents::modal(ui.ctx(), egui::Id::new("settings.datasets.delete")).show(
                ui.ctx(),
                |ui| {
                    PresentationAnalysisEditorModal::label(ui, "Delete this dataset");
                    ui.heading("Delete this dataset?");
                    ui.label("Saved history keeps its puzzle and game snapshots. Active work blocks deletion.");
                    cancel = crate::presentation::theme::components::PresentationThemeEnumButton::Ghost.show(ui, "Keep Dataset").clicked();
                    confirm = crate::presentation::theme::components::PresentationThemeEnumButton::DangerSolid.show(ui, "Confirm Delete Dataset").clicked();
                },
            );
            if cancel || modal.should_close() {
                self.pending_delete = None;
            } else if confirm {
                self.pending_delete = None;
                intent = Some(PresentationSettingsDatasetsIntent::Delete(pending));
            }
        }
        intent
    }

    fn show_row(
        &mut self,
        ui: &mut Ui,
        row: &DesktopDatasetRow,
    ) -> Option<PresentationSettingsDatasetsIntent> {
        let mut intent = None;
        let origin = match row.origin {
            StorageDatasetRegistryEnumOrigin::Starter => "Packaged starter",
            StorageDatasetRegistryEnumOrigin::Personal => "Personal",
        };
        let records = row.records.map_or_else(
            || "record count unavailable".to_owned(),
            |count| format!("{count} records"),
        );
        ui.label(format!(
            "{} {}: {origin}, {records}, license {}",
            row.name, row.version, row.license
        ));
        if row.superseded {
            ui.small("Replaced. This version is kept until active work finishes");
        }
        if !row.refusal.is_empty() {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!(
                    "{} failed integrity validation and is unavailable. Library, history and unaffected play continue",
                    row.name
                ),
            );
        }
        if row.origin == StorageDatasetRegistryEnumOrigin::Personal && !row.superseded {
            ui.horizontal(|ui| {
                if ui.button(format!("Replace {}", row.name)).clicked() {
                    intent = Some(PresentationSettingsDatasetsIntent::Replace(row.id));
                }
                if ui.button(format!("Delete {}", row.name)).clicked() {
                    self.pending_delete = Some(row.id);
                }
            });
        }
        intent
    }
}
