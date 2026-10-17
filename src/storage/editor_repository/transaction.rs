use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use tokio::sync::OwnedSemaphorePermit;

use crate::board::editor::{BoardEditorDraft, BoardEditorDraftId};
use crate::storage::editor_repository::{
    StorageEditorRepository, StorageEditorRepositoryCursor, StorageEditorRepositoryEntry,
    StorageEditorRepositoryEnumMutation, StorageEditorRepositoryEnumRequest,
    StorageEditorRepositoryEnumResponse, StorageEditorRepositoryError, StorageEditorRepositoryPage,
    StorageEditorRepositoryReceipt, StorageEditorRepositoryRecovered,
    StorageEditorRepositoryRevision,
};

struct StorageEditorRepositoryStored {
    id: BoardEditorDraftId,
    storage_revision: StorageEditorRepositoryRevision,
    draft: Option<BoardEditorDraft>,
}

impl StorageEditorRepositoryStored {
    const ID_BYTES: usize = 36;
    const REVISION_BYTES: usize = 20;

    fn text<'row>(
        row: &'row rusqlite::Row<'_>,
        index: usize,
        maximum: usize,
    ) -> Result<&'row str, StorageEditorRepositoryError> {
        match row.get_ref(index)? {
            rusqlite::types::ValueRef::Text(bytes)
                if !bytes.is_empty() && bytes.len() <= maximum =>
            {
                std::str::from_utf8(bytes).map_err(|_error| StorageEditorRepositoryError::Corrupt)
            }
            rusqlite::types::ValueRef::Null
            | rusqlite::types::ValueRef::Integer(_)
            | rusqlite::types::ValueRef::Real(_)
            | rusqlite::types::ValueRef::Text(_)
            | rusqlite::types::ValueRef::Blob(_) => Err(StorageEditorRepositoryError::Corrupt),
        }
    }

    fn read(row: &rusqlite::Row<'_>) -> Result<Self, StorageEditorRepositoryError> {
        let id: BoardEditorDraftId = Self::text(row, 0, Self::ID_BYTES)?
            .parse()
            .map_err(|_error| StorageEditorRepositoryError::Corrupt)?;
        let storage_revision = StorageEditorRepositoryRevision::try_from(
            Self::text(row, 1, Self::REVISION_BYTES)?.to_owned(),
        )?;
        if storage_revision == StorageEditorRepositoryRevision::ABSENT {
            return Err(StorageEditorRepositoryError::Corrupt);
        }
        let version = u32::try_from(row.get::<_, i64>(2)?)?;
        if version > StorageEditorRepository::FORMAT_VERSION {
            return Err(StorageEditorRepositoryError::FutureVersion {
                found: version,
                supported: StorageEditorRepository::FORMAT_VERSION,
            });
        }
        if version != StorageEditorRepository::FORMAT_VERSION {
            return Err(StorageEditorRepositoryError::Corrupt);
        }
        let draft = if row.get::<_, bool>(4)? {
            None
        } else {
            let body = Self::text(row, 3, StorageEditorRepository::MAX_DRAFT_BYTES)?;
            let draft: BoardEditorDraft = serde_json::from_str(body)?;
            if draft.id() != id {
                return Err(StorageEditorRepositoryError::Corrupt);
            }
            Some(draft)
        };
        Ok(Self {
            id,
            storage_revision,
            draft,
        })
    }

    fn load(
        connection: &Connection,
        id: BoardEditorDraftId,
    ) -> Result<Option<Self>, StorageEditorRepositoryError> {
        connection.query_row(
            "SELECT CASE WHEN length(CAST(draft_id AS BLOB)) <= ?3 THEN draft_id END, CASE WHEN length(CAST(storage_revision AS BLOB)) <= ?4 THEN storage_revision END, schema_version, CASE WHEN length(CAST(body AS BLOB)) <= ?2 THEN body END, body IS NULL FROM editor_drafts WHERE draft_id = ?1",
            params![id.to_string(), i64::try_from(StorageEditorRepository::MAX_DRAFT_BYTES)?, i64::try_from(Self::ID_BYTES)?, i64::try_from(Self::REVISION_BYTES)?],
            |row| Ok(Self::read(row)),
        ).optional()?.transpose()
    }

    fn check_expected(
        stored: Option<&Self>,
        expected: StorageEditorRepositoryRevision,
    ) -> Result<(), StorageEditorRepositoryError> {
        let actual = stored.map_or(StorageEditorRepositoryRevision::ABSENT, |stored| {
            stored.storage_revision
        });
        if actual != expected {
            return Err(StorageEditorRepositoryError::Conflict { expected, actual });
        }
        if let Some(stored) = stored
            && stored.draft.is_none()
        {
            return Err(StorageEditorRepositoryError::Deleted { id: stored.id });
        }
        Ok(())
    }
}

