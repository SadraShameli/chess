use bevy_egui::egui::{self, Ui};

use crate::notation::pgn_export::NotationPgnExportEnumSelection;
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::presentation::library::view::{
    PresentationLibraryEnumEffect, PresentationLibraryEnumFileRequest, PresentationLibraryPanel,
};
use crate::storage::library_query::StorageLibraryQueryEntry;

impl PresentationLibraryPanel {
    pub(super) fn export_buttons(
        ui: &mut Ui,
        frame: &mut super::PresentationLibraryFrame,
        row: &StorageLibraryQueryEntry,
        enabled: bool,
    ) -> Option<PresentationLibraryEnumFileRequest> {
        let mut chosen = None;
        for (label, selection) in [
            ("Export full PGN", NotationPgnExportEnumSelection::Full),
            ("Export mainline PGN", NotationPgnExportEnumSelection::Mainline),
        ] {
            let button = ui.add_enabled(enabled, egui::Button::new(label));
            if frame.clicked(ui, &button) {
                chosen = Some(PresentationLibraryEnumFileRequest::ExportDestination {
                    game: row.id(),
                    storage_revision: row.content_revision(),
                    selection,
                });
            }
        }
        chosen
    }

    pub(super) fn export_status(&self, ui: &mut Ui) {
        if self
            .controller
            .is_pending(PresentationLibraryEnumLane::Export)
        {
            ui.spinner();
            ui.label("Exporting PGN");
        }
        if let Some(bytes) = self.controller.export_bytes() {
            ui.label(format!("Exported {bytes} bytes"));
        }
    }

    pub(super) fn export_effect(
        request: PresentationLibraryEnumFileRequest,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        effects.push(PresentationLibraryEnumEffect::SelectFile(request));
    }
}
