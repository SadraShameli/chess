use std::collections::HashMap;

use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::board::start::BoardStart;
use crate::clock::duration::ClockDuration;
use crate::game::enums::GameMoveId;
use crate::game::session::{GameMetadata, GameMetadataText};
use crate::game::tree::{GameTree, GameTreeError};
use crate::notation::game_identity::NotationGameIdentityEnumResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotationGameClock {
    node: GameMoveId,
    remaining: ClockDuration,
}

impl NotationGameClock {
    pub const fn new(node: GameMoveId, remaining: ClockDuration) -> Self {
        Self { node, remaining }
    }

    pub const fn node(self) -> GameMoveId {
        self.node
    }

    pub const fn remaining(self) -> ClockDuration {
        self.remaining
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotationGameEnumWarning {
    UnknownDirective {
        node: GameMoveId,
        directive: String,
    },
    UnavailableRating {
        color: BoardEnumColor,
        value: GameMetadataText,
    },
}

impl NotationGameEnumWarning {
    pub(crate) fn known_rating(value: &str) -> Option<u32> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        value.parse().ok()
    }
}

pub struct NotationGameParts {
    pub start: BoardStart,
    pub tree: GameTree,
    pub mainline_tip: GameMoveId,
    pub metadata: GameMetadata,
    pub result: NotationGameIdentityEnumResult,
    pub result_header: Option<NotationGameIdentityEnumResult>,
    pub time_control: Option<GameMetadataText>,
    pub clocks: Vec<NotationGameClock>,
    pub warnings: Vec<NotationGameEnumWarning>,
}

#[derive(Debug, Error)]
pub enum NotationGameError {
    #[error(transparent)]
    Tree(#[from] GameTreeError),
    #[error("The notation start differs from the immutable tree root")]
    StartMismatch,
    #[error("The Result header differs from the movetext result marker")]
    ResultMismatch,
    #[error("A notation node has multiple clock annotations")]
    DuplicateClock,
    #[error("An unavailable source rating conflicts with another rating for the same player")]
    ConflictingRating,
}

#[derive(Debug, Clone)]
pub struct NotationGame {
    start: BoardStart,
    tree: GameTree,
    mainline_tip: GameMoveId,
    metadata: GameMetadata,
    result: NotationGameIdentityEnumResult,
    result_header: Option<NotationGameIdentityEnumResult>,
    time_control: Option<GameMetadataText>,
    clocks: Vec<NotationGameClock>,
    clock_indices: HashMap<GameMoveId, usize>,
    warnings: Vec<NotationGameEnumWarning>,
}

impl NotationGame {
    pub fn new(parts: NotationGameParts) -> Result<Self, NotationGameError> {
        if parts.tree.node(parts.tree.root())?.position() != parts.start.position() {
            return Err(NotationGameError::StartMismatch);
        }
        parts.tree.path_to(parts.mainline_tip)?;
        if parts
            .result_header
            .is_some_and(|header| header != parts.result)
        {
            return Err(NotationGameError::ResultMismatch);
        }
        let mut clock_indices = HashMap::with_capacity(parts.clocks.len());
        for (index, clock) in parts.clocks.iter().enumerate() {
            parts.tree.node(clock.node())?;
            if clock_indices.insert(clock.node(), index).is_some() {
                return Err(NotationGameError::DuplicateClock);
            }
        }
        let mut rating_colors = Vec::new();
        for warning in &parts.warnings {
            match warning {
                NotationGameEnumWarning::UnknownDirective { node, .. } => {
                    parts.tree.node(*node)?;
                }
                NotationGameEnumWarning::UnavailableRating { color, value } => {
                    let known = match color {
                        BoardEnumColor::White => parts.metadata.known().white_elo,
                        BoardEnumColor::Black => parts.metadata.known().black_elo,
                    };
                    if known.is_some()
                        || rating_colors.contains(color)
                        || NotationGameEnumWarning::known_rating(value.as_str()).is_some()
                    {
                        return Err(NotationGameError::ConflictingRating);
                    }
                    rating_colors.push(*color);
                }
            }
        }
        Ok(Self {
            start: parts.start,
            tree: parts.tree,
            mainline_tip: parts.mainline_tip,
            metadata: parts.metadata,
            result: parts.result,
            result_header: parts.result_header,
            time_control: parts.time_control,
            clocks: parts.clocks,
            clock_indices,
            warnings: parts.warnings,
        })
    }

    pub fn start(&self) -> &BoardStart {
        &self.start
    }

    pub fn tree(&self) -> &GameTree {
        &self.tree
    }

    pub const fn mainline_tip(&self) -> GameMoveId {
        self.mainline_tip
    }

    pub fn metadata(&self) -> &GameMetadata {
        &self.metadata
    }

    pub const fn result(&self) -> NotationGameIdentityEnumResult {
        self.result
    }

    pub const fn result_header(&self) -> Option<NotationGameIdentityEnumResult> {
        self.result_header
    }

    pub fn time_control(&self) -> Option<&GameMetadataText> {
        self.time_control.as_ref()
    }

    pub fn clocks(&self) -> &[NotationGameClock] {
        &self.clocks
    }

    pub fn clock(&self, node: GameMoveId) -> Option<ClockDuration> {
        self.clock_indices
            .get(&node)
            .and_then(|index| self.clocks.get(*index))
            .map(|clock| clock.remaining())
    }

    pub fn warnings(&self) -> &[NotationGameEnumWarning] {
        &self.warnings
    }
}
