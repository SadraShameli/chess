use std::num::{NonZeroU64, TryFromIntError};
use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, oneshot};

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

#[derive(Debug, Error)]
pub enum StoragePuzzleError {
    #[error(transparent)]
    Document(#[from] StorageDocumentError),
    #[error(transparent)]
    Codec(#[from] PuzzlePersistenceError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("The puzzle storage revision changed: expected {expected}, found {actual}")]
    Conflict { expected: u64, actual: u64 },
    #[error("The pinned puzzle collection or event prefix changed")]
    CollectionConflict,
    #[error("The saved puzzle repository rows are incomplete or inconsistent")]
    Corrupt,
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

    fn read(row: &rusqlite::Row<'_>) -> Result<Self, StoragePuzzleError> {
        let id = PuzzleSessionId::new(Self::canonical_decimal(row.get(0)?)?);
        let storage_revision =
            PuzzlePersistenceStorageRevision::new(Self::canonical_decimal(row.get(1)?)?.get());
        let persisted_event_count =
            usize::try_from(row.get::<_, i64>(2)?).map_err(|_error| StoragePuzzleError::Corrupt)?;
        let has_collection: bool = row.get(3)?;
        let has_checkpoint: bool = row.get(4)?;
        if persisted_event_count > PuzzlePersistenceCodec::MAX_EVENTS
            || !has_collection
            || !has_checkpoint
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
    count: i64,
    digest: Vec<u8>,
    schema: u32,
    journal_version: u32,
    journal_digest: Vec<u8>,
}

impl StoragePuzzleStoredPrefix {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            revision: row.get(0)?,
            count: row.get(1)?,
            digest: row.get(2)?,
            schema: row.get(3)?,
            journal_version: row.get(4)?,
            journal_digest: row.get(5)?,
        })
    }

    fn validate(&self) -> Result<PuzzlePersistenceJournalProof, StoragePuzzleError> {
        let count = usize::try_from(self.count)?;
        if self.schema != PuzzlePersistenceCodec::SCHEMA || self.digest.len() != 32 {
            return Err(StoragePuzzleError::Corrupt);
        }
        if count > PuzzlePersistenceCodec::MAX_EVENTS {
            return Err(PuzzlePersistenceError::Budget.into());
        }
        let proof =
            PuzzlePersistenceJournalProof::restore(self.journal_version, &self.journal_digest)?;
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

struct StoragePuzzleStoredRun {
    prefix: StoragePuzzleStoredPrefix,
    collection: Option<String>,
    checkpoint: Option<String>,
}

pub(super) struct StoragePuzzleTransaction;

impl StoragePuzzleTransaction {
    pub(super) fn inventory(
        connection: &Connection,
        after: Option<PuzzleSessionId>,
        memory_reservation: OwnedSemaphorePermit,
    ) -> Result<StoragePuzzleRunPage, StoragePuzzleError> {
        let mut statement = connection.prepare_cached(
            "SELECT CASE WHEN length(CAST(r.run_id AS BLOB)) <= ?3 THEN r.run_id END, CASE WHEN length(CAST(r.storage_revision AS BLOB)) <= ?3 THEN r.storage_revision END, r.event_count, EXISTS(SELECT 1 FROM puzzle_collections c WHERE c.collection_id = r.collection_id), EXISTS(SELECT 1 FROM puzzle_checkpoints p WHERE p.run_id = r.run_id) FROM puzzle_runs r WHERE ?1 IS NULL OR (length(r.run_id), r.run_id COLLATE BINARY) > (length(?1), ?1 COLLATE BINARY) ORDER BY length(r.run_id), r.run_id COLLATE BINARY LIMIT ?2",
        )?;
        let cursor = after.map(|id| id.value().get().to_string());
        let mut rows = statement.query(params![
            cursor,
            i64::try_from(StoragePuzzleRepository::INVENTORY_PAGE_SIZE)?,
            i64::try_from(StoragePuzzleRunMetadata::MAX_DECIMAL_BYTES)?,
        ])?;
        let mut entries = Vec::with_capacity(StoragePuzzleRepository::INVENTORY_PAGE_SIZE);
        while let Some(row) = rows.next()? {
            entries.push(StoragePuzzleRunMetadata::read(row)?);
        }
        Ok(StoragePuzzleRunPage {
            entries,
            _memory_reservation: memory_reservation,
        })
    }

    pub(super) fn save(
        connection: &mut Connection,
        request: PuzzlePersistenceSaveRequest,
    ) -> Result<PuzzlePersistenceReceipt, StoragePuzzleError> {
        let snapshot = request.snapshot();
        let id = snapshot.context().session_id().value().get().to_string();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored: Option<StoragePuzzleStoredPrefix> = transaction.query_row(
            "SELECT r.storage_revision, r.event_count, c.digest, c.schema_version, r.journal_version, r.journal_digest FROM puzzle_runs r JOIN puzzle_collections c ON c.collection_id = r.collection_id WHERE r.run_id = ?1", [&id], StoragePuzzleStoredPrefix::read
        ).optional()?;
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
                transaction.execute("INSERT INTO puzzle_collections (collection_id, schema_version, digest, body) VALUES (?1, ?2, ?3, ?4)", params![id, PuzzlePersistenceCodec::SCHEMA, snapshot.collection_digest(), snapshot.collection()])?;
                transaction.execute("INSERT INTO puzzle_runs (run_id, collection_id, storage_revision, event_count, journal_version, journal_digest) VALUES (?1, ?1, '0', 0, ?2, ?3)", params![id, PuzzlePersistenceJournalProof::VERSION, snapshot.base_journal_proof().bytes()])?;
            }
        }
        {
            let mut insert = transaction.prepare_cached(
                "INSERT INTO puzzle_events (run_id, ordinal, body) VALUES (?1, ?2, ?3)",
            )?;
            for (offset, event) in snapshot.events().enumerate() {
                let ordinal = snapshot
                    .base_event_count()
                    .checked_add(offset)
                    .ok_or(StoragePuzzleError::Corrupt)?;
                insert.execute(params![id, i64::try_from(ordinal)?, event])?;
            }
        }
        for (attempt, body) in snapshot.attempt_rows()? {
            transaction.execute("INSERT INTO puzzle_attempts (run_id, attempt_id, body) VALUES (?1, ?2, ?3) ON CONFLICT(run_id, attempt_id) DO UPDATE SET body = excluded.body", params![id, attempt.to_string(), body])?;
        }
        transaction.execute("INSERT INTO puzzle_checkpoints (run_id, body) VALUES (?1, ?2) ON CONFLICT(run_id) DO UPDATE SET body = excluded.body", params![id, snapshot.checkpoint()])?;
        transaction.execute(
            "UPDATE puzzle_runs SET storage_revision = ?2, event_count = ?3, journal_digest = ?4 WHERE run_id = ?1",
            params![
                id,
                revision.to_string(),
                i64::try_from(snapshot.event_count())?,
                snapshot.journal_proof().bytes()
            ],
        )?;
        transaction.commit()?;
        Ok(PuzzlePersistenceReceipt::committed(
            request,
            PuzzlePersistenceStorageRevision::new(revision),
        ))
    }

