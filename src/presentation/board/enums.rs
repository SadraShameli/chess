use bevy_egui::egui::{Color32, Key, Vec2, Visuals};

use crate::board::square::BoardSquare;
use crate::game::premove::GamePremoveIntent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresentationBoardEnumInputSource {
    Pointer,
    Keyboard,
    Accessibility { occurrence: usize },
}

#[derive(Debug, Clone, Copy, strum::IntoStaticStr, strum::Display, strum::EnumIter)]
pub(crate) enum PresentationBoardEnumPremoveSquare {
    #[strum(serialize = "Premove from")]
    Source,
    #[strum(serialize = "Premove to")]
    Destination,
}

impl PresentationBoardEnumPremoveSquare {
    pub(crate) const fn square(self, intent: GamePremoveIntent) -> BoardSquare {
        match self {
            Self::Source => intent.origin(),
            Self::Destination => intent.destination(),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        self.into()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumIter)]
pub enum PresentationBoardEnumDirection {
    Up,
    Down,
    Left,
    Right,
}

impl PresentationBoardEnumDirection {
    pub const fn key(self) -> Key {
        match self {
            Self::Up => Key::ArrowUp,
            Self::Down => Key::ArrowDown,
            Self::Left => Key::ArrowLeft,
            Self::Right => Key::ArrowRight,
        }
    }

    pub const fn vector(self) -> Vec2 {
        match self {
            Self::Up => Vec2::new(0.0, -1.0),
            Self::Down => Vec2::new(0.0, 1.0),
            Self::Left => Vec2::new(-1.0, 0.0),
            Self::Right => Vec2::new(1.0, 0.0),
        }
    }
}
