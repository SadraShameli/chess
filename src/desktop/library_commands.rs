use thiserror::Error;

use crate::game::enums::GameId;
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_repository::StorageLibraryError;
use crate::storage::trash::{
    StorageTrashClosedGameEvidence, StorageTrashConfirmedRequest, StorageTrashPlan,
    StorageTrashPreviewRequest,
};

pub trait DesktopLibraryCommandsLifecycleProvider {
    type Error: std::error::Error + Send + Sync + 'static;

    fn closed_revision(&self, game: GameId) -> Result<Option<GameStorageRevision>, Self::Error>;
}

#[derive(Debug, Error)]
pub enum DesktopLibraryCommandsError<E: std::error::Error + 'static> {
    #[error("Library confirmation is invalid: {0}")]
    Storage(#[source] Box<StorageLibraryError>),
    #[error("Library lifecycle observation failed: {0}")]
    Lifecycle(#[source] E),
}

impl<E: std::error::Error + 'static> From<StorageLibraryError> for DesktopLibraryCommandsError<E> {
    fn from(error: StorageLibraryError) -> Self {
        Self::Storage(Box::new(error))
    }
}

pub struct DesktopLibraryCommandsConfirmation {
    plan: StorageTrashPlan,
}

impl DesktopLibraryCommandsConfirmation {
    pub const fn confirm(plan: StorageTrashPlan) -> Self {
        Self { plan }
    }

    pub fn prepare<P: DesktopLibraryCommandsLifecycleProvider>(
        self,
        lifecycle: &P,
        timestamp: GameLibraryOrganizationTimestamp,
    ) -> Result<StorageTrashConfirmedRequest, DesktopLibraryCommandsError<P::Error>> {
        let closed = match self.plan.action() {
            StorageTrashPreviewRequest::RemoveFolder(_) => None,
            StorageTrashPreviewRequest::MoveToTrash(id)
            | StorageTrashPreviewRequest::PermanentlyDelete(id) => {
                let observed = lifecycle
                    .closed_revision(id)
                    .map_err(DesktopLibraryCommandsError::Lifecycle)?;
                match (observed, self.plan.content_revision()) {
                    (Some(actual), Some(expected)) if actual == expected => {
                        Some(StorageTrashClosedGameEvidence::new(id, expected))
                    }
                    _ => return Err(StorageLibraryError::LifecycleRequired.into()),
                }
            }
        };
        Ok(StorageTrashConfirmedRequest::new(
            self.plan.approve(),
            timestamp,
            closed,
        )?)
    }
}

#[cfg(test)]
#[path = "../../tests/desktop/library_commands.rs"]
mod tests;
