use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::game::enums::{GameId, GameIdentityError, GameMoveId};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::storage_repository::{GameStorageWallTimeError, GameStorageWallTimeProvider};
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::import_session::{
    StorageImportSession, StorageImportSessionBodyProof, StorageImportSessionCancellation,
    StorageImportSessionCandidate, StorageImportSessionCandidateBody,
    StorageImportSessionCandidatePage, StorageImportSessionCandidateQuery,
    StorageImportSessionCommitFootprint, StorageImportSessionCommitReceipt,
    StorageImportSessionDestination, StorageImportSessionEnumCommitOutcome,
    StorageImportSessionEnumVolume, StorageImportSessionError,
    StorageImportSessionFilesystemProvider, StorageImportSessionIdentityProvider,
    StorageImportSessionPageRequest, StorageImportSessionPreparedCommit,
    StorageImportSessionRepository, StorageImportSessionRepositoryError,
    StorageImportSessionRepositoryFuture, StorageImportSessionResources,
    StorageImportSessionSnapshot, StorageImportSessionSpace,
};
use crate::storage::library_import::StorageLibraryImportRepository;
use crate::storage::library_repository::StorageLibraryRepository;

type StorageLibraryImportTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[path = "library_import_space_review.rs"]
mod space_review;

struct StorageLibraryImportTestWall {
    reads: AtomicUsize,
}

impl GameStorageWallTimeProvider for StorageLibraryImportTestWall {
    fn now(&self) -> Result<GameLibraryOrganizationTimestamp, GameStorageWallTimeError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(GameLibraryOrganizationTimestamp::new(123_456)?)
    }
}

struct StorageLibraryImportTestIds(AtomicU64);

impl StorageImportSessionIdentityProvider for StorageLibraryImportTestIds {
    fn next_game(&self) -> Result<GameId, GameIdentityError> {
        uuid::Uuid::from_u128((1_u128 << 96) | u128::from(self.0.fetch_add(1, Ordering::SeqCst)))
            .to_string()
            .parse()
    }
    fn next_move(&self) -> Result<GameMoveId, GameIdentityError> {
        uuid::Uuid::from_u128((1_u128 << 96) | u128::from(self.0.fetch_add(1, Ordering::SeqCst)))
            .to_string()
            .parse()
    }
}

struct StorageLibraryImportTestFilesystem {
    created: AtomicUsize,
    free: AtomicU64,
    commit: Mutex<Option<File>>,
    block_read: AtomicBool,
    block_offset: AtomicU64,
    gate: (Mutex<bool>, std::sync::Condvar),
    entered: tokio::sync::Notify,
}

impl StorageLibraryImportTestFilesystem {
    fn new() -> Self {
        Self {
            created: AtomicUsize::new(0),
            free: AtomicU64::new(u64::MAX),
            commit: Mutex::new(None),
            block_read: AtomicBool::new(false),
            block_offset: AtomicU64::new(u64::MAX),
            gate: (Mutex::new(false), std::sync::Condvar::new()),
            entered: tokio::sync::Notify::new(),
        }
    }
    fn release_read(&self) -> StorageLibraryImportTestResult {
        *self
            .gate
            .0
            .lock()
            .map_err(|_poisoned| "poisoned read gate")? = true;
        self.gate.1.notify_all();
        Ok(())
    }
    fn block_late_read(&self) -> StorageLibraryImportTestResult {
        self.mutate(|file| {
            file.seek(SeekFrom::Start(32))?;
            let mut lengths = [0; 16];
            file.read_exact(&mut lengths)?;
            let body =
                u64::from_le_bytes(lengths[..8].try_into().map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                })?);
            let projection =
                u64::from_le_bytes(lengths[8..].try_into().map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                })?);
            self.block_offset
                .store(112 + body + projection, Ordering::SeqCst);
            self.block_read.store(true, Ordering::SeqCst);
            Ok(())
        })
    }
    fn mutate(
        &self,
        mutation: impl FnOnce(&mut File) -> std::io::Result<()>,
    ) -> StorageLibraryImportTestResult {
        let mut commit = self
            .commit
            .lock()
            .map_err(|_poisoned| "poisoned commit fixture")?;
        mutation(commit.as_mut().ok_or("missing commit spool")?)?;
        Ok(())
    }
}

impl StorageImportSessionFilesystemProvider for StorageLibraryImportTestFilesystem {
    fn create_spool(&self) -> Result<File, StorageImportSessionError> {
        let file = tempfile::tempfile()?;
        if self.created.fetch_add(1, Ordering::SeqCst) == 8 {
            *self
                .commit
                .lock()
                .map_err(|_poisoned| StorageImportSessionError::Poisoned)? =
                Some(file.try_clone()?);
        }
        Ok(file)
    }
    fn duplicate(&self, file: &File) -> std::io::Result<File> {
        file.try_clone()
    }
    fn observe_space(
        &self,
        _: StorageImportSessionEnumVolume,
    ) -> Result<StorageImportSessionSpace, StorageImportSessionError> {
        Ok(StorageImportSessionSpace::new(
            1,
            self.free.load(Ordering::SeqCst),
        ))
    }
    fn read_at(&self, file: &mut File, offset: u64, bytes: &mut [u8]) -> std::io::Result<usize> {
        if offset >= self.block_offset.load(Ordering::SeqCst)
            && self.block_read.swap(false, Ordering::SeqCst)
        {
            self.entered.notify_one();
            let mut gate = self
                .gate
                .0
                .lock()
                .map_err(|_poisoned| std::io::ErrorKind::Other)?;
            while !*gate {
                gate = self
                    .gate
                    .1
                    .wait(gate)
                    .map_err(|_poisoned| std::io::ErrorKind::Other)?;
            }
        }
        file.seek(SeekFrom::Start(offset))?;
        file.read(bytes)
    }
    fn write_at(&self, file: &mut File, offset: u64, bytes: &[u8]) -> std::io::Result<()> {
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(bytes)
    }
    fn length(&self, file: &File) -> std::io::Result<u64> {
        Ok(file.metadata()?.len())
    }
    fn flush(&self, file: &mut File) -> std::io::Result<()> {
        file.sync_all()
    }
    fn release(&self, file: &mut File) -> std::io::Result<()> {
        file.set_len(0)
    }
    fn cleanup_retained(&self) -> Result<(), StorageImportSessionError> {
        Ok(())
    }
}

