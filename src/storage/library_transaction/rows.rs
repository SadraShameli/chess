use std::sync::Arc;

use diesel::SqliteConnection;
use diesel::prelude::*;
use tokio::sync::Semaphore;

use crate::game::enums::GameId;
use crate::game::library_organization::{
    GameLibraryOrganizationFolderId, GameLibraryOrganizationRevision, GameLibraryOrganizationState,
    GameLibraryOrganizationTagSet, GameLibraryOrganizationTimestamp,
};
use crate::game::session::{GameMetadataDate, GameMetadataOpeningCode, GameMetadataText};
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::game_document::StorageGameDocumentEnumContentKind;
use crate::storage::library_inventory::{
    StorageLibraryInventoryCursor, StorageLibraryInventoryEntry, StorageLibraryInventoryPage,
};
use crate::storage::library_prepared::{StorageLibraryProjection, StorageLibraryProjectionOptions};
use crate::storage::library_query::{
    StorageLibraryQuery, StorageLibraryQueryEntry, StorageLibraryQueryEntryOptions,
    StorageLibraryQueryPage, StorageLibraryQueryRow,
};
use crate::storage::library_repository::{
    StorageLibraryError, StorageLibraryMemory, StorageLibraryStoredRecord,
    StorageLibraryStoredRecordOptions,
};
use crate::storage::library_transaction::StorageLibraryTransaction;
use crate::storage::schema::{documents, library_entries, library_entry_tags};
use crate::storage::trash::StorageTrashEnumLocation;

pub(super) struct StorageLibraryTransactionEntryState {
    content: GameStorageRevision,
    organization: GameLibraryOrganizationState,
    location: StorageTrashEnumLocation,
    modified: Option<GameLibraryOrganizationTimestamp>,
    kind: StorageGameDocumentEnumContentKind,
}

impl StorageLibraryTransactionEntryState {
    pub(super) const fn content(&self) -> GameStorageRevision {
        self.content
    }
    pub(super) fn organization(&self) -> &GameLibraryOrganizationState {
        &self.organization
    }
    pub(super) const fn location(&self) -> StorageTrashEnumLocation {
        self.location
    }
    pub(super) const fn modified(&self) -> Option<GameLibraryOrganizationTimestamp> {
        self.modified
    }
    pub(super) const fn kind(&self) -> StorageGameDocumentEnumContentKind {
        self.kind
    }
}

diesel::define_sql_function! {
    #[sql_name = "octet_length"]
    fn library_body_length(body: diesel::sql_types::Text) -> diesel::sql_types::BigInt;
}

pub(super) struct StorageLibraryTransactionRows;

impl StorageLibraryTransactionRows {
    const TAG_MEMORY: usize = GameLibraryOrganizationTagSet::MAX_TAGS
        * std::mem::size_of::<crate::game::library_organization::GameLibraryOrganizationTagId>();

