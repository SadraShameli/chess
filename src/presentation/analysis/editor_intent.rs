mod service;

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use bevy_egui::egui::{self, Id, Ui};
use thiserror::Error;

use crate::desktop::annotation_commands::{
    DesktopAnnotationCommands, DesktopAnnotationCommandsEnumAction,
    DesktopAnnotationCommandsEnumPending,
};
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::error::DesktopFailure;
use crate::desktop::game_editor::DesktopGameEditorContext;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumDisposition, DesktopLibraryServiceError,
};
pub(crate) use crate::presentation::analysis::editor_intent::service::PresentationAnalysisEditorServiceCompletion;

#[derive(Debug, Error)]
pub enum PresentationAnalysisEditorError {
    #[error("Open a game before editing")]
    NoSelection,
    #[error("Wait for the game to finish updating")]
    SourcePending,
    #[error("The game changed. Keep your draft or discard it to use the current position")]
    Conflict,
    #[error("Wait for the current action to finish")]
    Busy,
    #[error("There are no changes to save")]
    NoChanges,
    #[error("This draft has reached its edit limit. Keep a copy of your notes before reopening it")]
    DraftVersionExhausted,
    #[error("This reply belongs to another action")]
    UnexpectedReply,
    #[error("This acknowledgment belongs to another action")]
    ForeignReceipt,
    #[error("The action has not been accepted yet")]
    MissingReceipt,
    #[error("The reply does not contain the requested information")]
    WrongResult,
    #[error("This page belongs to an earlier position")]
    StalePage,
    #[error("Preview the variation again before confirming")]
    MissingPreview,
    #[error("There are no more items on this page")]
    EndOfPages,
    #[error("Reopen the panel before submitting more actions")]
    InputCapacity,
    #[error("The save has not finished for this draft")]
    NotSaved,
    #[error("Choose a valid square for the drawing")]
    InvalidSquare,
    #[error("An arrow must connect two different squares")]
    EmptyArrow,
}

#[derive(Debug, Clone)]
pub struct PresentationAnalysisEditorIntent {
    request_id: DesktopRequestId,
    expected: DesktopGameEditorContext,
    action: DesktopAnnotationCommandsEnumAction,
}

impl PresentationAnalysisEditorIntent {
    pub const fn new(
        request_id: DesktopRequestId,
        expected: DesktopGameEditorContext,
        action: DesktopAnnotationCommandsEnumAction,
    ) -> Self {
        Self {
            request_id,
            expected,
            action,
        }
    }

    pub const fn request_id(&self) -> DesktopRequestId {
        self.request_id
    }

    pub const fn expected(&self) -> DesktopGameEditorContext {
        self.expected
    }

    pub const fn action(&self) -> &DesktopAnnotationCommandsEnumAction {
        &self.action
    }

    pub fn into_action(self) -> DesktopAnnotationCommandsEnumAction {
        self.action
    }

    pub fn admit(
        self,
        receipt: DesktopApplicationReceipt,
    ) -> Result<DesktopAnnotationCommands, PresentationAnalysisEditorError> {
        if self.request_id != receipt.request_id() {
            return Err(PresentationAnalysisEditorError::ForeignReceipt);
        }
        Ok(DesktopAnnotationCommands::new(
            receipt,
            self.expected,
            self.action,
        ))
    }
}

struct PresentationAnalysisEditorRequest {
    intent: PresentationAnalysisEditorIntent,
    receipt: Option<DesktopApplicationReceipt>,
}

impl PresentationAnalysisEditorRequest {
    const fn new(intent: PresentationAnalysisEditorIntent) -> Self {
        Self {
            intent,
            receipt: None,
        }
    }

    fn attach(
        &mut self,
        intent: &PresentationAnalysisEditorIntent,
        receipt: DesktopApplicationReceipt,
    ) -> Result<(), PresentationAnalysisEditorError> {
        if intent.request_id() != self.intent.request_id()
            || intent.expected() != self.intent.expected()
        {
            return Err(PresentationAnalysisEditorError::UnexpectedReply);
        }
        if receipt.request_id() != self.intent.request_id()
            || self
                .receipt
                .as_ref()
                .is_some_and(|attached| !attached.shares_outcome(&receipt))
        {
            return Err(PresentationAnalysisEditorError::ForeignReceipt);
        }
        self.receipt = Some(receipt);
        Ok(())
    }