struct StorageLibraryImportTestRepository {
    actual: StorageLibraryImportRepository<StorageLibraryImportTestWall>,
    foreign: Mutex<Option<StorageLibraryImportRepository<StorageLibraryImportTestWall>>>,
    token: Mutex<Option<StorageImportSessionCancellation>>,
    filesystem: Arc<StorageLibraryImportTestFilesystem>,
    refuse_space: AtomicBool,
    pause_receipt: AtomicBool,
    receipt_entered: tokio::sync::Notify,
    receipt_resume: tokio::sync::Notify,
}

impl StorageLibraryImportTestRepository {
    fn cancel(&self) -> StorageLibraryImportTestResult {
        self.token
            .lock()
            .map_err(|_poisoned| "poisoned token")?
            .as_ref()
            .ok_or("commit not admitted")?
            .cancel();
        Ok(())
    }
}

impl StorageImportSessionRepository for StorageLibraryImportTestRepository {
    type PreparedProjection =
        crate::storage::library_prepared::StorageLibraryImportPreparedProjection;
    fn snapshot(&self) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionSnapshot> {
        self.actual.snapshot()
    }
    fn candidates(
        &self,
        query: StorageImportSessionCandidateQuery,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCandidatePage> {
        self.actual.candidates(query)
    }
    fn read_candidate(
        &self,
        candidate: StorageImportSessionCandidate,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCandidateBody> {
        self.actual.read_candidate(candidate)
    }
    fn prepare_projection(
        &self,
        record: &crate::storage::library_record::StorageLibraryRecord,
        body: StorageImportSessionBodyProof<'_>,
        cancellation: &crate::capacity::executor::CapacityExecutorCancellation,
    ) -> Result<Self::PreparedProjection, StorageImportSessionRepositoryError> {
        self.actual.prepare_projection(record, body, cancellation)
    }
    fn estimate_commit_space(
        &self,
        snapshot: StorageImportSessionSnapshot,
        footprint: StorageImportSessionCommitFootprint,
        cancellation: StorageImportSessionCancellation,
    ) -> StorageImportSessionRepositoryFuture<'_, u64> {
        self.actual
            .estimate_commit_space(snapshot, footprint, cancellation)
    }
    fn commit_import(
        &self,
        prepared: StorageImportSessionPreparedCommit,
        cancellation: StorageImportSessionCancellation,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCommitReceipt> {
        Box::pin(async move {
            *self
                .token
                .lock()
                .map_err(|_poisoned| StorageImportSessionRepositoryError::InvalidResponse)? =
                Some(cancellation.clone());
            if self.refuse_space.load(Ordering::SeqCst) {
                self.filesystem.free.store(0, Ordering::SeqCst);
            }
            let foreign = self
                .foreign
                .lock()
                .map_err(|_poisoned| StorageImportSessionRepositoryError::InvalidResponse)?
                .take();
            let receipt = if let Some(foreign) = foreign {
                foreign.commit_import(prepared, cancellation).await?
            } else {
                self.actual.commit_import(prepared, cancellation).await?
            };
            if self.pause_receipt.swap(false, Ordering::SeqCst) {
                self.receipt_entered.notify_one();
                self.receipt_resume.notified().await;
            }
            Ok(receipt)
        })
    }
}

type StorageLibraryImportTestSession = StorageImportSession<
    StorageLibraryImportTestRepository,
    StorageLibraryImportTestFilesystem,
    StorageLibraryImportTestIds,
>;

struct StorageLibraryImportTestFixture {
    directory: tempfile::TempDir,
    budget: CapacityBudget,
    executor: Arc<CapacityExecutor>,
    store: StorageDocumentStore,
    wall: Arc<StorageLibraryImportTestWall>,
    filesystem: Arc<StorageLibraryImportTestFilesystem>,
    port: Arc<StorageLibraryImportTestRepository>,
    next_ids: AtomicU64,
}

impl StorageLibraryImportTestFixture {
    async fn new() -> StorageLibraryImportTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::discover()?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store =
            StorageDocumentStore::open(directory.path().join("import.sqlite"), budget.clone())
                .await?;
        let wall = Arc::new(StorageLibraryImportTestWall {
            reads: AtomicUsize::new(0),
        });
        let filesystem = Arc::new(StorageLibraryImportTestFilesystem::new());
        let port = Arc::new(StorageLibraryImportTestRepository {
            actual: StorageLibraryImportRepository::new(
                store.client(),
                StorageImportSessionDestination::unfiled(),
                Arc::clone(&wall),
            ),
            foreign: Mutex::new(None),
            token: Mutex::new(None),
            filesystem: Arc::clone(&filesystem),
            refuse_space: AtomicBool::new(false),
            pause_receipt: AtomicBool::new(false),
            receipt_entered: tokio::sync::Notify::new(),
            receipt_resume: tokio::sync::Notify::new(),
        });
        Ok(Self {
            directory,
            budget,
            executor,
            store,
            wall,
            filesystem,
            port,
            next_ids: AtomicU64::new(100),
        })
    }
    fn repository(&self) -> StorageLibraryImportRepository<StorageLibraryImportTestWall> {
        StorageLibraryImportRepository::new(
            self.store.client(),
            StorageImportSessionDestination::unfiled(),
            Arc::clone(&self.wall),
        )
    }
    async fn certify(&self) -> StorageLibraryImportTestResult {
        let library = StorageLibraryRepository::new(self.store.client(), &self.executor);
        let page = library
            .adoption_page(
                crate::storage::library_repository::StorageLibraryAdoptionCursor::start(),
            )
            .await?;
        assert!(
            library
                .begin_certify_adoption(page.catalog_revision())?
                .wait()
                .await?
                .is_certified()
        );
        Ok(())
    }
    fn session(
        &self,
        pgn: &str,
    ) -> StorageLibraryImportTestResult<StorageLibraryImportTestSession> {
        let mut source = tempfile::tempfile()?;
        source.write_all(pgn.as_bytes())?;
        Ok(StorageImportSession::new(
            source,
            StorageImportSessionResources {
                executor: Arc::clone(&self.executor),
                repository: Arc::clone(&self.port),
                filesystem: Arc::clone(&self.filesystem),
                identities: Arc::new(StorageLibraryImportTestIds(AtomicU64::new(
                    self.next_ids.fetch_add(1_000_000, Ordering::SeqCst),
                ))),
            },
        )?)
    }
    async fn close(self) -> StorageLibraryImportTestResult {
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|_retained_executor| "import retained the executor")?
            .shutdown()
            .await?;
        drop((self.directory, self.budget));
        Ok(())
    }
}

