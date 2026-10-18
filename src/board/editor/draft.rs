use std::fmt::{Display, Formatter};
use std::num::NonZeroU32;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use shakmaty::{Bitboard, Board, EnPassantMode, Piece, Position, Role, Setup};
use thiserror::Error;
use uuid::Uuid;

use crate::board::editor::session::{BoardEditorSessionEnumEdit, BoardEditorSessionError};
use crate::board::editor::validation::{
    BoardEditorValidationEnumStatus, BoardEditorValidationError,
};
use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use crate::board::piece::BoardPiece;
use crate::board::position::{BoardPosition, BoardPositionError};
use crate::board::square::BoardSquare;
use crate::board::start::BoardStart;

#[derive(Debug, Error)]
pub enum BoardEditorDraftError {
    #[error("Invalid editor draft identifier: {0}")]
    Identifier(#[from] uuid::Error),
    #[error("Editor draft identifiers must be non-nil lowercase canonical UUID strings")]
    NoncanonicalIdentifier,
    #[error("Editor draft revisions must be canonical unsigned decimal strings")]
    NoncanonicalRevision,
    #[error("The editor draft revision counter is exhausted")]
    RevisionExhausted,
    #[error("The editor draft changed: expected revision {expected}, current revision {actual}")]
    StaleRevision {
        expected: BoardEditorDraftRevision,
        actual: BoardEditorDraftRevision,
    },
    #[error("The saved editor draft has more than one piece at {square}")]
    DuplicatePiece { square: BoardSquare },
    #[error("The saved editor draft repeats the castling right at {square}")]
    DuplicateCastlingRight { square: BoardSquare },
    #[error("The saved editor draft status does not match validation of its position fields")]
    InconsistentStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BoardEditorDraftId(Uuid);

impl BoardEditorDraftId {
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl TryFrom<Uuid> for BoardEditorDraftId {
    type Error = BoardEditorDraftError;

    fn try_from(value: Uuid) -> Result<Self, Self::Error> {
        if value.is_nil() {
            return Err(BoardEditorDraftError::NoncanonicalIdentifier);
        }
        Ok(Self(value))
    }
}

impl FromStr for BoardEditorDraftId {
    type Err = BoardEditorDraftError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parsed = Uuid::parse_str(value)?;
        if parsed.to_string() != value {
            return Err(BoardEditorDraftError::NoncanonicalIdentifier);
        }
        Self::try_from(parsed)
    }
}

impl TryFrom<String> for BoardEditorDraftId {
    type Error = BoardEditorDraftError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<BoardEditorDraftId> for String {
    fn from(value: BoardEditorDraftId) -> Self {
        value.0.to_string()
    }
}

impl Display for BoardEditorDraftId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BoardEditorDraftRevision(u64);

impl BoardEditorDraftRevision {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Result<Self, BoardEditorDraftError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(BoardEditorDraftError::RevisionExhausted)
    }

    pub fn check_expected(self, expected: Self) -> Result<(), BoardEditorDraftError> {
        if self == expected {
            Ok(())
        } else {
            Err(BoardEditorDraftError::StaleRevision {
                expected,
                actual: self,
            })
        }
    }
}

impl TryFrom<String> for BoardEditorDraftRevision {
    type Error = BoardEditorDraftError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let parsed: u64 = value
            .parse()
            .map_err(|_error| BoardEditorDraftError::NoncanonicalRevision)?;
        if parsed.to_string() != value {
            return Err(BoardEditorDraftError::NoncanonicalRevision);
        }
        Ok(Self(parsed))
    }
}

impl From<BoardEditorDraftRevision> for String {
    fn from(value: BoardEditorDraftRevision) -> Self {
        value.0.to_string()
    }
}

impl Display for BoardEditorDraftRevision {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "BoardEditorDraftFieldsRecord",
    into = "BoardEditorDraftFieldsRecord"
)]
pub struct BoardEditorDraftFields {
    board: Board,
    variant: BoardEnumVariant,
    turn: BoardEnumColor,
    castling_rights: Bitboard,
    en_passant: Option<BoardSquare>,
    halfmoves: i64,
    fullmoves: i64,
}

impl BoardEditorDraftFields {
    pub fn from_position(position: &BoardPosition) -> Self {
        Self::from_setup(
            position.chess.to_setup(EnPassantMode::Always),
            position.variant(),
        )
    }

    pub fn empty(variant: BoardEnumVariant) -> Self {
        Self::from_setup(Setup::empty(), variant)
    }

    pub const fn variant(&self) -> BoardEnumVariant {
        self.variant
    }

    pub const fn turn(&self) -> BoardEnumColor {
        self.turn
    }

