use std::sync::Arc;

use diesel::connection::DefaultLoadingMode;
use diesel::dsl::{count_star, exists, sql};
use diesel::sql_types::BigInt;
use diesel::sqlite::{Sqlite, SqliteConnection};
use diesel::{
    BoolExpressionMethods, ExpressionMethods, JoinOnDsl, OptionalExtension, QueryDsl, Queryable,
    RunQueryDsl, Selectable, SelectableHelper,
};
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
use crate::storage::schema::{documents, games, library_entries, library_entry_tags};
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

#[derive(Queryable)]
struct StorageLibraryTransactionStateRow {
    content: String,
    organization: String,
    favorite: i64,
    folder: Option<String>,
    location: String,
    modified: Option<i64>,
    kind: String,
    document: String,
    trashed: Option<i64>,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = library_entries, check_for_backend(Sqlite))]
struct StorageLibraryTransactionProjectionRow {
    game_id: String,
    content_revision: String,
    organization_revision: String,
    content_kind: String,
    rules_variant: String,
    start_kind: String,
    mode: Option<String>,
    result: String,
    white_display: Option<String>,
    white_key: Option<String>,
    black_display: Option<String>,
    black_key: Option<String>,
    date_year: Option<i64>,
    date_month: Option<i64>,
    date_day: Option<i64>,
    opening_display: Option<String>,
    opening_key: Option<String>,
    opening_code: Option<String>,
    plies: i64,
    modified_ms: Option<i64>,
    favorite: i64,
    folder_id: Option<String>,
    location: String,
    trashed_ms: Option<i64>,
}

pub(super) struct StorageLibraryTransactionRows;

impl StorageLibraryTransactionRows {
    const TAG_MEMORY: usize = GameLibraryOrganizationTagSet::MAX_TAGS
        * std::mem::size_of::<crate::game::library_organization::GameLibraryOrganizationTagId>();
    const TEXT_BYTES: &'static str = "coalesce(length(CAST(library_entries.white_display AS BLOB)), 0) + coalesce(length(CAST(library_entries.white_key AS BLOB)), 0) + coalesce(length(CAST(library_entries.black_display AS BLOB)), 0) + coalesce(length(CAST(library_entries.black_key AS BLOB)), 0) + coalesce(length(CAST(library_entries.opening_display AS BLOB)), 0) + coalesce(length(CAST(library_entries.opening_key AS BLOB)), 0) + coalesce(length(CAST(library_entries.opening_code AS BLOB)), 0)";

    pub(super) fn document_revision(
        transaction: &mut SqliteConnection,
        id: GameId,
    ) -> Result<Option<GameStorageRevision>, StorageLibraryError> {
        let value: Option<String> = documents::table
            .find(id.as_uuid().to_string())
            .select(documents::revision)
            .first(transaction)
            .optional()?;
        value
            .as_deref()
            .map(StorageLibraryError::content_revision)
            .transpose()
    }

    pub(super) fn tags(
        transaction: &mut SqliteConnection,
        id: GameId,
    ) -> Result<GameLibraryOrganizationTagSet, StorageLibraryError> {
        let rows = library_entry_tags::table
            .filter(library_entry_tags::game_id.eq(id.as_uuid().to_string()))
            .order(library_entry_tags::tag_id.asc())
            .limit(i64::try_from(GameLibraryOrganizationTagSet::MAX_TAGS + 1)?)
            .select(library_entry_tags::tag_id)
            .load_iter::<String, DefaultLoadingMode>(transaction)?;
        let tags = rows
            .map(|row| Ok(row?.parse()?))
            .collect::<Result<Vec<_>, StorageLibraryError>>()?;
        Ok(GameLibraryOrganizationTagSet::new(tags)?)
    }

