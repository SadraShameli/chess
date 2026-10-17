use bevy_egui::egui::{self, Ui};

use crate::desktop::identity::DesktopRequestId;
use crate::game::library_organization::GameLibraryOrganizationGameTarget;
use crate::presentation::library::view::{PresentationLibraryEnumEffect, PresentationLibraryPanel};
use crate::storage::library_query::StorageLibraryQueryEntry;
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::storage::trash::{StorageTrashEnumLocation, StorageTrashPreviewRequest};

impl PresentationLibraryPanel {
    pub(super) fn in_trash(&self) -> bool {
        self.controller.options().location == StorageTrashEnumLocation::Trash
    }

    pub(super) fn trash_controls(
        &mut self,
        ui: &mut Ui,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        let (label, target) = if self.in_trash() {
            ("Show Library games", StorageTrashEnumLocation::Library)
        } else {
            ("Show Trash", StorageTrashEnumLocation::Trash)
        };
        let switch = ui.add_enabled(
            !self.controller.is_query_pending(),
            egui::Button::new(label),
        );
        if self.frame.clicked(ui, &switch) {
            let mut options = self.controller.options().clone();
            options.location = target;
            let result = self.controller.query(DesktopRequestId::new(), options);
            self.collect(result, effects);
        }
    }

    pub(super) fn restore_target(
        row: &StorageLibraryQueryEntry,
    ) -> GameLibraryOrganizationGameTarget {
        GameLibraryOrganizationGameTarget::new(row.id(), row.organization().revision())
    }

    pub(super) fn request_game_trash(
        &mut self,
        request: StorageTrashPreviewRequest,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        let result = self
            .controller
            .preview_trash(DesktopRequestId::new(), request);
        self.collect(result, effects);
    }

    pub(super) fn game_trash_confirmation(
        &mut self,
        ui: &mut Ui,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        let Some(plan) = self.controller.trash_plan() else {
            return;
        };
        let (game, permanent) = match plan.action() {
            StorageTrashPreviewRequest::MoveToTrash(game) => (game, false),
            StorageTrashPreviewRequest::PermanentlyDelete(game) => (game, true),
            StorageTrashPreviewRequest::RemoveFolder(_) => return,
        };
        let busy = self
            .controller
            .is_pending(PresentationLibraryEnumLane::Trash);
        ui.group(|ui| {
            if permanent {
                ui.label(format!(
                    "Permanently delete game {}? This cannot be undone.",
                    game.as_uuid()
                ));
            } else {
                ui.label(format!(
                    "Move game {} to Trash? It can be restored from Trash.",
                    game.as_uuid()
                ));
            }
            let label = if permanent {
                "Confirm permanent delete"
            } else {
                "Confirm Move to Trash"
            };
            let confirm = ui.add_enabled(!busy, egui::Button::new(label));
            if self.frame.clicked(ui, &confirm) {
                let result = self.controller.confirm_game_trash(DesktopRequestId::new());
                self.collect(result, effects);
            }
            let cancel = ui.add_enabled(!busy, egui::Button::new("Keep game"));
            if self.frame.clicked(ui, &cancel) {
                self.controller.dismiss_trash_plan();
            }
        });
    }
}
