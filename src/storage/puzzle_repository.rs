use std::num::{NonZeroU64, TryFromIntError};
use std::sync::Arc;

use diesel::connection::DefaultLoadingMode;
use diesel::dsl::{case_when, exists, sql};
use diesel::expression::SqlLiteral;
use diesel::sql_types::{BigInt, Text};
use diesel::sqlite::{Sqlite, SqliteConnection};
use diesel::{
    BoolExpressionMethods, Connection, ExpressionMethods, JoinOnDsl, NullableExpressionMethods,
    OptionalExtension, QueryDsl, Queryable, RunQueryDsl, Selectable, SelectableHelper,
};
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
use crate::storage::schema::{
    puzzle_attempts, puzzle_checkpoints, puzzle_collections, puzzle_events, puzzle_runs,
};

#[derive(Queryable)]
struct StoragePuzzleInventoryRow {
    id: Option<String>,
    revision: Option<String>,
    count: i64,
    has_collection: bool,
    has_checkpoint: bool,
}

#[derive(Debug, Error)]
pub enum StoragePuzzleError {
    #[error(transparent)]
    Document(#[from] StorageDocumentError),
    #[error(transparent)]
    Codec(#[from] PuzzlePersistenceError),
    #[error(transparent)]
    Sqlite(#[from] diesel::result::Error),
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

    fn read(row: StoragePuzzleInventoryRow) -> Result<Self, StoragePuzzleError> {
        let id = PuzzleSessionId::new(Self::canonical_decimal(row.id)?);
        let storage_revision =
            PuzzlePersistenceStorageRevision::new(Self::canonical_decimal(row.revision)?.get());
        let persisted_event_count =
            usize::try_from(row.count).map_err(|_error| StoragePuzzleError::Corrupt)?;
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

#[derive(Queryable, Selectable)]
#[diesel(table_name = puzzle_runs, check_for_backend(Sqlite))]
struct StoragePuzzleStoredPrefix {
    #[diesel(column_name = storage_revision)]
    revision: String,
    #[diesel(column_name = event_count)]
    count: i64,
    #[diesel(select_expression = puzzle_collections::digest)]
    digest: Vec<u8>,
    #[diesel(select_expression = puzzle_collections::schema_version)]
    schema: i64,
    journal_version: i64,
    journal_digest: Vec<u8>,
}

impl StoragePuzzleStoredPrefix {
    fn read(
        connection: &mut SqliteConnection,
        id: &str,
    ) -> Result<Option<Self>, StoragePuzzleError> {
        Ok(puzzle_runs::table
            .inner_join(
                puzzle_collections::table
                    .on(puzzle_collections::collection_id.eq(puzzle_runs::collection_id)),
            )
            .filter(puzzle_runs::run_id.eq(id))
            .select(Self::as_select())
            .first(connection)
            .optional()?)
    }

    fn validate(&self) -> Result<PuzzlePersistenceJournalProof, StoragePuzzleError> {
        let count = usize::try_from(self.count)?;
        if self.schema != i64::from(PuzzlePersistenceCodec::SCHEMA)
            || self.digest.len() != Sha256::output_size()
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

#[derive(Queryable, Selectable)]
#[diesel(table_name = puzzle_runs, check_for_backend(Sqlite))]
struct StoragePuzzleStoredRun {
    #[diesel(embed)]
    prefix: StoragePuzzleStoredPrefix,
    #[diesel(select_expression = sql::<BigInt>("length(CAST(puzzle_collections.body AS BLOB))"))]
    #[diesel(select_expression_type = SqlLiteral<BigInt>)]
    collection_bytes: i64,
    #[diesel(select_expression = sql::<BigInt>("length(CAST(puzzle_checkpoints.body AS BLOB))"))]
    #[diesel(select_expression_type = SqlLiteral<BigInt>)]
    checkpoint_bytes: i64,
}

pub(super) struct StoragePuzzleTransaction;

impl StoragePuzzleTransaction {
    pub(super) fn inventory(
        connection: &mut SqliteConnection,
        after: Option<PuzzleSessionId>,
        memory_reservation: OwnedSemaphorePermit,
    ) -> Result<StoragePuzzleRunPage, StoragePuzzleError> {
        let maximum = i64::try_from(StoragePuzzleRunMetadata::MAX_DECIMAL_BYTES)?;
        let mut query = puzzle_runs::table.into_boxed::<Sqlite>();
        if let Some(after) = after {
            let key = after.value().get().to_string();
            let length = i64::try_from(key.len())?;
            query = query.filter(
                sql::<BigInt>("length(puzzle_runs.run_id)")
                    .gt(length)
                    .or(sql::<BigInt>("length(puzzle_runs.run_id)")
                        .eq(length)
                        .and(sql::<Text>("puzzle_runs.run_id COLLATE BINARY").gt(key))),
            );
        }
        let rows = query
            .order((
                sql::<BigInt>("length(puzzle_runs.run_id)").asc(),
                sql::<Text>("puzzle_runs.run_id COLLATE BINARY").asc(),
            ))
            .limit(i64::try_from(StoragePuzzleRepository::INVENTORY_PAGE_SIZE)?)
            .select((
                case_when(
                    sql::<BigInt>("length(CAST(puzzle_runs.run_id AS BLOB))").le(maximum),
                    puzzle_runs::run_id.nullable(),
                )
                .otherwise(None::<String>),
                case_when(
                    sql::<BigInt>("length(CAST(puzzle_runs.storage_revision AS BLOB))").le(maximum),
                    puzzle_runs::storage_revision.nullable(),
                )
                .otherwise(None::<String>),
                puzzle_runs::event_count,
                exists(
                    puzzle_collections::table
                        .filter(puzzle_collections::collection_id.eq(puzzle_runs::collection_id)),
                ),
                exists(
                    puzzle_checkpoints::table
                        .filter(puzzle_checkpoints::run_id.eq(puzzle_runs::run_id)),
                ),
            ))
            .load_iter::<StoragePuzzleInventoryRow, DefaultLoadingMode>(connection)?;
        let mut entries = Vec::with_capacity(StoragePuzzleRepository::INVENTORY_PAGE_SIZE);
        for row in rows {
            entries.push(StoragePuzzleRunMetadata::read(row?)?);
        }
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
        let revision = connection.immediate_transaction(|transaction| {
            let stored = StoragePuzzleStoredPrefix::read(transaction, &id)?;
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
                        .values((
                            puzzle_collections::collection_id.eq(&id),
                            puzzle_collections::schema_version
                                .eq(i64::from(PuzzlePersistenceCodec::SCHEMA)),
                            puzzle_collections::digest.eq(snapshot.collection_digest().as_slice()),
                            puzzle_collections::body.eq(snapshot.collection()),
                        ))
                        .execute(transaction)?;
                    let proof = snapshot.base_journal_proof();
                    diesel::insert_into(puzzle_runs::table)
                        .values((
                            puzzle_runs::run_id.eq(&id),
                            puzzle_runs::collection_id.eq(&id),
                            puzzle_runs::storage_revision.eq(
                                PuzzlePersistenceStorageRevision::INITIAL
                                    .value()
                                    .to_string(),
                            ),
                            puzzle_runs::event_count.eq(0_i64),
                            puzzle_runs::journal_version
                                .eq(i64::from(PuzzlePersistenceJournalProof::VERSION)),
                            puzzle_runs::journal_digest.eq(proof.bytes().as_slice()),
                        ))
                        .execute(transaction)?;
                }
            }
            for (offset, event) in snapshot.events().enumerate() {
                let ordinal = snapshot
                    .base_event_count()
                    .checked_add(offset)
                    .ok_or(StoragePuzzleError::Corrupt)?;
                diesel::insert_into(puzzle_events::table)
                    .values((
                        puzzle_events::run_id.eq(&id),
                        puzzle_events::ordinal.eq(i64::try_from(ordinal)?),
                        puzzle_events::body.eq(event),
                    ))
                    .execute(transaction)?;
            }
            for (attempt, body) in snapshot.attempt_rows()? {
                diesel::insert_into(puzzle_attempts::table)
                    .values((
                        puzzle_attempts::run_id.eq(&id),
                        puzzle_attempts::attempt_id.eq(attempt.to_string()),
                        puzzle_attempts::body.eq(&body),
                    ))
                    .on_conflict((puzzle_attempts::run_id, puzzle_attempts::attempt_id))
                    .do_update()
                    .set(puzzle_attempts::body.eq(&body))
                    .execute(transaction)?;
            }
            diesel::insert_into(puzzle_checkpoints::table)
                .values((
                    puzzle_checkpoints::run_id.eq(&id),
                    puzzle_checkpoints::body.eq(snapshot.checkpoint()),
                ))
                .on_conflict(puzzle_checkpoints::run_id)
                .do_update()
                .set(puzzle_checkpoints::body.eq(snapshot.checkpoint()))
                .execute(transaction)?;
            let proof = snapshot.journal_proof();
            diesel::update(puzzle_runs::table.find(&id))
                .set((
                    puzzle_runs::storage_revision.eq(revision.to_string()),
                    puzzle_runs::event_count.eq(i64::try_from(snapshot.event_count())?),
                    puzzle_runs::journal_digest.eq(proof.bytes().as_slice()),
                ))
                .execute(transaction)?;
            Ok(revision)
        })?;
        Ok(PuzzlePersistenceReceipt::committed(
            request,
            PuzzlePersistenceStorageRevision::new(revision),
        ))
    }

    pub(super) fn load(
        connection: &mut SqliteConnection,
        id: PuzzleSessionId,
        memory: &Arc<tokio::sync::Semaphore>,
    ) -> Result<Option<StoragePuzzleRecovered>, StoragePuzzleError> {
        connection.transaction(|transaction| {
            let id_value = id.value().get().to_string();
            let run = || {
                puzzle_runs::table
                    .inner_join(
                        puzzle_collections::table
                            .on(puzzle_collections::collection_id.eq(puzzle_runs::collection_id)),
                    )
                    .inner_join(
                        puzzle_checkpoints::table
                            .on(puzzle_checkpoints::run_id.eq(puzzle_runs::run_id)),
                    )
                    .filter(puzzle_runs::run_id.eq(&id_value))
            };
            let metadata = run()
                .select(StoragePuzzleStoredRun::as_select())
                .first::<StoragePuzzleStoredRun>(transaction)
                .optional()?;
            let Some(metadata) = metadata else {
                let found: bool = diesel::select(exists(puzzle_runs::table.find(&id_value)))
                    .get_result(transaction)?;
                return if found {
                    Err(StoragePuzzleError::Corrupt)
                } else {
                    Ok(None)
                };
            };
            let journal_proof = metadata.prefix.validate()?;
            let count = usize::try_from(metadata.prefix.count)?;
            let revision = metadata.prefix.revision;
            let collection_bytes = usize::try_from(metadata.collection_bytes)?;
            let checkpoint_bytes = usize::try_from(metadata.checkpoint_bytes)?;
            let event_bytes: i64 = puzzle_events::table
                .filter(puzzle_events::run_id.eq(&id_value))
                .select(sql::<BigInt>(
                    "coalesce(sum(length(CAST(body AS BLOB))), 0)",
                ))
                .get_result(transaction)?;
            let event_bytes = usize::try_from(event_bytes)?;
            let bytes = collection_bytes
                .checked_add(checkpoint_bytes)
                .and_then(|bytes| bytes.checked_add(event_bytes))
                .ok_or(PuzzlePersistenceError::Budget)?;
            if bytes > PuzzlePersistenceCodec::MAX_DOCUMENT_BYTES {
                return Err(PuzzlePersistenceError::Budget.into());
            }
            let memory_reservation = StorageDocumentClient::reserve_memory(memory, bytes)?;
            let (collection, checkpoint): (String, String) = run()
                .select((puzzle_collections::body, puzzle_checkpoints::body))
                .first(transaction)?;
            let digest: [u8; 32] = Sha256::digest(collection.as_bytes()).into();
            if metadata.prefix.digest.as_slice() != digest.as_slice() {
                return Err(StoragePuzzleError::Corrupt);
            }
            let mut replayed_proof = PuzzlePersistenceJournalProof::initial(&digest);
            let rows = puzzle_events::table
                .filter(puzzle_events::run_id.eq(&id_value))
                .order(puzzle_events::ordinal.asc())
                .select((puzzle_events::ordinal, puzzle_events::body))
                .load_iter::<(i64, String), DefaultLoadingMode>(transaction)?;
            let mut events = Vec::with_capacity(count);
            for row in rows {
                let (ordinal, body) = row?;
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
            let expected_attempts = owner.attempt_rows()?;
            let rows = puzzle_attempts::table
                .filter(puzzle_attempts::run_id.eq(&id_value))
                .order(puzzle_attempts::attempt_id.asc())
                .select((
                    puzzle_attempts::attempt_id,
                    case_when(
                        sql::<BigInt>("length(CAST(body AS BLOB))")
                            .le(i64::try_from(PuzzlePersistenceCodec::MAX_DOCUMENT_BYTES)?),
                        puzzle_attempts::body.nullable(),
                    )
                    .otherwise(None::<String>),
                ))
                .load_iter::<(String, Option<String>), DefaultLoadingMode>(transaction)?;
            let mut attempts = std::collections::BTreeMap::new();
            for row in rows {
                let (attempt, body) = row?;
                if attempts.len() >= expected_attempts.len() {
                    return Err(StoragePuzzleError::Corrupt);
                }
                attempts.insert(
                    attempt.parse::<u64>()?,
                    body.ok_or(PuzzlePersistenceError::Budget)?,
                );
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
                _memory_reservation: memory_reservation,
            }))
        })
    }
}
