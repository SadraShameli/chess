use thiserror::Error;

use crate::board::r#move::BoardMove;
use crate::board::position::BoardPosition;
use crate::board::square::BoardSquare;
use crate::clock::state::ClockStateSnapshot;
use crate::game::enums::{GameEnumMarkingColor, GameMoveId};

#[derive(Debug, Error)]
pub enum GameMoveNodeAnnotationError {
    #[error("Plain-text notes must not contain a NUL character")]
    InvalidNotes,
    #[error("An arrow must connect two different squares")]
    EmptyArrow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameMoveNodeNag(u8);

impl GameMoveNodeNag {
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameMoveNodeEnumMarking {
    Circle(BoardSquare),
    Arrow {
        origin: BoardSquare,
        destination: BoardSquare,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameMoveNodeMarking {
    color: GameEnumMarkingColor,
    shape: GameMoveNodeEnumMarking,
}

impl GameMoveNodeMarking {
    pub fn circle(color: GameEnumMarkingColor, square: BoardSquare) -> Self {
        Self {
            color,
            shape: GameMoveNodeEnumMarking::Circle(square),
        }
    }

    pub fn arrow(
        color: GameEnumMarkingColor,
        origin: BoardSquare,
        destination: BoardSquare,
    ) -> Result<Self, GameMoveNodeAnnotationError> {
        if origin == destination {
            return Err(GameMoveNodeAnnotationError::EmptyArrow);
        }
        Ok(Self {
            color,
            shape: GameMoveNodeEnumMarking::Arrow {
                origin,
                destination,
            },
        })
    }

    pub fn color(self) -> GameEnumMarkingColor {
        self.color
    }

    pub fn origin(self) -> BoardSquare {
        match self.shape {
            GameMoveNodeEnumMarking::Circle(square) => square,
            GameMoveNodeEnumMarking::Arrow { origin, .. } => origin,
        }
    }

    pub fn destination(self) -> Option<BoardSquare> {
        match self.shape {
            GameMoveNodeEnumMarking::Circle(_) => None,
            GameMoveNodeEnumMarking::Arrow { destination, .. } => Some(destination),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameMoveNodeAnnotations {
    notes: String,
    nags: Vec<GameMoveNodeNag>,
    markings: Vec<GameMoveNodeMarking>,
}

impl GameMoveNodeAnnotations {
    pub fn new(
        notes: String,
        nags: Vec<GameMoveNodeNag>,
        markings: Vec<GameMoveNodeMarking>,
    ) -> Result<Self, GameMoveNodeAnnotationError> {
        if notes.contains('\0') {
            return Err(GameMoveNodeAnnotationError::InvalidNotes);
        }
        Ok(Self {
            notes,
            nags,
            markings,
        })
    }

    pub fn notes(&self) -> &str {
        &self.notes
    }

    pub fn nags(&self) -> &[GameMoveNodeNag] {
        &self.nags
    }

    pub fn markings(&self) -> &[GameMoveNodeMarking] {
        &self.markings
    }
}

#[derive(Debug, Clone)]
enum GameMoveNodeEnumOrigin {
    Root,
    Move {
        parent: GameMoveId,
        movement: BoardMove,
        san: String,
    },
}

#[derive(Debug, Clone)]
pub struct GameMoveNode {
    id: GameMoveId,
    origin: GameMoveNodeEnumOrigin,
    position: BoardPosition,
    children: Vec<GameMoveId>,
    annotations: GameMoveNodeAnnotations,
    clock: Option<ClockStateSnapshot>,
    before_clock: Option<ClockStateSnapshot>,
}

impl GameMoveNode {
    pub(crate) fn root(
        id: GameMoveId,
        position: BoardPosition,
        clock: Option<ClockStateSnapshot>,
    ) -> Self {
        Self {
            id,
            origin: GameMoveNodeEnumOrigin::Root,
            position,
            children: Vec::new(),
            annotations: GameMoveNodeAnnotations::default(),
            clock,
            before_clock: None,
        }
    }

    pub(crate) fn child(
        id: GameMoveId,
        parent: GameMoveId,
        movement: BoardMove,
        san: String,
        position: BoardPosition,
    ) -> Self {
        Self {
            id,
            origin: GameMoveNodeEnumOrigin::Move {
                parent,
                movement,
                san,
            },
            position,
            children: Vec::new(),
            annotations: GameMoveNodeAnnotations::default(),
            clock: None,
            before_clock: None,
        }
    }

    pub fn id(&self) -> GameMoveId {
        self.id
    }

    pub fn parent(&self) -> Option<GameMoveId> {
        match self.origin {
            GameMoveNodeEnumOrigin::Root => None,
            GameMoveNodeEnumOrigin::Move { parent, .. } => Some(parent),
        }
    }

    pub fn movement(&self) -> Option<BoardMove> {
        match self.origin {
            GameMoveNodeEnumOrigin::Root => None,
            GameMoveNodeEnumOrigin::Move { movement, .. } => Some(movement),
        }
    }

    pub fn san(&self) -> Option<&str> {
        match &self.origin {
            GameMoveNodeEnumOrigin::Root => None,
            GameMoveNodeEnumOrigin::Move { san, .. } => Some(san),
        }
    }

    pub fn position(&self) -> &BoardPosition {
        &self.position
    }

    pub fn children(&self) -> &[GameMoveId] {
        &self.children
    }

    pub fn annotations(&self) -> &GameMoveNodeAnnotations {
        &self.annotations
    }

    pub fn clock(&self) -> Option<&ClockStateSnapshot> {
        self.clock.as_ref()
    }

    pub fn before_clock(&self) -> Option<&ClockStateSnapshot> {
        self.before_clock.as_ref()
    }

    pub(crate) fn children_mut(&mut self) -> &mut Vec<GameMoveId> {
        &mut self.children
    }

    pub(crate) fn set_clock(&mut self, clock: Option<ClockStateSnapshot>) {
        self.clock = clock;
    }

    pub(crate) fn set_before_clock(&mut self, clock: Option<ClockStateSnapshot>) {
        self.before_clock = clock;
    }

    pub(crate) fn set_annotations(&mut self, annotations: GameMoveNodeAnnotations) {
        self.annotations = annotations;
    }
}
