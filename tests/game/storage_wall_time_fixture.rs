use std::sync::Arc;

use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::storage_repository::{GameStorageWallTimeError, GameStorageWallTimeProvider};

pub(crate) struct GameStorageWallTimeFixture;

impl GameStorageWallTimeFixture {
    const UTC_MILLIS: i64 = 1_000;

    pub(crate) fn shared() -> Arc<dyn GameStorageWallTimeProvider> {
        Arc::new(Self)
    }
}

impl GameStorageWallTimeProvider for GameStorageWallTimeFixture {
    fn now(&self) -> Result<GameLibraryOrganizationTimestamp, GameStorageWallTimeError> {
        Ok(GameLibraryOrganizationTimestamp::new(Self::UTC_MILLIS)?)
    }
}