#[tokio::test]
async fn import_requires_actor_certification_even_with_an_empty_index()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    assert!(
        !fixture
            .repository()
            .snapshot()
            .await?
            .is_adoption_complete()
    );
    let mut session = fixture.session("1. e4 *")?;
    assert!(session.prepare().await.is_err());
    session.cancel_retained().await?;
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn atomic_import_inserts_notation_records_and_bumps_catalog_once()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let before = fixture.repository().snapshot().await?;
    let mut session =
        fixture.session("[White \"Zoë\"]\n\n1. e4 {αβ} *\n\n[White \"İpek\"]\n\n1. d4 *")?;
    assert_eq!(session.prepare().await?.valid(), 2);
    let page = session
        .page(StorageImportSessionPageRequest::new(0, 100, 1_000_000)?)
        .await?;
    let confirmation = session.freeze().await?;
    let outcome = session.commit(confirmation).await?;
    assert!(
        matches!(outcome.as_ref(), StorageImportSessionEnumCommitOutcome::Committed(receipt) if receipt.count() == 2)
    );
    assert_eq!(fixture.wall.reads.load(Ordering::SeqCst), 1);
    let after = fixture.repository().snapshot().await?;
    assert!(after.is_adoption_complete());
    assert_eq!(
        after.catalog_stamp().value(),
        before.catalog_stamp().value() + 1
    );
    let library = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor);
    for row in page.rows() {
        let restored = library
            .load(row.game().ok_or("missing game")?)
            .await?
            .ok_or("missing saved record")?;
        assert_eq!(restored.content_revision().value(), 1);
        assert_eq!(restored.record().mode(), None);
        assert!(!restored.organization().is_favorite());
    }
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn generic_write_revokes_coverage_but_failed_write_preserves_it()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let client = fixture.store.client();
    let id = uuid::Uuid::from_u128(9000);
    assert!(client.save(id, 1, "{}".into()).await.is_err());
    assert!(
        fixture
            .repository()
            .snapshot()
            .await?
            .is_adoption_complete()
    );
    client.save(id, 0, "{}".into()).await?;
    assert!(
        !fixture
            .repository()
            .snapshot()
            .await?
            .is_adoption_complete()
    );
    fixture.certify().await?;
    assert!(
        fixture
            .repository()
            .snapshot()
            .await?
            .is_adoption_complete()
    );
    fixture.close().await
}

#[tokio::test]
async fn raced_certification_refuses_the_old_observed_catalog() -> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    let library = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor);
    let observed = library
        .adoption_page(crate::storage::library_repository::StorageLibraryAdoptionCursor::start())
        .await?
        .catalog_revision();
    fixture
        .store
        .client()
        .save(uuid::Uuid::from_u128(9001), 0, "{}".into())
        .await?;
    assert!(
        !library
            .begin_certify_adoption(observed)?
            .wait()
            .await?
            .is_certified()
    );
    assert!(
        !fixture
            .repository()
            .snapshot()
            .await?
            .is_adoption_complete()
    );
    fixture.close().await
}

#[tokio::test]
async fn catalog_race_refuses_an_approved_import_without_any_insert()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let page = session
        .page(StorageImportSessionPageRequest::new(0, 100, 1_000_000)?)
        .await?;
    let confirmation = session.freeze().await?;
    fixture
        .store
        .client()
        .save(uuid::Uuid::from_u128(9002), 0, "{}".into())
        .await?;
    assert!(session.commit(confirmation).await.is_err());
    for row in page.rows() {
        assert!(
            fixture
                .store
                .client()
                .read(row.game().ok_or("missing id")?.as_uuid())
                .await?
                .is_none()
        );
    }
    session.cancel_retained().await?;
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn late_truncation_rolls_back_earlier_frames() -> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let page = session
        .page(StorageImportSessionPageRequest::new(0, 100, 1_000_000)?)
        .await?;
    let confirmation = session.freeze().await?;
    fixture
        .filesystem
        .mutate(|file| file.set_len(file.metadata()?.len() - 1))?;
    assert!(session.commit(confirmation).await.is_err());
    for row in page.rows() {
        assert!(
            fixture
                .store
                .client()
                .read(row.game().ok_or("missing id")?.as_uuid())
                .await?
                .is_none()
        );
    }
    session.cancel_retained().await?;
    assert_eq!(
        fixture
            .repository()
            .snapshot()
            .await?
            .catalog_stamp()
            .value(),
        0
    );
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn trailing_eof_bytes_roll_back_the_complete_stream() -> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    fixture.filesystem.mutate(|file| {
        file.seek(SeekFrom::End(0))?;
        file.write_all(b"x")
    })?;
    assert!(session.commit(confirmation).await.is_err());
    session.cancel_retained().await?;
    assert_eq!(
        fixture
            .repository()
            .snapshot()
            .await?
            .catalog_stamp()
            .value(),
        0
    );
    drop(session);
    fixture.close().await
}

