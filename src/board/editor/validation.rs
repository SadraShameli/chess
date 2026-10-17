use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};
use shakmaty::{Chess, FromSetup, PositionErrorKinds, Rank};
use thiserror::Error;

use crate::board::editor::draft::BoardEditorDraftFields;
use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use crate::board::position::{BoardPosition, BoardPositionError};
use crate::board::square::BoardSquare;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardEditorValidationEnumStatus {
    Valid,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BoardEditorValidationEnumFinding {
    #[error("Place one king of each color on the empty board")]
    EmptyBoard,
    #[error("Place exactly one {color:?} king; the draft contains {actual}")]
    KingCount {
        color: BoardEnumColor,
        actual: usize,
    },
    #[error("Move or remove pawns on the first or eighth rank: {squares:?}")]
    PawnsOnBackrank { squares: Vec<BoardSquare> },
    #[error(
        "Correct the king and original rook placements or revoke incompatible {variant:?} castling rights: {rook_squares:?}"
    )]
    CastlingRights {
        variant: BoardEnumVariant,
        rook_squares: Vec<BoardSquare>,
    },
    #[error(
        "Clear or correct the en passant target {square:?} and its pawn context for {turn:?} to move"
    )]
    EnPassant {
        square: Option<BoardSquare>,
        turn: BoardEnumColor,
    },
    #[error("Change the turn or piece placement so the nonmoving {color:?} king is not in check")]
    OppositeCheck { color: BoardEnumColor },
    #[error(
        "Correct the checking pieces or en passant context: the check against {turn:?} is impossible"
    )]
    ImpossibleCheck { turn: BoardEnumColor },
    #[error("Remove excess pieces to satisfy the canonical board material constraints")]
    TooMuchMaterial,
    #[error("Correct the piece placement for the declared {variant:?} variant")]
    Variant { variant: BoardEnumVariant },
    #[error("Set the halfmove counter to 0..1000000; the draft contains {value}")]
    Halfmoves { value: i64 },
    #[error("Set the fullmove counter to 1..1000000; the draft contains {value}")]
    Fullmoves { value: i64 },
    #[error("Correct the draft fields rejected by the canonical board validator: {reason}")]
    CanonicalPosition { reason: String },
}

#[derive(Debug, Error)]
#[error("Correct the editor draft before play, analysis or FEN export: {findings:?}")]
pub struct BoardEditorValidationError {
    findings: Vec<BoardEditorValidationEnumFinding>,
    #[source]
    source: Option<BoardPositionError>,
}

impl BoardEditorValidationError {
    pub fn findings(&self) -> &[BoardEditorValidationEnumFinding] {
        &self.findings
    }
}

impl BoardEditorDraftFields {
    pub fn validate(&self) -> Result<BoardPosition, BoardEditorValidationError> {
        let mut findings = Vec::new();
        let halfmoves = u32::try_from(self.halfmoves())
            .ok()
            .filter(|value| *value <= BoardPosition::MAX_COUNTER);
        let fullmoves = u32::try_from(self.fullmoves())
            .ok()
            .and_then(NonZeroU32::new)
            .filter(|value| value.get() <= BoardPosition::MAX_COUNTER);
        if halfmoves.is_none() {
            findings.push(BoardEditorValidationEnumFinding::Halfmoves {
                value: self.halfmoves(),
            });
        }
        if fullmoves.is_none() {
            findings.push(BoardEditorValidationEnumFinding::Fullmoves {
                value: self.fullmoves(),
            });
        }
        let (Some(halfmoves), Some(fullmoves)) = (halfmoves, fullmoves) else {
            return Err(BoardEditorValidationError {
                findings,
                source: None,
            });
        };
        let chess = Chess::from_setup(
            self.setup(halfmoves, fullmoves),
            self.variant().castling_mode(),
        )
        .map_err(|error| BoardEditorValidationError {
            findings: self.position_findings(error.kinds()),
            source: Some(BoardPositionError::IllegalPosition(error.kinds())),
        })?;
        BoardPosition::from_chess(chess, self.variant()).map_err(|source| {
            BoardEditorValidationError {
                findings: vec![BoardEditorValidationEnumFinding::CanonicalPosition {
                    reason: source.to_string(),
                }],
                source: Some(source),
            }
        })
    }

    fn position_findings(
        &self,
        kinds: PositionErrorKinds,
    ) -> Vec<BoardEditorValidationEnumFinding> {
        let mut findings = Vec::new();
        if kinds.contains(PositionErrorKinds::EMPTY_BOARD) {
            findings.push(BoardEditorValidationEnumFinding::EmptyBoard);
        }
        if kinds.intersects(PositionErrorKinds::MISSING_KING | PositionErrorKinds::TOO_MANY_KINGS) {
            for color in [BoardEnumColor::White, BoardEnumColor::Black] {
                let actual = self
                    .pieces()
                    .filter(|(_, piece)| {
                        piece.kind() == BoardEnumPieceKind::King && piece.color() == color
                    })
                    .count();
                if actual != 1 {
                    findings.push(BoardEditorValidationEnumFinding::KingCount { color, actual });
                }
            }
        }
        if kinds.contains(PositionErrorKinds::PAWNS_ON_BACKRANK) {
            findings.push(BoardEditorValidationEnumFinding::PawnsOnBackrank {
                squares: self
                    .pieces()
                    .filter(|(square, piece)| {
                        piece.kind() == BoardEnumPieceKind::Pawn
                            && matches!(square.0.rank(), Rank::First | Rank::Eighth)
                    })
                    .map(|(square, _)| square)
                    .collect(),
            });
        }
        if kinds.contains(PositionErrorKinds::INVALID_CASTLING_RIGHTS) {
            findings.push(BoardEditorValidationEnumFinding::CastlingRights {
                variant: self.variant(),
                rook_squares: self.castling_rights().collect(),
            });
        }
        if kinds.contains(PositionErrorKinds::INVALID_EP_SQUARE) {
            findings.push(BoardEditorValidationEnumFinding::EnPassant {
                square: self.en_passant(),
                turn: self.turn(),
            });
        }
        if kinds.contains(PositionErrorKinds::OPPOSITE_CHECK) {
            findings.push(BoardEditorValidationEnumFinding::OppositeCheck {
                color: self.turn().opposite(),
            });
        }
        if kinds.contains(PositionErrorKinds::IMPOSSIBLE_CHECK) {
            findings.push(BoardEditorValidationEnumFinding::ImpossibleCheck { turn: self.turn() });
        }
        if kinds.contains(PositionErrorKinds::TOO_MUCH_MATERIAL) {
            findings.push(BoardEditorValidationEnumFinding::TooMuchMaterial);
        }
        if kinds.contains(PositionErrorKinds::VARIANT) {
            findings.push(BoardEditorValidationEnumFinding::Variant {
                variant: self.variant(),
            });
        }
        findings
    }
}
