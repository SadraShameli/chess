use shakmaty::san::{SanError, SanPlus};
use shakmaty::uci::{ParseUciMoveError, UciMove};
use std::num::NonZeroU32;

use shakmaty::{CastlingSide, Chess, Color, EnPassantMode, FromSetup, Move, Position};
use thiserror::Error;

use crate::board::enums::{BoardEnumPieceKind, BoardEnumVariant};
use crate::board::position::{BoardPosition, BoardPositionError};
use crate::board::square::BoardSquare;

#[derive(Debug, Error)]
pub enum BoardMoveError {
    #[error("Invalid UCI move syntax: {0}")]
    Syntax(#[from] ParseUciMoveError),
    #[error("The move is illegal in the selected position")]
    IllegalMove,
    #[error("The move does not use this variant's canonical castling encoding")]
    CastlingEncoding,
    #[error("Choose a knight, bishop, rook or queen for promotion")]
    PromotionRequired,
    #[error("Select the castling rook to distinguish castling from a king move")]
    AmbiguousCastling,
    #[error("Invalid SAN syntax: {0}")]
    SanSyntax(#[from] shakmaty::san::ParseSanError),
    #[error("Invalid SAN move: {0}")]
    San(#[from] SanError),
    #[error(transparent)]
    Position(#[from] BoardPositionError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardMove {
    inner: Move,
    side: Color,
}

impl BoardMove {
    pub fn origin(self) -> Option<BoardSquare> {
        self.inner.from().map(BoardSquare)
    }

    pub fn destination(self) -> BoardSquare {
        BoardSquare(match self.inner {
            Move::Castle { king, rook } => {
                CastlingSide::from_king_side(king < rook).king_to(self.side)
            }
            Move::Normal { to, .. } | Move::EnPassant { to, .. } | Move::Put { to, .. } => to,
        })
    }

    pub fn promotion(self) -> Option<BoardEnumPieceKind> {
        self.inner.promotion().map(BoardEnumPieceKind::from)
    }

    pub fn castling_rook(self) -> Option<BoardSquare> {
        match self.inner {
            Move::Castle { rook, .. } => Some(BoardSquare(rook)),
            Move::Normal { .. } | Move::EnPassant { .. } | Move::Put { .. } => None,
        }
    }

    pub fn uci(self, variant: BoardEnumVariant) -> String {
        self.inner.to_uci(variant.castling_mode()).to_string()
    }
}

impl BoardPosition {
    pub fn legal_moves(&self) -> Vec<BoardMove> {
        self.chess
            .legal_moves()
            .into_iter()
            .map(|inner| BoardMove {
                inner,
                side: self.chess.turn(),
            })
            .collect()
    }

    pub fn parse_uci(&self, value: &str) -> Result<BoardMove, BoardMoveError> {
        let parsed: UciMove = value.parse()?;
        let inner = parsed
            .to_move(&self.chess)
            .map_err(|_error| BoardMoveError::IllegalMove)?;
        let movement = BoardMove {
            inner,
            side: self.chess.turn(),
        };
        if movement.uci(self.variant()) != value {
            return Err(BoardMoveError::CastlingEncoding);
        }
        Ok(movement)
    }

    pub fn move_intent(
        &self,
        from: BoardSquare,
        to: BoardSquare,
        promotion: Option<BoardEnumPieceKind>,
    ) -> Result<BoardMove, BoardMoveError> {
        if promotion.is_some_and(|piece| !piece.is_promotion()) {
            return Err(BoardMoveError::PromotionRequired);
        }
        let mut candidates = self
            .legal_moves()
            .into_iter()
            .filter(|movement| movement.origin() == Some(from));
        if let Some(castling) = candidates
            .clone()
            .find(|movement| movement.castling_rook() == Some(to))
        {
            return if promotion.is_none() {
                Ok(castling)
            } else {
                Err(BoardMoveError::IllegalMove)
            };
        }
        let mut selected = None;
        let mut requires_promotion = false;
        for movement in candidates
            .by_ref()
            .filter(|movement| movement.destination() == to)
        {
            requires_promotion |= movement.promotion().is_some();
            if movement.promotion() == promotion {
                if selected.is_some() {
                    return Err(BoardMoveError::AmbiguousCastling);
                }
                selected = Some(movement);
            }
        }
        selected.ok_or(if requires_promotion && promotion.is_none() {
            BoardMoveError::PromotionRequired
        } else {
            BoardMoveError::IllegalMove
        })
    }

    pub fn play(&self, movement: BoardMove) -> Result<Self, BoardMoveError> {
        Ok(Self::from_chess(
            self.played_chess(movement)?,
            self.variant(),
        )?)
    }

    pub fn play_uci(&self, value: &str) -> Result<Self, BoardMoveError> {
        self.play(self.parse_uci(value)?)
    }

    pub fn play_for_mate_proof(&self, movement: BoardMove) -> Result<Self, BoardMoveError> {
        let chess = self.played_chess(movement)?;
        let mut setup = chess.to_setup(EnPassantMode::Always);
        setup.halfmoves = 0;
        setup.fullmoves = NonZeroU32::MIN;
        let normalized = Chess::from_setup(setup, self.variant().castling_mode())
            .map_err(|error| BoardPositionError::IllegalPosition(error.kinds()))?;
        Ok(Self::from_chess(normalized, self.variant())?)
    }

    fn played_chess(&self, movement: BoardMove) -> Result<Chess, BoardMoveError> {
        if movement.side != self.chess.turn() {
            return Err(BoardMoveError::IllegalMove);
        }
        self.chess
            .clone()
            .play(movement.inner)
            .map_err(|_error| BoardMoveError::IllegalMove)
    }

    pub fn san(&self, value: &str) -> Result<String, BoardMoveError> {
        let movement = self.parse_uci(value)?;
        Ok(SanPlus::from_move(self.chess.clone(), movement.inner).to_string())
    }

    pub fn parse_san(&self, value: &str) -> Result<BoardMove, BoardMoveError> {
        let notation: SanPlus = value.parse()?;
        let inner = notation.san.to_move(&self.chess)?;
        let movement = BoardMove {
            inner,
            side: self.chess.turn(),
        };
        let canonical = SanPlus::from_move(self.chess.clone(), inner);
        if canonical.suffix != notation.suffix {
            return Err(BoardMoveError::IllegalMove);
        }
        Ok(movement)
    }
}
