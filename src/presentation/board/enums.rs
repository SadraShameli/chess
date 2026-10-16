#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresentationBoardEnumInputSource {
    Pointer,
    Keyboard,
    Accessibility { occurrence: usize },
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
use bevy_egui::egui::{Key, Vec2};

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

    pub const fn vector(self) -> Vec2 {
        match self {
            Self::Up => Vec2::new(0.0, -1.0),
            Self::Down => Vec2::new(0.0, 1.0),
            Self::Left => Vec2::new(-1.0, 0.0),
            Self::Right => Vec2::new(1.0, 0.0),
        }
    }
}
