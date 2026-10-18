use bevy_egui::egui::{self, Ui};

use crate::desktop::backup_commands::DesktopBackupCommandsEntry;
use crate::storage::backup::manifest::BackupEnumCategory;
use crate::storage::backup::scheduler::{BackupEnumDue, BackupScheduler, BackupState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationSettingsBackupEnumIntent {
    BackupNow,
    RetryAutomatic,
    ChooseDestination,
    SelectArchive,
    ImportArchive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationSettingsBackupRow {
    pub label: String,
    pub category: BackupEnumCategory,
    pub megabytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationSettingsBackupStatus {
    pub warning: Option<String>,
    pub schedule: String,
    pub rows: Vec<PresentationSettingsBackupRow>,
}

pub struct PresentationSettingsBackup;

impl PresentationSettingsBackup {
    const BYTES_PER_MEGABYTE: u64 = 1024 * 1024;

    pub fn status(
        state: &BackupState,
        now_ms: u64,
        entries: &[DesktopBackupCommandsEntry],
    ) -> PresentationSettingsBackupStatus {
        let warning = state.warning().map(|detail| {
            format!(
                "The last automatic backup failed: {detail}. Play is not affected; use Retry or Backup Now."
            )
        });
        let schedule = match BackupScheduler::due(state, now_ms) {
            BackupEnumDue::NoUserData => {
                "Automatic backups start after your first saved data".to_owned()
            }
            BackupEnumDue::NotYet { due_ms } => {
                let hours = due_ms.saturating_sub(now_ms) / (60 * 60 * 1000);
                format!("Next automatic backup in about {hours} hours")
            }
            BackupEnumDue::Due { .. } => "An automatic backup is due".to_owned(),
        };
        let rows = entries
            .iter()
            .map(|entry| PresentationSettingsBackupRow {
                label: format!(
                    "{} backup {}",
                    entry.manifest.category().slug(),
                    entry.manifest.id()
                ),
                category: entry.manifest.category(),
                megabytes: entry
                    .manifest
                    .payload_bytes()
                    .div_ceil(Self::BYTES_PER_MEGABYTE),
            })
            .collect();
        PresentationSettingsBackupStatus {
            warning,
            schedule,
            rows,
        }
    }

    pub fn show(
        ui: &mut Ui,
        status: &PresentationSettingsBackupStatus,
    ) -> Option<PresentationSettingsBackupEnumIntent> {
        let mut intent = None;
        if let Some(warning) = &status.warning {
            ui.colored_label(ui.visuals().warn_fg_color, warning);
            if ui.button("Retry automatic backup").clicked() {
                intent = Some(PresentationSettingsBackupEnumIntent::RetryAutomatic);
            }
        }
        ui.label(&status.schedule);
        ui.horizontal(|ui| {
            if ui.button("Backup Now").clicked() {
                intent = Some(PresentationSettingsBackupEnumIntent::BackupNow);
            }
            if ui.button("Choose backup folder").clicked() {
                intent = Some(PresentationSettingsBackupEnumIntent::ChooseDestination);
            }
            if ui.button("Restore from backup").clicked() {
                intent = Some(PresentationSettingsBackupEnumIntent::SelectArchive);
            }
            if ui.button("Restore from a backup file").clicked() {
                intent = Some(PresentationSettingsBackupEnumIntent::ImportArchive);
            }
        });
        for row in &status.rows {
            ui.label(egui::RichText::new(format!(
                "{} ({} MB)",
                row.label, row.megabytes
            )));
        }
        intent
    }
}
