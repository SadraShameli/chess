use crate::board::enums::BoardEnumVariant;
use crate::board::position::{BoardPosition, BoardPositionError};

pub struct NotationFen;

impl NotationFen {
    pub fn import(
        fen: &str,
        variant: BoardEnumVariant,
    ) -> Result<BoardPosition, BoardPositionError> {
        BoardPosition::from_fen(fen, variant)
    }

    pub fn export(position: &BoardPosition) -> String {
        position.fen()
    }
}
