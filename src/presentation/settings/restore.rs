use bevy_egui::egui::Ui;

use crate::desktop::backup_commands::{
    DesktopBackupCommandsEnumPrerequisite, DesktopBackupCommandsPrerequisites,
    DesktopBackupCommandsPreviewHandle,
};
use crate::storage::backup::preview::BackupPreviewEnumFinding;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationSettingsRestoreEnumIntent {
    Confirm(DesktopBackupCommandsPreviewHandle),
    Cancel,
}

pub struct PresentationSettingsRestore;

impl PresentationSettingsRestore {
    pub fn describe(finding: &BackupPreviewEnumFinding) -> String {
        match finding {
            BackupPreviewEnumFinding::CrossPlatform {
                archive,
                destination,
            } => format!(
                "This backup was made on {archive:?} and will be restored on {destination:?}."
            ),
            BackupPreviewEnumFinding::DestinationLocationsKept { stored } => format!(
                "{} stored locations ({}) are replaced by this computer's own locations where they exist.",
                stored.len(),
                stored.first().map_or("none", String::as_str)
            ),
            BackupPreviewEnumFinding::DatasetReferenced(dependency) => format!(
                "The backup references {}; its files are not part of the backup.",
                dependency.name
            ),
            BackupPreviewEnumFinding::DependencyUnavailable(dependency) => format!(
                "{:?} {} is not installed here; the related options will be unavailable.",
                dependency.kind, dependency.name
            ),
            BackupPreviewEnumFinding::OlderSchemaWillMigrate { pending } => format!(
                "The backup will be upgraded by {} database changes before use.",
                pending.len()
            ),
        }
    }

    pub fn blocked_reason(
        prerequisites: DesktopBackupCommandsPrerequisites,
    ) -> Option<&'static str> {
        prerequisites.first_unmet().map(|unmet| match unmet {
            DesktopBackupCommandsEnumPrerequisite::GamesPausedAndSaved => {
                "Pause and save every open game before restoring."
            }
            DesktopBackupCommandsEnumPrerequisite::EditorsHaveNoUnsavedEdits => {
                "Save or discard unsaved edits before restoring."
            }
        })
    }

    pub fn show(
        ui: &mut Ui,
        handle: DesktopBackupCommandsPreviewHandle,
        findings: &[BackupPreviewEnumFinding],
        prerequisites: DesktopBackupCommandsPrerequisites,
    ) -> Option<PresentationSettingsRestoreEnumIntent> {
        ui.heading("Restore from backup");
        ui.label("A safety backup of the current data is created first. Games and puzzle runs reopen paused.");
        for finding in findings {
            ui.label(Self::describe(finding));
        }
        let blocked = Self::blocked_reason(prerequisites);
        if let Some(reason) = blocked {
            ui.colored_label(ui.visuals().warn_fg_color, reason);
        }
        let mut intent = None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(blocked.is_none(), bevy_egui::egui::Button::new("Restore"))
                .clicked()
            {
                intent = Some(PresentationSettingsRestoreEnumIntent::Confirm(handle));
            }
            if ui.button("Cancel").clicked() {
                intent = Some(PresentationSettingsRestoreEnumIntent::Cancel);
            }
        });
        intent
    }
}
