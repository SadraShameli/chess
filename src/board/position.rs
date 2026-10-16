use std::num::ParseIntError;

use shakmaty::fen::{Fen, ParseFenError};
use shakmaty::{
    Bitboard, Board, Chess, Color, EnPassantMode, Position, PositionErrorKinds, Square,
};
use thiserror::Error;

use crate::board::enums::{BoardEnumColor, BoardEnumVariant};

#[derive(Debug, Error)]
pub enum BoardPositionError {
    #[error("A FEN must contain six ASCII fields and at most 512 bytes")]
    InvalidFenShape,
    #[error("Invalid FEN syntax: {0}")]
    Fen(#[from] ParseFenError),
    #[error("Invalid move counter: {0}")]
    Counter(#[from] ParseIntError),
    #[error("Halfmove must be 0..1000000 and fullmove must be 1..1000000")]
    CounterRange,
    #[error("Structurally invalid position: {0:?}")]
    IllegalPosition(PositionErrorKinds),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BoardPositionIdentity {
    variant: BoardEnumVariant,
    board: Board,
    turn: Color,
    castling_rights: Bitboard,
    en_passant: Option<Square>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardPosition {
    pub(super) chess: Chess,
    variant: BoardEnumVariant,
}

impl BoardPosition {
    pub const MAX_FEN_BYTES: usize = 512;
    pub const MAX_COUNTER: u32 = 1_000_000;

    pub fn standard() -> Self {
        Self {
            chess: Chess::default(),
            variant: BoardEnumVariant::Standard,
        }
    }

    pub fn from_fen(fen: &str, variant: BoardEnumVariant) -> Result<Self, BoardPositionError> {
        if fen.len() > Self::MAX_FEN_BYTES || !fen.is_ascii() {
            return Err(BoardPositionError::InvalidFenShape);
        }
        let mut fields = fen.split_ascii_whitespace();
        for _ in 0..4 {
            fields.next().ok_or(BoardPositionError::InvalidFenShape)?;
        }
        let halfmove: u32 = fields
            .next()
            .ok_or(BoardPositionError::InvalidFenShape)?
            .parse()?;
        let fullmove: u32 = fields
            .next()
            .ok_or(BoardPositionError::InvalidFenShape)?
            .parse()?;
        if fields.next().is_some() {
            return Err(BoardPositionError::InvalidFenShape);
        }
        Self::validate_counters(halfmove, fullmove)?;
        let parsed: Fen = fen.parse()?;
        let chess: Chess = parsed
            .into_position(variant.castling_mode())
            .map_err(|error| BoardPositionError::IllegalPosition(error.kinds()))?;
        Self::from_chess(chess, variant)
    }

    pub fn variant(&self) -> BoardEnumVariant {
        self.variant
    }

    pub fn turn(&self) -> BoardEnumColor {
        self.chess.turn().into()
    }

    pub fn fen(&self) -> String {
        match self.variant {
            BoardEnumVariant::Standard => {
                Fen::from_position(&self.chess, EnPassantMode::Always).to_string()
            }
            BoardEnumVariant::Chess960 => self.shredder_fen(),
        }
    }

    pub fn placement(&self) -> String {
        self.chess.board().to_string()
    }

    pub fn identity(&self) -> BoardPositionIdentity {
        BoardPositionIdentity {
            variant: self.variant,
            board: self.chess.board().clone(),
            turn: self.chess.turn(),
            castling_rights: self.chess.castles().castling_rights(),
            en_passant: self.chess.legal_ep_square(),
        }
    }

    pub fn is_check(&self) -> bool {
        self.chess.is_check()
    }

    pub(super) fn from_chess(
        chess: Chess,
        variant: BoardEnumVariant,
    ) -> Result<Self, BoardPositionError> {
        Self::validate_counters(chess.halfmoves(), chess.fullmoves().get())?;
        Ok(Self { chess, variant })
    }

    fn validate_counters(halfmove: u32, fullmove: u32) -> Result<(), BoardPositionError> {
        if halfmove > Self::MAX_COUNTER || fullmove == 0 || fullmove > Self::MAX_COUNTER {
            Err(BoardPositionError::CounterRange)
        } else {
            Ok(())
        }
    }
}
