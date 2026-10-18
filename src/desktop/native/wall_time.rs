use std::time::{SystemTime, UNIX_EPOCH};

use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::storage_repository::{GameStorageWallTimeError, GameStorageWallTimeProvider};

pub struct DesktopNativeWallTime;

impl GameStorageWallTimeProvider for DesktopNativeWallTime {
    fn now(&self) -> Result<GameLibraryOrganizationTimestamp, GameStorageWallTimeError> {
        let elapsed = SystemTime::now().duration_since(UNIX_EPOCH)?;
        Ok(GameLibraryOrganizationTimestamp::new(i64::try_from(
            elapsed.as_millis(),
        )?)?)
    }
}
