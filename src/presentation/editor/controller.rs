mod admission;
mod board_input;

use thiserror::Error;

use crate::board::editor::{
    BoardEditorDraftFields, BoardEditorSessionEnumOperation, BoardEditorSessionEnumSelection,
    BoardEditorValidationEnumStatus,
};
use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::board::square::BoardSquare;
use crate::desktop::application::{DesktopApplicationError, DesktopApplicationReceipt};
use crate::desktop::editor_commands::{
    DesktopEditorCommandsContext, DesktopEditorCommandsEnumOperation,
    DesktopEditorCommandsEnumWriteStatus, DesktopEditorCommandsSnapshot,
};
use crate::desktop::messages::DesktopRequest;
use crate::presentation::editor::controller::admission::PresentationEditorControllerPending;

pub use admission::PresentationEditorControllerEnumEffect;
pub use board_input::{
    PresentationEditorControllerDrag, PresentationEditorControllerEnumKey,
    PresentationEditorControllerPalette,
};

pub trait PresentationEditorCommandPort {
    fn prepare(
        &self,
        operation: DesktopEditorCommandsEnumOperation,
    ) -> Result<DesktopRequest, DesktopApplicationError>;
    fn try_submit(
        &self,
        request: &DesktopRequest,
    ) -> Result<DesktopApplicationReceipt, DesktopApplicationError>;
}

#[derive(Debug, Error)]
pub enum PresentationEditorControllerError {
    #[error("Wait for the current editor action or explicitly retry its retained request")]
    Busy,
    #[error("Open an editor draft before using its controls")]
    NoDraft,
    #[error("Resolve the unsaved editor prompt before using this control")]
    Prompt,
    #[error("The original editor write is unresolved")]
    WriteUnresolved,
    #[error("This result does not match the accepted application receipt")]
    StaleResult,
    #[error("This action requires an occupied source square")]
    EmptySquare,
    #[error("Focus a square on the editor board first")]
    NoFocus,
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
}

pub struct PresentationEditorControllerProjection<'a> {
    snapshot: &'a DesktopEditorCommandsSnapshot,
    focused: Option<BoardSquare>,
}

impl PresentationEditorControllerProjection<'_> {
    pub const fn context(&self) -> DesktopEditorCommandsContext {
        self.snapshot.context()
    }
    pub const fn fields(&self) -> &BoardEditorDraftFields {
        self.snapshot.draft().fields()
    }
    pub const fn selection(&self) -> BoardEditorSessionEnumSelection {
        self.snapshot.selection()
    }
    pub const fn focused(&self) -> Option<BoardSquare> {
        self.focused
    }
    pub fn can_play(&self) -> bool {
        self.snapshot.status() == BoardEditorValidationEnumStatus::Valid
    }
}

pub struct PresentationEditorController {
    snapshot: Option<DesktopEditorCommandsSnapshot>,
    pending: Option<PresentationEditorControllerPending>,
    focused: Option<BoardSquare>,
    placement_color: BoardEnumColor,
    placement_kind: BoardEnumPieceKind,
}

impl PresentationEditorController {
    pub fn new(snapshot: Option<DesktopEditorCommandsSnapshot>) -> Self {
        let mut controller = Self {
            snapshot: None,
            pending: None,
            focused: None,
            placement_color: BoardEnumColor::White,
            placement_kind: BoardEnumPieceKind::Pawn,
        };
        controller.install_snapshot(snapshot);
        controller
    }

    pub const fn snapshot(&self) -> Option<&DesktopEditorCommandsSnapshot> {
        self.snapshot.as_ref()
    }
    pub fn projection(&self) -> Option<PresentationEditorControllerProjection<'_>> {
        Some(PresentationEditorControllerProjection {
            snapshot: self.snapshot.as_ref()?,
            focused: self.focused,
        })
    }
    pub const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub const fn focused_square(&self) -> Option<BoardSquare> {
        self.focused
    }
    pub fn receipt(&self) -> Option<&DesktopApplicationReceipt> {
        self.pending.as_ref()?.receipt()
    }
    pub fn pending_request(&self) -> Option<&DesktopRequest> {
        self.pending
            .as_ref()
            .map(PresentationEditorControllerPending::request)
    }

    pub fn can_edit(&self) -> bool {
        !self.is_pending()
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.prompt().is_none()
                    && matches!(
                        snapshot.write(),
                        DesktopEditorCommandsEnumWriteStatus::Idle
                            | DesktopEditorCommandsEnumWriteStatus::Saving { .. }
                    )
            })
    }

    pub fn replace_snapshot(
        &mut self,
        snapshot: Option<DesktopEditorCommandsSnapshot>,
    ) -> Result<(), PresentationEditorControllerError> {
        if self.pending.is_some() {
            return Err(PresentationEditorControllerError::Busy);
        }
        if let (Some(previous), Some(next)) = (&self.snapshot, &snapshot)
            && (previous.context().session() != next.context().session()
                || previous.context().revision() > next.context().revision())
        {
            return Err(PresentationEditorControllerError::StaleResult);
        }
        self.install_snapshot(snapshot);
        Ok(())
    }

    fn install_snapshot(&mut self, snapshot: Option<DesktopEditorCommandsSnapshot>) {
        if self
            .snapshot
            .as_ref()
            .map(|current| current.context().session())
            != snapshot.as_ref().map(|next| next.context().session())
        {
            self.focused = None;
        }
        if let Some(BoardEditorSessionEnumSelection::Palette { color, kind }) = snapshot
            .as_ref()
            .map(DesktopEditorCommandsSnapshot::selection)
        {
            self.placement_color = color;
            self.placement_kind = kind;
        }
        self.snapshot = snapshot;
    }

    fn apply_operation(
        &self,
        operation: BoardEditorSessionEnumOperation,
    ) -> Result<DesktopEditorCommandsEnumOperation, PresentationEditorControllerError> {
        Ok(DesktopEditorCommandsEnumOperation::Apply {
            expected: self.editable_snapshot()?.context(),
            operation,
        })
    }

    fn editable_snapshot(
        &self,
    ) -> Result<&DesktopEditorCommandsSnapshot, PresentationEditorControllerError> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or(PresentationEditorControllerError::NoDraft)?;
        if !self.can_edit() {
            return Err(if self.is_pending() {
                PresentationEditorControllerError::Busy
            } else if snapshot.prompt().is_some() {
                PresentationEditorControllerError::Prompt
            } else {
                PresentationEditorControllerError::WriteUnresolved
            });
        }
        Ok(snapshot)
    }
}
