use thiserror::Error;

use crate::clock::duration::ClockInstant;
use crate::game::engine_provider::{GameEngineContext, GameEngineQuiescenceReceipt};
use crate::game::enums::{GameEnumEngineReadiness, GameEnumMode, GameId, GameRevision};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamePauseEnumFault {
    Engine,
    Graphics,
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamePauseEnumCause {
    Manual,
    ComputerSleep,
    Relaunch,
    TabClosed,
    ApplicationQuit,
    UpdateInstall,
    Fault(GamePauseEnumFault),
}

#[derive(Debug, Error)]
pub enum GamePauseError {
    #[error("The engine must be observed idle or stopped before changing this game")]
    EngineBarrierRequired,
    #[error("A local game must not carry an engine barrier")]
    UnexpectedEngineBarrier,
    #[error("The engine barrier belongs to a different game revision")]
    StaleEngineBarrier {
        expected: GameEngineContext,
        received: GameEngineContext,
    },
    #[error("Resolve the interrupted resource through explicit Retry before resuming")]
    RecoveryRequired(GamePauseEnumFault),
    #[error("The reported recovery does not match the pending fault")]
    WrongRecovery,
}

#[derive(Debug)]
pub enum GamePauseEnumEngineBarrier {
    NoEngine,
    Quiescent(GameEngineQuiescenceReceipt),
    FailedEngine,
}

impl GamePauseEnumEngineBarrier {
    pub(super) fn validate(
        self,
        game: GameId,
        revision: GameRevision,
        mode: GameEnumMode,
        cause: Option<GamePauseEnumCause>,
    ) -> Result<(), GamePauseError> {
        match (mode, self) {
            (GameEnumMode::LocalTwoPlayer, Self::NoEngine) => Ok(()),
            (GameEnumMode::LocalTwoPlayer, Self::Quiescent(_) | Self::FailedEngine) => {
                Err(GamePauseError::UnexpectedEngineBarrier)
            }
            (GameEnumMode::HumanVersusEngine, Self::Quiescent(receipt)) => {
                let expected = GameEngineContext::new(game, revision);
                let received = receipt.context();
                if received == expected {
                    Ok(())
                } else {
                    Err(GamePauseError::StaleEngineBarrier { expected, received })
                }
            }
            (GameEnumMode::HumanVersusEngine, Self::FailedEngine)
                if cause == Some(GamePauseEnumCause::Fault(GamePauseEnumFault::Engine)) =>
            {
                Ok(())
            }
            (GameEnumMode::HumanVersusEngine, Self::NoEngine | Self::FailedEngine) => {
                Err(GamePauseError::EngineBarrierRequired)
            }
        }
    }
}

pub struct GamePauseRequest {
    pub expected_revision: GameRevision,
    pub cause: GamePauseEnumCause,
    pub received_at: ClockInstant,
    pub engine: GamePauseEnumEngineBarrier,
}

pub struct GameResumeRequest {
    pub expected_revision: GameRevision,
    pub received_at: ClockInstant,
    pub readiness: GameEnumEngineReadiness,
}

#[cfg(test)]
#[path = "../../tests/game/engine_barrier.rs"]
mod tests;