impl StorageLibraryImportTestFixture {
    async fn sql(&self, sql: &str) -> StorageLibraryImportTestResult {
        let response = self.store.client().begin_library(crate::storage::library_transaction::StorageLibraryEnumTransactionRequest::ImportTestSql(sql.to_owned()))?.wait().await?;
        assert!(matches!(response, crate::storage::library_transaction::StorageLibraryEnumTransactionResponse::ImportTestSql));
        Ok(())
    }
    async fn ids(
        &self,
        session: &mut StorageLibraryImportTestSession,
    ) -> StorageLibraryImportTestResult<Vec<GameId>> {
        let mut ids = Vec::new();
        let mut offset = 0;
        loop {
            let page = session
                .page(StorageImportSessionPageRequest::new(
                    offset, 100, 1_000_000,
                )?)
                .await?;
            ids.extend(page.rows().iter().filter_map(|row| row.game()));
            let Some(next) = page.next_cursor() else {
                break;
            };
            offset = next;
        }
        Ok(ids)
    }
    async fn assert_missing(&self, ids: &[GameId]) -> StorageLibraryImportTestResult {
        for id in ids {
            assert!(self.store.client().read(id.as_uuid()).await?.is_none());
        }
        Ok(())
    }
    async fn committed(&self, pgn: &str) -> StorageLibraryImportTestResult<Vec<GameId>> {
        let mut session = self.session(pgn)?;
        session.prepare().await?;
        let ids = self.ids(&mut session).await?;
        let confirmation = session.freeze().await?;
        session.commit(confirmation).await?;
        Ok(ids)
    }
    async fn candidate_query(
        &self,
        id: GameId,
        cursor: crate::storage::import_session::StorageImportSessionCandidateCursor,
    ) -> StorageLibraryImportTestResult<StorageImportSessionCandidateQuery> {
        let document = self
            .store
            .client()
            .read(id.as_uuid())
            .await?
            .ok_or("missing candidate source")?;
        let record =
            crate::storage::game_document::StorageGameDocument::recover_record(document.body())?;
        Ok(StorageImportSessionCandidateQuery::new(
            self.repository().snapshot().await?.catalog_stamp(),
            Arc::new(record.identity()?),
            cursor,
        )?)
    }
}

#[tokio::test]
async fn final_catalog_failure_rolls_back_every_insert() -> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let ids = fixture.ids(&mut session).await?;
    fixture.sql("CREATE TEMP TRIGGER import_fail_catalog BEFORE UPDATE ON library_state BEGIN SELECT RAISE(ABORT, 'catalog refused'); END").await?;
    let confirmation = session.freeze().await?;
    let error = session
        .commit(confirmation)
        .await
        .err()
        .ok_or("expected final catalog failure")?;
    assert!(error.to_string().contains("catalog refused"), "{error}");
    fixture.assert_missing(&ids).await?;
    assert_eq!(
        fixture
            .repository()
            .snapshot()
            .await?
            .catalog_stamp()
            .value(),
        0
    );
    session.cancel_retained().await?;
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn real_sqlite_full_rolls_back_streamed_inserts() -> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let mut session = fixture.session(&format!(
        "1. e4 {{ {} }} *\n\n1. d4 {{ {} }} *",
        "α".repeat(100_000),
        "β".repeat(100_000)
    ))?;
    session.prepare().await?;
    let ids = fixture.ids(&mut session).await?;
    let confirmation = session.freeze().await?;
    fixture.sql("PRAGMA max_page_count = 1").await?;
    let error = session
        .commit(confirmation)
        .await
        .err()
        .ok_or("expected SQLITE_FULL")?;
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("full"), "{diagnostic}");
    fixture.assert_missing(&ids).await?;
    session.cancel_retained().await?;
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn in_transaction_space_recheck_refuses_after_admission() -> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let ids = fixture.ids(&mut session).await?;
    let confirmation = session.freeze().await?;
    fixture.port.refuse_space.store(true, Ordering::SeqCst);
    let error = session
        .commit(confirmation)
        .await
        .err()
        .ok_or("expected space refusal")?;
    assert!(error.to_string().contains("additional bytes"));
    fixture.assert_missing(&ids).await?;
    session.cancel_retained().await?;
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn puzzle_growth_without_catalog_change_invalidates_the_space_allowance()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let ids = fixture.ids(&mut session).await?;
    let confirmation = session.freeze().await?;
    fixture.sql("INSERT INTO puzzle_collections(collection_id, schema_version, digest, body) VALUES ('space-fixture', 1, zeroblob(32), json_object('padding', hex(zeroblob(1048576))))").await?;
    assert_eq!(
        fixture.repository().snapshot().await?.catalog_stamp(),
        confirmation.catalog_stamp()
    );
    let error = session
        .commit(confirmation)
        .await
        .err()
        .ok_or("expected increased allowance refusal")?;
    assert!(error.to_string().contains("allowance increased"));
    fixture.assert_missing(&ids).await?;
    session.cancel_retained().await?;
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn late_existing_identifier_preserves_its_body_and_rolls_back_earlier_frames()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let ids = fixture.ids(&mut session).await?;
    let last = ids.last().ok_or("missing last game")?;
    let confirmation = session.freeze().await?;
    fixture
        .sql(&format!(
            "INSERT INTO documents(id, revision, body) VALUES ('{}', '9', '{{\"kept\":true}}')",
            last.as_uuid()
        ))
        .await?;
    assert!(session.commit(confirmation).await.is_err());
    fixture.assert_missing(&ids[..ids.len() - 1]).await?;
    let retained = fixture
        .store
        .client()
        .read(last.as_uuid())
        .await?
        .ok_or("lost existing document")?;
    assert_eq!(retained.body(), "{\"kept\":true}");
    assert_eq!(retained.revision(), 9);
    session.cancel_retained().await?;
    drop(retained);
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn last_insert_unique_failure_rolls_back_the_complete_batch() -> StorageLibraryImportTestResult
{
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let ids = fixture.ids(&mut session).await?;
    let last = ids.last().ok_or("missing last game")?;
    fixture.sql(&format!("CREATE TEMP TRIGGER import_duplicate BEFORE INSERT ON documents WHEN NEW.id = '{}' BEGIN INSERT INTO documents(id, revision, body) VALUES (NEW.id, '1', '{{}}'); END", last.as_uuid())).await?;
    let confirmation = session.freeze().await?;
    let error = session
        .commit(confirmation)
        .await
        .err()
        .ok_or("expected late unique failure")?;
    assert!(error.to_string().contains("UNIQUE"), "{error}");
    fixture.assert_missing(&ids).await?;
    assert_eq!(
        fixture
            .repository()
            .snapshot()
            .await?
            .catalog_stamp()
            .value(),
        0
    );
    session.cancel_retained().await?;
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn cancelled_late_read_observes_actor_rollback_and_releases_batch_budget()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    fixture.capture_page_limit().await?;
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let ids = fixture.ids(&mut session).await?;
    let confirmation = session.freeze().await?;
    fixture.filesystem.block_late_read()?;
    {
        let commit = session.commit(confirmation);
        tokio::pin!(commit);
        tokio::select! {
            result = &mut commit => { result?; return Err("commit finished before controlled late read".into()); }
            () = fixture.filesystem.entered.notified() => {}
        }
    }
    assert!(
        fixture
            .store
            .client()
            .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)
            .is_err()
    );
    fixture.port.cancel()?;
    fixture.filesystem.release_read()?;
    assert!(matches!(
        session.cancel_retained().await?.as_ref(),
        crate::storage::import_session::StorageImportSessionEnumCancelOutcome::Cancelled
    ));
    fixture.assert_missing(&ids).await?;
    fixture.assert_page_limit_restored().await?;
    fixture.store.client().inventory(None).await?;
    let reservation = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(reservation);
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn cancellation_after_commit_retains_the_real_receipt_and_does_not_resubmit()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let ids = fixture.ids(&mut session).await?;
    let confirmation = session.freeze().await?;
    fixture.port.pause_receipt.store(true, Ordering::SeqCst);
    {
        let commit = session.commit(confirmation);
        tokio::pin!(commit);
        tokio::select! {
            result = &mut commit => { result?; return Err("receipt was not retained".into()); }
            () = fixture.port.receipt_entered.notified() => {}
        }
    }
    fixture.port.cancel()?;
    fixture.port.receipt_resume.notify_one();
    let outcome = session.cancel_retained().await?;
    assert!(
        matches!(outcome.as_ref(), crate::storage::import_session::StorageImportSessionEnumCancelOutcome::Committed(receipt) if receipt.count() == 2)
    );
    for id in ids {
        assert_eq!(
            fixture
                .store
                .client()
                .read(id.as_uuid())
                .await?
                .ok_or("lost committed game")?
                .revision(),
            1
        );
    }
    assert_eq!(fixture.wall.reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture
            .repository()
            .snapshot()
            .await?
            .catalog_stamp()
            .value(),
        1
    );
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn prepared_projection_from_another_actor_is_refused_atomically()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    let foreign = StorageDocumentStore::open(
        fixture.directory.path().join("foreign.sqlite"),
        fixture.budget.clone(),
    )
    .await?;
    fixture.certify().await?;
    let foreign_library = StorageLibraryRepository::new(foreign.client(), &fixture.executor);
    let observed = foreign_library
        .adoption_page(crate::storage::library_repository::StorageLibraryAdoptionCursor::start())
        .await?;
    assert!(
        foreign_library
            .begin_certify_adoption(observed.catalog_revision())?
            .wait()
            .await?
            .is_certified()
    );
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let ids = fixture.ids(&mut session).await?;
    let confirmation = session.freeze().await?;
    *fixture
        .port
        .foreign
        .lock()
        .map_err(|_poisoned| "poisoned foreign fixture")? =
        Some(StorageLibraryImportRepository::new(
            foreign.client(),
            StorageImportSessionDestination::unfiled(),
            Arc::clone(&fixture.wall),
        ));
    let error = session
        .commit(confirmation)
        .await
        .err()
        .ok_or("expected foreign preparation refusal")?;
    assert!(error.to_string().contains("different storage byte budget"));
    fixture.assert_missing(&ids).await?;
    for id in &ids {
        assert!(foreign.client().read(id.as_uuid()).await?.is_none());
    }
    session.cancel_retained().await?;
    drop(session);
    foreign.close().await?;
    fixture.close().await
}

