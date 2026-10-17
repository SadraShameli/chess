use std::num::{NonZeroU64, TryFromIntError};
use std::sync::Arc;

use diesel::SqliteConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Nullable, Text};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, oneshot};

use crate::puzzle::attempt::PuzzleAttemptId;
use crate::puzzle::history::{PuzzleHistoryAuthorization, PuzzleHistoryEnumScope};
use crate::puzzle::persistence::{
    PuzzlePersistenceCodec, PuzzlePersistenceError, PuzzlePersistenceJournalProof,
    PuzzlePersistenceReceipt, PuzzlePersistenceRun, PuzzlePersistenceSaveRequest,
    PuzzlePersistenceStorageRevision,
};
use crate::puzzle::session::PuzzleSessionId;
use crate::puzzle::storage_repository::{PuzzleStorageRepository, PuzzleStorageWrite};
use crate::storage::document_store::{
    StorageDocumentClient, StorageDocumentError, StorageDocumentStore,
};
use crate::storage::schema::{
    puzzle_attempts, puzzle_checkpoints, puzzle_collections, puzzle_deleted_attempts,
    puzzle_events, puzzle_runs,
};

mod rows;

use rows::{
    StoragePuzzleGuardedAttemptRow, StoragePuzzleGuardedRunRow, StoragePuzzleInventoryRow,
    StoragePuzzleNewAttempt, StoragePuzzleNewCheckpoint, StoragePuzzleNewCollection,
    StoragePuzzleNewEvent, StoragePuzzleNewRun, StoragePuzzlePrefixRow, StoragePuzzleRunAdvance,
};

