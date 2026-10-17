use std::sync::Arc;

use rusqlite::{Row, Rows, Transaction, params};
use tokio::sync::Semaphore;

use crate::game::library_organization::{
    GameLibraryOrganizationFolderId, GameLibraryOrganizationName,
};
use crate::storage::library_metadata::{
    StorageLibraryMetadataEnumRequest, StorageLibraryMetadataEnumResponse,
    StorageLibraryMetadataFolder, StorageLibraryMetadataFolderCursor,
    StorageLibraryMetadataFolderPage, StorageLibraryMetadataFolderRead,
    StorageLibraryMetadataFolderSnapshot, StorageLibraryMetadataPage,
    StorageLibraryMetadataRetained, StorageLibraryMetadataTag, StorageLibraryMetadataTagCursor,
    StorageLibraryMetadataTagPage,
};
use crate::storage::library_repository::{
    StorageLibraryCatalogRevision, StorageLibraryError, StorageLibraryMemory,
};
use crate::storage::library_transaction::StorageLibraryTransaction;

pub(super) struct StorageLibraryTransactionMetadata;

impl StorageLibraryTransactionMetadata {
    pub(super) fn execute(
        transaction: &Transaction<'_>,
        request: &StorageLibraryMetadataEnumRequest,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryMetadataEnumResponse, StorageLibraryError> {
        let catalog = StorageLibraryTransaction::catalog(transaction)?;
        match *request {
            StorageLibraryMetadataEnumRequest::Folders { parent, cursor } => {
                if let Some(cursor) = cursor {
                    cursor.validate(catalog, parent)?;
                }
                Self::folders(transaction, catalog, parent, cursor, memory)
                    .map(StorageLibraryMetadataEnumResponse::Folders)
            }
            StorageLibraryMetadataEnumRequest::Tags { cursor } => {
                if let Some(cursor) = cursor {
                    cursor.validate(catalog)?;
                }
                Self::tags(transaction, catalog, cursor, memory)
                    .map(StorageLibraryMetadataEnumResponse::Tags)
            }
            StorageLibraryMetadataEnumRequest::Folder {
                id,
                catalog: expected,
            } => {
                StorageLibraryMetadataEnumRequest::validate_catalog(expected, catalog)?;
                Self::folder(transaction, catalog, id, memory)
                    .map(StorageLibraryMetadataEnumResponse::Folder)
            }
        }
    }

    fn folders(
        transaction: &Transaction<'_>,
        catalog: StorageLibraryCatalogRevision,
        parent: Option<GameLibraryOrganizationFolderId>,
        cursor: Option<StorageLibraryMetadataFolderCursor>,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryMetadataFolderPage, StorageLibraryError> {
        let reservation = StorageLibraryMemory::new(
            memory,
            StorageLibraryMetadataPage::<
                StorageLibraryMetadataFolder,
                StorageLibraryMetadataFolderCursor,
            >::reservation_bytes(StorageLibraryMetadataEnumRequest::PAGE_SIZE)?,
        )?;
        if let Some(parent) = parent {
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM library_folders WHERE folder_id = ?1)",
                [parent.to_string()],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(StorageLibraryError::MissingFolder(parent));
            }
        }
        let sql = if cursor.is_some() {
            "SELECT folder_id, parent_id, name, revision FROM library_folders WHERE parent_id IS ?1 AND folder_id > ?2 ORDER BY folder_id LIMIT ?3"
        } else {
            "SELECT folder_id, parent_id, name, revision FROM library_folders WHERE parent_id IS ?1 ORDER BY folder_id LIMIT ?2"
        };
        let mut statement = transaction.prepare(sql)?;
        let parent_key = parent.map(|id| id.to_string());
        let limit = Self::sql_limit()?;
        let mut rows = match cursor {
            Some(cursor) => {
                statement.query(params![parent_key, cursor.after().to_string(), limit])?
            }
            None => statement.query(params![parent_key, limit])?,
        };
        let (entries, has_more) = Self::entries(&mut rows, Self::folder_row)?;
        let next = if has_more {
            entries
                .last()
                .map(|folder| StorageLibraryMetadataFolderCursor::new(catalog, parent, folder.id()))
        } else {
            None
        };
        Ok(StorageLibraryMetadataFolderPage::new(
            entries,
            next,
            catalog,
            reservation,
        ))
    }

