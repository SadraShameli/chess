use thiserror::Error;

use crate::game::enums::{GameId, GameRevision};
use crate::game::save::{GameSaveEnumAcknowledgment, GameSaveEnumStatus};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::EnumString,
    strum::IntoStaticStr,
    strum::Display,
    strum::EnumIter,
)]
pub enum GameSaveCoordinatorEnumBarrier {
    #[strum(serialize = "close this game")]
    CloseGame,
    #[strum(serialize = "quit the application")]
    QuitApplication,
    #[strum(serialize = "move this game to Trash")]
    MoveToTrash,
}

impl GameSaveCoordinatorEnumBarrier {
    pub fn label(self) -> &'static str {
        self.into()
    }
}

#[derive(Debug, Error)]
pub enum GameSaveCoordinatorError {
    #[error("Save the latest changes before you {}", .barrier.label())]
    UnsavedChanges {
        barrier: GameSaveCoordinatorEnumBarrier,
        revision: GameRevision,
    },
    #[error("The save receipt belongs to game {received:?}, expected {expected:?}")]
    ForeignGame { expected: GameId, received: GameId },
    #[error("The save receipt revision {received} is newer than memory revision {memory}")]
    FutureRevision {
        memory: GameRevision,
        received: GameRevision,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSaveCoordinator {
    game: GameId,
    memory: GameRevision,
    durable: Option<GameRevision>,
    failure: Option<String>,
}

impl GameSaveCoordinator {
    pub const fn new(game: GameId, memory: GameRevision) -> Self {
        Self {
            game,
            memory,
            durable: None,
            failure: None,
        }
    }

    pub const fn game(&self) -> GameId {
        self.game
    }

    pub const fn memory_revision(&self) -> GameRevision {
        self.memory
    }

    pub const fn durable_revision(&self) -> Option<GameRevision> {
        self.durable
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub fn observe_memory(&mut self, revision: GameRevision) {
        if revision > self.memory {
            self.memory = revision;
        }
    }

    pub fn status(&self) -> GameSaveEnumStatus {
        if self.failure.is_none() && self.durable == Some(self.memory) {
            GameSaveEnumStatus::Current
        } else {
            GameSaveEnumStatus::Required
        }
    }

    pub fn newest_to_save(&self) -> GameRevision {
        self.memory
    }

    pub fn record_failure(&mut self, message: String) {
        self.failure = Some(message);
    }

    pub fn acknowledge(
        &mut self,
        game: GameId,
        revision: GameRevision,
    ) -> Result<GameSaveEnumAcknowledgment, GameSaveCoordinatorError> {
        if game != self.game {
            return Err(GameSaveCoordinatorError::ForeignGame {
                expected: self.game,
                received: game,
            });
        }
        if revision > self.memory {
            return Err(GameSaveCoordinatorError::FutureRevision {
                memory: self.memory,
                received: revision,
            });
        }
        if self.durable.is_none_or(|durable| revision > durable) {
            self.durable = Some(revision);
        }
        if revision == self.memory {
            self.failure = None;
            Ok(GameSaveEnumAcknowledgment::Current)
        } else {
            Ok(GameSaveEnumAcknowledgment::Older)
        }
    }

    pub fn require_saved(
        &self,
        barrier: GameSaveCoordinatorEnumBarrier,
    ) -> Result<(), GameSaveCoordinatorError> {
        match self.status() {
            GameSaveEnumStatus::Current => Ok(()),
            GameSaveEnumStatus::Required => Err(GameSaveCoordinatorError::UnsavedChanges {
                barrier,
                revision: self.memory,
            }),
        }
    }
}
