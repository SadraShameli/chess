use crate::board::editor::{
    BoardEditorSessionEnumEdit, BoardEditorSessionEnumOperation, BoardEditorSessionEnumSelection,
};
use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::board::square::BoardSquare;
use crate::desktop::editor_commands::{
    DesktopEditorCommandsContext, DesktopEditorCommandsEnumOperation,
};
use crate::presentation::board::enums::PresentationBoardEnumDirection;
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::editor::controller::{
    PresentationEditorController, PresentationEditorControllerError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationEditorControllerEnumKey {
    Piece(BoardEnumPieceKind),
    ToggleColor,
    Place,
    Remove,
    Undo,
    Redo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationEditorControllerPalette {
    context: DesktopEditorCommandsContext,
    color: BoardEnumColor,
    kind: BoardEnumPieceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationEditorControllerDrag {
    context: DesktopEditorCommandsContext,
    origin: BoardSquare,
}

impl PresentationEditorControllerDrag {
    pub const fn context(self) -> DesktopEditorCommandsContext {
        self.context
    }
    pub const fn origin(self) -> BoardSquare {
        self.origin
    }
}

impl PresentationEditorControllerPalette {
    pub const fn new(
        context: DesktopEditorCommandsContext,
        color: BoardEnumColor,
        kind: BoardEnumPieceKind,
    ) -> Self {
        Self {
            context,
            color,
            kind,
        }
    }
    pub const fn context(self) -> DesktopEditorCommandsContext {
        self.context
    }
    pub const fn color(self) -> BoardEnumColor {
        self.color
    }
    pub const fn kind(self) -> BoardEnumPieceKind {
        self.kind
    }
}

impl PresentationEditorController {
    pub fn focus_square(&mut self, square: BoardSquare) {
        self.focused = Some(square);
    }

    pub fn focus_direction(
        &mut self,
        direction: PresentationBoardEnumDirection,
        scene: &PresentationBoardScene,
    ) -> Result<(), PresentationEditorControllerError> {
        let square = self
            .focused
            .ok_or(PresentationEditorControllerError::NoFocus)?;
        if let Some(next) = PresentationBoardSemanticGrid::neighbor(square, direction, scene) {
            self.focused = Some(next);
        }
        Ok(())
    }

    pub fn select_palette(
        &self,
        color: BoardEnumColor,
        kind: BoardEnumPieceKind,
    ) -> Result<DesktopEditorCommandsEnumOperation, PresentationEditorControllerError> {
        self.apply_operation(BoardEditorSessionEnumOperation::Select(
            BoardEditorSessionEnumSelection::Palette { color, kind },
        ))
    }

    pub fn click_square(
        &mut self,
        square: BoardSquare,
    ) -> Result<DesktopEditorCommandsEnumOperation, PresentationEditorControllerError> {
        self.focused = Some(square);
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or(PresentationEditorControllerError::NoDraft)?;
        let operation = match snapshot.selection() {
            BoardEditorSessionEnumSelection::None => {
                if snapshot.draft().fields().piece_at(square).is_none() {
                    return Err(PresentationEditorControllerError::EmptySquare);
                }
                BoardEditorSessionEnumOperation::Select(BoardEditorSessionEnumSelection::Square(
                    square,
                ))
            }
            BoardEditorSessionEnumSelection::Palette { .. }
            | BoardEditorSessionEnumSelection::Square(_) => {
                BoardEditorSessionEnumOperation::PlaceSelected(square)
            }
        };
        self.apply_operation(operation)
    }

    pub fn begin_drag(
        &self,
        origin: BoardSquare,
    ) -> Result<PresentationEditorControllerDrag, PresentationEditorControllerError> {
        let snapshot = self.editable_snapshot()?;
        if snapshot.draft().fields().piece_at(origin).is_none() {
            return Err(PresentationEditorControllerError::EmptySquare);
        }
        Ok(PresentationEditorControllerDrag {
            context: snapshot.context(),
            origin,
        })
    }

    pub fn drag_square(
        &self,
        drag: PresentationEditorControllerDrag,
        to: BoardSquare,
    ) -> Result<DesktopEditorCommandsEnumOperation, PresentationEditorControllerError> {
        if self.editable_snapshot()?.context() != drag.context {
            return Err(PresentationEditorControllerError::StaleResult);
        }
        self.apply_operation(BoardEditorSessionEnumOperation::Edit(
            BoardEditorSessionEnumEdit::Move {
                from: drag.origin,
                to,
            },
        ))
    }

    pub fn drop_palette(
        &self,
        color: BoardEnumColor,
        kind: BoardEnumPieceKind,
        square: BoardSquare,
    ) -> Result<DesktopEditorCommandsEnumOperation, PresentationEditorControllerError> {
        self.apply_operation(BoardEditorSessionEnumOperation::Edit(
            BoardEditorSessionEnumEdit::Place {
                square,
                color,
                kind,
            },
        ))
    }

    pub fn drop_palette_payload(
        &self,
        palette: PresentationEditorControllerPalette,
        square: BoardSquare,
    ) -> Result<DesktopEditorCommandsEnumOperation, PresentationEditorControllerError> {
        if self
            .snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.context() != palette.context)
        {
            return Err(PresentationEditorControllerError::StaleResult);
        }
        self.drop_palette(palette.color, palette.kind, square)
    }

    pub fn remove_square(
        &self,
        square: BoardSquare,
    ) -> Result<DesktopEditorCommandsEnumOperation, PresentationEditorControllerError> {
        self.apply_operation(BoardEditorSessionEnumOperation::Edit(
            BoardEditorSessionEnumEdit::Remove { square },
        ))
    }

    pub fn key(
        &mut self,
        key: PresentationEditorControllerEnumKey,
    ) -> Result<DesktopEditorCommandsEnumOperation, PresentationEditorControllerError> {
        match key {
            PresentationEditorControllerEnumKey::Piece(kind) => {
                self.select_palette(self.placement_color, kind)
            }
            PresentationEditorControllerEnumKey::ToggleColor => {
                self.select_palette(self.placement_color.opposite(), self.placement_kind)
            }
            PresentationEditorControllerEnumKey::Place => {
                self.apply_operation(BoardEditorSessionEnumOperation::PlaceSelected(
                    self.focused
                        .ok_or(PresentationEditorControllerError::NoFocus)?,
                ))
            }
            PresentationEditorControllerEnumKey::Remove => self.remove_square(
                self.focused
                    .ok_or(PresentationEditorControllerError::NoFocus)?,
            ),
            PresentationEditorControllerEnumKey::Undo => {
                self.apply_operation(BoardEditorSessionEnumOperation::Undo)
            }
            PresentationEditorControllerEnumKey::Redo => {
                self.apply_operation(BoardEditorSessionEnumOperation::Redo)
            }
        }
    }
}