#[tokio::test]
async fn candidate_clones_keep_the_producer_budget_until_the_final_clone_drops()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let ids = fixture.committed("1. e4 {retained annotations} *").await?;
    let candidate = StorageImportSessionCandidate::new(ids[0], 1);
    let body = fixture.repository().read_candidate(candidate).await?;
    let clone = body.clone();
    drop(body);
    fixture.store.client().inventory(None).await?;
    assert!(
        fixture
            .store
            .client()
            .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)
            .is_err()
    );
    assert!(clone.body().contains("retained annotations"));
    drop(clone);
    fixture.store.client().inventory(None).await?;
    let reservation = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET)?;
    drop(reservation);
    fixture.close().await
}

#[tokio::test]
async fn candidate_read_returns_typed_stale_and_projection_freshness_failures()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let ids = fixture.committed("1. e4 *").await?;
    assert!(matches!(
        fixture
            .repository()
            .read_candidate(StorageImportSessionCandidate::new(ids[0], 2))
            .await,
        Err(StorageImportSessionRepositoryError::StaleCandidate)
    ));
    fixture
        .sql(&format!(
            "UPDATE library_entries SET content_revision = '2' WHERE game_id = '{}'",
            ids[0].as_uuid()
        ))
        .await?;
    assert!(matches!(
        fixture
            .repository()
            .read_candidate(StorageImportSessionCandidate::new(ids[0], 1))
            .await,
        Err(StorageImportSessionRepositoryError::StaleCandidate)
    ));
    fixture.close().await
}