    fn tags(
        transaction: &Transaction<'_>,
        catalog: StorageLibraryCatalogRevision,
        cursor: Option<StorageLibraryMetadataTagCursor>,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryMetadataTagPage, StorageLibraryError> {
        let reservation =
            StorageLibraryMemory::new(
                memory,
                StorageLibraryMetadataPage::<
                    StorageLibraryMetadataTag,
                    StorageLibraryMetadataTagCursor,
                >::reservation_bytes(StorageLibraryMetadataEnumRequest::PAGE_SIZE)?,
            )?;
        let sql = if cursor.is_some() {
            "SELECT tag_id, name, revision FROM library_tags WHERE tag_id > ?1 ORDER BY tag_id LIMIT ?2"
        } else {
            "SELECT tag_id, name, revision FROM library_tags ORDER BY tag_id LIMIT ?1"
        };
        let mut statement = transaction.prepare(sql)?;
        let limit = Self::sql_limit()?;
        let mut rows = match cursor {
            Some(cursor) => statement.query(params![cursor.after().to_string(), limit])?,
            None => statement.query([limit])?,
        };
        let (entries, has_more) = Self::entries(&mut rows, Self::tag_row)?;
        let next = if has_more {
            entries
                .last()
                .map(|tag| StorageLibraryMetadataTagCursor::new(catalog, tag.id()))
        } else {
            None
        };
        Ok(StorageLibraryMetadataTagPage::new(
            entries,
            next,
            catalog,
            reservation,
        ))
    }

    fn folder(
        transaction: &Transaction<'_>,
        catalog: StorageLibraryCatalogRevision,
        id: GameLibraryOrganizationFolderId,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryMetadataFolderRead, StorageLibraryError> {
        let reservation = StorageLibraryMemory::new(
            memory,
            StorageLibraryMetadataRetained::<StorageLibraryMetadataFolderSnapshot>::reservation_bytes(
                1, 0,
            )?,
        )?;
        let mut statement = transaction.prepare(
            "SELECT folder_id, parent_id, name, revision FROM library_folders WHERE folder_id = ?1",
        )?;
        let mut rows = statement.query([id.to_string()])?;
        let row = rows.next()?.ok_or(StorageLibraryError::MissingFolder(id))?;
        Ok(StorageLibraryMetadataFolderRead::new(
            Self::folder_row(row)?,
            catalog,
            reservation,
        ))
    }

    fn sql_limit() -> Result<i64, StorageLibraryError> {
        Ok(i64::try_from(
            StorageLibraryMetadataEnumRequest::PAGE_SIZE
                .checked_add(1)
                .ok_or(crate::storage::document_store::StorageDocumentError::DocumentTooLarge)?,
        )?)
    }

    fn entries<T>(
        rows: &mut Rows<'_>,
        parse: impl Fn(&Row<'_>) -> Result<T, StorageLibraryError>,
    ) -> Result<(Vec<T>, bool), StorageLibraryError> {
        let mut entries = Vec::with_capacity(StorageLibraryMetadataEnumRequest::PAGE_SIZE);
        while let Some(row) = rows.next()? {
            if entries.len() == StorageLibraryMetadataEnumRequest::PAGE_SIZE {
                return Ok((entries, true));
            }
            entries.push(parse(row)?);
        }
        Ok((entries, false))
    }

    fn folder_row(row: &Row<'_>) -> Result<StorageLibraryMetadataFolder, StorageLibraryError> {
        Ok(StorageLibraryMetadataFolder::new(
            Self::text(row, 0)?.parse()?,
            row.get_ref(1)?
                .as_str_or_null()
                .map_err(rusqlite::Error::from)?
                .map(str::parse)
                .transpose()?,
            Self::name(row, 2)?,
            Self::text(row, 3)?.parse()?,
        ))
    }

    fn tag_row(row: &Row<'_>) -> Result<StorageLibraryMetadataTag, StorageLibraryError> {
        Ok(StorageLibraryMetadataTag::new(
            Self::text(row, 0)?.parse()?,
            Self::name(row, 1)?,
            Self::text(row, 2)?.parse()?,
        ))
    }

    fn text<'row>(row: &'row Row<'_>, column: usize) -> Result<&'row str, StorageLibraryError> {
        Ok(row
            .get_ref(column)?
            .as_str()
            .map_err(rusqlite::Error::from)?)
    }

    fn name(
        row: &Row<'_>,
        column: usize,
    ) -> Result<GameLibraryOrganizationName, StorageLibraryError> {
        let value = Self::text(row, column)?;
        if value.len() > GameLibraryOrganizationName::MAX_BYTES {
            return Err(
                crate::game::library_organization::GameLibraryOrganizationError::Name.into(),
            );
        }
        Ok(GameLibraryOrganizationName::new(value.to_owned())?)
    }
}
