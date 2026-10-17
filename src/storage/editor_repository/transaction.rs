use diesel::connection::SimpleConnection;
use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Nullable, Text};
use diesel::{Queryable, SqliteConnection};
use tokio::sync::OwnedSemaphorePermit;

use crate::board::editor::{BoardEditorDraft, BoardEditorDraftId};
use crate::storage::editor_repository::{
    StorageEditorRepository, StorageEditorRepositoryCursor, StorageEditorRepositoryEntry,
    StorageEditorRepositoryEnumMutation, StorageEditorRepositoryEnumRequest,
    StorageEditorRepositoryEnumResponse, StorageEditorRepositoryError, StorageEditorRepositoryPage,
    StorageEditorRepositoryReceipt, StorageEditorRepositoryRecovered,
    StorageEditorRepositoryRevision,
};
use crate::storage::schema::editor_drafts;

macro_rules! storage_editor_repository_columns {
    ($limits:expr) => {
        (
            sql::<Nullable<Text>>("CASE WHEN length(CAST(draft_id AS BLOB)) <= ")
                .bind::<BigInt, _>($limits.id)
                .sql(" THEN draft_id END"),
            sql::<Nullable<Text>>("CASE WHEN length(CAST(storage_revision AS BLOB)) <= ")
                .bind::<BigInt, _>($limits.revision)
                .sql(" THEN storage_revision END"),
            editor_drafts::schema_version,
            sql::<Nullable<Text>>("CASE WHEN length(CAST(body AS BLOB)) <= ")
                .bind::<BigInt, _>($limits.body)
                .sql(" THEN body END"),
            sql::<Bool>("body IS NULL"),
        )
    };
}

struct StorageEditorRepositoryLimits {
    id: i64,
    revision: i64,
    body: i64,
}

impl StorageEditorRepositoryLimits {
    fn new() -> Result<Self, StorageEditorRepositoryError> {
        Ok(Self {
            id: i64::try_from(StorageEditorRepositoryStored::ID_BYTES)?,
            revision: i64::try_from(StorageEditorRepositoryStored::REVISION_BYTES)?,
            body: i64::try_from(StorageEditorRepository::MAX_DRAFT_BYTES)?,
        })
    }
}

#[derive(Queryable)]
struct StorageEditorRepositoryRow {
    id: Option<String>,
    storage_revision: Option<String>,
    schema_version: i32,
    body: Option<String>,
    is_deleted: bool,
}

struct StorageEditorRepositoryStored {
    id: BoardEditorDraftId,
    storage_revision: StorageEditorRepositoryRevision,
    draft: Option<BoardEditorDraft>,
}

impl StorageEditorRepositoryStored {
    const ID_BYTES: usize = 36;
    const REVISION_BYTES: usize = 20;

    fn text(value: Option<&str>) -> Result<&str, StorageEditorRepositoryError> {
        value
            .filter(|text| !text.is_empty())
            .ok_or(StorageEditorRepositoryError::Corrupt)
    }

