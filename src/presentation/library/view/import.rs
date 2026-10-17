use bevy_egui::egui::{self, Ui};

use crate::desktop::file_handle::DesktopFileHandleId;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::{DesktopLibraryServiceExport, DesktopLibraryServiceFile};
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::presentation::library::error::PresentationLibraryError;
use crate::presentation::library::view::{
    PresentationLibraryEnumEffect, PresentationLibraryEnumFileRequest, PresentationLibraryPanel,
};
use crate::storage::import_session::{
    StorageImportSessionDetailRequest, StorageImportSessionEnumChoice,
    StorageImportSessionEnumCommitOutcome, StorageImportSessionPageRequest,
};

impl PresentationLibraryPanel {
    const IMPORT_PAGE_ROWS: usize = 100;
    const IMPORT_PAGE_BYTES: usize = 1024 * 1024;
    const IMPORT_DETAIL_ROWS: usize = 20;
    const IMPORT_DETAIL_BYTES: usize = 64 * 1024;

    pub fn file_selected(
        &mut self,
        request: PresentationLibraryEnumFileRequest,
        handle: DesktopFileHandleId,
    ) -> Vec<PresentationLibraryEnumEffect> {
        let file = DesktopLibraryServiceFile::new(handle);
        let result = match request {
            PresentationLibraryEnumFileRequest::ImportSource => self
                .controller
                .import_prepare(DesktopRequestId::new(), file),
            PresentationLibraryEnumFileRequest::ExportDestination {
                game,
                storage_revision,
                selection,
            } => self.controller.export_game(
                DesktopRequestId::new(),
                DesktopLibraryServiceExport::new(game, storage_revision, selection, file),
            ),
        };
        let mut effects = Vec::new();
        self.collect(result, &mut effects);
        effects
    }

    pub(super) fn import_controls(
        &mut self,
        ui: &mut Ui,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        let busy = self
            .controller
            .is_pending(PresentationLibraryEnumLane::Import);
        if busy {
            ui.spinner();
            ui.label("Working on the import preview");
        }
        let Some(summary) = self.controller.import().summary().copied() else {
            let choose = ui.add_enabled(!busy, egui::Button::new("Choose PGN file to import"));
            if self.frame.clicked(ui, &choose) {
                effects.push(PresentationLibraryEnumEffect::SelectFile(
                    PresentationLibraryEnumFileRequest::ImportSource,
                ));
            }
            self.import_outcome(ui);
            return;
        };
        ui.label(format!(
            "{} games ready, {} excluded, {} duplicates, {} selected for import. Duplicates are skipped unless you keep a copy.",
            summary.valid(),
            summary.excluded(),
            summary.duplicates(),
            summary.selected()
        ));
        let rows = ui.add_enabled(!busy, egui::Button::new("Show preview rows"));
        if self.frame.clicked(ui, &rows) {
            let result = StorageImportSessionPageRequest::new(
                0,
                Self::IMPORT_PAGE_ROWS,
                Self::IMPORT_PAGE_BYTES,
            )
            .map_err(PresentationLibraryError::from)
            .and_then(|request| {
                self.controller
                    .import_page(DesktopRequestId::new(), request)
            });
            self.collect(result, effects);
        }
        self.import_rows(ui, busy, effects);
        self.import_details(ui);
        let freeze = ui.add_enabled(!busy, egui::Button::new("Freeze preview"));
        if self.frame.clicked(ui, &freeze) {
            let result = self.controller.import_freeze(DesktopRequestId::new());
            self.collect(result, effects);
        }
        if let Some(confirmation) = self.controller.import().confirmation() {
            let committed = confirmation.summary().selected();
            let commit = ui.add_enabled(
                !busy,
                egui::Button::new(format!("Commit import of {committed} games")),
            );
            if self.frame.clicked(ui, &commit) {
                let result = self.controller.import_commit(DesktopRequestId::new());
                self.collect(result, effects);
            }
        }
        let cancel = ui.add_enabled(!busy, egui::Button::new("Cancel import"));
        if self.frame.clicked(ui, &cancel) {
            let result = self.controller.import_cancel(DesktopRequestId::new());
            self.collect(result, effects);
        }
    }