    fn check(
        &self,
        receipt: &DesktopApplicationReceipt,
        expected: DesktopGameEditorContext,
    ) -> Result<(), PresentationAnalysisEditorError> {
        if expected != self.intent.expected() {
            return Err(PresentationAnalysisEditorError::UnexpectedReply);
        }
        let attached = self
            .receipt
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::MissingReceipt)?;
        if receipt.request_id() != self.intent.request_id() || !attached.shares_outcome(receipt) {
            return Err(PresentationAnalysisEditorError::ForeignReceipt);
        }
        Ok(())
    }
}

pub(crate) struct PresentationAnalysisEditorPending {
    request: PresentationAnalysisEditorRequest,
    retry: Option<PresentationAnalysisEditorRequest>,
    failure: Option<DesktopFailure>,
    service_failure: Option<Arc<DesktopLibraryServiceError>>,
    service_pending: Option<DesktopAnnotationCommandsEnumPending>,
}

impl PresentationAnalysisEditorPending {
    pub(crate) const fn new(intent: PresentationAnalysisEditorIntent) -> Self {
        Self {
            request: PresentationAnalysisEditorRequest::new(intent),
            retry: None,
            failure: None,
            service_failure: None,
            service_pending: None,
        }
    }

    pub(crate) const fn intent(&self) -> &PresentationAnalysisEditorIntent {
        &self.request.intent
    }

    pub(crate) fn active_intent(&self) -> &PresentationAnalysisEditorIntent {
        self.retry
            .as_ref()
            .map_or(&self.request.intent, |retry| &retry.intent)
    }

    pub(crate) fn retry_request_id(&self) -> Option<DesktopRequestId> {
        self.retry.as_ref().map(|retry| retry.intent.request_id())
    }

    pub(crate) fn retry_service(
        &mut self,
        request: DesktopRequestId,
    ) -> PresentationAnalysisEditorIntent {
        let intent = PresentationAnalysisEditorIntent::new(
            request,
            self.request.intent.expected(),
            DesktopAnnotationCommandsEnumAction::RetrySave,
        );
        self.retry = Some(PresentationAnalysisEditorRequest::new(intent.clone()));
        intent
    }

    pub(crate) fn attach(
        &mut self,
        intent: &PresentationAnalysisEditorIntent,
        receipt: DesktopApplicationReceipt,
    ) -> Result<(), PresentationAnalysisEditorError> {
        match self
            .retry
            .as_mut()
            .filter(|retry| retry.intent.request_id() == intent.request_id())
        {
            Some(retry) => retry.attach(intent, receipt),
            None => self.request.attach(intent, receipt),
        }
    }

    pub(crate) fn check(
        &self,
        receipt: &DesktopApplicationReceipt,
        expected: DesktopGameEditorContext,
    ) -> Result<(), PresentationAnalysisEditorError> {
        match self
            .retry
            .as_ref()
            .filter(|retry| retry.intent.request_id() == receipt.request_id())
        {
            Some(retry) => retry.check(receipt, expected),
            None => self.request.check(receipt, expected),
        }
    }

    pub(crate) fn fail(&mut self, failure: &DesktopFailure) {
        self.failure = Some(failure.clone());
        self.service_failure = None;
    }

    pub(crate) const fn failure(&self) -> Option<&DesktopFailure> {
        self.failure.as_ref()
    }

    pub(crate) fn recover(&mut self) {
        self.failure = None;
        self.service_failure = None;
    }

    pub(crate) fn has_failure(&self) -> bool {
        self.failure.is_some() || self.service_failure.is_some()
    }

    pub(crate) fn service_pending(&self) -> Option<DesktopAnnotationCommandsEnumPending> {
        self.service_pending
    }

    pub(crate) fn set_service_pending(
        &mut self,
        pending: Option<DesktopAnnotationCommandsEnumPending>,
    ) {
        self.service_pending = pending;
    }

    pub(crate) fn service_failure(&self) -> Option<&DesktopLibraryServiceError> {
        self.service_failure.as_deref()
    }

    pub(crate) fn observe_service_error(
        &mut self,
        receipt: &DesktopApplicationReceipt,
        expected: DesktopGameEditorContext,
        error: &Arc<DesktopLibraryServiceError>,
    ) -> Result<DesktopLibraryServiceEnumDisposition, PresentationAnalysisEditorError> {
        self.check(receipt, expected)?;
        let disposition = error.disposition();
        if let DesktopLibraryServiceEnumDisposition::Retained {
            request,
            expected: retained_expected,
        } = disposition
        {
            if request != self.request.intent.request_id()
                || retained_expected != Some(self.request.intent.expected())
            {
                return Err(PresentationAnalysisEditorError::UnexpectedReply);
            }
            self.failure = None;
            self.service_failure = Some(Arc::clone(error));
        }
        Ok(disposition)
    }
}