    fn read(row: StorageEditorRepositoryRow) -> Result<Self, StorageEditorRepositoryError> {
        let id: BoardEditorDraftId = Self::text(row.id.as_deref())?
            .parse()
            .map_err(|_error| StorageEditorRepositoryError::Corrupt)?;
        let storage_revision = StorageEditorRepositoryRevision::try_from(
            Self::text(row.storage_revision.as_deref())?.to_owned(),
        )?;
        if storage_revision == StorageEditorRepositoryRevision::ABSENT {
            return Err(StorageEditorRepositoryError::Corrupt);
        }
        let version = u32::try_from(row.schema_version)?;
        if version > StorageEditorRepository::FORMAT_VERSION {
            return Err(StorageEditorRepositoryError::FutureVersion {
                found: version,
                supported: StorageEditorRepository::FORMAT_VERSION,
            });
        }
        if version != StorageEditorRepository::FORMAT_VERSION {
            return Err(StorageEditorRepositoryError::Corrupt);
        }
        let draft = if row.is_deleted {
            None
        } else {
            let draft: BoardEditorDraft = serde_json::from_str(Self::text(row.body.as_deref())?)?;
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
        connection: &mut SqliteConnection,
        id: BoardEditorDraftId,
    ) -> Result<Option<Self>, StorageEditorRepositoryError> {
        let limits = StorageEditorRepositoryLimits::new()?;
        editor_drafts::table
            .filter(editor_drafts::draft_id.eq(id.to_string()))
            .select(storage_editor_repository_columns!(limits))
            .first::<StorageEditorRepositoryRow>(connection)
            .optional()?
            .map(Self::read)
            .transpose()
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

pub(in crate::storage) struct StorageEditorRepositoryExecution {
    outcome: Result<StorageEditorRepositoryEnumResponse, StorageEditorRepositoryError>,
    unresolved: bool,
}

impl StorageEditorRepositoryExecution {
    pub(in crate::storage) const fn is_unresolved(&self) -> bool {
        self.unresolved
    }

    pub(in crate::storage) fn into_outcome(
        self,
    ) -> Result<StorageEditorRepositoryEnumResponse, StorageEditorRepositoryError> {
        self.outcome
    }
}

pub(in crate::storage) struct StorageEditorRepositoryTransaction;

impl StorageEditorRepositoryTransaction {
    pub(in crate::storage) fn execute(
        connection: &mut SqliteConnection,
        request: StorageEditorRepositoryEnumRequest,
        memory_reservation: OwnedSemaphorePermit,
    ) -> StorageEditorRepositoryExecution {
        let outcome = match request {
            StorageEditorRepositoryEnumRequest::Save {
                draft,
                expected_storage_revision,
            } => {
                return Self::write(connection, |transaction| {
                    Self::save(transaction, &draft, expected_storage_revision)
                });
            }
            StorageEditorRepositoryEnumRequest::Delete {
                id,
                expected_storage_revision,
            } => {
                return Self::write(connection, |transaction| {
                    Self::delete(transaction, id, expected_storage_revision)
                });
            }
            StorageEditorRepositoryEnumRequest::Read { id } => {
                Self::read(connection, id, memory_reservation)
                    .map(StorageEditorRepositoryEnumResponse::Read)
            }
            StorageEditorRepositoryEnumRequest::List { after } => {
                Self::list(connection, after, memory_reservation)
                    .map(StorageEditorRepositoryEnumResponse::List)
            }
        };
        StorageEditorRepositoryExecution {
            outcome,
            unresolved: false,
        }
    }

    fn write(
        connection: &mut SqliteConnection,
        operation: impl FnOnce(
            &mut SqliteConnection,
        )
            -> Result<StorageEditorRepositoryReceipt, StorageEditorRepositoryError>,
    ) -> StorageEditorRepositoryExecution {
        if let Err(error) = connection.batch_execute("BEGIN IMMEDIATE") {
            return StorageEditorRepositoryExecution {
                outcome: Err(StorageEditorRepositoryError::Database(error)),
                unresolved: false,
            };
        }
        match operation(connection) {
            Ok(receipt) => match connection.batch_execute("COMMIT") {
                Ok(()) => StorageEditorRepositoryExecution {
                    outcome: Ok(StorageEditorRepositoryEnumResponse::Write(receipt)),
                    unresolved: false,
                },
                Err(commit) => Self::abandoned(
                    connection,
                    StorageEditorRepositoryError::Database(commit),
                    Err,
                ),
            },
            Err(primary) => Self::abandoned(connection, primary, |error| {
                Ok(StorageEditorRepositoryEnumResponse::RolledBack(error))
            }),
        }
    }

    fn abandoned(
        connection: &mut SqliteConnection,
        primary: StorageEditorRepositoryError,
        confirmed: impl FnOnce(
            StorageEditorRepositoryError,
        ) -> Result<
            StorageEditorRepositoryEnumResponse,
            StorageEditorRepositoryError,
        >,
    ) -> StorageEditorRepositoryExecution {
        match Self::rollback(connection) {
            Ok(()) => StorageEditorRepositoryExecution {
                outcome: confirmed(primary),
                unresolved: false,
            },
            Err(rollback) => StorageEditorRepositoryExecution {
                outcome: Err(StorageEditorRepositoryError::Rollback {
                    primary: Box::new(primary),
                    rollback,
                }),
                unresolved: true,
            },
        }
    }

    fn rollback(connection: &mut SqliteConnection) -> Result<(), diesel::result::Error> {
        drop(connection.batch_execute("BEGIN DEFERRED"));
        connection.batch_execute("ROLLBACK")
    }

    fn save(
        transaction: &mut SqliteConnection,
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
            diesel::update(editor_drafts::table.find(draft.id().to_string()))
                .set((
                    editor_drafts::storage_revision.eq(storage_revision.to_string()),
                    editor_drafts::body.eq(Some(body)),
                ))
                .execute(transaction)?
        } else {
            diesel::insert_into(editor_drafts::table)
                .values((
                    editor_drafts::draft_id.eq(draft.id().to_string()),
                    editor_drafts::storage_revision.eq(storage_revision.to_string()),
                    editor_drafts::schema_version
                        .eq(i32::try_from(StorageEditorRepository::FORMAT_VERSION)?),
                    editor_drafts::body.eq(Some(body)),
                ))
                .execute(transaction)?
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
        transaction: &mut SqliteConnection,
        id: BoardEditorDraftId,
        expected: StorageEditorRepositoryRevision,
    ) -> Result<StorageEditorRepositoryReceipt, StorageEditorRepositoryError> {
        let stored = StorageEditorRepositoryStored::load(transaction, id)?;
        StorageEditorRepositoryStored::check_expected(stored.as_ref(), expected)?;
        if stored.is_none() {
            return Err(StorageEditorRepositoryError::NotFound { id });
        }
        let storage_revision = expected.next()?;
        let changed = diesel::update(editor_drafts::table.find(id.to_string()))
            .set((
                editor_drafts::storage_revision.eq(storage_revision.to_string()),
                editor_drafts::body.eq(None::<String>),
            ))
            .execute(transaction)?;
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
        connection: &mut SqliteConnection,
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
        connection: &mut SqliteConnection,
        after: Option<StorageEditorRepositoryCursor>,
        memory_reservation: OwnedSemaphorePermit,
    ) -> Result<StorageEditorRepositoryPage, StorageEditorRepositoryError> {
        let limits = StorageEditorRepositoryLimits::new()?;
        let cursor = after
            .map(|cursor| cursor.id().to_string())
            .unwrap_or_default();
        let rows = editor_drafts::table
            .filter(editor_drafts::body.is_not_null())
            .filter(editor_drafts::draft_id.gt(cursor))
            .order_by(editor_drafts::draft_id)
            .limit(i64::try_from(StorageEditorRepository::PAGE_SIZE)?)
            .select(storage_editor_repository_columns!(limits))
            .load::<StorageEditorRepositoryRow>(connection)?;
        let mut entries = Vec::with_capacity(StorageEditorRepository::PAGE_SIZE);
        for row in rows {
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
