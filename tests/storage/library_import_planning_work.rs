use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use diesel::Connection as _;
use diesel::SqliteConnection;
use diesel::connection::InstrumentationEvent;
use rusqlite::Connection;
use tokio::sync::Semaphore;

use crate::board::start::BoardStart;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::{CapacityExecutor, CapacityExecutorCancellation};
use crate::game::enums::GameId;
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::move_node::GameMoveNodeAnnotations;
use crate::game::session::{GameMetadata, GameMetadataKnownTags, GameMetadataText};
use crate::game::storage_repository::GameStorageRevision;
use crate::game::tree::GameTree;
use crate::notation::game::{NotationGame, NotationGameParts};
use crate::notation::game_identity::NotationGameIdentityEnumResult;
use crate::package::digest::PackageDigest;
use crate::storage::connection::StorageConnection;
use crate::storage::document_store::{StorageDocumentClient, StorageDocumentStore};
use crate::storage::game_document::StorageGameDocument;
use crate::storage::import_session::{
    StorageImportSessionCancellation, StorageImportSessionCommitFootprint,
    StorageImportSessionError, StorageImportSessionPreparedCommit,
};
use crate::storage::library_import::StorageLibraryImportPageLimit;
use crate::storage::library_import::space::StorageLibraryImportSpace;
use crate::storage::library_import::space::layout::StorageLibraryImportSpaceLayout;
use crate::storage::library_import::space::trees::{
    StorageLibraryImportSpaceEnumCardinality, StorageLibraryImportSpaceEnumTable,
    StorageLibraryImportSpaceTrees,
};
use crate::storage::library_prepared::{
    StorageLibraryEnumContentChange, StorageLibraryPreparedMemory, StorageLibraryPreparedWrite,
    StorageLibraryPreparedWriteOptions,
};
use crate::storage::library_repository::StorageLibraryError;
use crate::storage::library_transaction::{
    StorageLibraryEnumTransactionRequest, StorageLibraryEnumTransactionResponse,
    StorageLibraryTransaction,
};
use crate::storage::migration::StorageMigrationRunner;

#[path = "library_import_space_budget.rs"]
mod budget_tests;

type StorageLibraryImportPlanningWorkResult<T = ()> =
    Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, PartialEq, Eq)]
struct StorageLibraryImportPlanningWorkFile {
    bytes: u64,
    digest: PackageDigest,
}

