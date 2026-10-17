use std::collections::BTreeSet;

use bevy_egui::egui;
use egui_kittest::{Harness, kittest::Queryable};

use crate::desktop::dataset_commands::{DesktopDatasetRow, DesktopDatasetStarter};
use crate::package::digest::PackageDigest;
use crate::presentation::opening::dataset_picker::PresentationOpeningDatasetPicker;
use crate::presentation::settings::datasets::{
    PresentationSettingsDatasets, PresentationSettingsDatasetsIntent,
};
use crate::storage::dataset_integrity::{
    StorageDatasetIntegrityEnumFeature, StorageDatasetIntegrityEnumProblem,
    StorageDatasetIntegrityFinding,
};
use crate::storage::dataset_package::{
    StorageDatasetPackageEnumSourceKind, StorageDatasetPackageEnumValidation,
};
use crate::storage::dataset_registry::{
    StorageDatasetId, StorageDatasetRegistryEnumKind, StorageDatasetRegistryEnumOrigin,
};

fn row(
    id: i32,
    name: &str,
    origin: StorageDatasetRegistryEnumOrigin,
    broken: bool,
) -> DesktopDatasetRow {
    DesktopDatasetRow {
        id: StorageDatasetId::new(id),
        kind: StorageDatasetRegistryEnumKind::Corpus,
        origin,
        name: name.to_owned(),
        version: "v1".to_owned(),
        license: "CC0".to_owned(),
        records: if broken { None } else { Some(1200) },
        validation: StorageDatasetPackageEnumValidation::IntegrityVerified,
        superseded: false,
        refusal: if broken {
            vec![StorageDatasetIntegrityFinding {
                feature: StorageDatasetIntegrityEnumFeature::Explorer,
                artifact: "master.db".to_owned(),
                problem: StorageDatasetIntegrityEnumProblem::Missing,
            }]
        } else {
            Vec::new()
        },
    }
}

struct PresentationDatasetsMounted {
    settings: PresentationSettingsDatasets,
    starters: Vec<DesktopDatasetStarter>,
    rows: Vec<DesktopDatasetRow>,
    intents: Vec<PresentationSettingsDatasetsIntent>,
}

impl PresentationDatasetsMounted {
    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if let Some(intent) = state.settings.show(ui, &state.starters, &state.rows) {
            state.intents.push(intent);
            ui.ctx().request_discard("Exercise dataset intents once");
        }
    }
}

#[test]
fn settings_show_sources_counts_truthful_refusals_and_confirm_before_deleting()
-> Result<(), Box<dyn std::error::Error>> {
    let starter = DesktopDatasetStarter {
        source: "lichess-standard".to_owned(),
        version: "2025-12".to_owned(),
        kind: StorageDatasetPackageEnumSourceKind::StandardGames,
        license: "CC0".to_owned(),
        origin: "https://database.lichess.org".to_owned(),
        checksum: PackageDigest::hash_slice(b"fixture"),
    };
    let state = PresentationDatasetsMounted {
        settings: PresentationSettingsDatasets::new(),
        starters: vec![starter],
        rows: vec![
            row(1, "Masters", StorageDatasetRegistryEnumOrigin::Starter, true),
            row(2, "Club games", StorageDatasetRegistryEnumOrigin::Personal, false),
        ],
        intents: Vec::new(),
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 1400.0))
        .build_ui_state(PresentationDatasetsMounted::show, state);
    harness.run_ok().ok_or("Datasets did not mount")?;
    harness.get_by_label("Datasets are never downloaded or updated automatically");
    harness.get_by_label("lichess-standard 2025-12 (CC0), from https://database.lichess.org");
    harness.get_by_label("Masters v1: Packaged starter, record count unavailable, license CC0");
    harness.get_by_label("Club games v1: Personal, 1200 records, license CC0");
    harness.get_by_label(
        "Masters failed integrity validation and is unavailable. Library, history and unaffected play continue",
    );
    harness.get_by_label("Delete Club games").click_accesskit();
    harness.run_ok().ok_or("Confirmation did not settle")?;
    harness.get_by_label("Delete this dataset?");
    assert!(harness.state().intents.is_empty());
    harness
        .get_by_label("Confirm Delete Dataset")
        .click_accesskit();
    harness.run_ok().ok_or("Deletion did not settle")?;
    assert_eq!(
        harness.state().intents,
        vec![PresentationSettingsDatasetsIntent::Delete(
            StorageDatasetId::new(2)
        )]
    );
    Ok(())
}

#[test]
fn picker_lists_only_usable_sources_and_discloses_combination() -> Result<(), Box<dyn std::error::Error>> {
    let rows = vec![
        row(1, "Masters", StorageDatasetRegistryEnumOrigin::Starter, true),
        row(2, "Club games", StorageDatasetRegistryEnumOrigin::Personal, false),
        row(3, "Online games", StorageDatasetRegistryEnumOrigin::Personal, false),
    ];
    assert_eq!(PresentationOpeningDatasetPicker::usable(&rows).len(), 2);
    let mut selected = BTreeSet::from([StorageDatasetId::new(2), StorageDatasetId::new(3)]);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 900.0))
        .build_ui(move |ui| {
            PresentationOpeningDatasetPicker::show(ui, &rows, &mut selected);
        });
    harness.run_ok().ok_or("Picker did not mount")?;
    harness.get_by_label("Opening data supports Standard chess only");
    harness.get_by_label("Sources are combined. Duplicate games count once");
    harness.get_by_label("Masters is unavailable because it failed integrity validation");
    Ok(())
}
