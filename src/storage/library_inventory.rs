use crate::game::enums::GameId;
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::library_repository::{
    StorageLibraryAdoptionPage, StorageLibraryCatalogRevision, StorageLibraryError,
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
    transport: StorageLibraryAdoptionPage,
}

impl StorageLibraryInventoryPage {
    pub const PAGE_SIZE: usize = crate::storage::library_query::StorageLibraryQuery::PAGE_SIZE;

    fn from_transport(transport: StorageLibraryAdoptionPage) -> Result<Self, StorageLibraryError> {
        let mut entries = Vec::with_capacity(transport.candidates().len());
        for candidate in transport.candidates() {
            entries.push(StorageLibraryInventoryEntry {
                id: candidate.id().to_string().parse()?,
                revision: candidate.content_revision(),
            });
        }
        let next = if transport.next_cursor().is_some() {
            entries
                .last()
                .map(|entry| StorageLibraryInventoryCursor::after(entry.id()))
        } else {
            None
        };
        Ok(Self {
            entries,
            next,
            transport,
        })
    }

    pub fn entries(&self) -> &[StorageLibraryInventoryEntry] {
        &self.entries
    }

    pub const fn next_cursor(&self) -> Option<StorageLibraryInventoryCursor> {
        self.next
    }

    pub const fn catalog_revision(&self) -> StorageLibraryCatalogRevision {
        self.transport.catalog_revision()
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
        let StorageLibraryEnumTransactionResponse::AdoptionPage(page) = response else {
            return Err(StorageLibraryError::Protocol);
        };
        StorageLibraryInventoryPage::from_transport(page)
    }
}
