use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};
use shakmaty::{Board, Chess, Color, FromSetup, Setup};
use thiserror::Error;

use crate::board::enums::BoardEnumVariant;
use crate::board::position::{BoardPosition, BoardPositionError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct BoardStartIndex(u16);

impl BoardStartIndex {
    pub const MIN: u16 = 0;
    pub const MAX: u16 = 959;
    pub const STANDARD: Self = Self(518);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn random<R: Rng + ?Sized>(random: &mut R) -> Self {
        Self(random.random_range(Self::MIN..=Self::MAX))
    }
}

impl TryFrom<u16> for BoardStartIndex {
    type Error = BoardStartIndexError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err(BoardStartIndexError { value })
        }
    }
}

impl From<BoardStartIndex> for u16 {
    fn from(index: BoardStartIndex) -> Self {
        index.value()
    }
}

#[derive(Debug, Error)]
#[error("Scharnagl index {value} is outside zero-based 0..959")]
pub struct BoardStartIndexError {
    value: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BoardStartEnumSelection {
    Standard,
    Chess960 {
        index: BoardStartIndex,
    },
    Dfrc {
        white_index: BoardStartIndex,
        black_index: BoardStartIndex,
    },
    Custom {
        variant: BoardEnumVariant,
    },
}

impl BoardStartEnumSelection {
    pub const fn variant(self) -> BoardEnumVariant {
        match self {
            Self::Standard => BoardEnumVariant::Standard,
            Self::Chess960 { .. } | Self::Dfrc { .. } => BoardEnumVariant::Chess960,
            Self::Custom { variant } => variant,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BoardStartRecord", into = "BoardStartRecord")]
pub struct BoardStart {
    selection: BoardStartEnumSelection,
    position: BoardPosition,
    initial_fen: String,
}

impl BoardStart {
    pub fn standard() -> Self {
        Self::from_position(BoardStartEnumSelection::Standard, BoardPosition::standard())
    }

    pub fn chess960(index: BoardStartIndex) -> Result<Self, BoardStartError> {
        Self::randomized(BoardStartEnumSelection::Chess960 { index }, index, index)
    }

    pub fn dfrc(
        white_index: BoardStartIndex,
        black_index: BoardStartIndex,
    ) -> Result<Self, BoardStartError> {
        Self::randomized(
            BoardStartEnumSelection::Dfrc {
                white_index,
                black_index,
            },
            white_index,
            black_index,
        )
    }

    pub fn custom(position: BoardPosition) -> Self {
        Self::from_position(
            BoardStartEnumSelection::Custom {
                variant: position.variant(),
            },
            position,
        )
    }

    pub fn random_chess960<R: Rng + ?Sized>(random: &mut R) -> Result<Self, BoardStartError> {
        Self::chess960(BoardStartIndex::random(random))
    }

    pub fn random_dfrc<R: Rng + ?Sized>(random: &mut R) -> Result<Self, BoardStartError> {
        Self::dfrc(
            BoardStartIndex::random(random),
            BoardStartIndex::random(random),
        )
    }

    pub const fn selection(&self) -> BoardStartEnumSelection {
        self.selection
    }

    pub fn position(&self) -> &BoardPosition {
        &self.position
    }

    pub fn initial_fen(&self) -> &str {
        &self.initial_fen
    }

    fn randomized(
        selection: BoardStartEnumSelection,
        white_index: BoardStartIndex,
        black_index: BoardStartIndex,
    ) -> Result<Self, BoardStartError> {
        let white = Board::chess960(u32::from(white_index.value()));
        let black = Board::chess960(u32::from(black_index.value()));
        let mut board = Board::empty();
        for (square, piece) in white
            .iter()
            .filter(|(_, piece)| piece.color == Color::White)
            .chain(
                black
                    .iter()
                    .filter(|(_, piece)| piece.color == Color::Black),
            )
        {
            board.set_piece_at(square, piece);
        }
        let setup = Setup {
            castling_rights: board.rooks(),
            board,
            ..Setup::default()
        };
        let chess = Chess::from_setup(setup, selection.variant().castling_mode())
            .map_err(|error| BoardPositionError::IllegalPosition(error.kinds()))?;
        let position = BoardPosition::from_chess(chess, selection.variant())?;
        Ok(Self::from_position(selection, position))
    }

    fn from_position(selection: BoardStartEnumSelection, position: BoardPosition) -> Self {
        let initial_fen = position.fen();
        Self {
            selection,
            position,
            initial_fen,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoardStartRecord {
    selection: BoardStartEnumSelection,
    initial_fen: String,
}

impl TryFrom<BoardStartRecord> for BoardStart {
    type Error = BoardStartError;

    fn try_from(record: BoardStartRecord) -> Result<Self, Self::Error> {
        let position = BoardPosition::from_fen(&record.initial_fen, record.selection.variant())?;
        let start = match record.selection {
            BoardStartEnumSelection::Standard => Self::standard(),
            BoardStartEnumSelection::Chess960 { index } => Self::chess960(index)?,
            BoardStartEnumSelection::Dfrc {
                white_index,
                black_index,
            } => Self::dfrc(white_index, black_index)?,
            BoardStartEnumSelection::Custom { .. } => return Ok(Self::custom(position)),
        };
        if start.position != position {
            return Err(BoardStartError::InconsistentFen);
        }
        Ok(start)
    }
}

impl From<BoardStart> for BoardStartRecord {
    fn from(start: BoardStart) -> Self {
        Self {
            selection: start.selection,
            initial_fen: start.initial_fen,
        }
    }
}

#[derive(Debug, Error)]
pub enum BoardStartError {
    #[error(transparent)]
    Position(#[from] BoardPositionError),
    #[error("The saved initial FEN does not match its selected start indexes")]
    InconsistentFen,
}
