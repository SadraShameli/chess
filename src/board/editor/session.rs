use thiserror::Error;

use crate::board::editor::draft::{
    BoardEditorDraft, BoardEditorDraftError, BoardEditorDraftFields, BoardEditorDraftId,
    BoardEditorDraftRevision,
};
use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use crate::board::position::BoardPosition;
use crate::board::square::BoardSquare;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardEditorSessionEnumSelection {
    None,
    Palette {
        color: BoardEnumColor,
        kind: BoardEnumPieceKind,
    },
    Square(BoardSquare),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardEditorSessionEnumEdit {
    Place {
        square: BoardSquare,
        color: BoardEnumColor,
        kind: BoardEnumPieceKind,
    },
    Remove {
        square: BoardSquare,
    },
    Move {
        from: BoardSquare,
        to: BoardSquare,
    },
    Variant(BoardEnumVariant),
    Turn(BoardEnumColor),
    GrantCastling(BoardSquare),
    RevokeCastling(BoardSquare),
    EnPassant(Option<BoardSquare>),
    Counters {
        halfmoves: i64,
        fullmoves: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardEditorSessionEnumSetup {
    ClearBoard,
    StandardSetup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardEditorSessionEnumConfirmation {
    Required,
    Confirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardEditorSessionEnumOperation {
    Select(BoardEditorSessionEnumSelection),
    PlaceSelected(BoardSquare),
    Edit(BoardEditorSessionEnumEdit),
    Replace {
        setup: BoardEditorSessionEnumSetup,
        confirmation: BoardEditorSessionEnumConfirmation,
    },
    Undo,
    Redo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardEditorSessionRequest {
    pub draft: BoardEditorDraftId,
    pub expected_revision: BoardEditorDraftRevision,
    pub operation: BoardEditorSessionEnumOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardEditorSessionEnumOutcome {
    Unchanged,
    SelectionChanged,
    PositionChanged,
}

#[derive(Debug, Error)]
pub enum BoardEditorSessionError {
    #[error(transparent)]
    Draft(#[from] BoardEditorDraftError),
    #[error("The editor request belongs to draft {requested}, current draft is {actual}")]
    WrongDraft {
        requested: BoardEditorDraftId,
        actual: BoardEditorDraftId,
    },
    #[error("Select a palette piece or an occupied square before placing")]
    NoSelection,
    #[error("There is no piece at {square} to select or move")]
    EmptySquare { square: BoardSquare },
    #[error("Confirm {setup:?} before replacing the draft position")]
    ConfirmationRequired { setup: BoardEditorSessionEnumSetup },
    #[error("There is no editor edit to undo in this session")]
    NoUndo,
    #[error("There is no editor edit to redo in this session")]
    NoRedo,
}

#[derive(Clone, Copy)]
enum BoardEditorSessionEnumHistory {
    Undo,
    Redo,
}

impl BoardEditorSessionEnumHistory {
    fn empty_error(self) -> BoardEditorSessionError {
        match self {
            Self::Undo => BoardEditorSessionError::NoUndo,
            Self::Redo => BoardEditorSessionError::NoRedo,
        }
    }
}

#[derive(Debug)]
pub struct BoardEditorSession {
    draft: BoardEditorDraft,
    selection: BoardEditorSessionEnumSelection,
    undo: VecDeque<BoardEditorDraftFields>,
    redo: VecDeque<BoardEditorDraftFields>,
}

impl BoardEditorSession {
    pub const HISTORY_LIMIT: usize = 256;

    pub fn new(id: BoardEditorDraftId, selected: Option<&BoardPosition>) -> Self {
        Self::from_draft(BoardEditorDraft::new(id, selected))
    }

    pub const fn from_draft(draft: BoardEditorDraft) -> Self {
        Self {
            draft,
            selection: BoardEditorSessionEnumSelection::None,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
        }
    }

    pub const fn draft(&self) -> &BoardEditorDraft {
        &self.draft
    }

    pub const fn selection(&self) -> BoardEditorSessionEnumSelection {
        self.selection
    }

    pub fn has_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn has_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn into_draft(self) -> BoardEditorDraft {
        self.draft
    }

    pub fn apply(
        &mut self,
        request: BoardEditorSessionRequest,
    ) -> Result<BoardEditorSessionEnumOutcome, BoardEditorSessionError> {
        if request.draft != self.draft.id() {
            return Err(BoardEditorSessionError::WrongDraft {
                requested: request.draft,
                actual: self.draft.id(),
            });
        }
        self.draft
            .revision()
            .check_expected(request.expected_revision)?;
        match request.operation {
            BoardEditorSessionEnumOperation::Select(selection) => self.select(selection),
            BoardEditorSessionEnumOperation::PlaceSelected(square) => {
                self.edit(match self.selection {
                    BoardEditorSessionEnumSelection::None => {
                        return Err(BoardEditorSessionError::NoSelection);
                    }
                    BoardEditorSessionEnumSelection::Palette { color, kind } => {
                        BoardEditorSessionEnumEdit::Place {
                            square,
                            color,
                            kind,
                        }
                    }
                    BoardEditorSessionEnumSelection::Square(from) => {
                        BoardEditorSessionEnumEdit::Move { from, to: square }
                    }
                })
            }
            BoardEditorSessionEnumOperation::Edit(edit) => self.edit(edit),
            BoardEditorSessionEnumOperation::Replace {
                setup,
                confirmation,
            } => self.replace(setup, confirmation),
            BoardEditorSessionEnumOperation::Undo => {
                self.restore(BoardEditorSessionEnumHistory::Undo)
            }
            BoardEditorSessionEnumOperation::Redo => {
                self.restore(BoardEditorSessionEnumHistory::Redo)
            }
        }
    }

    fn select(
        &mut self,
        selection: BoardEditorSessionEnumSelection,
    ) -> Result<BoardEditorSessionEnumOutcome, BoardEditorSessionError> {
        if let BoardEditorSessionEnumSelection::Square(square) = selection {
            self.draft
                .fields()
                .piece_at(square)
                .ok_or(BoardEditorSessionError::EmptySquare { square })?;
        }
        if self.selection == selection {
            return Ok(BoardEditorSessionEnumOutcome::Unchanged);
        }
        self.selection = selection;
        Ok(BoardEditorSessionEnumOutcome::SelectionChanged)
    }

    fn edit(
        &mut self,
        edit: BoardEditorSessionEnumEdit,
    ) -> Result<BoardEditorSessionEnumOutcome, BoardEditorSessionError> {
        let mut fields = self.draft.fields().clone();
        fields.apply_edit(edit)?;
        self.accept(fields)
    }

    fn replace(
        &mut self,
        setup: BoardEditorSessionEnumSetup,
        confirmation: BoardEditorSessionEnumConfirmation,
    ) -> Result<BoardEditorSessionEnumOutcome, BoardEditorSessionError> {
        match confirmation {
            BoardEditorSessionEnumConfirmation::Required => {
                return Err(BoardEditorSessionError::ConfirmationRequired { setup });
            }
            BoardEditorSessionEnumConfirmation::Confirmed => {}
        }
        let fields = match setup {
            BoardEditorSessionEnumSetup::ClearBoard => {
                BoardEditorDraftFields::empty(self.draft.fields().variant())
            }
            BoardEditorSessionEnumSetup::StandardSetup => {
                BoardEditorDraftFields::from_position(&BoardPosition::standard())
            }
        };
        self.accept(fields)
    }

    fn accept(
        &mut self,
        fields: BoardEditorDraftFields,
    ) -> Result<BoardEditorSessionEnumOutcome, BoardEditorSessionError> {
        if &fields == self.draft.fields() {
            return Ok(BoardEditorSessionEnumOutcome::Unchanged);
        }
        let revision = self.draft.revision().next()?;
        if self.undo.len() == Self::HISTORY_LIMIT {
            self.undo.pop_front();
        }
        self.undo
            .push_back(self.draft.replace_fields(fields, revision));
        self.redo.clear();
        self.selection = BoardEditorSessionEnumSelection::None;
        Ok(BoardEditorSessionEnumOutcome::PositionChanged)
    }

    fn restore(
        &mut self,
        history: BoardEditorSessionEnumHistory,
    ) -> Result<BoardEditorSessionEnumOutcome, BoardEditorSessionError> {
        let (source, target) = match history {
            BoardEditorSessionEnumHistory::Undo => (&mut self.undo, &mut self.redo),
            BoardEditorSessionEnumHistory::Redo => (&mut self.redo, &mut self.undo),
        };
        if source.is_empty() {
            return Err(history.empty_error());
        }
        let revision = self.draft.revision().next()?;
        let fields = source.pop_back().ok_or_else(|| history.empty_error())?;
        target.push_back(self.draft.replace_fields(fields, revision));
        self.selection = BoardEditorSessionEnumSelection::None;
        Ok(BoardEditorSessionEnumOutcome::PositionChanged)
    }
}
use std::collections::VecDeque;
