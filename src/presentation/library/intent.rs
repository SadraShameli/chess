use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::game_editor::{DesktopGameEditorContext, DesktopGameEditorPublication};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::DesktopLibraryServiceEnumAction;
use crate::desktop::library_service::DesktopLibraryServiceOperation;
use crate::desktop::messages::{DesktopEnumCommand, DesktopRequest};
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::presentation::library::error::PresentationLibraryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationLibraryIntent {
    input: DesktopRequestId,
    operation: DesktopLibraryServiceOperation,
}

impl PresentationLibraryIntent {
    pub(crate) const fn new(
        input: DesktopRequestId,
        operation: DesktopLibraryServiceOperation,
    ) -> Self {
        Self { input, operation }
    }

    pub const fn input_id(&self) -> DesktopRequestId {
        self.input
    }

    pub const fn operation(&self) -> &DesktopLibraryServiceOperation {
        &self.operation
    }

    pub(crate) fn is_open_action(&self) -> bool {
        matches!(self.operation.action(), DesktopLibraryServiceEnumAction::Open { .. } | DesktopLibraryServiceEnumAction::Editor { action: crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumAction::RetrySave, .. })
    }
}

pub(crate) struct PresentationLibraryPending {
    pub(crate) intent: PresentationLibraryIntent,
    pub(crate) lane: PresentationLibraryEnumLane,
    request: Option<DesktopRequest>,
    receipt: Option<DesktopApplicationReceipt>,
    open_origin: Option<PresentationLibraryOpenOrigin>,
}

struct PresentationLibraryOpenOrigin {
    intent: PresentationLibraryIntent,
    request: DesktopRequest,
    receipt: DesktopApplicationReceipt,
    certified: DesktopGameEditorContext,
}

impl PresentationLibraryPending {
    pub(crate) const fn new(
        intent: PresentationLibraryIntent,
        lane: PresentationLibraryEnumLane,
    ) -> Self {
        Self {
            intent,
            lane,
            request: None,
            receipt: None,
            open_origin: None,
        }
    }

    pub(crate) fn attach_request(
        &mut self,
        request: DesktopRequest,
    ) -> Result<(), PresentationLibraryError> {
        if !matches!(request.command().operation(), DesktopEnumCommand::Library(operation) if operation == self.intent.operation())
            || self
                .request
                .as_ref()
                .is_some_and(|original| original.request_id() != request.request_id())
        {
            return Err(PresentationLibraryError::ForeignRequest);
        }
        self.request = Some(request);
        Ok(())
    }

    pub(crate) fn attach_receipt(
        &mut self,
        receipt: DesktopApplicationReceipt,
    ) -> Result<(), PresentationLibraryError> {
        let request = self
            .request
            .as_ref()
            .ok_or(PresentationLibraryError::MissingRequest)?;
        if request.request_id() != receipt.request_id()
            || self
                .receipt
                .as_ref()
                .is_some_and(|original| !original.shares_outcome(&receipt))
        {
            return Err(PresentationLibraryError::ForeignReceipt);
        }
        self.receipt = Some(receipt);
        Ok(())
    }

    pub(crate) fn check(
        &self,
        receipt: &DesktopApplicationReceipt,
    ) -> Result<(), PresentationLibraryError> {
        let attached = self
            .receipt
            .as_ref()
            .ok_or(PresentationLibraryError::MissingReceipt)?;
        if !attached.shares_outcome(receipt) {
            return Err(PresentationLibraryError::ForeignReceipt);
        }
        Ok(())
    }

    pub(crate) const fn request(&self) -> Option<&DesktopRequest> {
        self.request.as_ref()
    }

    pub(crate) const fn receipt(&self) -> Option<&DesktopApplicationReceipt> {
        self.receipt.as_ref()
    }

    pub(crate) fn retained_request(
        &self,
        intent: &PresentationLibraryIntent,
    ) -> Option<&DesktopRequest> {
        if &self.intent == intent {
            self.request()
        } else {
            self.open_origin
                .as_ref()
                .filter(|origin| &origin.intent == intent)
                .map(|origin| &origin.request)
        }
    }

    pub(crate) fn retained_receipt(
        &self,
        intent: &PresentationLibraryIntent,
    ) -> Option<&DesktopApplicationReceipt> {
        if &self.intent == intent {
            self.receipt()
        } else {
            self.open_origin
                .as_ref()
                .filter(|origin| &origin.intent == intent)
                .map(|origin| &origin.receipt)
        }
    }

    pub(crate) fn original_input(&self) -> Option<DesktopRequestId> {
        self.open_origin
            .as_ref()
            .map(|origin| origin.intent.input_id())
    }

    pub(crate) fn certified_open(&self) -> Option<DesktopGameEditorContext> {
        self.open_origin.as_ref().map(|origin| origin.certified)
    }

    pub(crate) fn observe_retained(
        &mut self,
        request: DesktopRequestId,
        expected: Option<DesktopGameEditorContext>,
    ) -> Result<(), PresentationLibraryError> {
        if let Some(origin) = &self.open_origin {
            if request != origin.receipt.request_id() || expected != Some(origin.certified) {
                return Err(PresentationLibraryError::ForeignContinuation);
            }
            return Ok(());
        }
        let receipt = self
            .receipt
            .as_ref()
            .ok_or(PresentationLibraryError::MissingReceipt)?;
        if request != receipt.request_id() {
            return Err(PresentationLibraryError::ForeignContinuation);
        }
        if let Some(certified) = expected {
            let DesktopLibraryServiceEnumAction::Open { game, .. } =
                self.intent.operation().action()
            else {
                return Err(PresentationLibraryError::ForeignContinuation);
            };
            if certified.game() != *game {
                return Err(PresentationLibraryError::ForeignContinuation);
            }
            self.open_origin = Some(PresentationLibraryOpenOrigin {
                intent: self.intent.clone(),
                request: self
                    .request
                    .as_ref()
                    .ok_or(PresentationLibraryError::MissingRequest)?
                    .clone(),
                receipt: receipt.clone(),
                certified,
            });
        }
        Ok(())
    }

    pub(crate) fn begin_open_retry(
        &mut self,
        intent: PresentationLibraryIntent,
    ) -> Result<(), PresentationLibraryError> {
        let certified = self
            .certified_open()
            .ok_or(PresentationLibraryError::ForeignContinuation)?;
        if !matches!(intent.operation().action(), DesktopLibraryServiceEnumAction::Editor { expected, action: crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumAction::RetrySave } if *expected == certified)
        {
            return Err(PresentationLibraryError::ForeignContinuation);
        }
        self.intent = intent;
        self.request = None;
        self.receipt = None;
        Ok(())
    }

    pub(crate) fn check_opened(
        &self,
        publication: &DesktopGameEditorPublication,
    ) -> Result<(), PresentationLibraryError> {
        if let Some(origin) = &self.open_origin {
            crate::presentation::analysis::editor_intent::PresentationAnalysisEditorServiceCompletion::check_snapshot(origin.certified, publication.context()).map_err(|_error| PresentationLibraryError::WrongResult)?;
            let DesktopLibraryServiceEnumAction::Open { game, .. } =
                origin.intent.operation().action()
            else {
                return Err(PresentationLibraryError::WrongResult);
            };
            if publication.context().game() != *game {
                return Err(PresentationLibraryError::WrongResult);
            }
            Ok(())
        } else if matches!(self.intent.operation().action(), DesktopLibraryServiceEnumAction::Open { game, .. } if *game == publication.context().game())
        {
            Ok(())
        } else {
            Err(PresentationLibraryError::WrongResult)
        }
    }
}