pub(in crate::storage) struct StorageEditorRepositoryTransaction;

impl StorageEditorRepositoryTransaction {
    pub(in crate::storage) fn execute(
        connection: &mut Connection,
        request: StorageEditorRepositoryEnumRequest,
        memory_reservation: OwnedSemaphorePermit,
    ) -> Result<StorageEditorRepositoryEnumResponse, StorageEditorRepositoryError> {
        match request {
            StorageEditorRepositoryEnumRequest::Save {
                draft,
                expected_storage_revision,
            } => Self::write(connection, |transaction| {
                Self::save(transaction, &draft, expected_storage_revision)
            }),
            StorageEditorRepositoryEnumRequest::Delete {
                id,
                expected_storage_revision,
            } => Self::write(connection, |transaction| {
                Self::delete(transaction, id, expected_storage_revision)
            }),
            StorageEditorRepositoryEnumRequest::Read { id } => {
                Self::read(connection, id, memory_reservation)
                    .map(StorageEditorRepositoryEnumResponse::Read)
            }
            StorageEditorRepositoryEnumRequest::List { after } => {
                Self::list(connection, after, memory_reservation)
                    .map(StorageEditorRepositoryEnumResponse::List)
            }
        }
    }

    fn write(
        connection: &mut Connection,
        operation: impl FnOnce(
            &Transaction<'_>,
        )
            -> Result<StorageEditorRepositoryReceipt, StorageEditorRepositoryError>,
    ) -> Result<StorageEditorRepositoryEnumResponse, StorageEditorRepositoryError> {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match operation(&transaction) {
            Ok(receipt) => {
                transaction.commit()?;
                Ok(StorageEditorRepositoryEnumResponse::Write(receipt))
            }
            Err(primary) => {
                if transaction.is_autocommit() {
                    return Ok(StorageEditorRepositoryEnumResponse::RolledBack(primary));
                }
                match transaction.rollback() {
                    Ok(()) => Ok(StorageEditorRepositoryEnumResponse::RolledBack(primary)),
                    Err(rollback) => Err(StorageEditorRepositoryError::Rollback {
                        primary: Box::new(primary),
                        rollback,
                    }),
                }
            }
        }
    }

    fn save(
        transaction: &Transaction<'_>,
        draft: &BoardEditorDraft,
        expected: StorageEditorRepositoryRevision,
    ) -> Result<StorageEditorRepositoryReceipt, StorageEditorRepositoryError> {
        let stored = StorageEditorRepositoryStored::load(transaction, draft.id())?;
        StorageEditorRepositoryStored::check_expected(stored.as_ref(), expected)?;
        let storage_revision = expected.next()?;
        let body = serde_json::to_string(draft)?;
        if body.len() > StorageEditorRepository::MAX_DRAFT_BYTES {
            return Err(StorageEditorRepositoryError::DraftTooLarge);
        }
        let mutation = StorageEditorRepositoryEnumMutation::Saved {
            draft_revision: draft.revision(),
            status: draft.status(),
        };
        let changed = if stored.is_some() {
            transaction.execute(
                "UPDATE editor_drafts SET storage_revision = ?2, body = ?3 WHERE draft_id = ?1",
                params![draft.id().to_string(), storage_revision.to_string(), body],
            )?
        } else {
            transaction.execute(
                "INSERT INTO editor_drafts (draft_id, storage_revision, schema_version, body) VALUES (?1, ?2, ?3, ?4)",
                params![draft.id().to_string(), storage_revision.to_string(), StorageEditorRepository::FORMAT_VERSION, body],
            )?
        };
        if changed != 1 {
            return Err(StorageEditorRepositoryError::Corrupt);
        }
        Ok(StorageEditorRepositoryReceipt {
            id: draft.id(),
            storage_revision,
            mutation,
        })
    }

