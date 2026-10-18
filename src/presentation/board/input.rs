use bevy_egui::egui::Pos2;

use crate::board::square::BoardSquare;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationBoardInputEnumGesture {
    Click(BoardSquare),
    Drag {
        origin: BoardSquare,
        destination: BoardSquare,
    },
}

pub struct PresentationBoardInput;

impl PresentationBoardInput {
    pub const DRAG_THRESHOLD_POINTS: f32 = 5.0;

    pub fn is_drag(press: Pos2, release: Pos2) -> bool {
        press.distance(release) >= Self::DRAG_THRESHOLD_POINTS
    }

    pub fn classify(
        origin: BoardSquare,
        destination: BoardSquare,
        press: Option<Pos2>,
        release: Option<Pos2>,
    ) -> PresentationBoardInputEnumGesture {
        let moved = match (press, release) {
            (Some(press), Some(release)) => Self::is_drag(press, release),
            (Some(_), None) | (None, Some(_)) | (None, None) => true,
        };
        if origin == destination || !moved {
            PresentationBoardInputEnumGesture::Click(origin)
        } else {
            PresentationBoardInputEnumGesture::Drag {
                origin,
                destination,
            }
        }
    }
}