    pub(super) fn load(
        connection: &mut Connection,
        id: PuzzleSessionId,
        memory: &Arc<tokio::sync::Semaphore>,
    ) -> Result<Option<StoragePuzzleRecovered>, StoragePuzzleError> {
        let transaction = connection.transaction()?;
        let id_value = id.value().get().to_string();
        let metadata: Option<StoragePuzzleStoredRun> = transaction.query_row(
            "SELECT r.storage_revision, r.event_count, c.digest, c.schema_version, r.journal_version, r.journal_digest, CASE WHEN length(CAST(c.body AS BLOB)) <= ?2 THEN c.body END, CASE WHEN length(CAST(p.body AS BLOB)) <= ?2 THEN p.body END FROM puzzle_runs r JOIN puzzle_collections c ON c.collection_id = r.collection_id JOIN puzzle_checkpoints p ON p.run_id = r.run_id WHERE r.run_id = ?1", params![id_value, i64::try_from(PuzzlePersistenceCodec::MAX_DOCUMENT_BYTES)?], |row| Ok(StoragePuzzleStoredRun { prefix: StoragePuzzleStoredPrefix::read(row)?, collection: row.get(6)?, checkpoint: row.get(7)? })
        ).optional()?;
        let Some(metadata) = metadata else {
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM puzzle_runs WHERE run_id = ?1)",
                [&id_value],
                |row| row.get(0),
            )?;
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
        let event_bytes: i64 = transaction.query_row("SELECT coalesce(sum(length(CAST(body AS BLOB))), 0) FROM puzzle_events WHERE run_id = ?1", [&id_value], |row| row.get(0))?;
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
        let events = {
            let mut statement = transaction.prepare_cached(
                "SELECT ordinal, body FROM puzzle_events WHERE run_id = ?1 ORDER BY ordinal",
            )?;
            let mut rows = statement.query([&id_value])?;
            let mut events = Vec::with_capacity(count);
            while let Some(row) = rows.next()? {
                let ordinal = usize::try_from(row.get::<_, i64>(0)?)?;
                if ordinal != events.len() || ordinal >= count {
                    return Err(StoragePuzzleError::Corrupt);
                }
                let body: String = row.get(1)?;
                replayed_proof = replayed_proof.extend(&body)?;
                events.push(body);
            }
            events
        };
        if events.len() != count || replayed_proof != journal_proof {
            return Err(StoragePuzzleError::Corrupt);
        }
        let owner = PuzzlePersistenceCodec::decode_parts(&collection, events, &checkpoint)?;
        if owner.run().context().session_id() != id
            || owner.journal_proof_at(count)? != journal_proof
        {
            return Err(StoragePuzzleError::Corrupt);
        }
        let expected_attempts = owner.attempt_rows()?;
        let stored_attempts = {
            let mut statement = transaction.prepare_cached("SELECT attempt_id, CASE WHEN length(CAST(body AS BLOB)) <= ?2 THEN body END FROM puzzle_attempts WHERE run_id = ?1 ORDER BY attempt_id")?;
            let mut rows = statement.query(params![
                id_value,
                i64::try_from(PuzzlePersistenceCodec::MAX_DOCUMENT_BYTES)?
            ])?;
            let mut attempts = std::collections::BTreeMap::new();
            while let Some(row) = rows.next()? {
                if attempts.len() >= expected_attempts.len() {
                    return Err(StoragePuzzleError::Corrupt);
                }
                let attempt: String = row.get(0)?;
                let body: Option<String> = row.get(1)?;
                let body = body.ok_or(PuzzlePersistenceError::Budget)?;
                attempts.insert(attempt.parse::<u64>()?, body);
            }
            attempts.into_iter().collect::<Vec<_>>()
        };
        if expected_attempts != stored_attempts {
            return Err(StoragePuzzleError::Corrupt);
        }
        let storage_revision = PuzzlePersistenceStorageRevision::new(revision.parse()?);
        if storage_revision == PuzzlePersistenceStorageRevision::INITIAL {
            return Err(StoragePuzzleError::Corrupt);
        }
        transaction.commit()?;
        Ok(Some(StoragePuzzleRecovered {
            owner,
            storage_revision,
            persisted_event_count: count,
            _memory_reservation: memory_reservation,
        }))
    }
}
