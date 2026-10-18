use bevy_egui::egui::Ui;

use crate::desktop::editor_commands::DesktopEditorCommandsContext;
use crate::presentation::editor::draft_list::{
    PresentationEditorDraftList, PresentationEditorDraftListEnumAction,
};
use crate::storage::editor_repository::StorageEditorRepositoryPage;

#[derive(Default)]
pub struct PresentationLibraryPositions {
    drafts: PresentationEditorDraftList,
}

impl PresentationLibraryPositions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        page: &StorageEditorRepositoryPage,
        expected: Option<DesktopEditorCommandsContext>,
    ) -> Option<PresentationEditorDraftListEnumAction> {
        ui.heading("Positions");
        self.drafts.show(ui, page, expected)
    }
}
