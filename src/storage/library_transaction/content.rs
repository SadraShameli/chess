use rusqlite::{Transaction, params};

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
use crate::storage::trash::StorageTrashEnumLocation;

pub(super) struct StorageLibraryTransactionContent;

impl StorageLibraryTransactionContent {
    pub(super) fn save(
        transaction: &Transaction<'_>,
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
        transaction.execute("INSERT INTO documents(id, revision, body) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET revision = excluded.revision, body = excluded.body", params![id.as_uuid().to_string(), revision.value().to_string(), prepared.body()])?;
        Self::project(
            transaction,
            StorageLibraryTransactionContentProjection {
                id,
                revision,
                projection: prepared.projection(),
                identity: prepared.identity(),
                modified: modified.map(|timestamp| timestamp.value()),
            },
        )?;
        Ok(StorageLibraryContentReceipt::new(
            id,
            revision,
            StorageLibraryTransaction::bump_catalog(transaction)?,
        ))
    }
    pub(super) fn adopt(
        transaction: &Transaction<'_>,
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
            },
        )?;
        Ok(StorageLibraryEnumAdoptionOutcome::Inserted(
            StorageLibraryTransaction::bump_catalog(transaction)?,
        ))
    }
    fn project(
        transaction: &Transaction<'_>,
        write: StorageLibraryTransactionContentProjection<'_>,
    ) -> Result<(), StorageLibraryError> {
        let p = write.projection;
        let date = p.date();
        transaction.execute(
            "INSERT INTO library_entries(game_id, content_revision, organization_revision, content_kind, rules_variant, start_kind, mode, result, white_display, white_key, black_display, black_key, date_year, date_month, date_day, opening_display, opening_key, opening_code, plies, identity_initial_fen, identity_mainline_uci, content_sha256, modified_ms, favorite, folder_id, location, trashed_ms)
             VALUES (?1, ?2, '0', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, 0, NULL, 'library', NULL)
             ON CONFLICT(game_id) DO UPDATE SET content_revision = excluded.content_revision, content_kind = excluded.content_kind, rules_variant = excluded.rules_variant, start_kind = excluded.start_kind, mode = excluded.mode, result = excluded.result, white_display = excluded.white_display, white_key = excluded.white_key, black_display = excluded.black_display, black_key = excluded.black_key, date_year = excluded.date_year, date_month = excluded.date_month, date_day = excluded.date_day, opening_display = excluded.opening_display, opening_key = excluded.opening_key, opening_code = excluded.opening_code, plies = excluded.plies, identity_initial_fen = excluded.identity_initial_fen, identity_mainline_uci = excluded.identity_mainline_uci, content_sha256 = excluded.content_sha256, modified_ms = excluded.modified_ms",
            params![write.id.as_uuid().to_string(), write.revision.value().to_string(), StorageLibraryProjection::kind_sql(p.kind()),
                StorageLibraryProjection::variant_sql(p.variant()), p.start().sql(), p.mode().map(StorageLibraryProjection::mode_sql), p.result().pgn_token(),
                p.white().map(|text| text.as_str()), p.white_key(), p.black().map(|text| text.as_str()), p.black_key(),
                date.and_then(|date| date.year()), date.and_then(|date| date.month()), date.and_then(|date| date.day()),
                p.opening().map(|text| text.as_str()), p.opening_key(), p.code().map(|code| code.as_str()), p.plies(),
                write.identity.initial_fen(), write.identity.mainline(), write.identity.body_digest().bytes().as_slice(), write.modified],
        )?;
        Ok(())
    }
}

struct StorageLibraryTransactionContentProjection<'a> {
    id: crate::game::enums::GameId,
    revision: GameStorageRevision,
    projection: &'a StorageLibraryProjection,
    identity: &'a StorageLibraryPreparedIdentity,
    modified: Option<i64>,
}
