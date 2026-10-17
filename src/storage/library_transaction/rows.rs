use std::sync::Arc;

use rusqlite::{OptionalExtension, Row, Transaction, params, params_from_iter};
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
use crate::storage::library_prepared::{
    StorageLibraryEnumStartKind, StorageLibraryProjection, StorageLibraryProjectionOptions,
};
use crate::storage::library_query::{
    StorageLibraryQuery, StorageLibraryQueryEntry, StorageLibraryQueryEntryOptions,
    StorageLibraryQueryPage,
};
use crate::storage::library_repository::{
    StorageLibraryAdoptionCandidate, StorageLibraryAdoptionCursor, StorageLibraryAdoptionPage,
    StorageLibraryError, StorageLibraryMemory, StorageLibraryStoredRecord,
    StorageLibraryStoredRecordOptions,
};
use crate::storage::library_transaction::StorageLibraryTransaction;
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

pub(super) struct StorageLibraryTransactionRows;

impl StorageLibraryTransactionRows {
    const PROJECTION_COLUMNS: &str = "e.game_id, e.content_revision, e.organization_revision, e.content_kind, e.rules_variant, e.start_kind, e.mode, e.result, e.white_display, e.white_key, e.black_display, e.black_key, e.date_year, e.date_month, e.date_day, e.opening_display, e.opening_key, e.opening_code, e.plies, e.modified_ms, e.favorite, e.folder_id, e.location, e.trashed_ms";
    const TAG_MEMORY: usize = GameLibraryOrganizationTagSet::MAX_TAGS
        * std::mem::size_of::<crate::game::library_organization::GameLibraryOrganizationTagId>();

