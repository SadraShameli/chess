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
use crate::desktop::file_handle::DesktopFileEnumPurpose;
use crate::desktop::identity::DesktopRevision;
use crate::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
    DesktopRequest,
};
use crate::presentation::settings::backup::{
    PresentationSettingsBackup, PresentationSettingsBackupEnumIntent,
};
use crate::presentation::settings::restore::{
    PresentationSettingsRestore, PresentationSettingsRestoreEnumIntent,
};
use crate::storage::backup::preview::BackupPreviewEnumFinding;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PresentationAppBackupEnumKind {
    Operation,
    FileSelect,
}

struct PresentationAppBackupPending {
    request: DesktopRequest,
    receipt: Option<DesktopApplicationReceipt>,
    kind: PresentationAppBackupEnumKind,
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
    file_owner: DesktopMessagesEntityId,
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
            file_owner: DesktopMessagesEntityId::new(),
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
        self.submit_command(
            port,
            DesktopCommand::new(DesktopEnumCommand::Backup(operation), None),
            PresentationAppBackupEnumKind::Operation,
        );
    }

    fn select_archive_file<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) {
        self.submit_command(
            port,
            DesktopCommand::new(
                DesktopEnumCommand::FileSelect {
                    owner: self.file_owner,
                    purpose: DesktopFileEnumPurpose::BackupArchive,
                },
                Some(DesktopRevision::ZERO),
            ),
            PresentationAppBackupEnumKind::FileSelect,
        );
    }

    fn submit_command<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
        command: DesktopCommand,
        kind: PresentationAppBackupEnumKind,
    ) {
        if self.pending.is_some() {
            return;
        }
        match port.request(command) {
            Ok(request) => {
                self.pending = Some(PresentationAppBackupPending {
                    request,
                    receipt: None,
                    kind,
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
        let kind = pending.kind;
        self.pending = None;
        match response {
            DesktopMessagesEnumResult::Value {
                value: DesktopEnumValue::FileSelect(selection),
                ..
            } if kind == PresentationAppBackupEnumKind::FileSelect => match selection {
                Some(file) => self.submit(
                    port,
                    DesktopBackupBridgeEnumOperation::Import {
                        owner: self.file_owner,
                        file,
                    },
                ),
                None => self.notice = Some("No backup file was selected".to_owned()),
            },
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
                Some(PresentationSettingsBackupEnumIntent::ImportArchive) => {
                    self.select_archive_file(port)
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
