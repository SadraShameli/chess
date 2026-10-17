use crate::game::enums::GameId;
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::library_repository::{
    StorageLibraryCatalogRevision, StorageLibraryError, StorageLibraryMemory,
};
use crate::storage::library_transaction::{
    StorageLibraryEnumTransactionRequest, StorageLibraryEnumTransactionResponse,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageLibraryInventoryCursor {
    after: Option<GameId>,
}

impl StorageLibraryInventoryCursor {
    pub const fn start() -> Self {
        Self { after: None }
    }

    pub const fn after(id: GameId) -> Self {
        Self { after: Some(id) }
    }

    pub(crate) const fn boundary(self) -> Option<GameId> {
        self.after
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageLibraryInventoryEntry {
    id: GameId,
    revision: GameStorageRevision,
}

impl StorageLibraryInventoryEntry {
    pub(crate) const fn new(id: GameId, revision: GameStorageRevision) -> Self {
        Self { id, revision }
    }

    pub const fn id(self) -> GameId {
        self.id
    }

    pub const fn content_revision(self) -> GameStorageRevision {
        self.revision
    }
}

pub struct StorageLibraryInventoryPage {
    entries: Vec<StorageLibraryInventoryEntry>,
    next: Option<StorageLibraryInventoryCursor>,
    catalog: StorageLibraryCatalogRevision,
    _memory: StorageLibraryMemory,
}

impl StorageLibraryInventoryPage {
    pub const PAGE_SIZE: usize = crate::storage::library_query::StorageLibraryQuery::PAGE_SIZE;

    pub(crate) fn new(
        entries: Vec<StorageLibraryInventoryEntry>,
        next: Option<StorageLibraryInventoryCursor>,
        catalog: StorageLibraryCatalogRevision,
        memory: StorageLibraryMemory,
    ) -> Self {
        Self {
            entries,
            next,
            catalog,
            _memory: memory,
        }
    }

    pub fn entries(&self) -> &[StorageLibraryInventoryEntry] {
        &self.entries
    }

    pub const fn next_cursor(&self) -> Option<StorageLibraryInventoryCursor> {
        self.next
    }

    pub const fn catalog_revision(&self) -> StorageLibraryCatalogRevision {
        self.catalog
    }
}

pub struct StorageLibraryInventoryRepository {
    client: StorageDocumentClient,
}

impl StorageLibraryInventoryRepository {
    pub const fn new(client: StorageDocumentClient) -> Self {
        Self { client }
    }

    pub async fn page(
        &self,
        cursor: StorageLibraryInventoryCursor,
    ) -> Result<StorageLibraryInventoryPage, StorageLibraryError> {
        let response = self
            .client
            .begin_library(StorageLibraryEnumTransactionRequest::PlayedInventory(
                cursor,
            ))?
            .wait()
            .await?;
        let StorageLibraryEnumTransactionResponse::PlayedInventory(page) = response else {
            return Err(StorageLibraryError::Protocol);
        };
        Ok(page)
    }
}
