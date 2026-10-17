use bevy_egui::egui::{Color32, Key, Visuals};

use crate::board::enums::BoardEnumColor;
use crate::board::square::BoardSquare;
use crate::game::premove::GamePremoveIntent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresentationBoardEnumInputSource {
    Pointer,
    Keyboard,
    Accessibility { occurrence: usize },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PresentationBoardEnumPremoveSquare {
    Source,
    Destination,
}

impl PresentationBoardEnumPremoveSquare {
    pub(crate) const ALL: [Self; 2] = [Self::Source, Self::Destination];

    pub(crate) const fn square(self, intent: GamePremoveIntent) -> BoardSquare {
        match self {
            Self::Source => intent.origin(),
            Self::Destination => intent.destination(),
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Source => "Premove from",
            Self::Destination => "Premove to",
        }
    }

    pub(crate) const fn semantic_label(self) -> &'static str {
        match self {
            Self::Source => ", queued premove source",
            Self::Destination => ", queued premove destination",
        }
    }

    pub(crate) fn color(self, visuals: &Visuals) -> Color32 {
        match self {
            Self::Source => visuals.selection.stroke.color,
            Self::Destination => visuals.warn_fg_color,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationBoardEnumCameraAction {
    RotateLeft,
    RotateRight,
    TiltUp,
    TiltDown,
    ZoomIn,
    ZoomOut,
    Reset,
    Flip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationBoardEnumDirection {
    Up,
    Down,
    Left,
    Right,
}

impl PresentationBoardEnumDirection {
    pub const ALL: [Self; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];

    pub const fn key(self) -> Key {
        match self {
            Self::Up => Key::ArrowUp,
            Self::Down => Key::ArrowDown,
            Self::Left => Key::ArrowLeft,
            Self::Right => Key::ArrowRight,
        }
    }

    pub(crate) const fn offset(self, orientation: BoardEnumColor) -> (i8, i8) {
        let sign = match orientation {
            BoardEnumColor::White => 1,
            BoardEnumColor::Black => -1,
        };
        match self {
            Self::Up => (0, sign),
            Self::Down => (0, -sign),
            Self::Left => (-sign, 0),
            Self::Right => (sign, 0),
        }
    }
}