    fn delete(
        transaction: &Transaction<'_>,
        id: BoardEditorDraftId,
        expected: StorageEditorRepositoryRevision,
    ) -> Result<StorageEditorRepositoryReceipt, StorageEditorRepositoryError> {
        let stored = StorageEditorRepositoryStored::load(transaction, id)?;
        StorageEditorRepositoryStored::check_expected(stored.as_ref(), expected)?;
        if stored.is_none() {
            return Err(StorageEditorRepositoryError::NotFound { id });
        }
        let storage_revision = expected.next()?;
        let changed = transaction.execute(
            "UPDATE editor_drafts SET storage_revision = ?2, body = NULL WHERE draft_id = ?1",
            params![id.to_string(), storage_revision.to_string()],
        )?;
        if changed != 1 {
            return Err(StorageEditorRepositoryError::Corrupt);
        }
        Ok(StorageEditorRepositoryReceipt {
            id,
            storage_revision,
            mutation: StorageEditorRepositoryEnumMutation::Deleted,
        })
    }

    fn read(
        connection: &Connection,
        id: BoardEditorDraftId,
        memory_reservation: OwnedSemaphorePermit,
    ) -> Result<Option<StorageEditorRepositoryRecovered>, StorageEditorRepositoryError> {
        Ok(
            StorageEditorRepositoryStored::load(connection, id)?.and_then(|stored| {
                stored.draft.map(|draft| StorageEditorRepositoryRecovered {
                    draft,
                    storage_revision: stored.storage_revision,
                    _memory_reservation: memory_reservation,
                })
            }),
        )
    }

    fn list(
        connection: &Connection,
        after: Option<StorageEditorRepositoryCursor>,
        memory_reservation: OwnedSemaphorePermit,
    ) -> Result<StorageEditorRepositoryPage, StorageEditorRepositoryError> {
        let mut statement = connection.prepare_cached("SELECT CASE WHEN length(CAST(draft_id AS BLOB)) <= ?3 THEN draft_id END, CASE WHEN length(CAST(storage_revision AS BLOB)) <= ?4 THEN storage_revision END, schema_version, CASE WHEN length(CAST(body AS BLOB)) <= ?2 THEN body END, body IS NULL FROM editor_drafts WHERE body IS NOT NULL AND draft_id > ?1 ORDER BY draft_id LIMIT ?5")?;
        let cursor = after
            .map(|cursor| cursor.id().to_string())
            .unwrap_or_default();
        let mut rows = statement.query(params![
            cursor,
            i64::try_from(StorageEditorRepository::MAX_DRAFT_BYTES)?,
            i64::try_from(StorageEditorRepositoryStored::ID_BYTES)?,
            i64::try_from(StorageEditorRepositoryStored::REVISION_BYTES)?,
            i64::try_from(StorageEditorRepository::PAGE_SIZE)?
        ])?;
        let mut entries = Vec::with_capacity(StorageEditorRepository::PAGE_SIZE);
        while let Some(row) = rows.next()? {
            let stored = StorageEditorRepositoryStored::read(row)?;
            let draft = stored.draft.ok_or(StorageEditorRepositoryError::Corrupt)?;
            entries.push(StorageEditorRepositoryEntry {
                id: stored.id,
                storage_revision: stored.storage_revision,
                draft_revision: draft.revision(),
                status: draft.status(),
            });
        }
        Ok(StorageEditorRepositoryPage {
            entries,
            _memory_reservation: memory_reservation,
        })
    }
}
