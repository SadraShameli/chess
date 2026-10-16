use serde::{Deserialize, Serialize};
use shakmaty::{CastlingMode, Color};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardEnumVariant {
    Standard,
    Chess960,
}

impl BoardEnumVariant {
    pub const fn castling_mode(self) -> CastlingMode {
        match self {
            Self::Standard => CastlingMode::Standard,
            Self::Chess960 => CastlingMode::Chess960,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardEnumColor {
    White,
    Black,
}

impl BoardEnumColor {
    pub const fn opposite(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }
}

impl From<Color> for BoardEnumColor {
    fn from(color: Color) -> Self {
        match color {
            Color::White => Self::White,
            Color::Black => Self::Black,
        }
    }
}

impl From<BoardEnumColor> for Color {
    fn from(color: BoardEnumColor) -> Self {
        match color {
            BoardEnumColor::White => Self::White,
            BoardEnumColor::Black => Self::Black,
        }
    }
}
