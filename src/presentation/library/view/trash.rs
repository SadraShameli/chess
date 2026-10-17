use bevy_egui::egui::{self, Ui};

use crate::desktop::identity::DesktopRequestId;
use crate::game::library_organization::GameLibraryOrganizationGameTarget;
use crate::presentation::library::view::{PresentationLibraryEnumEffect, PresentationLibraryPanel};
use crate::storage::library_query::StorageLibraryQueryEntry;
use crate::storage::trash::StorageTrashEnumLocation;

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
}
