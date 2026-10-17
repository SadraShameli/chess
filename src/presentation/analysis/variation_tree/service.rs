use std::sync::Arc;

use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::game_editor::DesktopGameEditorContext;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumDisposition, DesktopLibraryServiceEnumResult,
    DesktopLibraryServiceError,
};
use crate::game::annotations::GameAnnotationsEnumChange;
use crate::presentation::analysis::editor_intent::{
    PresentationAnalysisEditorError, PresentationAnalysisEditorIntent,
    PresentationAnalysisEditorServiceCompletion,
};
use crate::presentation::analysis::variation_tree::PresentationAnalysisVariationTree;

impl PresentationAnalysisVariationTree {
    pub fn service_failure(&self) -> Option<&DesktopLibraryServiceError> {
        self.pending
            .as_ref()
            .and_then(|pending| pending.service_failure())
    }

    pub fn observe_service_result(
        &mut self,
        application: &DesktopApplicationReceipt,
        expected: DesktopGameEditorContext,
        result: &DesktopLibraryServiceEnumResult,
    ) -> Result<(), PresentationAnalysisEditorError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        pending.check(application, expected)?;
        match result {
            DesktopLibraryServiceEnumResult::Saved { snapshot, receipt } => {
                if !matches!(
                    pending.intent().action(),
                    DesktopAnnotationCommandsEnumAction::AppendStudyBranch { .. }
                        | DesktopAnnotationCommandsEnumAction::ConfirmVariation { .. }
                ) {
                    return Err(PresentationAnalysisEditorError::WrongResult);
                }
                PresentationAnalysisEditorServiceCompletion::saved(expected, snapshot, receipt)?;
                if self.context() != Some(expected) && self.context() != Some(snapshot.context()) {
                    self.finish_pending();
                    return Err(PresentationAnalysisEditorError::StalePage);
                }
                self.finish_pending();
                self.synchronize(snapshot, None);
                self.feedback = Some("Variation saved.".to_owned());
                Ok(())
            }
            DesktopLibraryServiceEnumResult::Editor(
                DesktopAnnotationCommandsEnumResult::Edited {
                    change: GameAnnotationsEnumChange::Changed,
                    snapshot,
                },
            ) => {
                if !matches!(
                    pending.intent().action(),
                    DesktopAnnotationCommandsEnumAction::AppendStudyBranch { .. }
                        | DesktopAnnotationCommandsEnumAction::ConfirmVariation { .. }
                ) {
                    return Err(PresentationAnalysisEditorError::WrongResult);
                }
                PresentationAnalysisEditorServiceCompletion::check_snapshot(
                    expected,
                    snapshot.context(),
                )?;
                if self.context() != Some(expected) && self.context() != Some(snapshot.context()) {
                    return Err(PresentationAnalysisEditorError::StalePage);
                }
                self.synchronize(snapshot, None);
                self.feedback = Some("Saving the variation. Please wait.".to_owned());
                Ok(())
            }
            DesktopLibraryServiceEnumResult::Editor(result) => {
                self.observe_result(application, expected, result)?;
                if matches!(
                    result,
                    DesktopAnnotationCommandsEnumResult::Edited {
                        change: GameAnnotationsEnumChange::Unchanged,
                        ..
                    }
                ) {
                    self.feedback = Some("The variation is already saved.".to_owned());
                }
                Ok(())
            }
            DesktopLibraryServiceEnumResult::PendingSnapshot { snapshot, pending } => {
                PresentationAnalysisEditorServiceCompletion::check_snapshot(
                    expected,
                    snapshot.context(),
                )?;
                if self.context() != Some(expected) && self.context() != Some(snapshot.context()) {
                    return Err(PresentationAnalysisEditorError::StalePage);
                }
                self.pending
                    .as_mut()
                    .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?
                    .set_service_pending(Some(*pending));
                self.source_pending = Some(*pending);
                self.feedback = Some("Finishing the change. Please wait.".to_owned());
                Ok(())
            }
            DesktopLibraryServiceEnumResult::Query(_)
            | DesktopLibraryServiceEnumResult::Metadata(_)
            | DesktopLibraryServiceEnumResult::Organized(_)
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
        let pending = self
            .pending
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        let disposition = pending.observe_service_error(application, expected, error)?;
        if matches!(disposition, DesktopLibraryServiceEnumDisposition::Refused) {
            if pending
                .service_pending()
                .is_some_and(|pending| self.source_pending == Some(pending))
            {
                self.source_pending = None;
            }
            self.finish_pending();
        }
        self.feedback = Some(error.to_string());
        Ok(())
    }

    pub fn retry_pending(
        &mut self,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        let pending = self
            .pending
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        if !pending.has_failure() {
            return Ok(None);
        }
        if pending.service_failure().is_none()
            && self.publication.as_ref().is_none_or(|publication| {
                !PresentationAnalysisEditorServiceCompletion::same_owner(
                    pending.intent().expected(),
                    publication.context(),
                )
            })
        {
            return Err(PresentationAnalysisEditorError::Conflict);
        }
        let intent = if pending.service_failure().is_some() {
            if !self.history.begin(request_id)? {
                return Ok(None);
            }
            if let Some(previous) = pending.retry_request_id() {
                self.history.complete(previous);
            }
            pending.retry_service(request_id)
        } else {
            pending.intent().clone()
        };
        pending.recover();
        self.feedback = Some("Retrying the change.".to_owned());
        Ok(Some(intent))
    }
}
