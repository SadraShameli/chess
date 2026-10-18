use std::collections::HashMap;

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::board::square::BoardSquare;
use crate::desktop::dto::DesktopBoardSnapshot;

pub struct PresentationBoardTransition;

impl PresentationBoardTransition {
    const FILES: u8 = 8;

    pub fn piece_move(
        previous: &DesktopBoardSnapshot,
        current: &DesktopBoardSnapshot,
    ) -> Option<(BoardSquare, BoardSquare)> {
        let before = Self::occupancy(previous);
        let after = Self::occupancy(current);
        let vacated = before
            .iter()
            .filter(|(square, piece)| after.get(*square) != Some(*piece));
        let vacated: Vec<_> = vacated.collect();
        after
            .iter()
            .filter(|(square, piece)| before.get(*square) != Some(*piece))
            .flat_map(|(destination, arrived)| {
                vacated
                    .iter()
                    .filter(|(_, left)| Self::is_same_mover(**left, *arrived))
                    .map(move |(origin, _)| (**origin, *destination, arrived.1))
            })
            .max_by_key(|(origin, destination, kind)| {
                (
                    *kind == BoardEnumPieceKind::King,
                    Self::distance(*origin, *destination),
                )
            })
            .map(|(origin, destination, _)| (origin, destination))
    }

    fn occupancy(
        snapshot: &DesktopBoardSnapshot,
    ) -> HashMap<BoardSquare, (BoardEnumColor, BoardEnumPieceKind)> {
        snapshot
            .pieces()
            .iter()
            .map(|piece| (piece.square(), (piece.color(), piece.kind())))
            .collect()
    }

    fn is_same_mover(
        left: (BoardEnumColor, BoardEnumPieceKind),
        arrived: (BoardEnumColor, BoardEnumPieceKind),
    ) -> bool {
        left.0 == arrived.0
            && (left.1 == arrived.1
                || (left.1 == BoardEnumPieceKind::Pawn && arrived.1.is_promotion()))
    }

    fn distance(origin: BoardSquare, destination: BoardSquare) -> u8 {
        let (origin, destination) = (u8::from(origin), u8::from(destination));
        (origin % Self::FILES)
            .abs_diff(destination % Self::FILES)
            .max((origin / Self::FILES).abs_diff(destination / Self::FILES))
    }
}
