use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardPiece {
    color: BoardEnumColor,
    kind: BoardEnumPieceKind,
}

impl BoardPiece {
    pub fn color(self) -> BoardEnumColor {
        self.color
    }

    pub fn kind(self) -> BoardEnumPieceKind {
        self.kind
    }
}

impl From<shakmaty::Piece> for BoardPiece {
    fn from(piece: shakmaty::Piece) -> Self {
        Self {
            color: piece.color.into(),
            kind: piece.role.into(),
        }
    }
}
