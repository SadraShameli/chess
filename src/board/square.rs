use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use shakmaty::{ParseSquareError, Square};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BoardSquareError {
    #[error("A square index must be between 0 and 63")]
    OutOfRange,
    #[error("Invalid square: {0}")]
    Invalid(#[from] ParseSquareError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct BoardSquare(pub(super) Square);

impl JsonSchema for BoardSquare {
    fn schema_name() -> Cow<'static, str> {
        "BoardSquare".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "integer", "minimum": 0, "maximum": 63})
    }
}

impl TryFrom<u8> for BoardSquare {
    type Error = BoardSquareError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value < 64 {
            Ok(Self(Square::new(u32::from(value))))
        } else {
            Err(BoardSquareError::OutOfRange)
        }
    }
}

impl From<BoardSquare> for u8 {
    fn from(square: BoardSquare) -> Self {
        square.0.into()
    }
}

impl FromStr for BoardSquare {
    type Err = BoardSquareError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}

impl Display for BoardSquare {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use std::borrow::Cow;
