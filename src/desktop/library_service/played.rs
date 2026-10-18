use std::sync::Arc;

use crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumPending;
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::game_editor::DesktopGameEditorContext;
use crate::desktop::game_supervisor::{
    DesktopGameSupervisorEditorReceipt, DesktopGameSupervisorError,
};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::port::DesktopLibraryServiceReply;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceError,
};
use crate::game::enums::GameId;

pub(super) struct DesktopLibraryServicePlayed {
    pub(super) completion: Arc<DesktopLibraryServiceReply>,
    pub(super) receipt: DesktopGameSupervisorEditorReceipt,
    retries: Vec<Arc<DesktopLibraryServiceReply>>,
}

impl DesktopLibraryServicePlayed {
    pub(super) fn new(
        completion: Arc<DesktopLibraryServiceReply>,
        receipt: DesktopGameSupervisorEditorReceipt,
    ) -> Self {
        Self {
            completion,
            receipt,
            retries: Vec::new(),
        }
    }

    pub(super) fn request_id(&self) -> DesktopRequestId {
        self.receipt.request_id()
    }

    pub(super) fn game(&self) -> Option<GameId> {
        self.completion.operation.game()
    }

    pub(super) fn has_pending(&self) -> bool {
        self.receipt.pending().is_some()
    }

    pub(super) fn has_application(&self, application: &DesktopApplicationReceipt) -> bool {
        self.completion.application.shares_outcome(application)
            || self
                .retries
                .iter()
                .any(|reply| reply.application.shares_outcome(application))
    }

    pub(super) fn retain_retry(
        &mut self,
        reply: Arc<DesktopLibraryServiceReply>,
    ) -> Result<(), DesktopLibraryServiceError> {
        if !self.has_application(&reply.application) {
            if self.retries.len()
                >= crate::desktop::library_service::DesktopLibraryServicePort::CAPACITY - 1
            {
                return Err(DesktopLibraryServiceError::Busy.continuation(&self.completion));
            }
            self.retries.push(reply);
        }
        Ok(())
    }

    pub(super) fn complete(
        &self,
        result: crate::desktop::library_service::port::DesktopLibraryServiceResponse,
    ) {
        let retryable = result.is_err() && self.has_pending();
        self.completion.complete(result.clone(), retryable);
        for reply in &self.retries {
            reply.complete(result.clone(), retryable);
        }
    }

    pub(super) fn requires_continuation(&self) -> bool {
        self.has_pending() || self.receipt.try_result().is_some_and(|result| {
            matches!(result, Err(error) if !matches!(error.as_ref(),
                DesktopGameSupervisorError::EditorCancelled
                | DesktopGameSupervisorError::Editor(crate::desktop::game_editor::DesktopGameEditorError::StaleContext)
                | DesktopGameSupervisorError::EditorNotOpen
                | DesktopGameSupervisorError::EditorReceipt
                | DesktopGameSupervisorError::EditorVariationToken
                | DesktopGameSupervisorError::EditorScopeOffset))
        })
    }

    pub(super) fn check_retry(
        &self,
        expected: DesktopGameEditorContext,
    ) -> Result<(), DesktopLibraryServiceError> {
        self.completion
            .editor_context()
            .ok_or(DesktopLibraryServiceError::ForeignReceipt)?
            .check_expected(expected)?;
        Ok(())
    }

    pub(super) fn retain_publication_context(&self) -> Result<(), DesktopLibraryServiceError> {
        if matches!(
            self.completion.operation.action(),
            DesktopLibraryServiceEnumAction::Open { .. }
        ) && let Some(DesktopAnnotationCommandsEnumPending::Publishing { context }) =
            self.receipt.pending()
        {
            self.completion
                .publication_context
                .get_or_init(|| context)
                .check_expected(context)?;
        }
        Ok(())
    }
}
