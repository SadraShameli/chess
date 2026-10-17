use crate::game::enums::GameId;
use crate::game::library_organization::{
    GameLibraryOrganizationFolderId, GameLibraryOrganizationGameTarget,
    GameLibraryOrganizationRevision, GameLibraryOrganizationTimestamp,
};
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_repository::{StorageLibraryCatalogRevision, StorageLibraryError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StorageTrashEnumLocation {
    #[default]
    Library,
    Trash,
}

impl StorageTrashEnumLocation {
    pub(crate) const fn sql(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Trash => "trash",
        }
    }
    pub(crate) fn parse(value: &str) -> Result<Self, StorageLibraryError> {
        match value {
            "library" => Ok(Self::Library),
            "trash" => Ok(Self::Trash),
            _ => Err(StorageLibraryError::StoredDiscriminant),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageTrashPreviewRequest {
    RemoveFolder(GameLibraryOrganizationFolderId),
    MoveToTrash(GameId),
    PermanentlyDelete(GameId),
}

#[derive(Debug, PartialEq, Eq)]
pub struct StorageTrashPlan {
    action: StorageTrashPreviewRequest,
    catalog: StorageLibraryCatalogRevision,
    content: Option<GameStorageRevision>,
    organization: GameLibraryOrganizationRevision,
    folders: u64,
    games: u64,
}

pub(crate) struct StorageTrashPlanOptions {
    pub(crate) action: StorageTrashPreviewRequest,
    pub(crate) catalog: StorageLibraryCatalogRevision,
    pub(crate) content: Option<GameStorageRevision>,
    pub(crate) organization: GameLibraryOrganizationRevision,
    pub(crate) folders: u64,
    pub(crate) games: u64,
}

impl StorageTrashPlan {
    pub(crate) const fn new(options: StorageTrashPlanOptions) -> Self {
        Self {
            action: options.action,
            catalog: options.catalog,
            content: options.content,
            organization: options.organization,
            folders: options.folders,
            games: options.games,
        }
    }
    pub const fn action(&self) -> StorageTrashPreviewRequest {
        self.action
    }
    pub const fn catalog_revision(&self) -> StorageLibraryCatalogRevision {
        self.catalog
    }
    pub const fn content_revision(&self) -> Option<GameStorageRevision> {
        self.content
    }
    pub const fn organization_revision(&self) -> GameLibraryOrganizationRevision {
        self.organization
    }
    pub const fn affected_folder_count(&self) -> u64 {
        self.folders
    }
    pub const fn affected_game_count(&self) -> u64 {
        self.games
    }
    pub(crate) fn approve(self) -> StorageTrashApprovedPlan {
        StorageTrashApprovedPlan(self)
    }
}

pub struct StorageTrashApprovedPlan(StorageTrashPlan);

#[derive(Debug)]
pub struct StorageTrashClosedGameEvidence {
    id: GameId,
    revision: GameStorageRevision,
}

impl StorageTrashClosedGameEvidence {
    pub(crate) const fn new(id: GameId, revision: GameStorageRevision) -> Self {
        Self { id, revision }
    }
    pub const fn id(&self) -> GameId {
        self.id
    }
    pub const fn content_revision(&self) -> GameStorageRevision {
        self.revision
    }
}

pub struct StorageTrashConfirmedRequest {
    approved: StorageTrashApprovedPlan,
    timestamp: GameLibraryOrganizationTimestamp,
    closed: Option<StorageTrashClosedGameEvidence>,
}

impl StorageTrashConfirmedRequest {
    pub fn new(
        approved: StorageTrashApprovedPlan,
        timestamp: GameLibraryOrganizationTimestamp,
        closed: Option<StorageTrashClosedGameEvidence>,
    ) -> Result<Self, StorageLibraryError> {
        match approved.0.action {
            StorageTrashPreviewRequest::MoveToTrash(id)
            | StorageTrashPreviewRequest::PermanentlyDelete(id) => {
                let evidence = closed
                    .as_ref()
                    .ok_or(StorageLibraryError::LifecycleRequired)?;
                if evidence.id != id || Some(evidence.revision) != approved.0.content {
                    return Err(StorageLibraryError::LifecycleRequired);
                }
            }
            StorageTrashPreviewRequest::RemoveFolder(_) => {
                if closed.is_some() {
                    return Err(StorageLibraryError::LifecycleRequired);
                }
            }
        }
        Ok(Self {
            approved,
            timestamp,
            closed,
        })
    }
    pub fn plan(&self) -> &StorageTrashPlan {
        &self.approved.0
    }
    pub const fn timestamp(&self) -> GameLibraryOrganizationTimestamp {
        self.timestamp
    }
    pub fn closed_game(&self) -> Option<&StorageTrashClosedGameEvidence> {
        self.closed.as_ref()
    }
}

#[derive(Debug)]
pub struct StorageTrashRestoreRequest {
    target: GameLibraryOrganizationGameTarget,
    timestamp: GameLibraryOrganizationTimestamp,
}

impl StorageTrashRestoreRequest {
    pub const fn new(
        target: GameLibraryOrganizationGameTarget,
        timestamp: GameLibraryOrganizationTimestamp,
    ) -> Self {
        Self { target, timestamp }
    }
    pub const fn target(&self) -> GameLibraryOrganizationGameTarget {
        self.target
    }
    pub const fn timestamp(&self) -> GameLibraryOrganizationTimestamp {
        self.timestamp
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageTrashEnumAppliedAction {
    RemovedFolder(GameLibraryOrganizationFolderId),
    Trashed(GameId),
    Restored(GameId),
    Deleted(GameId),
}

#[derive(Debug)]
pub struct StorageTrashReceipt {
    action: StorageTrashEnumAppliedAction,
    catalog: StorageLibraryCatalogRevision,
    affected_games: u64,
}

impl StorageTrashReceipt {
    pub(crate) const fn new(
        action: StorageTrashEnumAppliedAction,
        catalog: StorageLibraryCatalogRevision,
        affected_games: u64,
    ) -> Self {
        Self {
            action,
            catalog,
            affected_games,
        }
    }
    pub const fn action(&self) -> StorageTrashEnumAppliedAction {
        self.action
    }
    pub const fn catalog_revision(&self) -> StorageLibraryCatalogRevision {
        self.catalog
    }
    pub const fn affected_game_count(&self) -> u64 {
        self.affected_games
    }
}
