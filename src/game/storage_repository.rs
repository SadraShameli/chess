use std::future::Future;
use std::num::TryFromIntError;
use std::panic::RefUnwindSafe;
use std::sync::Arc;
use std::time::SystemTimeError;

use thiserror::Error;

use crate::clock::duration::ClockInstant;
use crate::game::enums::{GameId, GameRevision};
use crate::game::library_organization::{
    GameLibraryOrganizationError, GameLibraryOrganizationTimestamp,
};
use crate::game::session::{GameSession, GameSessionError, GameSessionRestoreOptions};

#[derive(Debug, Error)]
pub enum GameStorageWallTimeError {
    #[error(transparent)]
    Clock(#[from] SystemTimeError),
    #[error(transparent)]
    Timestamp(#[from] GameLibraryOrganizationError),
    #[error(transparent)]
    Range(#[from] TryFromIntError),
    #[error("The game content-change wall clock is unavailable")]
    Unavailable,
}

pub trait GameStorageWallTimeProvider: Send + Sync + RefUnwindSafe {
    fn now(&self) -> Result<GameLibraryOrganizationTimestamp, GameStorageWallTimeError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStorageEnumEditCause {
    Created,
    AcceptedMove,
    Result,
    Takeback,
    StudyTree,
    Annotation,
    Metadata,
}

#[derive(Debug, Error)]
pub enum GameStorageContentEditError {
    #[error("The accepted content edit revision {accepted} exceeds covered revision {covered}")]
    FutureRevision {
        accepted: GameRevision,
        covered: GameRevision,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameStorageContentEdit {
    revision: GameRevision,
    cause: GameStorageEnumEditCause,
    timestamp: GameLibraryOrganizationTimestamp,
}

impl GameStorageContentEdit {
    pub fn new(
        accepted_revision: GameRevision,
        covered_revision: GameRevision,
        cause: GameStorageEnumEditCause,
        timestamp: GameLibraryOrganizationTimestamp,
    ) -> Result<Self, GameStorageContentEditError> {
        let edit = Self::from_accepted_revision(accepted_revision, cause, timestamp);
        edit.validate_covered_revision(covered_revision)?;
        Ok(edit)
    }

    pub(crate) const fn from_accepted_revision(
        revision: GameRevision,
        cause: GameStorageEnumEditCause,
        timestamp: GameLibraryOrganizationTimestamp,
    ) -> Self {
        Self {
            revision,
            cause,
            timestamp,
        }
    }

    pub const fn revision(self) -> GameRevision {
        self.revision
    }

    pub const fn cause(self) -> GameStorageEnumEditCause {
        self.cause
    }

    pub const fn timestamp(self) -> GameLibraryOrganizationTimestamp {
        self.timestamp
    }

    fn validate_covered_revision(
        self,
        covered: GameRevision,
    ) -> Result<(), GameStorageContentEditError> {
        if self.revision > covered {
            return Err(GameStorageContentEditError::FutureRevision {
                accepted: self.revision,
                covered,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStorageEnumContentChange {
    Checkpoint,
    Changed(GameStorageContentEdit),
}

#[derive(Debug, Error)]
pub enum GameStorageSaveRequestError {
    #[error(transparent)]
    Session(#[from] GameSessionError),
    #[error(transparent)]
    ContentEdit(#[from] GameStorageContentEditError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameStorageRevision(u64);

impl GameStorageRevision {
    pub const INITIAL: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone)]
pub struct GameStorageSaveRequest {
    snapshot: Arc<GameSessionRestoreOptions>,
    expected_storage_revision: GameStorageRevision,
    content_change: GameStorageEnumContentChange,
}

impl GameStorageSaveRequest {
    pub fn capture(
        session: &GameSession,
        now: ClockInstant,
        expected_storage_revision: GameStorageRevision,
    ) -> Result<Self, GameSessionError> {
        Ok(Self {
            snapshot: Arc::new(session.restoration(now)?),
            expected_storage_revision,
            content_change: GameStorageEnumContentChange::Checkpoint,
        })
    }

    pub fn capture_with_change(
        session: &GameSession,
        now: ClockInstant,
        expected_storage_revision: GameStorageRevision,
        change: GameStorageEnumContentChange,
    ) -> Result<Self, GameStorageSaveRequestError> {
        let mut request = Self::capture(session, now, expected_storage_revision)?;
        if let GameStorageEnumContentChange::Changed(edit) = change {
            edit.validate_covered_revision(request.game_revision())?;
        }
        request.content_change = change;
        Ok(request)
    }

    pub const fn content_change(&self) -> GameStorageEnumContentChange {
        self.content_change
    }

    pub fn game(&self) -> GameId {
        self.snapshot.session.id
    }

    pub fn game_revision(&self) -> GameRevision {
        self.snapshot.revision
    }

    pub const fn expected_storage_revision(&self) -> GameStorageRevision {
        self.expected_storage_revision
    }

    pub(crate) fn expect_storage_revision(mut self, revision: GameStorageRevision) -> Self {
        self.expected_storage_revision = revision;
        self
    }

    pub(crate) fn snapshot(&self) -> &GameSessionRestoreOptions {
        &self.snapshot
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameStorageReceipt {
    game: GameId,
    game_revision: GameRevision,
    storage_revision: GameStorageRevision,
}

impl GameStorageReceipt {
    pub(crate) const fn new(
        game: GameId,
        game_revision: GameRevision,
        storage_revision: GameStorageRevision,
    ) -> Self {
        Self {
            game,
            game_revision,
            storage_revision,
        }
    }

    pub const fn game(self) -> GameId {
        self.game
    }

    pub const fn game_revision(self) -> GameRevision {
        self.game_revision
    }

    pub const fn storage_revision(self) -> GameStorageRevision {
        self.storage_revision
    }
}

pub struct GameStorageRecovered {
    session: GameSession,
    storage_revision: GameStorageRevision,
}

impl GameStorageRecovered {
    pub(crate) const fn new(session: GameSession, storage_revision: GameStorageRevision) -> Self {
        Self {
            session,
            storage_revision,
        }
    }

    pub const fn session(&self) -> &GameSession {
        &self.session
    }

    pub const fn storage_revision(&self) -> GameStorageRevision {
        self.storage_revision
    }

    pub fn into_session(self) -> GameSession {
        self.session
    }
}

pub trait GameStorageRepository: Sync {
    type Error: std::error::Error + Send + Sync;

    fn save(
        &self,
        request: GameStorageSaveRequest,
    ) -> impl Future<Output = Result<GameStorageReceipt, Self::Error>> + Send;
    fn load(
        &self,
        game: GameId,
    ) -> impl Future<Output = Result<Option<GameStorageRecovered>, Self::Error>> + Send;
}
