use bevy_egui::egui::Ui;

use crate::board::editor::{BoardEditorDraftId, BoardEditorValidationEnumStatus};
use crate::desktop::editor_commands::DesktopEditorCommandsContext;
use crate::storage::editor_repository::{
    StorageEditorRepositoryCursor, StorageEditorRepositoryEntry, StorageEditorRepositoryPage,
    StorageEditorRepositoryRevision,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentationEditorDraftListEnumAction {
    Open(BoardEditorDraftId),
    Delete {
        id: BoardEditorDraftId,
        storage_revision: StorageEditorRepositoryRevision,
        expected: Option<DesktopEditorCommandsContext>,
    },
    List {
        after: Option<StorageEditorRepositoryCursor>,
    },
}

#[derive(Default)]
pub struct PresentationEditorDraftList {
    delete_confirmation: Option<StorageEditorRepositoryEntry>,
    submitted_frame: Option<u64>,
}

impl PresentationEditorDraftList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        page: &StorageEditorRepositoryPage,
        expected: Option<DesktopEditorCommandsContext>,
    ) -> Option<PresentationEditorDraftListEnumAction> {
        let mut action = None;
        if self
            .delete_confirmation
            .is_some_and(|confirmation| !page.entries().contains(&confirmation))
        {
            self.delete_confirmation = None;
        }
        ui.label("Saved position drafts");
        ui.label("Valid and invalid editor drafts are separate from recorded games.");
        if page.entries().is_empty() {
            ui.label("No saved position drafts on this page");
        }
        for entry in page.entries() {
            ui.push_id(entry.id(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(entry.id().to_string());
                    ui.label(match entry.status() {
                        BoardEditorValidationEnumStatus::Valid => "Valid position draft",
                        BoardEditorValidationEnumStatus::Invalid => "Invalid position draft",
                    });
                    if ui.button(format!("Open draft {}", entry.id())).clicked() {
                        action = Some(PresentationEditorDraftListEnumAction::Open(entry.id()));
                    }
                    if ui.button(format!("Delete draft {}", entry.id())).clicked() {
                        self.delete_confirmation = Some(*entry);
                    }
                });
            });
        }
        if let Some(entry) = self.delete_confirmation {
            ui.group(|ui| {
                ui.label(format!("Delete saved draft {}?", entry.id()));
                if ui.button("Confirm Delete Draft").clicked() {
                    action = Some(PresentationEditorDraftListEnumAction::Delete {
                        id: entry.id(),
                        storage_revision: entry.storage_revision(),
                        expected,
                    });
                    self.delete_confirmation = None;
                }
                if ui.button("Keep Saved Draft").clicked() {
                    self.delete_confirmation = None;
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Refresh Position Drafts").clicked() {
                action = Some(PresentationEditorDraftListEnumAction::List { after: None });
            }
            if let Some(after) = page.next_cursor()
                && ui.button("Next Position Drafts").clicked()
            {
                action = Some(PresentationEditorDraftListEnumAction::List { after: Some(after) });
            }
        });
        let frame = ui.ctx().cumulative_frame_nr();
        if action.is_some() && self.submitted_frame != Some(frame) {
            self.submitted_frame = Some(frame);
            action
        } else {
            None
        }
    }
}
