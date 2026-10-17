use std::sync::Arc;

use crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumResult;
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::game_editor::{DesktopGameEditorContext, DesktopGameEditorPublication};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumDisposition, DesktopLibraryServiceEnumResult,
    DesktopLibraryServiceError,
};
use crate::game::annotations::GameAnnotationsEnumChange;
use crate::game::enums::GameRevision;
use crate::presentation::analysis::annotations::{
    PresentationAnalysisAnnotations, PresentationAnalysisAnnotationsDraft,
    PresentationAnalysisAnnotationsEnumEffect,
};
use crate::presentation::analysis::editor_intent::{
    PresentationAnalysisEditorError, PresentationAnalysisEditorServiceCompletion,
};

impl PresentationAnalysisAnnotations {
    pub fn service_failure(&self) -> Option<&DesktopLibraryServiceError> {
        self.save
            .as_ref()
            .and_then(|save| save.request.service_failure())
    }

    pub fn observe_service_result(
        &mut self,
        application: &DesktopApplicationReceipt,
        expected: DesktopGameEditorContext,
        result: &DesktopLibraryServiceEnumResult,
    ) -> Result<Option<PresentationAnalysisAnnotationsEnumEffect>, PresentationAnalysisEditorError>
    {
        let save = self
            .save
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        save.request.check(application, expected)?;
        match result {
            DesktopLibraryServiceEnumResult::Saved { snapshot, receipt } => {
                let covered = PresentationAnalysisEditorServiceCompletion::saved(
                    expected, snapshot, receipt,
                )?;
                if (covered
                    == PresentationAnalysisEditorServiceCompletion::revision(snapshot.context())
                    && !Self::same_position_owner(expected, snapshot.context()))
                    || save.accepted.as_ref().is_some_and(|accepted| {
                        PresentationAnalysisEditorServiceCompletion::revision(accepted.context())
                            > covered
                    })
                {
                    return Err(PresentationAnalysisEditorError::UnexpectedReply);
                }
                self.complete_service_save(snapshot, covered)
            }
            DesktopLibraryServiceEnumResult::Editor(
                DesktopAnnotationCommandsEnumResult::Edited {
                    change: GameAnnotationsEnumChange::Unchanged,
                    snapshot,
                },
            ) => {
                if snapshot.context() != expected {
                    return Err(PresentationAnalysisEditorError::UnexpectedReply);
                }
                self.complete_service_save(
                    snapshot,
                    PresentationAnalysisEditorServiceCompletion::revision(expected),
                )
            }
            DesktopLibraryServiceEnumResult::Editor(result) => {
                self.observe_result(application, expected, result)?;
                Ok(None)
            }
            DesktopLibraryServiceEnumResult::PendingSnapshot { snapshot, pending } => {
                PresentationAnalysisEditorServiceCompletion::check_snapshot(
                    expected,
                    snapshot.context(),
                )?;
                if !Self::same_position_owner(expected, snapshot.context()) {
                    return Err(PresentationAnalysisEditorError::UnexpectedReply);
                }
                if self.publication.as_ref().is_none_or(|current| {
                    !PresentationAnalysisEditorServiceCompletion::same_owner(
                        expected,
                        current.context(),
                    )
                }) {
                    return Err(PresentationAnalysisEditorError::Conflict);
                }
                self.save
                    .as_mut()
                    .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?
                    .request
                    .set_service_pending(Some(*pending));
                self.source_pending = Some(*pending);
                self.feedback = Some("Finishing the save. Your draft is still here.".to_owned());
                Ok(None)
            }
            DesktopLibraryServiceEnumResult::Query(_)
            | DesktopLibraryServiceEnumResult::Metadata(_)
            | DesktopLibraryServiceEnumResult::Organized(_)
            | DesktopLibraryServiceEnumResult::Import(_)
            | DesktopLibraryServiceEnumResult::Exported(_)
            | DesktopLibraryServiceEnumResult::TrashPlan(_)
            | DesktopLibraryServiceEnumResult::Trashed(_)
            | DesktopLibraryServiceEnumResult::Restored(_)
            | DesktopLibraryServiceEnumResult::Opened(_) => {
                Err(PresentationAnalysisEditorError::WrongResult)
            }
        }
    }

    pub fn observe_service_error(
        &mut self,
        application: &DesktopApplicationReceipt,
        expected: DesktopGameEditorContext,
        error: &Arc<DesktopLibraryServiceError>,
    ) -> Result<(), PresentationAnalysisEditorError> {
        let save = self
            .save
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        let disposition = save
            .request
            .observe_service_error(application, expected, error)?;
        if matches!(disposition, DesktopLibraryServiceEnumDisposition::Refused) {
            if save
                .request
                .service_pending()
                .is_some_and(|pending| self.source_pending == Some(pending))
            {
                self.source_pending = None;
            }
            self.finish_save();
        }
        self.feedback = Some(error.to_string());
        Ok(())
    }

    fn complete_service_save(
        &mut self,
        snapshot: &DesktopGameEditorPublication,
        covered: GameRevision,
    ) -> Result<Option<PresentationAnalysisAnnotationsEnumEffect>, PresentationAnalysisEditorError>
    {
        let save = self
            .save
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        let expected = save.request.intent().expected();
        let is_latest =
            PresentationAnalysisEditorServiceCompletion::revision(snapshot.context()) == covered;
        if is_latest && !save.matches_publication(snapshot) {
            return Err(PresentationAnalysisEditorError::WrongResult);
        }
        let draft = self
            .draft
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::NoSelection)?;
        let draft_matches = save.matches_draft(draft);
        let is_current = self.publication.as_ref().is_some_and(|current| {
            current.context() == expected
                || current.context() == snapshot.context()
                || save
                    .accepted
                    .as_ref()
                    .is_some_and(|accepted| accepted.context() == current.context())
        });
        self.finish_save();
        if !is_current || !is_latest || !draft_matches {
            self.is_conflicted = true;
            if is_current {
                self.publication = Some(snapshot.clone());
                self.source_pending = None;
            }
            self.feedback = Some("Your earlier changes were saved. Your draft is still here. Review it before leaving.".to_owned());
            return Ok(None);
        }
        self.publication = Some(snapshot.clone());
        self.source_pending = None;
        self.clear_temporary();
        self.draft = Some(PresentationAnalysisAnnotationsDraft::capture(snapshot));
        self.is_conflicted = false;
        self.feedback = Some("Changes saved.".to_owned());
        let Some(switch) = self.guard.take() else {
            return Ok(None);
        };
        if switch.expected != expected {
            self.history.complete(switch.request_id);
            return Ok(None);
        }
        self.release_switch(switch, snapshot.context()).map(Some)
    }
}
