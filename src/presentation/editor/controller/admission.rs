use crate::board::start::BoardStart;
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::editor_commands::{
    DesktopEditorCommandsContext, DesktopEditorCommandsEnumOperation,
    DesktopEditorCommandsEnumResult, DesktopEditorCommandsPage,
};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::messages::{DesktopMessagesEnumResult, DesktopRequest};
use crate::presentation::editor::controller::{
    PresentationEditorCommandPort, PresentationEditorController, PresentationEditorControllerError,
};

pub(super) struct PresentationEditorControllerPending {
    request: DesktopRequest,
    receipt: Option<DesktopApplicationReceipt>,
}

impl PresentationEditorControllerPending {
    pub(super) const fn receipt(&self) -> Option<&DesktopApplicationReceipt> {
        self.receipt.as_ref()
    }
    pub(super) const fn request(&self) -> &DesktopRequest {
        &self.request
    }
}

#[derive(Debug, Clone)]
pub enum PresentationEditorControllerEnumEffect {
    None,
    NewGame {
        context: DesktopEditorCommandsContext,
        start: BoardStart,
    },
    CopyFen {
        context: DesktopEditorCommandsContext,
        fen: String,
    },
    List(DesktopEditorCommandsPage),
}

impl PresentationEditorController {
    pub fn submit<P: PresentationEditorCommandPort>(
        &mut self,
        operation: DesktopEditorCommandsEnumOperation,
        port: &P,
    ) -> Result<(), PresentationEditorControllerError> {
        if self.pending.is_some() {
            return Err(PresentationEditorControllerError::Busy);
        }
        let request = port.prepare(operation)?;
        self.pending = Some(PresentationEditorControllerPending {
            request,
            receipt: None,
        });
        self.retry(port)
    }

    pub fn retry<P: PresentationEditorCommandPort>(
        &mut self,
        port: &P,
    ) -> Result<(), PresentationEditorControllerError> {
        let pending = self
            .pending
            .as_mut()
            .ok_or(PresentationEditorControllerError::StaleResult)?;
        if pending.receipt.is_some() {
            return Err(PresentationEditorControllerError::Busy);
        }
        let receipt = port.try_submit(&pending.request)?;
        if receipt.request_id() != pending.request.request_id() {
            return Err(PresentationEditorControllerError::StaleResult);
        }
        pending.receipt = Some(receipt);
        Ok(())
    }

    pub fn accept(
        &mut self,
        request: DesktopRequestId,
    ) -> Result<PresentationEditorControllerEnumEffect, PresentationEditorControllerError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(PresentationEditorControllerError::StaleResult)?;
        let receipt = pending
            .receipt
            .as_ref()
            .ok_or(PresentationEditorControllerError::StaleResult)?;
        if pending.request.request_id() != request || receipt.request_id() != request {
            return Err(PresentationEditorControllerError::StaleResult);
        }
        let result = match receipt.response() {
            Some(DesktopMessagesEnumResult::Value {
                value: DesktopEnumValue::Editor(result),
                ..
            }) => result.as_ref().clone(),
            Some(
                DesktopMessagesEnumResult::Value { .. }
                | DesktopMessagesEnumResult::LibraryFailure(_)
                | DesktopMessagesEnumResult::EditorFailure(_)
                | DesktopMessagesEnumResult::Failure(_)
                | DesktopMessagesEnumResult::Cancelled
                | DesktopMessagesEnumResult::Stopped
                | DesktopMessagesEnumResult::Paused
                | DesktopMessagesEnumResult::ReconciliationRequired
                | DesktopMessagesEnumResult::QuitReady,
            )
            | None => return Err(PresentationEditorControllerError::StaleResult),
        };
        let effect = match result {
            DesktopEditorCommandsEnumResult::Snapshot(snapshot)
            | DesktopEditorCommandsEnumResult::Written { snapshot, .. } => {
                self.install_snapshot(snapshot);
                PresentationEditorControllerEnumEffect::None
            }
            DesktopEditorCommandsEnumResult::WritePending(_) => {
                return Err(PresentationEditorControllerError::Busy);
            }
            DesktopEditorCommandsEnumResult::Fen { context, fen } => {
                self.check_effect_context(context)?;
                PresentationEditorControllerEnumEffect::CopyFen { context, fen }
            }
            DesktopEditorCommandsEnumResult::Play { context, start } => {
                self.check_effect_context(context)?;
                PresentationEditorControllerEnumEffect::NewGame { context, start }
            }
            DesktopEditorCommandsEnumResult::List(page) => {
                PresentationEditorControllerEnumEffect::List(page)
            }
        };
        self.pending = None;
        Ok(effect)
    }

    pub fn acknowledge_failure(
        &mut self,
        request: DesktopRequestId,
    ) -> Result<(), PresentationEditorControllerError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(PresentationEditorControllerError::StaleResult)?;
        let receipt = pending
            .receipt
            .as_ref()
            .ok_or(PresentationEditorControllerError::StaleResult)?;
        if pending.request.request_id() != request
            || !matches!(
                receipt.response(),
                Some(
                    DesktopMessagesEnumResult::Failure(_)
                        | DesktopMessagesEnumResult::EditorFailure(_)
                        | DesktopMessagesEnumResult::Cancelled
                )
            )
        {
            return Err(PresentationEditorControllerError::StaleResult);
        }
        self.pending = None;
        Ok(())
    }

    fn check_effect_context(
        &self,
        context: DesktopEditorCommandsContext,
    ) -> Result<(), PresentationEditorControllerError> {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.context() == context)
        {
            Ok(())
        } else {
            Err(PresentationEditorControllerError::StaleResult)
        }
    }
}