#[tokio::test]
async fn keyset_bucket_is_bounded_and_keeps_trash_duplicates_without_changing_bodies()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let pgn = (0..102)
        .map(|index| format!("[White \"Player{index}\"]\n\n1. e4 {{original {index}}} *\n\n"))
        .collect::<String>();
    let ids = fixture.committed(&pgn).await?;
    assert_eq!(ids.len(), 102);
    fixture
        .sql(&format!(
            "UPDATE library_entries SET location = 'trash', trashed_ms = 123 WHERE game_id = '{}'",
            ids[0].as_uuid()
        ))
        .await?;
    let original = fixture
        .store
        .client()
        .read(ids[0].as_uuid())
        .await?
        .ok_or("missing trash body")?;
    let cursor = crate::storage::import_session::StorageImportSessionCandidateCursor::start();
    let query = fixture.candidate_query(ids[0], cursor).await?;
    let page = fixture.repository().candidates(query.clone()).await?;
    page.validate(&query)?;
    assert_eq!(page.candidates().len(), 100);
    assert_eq!(page.candidates()[0].game(), ids[0]);
    let next = page.next_cursor().ok_or("missing keyset")?;
    assert_eq!(next.boundary(), Some(page.candidates()[99].game()));
    let invalid = StorageImportSessionCandidatePage::new(
        query.catalog_stamp(),
        vec![page.candidates()[0]],
        Some(crate::storage::import_session::StorageImportSessionCandidateCursor::start()),
    )?;
    assert!(invalid.validate(&query).is_err());
    let second = fixture
        .repository()
        .candidates(fixture.candidate_query(ids[0], next).await?)
        .await?;
    assert_eq!(second.candidates().len(), 2);
    assert!(second.next_cursor().is_none());
    assert_eq!(second.candidates()[0].game(), ids[100]);
    let retained = fixture
        .repository()
        .read_candidate(page.candidates()[0])
        .await?;
    assert_eq!(retained.body(), original.body());
    drop((retained, original, page, second));
    fixture.close().await
}

#[tokio::test]
async fn typed_checkpoint_retains_coverage_and_invalidates_an_old_preview()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let ids = fixture.committed("1. e4 *").await?;
    let library = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor);
    let original = fixture
        .store
        .client()
        .read(ids[0].as_uuid())
        .await?
        .ok_or("missing checkpoint source")?;
    let before = fixture.repository().snapshot().await?;
    let mut preview = fixture.session("1. d4 *")?;
    preview.prepare().await?;
    let preview_ids = fixture.ids(&mut preview).await?;
    let confirmation = preview.freeze().await?;
    let prepared = library
        .prepare_write(
            ids[0],
            crate::game::storage_repository::GameStorageRevision::new(1),
            original.body().to_owned(),
            crate::storage::library_prepared::StorageLibraryEnumContentChange::Checkpoint,
        )
        .await?;
    library.begin_save(prepared)?.wait().await?;
    let after = fixture.repository().snapshot().await?;
    assert!(after.is_adoption_complete());
    assert_eq!(
        after.catalog_stamp().value(),
        before.catalog_stamp().value() + 1
    );
    let retained = fixture
        .store
        .client()
        .read(ids[0].as_uuid())
        .await?
        .ok_or("missing checkpoint")?;
    assert_eq!(retained.body(), original.body());
    assert!(preview.commit(confirmation).await.is_err());
    fixture.assert_missing(&preview_ids).await?;
    preview.cancel_retained().await?;
    drop((original, retained, preview));
    fixture.close().await
}

#[tokio::test]
async fn folder_destination_is_exact_and_new_records_have_default_organization()
-> StorageLibraryImportTestResult {
    let mut fixture = StorageLibraryImportTestFixture::new().await?;
    let folder = uuid::Uuid::from_u128(7000);
    fixture.sql(&format!("INSERT INTO library_folders(folder_id, parent_id, revision, name) VALUES ('{folder}', NULL, '7', 'Exact')")).await?;
    let destination = StorageImportSessionDestination::folder(folder, 7)?;
    Arc::get_mut(&mut fixture.port)
        .ok_or("shared test port")?
        .actual
        .destination = destination;
    fixture.certify().await?;
    let ids = fixture.committed("1. e4 *\n\n1. d4 *").await?;
    let library = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor);
    for id in ids {
        let recovered = library.load(id).await?.ok_or("missing folder game")?;
        assert_eq!(
            recovered.organization().folder().map(|id| id.as_uuid()),
            Some(folder)
        );
        assert_eq!(recovered.organization().revision().value(), 0);
        assert!(!recovered.organization().is_favorite());
        assert!(recovered.organization().tags().ids().is_empty());
        assert_eq!(
            recovered.modified().map(|timestamp| timestamp.value()),
            Some(123_456)
        );
    }
    fixture.close().await
}

#[derive(Clone, Copy)]
enum StorageLibraryImportTestEnumFolderMutation {
    Rename,
    Reparent,
    Delete,
}

