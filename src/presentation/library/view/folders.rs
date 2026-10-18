use bevy_egui::egui::{self, Ui};

use crate::desktop::identity::DesktopRequestId;
use crate::game::library_organization::GameLibraryOrganizationFolderId;
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::presentation::library::view::{PresentationLibraryEnumEffect, PresentationLibraryPanel};
use crate::storage::trash::StorageTrashPreviewRequest;

impl PresentationLibraryPanel {
    pub(super) fn request_folder_removal(
        &mut self,
        folder: GameLibraryOrganizationFolderId,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        let result = self.controller.preview_trash(
            DesktopRequestId::new(),
            StorageTrashPreviewRequest::RemoveFolder(folder),
        );
        self.collect(result, effects);
    }

    pub(super) fn folder_removal(
        &mut self,
        ui: &mut Ui,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        let Some(plan) = self.controller.trash_plan() else {
            return;
        };
        let StorageTrashPreviewRequest::RemoveFolder(folder) = plan.action() else {
            return;
        };
        let folders = plan.affected_folder_count();
        let games = plan.affected_game_count();
        let busy = self
            .controller
            .is_pending(PresentationLibraryEnumLane::Trash);
        ui.group(|ui| {
            ui.label(format!(
                "Remove folder {folder}? {folders} folders are removed and {games} games move to Unfiled. No game is deleted."
            ));
            let confirm = ui.add_enabled(!busy, egui::Button::new("Confirm folder removal"));
            if self.frame.clicked(ui, &confirm) {
                let result = self.controller.remove_folder(DesktopRequestId::new());
                self.collect(result, effects);
            }
            let cancel = ui.add_enabled(!busy, egui::Button::new("Keep folder"));
            if self.frame.clicked(ui, &cancel) {
                self.controller.dismiss_trash_plan();
            }
        });
    }
}