    pub(super) fn document_revision(
        transaction: &Transaction<'_>,
        id: GameId,
    ) -> Result<Option<GameStorageRevision>, StorageLibraryError> {
        let value: Option<String> = transaction
            .query_row(
                "SELECT revision FROM documents WHERE id = ?1",
                [id.as_uuid().to_string()],
                |row| row.get(0),
            )
            .optional()?;
        value
            .as_deref()
            .map(StorageLibraryError::content_revision)
            .transpose()
    }
    pub(super) fn tags(
        transaction: &Transaction<'_>,
        id: GameId,
    ) -> Result<GameLibraryOrganizationTagSet, StorageLibraryError> {
        let mut statement = transaction.prepare(
            "SELECT tag_id FROM library_entry_tags WHERE game_id = ?1 ORDER BY tag_id LIMIT ?2",
        )?;
        let mut rows = statement.query(params![
            id.as_uuid().to_string(),
            i64::try_from(GameLibraryOrganizationTagSet::MAX_TAGS + 1)?
        ])?;
        let mut tags = Vec::new();
        while let Some(row) = rows.next()? {
            let value: String = row.get(0)?;
            tags.push(value.parse()?);
        }
        Ok(GameLibraryOrganizationTagSet::new(tags)?)
    }
    pub(super) fn state(
        transaction: &Transaction<'_>,
        id: GameId,
    ) -> Result<Option<StorageLibraryTransactionEntryState>, StorageLibraryError> {
        let mut statement = transaction.prepare("SELECT e.content_revision, e.organization_revision, e.favorite, e.folder_id, e.location, e.modified_ms, e.content_kind, d.revision, e.trashed_ms FROM library_entries e JOIN documents d ON d.id = e.game_id WHERE e.game_id = ?1")?;
        let mut rows = statement.query([id.as_uuid().to_string()])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let revision: String = row.get(0)?;
        let document: String = row.get(7)?;
        let content = StorageLibraryError::content_revision(&revision)?;
        if content != StorageLibraryError::content_revision(&document)? {
            return Err(StorageLibraryError::ProjectionStale);
        }
        let organization: String = row.get(1)?;
        let favorite: i64 = row.get(2)?;
        let folder: Option<String> = row.get(3)?;
        let location: String = row.get(4)?;
        let location = StorageTrashEnumLocation::parse(&location)?;
        Self::validate_location(location, row.get(8)?)?;
        let modified = row
            .get::<_, Option<i64>>(5)?
            .map(GameLibraryOrganizationTimestamp::new)
            .transpose()?;
        let kind: String = row.get(6)?;
        Ok(Some(StorageLibraryTransactionEntryState {
            content,
            organization: GameLibraryOrganizationState::new(
                organization.parse()?,
                Self::favorite(favorite)?,
                folder
                    .as_deref()
                    .map(str::parse::<GameLibraryOrganizationFolderId>)
                    .transpose()?,
                Self::tags(transaction, id)?,
            ),
            location,
            modified,
            kind: StorageLibraryProjection::parse_kind(&kind)?,
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
        transaction: &Transaction<'_>,
        id: GameId,
        memory: &Arc<Semaphore>,
    ) -> Result<Option<StorageLibraryStoredRecord>, StorageLibraryError> {
        let mut reservation = StorageLibraryMemory::new(
            memory,
            Self::TAG_MEMORY + std::mem::size_of::<StorageLibraryStoredRecord>(),
        )?;
        let Some(state) = Self::state(transaction, id)? else {
            return Ok(None);
        };
        let length: i64 = transaction.query_row(
            "SELECT length(CAST(body AS BLOB)) FROM documents WHERE id = ?1",
            [id.as_uuid().to_string()],
            |row| row.get(0),
        )?;
        let length = usize::try_from(length)?;
        reservation.reserve(
            memory,
            length
                .checked_mul(4)
                .ok_or(StorageLibraryError::TextTooLarge)?,
        )?;
        let body: String = transaction.query_row(
            "SELECT body FROM documents WHERE id = ?1",
            [id.as_uuid().to_string()],
            |row| row.get(0),
        )?;
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
    fn row_bytes(row: &Row<'_>) -> Result<usize, StorageLibraryError> {
        let mut bytes = std::mem::size_of::<StorageLibraryQueryEntry>() + Self::TAG_MEMORY;
        for index in [8, 9, 10, 11, 15, 16, 17] {
            let value = row.get_ref(index)?;
            let length = match value {
                rusqlite::types::ValueRef::Text(text) => text.len(),
                rusqlite::types::ValueRef::Null => 0,
                rusqlite::types::ValueRef::Integer(_)
                | rusqlite::types::ValueRef::Real(_)
                | rusqlite::types::ValueRef::Blob(_) => {
                    return Err(StorageLibraryError::StoredDiscriminant);
                }
            };
            bytes = bytes
                .checked_add(
                    length
                        .checked_mul(4)
                        .ok_or(StorageLibraryError::TextTooLarge)?,
                )
                .ok_or(StorageLibraryError::TextTooLarge)?;
        }
        Ok(bytes)
    }
    fn entry(
        transaction: &Transaction<'_>,
        row: &Row<'_>,
        date_filtered: bool,
    ) -> Result<StorageLibraryQueryEntry, StorageLibraryError> {
        let id: GameId = row.get::<_, String>(0)?.parse()?;
        let revision = StorageLibraryError::content_revision(&row.get::<_, String>(1)?)?;
        let organization: GameLibraryOrganizationRevision = row.get::<_, String>(2)?.parse()?;
        let white = row
            .get::<_, Option<String>>(8)?
            .map(GameMetadataText::new)
            .transpose()?;
        let black = row
            .get::<_, Option<String>>(10)?
            .map(GameMetadataText::new)
            .transpose()?;
        let date = GameMetadataDate::new(row.get(12)?, row.get(13)?, row.get(14)?)?;
        let projection = StorageLibraryProjection::new(StorageLibraryProjectionOptions {
            kind: StorageLibraryProjection::parse_kind(&row.get::<_, String>(3)?)?,
            variant: StorageLibraryProjection::parse_variant(&row.get::<_, String>(4)?)?,
            start: StorageLibraryEnumStartKind::parse(&row.get::<_, String>(5)?)?,
            mode: row
                .get::<_, Option<String>>(6)?
                .as_deref()
                .map(StorageLibraryProjection::parse_mode)
                .transpose()?,
            result: StorageLibraryProjection::parse_result(&row.get::<_, String>(7)?)?,
            white,
            black,
            date: Some(date),
            opening: row
                .get::<_, Option<String>>(15)?
                .map(GameMetadataText::new)
                .transpose()?,
            code: row
                .get::<_, Option<String>>(17)?
                .map(GameMetadataOpeningCode::new)
                .transpose()?,
            plies: row.get(18)?,
        })?;
        if row.get::<_, Option<String>>(9)?.as_deref() != projection.white_key()
            || row.get::<_, Option<String>>(11)?.as_deref() != projection.black_key()
            || row.get::<_, Option<String>>(16)?.as_deref() != projection.opening_key()
        {
            return Err(StorageLibraryError::StoredDiscriminant);
        }
        let favorite = Self::favorite(row.get(20)?)?;
        let folder = row
            .get::<_, Option<String>>(21)?
            .as_deref()
            .map(str::parse)
            .transpose()?;
        let location = StorageTrashEnumLocation::parse(&row.get::<_, String>(22)?)?;
        Self::validate_location(location, row.get(23)?)?;
        Ok(StorageLibraryQueryEntry::new(
            StorageLibraryQueryEntryOptions {
                id,
                content_revision: revision,
                projection,
                organization: GameLibraryOrganizationState::new(
                    organization,
                    favorite,
                    folder,
                    Self::tags(transaction, id)?,
                ),
                modified: row
                    .get::<_, Option<i64>>(19)?
                    .map(GameLibraryOrganizationTimestamp::new)
                    .transpose()?,
                location,
                date_filtered,
            },
        ))
    }
    pub(super) fn query(
        transaction: &Transaction<'_>,
        query: &StorageLibraryQuery,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryQueryPage, StorageLibraryError> {
        let catalog = StorageLibraryTransaction::catalog(transaction)?;
        let compiled = query.compile(catalog)?;
        let mut reservation = StorageLibraryMemory::new(
            memory,
            query.transport_bytes()?
                + StorageLibraryQuery::PAGE_SIZE * std::mem::size_of::<StorageLibraryQueryEntry>(),
        )?;
        let sql = format!(
            "SELECT {} FROM library_entries e JOIN documents d ON d.id = e.game_id WHERE e.content_revision = d.revision AND {} ORDER BY {} LIMIT {}",
            Self::PROJECTION_COLUMNS,
            compiled.predicate(),
            compiled.ordering(),
            i64::try_from(StorageLibraryQuery::PAGE_SIZE + 1)?
        );
        let mut statement = transaction.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(compiled.values()))?;
        let mut entries = Vec::with_capacity(StorageLibraryQuery::PAGE_SIZE);
        let mut has_more = false;
        while let Some(row) = rows.next()? {
            if entries.len() == StorageLibraryQuery::PAGE_SIZE {
                has_more = true;
                break;
            }
            reservation.reserve(memory, Self::row_bytes(row)?)?;
            entries.push(Self::entry(transaction, row, query.is_date_filtered())?);
        }
        let next = if has_more {
            entries.last().map(|entry| query.cursor(catalog, entry))
        } else {
            None
        };
        let unindexed: i64 = transaction.query_row("SELECT COUNT(*) FROM documents d LEFT JOIN library_entries e ON e.game_id = d.id WHERE e.game_id IS NULL OR e.content_revision <> d.revision", [], |row| row.get(0))?;
        let unindexed = u64::try_from(unindexed)?;
        Ok(StorageLibraryQueryPage::new(
            entries,
            next,
            catalog,
            unindexed,
            reservation,
        ))
    }
    pub(super) fn adoption_page(
        transaction: &Transaction<'_>,
        cursor: StorageLibraryAdoptionCursor,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryAdoptionPage, StorageLibraryError> {
        let reservation = StorageLibraryMemory::new(
            memory,
            StorageLibraryQuery::PAGE_SIZE * std::mem::size_of::<StorageLibraryAdoptionCandidate>(),
        )?;
        let mut statement = transaction.prepare("SELECT d.id, d.revision, (EXISTS(SELECT 1 FROM games g WHERE g.game_id = d.id) OR EXISTS(SELECT 1 FROM library_entries e WHERE e.game_id = d.id)) FROM documents d WHERE (?1 IS NULL OR d.id > ?1) ORDER BY d.id LIMIT ?2")?;
        let mut rows = statement.query(params![
            cursor.boundary().map(|id| id.to_string()),
            i64::try_from(StorageLibraryQuery::PAGE_SIZE + 1)?
        ])?;
        let mut candidates = Vec::with_capacity(StorageLibraryQuery::PAGE_SIZE);
        let mut has_more = false;
        while let Some(row) = rows.next()? {
            if candidates.len() == StorageLibraryQuery::PAGE_SIZE {
                has_more = true;
                break;
            }
            candidates.push(StorageLibraryAdoptionCandidate::new(
                row.get::<_, String>(0)?.parse()?,
                StorageLibraryError::content_revision(&row.get::<_, String>(1)?)?,
                row.get(2)?,
            ));
        }
        let next = if has_more {
            candidates
                .last()
                .map(|candidate| StorageLibraryAdoptionCursor::after(candidate.id()))
        } else {
            None
        };
        Ok(StorageLibraryAdoptionPage::new(
            candidates,
            next,
            StorageLibraryTransaction::catalog(transaction)?,
            reservation,
        ))
    }

    pub(super) fn played_inventory(
        transaction: &Transaction<'_>,
        cursor: StorageLibraryInventoryCursor,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryAdoptionPage, StorageLibraryError> {
        let reservation = StorageLibraryMemory::new(
            memory,
            StorageLibraryInventoryPage::PAGE_SIZE
                * (std::mem::size_of::<StorageLibraryAdoptionCandidate>()
                    + std::mem::size_of::<StorageLibraryInventoryEntry>()),
        )?;
        let mut statement = transaction.prepare(
            "SELECT e.game_id, e.content_revision FROM library_entries e JOIN documents d ON d.id = e.game_id WHERE e.content_revision = d.revision AND e.content_kind = ?1 AND e.location = ?2 AND e.trashed_ms IS NULL AND (?3 IS NULL OR e.game_id > ?3) ORDER BY e.game_id LIMIT ?4",
        )?;
        let mut rows = statement.query(params![
            StorageLibraryProjection::kind_sql(StorageGameDocumentEnumContentKind::Played),
            StorageTrashEnumLocation::Library.sql(),
            cursor.boundary().map(|id| id.as_uuid().to_string()),
            i64::try_from(StorageLibraryInventoryPage::PAGE_SIZE + 1)?,
        ])?;
        let mut candidates = Vec::with_capacity(StorageLibraryInventoryPage::PAGE_SIZE);
        let mut has_more = false;
        while let Some(row) = rows.next()? {
            if candidates.len() == StorageLibraryInventoryPage::PAGE_SIZE {
                has_more = true;
                break;
            }
            let id = row.get::<_, String>(0)?.parse()?;
            candidates.push(StorageLibraryAdoptionCandidate::new(
                id,
                StorageLibraryError::content_revision(&row.get::<_, String>(1)?)?,
                true,
            ));
        }
        let next = if has_more {
            candidates
                .last()
                .map(|candidate| StorageLibraryAdoptionCursor::after(candidate.id()))
        } else {
            None
        };
        Ok(StorageLibraryAdoptionPage::new(
            candidates,
            next,
            StorageLibraryTransaction::catalog(transaction)?,
            reservation,
        ))
    }
}
