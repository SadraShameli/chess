use std::future::Future;
use std::sync::Arc;

use crate::clock::duration::ClockInstant;
use crate::game::enums::{GameId, GameRevision};
use crate::game::session::{GameSession, GameSessionError, GameSessionRestoreOptions};

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
        })
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
