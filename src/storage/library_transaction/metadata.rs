use std::sync::Arc;

use diesel::connection::DefaultLoadingMode;
use diesel::dsl::exists;
use diesel::expression::SqlLiteral;
use diesel::sql_types::{Bool, Nullable, Text};
use diesel::sqlite::SqliteConnection;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, Queryable, RunQueryDsl};
use tokio::sync::Semaphore;

use crate::game::library_organization::{
    GameLibraryOrganizationError, GameLibraryOrganizationFolderId, GameLibraryOrganizationName,
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
use crate::storage::library_transaction::enums::StorageLibraryTransactionMetadataEnumText;
use crate::storage::schema::{library_folders, library_tags};

#[derive(Queryable)]
struct StorageLibraryTransactionMetadataFolderRow {
    id: Option<String>,
    parent: Option<String>,
    revision: Option<String>,
    name: Option<String>,
    parent_is_bounded: bool,
}

type StorageLibraryTransactionMetadataFolderSelection = (
    SqlLiteral<Nullable<Text>>,
    SqlLiteral<Nullable<Text>>,
    SqlLiteral<Nullable<Text>>,
    SqlLiteral<Nullable<Text>>,
    SqlLiteral<Bool>,
);

impl StorageLibraryTransactionMetadataFolderRow {
    fn selection() -> StorageLibraryTransactionMetadataFolderSelection {
        (
            StorageLibraryTransactionMetadataEnumText::FolderId.bounded(),
            StorageLibraryTransactionMetadataEnumText::ParentId.bounded(),
            StorageLibraryTransactionMetadataEnumText::Revision.bounded(),
            StorageLibraryTransactionMetadataEnumText::Name.bounded(),
            StorageLibraryTransactionMetadataEnumText::ParentId.is_bounded(),
        )
    }
}

#[derive(Queryable)]
struct StorageLibraryTransactionMetadataTagRow {
    id: Option<String>,
    revision: Option<String>,
    name: Option<String>,
}

pub(super) struct StorageLibraryTransactionMetadata;

impl StorageLibraryTransactionMetadata {
    const DECODING_BYTES: usize = 4 * GameLibraryOrganizationName::MAX_BYTES
        + std::mem::size_of::<StorageLibraryTransactionMetadataFolderRow>();

    pub(super) fn execute(
        transaction: &mut SqliteConnection,
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
        transaction: &mut SqliteConnection,
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
        let _decoding = StorageLibraryMemory::new(memory, Self::DECODING_BYTES)?;
        let mut query = library_folders::table.into_boxed::<diesel::sqlite::Sqlite>();
        if let Some(parent) = parent {
            let found: bool =
                diesel::select(exists(library_folders::table.find(parent.to_string())))
                    .get_result(transaction)?;
            if !found {
                return Err(StorageLibraryError::MissingFolder(parent));
            }
            query = query.filter(library_folders::parent_id.eq(parent.to_string()));
        } else {
            query = query.filter(library_folders::parent_id.is_null());
        }
        if let Some(cursor) = cursor {
            query = query.filter(library_folders::folder_id.gt(cursor.after().to_string()));
        }
        let rows = query
            .order(library_folders::folder_id.asc())
            .limit(Self::sql_limit()?)
            .select(StorageLibraryTransactionMetadataFolderRow::selection())
            .load_iter::<StorageLibraryTransactionMetadataFolderRow, DefaultLoadingMode>(
                transaction,
            )?;
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
        transaction: &mut SqliteConnection,
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
        let _decoding = StorageLibraryMemory::new(memory, Self::DECODING_BYTES)?;
        let mut query = library_tags::table.into_boxed::<diesel::sqlite::Sqlite>();
        if let Some(cursor) = cursor {
            query = query.filter(library_tags::tag_id.gt(cursor.after().to_string()));
        }
        let rows = query
            .order(library_tags::tag_id.asc())
            .limit(Self::sql_limit()?)
            .select((
                StorageLibraryTransactionMetadataEnumText::TagId.bounded(),
                StorageLibraryTransactionMetadataEnumText::Revision.bounded(),
                StorageLibraryTransactionMetadataEnumText::Name.bounded(),
            ))
            .load_iter::<StorageLibraryTransactionMetadataTagRow, DefaultLoadingMode>(
                transaction,
            )?;
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
        transaction: &mut SqliteConnection,
        catalog: StorageLibraryCatalogRevision,
        id: GameLibraryOrganizationFolderId,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryMetadataFolderRead, StorageLibraryError> {
        let reservation = StorageLibraryMemory::new(
            memory,
            StorageLibraryMetadataRetained::<StorageLibraryMetadataFolderSnapshot>::reservation_bytes(1, 0)?,
        )?;
        let _decoding = StorageLibraryMemory::new(memory, Self::DECODING_BYTES)?;
        let row = library_folders::table
            .find(id.to_string())
            .select(StorageLibraryTransactionMetadataFolderRow::selection())
            .first::<StorageLibraryTransactionMetadataFolderRow>(transaction)
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

    fn entries<T, U>(
        rows: impl Iterator<Item = diesel::QueryResult<T>>,
        parse: impl Fn(T) -> Result<U, StorageLibraryError>,
    ) -> Result<(Vec<U>, bool), StorageLibraryError> {
        let mut entries = Vec::with_capacity(StorageLibraryMetadataEnumRequest::PAGE_SIZE);
        for row in rows {
            let row = row?;
            if entries.len() == StorageLibraryMetadataEnumRequest::PAGE_SIZE {
                return Ok((entries, true));
            }
            entries.push(parse(row)?);
        }
        Ok((entries, false))
    }

    fn folder_row(
        row: StorageLibraryTransactionMetadataFolderRow,
    ) -> Result<StorageLibraryMetadataFolder, StorageLibraryError> {
        if !row.parent_is_bounded {
            return Err(StorageLibraryError::StoredDiscriminant);
        }
        Ok(StorageLibraryMetadataFolder::new(
            row.id
                .ok_or(StorageLibraryError::StoredDiscriminant)?
                .parse()?,
            row.parent.as_deref().map(str::parse).transpose()?,
            Self::name(row.name)?,
            row.revision
                .ok_or(StorageLibraryError::StoredDiscriminant)?
                .parse()?,
        ))
    }

    fn tag_row(
        row: StorageLibraryTransactionMetadataTagRow,
    ) -> Result<StorageLibraryMetadataTag, StorageLibraryError> {
        Ok(StorageLibraryMetadataTag::new(
            row.id
                .ok_or(StorageLibraryError::StoredDiscriminant)?
                .parse()?,
            Self::name(row.name)?,
            row.revision
                .ok_or(StorageLibraryError::StoredDiscriminant)?
                .parse()?,
        ))
    }

    fn name(value: Option<String>) -> Result<GameLibraryOrganizationName, StorageLibraryError> {
        Ok(GameLibraryOrganizationName::new(
            value.ok_or(GameLibraryOrganizationError::Name)?,
        )?)
    }
}