#[derive(Debug, Error)]
pub enum StoragePuzzleError {
    #[error(transparent)]
    Document(#[from] StorageDocumentError),
    #[error(transparent)]
    Codec(#[from] PuzzlePersistenceError),
    #[error(transparent)]
    Diesel(#[from] diesel::result::Error),
    #[error("The puzzle storage revision changed: expected {expected}, found {actual}")]
    Conflict { expected: u64, actual: u64 },
    #[error("The pinned puzzle collection or event prefix changed")]
    CollectionConflict,
    #[error("The saved puzzle repository rows are incomplete or inconsistent")]
    Corrupt,
    #[error(
        "Individual puzzle attempts cannot be deleted from a saved run without invalidating it"
    )]
    AttemptDeletionUnsupported,
    #[error("The puzzle storage revision is exhausted")]
    RevisionOverflow,
    #[error("The puzzle write completion has already been consumed")]
    CompletionConsumed,
    #[error("The puzzle database reply channel closed: {0}")]
    Channel(#[from] oneshot::error::RecvError),
    #[error("The puzzle database value exceeds its supported range: {0}")]
    Range(#[from] TryFromIntError),
    #[error("The puzzle database revision is invalid: {0}")]
    Revision(#[from] std::num::ParseIntError),
}

pub struct StoragePuzzleWrite {
    receiver: Option<oneshot::Receiver<Result<PuzzlePersistenceReceipt, StoragePuzzleError>>>,
}

impl StoragePuzzleWrite {
    pub(crate) const fn new(
        receiver: oneshot::Receiver<Result<PuzzlePersistenceReceipt, StoragePuzzleError>>,
    ) -> Self {
        Self {
            receiver: Some(receiver),
        }
    }

    pub async fn wait(&mut self) -> Result<PuzzlePersistenceReceipt, StoragePuzzleError> {
        let receiver = self
            .receiver
            .as_mut()
            .ok_or(StoragePuzzleError::CompletionConsumed)?;
        let completion = receiver.await;
        self.receiver = None;
        completion?
    }
}

pub struct StoragePuzzleRecovered {
    owner: PuzzlePersistenceRun,
    storage_revision: PuzzlePersistenceStorageRevision,
    persisted_event_count: usize,
    deleted_attempts: Vec<PuzzleAttemptId>,
    _memory_reservation: OwnedSemaphorePermit,
}

impl StoragePuzzleRecovered {
    pub const fn owner(&self) -> &PuzzlePersistenceRun {
        &self.owner
    }
    pub const fn storage_revision(&self) -> PuzzlePersistenceStorageRevision {
        self.storage_revision
    }
    pub const fn persisted_event_count(&self) -> usize {
        self.persisted_event_count
    }
    pub fn deleted_attempts(&self) -> &[PuzzleAttemptId] {
        &self.deleted_attempts
    }
    pub fn into_owner(self) -> PuzzlePersistenceRun {
        self.owner
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoragePuzzleRunMetadata {
    id: PuzzleSessionId,
    storage_revision: PuzzlePersistenceStorageRevision,
    persisted_event_count: usize,
}

impl StoragePuzzleRunMetadata {
    const MAX_DECIMAL_BYTES: usize = 20;

    pub const fn id(&self) -> PuzzleSessionId {
        self.id
    }

    pub const fn storage_revision(&self) -> PuzzlePersistenceStorageRevision {
        self.storage_revision
    }

    pub const fn persisted_event_count(&self) -> usize {
        self.persisted_event_count
    }

    fn canonical_decimal(value: Option<String>) -> Result<NonZeroU64, StoragePuzzleError> {
        let value = value.ok_or(StoragePuzzleError::Corrupt)?;
        let parsed: NonZeroU64 = value
            .parse()
            .map_err(|_error| StoragePuzzleError::Corrupt)?;
        if parsed.to_string() != value {
            return Err(StoragePuzzleError::Corrupt);
        }
        Ok(parsed)
    }

    fn read(row: StoragePuzzleInventoryRow) -> Result<Self, StoragePuzzleError> {
        let id = PuzzleSessionId::new(Self::canonical_decimal(row.run_id)?);
        let storage_revision = PuzzlePersistenceStorageRevision::new(
            Self::canonical_decimal(row.storage_revision)?.get(),
        );
        let persisted_event_count =
            usize::try_from(row.event_count).map_err(|_error| StoragePuzzleError::Corrupt)?;
        if persisted_event_count > PuzzlePersistenceCodec::MAX_EVENTS
            || !row.has_collection
            || !row.has_checkpoint
        {
            return Err(StoragePuzzleError::Corrupt);
        }
        Ok(Self {
            id,
            storage_revision,
            persisted_event_count,
        })
    }
}

pub struct StoragePuzzleRunPage {
    entries: Vec<StoragePuzzleRunMetadata>,
    _memory_reservation: OwnedSemaphorePermit,
}

impl StoragePuzzleRunPage {
    pub fn entries(&self) -> &[StoragePuzzleRunMetadata] {
        &self.entries
    }

    pub fn next_cursor(&self) -> Option<PuzzleSessionId> {
        if self.entries.len() == StoragePuzzleRepository::INVENTORY_PAGE_SIZE {
            self.entries.last().map(StoragePuzzleRunMetadata::id)
        } else {
            None
        }
    }

    pub(super) const fn memory_bytes() -> usize {
        StoragePuzzleRepository::INVENTORY_PAGE_SIZE
            * std::mem::size_of::<StoragePuzzleRunMetadata>()
    }
}

#[derive(Clone)]
pub struct StoragePuzzleRepository {
    client: StorageDocumentClient,
}

impl StoragePuzzleRepository {
    pub const INVENTORY_PAGE_SIZE: usize = StorageDocumentStore::INVENTORY_PAGE_SIZE;

    pub const fn new(client: StorageDocumentClient) -> Self {
        Self { client }
    }

    pub fn begin_save(
        &self,
        request: PuzzlePersistenceSaveRequest,
    ) -> Result<StoragePuzzleWrite, StoragePuzzleError> {
        self.client.begin_puzzle_save(request)
    }

    pub async fn load(
        &self,
        id: PuzzleSessionId,
    ) -> Result<Option<StoragePuzzleRecovered>, StoragePuzzleError> {
        self.client.read_puzzle(id).await
    }

    pub async fn delete_history(
        &self,
        authorization: PuzzleHistoryAuthorization,
    ) -> Result<usize, StoragePuzzleError> {
        self.client.delete_puzzle_history(authorization).await
    }

    pub async fn inventory(
        &self,
        after: Option<PuzzleSessionId>,
    ) -> Result<StoragePuzzleRunPage, StoragePuzzleError> {
        self.client.inventory_puzzle_runs(after).await
    }
}

impl PuzzleStorageWrite for StoragePuzzleWrite {
    type Error = StoragePuzzleError;

    async fn wait(&mut self) -> Result<PuzzlePersistenceReceipt, Self::Error> {
        Self::wait(self).await
    }
}

impl PuzzleStorageRepository for StoragePuzzleRepository {
    type Error = StoragePuzzleError;
    type Write = StoragePuzzleWrite;

    fn begin_save(
        &self,
        request: PuzzlePersistenceSaveRequest,
    ) -> Result<Self::Write, Self::Error> {
        Self::begin_save(self, request)
    }
}

struct StoragePuzzleStoredPrefix {
    revision: String,
    count: i32,
    digest: Vec<u8>,
    schema: i32,
    journal_version: i32,
    journal_digest: Vec<u8>,
}

impl StoragePuzzleStoredPrefix {
    fn validate(&self) -> Result<PuzzlePersistenceJournalProof, StoragePuzzleError> {
        let count = usize::try_from(self.count)?;
        if u32::try_from(self.schema).ok() != Some(PuzzlePersistenceCodec::SCHEMA)
            || self.digest.len() != 32
        {
            return Err(StoragePuzzleError::Corrupt);
        }
        if count > PuzzlePersistenceCodec::MAX_EVENTS {
            return Err(PuzzlePersistenceError::Budget.into());
        }
        let proof = PuzzlePersistenceJournalProof::restore(
            u32::try_from(self.journal_version)?,
            &self.journal_digest,
        )?;
        let initial = PuzzlePersistenceJournalProof::initial(
            self.digest
                .as_slice()
                .try_into()
                .map_err(PuzzlePersistenceError::from)?,
        );
        if count == 0 && proof != initial {
            return Err(StoragePuzzleError::Corrupt);
        }
        Ok(proof)
    }
}

impl From<StoragePuzzlePrefixRow> for StoragePuzzleStoredPrefix {
    fn from(row: StoragePuzzlePrefixRow) -> Self {
        Self {
            revision: row.revision,
            count: row.count,
            digest: row.digest,
            schema: row.schema,
            journal_version: row.journal_version,
            journal_digest: row.journal_digest,
        }
    }
}

struct StoragePuzzleStoredRun {
    prefix: StoragePuzzleStoredPrefix,
    collection: Option<String>,
    checkpoint: Option<String>,
}

impl From<StoragePuzzleGuardedRunRow> for StoragePuzzleStoredRun {
    fn from(row: StoragePuzzleGuardedRunRow) -> Self {
        Self {
            prefix: StoragePuzzleStoredPrefix {
                revision: row.revision,
                count: row.count,
                digest: row.digest,
                schema: row.schema,
                journal_version: row.journal_version,
                journal_digest: row.journal_digest,
            },
            collection: row.collection,
            checkpoint: row.checkpoint,
        }
    }
}

pub(super) struct StoragePuzzleTransaction;

impl StoragePuzzleTransaction {
    const EVENT_INSERT_CHUNK: usize = 1_000;

    pub(super) fn inventory(
        connection: &mut SqliteConnection,
        after: Option<PuzzleSessionId>,
        memory_reservation: OwnedSemaphorePermit,
    ) -> Result<StoragePuzzleRunPage, StoragePuzzleError> {
        let cursor = after.map(|id| id.value().get().to_string());
        let rows = diesel::sql_query(
            "SELECT CASE WHEN length(CAST(r.run_id AS BLOB)) <= ?3 THEN r.run_id END AS run_id, CASE WHEN length(CAST(r.storage_revision AS BLOB)) <= ?3 THEN r.storage_revision END AS storage_revision, r.event_count AS event_count, EXISTS(SELECT 1 FROM puzzle_collections c WHERE c.collection_id = r.collection_id) AS has_collection, EXISTS(SELECT 1 FROM puzzle_checkpoints p WHERE p.run_id = r.run_id) AS has_checkpoint FROM puzzle_runs r WHERE ?1 IS NULL OR (length(r.run_id), r.run_id COLLATE BINARY) > (length(?1), ?1 COLLATE BINARY) ORDER BY length(r.run_id), r.run_id COLLATE BINARY LIMIT ?2",
        )
        .bind::<Nullable<Text>, _>(cursor)
        .bind::<BigInt, _>(i64::try_from(StoragePuzzleRepository::INVENTORY_PAGE_SIZE)?)
        .bind::<BigInt, _>(i64::try_from(StoragePuzzleRunMetadata::MAX_DECIMAL_BYTES)?)
        .load::<StoragePuzzleInventoryRow>(connection)?;
        let entries = rows
            .into_iter()
            .map(StoragePuzzleRunMetadata::read)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(StoragePuzzleRunPage {
            entries,
            _memory_reservation: memory_reservation,
        })
    }

    pub(super) fn save(
        connection: &mut SqliteConnection,
        request: PuzzlePersistenceSaveRequest,
    ) -> Result<PuzzlePersistenceReceipt, StoragePuzzleError> {
        let snapshot = request.snapshot();
        let id = snapshot.context().session_id().value().get().to_string();
        let revision = connection.immediate_transaction(|connection| {
            let stored: Option<StoragePuzzleStoredPrefix> = puzzle_runs::table
                .inner_join(puzzle_collections::table)
                .filter(puzzle_runs::run_id.eq(&id))
                .select((
                    puzzle_runs::storage_revision,
                    puzzle_runs::event_count,
                    puzzle_collections::digest,
                    puzzle_collections::schema_version,
                    puzzle_runs::journal_version,
                    puzzle_runs::journal_digest,
                ))
                .first::<StoragePuzzlePrefixRow>(connection)
                .optional()?
                .map(StoragePuzzleStoredPrefix::from);
            let actual = stored
                .as_ref()
                .map(|prefix| prefix.revision.parse::<u64>())
                .transpose()?
                .unwrap_or(0);
            let expected = request.expected_storage_revision().value();
            if actual != expected {
                return Err(StoragePuzzleError::Conflict { expected, actual });
            }
            let revision = actual
                .checked_add(1)
                .ok_or(StoragePuzzleError::RevisionOverflow)?;
            match stored {
                Some(prefix) => {
                    let proof = prefix.validate()?;
                    if actual == PuzzlePersistenceStorageRevision::INITIAL.value() {
                        return Err(StoragePuzzleError::Corrupt);
                    }
                    if prefix.digest.as_slice() != snapshot.collection_digest().as_slice()
                        || usize::try_from(prefix.count)? != snapshot.base_event_count()
                        || proof != snapshot.base_journal_proof()
                    {
                        return Err(StoragePuzzleError::CollectionConflict);
                    }
                }
                None => {
                    if snapshot.base_event_count() != 0
                        || snapshot.base_journal_proof()
                            != PuzzlePersistenceJournalProof::initial(snapshot.collection_digest())
                    {
                        return Err(StoragePuzzleError::CollectionConflict);
                    }
                    diesel::insert_into(puzzle_collections::table)
                        .values(&StoragePuzzleNewCollection {
                            collection_id: &id,
                            schema_version: i32::try_from(PuzzlePersistenceCodec::SCHEMA)?,
                            digest: snapshot.collection_digest().as_slice(),
                            body: snapshot.collection(),
                        })
                        .execute(connection)?;
                    diesel::insert_into(puzzle_runs::table)
                        .values(&StoragePuzzleNewRun {
                            run_id: &id,
                            collection_id: &id,
                            storage_revision: "0",
                            event_count: 0,
                            journal_version: i32::try_from(PuzzlePersistenceJournalProof::VERSION)?,
                            journal_digest: snapshot.base_journal_proof().bytes().as_slice(),
                        })
                        .execute(connection)?;
                }
            }
            let mut rows = Vec::with_capacity(Self::EVENT_INSERT_CHUNK);
            for (offset, event) in snapshot.events().enumerate() {
                let ordinal = snapshot
                    .base_event_count()
                    .checked_add(offset)
                    .ok_or(StoragePuzzleError::Corrupt)?;
                rows.push(StoragePuzzleNewEvent {
                    run_id: &id,
                    ordinal: i32::try_from(ordinal)?,
                    body: event,
                });
                if rows.len() == Self::EVENT_INSERT_CHUNK {
                    diesel::insert_into(puzzle_events::table)
                        .values(&rows)
                        .execute(connection)?;
                    rows.clear();
                }
            }
            if !rows.is_empty() {
                diesel::insert_into(puzzle_events::table)
                    .values(&rows)
                    .execute(connection)?;
            }
            let deleted = puzzle_deleted_attempts::table
                .filter(puzzle_deleted_attempts::run_id.eq(&id))
                .select(puzzle_deleted_attempts::attempt_id)
                .load::<String>(connection)?;
            for (attempt, body) in snapshot.attempt_rows()? {
                let attempt = attempt.to_string();
                if deleted.contains(&attempt) {
                    continue;
                }
                diesel::insert_into(puzzle_attempts::table)
                    .values(&StoragePuzzleNewAttempt {
                        run_id: &id,
                        attempt_id: &attempt,
                        body: &body,
                    })
                    .on_conflict((puzzle_attempts::run_id, puzzle_attempts::attempt_id))
                    .do_update()
                    .set(puzzle_attempts::body.eq(diesel::upsert::excluded(puzzle_attempts::body)))
                    .execute(connection)?;
            }
            diesel::insert_into(puzzle_checkpoints::table)
                .values(&StoragePuzzleNewCheckpoint {
                    run_id: &id,
                    body: snapshot.checkpoint(),
                })
                .on_conflict(puzzle_checkpoints::run_id)
                .do_update()
                .set(
                    puzzle_checkpoints::body.eq(diesel::upsert::excluded(puzzle_checkpoints::body)),
                )
                .execute(connection)?;
            diesel::update(puzzle_runs::table.filter(puzzle_runs::run_id.eq(&id)))
                .set(&StoragePuzzleRunAdvance {
                    storage_revision: &revision.to_string(),
                    event_count: i32::try_from(snapshot.event_count())?,
                    journal_digest: snapshot.journal_proof().bytes().as_slice(),
                })
                .execute(connection)?;
            Ok::<u64, StoragePuzzleError>(revision)
        })?;
        Ok(PuzzlePersistenceReceipt::committed(
            request,
            PuzzlePersistenceStorageRevision::new(revision),
        ))
    }

    pub(super) fn delete(
        connection: &mut SqliteConnection,
        authorization: &PuzzleHistoryAuthorization,
    ) -> Result<usize, StoragePuzzleError> {
        connection.immediate_transaction(|connection| match authorization.scope() {
            PuzzleHistoryEnumScope::Attempt { session, attempt } => {
                let run = session.value().get().to_string();
                let attempt = attempt.value().get().to_string();
                let removed = diesel::delete(
                    puzzle_attempts::table
                        .filter(puzzle_attempts::run_id.eq(&run))
                        .filter(puzzle_attempts::attempt_id.eq(&attempt)),
                )
                .execute(connection)?;
                if removed > 0 {
                    diesel::insert_into(puzzle_deleted_attempts::table)
                        .values((
                            puzzle_deleted_attempts::run_id.eq(&run),
                            puzzle_deleted_attempts::attempt_id.eq(&attempt),
                        ))
                        .execute(connection)?;
                }
                Ok(removed)
            }
            PuzzleHistoryEnumScope::Session(session) => {
                let id = session.value().get().to_string();
                diesel::delete(
                    puzzle_deleted_attempts::table.filter(puzzle_deleted_attempts::run_id.eq(&id)),
                )
                .execute(connection)?;
                diesel::delete(puzzle_attempts::table.filter(puzzle_attempts::run_id.eq(&id)))
                    .execute(connection)?;
                diesel::delete(
                    puzzle_checkpoints::table.filter(puzzle_checkpoints::run_id.eq(&id)),
                )
                .execute(connection)?;
                diesel::delete(puzzle_events::table.filter(puzzle_events::run_id.eq(&id)))
                    .execute(connection)?;
                let runs = diesel::delete(puzzle_runs::table.filter(puzzle_runs::run_id.eq(&id)))
                    .execute(connection)?;
                diesel::delete(
                    puzzle_collections::table.filter(puzzle_collections::collection_id.eq(&id)),
                )
                .execute(connection)?;
                Ok(runs)
            }
            PuzzleHistoryEnumScope::All => {
                diesel::delete(puzzle_deleted_attempts::table).execute(connection)?;
                diesel::delete(puzzle_attempts::table).execute(connection)?;
                diesel::delete(puzzle_checkpoints::table).execute(connection)?;
                diesel::delete(puzzle_events::table).execute(connection)?;
                let runs = diesel::delete(puzzle_runs::table).execute(connection)?;
                diesel::delete(puzzle_collections::table).execute(connection)?;
                Ok(runs)
            }
        })
    }

    pub(super) fn load(
        connection: &mut SqliteConnection,
        id: PuzzleSessionId,
        memory: &Arc<tokio::sync::Semaphore>,
    ) -> Result<Option<StoragePuzzleRecovered>, StoragePuzzleError> {
        connection.transaction::<_, StoragePuzzleError, _>(|connection| {
            let id_value = id.value().get().to_string();
            let limit = i64::try_from(PuzzlePersistenceCodec::MAX_DOCUMENT_BYTES)?;
            let metadata = diesel::sql_query(
                "SELECT r.storage_revision AS revision, r.event_count AS count, c.digest AS digest, c.schema_version AS schema, r.journal_version AS journal_version, r.journal_digest AS journal_digest, CASE WHEN length(CAST(c.body AS BLOB)) <= ?2 THEN c.body END AS collection, CASE WHEN length(CAST(p.body AS BLOB)) <= ?2 THEN p.body END AS checkpoint FROM puzzle_runs r JOIN puzzle_collections c ON c.collection_id = r.collection_id JOIN puzzle_checkpoints p ON p.run_id = r.run_id WHERE r.run_id = ?1",
            )
            .bind::<Text, _>(&id_value)
            .bind::<BigInt, _>(limit)
            .get_result::<StoragePuzzleGuardedRunRow>(connection)
            .optional()?
            .map(StoragePuzzleStoredRun::from);
            let Some(metadata) = metadata else {
                let exists = diesel::select(diesel::dsl::exists(
                    puzzle_runs::table.filter(puzzle_runs::run_id.eq(&id_value)),
                ))
                .get_result::<bool>(connection)?;
                return if exists {
                    Err(StoragePuzzleError::Corrupt)
                } else {
                    Ok(None)
                };
            };
            let collection = metadata.collection.ok_or(PuzzlePersistenceError::Budget)?;
            let checkpoint = metadata.checkpoint.ok_or(PuzzlePersistenceError::Budget)?;
            let journal_proof = metadata.prefix.validate()?;
            let count = usize::try_from(metadata.prefix.count)?;
            let revision = metadata.prefix.revision;
            let digest: [u8; 32] = Sha256::digest(collection.as_bytes()).into();
            if metadata.prefix.digest.as_slice() != digest.as_slice() {
                return Err(StoragePuzzleError::Corrupt);
            }
            let event_bytes = puzzle_events::table
                .filter(puzzle_events::run_id.eq(&id_value))
                .select(diesel::dsl::sql::<BigInt>(
                    "coalesce(sum(length(CAST(body AS BLOB))), 0)",
                ))
                .get_result::<i64>(connection)?;
            let event_bytes = usize::try_from(event_bytes)?;
            let bytes = collection
                .len()
                .checked_add(checkpoint.len())
                .and_then(|bytes| bytes.checked_add(event_bytes))
                .ok_or(PuzzlePersistenceError::Budget)?;
            if bytes > PuzzlePersistenceCodec::MAX_DOCUMENT_BYTES {
                return Err(PuzzlePersistenceError::Budget.into());
            }
            let memory_reservation = StorageDocumentClient::reserve_memory(memory, bytes)?;
            let mut replayed_proof = PuzzlePersistenceJournalProof::initial(&digest);
            let stored_events = puzzle_events::table
                .filter(puzzle_events::run_id.eq(&id_value))
                .order(puzzle_events::ordinal.asc())
                .select((puzzle_events::ordinal, puzzle_events::body))
                .load::<(i32, String)>(connection)?;
            let mut events = Vec::with_capacity(count);
            for (ordinal, body) in stored_events {
                let ordinal = usize::try_from(ordinal)?;
                if ordinal != events.len() || ordinal >= count {
                    return Err(StoragePuzzleError::Corrupt);
                }
                replayed_proof = replayed_proof.extend(&body)?;
                events.push(body);
            }
            if events.len() != count || replayed_proof != journal_proof {
                return Err(StoragePuzzleError::Corrupt);
            }
            let owner = PuzzlePersistenceCodec::decode_parts(&collection, events, &checkpoint)?;
            if owner.run().context().session_id() != id
                || owner.journal_proof_at(count)? != journal_proof
            {
                return Err(StoragePuzzleError::Corrupt);
            }
            let deleted_attempts = puzzle_deleted_attempts::table
                .filter(puzzle_deleted_attempts::run_id.eq(&id_value))
                .select(puzzle_deleted_attempts::attempt_id)
                .load::<String>(connection)?
                .into_iter()
                .map(|attempt| attempt.parse::<u64>())
                .collect::<Result<Vec<_>, _>>()?;
            let expected_attempts = owner
                .attempt_rows()?
                .into_iter()
                .filter(|(attempt, _body)| !deleted_attempts.contains(attempt))
                .collect::<Vec<_>>();
            let rows = diesel::sql_query(
                "SELECT attempt_id, CASE WHEN length(CAST(body AS BLOB)) <= ?2 THEN body END AS body FROM puzzle_attempts WHERE run_id = ?1 ORDER BY attempt_id LIMIT ?3",
            )
            .bind::<Text, _>(&id_value)
            .bind::<BigInt, _>(limit)
            .bind::<BigInt, _>(i64::try_from(expected_attempts.len())?.saturating_add(1))
            .load::<StoragePuzzleGuardedAttemptRow>(connection)?;
            let mut attempts = std::collections::BTreeMap::new();
            for row in rows {
                if attempts.len() >= expected_attempts.len() {
                    return Err(StoragePuzzleError::Corrupt);
                }
                let body = row.body.ok_or(PuzzlePersistenceError::Budget)?;
                attempts.insert(row.attempt_id.parse::<u64>()?, body);
            }
            let stored_attempts = attempts.into_iter().collect::<Vec<_>>();
            if expected_attempts != stored_attempts {
                return Err(StoragePuzzleError::Corrupt);
            }
            let storage_revision = PuzzlePersistenceStorageRevision::new(revision.parse()?);
            if storage_revision == PuzzlePersistenceStorageRevision::INITIAL {
                return Err(StoragePuzzleError::Corrupt);
            }
            Ok(Some(StoragePuzzleRecovered {
                owner,
                storage_revision,
                persisted_event_count: count,
                deleted_attempts: deleted_attempts
                    .into_iter()
                    .map(|attempt| {
                        NonZeroU64::new(attempt)
                            .map(PuzzleAttemptId::new)
                            .ok_or(StoragePuzzleError::Corrupt)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                _memory_reservation: memory_reservation,
            }))
        })
    }
}