impl StorageLibraryImportPlanningWorkFile {
    fn read(path: &Path) -> StorageLibraryImportPlanningWorkResult<Self> {
        match File::open(path) {
            Ok(file) => Ok(Self {
                bytes: file.metadata()?.len(),
                digest: PackageDigest::read(file)?,
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                bytes: 0,
                digest: PackageDigest::hash_slice(&[]),
            }),
            Err(error) => Err(error.into()),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct StorageLibraryImportPlanningWorkDocument {
    id: GameId,
    revision: u64,
    body_bytes: u64,
    body_digest: PackageDigest,
}

#[derive(Debug, PartialEq, Eq)]
struct StorageLibraryImportPlanningWorkTreeShape {
    name: String,
    page_type: String,
    pages: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct StorageLibraryImportPlanningWorkSnapshot {
    main: StorageLibraryImportPlanningWorkFile,
    wal: StorageLibraryImportPlanningWorkFile,
    documents: Vec<StorageLibraryImportPlanningWorkDocument>,
    metadata: Vec<String>,
    schema: Vec<String>,
    nonoverflow: Vec<StorageLibraryImportPlanningWorkTreeShape>,
    document_overflow_pages: u64,
    catalog: String,
    total_changes: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct StorageLibraryImportPlanningWorkBound {
    inventory_vm_steps: u64,
    new_page_budget: u64,
    old_tree_pages: u64,
    allocator_operations: u64,
}

struct StorageLibraryImportPlanningWorkObservation {
    expected_documents: Vec<StorageLibraryImportPlanningWorkDocument>,
    checkpoints: [(i64, i64, i64); 2],
    snapshots: [StorageLibraryImportPlanningWorkSnapshot; 5],
    deferred: StorageLibraryImportPlanningWorkBound,
    immediate: StorageLibraryImportPlanningWorkBound,
    directory_removed: bool,
}

impl StorageLibraryImportPlanningWorkObservation {
    fn assert_controls(&self) {
        assert!(self.directory_removed);
        assert_eq!(self.checkpoints, [(0, 0, 0); 2]);
        let before = &self.snapshots[0];
        assert!(before.main.bytes > 0);
        assert_eq!(before.wal.bytes, 0);
        assert_eq!(before.documents, self.expected_documents);
        assert_eq!(before.metadata.len(), before.documents.len());
        assert!(!before.schema.is_empty());
        assert!(!before.nonoverflow.is_empty());
        for snapshot in &self.snapshots[1..] {
            assert_eq!(snapshot, before);
        }
        assert!(self.deferred.inventory_vm_steps > 0);
        assert_eq!(self.deferred, self.immediate);
    }
}

#[derive(Debug)]
pub(crate) struct StorageLibraryImportPlanningWorkRollback;

impl std::fmt::Display for StorageLibraryImportPlanningWorkRollback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("The planning transaction was rolled back on purpose")
    }
}

impl std::error::Error for StorageLibraryImportPlanningWorkRollback {}

impl StorageLibraryImportPlanningWorkRollback {
    pub(crate) fn run<T>(
        connection: &mut SqliteConnection,
        immediate: bool,
        work: impl FnOnce(&mut SqliteConnection) -> StorageLibraryImportPlanningWorkResult<T>,
    ) -> StorageLibraryImportPlanningWorkResult<T> {
        let mut slot = None;
        let body = |transaction: &mut SqliteConnection| -> StorageLibraryImportPlanningWorkResult {
            slot = Some(work(transaction));
            Err(Box::new(Self))
        };
        let outcome = if immediate {
            connection.immediate_transaction(body)
        } else {
            connection.transaction(body)
        };
        match outcome {
            Err(error) if error.is::<Self>() => {}
            Err(error) if error.to_string().contains("no transaction is active") => {}
            Err(error) => return Err(error),
            Ok(()) => {}
        }
        slot.ok_or("the planning transaction body did not run")?
    }
}

pub(crate) struct StorageLibraryImportPlanningWorkFixture {
    pub(crate) diesel: StorageConnection,
    pub(crate) connection: Connection,
    statements: Arc<std::sync::Mutex<Vec<String>>>,
    pub(crate) directory: tempfile::TempDir,
    expected_documents: Vec<StorageLibraryImportPlanningWorkDocument>,
}

impl StorageLibraryImportPlanningWorkFixture {
    const PAGE_BYTES: u64 = 4096;
    const OLD_RECORDS: u64 = 2;
    pub(crate) const SMALL_NOTE_BYTES: usize = 16;
    const EXTRA_OVERFLOW_PAGES_PER_DOCUMENT: u64 = 64;
    const TINY_BODY_BYTES: u64 = 1024;
    const TINY_PROJECTION_BYTES: u64 = 1024;

    fn footprint() -> StorageLibraryImportPlanningWorkResult<StorageImportSessionCommitFootprint> {
        Ok(StorageImportSessionCommitFootprint {
            count: 1,
            bodies: Self::TINY_BODY_BYTES,
            projections: Self::TINY_PROJECTION_BYTES,
            largest: Self::TINY_BODY_BYTES
                + Self::TINY_PROJECTION_BYTES
                + StorageImportSessionPreparedCommit::header_length()?,
        })
    }

    fn document(
        value: u64,
        note_bytes: usize,
    ) -> StorageLibraryImportPlanningWorkResult<(GameId, String)> {
        let id = uuid::Uuid::from_u128(u128::from(value))
            .to_string()
            .parse()?;
        let root = uuid::Uuid::from_u128(1000 + 2 * u128::from(value))
            .to_string()
            .parse()?;
        let tip = uuid::Uuid::from_u128(1001 + 2 * u128::from(value))
            .to_string()
            .parse()?;
        let start = BoardStart::standard();
        let movement = start.position().parse_uci("e2e4")?;
        let mut tree = GameTree::new(root, start.position().clone(), None);
        tree.append(root, tip, movement, None)?;
        tree.set_annotations(
            tip,
            GameMoveNodeAnnotations::new("n".repeat(note_bytes), Vec::new(), Vec::new())?,
        )?;
        let game = NotationGame::new(NotationGameParts {
            start,
            tree,
            mainline_tip: tip,
            metadata: GameMetadata::new(
                GameMetadataKnownTags {
                    white: Some(GameMetadataText::new("Retained white".to_owned())?),
                    black: Some(GameMetadataText::new("Retained black".to_owned())?),
                    ..GameMetadataKnownTags::default()
                },
                Vec::new(),
            )?,
            result: NotationGameIdentityEnumResult::Unfinished,
            result_header: None,
            time_control: None,
            clocks: Vec::new(),
            warnings: Vec::new(),
        })?;
        Ok((
            id,
            StorageGameDocument::capture_notation(id, &game)?.json()?,
        ))
    }

    fn new(
        note_bytes: usize,
        cancellation: &CapacityExecutorCancellation,
    ) -> StorageLibraryImportPlanningWorkResult<Self> {
        Self::with_records(note_bytes, Self::OLD_RECORDS, cancellation)
    }

    pub(crate) fn with_records(
        note_bytes: usize,
        records: u64,
        cancellation: &CapacityExecutorCancellation,
    ) -> StorageLibraryImportPlanningWorkResult<Self> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("planning.sqlite");
        let connection = Connection::open(&path)?;
        connection.pragma_update(None, "page_size", i64::try_from(Self::PAGE_BYTES)?)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "wal_autocheckpoint", 0)?;
        let mut diesel = StorageConnection::open(&path)?;
        StorageMigrationRunner::run_embedded(diesel.diesel())?;
        let statements = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&statements);
        diesel
            .diesel()
            .set_instrumentation(move |event: InstrumentationEvent<'_>| {
                if let InstrumentationEvent::StartQuery { query, .. } = event
                    && let Ok(mut log) = sink.lock()
                {
                    log.push(query.to_string());
                }
            });
        let memory = Arc::new(Semaphore::new(StorageDocumentStore::MEMORY_BUDGET));
        let mut expected_documents = Vec::new();
        for value in 1..=records {
            let (id, body) = Self::document(value, note_bytes)?;
            expected_documents.push(StorageLibraryImportPlanningWorkDocument {
                id,
                revision: 1,
                body_bytes: u64::try_from(body.len())?,
                body_digest: PackageDigest::hash_slice(body.as_bytes()),
            });
            let reservation = StorageDocumentClient::reserve_memory(
                &memory,
                StorageLibraryPreparedMemory::reservation_bytes(body.capacity())?,
            )?;
            let prepared = StorageLibraryPreparedWrite::from_encoded(
                StorageLibraryPreparedWriteOptions {
                    id,
                    expected: GameStorageRevision::INITIAL,
                    memory: StorageLibraryPreparedMemory::new(body.capacity(), reservation)?,
                    body,
                    change: StorageLibraryEnumContentChange::Changed(
                        GameLibraryOrganizationTimestamp::new(123_456)?,
                    ),
                },
                cancellation,
            )?;
            if !matches!(
                StorageLibraryTransaction::execute(
                    diesel.diesel(),
                    StorageLibraryEnumTransactionRequest::Save(prepared),
                    &memory,
                    uuid::Uuid::nil(),
                )?,
                StorageLibraryEnumTransactionResponse::Saved(_)
            ) {
                return Err("planning fixture did not save a real library record".into());
            }
        }
        Ok(Self {
            diesel,
            statements,
            connection,
            directory,
            expected_documents,
        })
    }

    pub(crate) fn statements(&self) -> StorageLibraryImportPlanningWorkResult<Vec<String>> {
        Ok(self
            .statements
            .lock()
            .map_err(|_poisoned| "the statement log is poisoned")?
            .iter()
            .map(|sql| sql.split(" -- binds").next().unwrap_or_default().to_owned())
            .collect())
    }

    pub(crate) fn checkpoint(&mut self) -> StorageLibraryImportPlanningWorkResult<(i64, i64, i64)> {
        use diesel::RunQueryDsl as _;

        #[derive(diesel::QueryableByName)]
        struct Checkpoint {
            #[diesel(sql_type = diesel::sql_types::BigInt)]
            busy: i64,
            #[diesel(sql_type = diesel::sql_types::BigInt)]
            log: i64,
            #[diesel(sql_type = diesel::sql_types::BigInt)]
            checkpointed: i64,
        }
        let row: Checkpoint = diesel::sql_query("PRAGMA wal_checkpoint(TRUNCATE)")
            .get_result(self.diesel.diesel())?;
        Ok((row.busy, row.log, row.checkpointed))
    }

    fn snapshot(
        &self,
    ) -> StorageLibraryImportPlanningWorkResult<StorageLibraryImportPlanningWorkSnapshot> {
        let mut documents = Vec::new();
        let mut statement = self
            .connection
            .prepare("SELECT id, revision, body FROM documents ORDER BY id")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let body: String = row.get(2)?;
            documents.push(StorageLibraryImportPlanningWorkDocument {
                id: row.get::<_, String>(0)?.parse()?,
                revision: row.get::<_, String>(1)?.parse()?,
                body_bytes: u64::try_from(body.len())?,
                body_digest: PackageDigest::hash_slice(body.as_bytes()),
            });
        }
        let metadata = self.connection.prepare(
            "SELECT json_array(game_id, content_revision, organization_revision, content_kind, rules_variant, start_kind, mode, result, white_display, white_key, black_display, black_key, date_year, date_month, date_day, opening_display, opening_key, opening_code, plies, identity_initial_fen, identity_mainline_uci, modified_ms, favorite, folder_id, location, trashed_ms) FROM library_entries ORDER BY game_id",
        )?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
        let schema = self
            .connection
            .prepare("SELECT sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY type, name")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        let nonoverflow = self.connection.prepare(
            "SELECT name, pagetype, count(*) FROM dbstat WHERE pagetype <> 'overflow' GROUP BY name, pagetype ORDER BY name, pagetype",
        )?
        .query_map([], |row| {
            let pages: i64 = row.get(2)?;
            Ok(StorageLibraryImportPlanningWorkTreeShape {
                name: row.get(0)?,
                page_type: row.get(1)?,
                pages: u64::try_from(pages)
                    .map_err(|_negative| rusqlite::Error::IntegralValueOutOfRange(2, pages))?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(StorageLibraryImportPlanningWorkSnapshot {
            main: StorageLibraryImportPlanningWorkFile::read(
                &self.directory.path().join("planning.sqlite"),
            )?,
            wal: StorageLibraryImportPlanningWorkFile::read(
                &self.directory.path().join("planning.sqlite-wal"),
            )?,
            documents,
            metadata,
            schema,
            nonoverflow,
            document_overflow_pages: u64::try_from(self.connection.query_row(
                "SELECT count(*) FROM dbstat WHERE name = 'documents' AND pagetype = 'overflow'",
                [],
                |row| row.get::<_, i64>(0),
            )?)?,
            catalog: self.connection.query_row(
                "SELECT revision FROM library_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )?,
            total_changes: self.connection.total_changes(),
        })
    }

    fn measure(
        &mut self,
        immediate: bool,
        footprint: StorageImportSessionCommitFootprint,
    ) -> StorageLibraryImportPlanningWorkResult<StorageLibraryImportPlanningWorkBound> {
        StorageLibraryImportPlanningWorkRollback::run(
            self.diesel.diesel(),
            immediate,
            |transaction| {
                let cancellation = StorageImportSessionCancellation::new();
                let layout = StorageLibraryImportSpaceLayout::read(transaction, &cancellation)?;
                let bound = StorageLibraryImportSpaceTrees::plan_budget(
                    transaction,
                    footprint,
                    layout.page,
                    &cancellation,
                )?;
                Ok(StorageLibraryImportPlanningWorkBound {
                    inventory_vm_steps: bound.inventory_vm_steps,
                    new_page_budget: bound.new_page_budget,
                    old_tree_pages: bound.old_tree_pages,
                    allocator_operations: bound.allocator_operations,
                })
            },
        )
    }

    fn observe(
        mut self,
        footprint: StorageImportSessionCommitFootprint,
    ) -> StorageLibraryImportPlanningWorkResult<StorageLibraryImportPlanningWorkObservation> {
        let first_checkpoint = self.checkpoint()?;
        let before = self.snapshot()?;
        let stable_before = self.snapshot()?;
        let deferred = self.measure(false, footprint)?;
        let after_deferred = self.snapshot()?;
        let immediate = self.measure(true, footprint)?;
        let after_immediate = self.snapshot()?;
        let last_checkpoint = self.checkpoint()?;
        let after_checkpoint = self.snapshot()?;
        self.connection
            .close()
            .map_err(|(_connection, error)| error)?;
        let path = self.directory.path().to_owned();
        self.directory.close()?;
        Ok(StorageLibraryImportPlanningWorkObservation {
            expected_documents: self.expected_documents,
            checkpoints: [first_checkpoint, last_checkpoint],
            snapshots: [
                before,
                stable_before,
                after_deferred,
                after_immediate,
                after_checkpoint,
            ],
            deferred,
            immediate,
            directory_removed: !path.try_exists()?,
        })
    }
}

#[tokio::test]
async fn tiny_import_inventory_work_is_independent_of_checkpointed_old_note_bytes()
-> StorageLibraryImportPlanningWorkResult {
    let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
    let observations = match executor
        .submit_waiting(|cancellation| {
            let extra_bytes =
                StorageLibraryImportPlanningWorkFixture::EXTRA_OVERFLOW_PAGES_PER_DOCUMENT
                    * (StorageLibraryImportPlanningWorkFixture::PAGE_BYTES - 4);
            let large_note_bytes = StorageLibraryImportPlanningWorkFixture::SMALL_NOTE_BYTES
                .checked_add(usize::try_from(extra_bytes)?)
                .ok_or("planning fixture note size overflow")?;
            let footprint = StorageLibraryImportPlanningWorkFixture::footprint()?;
            let small = StorageLibraryImportPlanningWorkFixture::new(
                StorageLibraryImportPlanningWorkFixture::SMALL_NOTE_BYTES,
                cancellation,
            )?
            .observe(footprint)?;
            let large =
                StorageLibraryImportPlanningWorkFixture::new(large_note_bytes, cancellation)?
                    .observe(footprint)?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((small, large, extra_bytes))
        })
        .await
    {
        Ok(job) => job.join().await,
        Err(error) => Err(error),
    };
    executor.shutdown().await?;
    let (small, large, extra_bytes) = observations??;
    small.assert_controls();
    large.assert_controls();
    let small_before = &small.snapshots[0];
    let large_before = &large.snapshots[0];
    assert_eq!(small_before.metadata, large_before.metadata);
    assert_eq!(small_before.schema, large_before.schema);
    assert_eq!(small_before.nonoverflow, large_before.nonoverflow);
    assert_eq!(small_before.catalog, large_before.catalog);
    assert_eq!(small_before.total_changes, large_before.total_changes);
    assert_eq!(
        small_before.documents.len(),
        usize::try_from(StorageLibraryImportPlanningWorkFixture::OLD_RECORDS)?
    );
    for (small_document, large_document) in
        small_before.documents.iter().zip(&large_before.documents)
    {
        assert_eq!(small_document.id, large_document.id);
        assert_eq!(small_document.revision, large_document.revision);
        assert_eq!(
            large_document.body_bytes,
            small_document.body_bytes + extra_bytes
        );
        assert_ne!(small_document.body_digest, large_document.body_digest);
    }
    assert_eq!(small_before.document_overflow_pages, 0);
    assert!(
        large_before.document_overflow_pages
            >= StorageLibraryImportPlanningWorkFixture::OLD_RECORDS
                * StorageLibraryImportPlanningWorkFixture::EXTRA_OVERFLOW_PAGES_PER_DOCUMENT
    );
    assert!(large_before.main.bytes > small_before.main.bytes);
    assert_eq!(
        small.deferred.new_page_budget,
        large.deferred.new_page_budget
    );
    assert_eq!(small.deferred.old_tree_pages, large.deferred.old_tree_pages);
    assert_eq!(
        small.deferred.allocator_operations,
        large.deferred.allocator_operations
    );
    eprintln!(
        "tiny import inventory work: small_overflow_pages={}, large_overflow_pages={}, small_deferred={:?}, large_deferred={:?}, small_immediate={:?}, large_immediate={:?}",
        small_before.document_overflow_pages,
        large_before.document_overflow_pages,
        small.deferred,
        large.deferred,
        small.immediate,
        large.immediate,
    );
    assert!(
        large.deferred.inventory_vm_steps <= small.deferred.inventory_vm_steps
            && large.immediate.inventory_vm_steps <= small.immediate.inventory_vm_steps,
        "old annotation overflow increased canonical estimator VM work: deferred {} -> {}, immediate {} -> {}",
        small.deferred.inventory_vm_steps,
        large.deferred.inventory_vm_steps,
        small.immediate.inventory_vm_steps,
        large.immediate.inventory_vm_steps,
    );
    Ok(())
}

#[test]
fn canonical_primary_key_probes_have_covering_query_plans() -> StorageLibraryImportPlanningWorkResult
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("probes.sqlite");
    StorageMigrationRunner::run_embedded(StorageConnection::open(&path)?.diesel())?;
    let connection = Connection::open(&path)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    let limit = StorageLibraryImportSpaceTrees::probe_limit(1)?;
    assert_eq!(limit, 81);
    for (table, index) in [
        (
            StorageLibraryImportSpaceEnumTable::Documents,
            "sqlite_autoindex_documents_1",
        ),
        (
            StorageLibraryImportSpaceEnumTable::Entries,
            "sqlite_autoindex_library_entries_1",
        ),
    ] {
        let plans = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {}", table.probe_sql()))?
            .query_map([i64::try_from(limit)?], |row| row.get::<_, String>(3))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        assert_eq!(plans.len(), 1, "{plans:?}");
        assert!(
            plans[0].contains(&format!("USING COVERING INDEX {index}")),
            "{plans:?}"
        );
        assert!(!plans[0].contains("TEMP B-TREE"), "{plans:?}");
    }
    Ok(())
}

#[tokio::test]
async fn bounded_probes_distinguish_exact_cardinality_from_a_lower_bound()
-> StorageLibraryImportPlanningWorkResult {
    let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
    let job = executor
        .submit_waiting(|token| {
            let limit = StorageLibraryImportSpaceTrees::probe_limit(1)?;
            let mut capped_steps = None;
            for records in [0, limit - 1, limit, 8 * limit] {
                let mut fixture = StorageLibraryImportPlanningWorkFixture::with_records(
                    StorageLibraryImportPlanningWorkFixture::SMALL_NOTE_BYTES,
                    records,
                    token,
                )?;
                let cancellation = StorageImportSessionCancellation::new();
                let steps = StorageLibraryImportPlanningWorkRollback::run(
                    fixture.diesel.diesel(),
                    false,
                    |transaction| {
                        let mut steps = [0; 2];
                        for (index, table) in StorageLibraryImportSpaceEnumTable::ALL
                            .into_iter()
                            .enumerate()
                        {
                            let probe =
                                table.probe(transaction, limit, &mut || cancellation.check())?;
                            assert_eq!(
                                probe.cardinality,
                                if records < limit {
                                    StorageLibraryImportSpaceEnumCardinality::Exact(records)
                                } else {
                                    StorageLibraryImportSpaceEnumCardinality::AtLeast(limit)
                                },
                            );
                            steps[index] = probe.inventory_vm_steps;
                        }
                        Ok(steps)
                    },
                )?;
                if records == limit {
                    capped_steps = Some(steps);
                } else if records > limit {
                    assert_eq!(Some(steps), capped_steps);
                }
                fixture
                    .connection
                    .close()
                    .map_err(|(_connection, error)| error)?;
                fixture.directory.close()?;
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await?;
    let result = job.join().await;
    executor.shutdown().await?;
    result??;
    Ok(())
}

#[tokio::test]
async fn tiny_import_inventory_work_stops_at_the_import_derived_probe_limit()
-> StorageLibraryImportPlanningWorkResult {
    let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
    let job = executor
        .submit_waiting(|token| {
            let footprint = StorageLibraryImportPlanningWorkFixture::footprint()?;
            let limit = StorageLibraryImportSpaceTrees::probe_limit(footprint.count())?;
            let small = StorageLibraryImportPlanningWorkFixture::with_records(
                StorageLibraryImportPlanningWorkFixture::SMALL_NOTE_BYTES,
                limit,
                token,
            )?
            .observe(footprint)?;
            let large = StorageLibraryImportPlanningWorkFixture::with_records(
                StorageLibraryImportPlanningWorkFixture::SMALL_NOTE_BYTES,
                8 * limit,
                token,
            )?
            .observe(footprint)?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((small, large))
        })
        .await?;
    let result = job.join().await;
    executor.shutdown().await?;
    let (small, large) = result??;
    small.assert_controls();
    large.assert_controls();
    assert!(large.snapshots[0].documents.len() > small.snapshots[0].documents.len());
    assert_eq!(small.deferred, large.deferred);
    assert_eq!(small.immediate, large.immediate);
    Ok(())
}

#[test]
fn cancelled_planning_returns_before_attempting_an_unsupported_layout()
-> StorageLibraryImportPlanningWorkResult {
    let mut connection = SqliteConnection::establish(":memory:")?;
    let cancellation = StorageImportSessionCancellation::new();
    cancellation.cancel();
    let outcome =
        StorageLibraryImportPlanningWorkRollback::run(&mut connection, false, |transaction| {
            assert!(matches!(
                StorageLibraryImportSpace::plan(
                    transaction,
                    StorageLibraryImportPlanningWorkFixture::footprint()?,
                    &cancellation,
                ),
                Err(StorageLibraryError::Import(
                    StorageImportSessionError::Cancelled
                )),
            ));
            Ok(())
        });
    outcome?;
    assert!(StorageLibraryImportPageLimit::is_autocommit(
        &mut connection
    )?);
    Ok(())
}

#[tokio::test]
async fn inventory_cancellation_is_observed_between_streamed_rows()
-> StorageLibraryImportPlanningWorkResult {
    let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
    let job = executor
        .submit_waiting(|token| {
            let limit = StorageLibraryImportSpaceTrees::probe_limit(1)?;
            let mut fixture = StorageLibraryImportPlanningWorkFixture::with_records(
                StorageLibraryImportPlanningWorkFixture::SMALL_NOTE_BYTES,
                limit + 1,
                token,
            )?;
            assert_eq!(fixture.checkpoint()?, (0, 0, 0));
            let before = fixture.snapshot()?;
            let cancellation = StorageImportSessionCancellation::new();
            let mut checks = 0_u64;
            StorageLibraryImportPlanningWorkRollback::run(
                fixture.diesel.diesel(),
                true,
                |transaction| {
                    let result = StorageLibraryImportSpaceTrees::plan_budget_checked(
                        transaction,
                        StorageLibraryImportPlanningWorkFixture::footprint()?,
                        StorageLibraryImportPlanningWorkFixture::PAGE_BYTES,
                        &mut || {
                            checks += 1;
                            if checks == 8 {
                                cancellation.cancel();
                            }
                            cancellation.check()
                        },
                    );
                    assert!(matches!(
                        result,
                        Err(StorageLibraryError::Import(
                            StorageImportSessionError::Cancelled
                        ))
                    ));
                    Ok(())
                },
            )?;
            assert_eq!(checks, 8);
            assert!(cancellation.is_cancelled());
            assert_eq!(fixture.snapshot()?, before);
            assert!(StorageLibraryImportPageLimit::is_autocommit(
                fixture.diesel.diesel()
            )?);
            fixture
                .connection
                .close()
                .map_err(|(_connection, error)| error)?;
            fixture.directory.close()?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await?;
    let result = job.join().await;
    executor.shutdown().await?;
    result??;
    Ok(())
}

#[tokio::test]
async fn partial_indexes_reuse_the_parent_probe_without_repeating_inventory()
-> StorageLibraryImportPlanningWorkResult {
    let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
    let job = executor
        .submit_waiting(|token| {
            let mut fixture = StorageLibraryImportPlanningWorkFixture::new(
                StorageLibraryImportPlanningWorkFixture::SMALL_NOTE_BYTES,
                token,
            )?;
            assert_eq!(fixture.checkpoint()?, (0, 0, 0));
            let footprint = StorageLibraryImportPlanningWorkFixture::footprint()?;
            let before = fixture.measure(false, footprint)?;
            fixture.connection.execute_batch(
                "CREATE INDEX import_empty_partial ON library_entries(game_id) WHERE favorite = 1",
            )?;
            assert_eq!(fixture.checkpoint()?, (0, 0, 0));
            let after = fixture.measure(false, footprint)?;
            assert!(after.inventory_vm_steps > 0);
            assert_eq!(after.inventory_vm_steps, before.inventory_vm_steps);
            assert!(after.new_page_budget >= before.new_page_budget);
            assert!(after.old_tree_pages >= before.old_tree_pages);
            fixture
                .connection
                .close()
                .map_err(|(_connection, error)| error)?;
            fixture.directory.close()?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await?;
    let result = job.join().await;
    executor.shutdown().await?;
    result??;
    Ok(())
}

#[tokio::test]
async fn tiny_import_bounds_are_stable_when_a_checkpointed_unrelated_document_grows()
-> StorageLibraryImportPlanningWorkResult {
    let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
    let job = executor.submit_waiting(|token| {
        let mut fixture = StorageLibraryImportPlanningWorkFixture::new(
            StorageLibraryImportPlanningWorkFixture::SMALL_NOTE_BYTES,
            token,
        )?;
        let unrelated = uuid::Uuid::from_u128(99_001).to_string();
        fixture.connection.execute(
            "INSERT INTO documents(id, revision, body) VALUES (?1, '1', '{}')",
            [&unrelated],
        )?;
        assert_eq!(fixture.checkpoint()?, (0, 0, 0));
        let footprint = StorageLibraryImportPlanningWorkFixture::footprint()?;
        let before = fixture.measure(false, footprint)?;
        fixture.connection.execute(
            "UPDATE documents SET body = json_object('unrelated', hex(zeroblob(16384))) WHERE id = ?1",
            [&unrelated],
        )?;
        assert_eq!(fixture.checkpoint()?, (0, 0, 0));
        let after = fixture.measure(false, footprint)?;
        assert_eq!(before.new_page_budget, after.new_page_budget);
        assert_eq!(before.old_tree_pages, after.old_tree_pages);
        assert_eq!(before.allocator_operations, after.allocator_operations);
        fixture.connection.close().map_err(|(_connection, error)| error)?;
        fixture.directory.close()?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }).await?;
    let result = job.join().await;
    executor.shutdown().await?;
    result??;
    Ok(())
}