    pub(super) fn state(
        transaction: &mut SqliteConnection,
        id: GameId,
    ) -> Result<Option<StorageLibraryTransactionEntryState>, StorageLibraryError> {
        let row = library_entries::table
            .inner_join(documents::table.on(documents::id.eq(library_entries::game_id)))
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
            .first::<StorageLibraryTransactionStateRow>(transaction)
            .optional()?;
        let Some(row) = row else {
            return Ok(None);
        };
        let content = StorageLibraryError::content_revision(&row.content)?;
        if content != StorageLibraryError::content_revision(&row.document)? {
            return Err(StorageLibraryError::ProjectionStale);
        }
        let location = StorageTrashEnumLocation::parse(&row.location)?;
        Self::validate_location(location, row.trashed)?;
        Ok(Some(StorageLibraryTransactionEntryState {
            content,
            organization: GameLibraryOrganizationState::new(
                row.organization.parse()?,
                Self::favorite(row.favorite)?,
                row.folder
                    .as_deref()
                    .map(str::parse::<GameLibraryOrganizationFolderId>)
                    .transpose()?,
                Self::tags(transaction, id)?,
            ),
            location,
            modified: row
                .modified
                .map(GameLibraryOrganizationTimestamp::new)
                .transpose()?,
            kind: StorageLibraryProjection::parse_kind(&row.kind)?,
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
        transaction: &mut SqliteConnection,
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
        let key = id.as_uuid().to_string();
        let length: i64 = documents::table
            .find(&key)
            .select(sql::<BigInt>("length(CAST(body AS BLOB))"))
            .first(transaction)?;
        reservation.reserve(
            memory,
            usize::try_from(length)?
                .checked_mul(4)
                .ok_or(StorageLibraryError::TextTooLarge)?,
        )?;
        let body = documents::table
            .find(&key)
            .select(documents::body)
            .first(transaction)?;
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

    fn row_bytes(text_bytes: i64) -> Result<usize, StorageLibraryError> {
        usize::try_from(text_bytes)?
            .checked_mul(4)
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<StorageLibraryQueryEntry>()))
            .and_then(|bytes| {
                bytes.checked_add(std::mem::size_of::<StorageLibraryTransactionProjectionRow>())
            })
            .and_then(|bytes| bytes.checked_add(Self::TAG_MEMORY))
            .ok_or(StorageLibraryError::TextTooLarge)
    }

    fn entry(
        transaction: &mut SqliteConnection,
        row: StorageLibraryTransactionProjectionRow,
        date_filtered: bool,
    ) -> Result<StorageLibraryQueryEntry, StorageLibraryError> {
        let id: GameId = row.game_id.parse()?;
        let revision = StorageLibraryError::content_revision(&row.content_revision)?;
        let organization: GameLibraryOrganizationRevision = row.organization_revision.parse()?;
        let date = GameMetadataDate::new(
            row.date_year.map(u16::try_from).transpose()?,
            row.date_month.map(u8::try_from).transpose()?,
            row.date_day.map(u8::try_from).transpose()?,
        )?;
        let projection = StorageLibraryProjection::new(StorageLibraryProjectionOptions {
            kind: StorageLibraryProjection::parse_kind(&row.content_kind)?,
            variant: StorageLibraryProjection::parse_variant(&row.rules_variant)?,
            start: StorageLibraryEnumStartKind::parse(&row.start_kind)?,
            mode: row
                .mode
                .as_deref()
                .map(StorageLibraryProjection::parse_mode)
                .transpose()?,
            result: StorageLibraryProjection::parse_result(&row.result)?,
            white: row.white_display.map(GameMetadataText::new).transpose()?,
            black: row.black_display.map(GameMetadataText::new).transpose()?,
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
        let location = StorageTrashEnumLocation::parse(&row.location)?;
        Self::validate_location(location, row.trashed_ms)?;
        Ok(StorageLibraryQueryEntry::new(
            StorageLibraryQueryEntryOptions {
                id,
                content_revision: revision,
                projection,
                organization: GameLibraryOrganizationState::new(
                    organization,
                    Self::favorite(row.favorite)?,
                    row.folder_id.as_deref().map(str::parse).transpose()?,
                    Self::tags(transaction, id)?,
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
        transaction: &mut SqliteConnection,
        query: &StorageLibraryQuery,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryQueryPage, StorageLibraryError> {
        let catalog = StorageLibraryTransaction::catalog(transaction)?;
        let mut reservation = StorageLibraryMemory::new(
            memory,
            query.transport_bytes()?
                + StorageLibraryQuery::PAGE_SIZE * std::mem::size_of::<StorageLibraryQueryEntry>(),
        )?;
        let lengths: Vec<i64> = query
            .selection(catalog)?
            .inner_join(documents::table.on(documents::id.eq(library_entries::game_id)))
            .filter(library_entries::content_revision.eq(documents::revision))
            .select(sql::<BigInt>(Self::TEXT_BYTES))
            .load(transaction)?;
        for bytes in lengths.iter().take(StorageLibraryQuery::PAGE_SIZE) {
            reservation.reserve(memory, Self::row_bytes(*bytes)?)?;
        }
        let rows = query
            .selection(catalog)?
            .inner_join(documents::table.on(documents::id.eq(library_entries::game_id)))
            .filter(library_entries::content_revision.eq(documents::revision))
            .limit(i64::try_from(StorageLibraryQuery::PAGE_SIZE)?)
            .select(StorageLibraryTransactionProjectionRow::as_select())
            .load::<StorageLibraryTransactionProjectionRow>(transaction)?;
        if rows.len() != lengths.len().min(StorageLibraryQuery::PAGE_SIZE) {
            return Err(StorageLibraryError::ProjectionStale);
        }
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            entries.push(Self::entry(transaction, row, query.is_date_filtered())?);
        }
        let next = if lengths.len() > StorageLibraryQuery::PAGE_SIZE {
            entries.last().map(|entry| query.cursor(catalog, entry))
        } else {
            None
        };
        let unindexed: i64 = documents::table
            .left_join(library_entries::table.on(library_entries::game_id.eq(documents::id)))
            .filter(
                library_entries::game_id
                    .is_null()
                    .or(library_entries::content_revision.ne(documents::revision)),
            )
            .select(count_star())
            .get_result(transaction)?;
        Ok(StorageLibraryQueryPage::new(
            entries,
            next,
            catalog,
            u64::try_from(unindexed)?,
            reservation,
        ))
    }

    pub(super) fn adoption_page(
        transaction: &mut SqliteConnection,
        cursor: StorageLibraryAdoptionCursor,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryAdoptionPage, StorageLibraryError> {
        let reservation = StorageLibraryMemory::new(
            memory,
            StorageLibraryQuery::PAGE_SIZE * std::mem::size_of::<StorageLibraryAdoptionCandidate>(),
        )?;
        let mut query = documents::table.into_boxed::<Sqlite>();
        if let Some(after) = cursor.boundary() {
            query = query.filter(documents::id.gt(after.to_string()));
        }
        let rows = query
            .order(documents::id.asc())
            .limit(i64::try_from(StorageLibraryQuery::PAGE_SIZE + 1)?)
            .select((
                documents::id,
                documents::revision,
                exists(games::table.filter(games::game_id.eq(documents::id))).or(exists(
                    library_entries::table.filter(library_entries::game_id.eq(documents::id)),
                )),
            ))
            .load_iter::<(String, String, bool), DefaultLoadingMode>(transaction)?;
        let mut candidates = Vec::with_capacity(StorageLibraryQuery::PAGE_SIZE);
        let mut has_more = false;
        for row in rows {
            let (id, revision, projected) = row?;
            if candidates.len() == StorageLibraryQuery::PAGE_SIZE {
                has_more = true;
                break;
            }
            candidates.push(StorageLibraryAdoptionCandidate::new(
                id.parse()?,
                StorageLibraryError::content_revision(&revision)?,
                projected,
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
        transaction: &mut SqliteConnection,
        cursor: StorageLibraryInventoryCursor,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryAdoptionPage, StorageLibraryError> {
        let reservation = StorageLibraryMemory::new(
            memory,
            StorageLibraryInventoryPage::PAGE_SIZE
                * (std::mem::size_of::<StorageLibraryAdoptionCandidate>()
                    + std::mem::size_of::<StorageLibraryInventoryEntry>()),
        )?;
        let mut query = library_entries::table
            .inner_join(documents::table.on(documents::id.eq(library_entries::game_id)))
            .filter(library_entries::content_revision.eq(documents::revision))
            .filter(
                library_entries::content_kind.eq(StorageLibraryProjection::kind_sql(
                    StorageGameDocumentEnumContentKind::Played,
                )),
            )
            .filter(library_entries::location.eq(StorageTrashEnumLocation::Library.sql()))
            .filter(library_entries::trashed_ms.is_null())
            .into_boxed::<Sqlite>();
        if let Some(after) = cursor.boundary() {
            query = query.filter(library_entries::game_id.gt(after.as_uuid().to_string()));
        }
        let rows = query
            .order(library_entries::game_id.asc())
            .limit(i64::try_from(StorageLibraryInventoryPage::PAGE_SIZE + 1)?)
            .select((library_entries::game_id, library_entries::content_revision))
            .load_iter::<(String, String), DefaultLoadingMode>(transaction)?;
        let mut candidates = Vec::with_capacity(StorageLibraryInventoryPage::PAGE_SIZE);
        let mut has_more = false;
        for row in rows {
            let (id, revision) = row?;
            if candidates.len() == StorageLibraryInventoryPage::PAGE_SIZE {
                has_more = true;
                break;
            }
            candidates.push(StorageLibraryAdoptionCandidate::new(
                id.parse()?,
                StorageLibraryError::content_revision(&revision)?,
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
