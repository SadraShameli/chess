use diesel::SqliteConnection;
use diesel::prelude::*;
use diesel::upsert::excluded;

use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_prepared::{
    StorageLibraryEnumContentChange, StorageLibraryPreparedIdentity, StorageLibraryPreparedWrite,
    StorageLibraryProjection,
};
use crate::storage::library_repository::{StorageLibraryContentReceipt, StorageLibraryError};
use crate::storage::library_transaction::StorageLibraryTransaction;
use crate::storage::library_transaction::rows::StorageLibraryTransactionRows;
use crate::storage::schema::{documents, library_entries};
use crate::storage::trash::StorageTrashEnumLocation;

pub(super) struct StorageLibraryTransactionContent;

impl StorageLibraryTransactionContent {
    pub(super) fn save(
        connection: &mut SqliteConnection,
        prepared: &StorageLibraryPreparedWrite,
    ) -> Result<StorageLibraryContentReceipt, StorageLibraryError> {
        let id = prepared.id();
        let actual = StorageLibraryTransactionRows::document_revision(connection, id)?;
        let revision = actual.unwrap_or(GameStorageRevision::INITIAL);
        if prepared.expected() != revision {
            return Err(StorageLibraryError::ContentConflict {
                expected: prepared.expected(),
                actual: revision,
            });
        }
        let indexed = StorageLibraryTransactionRows::state(connection, id)?;
        if let Some(existing) = &indexed {
            if existing.location() == StorageTrashEnumLocation::Trash {
                return Err(StorageLibraryError::InTrash(id));
            }
            if existing.kind() != prepared.projection().kind() {
                return Err(StorageLibraryError::ContentKind);
            }
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
                connection,
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
            diesel::update(documents::table.filter(documents::id.eq(id.as_uuid().to_string())))
                .set((
                    documents::revision.eq(revision.value().to_string()),
                    documents::body.eq(prepared.body()),
                ))
                .execute(connection)?;
            Self::project(
                connection,
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
            StorageLibraryTransaction::diesel_bump_catalog(connection)?,
        ))
    }
    pub(super) fn insert_new(
        connection: &mut SqliteConnection,
        insert: StorageLibraryTransactionContentInsert<'_>,
    ) -> Result<(), StorageLibraryError> {
        if StorageLibraryTransactionRows::document_revision(connection, insert.id)?.is_some() {
            return Err(StorageLibraryError::ImportExisting);
        }
        diesel::insert_into(documents::table)
            .values((
                documents::id.eq(insert.id.as_uuid().to_string()),
                documents::revision.eq("1"),
                documents::body.eq(insert.body),
            ))
            .execute(connection)?;
        Self::project(
            connection,
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
    fn project(
        connection: &mut SqliteConnection,
        write: StorageLibraryTransactionContentProjection<'_>,
    ) -> Result<(), StorageLibraryError> {
        let p = write.projection;
        let body_digest = write.identity.body_digest();
        let digest = body_digest.bytes();
        let date = p.date();
        let year = date.and_then(|date| date.year()).map(i32::from);
        let month = date.and_then(|date| date.month()).map(i32::from);
        let day = date.and_then(|date| date.day()).map(i32::from);
        let row = (
            library_entries::game_id.eq(write.id.as_uuid().to_string()),
            library_entries::content_revision.eq(write.revision.value().to_string()),
            library_entries::organization_revision.eq("0"),
            library_entries::content_kind.eq(StorageLibraryProjection::kind_sql(p.kind())),
            library_entries::rules_variant.eq(StorageLibraryProjection::variant_sql(p.variant())),
            library_entries::start_kind.eq(p.start().sql()),
            library_entries::mode.eq(p.mode().map(StorageLibraryProjection::mode_sql)),
            library_entries::result.eq(p.result().pgn_token()),
            library_entries::white_display.eq(p.white().map(|text| text.as_str())),
            library_entries::white_key.eq(p.white_key()),
            library_entries::black_display.eq(p.black().map(|text| text.as_str())),
            library_entries::black_key.eq(p.black_key()),
            library_entries::date_year.eq(year),
            library_entries::date_month.eq(month),
            library_entries::date_day.eq(day),
            library_entries::opening_display.eq(p.opening().map(|text| text.as_str())),
            library_entries::opening_key.eq(p.opening_key()),
            library_entries::opening_code.eq(p.code().map(|code| code.as_str())),
            library_entries::plies.eq(i32::try_from(p.plies())?),
            library_entries::identity_initial_fen.eq(write.identity.initial_fen()),
            library_entries::identity_mainline_uci.eq(write.identity.mainline()),
            library_entries::content_sha256.eq(digest.as_slice()),
            library_entries::modified_ms.eq(write.modified),
            library_entries::favorite.eq(0),
            library_entries::folder_id.eq(write.folder.map(|folder| folder.to_string())),
            library_entries::location.eq(StorageTrashEnumLocation::Library.sql()),
            library_entries::trashed_ms.eq(None::<i64>),
        );
        diesel::insert_into(library_entries::table)
            .values(row)
            .on_conflict(library_entries::game_id)
            .do_update()
            .set((
                library_entries::content_revision.eq(excluded(library_entries::content_revision)),
                library_entries::content_kind.eq(excluded(library_entries::content_kind)),
                library_entries::rules_variant.eq(excluded(library_entries::rules_variant)),
                library_entries::start_kind.eq(excluded(library_entries::start_kind)),
                library_entries::mode.eq(excluded(library_entries::mode)),
                library_entries::result.eq(excluded(library_entries::result)),
                library_entries::white_display.eq(excluded(library_entries::white_display)),
                library_entries::white_key.eq(excluded(library_entries::white_key)),
                library_entries::black_display.eq(excluded(library_entries::black_display)),
                library_entries::black_key.eq(excluded(library_entries::black_key)),
                library_entries::date_year.eq(excluded(library_entries::date_year)),
                library_entries::date_month.eq(excluded(library_entries::date_month)),
                library_entries::date_day.eq(excluded(library_entries::date_day)),
                library_entries::opening_display.eq(excluded(library_entries::opening_display)),
                library_entries::opening_key.eq(excluded(library_entries::opening_key)),
                library_entries::opening_code.eq(excluded(library_entries::opening_code)),
                library_entries::plies.eq(excluded(library_entries::plies)),
                library_entries::identity_initial_fen
                    .eq(excluded(library_entries::identity_initial_fen)),
                library_entries::identity_mainline_uci
                    .eq(excluded(library_entries::identity_mainline_uci)),
                library_entries::content_sha256.eq(excluded(library_entries::content_sha256)),
                library_entries::modified_ms.eq(excluded(library_entries::modified_ms)),
            ))
            .execute(connection)?;
        Ok(())
    }
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