impl StorageLibraryImportTestEnumFolderMutation {
    fn sql(self, folder: uuid::Uuid, parent: uuid::Uuid) -> String {
        match self {
            Self::Rename => format!(
                "UPDATE library_folders SET name = 'Renamed', revision = '8' WHERE folder_id = '{folder}'"
            ),
            Self::Reparent => format!(
                "UPDATE library_folders SET parent_id = '{parent}', revision = '8' WHERE folder_id = '{folder}'"
            ),
            Self::Delete => format!("DELETE FROM library_folders WHERE folder_id = '{folder}'"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StorageLibraryImportTestEnumCorruption {
    Body,
    Projection,
    BodyAndDigest,
    AggregateOrdinal,
}

#[tokio::test]
async fn folder_rename_reparent_and_delete_refuse_frozen_imports() -> StorageLibraryImportTestResult
{
    for mutation in [
        StorageLibraryImportTestEnumFolderMutation::Rename,
        StorageLibraryImportTestEnumFolderMutation::Reparent,
        StorageLibraryImportTestEnumFolderMutation::Delete,
    ] {
        let mut fixture = StorageLibraryImportTestFixture::new().await?;
        let folder = uuid::Uuid::from_u128(7000);
        let parent = uuid::Uuid::from_u128(7001);
        fixture.sql(&format!("INSERT INTO library_folders(folder_id, parent_id, revision, name) VALUES ('{folder}', NULL, '7', 'Exact'), ('{parent}', NULL, '0', 'Parent')")).await?;
        Arc::get_mut(&mut fixture.port)
            .ok_or("shared test port")?
            .actual
            .destination = StorageImportSessionDestination::folder(folder, 7)?;
        fixture.certify().await?;
        let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
        session.prepare().await?;
        let ids = fixture.ids(&mut session).await?;
        let confirmation = session.freeze().await?;
        fixture.sql(&mutation.sql(folder, parent)).await?;
        let error = session
            .commit(confirmation)
            .await
            .err()
            .ok_or("expected stale destination")?;
        assert!(error.to_string().contains("destination changed"));
        fixture.assert_missing(&ids).await?;
        session.cancel_retained().await?;
        drop(session);
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn corrupt_body_projection_and_aggregate_hash_roll_back_earlier_frames()
-> StorageLibraryImportTestResult {
    for corruption in [
        StorageLibraryImportTestEnumCorruption::Body,
        StorageLibraryImportTestEnumCorruption::Projection,
        StorageLibraryImportTestEnumCorruption::BodyAndDigest,
        StorageLibraryImportTestEnumCorruption::AggregateOrdinal,
    ] {
        let fixture = StorageLibraryImportTestFixture::new().await?;
        fixture.certify().await?;
        let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
        session.prepare().await?;
        let ids = fixture.ids(&mut session).await?;
        let confirmation = session.freeze().await?;
        fixture.filesystem.mutate(|file| {
            file.seek(SeekFrom::Start(32))?;
            let mut lengths = [0; 16];
            file.read_exact(&mut lengths)?;
            let body =
                u64::from_le_bytes(lengths[..8].try_into().map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                })?);
            let projection =
                u64::from_le_bytes(lengths[8..].try_into().map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                })?);
            let second = 112 + body + projection;
            if corruption == StorageLibraryImportTestEnumCorruption::AggregateOrdinal {
                file.seek(SeekFrom::Start(second))?;
                file.write_all(&3_u64.to_le_bytes())?;
            } else {
                file.seek(SeekFrom::Start(second + 32))?;
                file.read_exact(&mut lengths)?;
                let body_length = u64::from_le_bytes(lengths[..8].try_into().map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                })?);
                let offset = second
                    + 112
                    + if corruption == StorageLibraryImportTestEnumCorruption::Projection {
                        body_length
                    } else {
                        0
                    };
                file.seek(SeekFrom::Start(offset))?;
                let mut byte = [0];
                file.read_exact(&mut byte)?;
                byte[0] ^= 1;
                file.seek(SeekFrom::Start(offset))?;
                file.write_all(&byte)?;
                if corruption == StorageLibraryImportTestEnumCorruption::BodyAndDigest {
                    let mut bytes = vec![
                        0;
                        usize::try_from(body_length).map_err(|error| {
                            std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                        })?
                    ];
                    file.seek(SeekFrom::Start(second + 112))?;
                    file.read_exact(&mut bytes)?;
                    let digest = crate::package::digest::PackageDigest::hash_slice(&bytes);
                    file.seek(SeekFrom::Start(second + 48))?;
                    file.write_all(digest.bytes())?;
                }
            }
            Ok(())
        })?;
        assert!(session.commit(confirmation).await.is_err());
        fixture.assert_missing(&ids).await?;
        assert_eq!(
            fixture
                .repository()
                .snapshot()
                .await?
                .catalog_stamp()
                .value(),
            0
        );
        session.cancel_retained().await?;
        drop(session);
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn importing_a_kept_copy_preserves_existing_library_and_trash_annotations()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let pgn = "[White \"Alice\"]\n[Black \"Bob\"]\n[Date \"2026.10.17\"]\n\n1. e4 {original annotation} e5 *";
    let original_ids = fixture
        .committed(&format!(
            "{pgn}\n\n[White \"Other\"]\n\n1. d4 {{trash annotation}} *"
        ))
        .await?;
    fixture
        .sql(&format!(
            "UPDATE library_entries SET location = 'trash', trashed_ms = 123 WHERE game_id = '{}'",
            original_ids[1].as_uuid()
        ))
        .await?;
    let library_before = fixture
        .store
        .client()
        .read(original_ids[0].as_uuid())
        .await?
        .ok_or("missing library original")?;
    let trash_before = fixture
        .store
        .client()
        .read(original_ids[1].as_uuid())
        .await?
        .ok_or("missing trash original")?;
    let mut session = fixture.session(pgn)?;
    assert_eq!(session.prepare().await?.duplicates(), 1);
    session
        .choose(
            1,
            crate::storage::import_session::StorageImportSessionEnumChoice::KeepCopy,
        )
        .await?;
    let confirmation = session.freeze().await?;
    session.commit(confirmation).await?;
    let library_after = fixture
        .store
        .client()
        .read(original_ids[0].as_uuid())
        .await?
        .ok_or("lost library original")?;
    let trash_after = fixture
        .store
        .client()
        .read(original_ids[1].as_uuid())
        .await?
        .ok_or("lost trash original")?;
    assert_eq!(
        library_before.body().as_bytes(),
        library_after.body().as_bytes()
    );
    assert_eq!(
        trash_before.body().as_bytes(),
        trash_after.body().as_bytes()
    );
    assert_eq!(library_before.revision(), library_after.revision());
    assert_eq!(trash_before.revision(), trash_after.revision());
    drop((
        library_before,
        library_after,
        trash_before,
        trash_after,
        session,
    ));
    fixture.close().await
}

#[tokio::test]
async fn reopening_the_database_resets_certification_with_existing_projections()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    fixture.committed("1. e4 *").await?;
    let path = fixture.directory.path().join("import.sqlite");
    fixture.store.close().await?;
    let reopened = StorageDocumentStore::open(path, fixture.budget.clone()).await?;
    let repository = StorageLibraryImportRepository::new(
        reopened.client(),
        StorageImportSessionDestination::unfiled(),
        Arc::clone(&fixture.wall),
    );
    assert!(!repository.snapshot().await?.is_adoption_complete());
    reopened.close().await?;
    drop(fixture.port);
    Arc::try_unwrap(fixture.executor)
        .map_err(|_retained_executor| "retained reopen executor")?
        .shutdown()
        .await?;
    Ok(())
}