    fn import_rows(
        &mut self,
        ui: &mut Ui,
        busy: bool,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        let mut choose = None;
        let mut detail = None;
        if let Some(page) = self.controller.import().page() {
            for row in page.rows() {
                ui.push_id(row.ordinal(), |ui| {
                    ui.group(|ui| {
                        ui.label(format!(
                            "Game {}: {} vs {}",
                            row.ordinal(),
                            row.white(),
                            row.black()
                        ));
                        if let Some(exclusion) = row.exclusion() {
                            ui.label(format!(
                                "Excluded at line {}, column {}: {}",
                                exclusion.line(),
                                exclusion.column(),
                                exclusion.reason()
                            ));
                            return;
                        }
                        ui.label(format!(
                            "{} exact and {} possible matches in the library",
                            row.exact_matches(),
                            row.potential_matches()
                        ));
                        let current = match row.choice() {
                            Some(StorageImportSessionEnumChoice::Import) => "Import",
                            Some(StorageImportSessionEnumChoice::Skip) => "Skip",
                            Some(StorageImportSessionEnumChoice::KeepCopy) => "Keep copy",
                            None => "No choice",
                        };
                        ui.label(format!("Current choice: {current}"));
                        let has_match = row.exact_matches() > 0 || row.potential_matches() > 0;
                        if has_match {
                            let skip = ui.add_enabled(!busy, egui::Button::new("Skip"));
                            if self.frame.clicked(ui, &skip) {
                                choose =
                                    Some((row.ordinal(), StorageImportSessionEnumChoice::Skip));
                            }
                            let keep = ui.add_enabled(!busy, egui::Button::new("Keep copy"));
                            if self.frame.clicked(ui, &keep) {
                                choose =
                                    Some((row.ordinal(), StorageImportSessionEnumChoice::KeepCopy));
                            }
                            let differences =
                                ui.add_enabled(!busy, egui::Button::new("Show differences"));
                            if self.frame.clicked(ui, &differences) {
                                detail = Some(row.ordinal());
                            }
                        }
                    });
                });
            }
        } else {
            ui.label("Rows have not been loaded");
        }
        if let Some((ordinal, choice)) = choose {
            let result = self
                .controller
                .import_choose(DesktopRequestId::new(), ordinal, choice);
            self.collect(result, effects);
        }
        if let Some(ordinal) = detail {
            let result = StorageImportSessionDetailRequest::new(
                ordinal,
                Self::IMPORT_DETAIL_ROWS,
                Self::IMPORT_DETAIL_BYTES,
            )
            .map_err(PresentationLibraryError::from)
            .and_then(|request| {
                self.controller
                    .import_detail(DesktopRequestId::new(), request)
            });
            self.collect(result, effects);
        }
    }

    fn import_details(&self, ui: &mut Ui) {
        let Some(detail) = self.controller.import().detail() else {
            return;
        };
        for relationship in detail.relationships() {
            let differences = relationship.differences();
            ui.label(format!(
                "{:?}: tree {}, mainline {}, notes {}, NAGs {}, markings {}, metadata {}, warnings {}, clocks {}",
                relationship.relation(),
                Self::changed(differences.tree()),
                Self::changed(differences.selected_mainline()),
                Self::changed(differences.notes()),
                Self::changed(differences.nags()),
                Self::changed(differences.markings()),
                Self::changed(differences.metadata()),
                Self::changed(differences.warnings()),
                Self::changed(differences.source_clocks()),
            ));
        }
    }

    fn changed(value: bool) -> &'static str {
        if value { "differs" } else { "same" }
    }

    fn import_outcome(&self, ui: &mut Ui) {
        match self.controller.import().outcome() {
            Some(StorageImportSessionEnumCommitOutcome::Committed(receipt)) => {
                ui.label(format!("Imported {} games", receipt.count()));
            }
            Some(StorageImportSessionEnumCommitOutcome::CommittedCleanupFailed {
                receipt,
                cleanup,
            }) => {
                ui.label(format!(
                    "Imported {} games, but cleanup of the staging space failed: {cleanup}",
                    receipt.count()
                ));
            }
            None => {}
        }
    }
}
