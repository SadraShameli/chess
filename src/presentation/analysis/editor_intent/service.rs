use crate::desktop::game_editor::{
    DesktopGameEditorContext, DesktopGameEditorEnumSource, DesktopGameEditorPublication,
};
use crate::desktop::library_service::DesktopLibraryServiceEnumSaveReceipt;
use crate::game::enums::GameRevision;
use crate::presentation::analysis::editor_intent::PresentationAnalysisEditorError;

pub(crate) struct PresentationAnalysisEditorServiceCompletion;

impl PresentationAnalysisEditorServiceCompletion {
    pub(crate) fn same_owner(
        expected: DesktopGameEditorContext,
        actual: DesktopGameEditorContext,
    ) -> bool {
        expected.game() == actual.game()
            && expected.epoch() == actual.epoch()
            && std::mem::discriminant(&expected.source())
                == std::mem::discriminant(&actual.source())
    }

    pub(crate) fn check_snapshot(
        expected: DesktopGameEditorContext,
        actual: DesktopGameEditorContext,
    ) -> Result<(), PresentationAnalysisEditorError> {
        if !Self::same_owner(expected, actual)
            || actual.generation().value() < expected.generation().value()
            || Self::revision(actual) < Self::revision(expected)
            || actual.storage_revision().value() < expected.storage_revision().value()
        {
            return Err(PresentationAnalysisEditorError::UnexpectedReply);
        }
        Ok(())
    }

    pub(crate) fn saved(
        expected: DesktopGameEditorContext,
        snapshot: &DesktopGameEditorPublication,
        receipt: &DesktopLibraryServiceEnumSaveReceipt,
    ) -> Result<GameRevision, PresentationAnalysisEditorError> {
        let actual = snapshot.context();
        Self::check_snapshot(expected, actual)?;
        let (game, storage, covered) = match (expected.source(), receipt) {
            (
                DesktopGameEditorEnumSource::Played { .. },
                DesktopLibraryServiceEnumSaveReceipt::Played(receipt),
            ) => (
                receipt.game(),
                receipt.storage_revision(),
                receipt.game_revision(),
            ),
            (
                DesktopGameEditorEnumSource::Notation { .. },
                DesktopLibraryServiceEnumSaveReceipt::Notation { receipt, covered },
            ) => (receipt.id(), receipt.content_revision(), *covered),
            (
                DesktopGameEditorEnumSource::Played { .. },
                DesktopLibraryServiceEnumSaveReceipt::Notation { .. },
            )
            | (
                DesktopGameEditorEnumSource::Notation { .. },
                DesktopLibraryServiceEnumSaveReceipt::Played(_),
            ) => return Err(PresentationAnalysisEditorError::ForeignReceipt),
        };
        if game != expected.game() {
            return Err(PresentationAnalysisEditorError::ForeignReceipt);
        }
        if covered <= Self::revision(expected)
            || covered > Self::revision(actual)
            || storage.value() <= expected.storage_revision().value()
            || storage.value() > actual.storage_revision().value()
        {
            return Err(PresentationAnalysisEditorError::NotSaved);
        }
        Ok(covered)
    }

    pub(crate) fn revision(context: DesktopGameEditorContext) -> GameRevision {
        match context.source() {
            DesktopGameEditorEnumSource::Played { revision }
            | DesktopGameEditorEnumSource::Notation { revision } => revision,
        }
    }
}