    pub const fn en_passant(&self) -> Option<BoardSquare> {
        self.en_passant
    }

    pub const fn halfmoves(&self) -> i64 {
        self.halfmoves
    }

    pub const fn fullmoves(&self) -> i64 {
        self.fullmoves
    }

    pub fn piece_at(&self, square: BoardSquare) -> Option<BoardPiece> {
        self.board.piece_at(square.0).map(BoardPiece::from)
    }

    pub fn pieces(&self) -> impl Iterator<Item = (BoardSquare, BoardPiece)> + '_ {
        self.board
            .iter()
            .map(|(square, piece)| (BoardSquare(square), piece.into()))
    }

    pub fn castling_rights(&self) -> impl Iterator<Item = BoardSquare> + '_ {
        self.castling_rights.into_iter().map(BoardSquare)
    }

    pub(super) fn setup(&self, halfmoves: u32, fullmoves: NonZeroU32) -> Setup {
        Setup {
            board: self.board.clone(),
            turn: self.turn.into(),
            castling_rights: self.castling_rights,
            ep_square: self.en_passant.map(|square| square.0),
            halfmoves,
            fullmoves,
            ..Setup::empty()
        }
    }

    fn from_setup(setup: Setup, variant: BoardEnumVariant) -> Self {
        Self {
            board: setup.board,
            variant,
            turn: setup.turn.into(),
            castling_rights: setup.castling_rights,
            en_passant: setup.ep_square.map(BoardSquare),
            halfmoves: i64::from(setup.halfmoves),
            fullmoves: i64::from(setup.fullmoves.get()),
        }
    }

    fn place(&mut self, square: BoardSquare, color: BoardEnumColor, kind: BoardEnumPieceKind) {
        let role = match kind {
            BoardEnumPieceKind::Pawn => Role::Pawn,
            BoardEnumPieceKind::Knight => Role::Knight,
            BoardEnumPieceKind::Bishop => Role::Bishop,
            BoardEnumPieceKind::Rook => Role::Rook,
            BoardEnumPieceKind::Queen => Role::Queen,
            BoardEnumPieceKind::King => Role::King,
        };
        self.board.set_piece_at(
            square.0,
            Piece {
                color: color.into(),
                role,
            },
        );
    }

    fn move_piece(
        &mut self,
        from: BoardSquare,
        to: BoardSquare,
    ) -> Result<(), BoardEditorSessionError> {
        let piece = self
            .board
            .remove_piece_at(from.0)
            .ok_or(BoardEditorSessionError::EmptySquare { square: from })?;
        self.board.set_piece_at(to.0, piece);
        Ok(())
    }
    pub(super) fn apply_edit(
        &mut self,
        edit: BoardEditorSessionEnumEdit,
    ) -> Result<(), BoardEditorSessionError> {
        match edit {
            BoardEditorSessionEnumEdit::Place {
                square,
                color,
                kind,
            } => self.place(square, color, kind),
            BoardEditorSessionEnumEdit::Remove { square } => {
                self.board.discard_piece_at(square.0);
            }
            BoardEditorSessionEnumEdit::Move { from, to } => self.move_piece(from, to)?,
            BoardEditorSessionEnumEdit::Variant(variant) => self.variant = variant,
            BoardEditorSessionEnumEdit::Turn(turn) => self.turn = turn,
            BoardEditorSessionEnumEdit::GrantCastling(rook) => self.castling_rights.add(rook.0),
            BoardEditorSessionEnumEdit::RevokeCastling(rook) => {
                self.castling_rights.discard(rook.0)
            }
            BoardEditorSessionEnumEdit::EnPassant(square) => self.en_passant = square,
            BoardEditorSessionEnumEdit::Counters {
                halfmoves,
                fullmoves,
            } => {
                self.halfmoves = halfmoves;
                self.fullmoves = fullmoves;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BoardEditorDraftRecord", into = "BoardEditorDraftRecord")]
pub struct BoardEditorDraft {
    id: BoardEditorDraftId,
    revision: BoardEditorDraftRevision,
    fields: BoardEditorDraftFields,
}

impl BoardEditorDraft {
    pub fn new(id: BoardEditorDraftId, selected: Option<&BoardPosition>) -> Self {
        let fields = selected.map_or_else(
            || BoardEditorDraftFields::from_position(&BoardPosition::standard()),
            BoardEditorDraftFields::from_position,
        );
        Self::from_parts(id, BoardEditorDraftRevision::ZERO, fields)
    }

    pub const fn from_parts(
        id: BoardEditorDraftId,
        revision: BoardEditorDraftRevision,
        fields: BoardEditorDraftFields,
    ) -> Self {
        Self {
            id,
            revision,
            fields,
        }
    }

    pub fn from_fen(
        id: BoardEditorDraftId,
        fen: &str,
        variant: BoardEnumVariant,
    ) -> Result<Self, BoardPositionError> {
        let position = BoardPosition::from_fen(fen, variant)?;
        Ok(Self::new(id, Some(&position)))
    }

    pub const fn id(&self) -> BoardEditorDraftId {
        self.id
    }

    pub const fn revision(&self) -> BoardEditorDraftRevision {
        self.revision
    }

    pub const fn fields(&self) -> &BoardEditorDraftFields {
        &self.fields
    }

    pub fn validate(&self) -> Result<BoardPosition, BoardEditorValidationError> {
        self.fields.validate()
    }

    pub fn status(&self) -> BoardEditorValidationEnumStatus {
        match self.validate() {
            Ok(_) => BoardEditorValidationEnumStatus::Valid,
            Err(_) => BoardEditorValidationEnumStatus::Invalid,
        }
    }

    pub fn validated_fen(&self) -> Result<String, BoardEditorValidationError> {
        Ok(self.validate()?.fen())
    }

    pub fn play_prefill(&self) -> Result<BoardStart, BoardEditorValidationError> {
        Ok(BoardStart::custom(self.validate()?))
    }

    pub(super) fn replace_fields(
        &mut self,
        fields: BoardEditorDraftFields,
        revision: BoardEditorDraftRevision,
    ) -> BoardEditorDraftFields {
        self.revision = revision;
        std::mem::replace(&mut self.fields, fields)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoardEditorDraftPlacementRecord {
    square: BoardSquare,
    color: BoardEnumColor,
    kind: BoardEnumPieceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoardEditorDraftFieldsRecord {
    variant: BoardEnumVariant,
    pieces: Vec<BoardEditorDraftPlacementRecord>,
    turn: BoardEnumColor,
    castling_rights: Vec<BoardSquare>,
    en_passant: Option<BoardSquare>,
    halfmoves: i64,
    fullmoves: i64,
}

impl TryFrom<BoardEditorDraftFieldsRecord> for BoardEditorDraftFields {
    type Error = BoardEditorDraftError;

    fn try_from(record: BoardEditorDraftFieldsRecord) -> Result<Self, Self::Error> {
        let mut fields = Self::empty(record.variant);
        for piece in record.pieces {
            if fields.piece_at(piece.square).is_some() {
                return Err(BoardEditorDraftError::DuplicatePiece {
                    square: piece.square,
                });
            }
            fields.place(piece.square, piece.color, piece.kind);
        }
        for rook in record.castling_rights {
            if fields.castling_rights.contains(rook.0) {
                return Err(BoardEditorDraftError::DuplicateCastlingRight { square: rook });
            }
            fields.castling_rights.add(rook.0);
        }
        fields.turn = record.turn;
        fields.en_passant = record.en_passant;
        fields.halfmoves = record.halfmoves;
        fields.fullmoves = record.fullmoves;
        Ok(fields)
    }
}

impl From<BoardEditorDraftFields> for BoardEditorDraftFieldsRecord {
    fn from(fields: BoardEditorDraftFields) -> Self {
        Self {
            variant: fields.variant,
            pieces: fields
                .pieces()
                .map(|(square, piece)| BoardEditorDraftPlacementRecord {
                    square,
                    color: piece.color(),
                    kind: piece.kind(),
                })
                .collect(),
            turn: fields.turn,
            castling_rights: fields.castling_rights().collect(),
            en_passant: fields.en_passant,
            halfmoves: fields.halfmoves,
            fullmoves: fields.fullmoves,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoardEditorDraftRecord {
    id: BoardEditorDraftId,
    revision: BoardEditorDraftRevision,
    fields: BoardEditorDraftFields,
    status: BoardEditorValidationEnumStatus,
}

impl TryFrom<BoardEditorDraftRecord> for BoardEditorDraft {
    type Error = BoardEditorDraftError;

    fn try_from(record: BoardEditorDraftRecord) -> Result<Self, Self::Error> {
        let draft = Self::from_parts(record.id, record.revision, record.fields);
        if draft.status() != record.status {
            return Err(BoardEditorDraftError::InconsistentStatus);
        }
        Ok(draft)
    }
}

impl From<BoardEditorDraft> for BoardEditorDraftRecord {
    fn from(draft: BoardEditorDraft) -> Self {
        let status = draft.status();
        Self {
            id: draft.id,
            revision: draft.revision,
            fields: draft.fields,
            status,
        }
    }
}
