use diesel::sqlite::SqliteConnection;
use diesel::{AsChangeset, ExpressionMethods, Insertable, QueryDsl, RunQueryDsl};

use crate::game::library_organization::GameLibraryOrganizationRevision;
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_prepared::{
    StorageLibraryEnumContentChange, StorageLibraryPreparedAdoption,
    StorageLibraryPreparedIdentity, StorageLibraryPreparedWrite, StorageLibraryProjection,
};
use crate::storage::library_repository::{
    StorageLibraryContentReceipt, StorageLibraryEnumAdoptionOutcome, StorageLibraryError,
};
use crate::storage::library_transaction::StorageLibraryTransaction;
use crate::storage::library_transaction::rows::StorageLibraryTransactionRows;
use crate::storage::schema::{documents, library_entries};
use crate::storage::trash::StorageTrashEnumLocation;

pub(super) struct StorageLibraryTransactionContent;

impl StorageLibraryTransactionContent {
    pub(super) fn save(
        transaction: &mut SqliteConnection,
        prepared: &StorageLibraryPreparedWrite,
    ) -> Result<StorageLibraryContentReceipt, StorageLibraryError> {
        let id = prepared.id();
        let actual = StorageLibraryTransactionRows::document_revision(transaction, id)?;
        let revision = actual.unwrap_or(GameStorageRevision::INITIAL);
        if prepared.expected() != revision {
            return Err(StorageLibraryError::ContentConflict {
                expected: prepared.expected(),
                actual: revision,
            });
        }
        let indexed = StorageLibraryTransactionRows::state(transaction, id)?;
        if let Some(existing) = &indexed {
            if existing.location() == StorageTrashEnumLocation::Trash {
                return Err(StorageLibraryError::InTrash(id));
            }
            if existing.kind() != prepared.projection().kind() {
                return Err(StorageLibraryError::ContentKind);
            }
        } else if actual.is_some() {
            return Err(StorageLibraryError::AdoptionRequired);
        }
        let modified = match (prepared.change(), indexed.as_ref()) {
            (StorageLibraryEnumContentChange::Changed(timestamp), Some(existing)) => Some(
                existing
                    .modified()
                    .map_or(timestamp, |previous| previous.max(timestamp)),
            ),
            (StorageLibraryEnumContentChange::Changed(timestamp), None) => Some(timestamp),
            (StorageLibraryEnumContentChange::Checkpoint, Some(existing)) => existing.modified(),
            (StorageLibraryEnumContentChange::Checkpoint, None) => {
                return Err(StorageLibraryError::NewContentTimestamp);
            }
        };
        let revision = GameStorageRevision::new(
            revision
                .value()
                .checked_add(1)
                .ok_or(StorageLibraryError::ContentExhausted)?,
        );
        if actual.is_none() {
            Self::insert_new(
                transaction,
                StorageLibraryTransactionContentInsert {
                    id,
                    body: prepared.body(),
                    projection: prepared.projection(),
                    identity: prepared.identity(),
                    modified: modified.map(|timestamp| timestamp.value()),
                    folder: None,
                },
            )?;
        } else {
            diesel::update(documents::table.find(id.as_uuid().to_string()))
                .set((
                    documents::revision.eq(revision.value().to_string()),
                    documents::body.eq(prepared.body()),
                ))
                .execute(transaction)?;
            Self::project(
                transaction,
                StorageLibraryTransactionContentProjection {
                    id,
                    revision,
                    projection: prepared.projection(),
                    identity: prepared.identity(),
                    modified: modified.map(|timestamp| timestamp.value()),
                    folder: None,
                },
            )?;
        }
        Ok(StorageLibraryContentReceipt::new(
            id,
            revision,
            StorageLibraryTransaction::bump_catalog(transaction)?,
        ))
    }
    pub(super) fn insert_new(
        transaction: &mut SqliteConnection,
        insert: StorageLibraryTransactionContentInsert<'_>,
    ) -> Result<(), StorageLibraryError> {
        if StorageLibraryTransactionRows::document_revision(transaction, insert.id)?.is_some() {
            return Err(StorageLibraryError::ImportExisting);
        }
        diesel::insert_into(documents::table)
            .values((
                documents::id.eq(insert.id.as_uuid().to_string()),
                documents::revision.eq(GameStorageRevision::new(1).value().to_string()),
                documents::body.eq(insert.body),
            ))
            .execute(transaction)?;
        Self::project(
            transaction,
            StorageLibraryTransactionContentProjection {
                id: insert.id,
                revision: GameStorageRevision::new(1),
                projection: insert.projection,
                identity: insert.identity,
                modified: insert.modified,
                folder: insert.folder,
            },
        )
    }
    pub(super) fn adopt(
        transaction: &mut SqliteConnection,
        prepared: &StorageLibraryPreparedAdoption,
    ) -> Result<StorageLibraryEnumAdoptionOutcome, StorageLibraryError> {
        let current = StorageLibraryTransactionRows::document_revision(transaction, prepared.id())?;
        if current != Some(prepared.observed()) {
            return Ok(StorageLibraryEnumAdoptionOutcome::Changed(current));
        }
        if let Some(existing) = StorageLibraryTransactionRows::state(transaction, prepared.id())? {
            if existing.content() == prepared.observed() {
                if existing.kind() != prepared.projection().kind() {
                    return Err(StorageLibraryError::ContentKind);
                }
                return Ok(StorageLibraryEnumAdoptionOutcome::AlreadyCurrent);
            }
            return Err(StorageLibraryError::ProjectionStale);
        }
        Self::project(
            transaction,
            StorageLibraryTransactionContentProjection {
                id: prepared.id(),
                revision: prepared.observed(),
                projection: prepared.projection(),
                identity: prepared.identity(),
                modified: None,
                folder: None,
            },
        )?;
        Ok(StorageLibraryEnumAdoptionOutcome::Inserted(
            StorageLibraryTransaction::bump_catalog(transaction)?,
        ))
    }
    fn project(
        transaction: &mut SqliteConnection,
        write: StorageLibraryTransactionContentProjection<'_>,
    ) -> Result<(), StorageLibraryError> {
        let p = write.projection;
        let date = p.date();
        let digest = write.identity.body_digest();
        let fields = StorageLibraryTransactionContentFields {
            content_revision: write.revision.value().to_string(),
            content_kind: StorageLibraryProjection::kind_sql(p.kind()),
            rules_variant: StorageLibraryProjection::variant_sql(p.variant()),
            start_kind: p.start().sql(),
            mode: p.mode().map(StorageLibraryProjection::mode_sql),
            result: p.result().pgn_token(),
            white_display: p.white().map(|text| text.as_str()),
            white_key: p.white_key(),
            black_display: p.black().map(|text| text.as_str()),
            black_key: p.black_key(),
            date_year: date.and_then(|date| date.year()).map(i64::from),
            date_month: date.and_then(|date| date.month()).map(i64::from),
            date_day: date.and_then(|date| date.day()).map(i64::from),
            opening_display: p.opening().map(|text| text.as_str()),
            opening_key: p.opening_key(),
            opening_code: p.code().map(|code| code.as_str()),
            plies: i64::from(p.plies()),
            identity_initial_fen: write.identity.initial_fen(),
            identity_mainline_uci: write.identity.mainline(),
            content_sha256: digest.bytes().as_slice(),
            modified_ms: write.modified,
        };
        diesel::insert_into(library_entries::table)
            .values((
                library_entries::game_id.eq(write.id.as_uuid().to_string()),
                library_entries::organization_revision
                    .eq(GameLibraryOrganizationRevision::ZERO.to_string()),
                library_entries::favorite.eq(i64::from(false)),
                library_entries::folder_id.eq(write.folder.map(|folder| folder.to_string())),
                library_entries::location.eq(StorageTrashEnumLocation::Library.sql()),
                library_entries::trashed_ms.eq(None::<i64>),
                &fields,
            ))
            .on_conflict(library_entries::game_id)
            .do_update()
            .set(&fields)
            .execute(transaction)?;
        Ok(())
    }
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = library_entries)]
#[diesel(treat_none_as_null = true, treat_none_as_default_value = false)]
struct StorageLibraryTransactionContentFields<'a> {
    content_revision: String,
    content_kind: &'a str,
    rules_variant: &'a str,
    start_kind: &'a str,
    mode: Option<&'a str>,
    result: &'a str,
    white_display: Option<&'a str>,
    white_key: Option<&'a str>,
    black_display: Option<&'a str>,
    black_key: Option<&'a str>,
    date_year: Option<i64>,
    date_month: Option<i64>,
    date_day: Option<i64>,
    opening_display: Option<&'a str>,
    opening_key: Option<&'a str>,
    opening_code: Option<&'a str>,
    plies: i64,
    identity_initial_fen: &'a str,
    identity_mainline_uci: &'a str,
    content_sha256: &'a [u8],
    modified_ms: Option<i64>,
}

struct StorageLibraryTransactionContentProjection<'a> {
    id: crate::game::enums::GameId,
    revision: GameStorageRevision,
    projection: &'a StorageLibraryProjection,
    identity: &'a StorageLibraryPreparedIdentity,
    modified: Option<i64>,
    folder: Option<uuid::Uuid>,
}

pub(super) struct StorageLibraryTransactionContentInsert<'a> {
    pub(super) id: crate::game::enums::GameId,
    pub(super) body: &'a str,
    pub(super) projection: &'a StorageLibraryProjection,
    pub(super) identity: &'a StorageLibraryPreparedIdentity,
    pub(super) modified: Option<i64>,
    pub(super) folder: Option<uuid::Uuid>,
}