    pub(super) fn document_revision(
        connection: &mut SqliteConnection,
        id: GameId,
    ) -> Result<Option<GameStorageRevision>, StorageLibraryError> {
        let value: Option<String> = documents::table
            .filter(documents::id.eq(id.as_uuid().to_string()))
            .select(documents::revision)
            .first(connection)
            .optional()?;
        value
            .as_deref()
            .map(StorageLibraryError::content_revision)
            .transpose()
    }
    pub(super) fn tags(
        connection: &mut SqliteConnection,
        id: GameId,
    ) -> Result<GameLibraryOrganizationTagSet, StorageLibraryError> {
        let values: Vec<String> = library_entry_tags::table
            .filter(library_entry_tags::game_id.eq(id.as_uuid().to_string()))
            .select(library_entry_tags::tag_id)
            .order(library_entry_tags::tag_id.asc())
            .limit(i64::try_from(GameLibraryOrganizationTagSet::MAX_TAGS + 1)?)
            .load(connection)?;
        let mut tags = Vec::with_capacity(values.len());
        for value in values {
            tags.push(value.parse()?);
        }
        Ok(GameLibraryOrganizationTagSet::new(tags)?)
    }
    pub(super) fn state(
        connection: &mut SqliteConnection,
        id: GameId,
    ) -> Result<Option<StorageLibraryTransactionEntryState>, StorageLibraryError> {
        type Row = (
            String,
            String,
            i32,
            Option<String>,
            StorageTrashEnumLocation,
            Option<i64>,
            StorageGameDocumentEnumContentKind,
            String,
            Option<i64>,
        );
        let row: Option<Row> = library_entries::table
            .inner_join(documents::table)
            .filter(library_entries::game_id.eq(id.as_uuid().to_string()))
            .select((
                library_entries::content_revision,
                library_entries::organization_revision,
                library_entries::favorite,
                library_entries::folder_id,
                library_entries::location,
                library_entries::modified_ms,
                library_entries::content_kind,
                documents::revision,
                library_entries::trashed_ms,
            ))
            .first(connection)
            .optional()?;
        let Some((
            revision,
            organization,
            favorite,
            folder,
            location,
            modified,
            kind,
            document,
            trashed,
        )) = row
        else {
            return Ok(None);
        };
        let content = StorageLibraryError::content_revision(&revision)?;
        if content != StorageLibraryError::content_revision(&document)? {
            return Err(StorageLibraryError::ProjectionStale);
        }
        Self::validate_location(location, trashed)?;
        let modified = modified
            .map(GameLibraryOrganizationTimestamp::new)
            .transpose()?;
        Ok(Some(StorageLibraryTransactionEntryState {
            content,
            organization: GameLibraryOrganizationState::new(
                organization.parse()?,
                Self::favorite(i64::from(favorite))?,
                folder
                    .as_deref()
                    .map(str::parse::<GameLibraryOrganizationFolderId>)
                    .transpose()?,
                Self::tags(connection, id)?,
            ),
            location,
            modified,
            kind,
        }))
    }
    fn favorite(value: i64) -> Result<bool, StorageLibraryError> {
        match value {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(StorageLibraryError::StoredDiscriminant),
        }
    }
    fn validate_location(
        location: StorageTrashEnumLocation,
        trashed: Option<i64>,
    ) -> Result<(), StorageLibraryError> {
        match (location, trashed) {
            (StorageTrashEnumLocation::Library, None) => Ok(()),
            (StorageTrashEnumLocation::Trash, Some(value)) => {
                GameLibraryOrganizationTimestamp::new(value)?;
                Ok(())
            }
            (StorageTrashEnumLocation::Library, Some(_))
            | (StorageTrashEnumLocation::Trash, None) => {
                Err(StorageLibraryError::StoredDiscriminant)
            }
        }
    }
    pub(super) fn load(
        connection: &mut SqliteConnection,
        id: GameId,
        memory: &Arc<Semaphore>,
    ) -> Result<Option<StorageLibraryStoredRecord>, StorageLibraryError> {
        let mut reservation = StorageLibraryMemory::new(
            memory,
            Self::TAG_MEMORY + std::mem::size_of::<StorageLibraryStoredRecord>(),
        )?;
        let Some(state) = Self::state(connection, id)? else {
            return Ok(None);
        };
        let length: i64 = documents::table
            .filter(documents::id.eq(id.as_uuid().to_string()))
            .select(library_body_length(documents::body))
            .first(connection)?;
        let length = usize::try_from(length)?;
        reservation.reserve(
            memory,
            length
                .checked_mul(4)
                .ok_or(StorageLibraryError::TextTooLarge)?,
        )?;
        let body: String = documents::table
            .filter(documents::id.eq(id.as_uuid().to_string()))
            .select(documents::body)
            .first(connection)?;
        Ok(Some(StorageLibraryStoredRecord::new(
            StorageLibraryStoredRecordOptions {
                id,
                body,
                revision: state.content,
                organization: state.organization,
                location: state.location,
                modified: state.modified,
                memory: reservation,
            },
        )))
    }
    fn row_bytes(row: &StorageLibraryQueryRow) -> Result<usize, StorageLibraryError> {
        let mut bytes = std::mem::size_of::<StorageLibraryQueryEntry>() + Self::TAG_MEMORY;
        for value in [
            &row.white_display,
            &row.white_key,
            &row.black_display,
            &row.black_key,
            &row.opening_display,
            &row.opening_key,
            &row.opening_code,
        ] {
            bytes = bytes
                .checked_add(
                    value
                        .as_ref()
                        .map_or(0, String::len)
                        .checked_mul(4)
                        .ok_or(StorageLibraryError::TextTooLarge)?,
                )
                .ok_or(StorageLibraryError::TextTooLarge)?;
        }
        Ok(bytes)
    }
    fn entry(
        connection: &mut SqliteConnection,
        row: StorageLibraryQueryRow,
        date_filtered: bool,
    ) -> Result<StorageLibraryQueryEntry, StorageLibraryError> {
        let id: GameId = row.game_id.parse()?;
        let revision = StorageLibraryError::content_revision(&row.content_revision)?;
        let organization: GameLibraryOrganizationRevision = row.organization_revision.parse()?;
        let white = row.white_display.map(GameMetadataText::new).transpose()?;
        let black = row.black_display.map(GameMetadataText::new).transpose()?;
        let date = GameMetadataDate::new(
            row.date_year.map(u16::try_from).transpose()?,
            row.date_month.map(u8::try_from).transpose()?,
            row.date_day.map(u8::try_from).transpose()?,
        )?;
        let projection = StorageLibraryProjection::new(StorageLibraryProjectionOptions {
            kind: row.content_kind,
            variant: row.rules_variant,
            start: row.start_kind,
            mode: row.mode,
            result: row.result.into(),
            white,
            black,
            date: Some(date),
            opening: row.opening_display.map(GameMetadataText::new).transpose()?,
            code: row
                .opening_code
                .map(GameMetadataOpeningCode::new)
                .transpose()?,
            plies: u32::try_from(row.plies)?,
        })?;
        if row.white_key.as_deref() != projection.white_key()
            || row.black_key.as_deref() != projection.black_key()
            || row.opening_key.as_deref() != projection.opening_key()
        {
            return Err(StorageLibraryError::StoredDiscriminant);
        }
        let favorite = Self::favorite(i64::from(row.favorite))?;
        let folder = row.folder_id.as_deref().map(str::parse).transpose()?;
        let location = row.location;
        Self::validate_location(location, row.trashed_ms)?;
        Ok(StorageLibraryQueryEntry::new(
            StorageLibraryQueryEntryOptions {
                id,
                content_revision: revision,
                projection,
                organization: GameLibraryOrganizationState::new(
                    organization,
                    favorite,
                    folder,
                    Self::tags(connection, id)?,
                ),
                modified: row
                    .modified_ms
                    .map(GameLibraryOrganizationTimestamp::new)
                    .transpose()?,
                location,
                date_filtered,
            },
        ))
    }
    pub(super) fn query(
        connection: &mut SqliteConnection,
        query: &StorageLibraryQuery,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryQueryPage, StorageLibraryError> {
        connection.transaction(|connection| {
            let catalog = StorageLibraryTransaction::diesel_catalog(connection)?;
            let mut reservation = StorageLibraryMemory::new(
                memory,
                query.transport_bytes()?
                    + StorageLibraryQuery::PAGE_SIZE
                        * std::mem::size_of::<StorageLibraryQueryEntry>(),
            )?;
            let rows = query.load(connection, catalog)?;
            let mut entries = Vec::with_capacity(StorageLibraryQuery::PAGE_SIZE);
            let mut has_more = false;
            for row in rows {
                if entries.len() == StorageLibraryQuery::PAGE_SIZE {
                    has_more = true;
                    break;
                }
                reservation.reserve(memory, Self::row_bytes(&row)?)?;
                entries.push(Self::entry(connection, row, query.is_date_filtered())?);
            }
            let next = if has_more {
                entries.last().map(|entry| query.cursor(catalog, entry))
            } else {
                None
            };
            Ok(StorageLibraryQueryPage::new(
                entries,
                next,
                catalog,
                reservation,
            ))
        })
    }
    pub(super) fn played_inventory(
        connection: &mut SqliteConnection,
        cursor: StorageLibraryInventoryCursor,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryInventoryPage, StorageLibraryError> {
        let reservation = StorageLibraryMemory::new(
            memory,
            StorageLibraryInventoryPage::PAGE_SIZE
                * std::mem::size_of::<StorageLibraryInventoryEntry>(),
        )?;
        let mut query = library_entries::table
            .inner_join(documents::table)
            .filter(library_entries::content_revision.eq(documents::revision))
            .filter(library_entries::content_kind.eq(StorageGameDocumentEnumContentKind::Played))
            .filter(library_entries::location.eq(StorageTrashEnumLocation::Library))
            .filter(library_entries::trashed_ms.is_null())
            .select((library_entries::game_id, library_entries::content_revision))
            .order(library_entries::game_id.asc())
            .limit(i64::try_from(StorageLibraryInventoryPage::PAGE_SIZE + 1)?)
            .into_boxed();
        if let Some(boundary) = cursor.boundary() {
            query = query.filter(library_entries::game_id.gt(boundary.as_uuid().to_string()));
        }
        let rows: Vec<(String, String)> = query.load(connection)?;
        let mut entries = Vec::with_capacity(StorageLibraryInventoryPage::PAGE_SIZE);
        let mut has_more = false;
        for (id, revision) in rows {
            if entries.len() == StorageLibraryInventoryPage::PAGE_SIZE {
                has_more = true;
                break;
            }
            entries.push(StorageLibraryInventoryEntry::new(
                id.parse()?,
                StorageLibraryError::content_revision(&revision)?,
            ));
        }
        let next = if has_more {
            entries
                .last()
                .map(|entry| StorageLibraryInventoryCursor::after(entry.id()))
        } else {
            None
        };
        Ok(StorageLibraryInventoryPage::new(
            entries,
            next,
            StorageLibraryTransaction::diesel_catalog(connection)?,
            reservation,
        ))
    }
}
