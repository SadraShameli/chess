use std::collections::BTreeSet;

use bevy_egui::egui::Ui;

use crate::desktop::dataset_commands::DesktopDatasetRow;
use crate::storage::dataset_registry::{StorageDatasetId, StorageDatasetRegistryEnumKind};

pub struct PresentationOpeningDatasetPicker;

impl PresentationOpeningDatasetPicker {
    pub fn usable(rows: &[DesktopDatasetRow]) -> Vec<&DesktopDatasetRow> {
        rows.iter()
            .filter(|row| {
                row.kind == StorageDatasetRegistryEnumKind::Corpus
                    && !row.superseded
                    && row.refusal.is_empty()
            })
            .collect()
    }

    pub fn show(
        ui: &mut Ui,
        rows: &[DesktopDatasetRow],
        selected: &mut BTreeSet<StorageDatasetId>,
    ) -> bool {
        let mut changed = false;
        ui.heading("Opening data sources");
        ui.small("Opening data supports Standard chess only");
        let usable = Self::usable(rows);
        if usable.is_empty() {
            ui.label("No usable opening data is installed");
        }
        for row in usable {
            let mut checked = selected.contains(&row.id);
            if ui
                .checkbox(&mut checked, format!("{} {}", row.name, row.version))
                .changed()
            {
                if checked {
                    selected.insert(row.id);
                } else {
                    selected.remove(&row.id);
                }
                changed = true;
            }
        }
        match selected.len() {
            0 => ui.label("No source is selected. Statistics are unavailable"),
            1 => ui.label("One source is selected"),
            _ => ui.label("Sources are combined. Duplicate games count once"),
        };
        for row in rows {
            if row.kind == StorageDatasetRegistryEnumKind::Corpus && !row.refusal.is_empty() {
                ui.label(format!(
                    "{} is unavailable because it failed integrity validation",
                    row.name
                ));
            }
        }
        changed
    }
}