#[tokio::test]
async fn populated_indexes_unicode_and_wal_spill_growth_fit_the_confirmed_bound()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    let populated = (0..150)
        .map(|index| format!("[White \"Seed{index}\"]\n[Black \"Référence\"]\n\n1. d4 *\n\n"))
        .collect::<String>();
    fixture.committed(&populated).await?;
    fixture
        .sql("PRAGMA cache_size = 4; PRAGMA cache_spill = ON; PRAGMA wal_autocheckpoint = 0")
        .await?;
    let mut large = format!(
        "[White \"{}\"]\n[Black \"Unicode\"]\n[Opening \"{}\"]\n\n1. e4 {{ {} }} *\n\n[White \"Second\"]\n\n1. c4 *",
        "İΣﬃ".repeat(16_000),
        "Åβ".repeat(16_000),
        "large long line ".repeat(12_000)
    );
    large.push_str("\n\n[White \"LongLine\"]\n\n");
    for cycle in 0..512 {
        large.push_str(&format!(
            "{}. Nf3 Nf6 {}. Ng1 Ng8 ",
            cycle * 2 + 1,
            cycle * 2 + 2
        ));
    }
    large.push('*');
    let mut session = fixture.session(&large)?;
    assert_eq!(session.prepare().await?.valid(), 3);
    let confirmation = session.freeze().await?;
    let size = || -> std::io::Result<u64> {
        let mut bytes = 0_u64;
        for name in ["import.sqlite", "import.sqlite-wal", "import.sqlite-shm"] {
            match std::fs::metadata(fixture.directory.path().join(name)) {
                Ok(metadata) => bytes += metadata.len(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(bytes)
    };
    let before = size()?;
    let allowance = confirmation.sqlite_bytes();
    session.commit(confirmation).await?;
    let growth = size()?.saturating_sub(before);
    assert!(
        growth <= allowance,
        "SQLite growth {growth} exceeds allowance {allowance}"
    );
    fixture.sql("PRAGMA wal_checkpoint(TRUNCATE)").await?;
    assert!(size()?.saturating_sub(before) <= allowance);
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn certification_race_reconciles_before_native_ready() -> StorageLibraryImportTestResult {
    use crate::desktop::native::library_startup::{
        DesktopNativeLibraryStartup, DesktopNativeLibraryStartupEnumReadiness,
    };

    let fixture = StorageLibraryImportTestFixture::new().await?;
    let mut owner = DesktopNativeLibraryStartup::new(
        fixture.store.client(),
        Arc::clone(&fixture.executor),
        tokio::time::Instant::now() + std::time::Duration::from_secs(60),
    );
    assert!(owner.advance().await?);
    fixture
        .store
        .client()
        .save(uuid::Uuid::from_u128(91_000), 0, "{}".to_owned())
        .await?;
    assert!(owner.advance().await?);
    assert_eq!(owner.progress().sweep(), 2);
    assert!(matches!(
        owner.readiness(),
        DesktopNativeLibraryStartupEnumReadiness::Adopting(_)
    ));
    assert!(
        !fixture
            .repository()
            .snapshot()
            .await?
            .is_adoption_complete()
    );
    owner.run().await?;
    assert!(
        matches!(owner.readiness(), DesktopNativeLibraryStartupEnumReadiness::Ready(catalog) if catalog.value() == fixture.repository().snapshot().await?.catalog_stamp().value())
    );
    assert!(
        fixture
            .repository()
            .snapshot()
            .await?
            .is_adoption_complete()
    );
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn observed_recovery_finding_revokes_previously_certified_actor_coverage()
-> StorageLibraryImportTestResult {
    use crate::desktop::native::library_startup::{
        DesktopNativeLibraryStartup, DesktopNativeLibraryStartupEnumIssueCause,
    };

    let fixture = StorageLibraryImportTestFixture::new().await?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut owner = DesktopNativeLibraryStartup::new(
        fixture.store.client(),
        Arc::clone(&fixture.executor),
        deadline,
    );
    owner.run().await?;
    assert!(
        fixture
            .repository()
            .snapshot()
            .await?
            .is_adoption_complete()
    );
    owner.record_recovery_issue(
        uuid::Uuid::from_u128(91_001),
        crate::game::storage_repository::GameStorageRevision::new(1),
        DesktopNativeLibraryStartupEnumIssueCause::Missing,
    )?;
    assert!(owner.has_pending());
    owner.drain_retained(deadline).await?;
    assert!(!owner.has_pending());
    assert!(
        !fixture
            .repository()
            .snapshot()
            .await?
            .is_adoption_complete()
    );
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn temporary_entry_fault_rolls_back_prior_rows_and_the_current_document()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    fixture.capture_page_limit().await?;
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let ids = fixture.ids(&mut session).await?;
    let last = ids.last().ok_or("missing second fault game")?;
    fixture.sql(&format!(
        "CREATE TEMP TRIGGER import_entry_fault BEFORE INSERT ON library_entries WHEN NEW.game_id = '{}' BEGIN SELECT RAISE(ABORT, 'entry statement fault'); END",
        last.as_uuid(),
    )).await?;
    let confirmation = session.freeze().await?;
    let error = session
        .commit(confirmation)
        .await
        .err()
        .ok_or("expected entry statement fault")?;
    assert!(
        error.to_string().contains("entry statement fault"),
        "{error}"
    );
    fixture.assert_missing(&ids).await?;
    fixture.assert_page_limit_restored().await?;
    assert_eq!(
        fixture
            .repository()
            .snapshot()
            .await?
            .catalog_stamp()
            .value(),
        0
    );
    session.cancel_retained().await?;
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn a_cancelled_estimate_retains_actor_usability_and_the_prior_page_limit()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    fixture.capture_page_limit().await?;
    let repository = fixture.repository();
    let snapshot = repository.snapshot().await?;
    let cancellation = StorageImportSessionCancellation::new();
    cancellation.cancel();
    let error = repository
        .estimate_commit_space(
            snapshot,
            StorageImportSessionCommitFootprint {
                count: 1,
                bodies: 1024,
                projections: 1024,
                largest: 2048 + StorageImportSessionPreparedCommit::header_length()?,
            },
            cancellation,
        )
        .await
        .err()
        .ok_or("expected cancelled actor estimate")?;
    assert!(error.to_string().contains("cancelled"), "{error}");
    fixture.assert_page_limit_restored().await?;
    assert_eq!(repository.snapshot().await?, snapshot);
    let ids = fixture.committed("1. e4 *").await?;
    assert_eq!(ids.len(), 1);
    drop(repository);
    fixture.close().await
}