#[derive(Default)]
pub(crate) struct PresentationAnalysisEditorHistory {
    recent: VecDeque<DesktopRequestId>,
    pending: HashSet<DesktopRequestId>,
}

impl PresentationAnalysisEditorHistory {
    const CAPACITY: usize = 256;

    pub(crate) fn contains(&self, request: DesktopRequestId) -> bool {
        self.pending.contains(&request) || self.recent.contains(&request)
    }

    pub(crate) fn record(&mut self, request: DesktopRequestId) -> bool {
        if self.contains(request) {
            return false;
        }
        self.complete(request);
        true
    }

    pub(crate) fn begin(
        &mut self,
        request: DesktopRequestId,
    ) -> Result<bool, PresentationAnalysisEditorError> {
        if self.contains(request) {
            return Ok(false);
        }
        if self.pending.len() >= Self::CAPACITY {
            return Err(PresentationAnalysisEditorError::InputCapacity);
        }
        self.pending.insert(request);
        Ok(true)
    }

    pub(crate) fn complete(&mut self, request: DesktopRequestId) {
        self.pending.remove(&request);
        if self.recent.contains(&request) {
            return;
        }
        if self.recent.len() >= Self::CAPACITY {
            self.recent.pop_front();
        }
        self.recent.push_back(request);
    }
}

#[derive(Default)]
pub(crate) struct PresentationAnalysisEditorModal {
    invoker: Option<Id>,
    needs_focus: bool,
}

impl PresentationAnalysisEditorModal {
    pub(crate) fn open(&mut self, invoker: Option<Id>) {
        self.invoker = invoker;
        self.needs_focus = true;
    }

    pub(crate) fn refocus(&mut self) {
        self.needs_focus = true;
    }

    pub(crate) fn capture_invoker(&mut self, context: &egui::Context) {
        if self.needs_focus && self.invoker.is_none() {
            self.invoker = context.memory(|memory| memory.focused());
        }
    }

    pub(crate) fn label(ui: &Ui, label: &str) {
        ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
            node.set_role(egui::accesskit::Role::Dialog);
            node.set_label(label);
            node.set_modal();
        });
        if ui.memory(|memory| memory.is_above_modal_layer(ui.layer_id()))
            && ui.input(|input| input.key_pressed(egui::Key::Tab))
        {
            ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        }
    }

    pub(crate) fn contain(&mut self, ui: &Ui, focusable: &[Id]) {
        if ui.is_sizing_pass() || !ui.memory(|memory| memory.is_above_modal_layer(ui.layer_id())) {
            return;
        }
        let Some(first) = focusable.first().copied() else {
            return;
        };
        let focused = ui.memory(|memory| memory.focused());
        let index = focused.and_then(|focused| focusable.iter().position(|id| *id == focused));
        let offset = ui.ctx().input_mut(|input| {
            if input.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab) {
                Some(focusable.len() - 1)
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
                Some(1)
            } else {
                None
            }
        });
        let target = if self.needs_focus {
            first
        } else {
            match (index, offset) {
                (Some(index), Some(offset)) => focusable[(index + offset) % focusable.len()],
                (Some(index), None) => focusable[index],
                (None, Some(_) | None) => first,
            }
        };
        if focused != Some(target) {
            ui.memory_mut(|memory| memory.request_focus(target));
        }
        self.needs_focus = false;
    }

    pub(crate) fn close(&mut self, context: &egui::Context) {
        if let Some(invoker) = self.invoker.take() {
            context.memory_mut(|memory| memory.request_focus(invoker));
        }
        self.needs_focus = false;
    }
}

#[derive(Default)]
pub(crate) struct PresentationAnalysisEditorFrame {
    frame: Option<u64>,
    inputs: Vec<Id>,
}

impl PresentationAnalysisEditorFrame {
    pub(crate) fn clicked(&mut self, ui: &Ui, response: &egui::Response) -> bool {
        if ui.is_sizing_pass() || !response.clicked() {
            return false;
        }
        let frame = ui.ctx().cumulative_frame_nr();
        if self.frame != Some(frame) {
            self.frame = Some(frame);
            self.inputs.clear();
        }
        if self.inputs.contains(&response.id) {
            return false;
        }
        self.inputs.push(response.id);
        true
    }
}

#[cfg(test)]
#[path = "../../../tests/presentation/analysis/editor_intent.rs"]
pub(crate) mod tests;
