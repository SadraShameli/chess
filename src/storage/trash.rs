use crate::game::enums::GameId;
use crate::game::library_organization::{
    GameLibraryOrganizationFolderId, GameLibraryOrganizationGameTarget,
    GameLibraryOrganizationRevision, GameLibraryOrganizationTimestamp,
};
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_repository::{StorageLibraryCatalogRevision, StorageLibraryError};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    strum::EnumString,
    strum::AsRefStr,
    strum::Display,
    diesel::AsExpression,
    diesel::FromSqlRow,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
pub enum StorageTrashEnumLocation {
    #[default]
    #[strum(serialize = "library")]
    Library,
    #[strum(serialize = "trash")]
    Trash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageTrashPreviewRequest {
    RemoveFolder(GameLibraryOrganizationFolderId),
    MoveToTrash(GameId),
    PermanentlyDelete(GameId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub fn is_advanced_by_close(&self, refreshed: &Self, closed: GameStorageRevision) -> bool {
        refreshed.action == self.action
            && refreshed.organization == self.organization
            && refreshed.folders == self.folders
            && refreshed.games == self.games
            && refreshed.content == Some(closed)
            && self.content.is_some_and(|content| content < closed)
    }
    pub(crate) fn approve(self) -> StorageTrashApprovedPlan {
        StorageTrashApprovedPlan(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageTrashApprovedPlan(StorageTrashPlan);

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
