use thiserror::Error;

use crate::game::enums::{GameEnumOutcomeReason, GameEnumResult, GameMoveId};

#[derive(Debug, Error)]
pub enum GameOutcomeError {
    #[error("The game result is inconsistent with its termination reason")]
    InconsistentReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameOutcome {
    result: GameEnumResult,
    reason: GameEnumOutcomeReason,
    origin: GameMoveId,
}

impl GameOutcome {
    pub fn new(
        result: GameEnumResult,
        reason: GameEnumOutcomeReason,
        origin: GameMoveId,
    ) -> Result<Self, GameOutcomeError> {
        if !reason.permits(result) {
            return Err(GameOutcomeError::InconsistentReason);
        }
        Ok(Self {
            result,
            reason,
            origin,
        })
    }

    pub fn result(&self) -> GameEnumResult {
        self.result
    }

    pub fn reason(&self) -> GameEnumOutcomeReason {
        self.reason
    }

    pub fn origin(&self) -> GameMoveId {
        self.origin
    }
}
