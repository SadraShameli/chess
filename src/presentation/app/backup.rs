use bevy_egui::egui::Ui;
use uuid::Uuid;

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplicationError, DesktopApplicationPort, DesktopApplicationReceipt,
    DesktopApplicationWakeProvider,
};
use crate::desktop::backup_bridge::{
    DesktopBackupBridgeEnumOperation, DesktopBackupBridgeEnumResult, DesktopBackupBridgeStatus,
};
use crate::desktop::backup_commands::{
    DesktopBackupCommandsPrerequisites, DesktopBackupCommandsPreviewHandle,
};
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEnumResult, DesktopRequest,
};
use crate::presentation::settings::backup::{
    PresentationSettingsBackup, PresentationSettingsBackupEnumIntent,
};
use crate::presentation::settings::restore::{
    PresentationSettingsRestore, PresentationSettingsRestoreEnumIntent,
};
use crate::storage::backup::preview::BackupPreviewEnumFinding;

struct PresentationAppBackupPending {
    request: DesktopRequest,
    receipt: Option<DesktopApplicationReceipt>,
}

pub(in crate::presentation::app) struct PresentationAppBackup {
    status: Option<DesktopBackupBridgeStatus>,
    preview: Option<(
        DesktopBackupCommandsPreviewHandle,
        Vec<BackupPreviewEnumFinding>,
    )>,
    selected: Option<Uuid>,
    notice: Option<String>,
    pending: Option<PresentationAppBackupPending>,
    requested: bool,
}

impl PresentationAppBackup {
    pub(in crate::presentation::app) const fn new() -> Self {
        Self {
            status: None,
            preview: None,
            selected: None,
            notice: None,
            pending: None,
            requested: false,
        }
    }

    pub(in crate::presentation::app) const fn is_busy(&self) -> bool {
        self.pending.is_some()
    }

    fn submit<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
        operation: DesktopBackupBridgeEnumOperation,
    ) {
        if self.pending.is_some() {
            return;
        }
        match port.request(DesktopCommand::new(
            DesktopEnumCommand::Backup(operation),
            None,
        )) {
            Ok(request) => {
                self.pending = Some(PresentationAppBackupPending {
                    request,
                    receipt: None,
                })
            }
            Err(error) => self.notice = Some(error.to_string()),
        }
    }

    pub(in crate::presentation::app) fn poll<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
    >(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) {
        let Some(pending) = &mut self.pending else {
            return;
        };
        if pending.receipt.is_none() {
            match port.try_submit(&pending.request) {
                Ok(receipt) => pending.receipt = Some(receipt),
                Err(DesktopApplicationError::Busy | DesktopApplicationError::EntityBusy) => return,
                Err(error) => {
                    self.notice = Some(error.to_string());
                    self.pending = None;
                    return;
                }
            }
        }
        let response = pending
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.response().cloned());
        let Some(response) = response else {
            return;
        };
        self.pending = None;
        match response {
            DesktopMessagesEnumResult::Value {
                value: DesktopEnumValue::Backup(result),
                ..
            } => self.accept(*result),
            DesktopMessagesEnumResult::Failure(failure) => {
                self.notice = Some(failure.to_string());
            }
            _ => self.notice = Some("The backup request did not complete".to_owned()),
        }
    }

    fn accept(&mut self, result: DesktopBackupBridgeEnumResult) {
        match result {
            DesktopBackupBridgeEnumResult::Status(status) => {
                self.notice = None;
                self.status = Some(status);
            }
            DesktopBackupBridgeEnumResult::Preview { handle, findings } => {
                self.notice = None;
                self.preview = Some((handle, findings));
            }
            DesktopBackupBridgeEnumResult::RestoreStaged { .. } => {
                self.preview = None;
                self.notice = Some(
                    "Restore is staged. Quit and relaunch Chess to finish; a safety backup was created."
                        .to_owned(),
                );
                self.requested = false;
            }
        }
    }

    pub(in crate::presentation::app) fn show<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
    >(
        &mut self,
        ui: &mut Ui,
        port: &DesktopApplicationPort<C, W>,
        now_ms: u64,
        prerequisites: DesktopBackupCommandsPrerequisites,
    ) {
        if !self.requested && self.pending.is_none() {
            self.requested = true;
            self.submit(port, DesktopBackupBridgeEnumOperation::Status);
        }
        self.poll(port);
        ui.separator();
        ui.heading("Backup");
        if let Some(notice) = &self.notice {
            ui.label(notice.clone());
        }
        if let Some(status) = &self.status {
            if status.restore_pending {
                ui.label("A restore is staged and will finish when Chess is next launched.");
            }
            let view = PresentationSettingsBackup::status(&status.state, now_ms, &status.entries);
            let choice = PresentationSettingsBackup::show(ui, &view);
            let entries: Vec<Uuid> = status
                .entries
                .iter()
                .map(|entry| entry.manifest.id())
                .collect();
            for (index, id) in entries.iter().enumerate() {
                if ui
                    .selectable_label(
                        self.selected == Some(*id),
                        format!("Select backup {}", index + 1),
                    )
                    .clicked()
                {
                    self.selected = Some(*id);
                }
            }
            match choice {
                Some(PresentationSettingsBackupEnumIntent::BackupNow) => {
                    self.submit(port, DesktopBackupBridgeEnumOperation::BackupNow)
                }
                Some(PresentationSettingsBackupEnumIntent::RetryAutomatic) => {
                    self.submit(port, DesktopBackupBridgeEnumOperation::RetryAutomatic)
                }
                Some(PresentationSettingsBackupEnumIntent::SelectArchive) => {
                    if let Some(backup) = self.selected {
                        self.submit(port, DesktopBackupBridgeEnumOperation::Preview { backup });
                    } else {
                        self.notice = Some("Select a backup in the list first".to_owned());
                    }
                }
                Some(PresentationSettingsBackupEnumIntent::ChooseDestination) => {
                    self.notice = Some(
                        "Backups are kept in the backups folder inside the application data folder"
                            .to_owned(),
                    );
                }
                None => {}
            }
        }
        if let Some((handle, findings)) = self.preview.clone() {
            match PresentationSettingsRestore::show(ui, handle, &findings, prerequisites) {
                Some(PresentationSettingsRestoreEnumIntent::Confirm(handle)) => self.submit(
                    port,
                    DesktopBackupBridgeEnumOperation::Restore {
                        preview: handle,
                        prerequisites,
                    },
                ),
                Some(PresentationSettingsRestoreEnumIntent::Cancel) => self.preview = None,
                None => {}
            }
        }
    }
}
