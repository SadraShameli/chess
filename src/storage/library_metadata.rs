use std::sync::Arc;

use tokio::sync::OwnedSemaphorePermit;

use crate::game::library_organization::{
    GameLibraryOrganizationFolderId, GameLibraryOrganizationName, GameLibraryOrganizationRevision,
    GameLibraryOrganizationTagId,
};
use crate::storage::document_store::{StorageDocumentClient, StorageDocumentError};
use crate::storage::library_query::StorageLibraryQuery;
use crate::storage::library_repository::{
    StorageLibraryCatalogRevision, StorageLibraryError, StorageLibraryMemory, StorageLibraryPending,
};
use crate::storage::library_transaction::{
    StorageLibraryEnumTransactionRequest, StorageLibraryEnumTransactionResponse,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLibraryMetadataFolder {
    id: GameLibraryOrganizationFolderId,
    parent: Option<GameLibraryOrganizationFolderId>,
    name: GameLibraryOrganizationName,
    revision: GameLibraryOrganizationRevision,
}

impl StorageLibraryMetadataFolder {
    pub(crate) const fn new(
        id: GameLibraryOrganizationFolderId,
        parent: Option<GameLibraryOrganizationFolderId>,
        name: GameLibraryOrganizationName,
        revision: GameLibraryOrganizationRevision,
    ) -> Self {
        Self {
            id,
            parent,
            name,
            revision,
        }
    }

    pub const fn id(&self) -> GameLibraryOrganizationFolderId {
        self.id
    }

    pub const fn parent(&self) -> Option<GameLibraryOrganizationFolderId> {
        self.parent
    }

    pub const fn name(&self) -> &GameLibraryOrganizationName {
        &self.name
    }

    pub const fn revision(&self) -> GameLibraryOrganizationRevision {
        self.revision
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLibraryMetadataTag {
    id: GameLibraryOrganizationTagId,
    name: GameLibraryOrganizationName,
    revision: GameLibraryOrganizationRevision,
}

impl StorageLibraryMetadataTag {
    pub(crate) const fn new(
        id: GameLibraryOrganizationTagId,
        name: GameLibraryOrganizationName,
        revision: GameLibraryOrganizationRevision,
    ) -> Self {
        Self { id, name, revision }
    }

    pub const fn id(&self) -> GameLibraryOrganizationTagId {
        self.id
    }

    pub const fn name(&self) -> &GameLibraryOrganizationName {
        &self.name
    }

    pub const fn revision(&self) -> GameLibraryOrganizationRevision {
        self.revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageLibraryMetadataFolderCursor {
    catalog: StorageLibraryCatalogRevision,
    parent: Option<GameLibraryOrganizationFolderId>,
    after: GameLibraryOrganizationFolderId,
}

impl StorageLibraryMetadataFolderCursor {
    pub(crate) const fn new(
        catalog: StorageLibraryCatalogRevision,
        parent: Option<GameLibraryOrganizationFolderId>,
        after: GameLibraryOrganizationFolderId,
    ) -> Self {
        Self {
            catalog,
            parent,
            after,
        }
    }

    pub(crate) fn validate(
        self,
        catalog: StorageLibraryCatalogRevision,
        parent: Option<GameLibraryOrganizationFolderId>,
    ) -> Result<(), StorageLibraryError> {
        if self.parent != parent {
            return Err(StorageLibraryError::CursorQuery);
        }
        StorageLibraryMetadataEnumRequest::validate_catalog(self.catalog, catalog)
    }

    pub(crate) const fn after(self) -> GameLibraryOrganizationFolderId {
        self.after
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageLibraryMetadataTagCursor {
    catalog: StorageLibraryCatalogRevision,
    after: GameLibraryOrganizationTagId,
}

impl StorageLibraryMetadataTagCursor {
    pub(crate) const fn new(
        catalog: StorageLibraryCatalogRevision,
        after: GameLibraryOrganizationTagId,
    ) -> Self {
        Self { catalog, after }
    }

    pub(crate) fn validate(
        self,
        catalog: StorageLibraryCatalogRevision,
    ) -> Result<(), StorageLibraryError> {
        StorageLibraryMetadataEnumRequest::validate_catalog(self.catalog, catalog)
    }

    pub(crate) const fn after(self) -> GameLibraryOrganizationTagId {
        self.after
    }
}

pub(crate) struct StorageLibraryMetadataRetained<T> {
    value: T,
    _memory: StorageLibraryMemory,
}

impl<T> StorageLibraryMetadataRetained<T> {
    pub(crate) const fn new(value: T, memory: StorageLibraryMemory) -> Self {
        Self {
            value,
            _memory: memory,
        }
    }

    pub(crate) fn reservation_bytes(
        names: usize,
        rows_bytes: usize,
    ) -> Result<usize, StorageLibraryError> {
        names
            .checked_add(1)
            .and_then(|count| count.checked_mul(GameLibraryOrganizationName::MAX_BYTES))
            .and_then(|bytes| bytes.checked_add(rows_bytes))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Self>()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Arc<Self>>()))
            .and_then(|bytes| bytes.checked_add(2 * std::mem::size_of::<usize>()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<OwnedSemaphorePermit>()))
            .and_then(|bytes| bytes.checked_add(3 * uuid::fmt::Hyphenated::LENGTH))
            .and_then(|bytes| bytes.checked_add(3 * std::mem::size_of::<u64>()))
            .ok_or(StorageLibraryError::Store(
                StorageDocumentError::DocumentTooLarge,
            ))
    }
}

pub(crate) struct StorageLibraryMetadataPage<T, C> {
    entries: Vec<T>,
    next: Option<C>,
    catalog: StorageLibraryCatalogRevision,
}

impl<T, C> StorageLibraryMetadataPage<T, C> {
    pub(crate) const fn new(
        entries: Vec<T>,
        next: Option<C>,
        catalog: StorageLibraryCatalogRevision,
    ) -> Self {
        Self {
            entries,
            next,
            catalog,
        }
    }

    pub(crate) fn reservation_bytes(count: usize) -> Result<usize, StorageLibraryError> {
        let rows_bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or(StorageDocumentError::DocumentTooLarge)?;
        StorageLibraryMetadataRetained::<Self>::reservation_bytes(count, rows_bytes)
    }
}

#[derive(Clone)]
pub struct StorageLibraryMetadataFolderPage {
    retained: Arc<
        StorageLibraryMetadataRetained<
            StorageLibraryMetadataPage<
                StorageLibraryMetadataFolder,
                StorageLibraryMetadataFolderCursor,
            >,
        >,
    >,
}

impl StorageLibraryMetadataFolderPage {
    pub(crate) fn new(
        entries: Vec<StorageLibraryMetadataFolder>,
        next: Option<StorageLibraryMetadataFolderCursor>,
        catalog: StorageLibraryCatalogRevision,
        memory: StorageLibraryMemory,
    ) -> Self {
        Self {
            retained: Arc::new(StorageLibraryMetadataRetained::new(
                StorageLibraryMetadataPage::new(entries, next, catalog),
                memory,
            )),
        }
    }

    pub fn entries(&self) -> &[StorageLibraryMetadataFolder] {
        &self.retained.value.entries
    }

    pub fn next_cursor(&self) -> Option<&StorageLibraryMetadataFolderCursor> {
        self.retained.value.next.as_ref()
    }

    pub fn catalog_revision(&self) -> StorageLibraryCatalogRevision {
        self.retained.value.catalog
    }
}

#[derive(Clone)]
pub struct StorageLibraryMetadataTagPage {
    retained: Arc<
        StorageLibraryMetadataRetained<
            StorageLibraryMetadataPage<StorageLibraryMetadataTag, StorageLibraryMetadataTagCursor>,
        >,
    >,
}

impl StorageLibraryMetadataTagPage {
    pub(crate) fn new(
        entries: Vec<StorageLibraryMetadataTag>,
        next: Option<StorageLibraryMetadataTagCursor>,
        catalog: StorageLibraryCatalogRevision,
        memory: StorageLibraryMemory,
    ) -> Self {
        Self {
            retained: Arc::new(StorageLibraryMetadataRetained::new(
                StorageLibraryMetadataPage::new(entries, next, catalog),
                memory,
            )),
        }
    }

    pub fn entries(&self) -> &[StorageLibraryMetadataTag] {
        &self.retained.value.entries
    }

    pub fn next_cursor(&self) -> Option<&StorageLibraryMetadataTagCursor> {
        self.retained.value.next.as_ref()
    }

    pub fn catalog_revision(&self) -> StorageLibraryCatalogRevision {
        self.retained.value.catalog
    }
}

pub(crate) struct StorageLibraryMetadataFolderSnapshot {
    folder: StorageLibraryMetadataFolder,
    catalog: StorageLibraryCatalogRevision,
}

#[derive(Clone)]
pub struct StorageLibraryMetadataFolderRead {
    retained: Arc<StorageLibraryMetadataRetained<StorageLibraryMetadataFolderSnapshot>>,
}

impl StorageLibraryMetadataFolderRead {
    pub(crate) fn new(
        folder: StorageLibraryMetadataFolder,
        catalog: StorageLibraryCatalogRevision,
        memory: StorageLibraryMemory,
    ) -> Self {
        Self {
            retained: Arc::new(StorageLibraryMetadataRetained::new(
                StorageLibraryMetadataFolderSnapshot { folder, catalog },
                memory,
            )),
        }
    }

    pub fn folder(&self) -> &StorageLibraryMetadataFolder {
        &self.retained.value.folder
    }

    pub fn catalog_revision(&self) -> StorageLibraryCatalogRevision {
        self.retained.value.catalog
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageLibraryMetadataEnumRequest {
    Folders {
        parent: Option<GameLibraryOrganizationFolderId>,
        cursor: Option<StorageLibraryMetadataFolderCursor>,
    },
    Tags {
        cursor: Option<StorageLibraryMetadataTagCursor>,
    },
    Folder {
        id: GameLibraryOrganizationFolderId,
        catalog: StorageLibraryCatalogRevision,
    },
}

impl StorageLibraryMetadataEnumRequest {
    pub const PAGE_SIZE: usize = StorageLibraryQuery::PAGE_SIZE;

    pub(crate) fn validate_catalog(
        expected: StorageLibraryCatalogRevision,
        actual: StorageLibraryCatalogRevision,
    ) -> Result<(), StorageLibraryError> {
        if expected != actual {
            return Err(StorageLibraryError::StaleCursor);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub enum StorageLibraryMetadataEnumResponse {
    Folders(StorageLibraryMetadataFolderPage),
    Tags(StorageLibraryMetadataTagPage),
    Folder(StorageLibraryMetadataFolderRead),
}

pub struct StorageLibraryMetadataRead {
    pending: StorageLibraryPending,
    request: StorageLibraryMetadataEnumRequest,
}

impl StorageLibraryMetadataRead {
    pub async fn wait(
        &mut self,
    ) -> Result<StorageLibraryMetadataEnumResponse, StorageLibraryError> {
        let StorageLibraryEnumTransactionResponse::Metadata(response) = self.pending.wait().await?
        else {
            return Err(StorageLibraryError::Protocol);
        };
        match (&self.request, &response) {
            (
                StorageLibraryMetadataEnumRequest::Folders { .. },
                StorageLibraryMetadataEnumResponse::Folders(_),
            )
            | (
                StorageLibraryMetadataEnumRequest::Tags { .. },
                StorageLibraryMetadataEnumResponse::Tags(_),
            )
            | (
                StorageLibraryMetadataEnumRequest::Folder { .. },
                StorageLibraryMetadataEnumResponse::Folder(_),
            ) => Ok(response),
            (
                StorageLibraryMetadataEnumRequest::Folders { .. },
                StorageLibraryMetadataEnumResponse::Tags(_)
                | StorageLibraryMetadataEnumResponse::Folder(_),
            )
            | (
                StorageLibraryMetadataEnumRequest::Tags { .. },
                StorageLibraryMetadataEnumResponse::Folders(_)
                | StorageLibraryMetadataEnumResponse::Folder(_),
            )
            | (
                StorageLibraryMetadataEnumRequest::Folder { .. },
                StorageLibraryMetadataEnumResponse::Folders(_)
                | StorageLibraryMetadataEnumResponse::Tags(_),
            ) => Err(StorageLibraryError::Protocol),
        }
    }
}

pub struct StorageLibraryMetadataRepository {
    client: StorageDocumentClient,
}

impl StorageLibraryMetadataRepository {
    pub const fn new(client: StorageDocumentClient) -> Self {
        Self { client }
    }

    pub fn begin(
        &self,
        request: StorageLibraryMetadataEnumRequest,
    ) -> Result<StorageLibraryMetadataRead, StorageLibraryError> {
        Ok(StorageLibraryMetadataRead {
            request,
            pending: self
                .client
                .begin_library(StorageLibraryEnumTransactionRequest::Metadata(request))?,
        })
    }

    pub async fn folders(
        &self,
        parent: Option<GameLibraryOrganizationFolderId>,
        cursor: Option<StorageLibraryMetadataFolderCursor>,
    ) -> Result<StorageLibraryMetadataFolderPage, StorageLibraryError> {
        let response = self
            .begin(StorageLibraryMetadataEnumRequest::Folders { parent, cursor })?
            .wait()
            .await?;
        let StorageLibraryMetadataEnumResponse::Folders(page) = response else {
            return Err(StorageLibraryError::Protocol);
        };
        Ok(page)
    }

    pub async fn tags(
        &self,
        cursor: Option<StorageLibraryMetadataTagCursor>,
    ) -> Result<StorageLibraryMetadataTagPage, StorageLibraryError> {
        let response = self
            .begin(StorageLibraryMetadataEnumRequest::Tags { cursor })?
            .wait()
            .await?;
        let StorageLibraryMetadataEnumResponse::Tags(page) = response else {
            return Err(StorageLibraryError::Protocol);
        };
        Ok(page)
    }

    pub async fn folder(
        &self,
        id: GameLibraryOrganizationFolderId,
        catalog: StorageLibraryCatalogRevision,
    ) -> Result<StorageLibraryMetadataFolderRead, StorageLibraryError> {
        let response = self
            .begin(StorageLibraryMetadataEnumRequest::Folder { id, catalog })?
            .wait()
            .await?;
        let StorageLibraryMetadataEnumResponse::Folder(folder) = response else {
            return Err(StorageLibraryError::Protocol);
        };
        Ok(folder)
    }
}
