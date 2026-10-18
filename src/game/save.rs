use thiserror::Error;

use crate::clock::duration::ClockInstant;
use crate::game::enums::{GameId, GameRevision};
use crate::game::pause::GamePauseEnumEngineBarrier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSaveEnumStatus {
    Current,
    Required,
}

impl GameSaveEnumStatus {
    pub(super) fn validate(self) -> Result<(), GameSaveError> {
        match self {
            Self::Current => Ok(()),
            Self::Required => Err(GameSaveError::SaveRequired),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSaveEnumAcknowledgment {
    Current,
    Older,
}

#[derive(Debug, Error)]
pub enum GameSaveError {
    #[error("Retry Save must successfully save this game before further game changes")]
    SaveRequired,
    #[error("The save receipt belongs to game {received:?}, expected {expected:?}")]
    ForeignGame { expected: GameId, received: GameId },
    #[error("The save receipt revision {received} is newer than current revision {current}")]
    FutureRevision {
        current: GameRevision,
        received: GameRevision,
    },
}

pub struct GameSaveFailureRequest {
    pub expected_revision: GameRevision,
    pub received_at: ClockInstant,
    pub engine: GamePauseEnumEngineBarrier,
}

#[cfg(test)]
#[path = "../../tests/game/save_receipt.rs"]
mod tests;
