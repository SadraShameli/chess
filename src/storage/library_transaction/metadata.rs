use std::sync::Arc;

use diesel::SqliteConnection;
use diesel::prelude::*;
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
use crate::storage::schema::{library_folders, library_tags};

type StorageLibraryMetadataFolderRow = (String, Option<String>, String, String);
type StorageLibraryMetadataTagRow = (String, String, String);

pub(super) struct StorageLibraryTransactionMetadata;

impl StorageLibraryTransactionMetadata {
    pub(super) fn execute(
        connection: &mut SqliteConnection,
        request: &StorageLibraryMetadataEnumRequest,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryMetadataEnumResponse, StorageLibraryError> {
        let catalog = StorageLibraryTransaction::diesel_catalog(connection)?;
        match *request {
            StorageLibraryMetadataEnumRequest::Folders { parent, cursor } => {
                if let Some(cursor) = cursor {
                    cursor.validate(catalog, parent)?;
                }
                Self::folders(connection, catalog, parent, cursor, memory)
                    .map(StorageLibraryMetadataEnumResponse::Folders)
            }
            StorageLibraryMetadataEnumRequest::Tags { cursor } => {
                if let Some(cursor) = cursor {
                    cursor.validate(catalog)?;
                }
                Self::tags(connection, catalog, cursor, memory)
                    .map(StorageLibraryMetadataEnumResponse::Tags)
            }
            StorageLibraryMetadataEnumRequest::Folder {
                id,
                catalog: expected,
            } => {
                StorageLibraryMetadataEnumRequest::validate_catalog(expected, catalog)?;
                Self::folder(connection, catalog, id, memory)
                    .map(StorageLibraryMetadataEnumResponse::Folder)
            }
        }
    }

    fn folders(
        connection: &mut SqliteConnection,
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
            let exists: bool = diesel::select(diesel::dsl::exists(
                library_folders::table.filter(library_folders::folder_id.eq(parent.to_string())),
            ))
            .get_result(connection)?;
            if !exists {
                return Err(StorageLibraryError::MissingFolder(parent));
            }
        }
        let mut query = library_folders::table
            .select((
                library_folders::folder_id,
                library_folders::parent_id,
                library_folders::name,
                library_folders::revision,
            ))
            .order(library_folders::folder_id.asc())
            .limit(Self::sql_limit()?)
            .into_boxed();
        query = match parent {
            Some(parent) => query.filter(library_folders::parent_id.eq(parent.to_string())),
            None => query.filter(library_folders::parent_id.is_null()),
        };
        if let Some(cursor) = cursor {
            query = query.filter(library_folders::folder_id.gt(cursor.after().to_string()));
        }
        let rows: Vec<StorageLibraryMetadataFolderRow> = query.load(connection)?;
        let (entries, has_more) = Self::entries(rows, Self::folder_row)?;
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
        connection: &mut SqliteConnection,
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
        let mut query = library_tags::table
            .select((
                library_tags::tag_id,
                library_tags::name,
                library_tags::revision,
            ))
            .order(library_tags::tag_id.asc())
            .limit(Self::sql_limit()?)
            .into_boxed();
        if let Some(cursor) = cursor {
            query = query.filter(library_tags::tag_id.gt(cursor.after().to_string()));
        }
        let rows: Vec<StorageLibraryMetadataTagRow> = query.load(connection)?;
        let (entries, has_more) = Self::entries(rows, Self::tag_row)?;
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
        connection: &mut SqliteConnection,
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
        let row: StorageLibraryMetadataFolderRow = library_folders::table
            .filter(library_folders::folder_id.eq(id.to_string()))
            .select((
                library_folders::folder_id,
                library_folders::parent_id,
                library_folders::name,
                library_folders::revision,
            ))
            .first(connection)
            .optional()?
            .ok_or(StorageLibraryError::MissingFolder(id))?;
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

    fn entries<R, T>(
        rows: Vec<R>,
        parse: impl Fn(R) -> Result<T, StorageLibraryError>,
    ) -> Result<(Vec<T>, bool), StorageLibraryError> {
        let mut entries = Vec::with_capacity(StorageLibraryMetadataEnumRequest::PAGE_SIZE);
        for row in rows {
            if entries.len() == StorageLibraryMetadataEnumRequest::PAGE_SIZE {
                return Ok((entries, true));
            }
            entries.push(parse(row)?);
        }
        Ok((entries, false))
    }

    fn folder_row(
        (id, parent, name, revision): StorageLibraryMetadataFolderRow,
    ) -> Result<StorageLibraryMetadataFolder, StorageLibraryError> {
        Ok(StorageLibraryMetadataFolder::new(
            id.parse()?,
            parent.as_deref().map(str::parse).transpose()?,
            Self::name(name)?,
            revision.parse()?,
        ))
    }

    fn tag_row(
        (id, name, revision): StorageLibraryMetadataTagRow,
    ) -> Result<StorageLibraryMetadataTag, StorageLibraryError> {
        Ok(StorageLibraryMetadataTag::new(
            id.parse()?,
            Self::name(name)?,
            revision.parse()?,
        ))
    }

    fn name(value: String) -> Result<GameLibraryOrganizationName, StorageLibraryError> {
        if value.len() > GameLibraryOrganizationName::MAX_BYTES {
            return Err(
                crate::game::library_organization::GameLibraryOrganizationError::Name.into(),
            );
        }
        Ok(GameLibraryOrganizationName::new(value)?)
    }
}
