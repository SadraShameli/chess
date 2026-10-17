use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[path = "library_import_populated_review.rs"]
mod populated_review;

use crate::game::enums::GameId;
use crate::storage::import_session::{
    StorageImportSession, StorageImportSessionEnumCommitOutcome, StorageImportSessionEnumVolume,
    StorageImportSessionError, StorageImportSessionFilesystemProvider,
    StorageImportSessionPageRequest, StorageImportSessionRepository, StorageImportSessionResources,
    StorageImportSessionSpace,
};
use crate::storage::library_import::tests::{
    StorageLibraryImportTestFilesystem, StorageLibraryImportTestFixture,
    StorageLibraryImportTestIds, StorageLibraryImportTestRepository,
    StorageLibraryImportTestResult,
};
use crate::storage::library_repository::{
    StorageLibraryAdoptionCursor, StorageLibraryEnumAdoptionPreparation, StorageLibraryRepository,
};

struct StorageLibraryImportSpaceReviewFilesystem {
    actual: Arc<StorageLibraryImportTestFilesystem>,
    files: Mutex<Vec<File>>,
    high_water: AtomicU64,
}

impl StorageLibraryImportSpaceReviewFilesystem {
    fn additional_high_water(&self, source: u64) -> StorageLibraryImportTestResult<u64> {
        Ok(self
            .high_water
            .load(Ordering::SeqCst)
            .checked_add(source)
            .ok_or("space review stage growth overflow")?)
    }

    fn record_lengths(&self) -> std::io::Result<()> {
        let files = self
            .files
            .lock()
            .map_err(|_poisoned| std::io::Error::other("poisoned space review files"))?;
        let bytes = files.iter().try_fold(0_u64, |total, file| {
            total
                .checked_add(file.metadata()?.len())
                .ok_or_else(|| std::io::Error::other("space review length overflow"))
        })?;
        self.high_water.fetch_max(bytes, Ordering::SeqCst);
        Ok(())
    }
}

impl StorageImportSessionFilesystemProvider for StorageLibraryImportSpaceReviewFilesystem {
    fn create_spool(&self) -> Result<File, StorageImportSessionError> {
        let file = self.actual.create_spool()?;
        self.files
            .lock()
            .map_err(|_poisoned| StorageImportSessionError::Poisoned)?
            .push(self.actual.duplicate(&file)?);
        Ok(file)
    }

    fn duplicate(&self, file: &File) -> std::io::Result<File> {
        self.actual.duplicate(file)
    }

    fn observe_space(
        &self,
        volume: StorageImportSessionEnumVolume,
    ) -> Result<StorageImportSessionSpace, StorageImportSessionError> {
        self.actual.observe_space(volume)
    }

    fn read_at(&self, file: &mut File, offset: u64, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.actual.read_at(file, offset, bytes)
    }

    fn write_at(&self, file: &mut File, offset: u64, bytes: &[u8]) -> std::io::Result<()> {
        self.actual.write_at(file, offset, bytes)?;
        self.record_lengths()
    }

    fn length(&self, file: &File) -> std::io::Result<u64> {
        self.actual.length(file)
    }

    fn flush(&self, file: &mut File) -> std::io::Result<()> {
        self.actual.flush(file)
    }

    fn release(&self, file: &mut File) -> std::io::Result<()> {
        self.actual.release(file)?;
        self.record_lengths()
    }

    fn cleanup_retained(&self) -> Result<(), StorageImportSessionError> {
        self.actual.cleanup_retained()?;
        self.files
            .lock()
            .map_err(|_poisoned| StorageImportSessionError::Poisoned)?
            .clear();
        Ok(())
    }
}

type StorageLibraryImportSpaceReviewSession = StorageImportSession<
    StorageLibraryImportTestRepository,
    StorageLibraryImportSpaceReviewFilesystem,
    StorageLibraryImportTestIds,
>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StorageLibraryImportSpaceReviewFiles {
    main: u64,
    wal: u64,
    shm: u64,
}

impl StorageLibraryImportSpaceReviewFiles {
    fn optional_length(path: &Path) -> std::io::Result<u64> {
        match path.metadata() {
            Ok(metadata) => Ok(metadata.len()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(error) => Err(error),
        }
    }

    fn read(directory: &Path) -> std::io::Result<Self> {
        Ok(Self {
            main: directory.join("import.sqlite").metadata()?.len(),
            wal: Self::optional_length(&directory.join("import.sqlite-wal"))?,
            shm: Self::optional_length(&directory.join("import.sqlite-shm"))?,
        })
    }

    fn additional_high_water(
        self,
        committed: Self,
        checkpointed: Self,
    ) -> StorageLibraryImportTestResult<u64> {
        Ok(committed
            .main
            .max(checkpointed.main)
            .checked_sub(self.main)
            .and_then(|main| {
                main.checked_add(committed.wal.max(checkpointed.wal).saturating_sub(self.wal))
            })
            .and_then(|bytes| {
                bytes.checked_add(committed.shm.max(checkpointed.shm).saturating_sub(self.shm))
            })
            .ok_or("space review database growth overflow")?)
    }
}

struct StorageLibraryImportSpaceReview {
    fixture: StorageLibraryImportTestFixture,
    retained: GameId,
    retained_body: String,
}

impl StorageLibraryImportSpaceReview {
    const RETAINED_PGN: &str = "[White \"Retained\"]\n\n1. d4 {preserved α annotation} *";
    const IMPORT_PGN: &str = "[White \"Space review\"]\n\n1. e4 {tiny import} *";
    const BALLAST_BYTES: usize = 2 * 1024 * 1024;
    const TINY_IMPORT_ALLOWANCE_BYTES: u64 = 512 * 1024;
    const BALLAST_ID: uuid::Uuid = uuid::Uuid::from_u128(92_001);

    async fn new() -> StorageLibraryImportTestResult<Self> {
        let fixture = StorageLibraryImportTestFixture::new().await?;
        fixture.certify().await?;
        fixture.sql("PRAGMA wal_autocheckpoint = 0; CREATE TEMP TABLE import_space_review_checks(value INTEGER NOT NULL CHECK(value = 1))").await?;
        let (mut session, _) = Self::session(&fixture, Self::RETAINED_PGN).await?;
        session.prepare().await?;
        let retained = Self::game(&mut session).await?;
        let confirmation = session.freeze().await?;
        assert!(
            matches!(session.commit(confirmation).await?.as_ref(), StorageImportSessionEnumCommitOutcome::Committed(receipt) if receipt.count() == 1)
        );
        drop(session);
        let document = fixture
            .store
            .client()
            .read(retained.as_uuid())
            .await?
            .ok_or("missing retained annotated game")?;
        assert_eq!(document.revision(), 1);
        assert!(document.body().contains("preserved α annotation"));
        let retained_body = document.body().to_owned();
        drop(document);
        let review = Self {
            fixture,
            retained,
            retained_body,
        };
        review.checkpoint().await?;
        Ok(review)
    }

    async fn session(
        fixture: &StorageLibraryImportTestFixture,
        pgn: &str,
    ) -> StorageLibraryImportTestResult<(
        StorageLibraryImportSpaceReviewSession,
        Arc<StorageLibraryImportSpaceReviewFilesystem>,
    )> {
        let bytes = pgn.as_bytes().to_vec();
        let source = fixture
            .executor
            .submit_waiting(move |_| -> std::io::Result<File> {
                let mut file = tempfile::tempfile()?;
                file.write_all(&bytes)?;
                Ok(file)
            })
            .await?
            .join()
            .await??;
        let filesystem = Arc::new(StorageLibraryImportSpaceReviewFilesystem {
            actual: Arc::clone(&fixture.filesystem),
            files: Mutex::new(Vec::new()),
            high_water: AtomicU64::new(0),
        });
        let session = StorageImportSession::new(
            source,
            StorageImportSessionResources {
                executor: Arc::clone(&fixture.executor),
                repository: Arc::clone(&fixture.port),
                filesystem: Arc::clone(&filesystem),
                identities: Arc::new(StorageLibraryImportTestIds(AtomicU64::new(
                    fixture.next_ids.fetch_add(1_000_000, Ordering::SeqCst),
                ))),
            },
        )?;
        Ok((session, filesystem))
    }

    async fn game(
        session: &mut StorageLibraryImportSpaceReviewSession,
    ) -> StorageLibraryImportTestResult<GameId> {
        let page = session
            .page(StorageImportSessionPageRequest::new(0, 1, 1_000_000)?)
            .await?;
        assert_eq!(page.rows().len(), 1);
        Ok(page.rows()[0].game().ok_or("missing space review game")?)
    }

    async fn files(&self) -> StorageLibraryImportTestResult<StorageLibraryImportSpaceReviewFiles> {
        self.fixture.space_files().await
    }

    async fn checkpoint(
        &self,
    ) -> StorageLibraryImportTestResult<StorageLibraryImportSpaceReviewFiles> {
        self.fixture.sql("PRAGMA wal_checkpoint(TRUNCATE)").await?;
        let files = self.files().await?;
        assert!(files.main > 0);
        assert_eq!(files.wal, 0);
        Ok(files)
    }

    async fn add_checkpointed_ballast(&self) -> StorageLibraryImportTestResult {
        let before = self.files().await?;
        let catalog = self.fixture.repository().snapshot().await?.catalog_stamp();
        let body = self
            .fixture
            .executor
            .submit_waiting(|_| Self::ballast_body())
            .await?
            .join()
            .await?;
        self.fixture
            .store
            .client()
            .save(Self::BALLAST_ID, 0, body)
            .await?;
        let saved = self.fixture.repository().snapshot().await?;
        assert!(!saved.is_adoption_complete());
        assert_eq!(saved.catalog_stamp().value(), catalog.value() + 1);
        let library =
            StorageLibraryRepository::new(self.fixture.store.client(), &self.fixture.executor);
        let page = library
            .adoption_page(StorageLibraryAdoptionCursor::start())
            .await?;
        assert_eq!(page.candidates().len(), 2);
        assert!(page.next_cursor().is_none());
        assert!(
            page.candidates()
                .iter()
                .any(|candidate| candidate.id() == self.retained.as_uuid())
        );
        let candidate = page
            .candidates()
            .iter()
            .find(|candidate| candidate.id() == Self::BALLAST_ID)
            .copied()
            .ok_or("missing unrelated ballast adoption candidate")?;
        assert!(matches!(
            library.prepare_adoption(candidate).await?,
            StorageLibraryEnumAdoptionPreparation::Unrelated
        ));
        assert!(
            library
                .begin_certify_adoption(page.catalog_revision())?
                .wait()
                .await?
                .is_certified()
        );
        let refreshed = self.fixture.repository().snapshot().await?;
        assert!(refreshed.is_adoption_complete());
        assert_eq!(refreshed.catalog_stamp(), saved.catalog_stamp());
        self.fixture.sql("INSERT INTO import_space_review_checks SELECT (SELECT count(*) FROM library_entries) = 1 AND (SELECT count(*) FROM documents) = 2").await?;
        let checkpointed = self.checkpoint().await?;
        assert!(checkpointed.main > before.main);
        assert_eq!(self.fixture.repository().snapshot().await?, refreshed);
        self.assert_retained(true).await
    }

    fn ballast_body() -> String {
        format!(
            "{{\"space_review_ballast\":\"{}\"}}",
            "x".repeat(Self::BALLAST_BYTES)
        )
    }

    async fn assert_retained(&self, has_ballast: bool) -> StorageLibraryImportTestResult {
        let document = self
            .fixture
            .store
            .client()
            .read(self.retained.as_uuid())
            .await?
            .ok_or("lost retained annotated game")?;
        assert_eq!(document.revision(), 1);
        assert_eq!(document.body(), self.retained_body);
        let library =
            StorageLibraryRepository::new(self.fixture.store.client(), &self.fixture.executor);
        assert_eq!(
            library
                .load(self.retained)
                .await?
                .ok_or("lost retained library entry")?
                .content_revision()
                .value(),
            1
        );
        if has_ballast {
            let expected = self
                .fixture
                .executor
                .submit_waiting(|_| Self::ballast_body())
                .await?
                .join()
                .await?;
            let ballast = self
                .fixture
                .store
                .client()
                .read(Self::BALLAST_ID)
                .await?
                .ok_or("lost unrelated ballast")?;
            assert_eq!(ballast.revision(), 1);
            assert_eq!(ballast.body(), expected);
        }
        Ok(())
    }

    async fn baseline_allowance(&self) -> StorageLibraryImportTestResult<u64> {
        let (mut session, filesystem) = Self::session(&self.fixture, Self::IMPORT_PGN).await?;
        assert_eq!(session.prepare().await?.valid(), 1);
        let confirmation = session.freeze().await?;
        let allowance = confirmation
            .sqlite_bytes()
            .checked_add(filesystem.additional_high_water(confirmation.source_bytes())?)
            .ok_or("space review allowance overflow")?;
        session.cancel_retained().await?;
        drop(session);
        Ok(allowance)
    }

    async fn control(&self) -> StorageLibraryImportTestResult<u64> {
        let before = self.checkpoint().await?;
        let (mut session, filesystem) = Self::session(&self.fixture, Self::IMPORT_PGN).await?;
        assert_eq!(session.prepare().await?.valid(), 1);
        let game = Self::game(&mut session).await?;
        let confirmation = session.freeze().await?;
        let source = confirmation.source_bytes();
        assert!(
            matches!(session.commit(confirmation).await?.as_ref(), StorageImportSessionEnumCommitOutcome::Committed(receipt) if receipt.count() == 1)
        );
        let committed = self.files().await?;
        assert!(committed.wal > 0);
        let checkpointed = self.checkpoint().await?;
        let stage = filesystem.high_water.load(Ordering::SeqCst);
        let database = before.additional_high_water(committed, checkpointed)?;
        let growth = database
            .checked_add(filesystem.additional_high_water(source)?)
            .ok_or("space review total growth overflow")?;
        assert_eq!(
            self.fixture
                .store
                .client()
                .read(game.as_uuid())
                .await?
                .ok_or("missing control import")?
                .revision(),
            1
        );
        eprintln!(
            "space review control: before={before:?}, committed={committed:?}, checkpointed={checkpointed:?}, database_growth={database}, stage_high_water={stage}, source_bytes={source}, total_additional_high_water={growth}"
        );
        drop(session);
        Ok(growth)
    }
}

impl StorageLibraryImportTestFixture {
    async fn space_files(
        &self,
    ) -> StorageLibraryImportTestResult<StorageLibraryImportSpaceReviewFiles> {
        let directory = self.directory.path().to_owned();
        Ok(self
            .executor
            .submit_waiting(move |_| StorageLibraryImportSpaceReviewFiles::read(&directory))
            .await?
            .join()
            .await??)
    }

    pub(super) async fn capture_page_limit(&self) -> StorageLibraryImportTestResult {
        self.sql("CREATE TEMP TABLE import_saved_limit AS SELECT max_page_count AS value FROM pragma_max_page_count").await
    }

    pub(super) async fn assert_page_limit_restored(&self) -> StorageLibraryImportTestResult {
        self.sql("CREATE TEMP TABLE IF NOT EXISTS import_limit_checks(value INTEGER NOT NULL CHECK(value = 1)); INSERT INTO import_limit_checks SELECT (SELECT value FROM import_saved_limit) = (SELECT max_page_count FROM pragma_max_page_count)").await
    }

    async fn require_scoped_page_limit(&self) -> StorageLibraryImportTestResult {
        self.sql("CREATE TEMP TRIGGER import_require_limit BEFORE INSERT ON documents BEGIN SELECT RAISE(ABORT, 'import page ceiling missing') WHERE (SELECT max_page_count FROM pragma_max_page_count) >= (SELECT value FROM import_saved_limit); END").await
    }
}

#[tokio::test]
async fn one_thousand_records_fit_a_useful_allowance_and_the_actual_file_bound()
-> StorageLibraryImportTestResult {
    let review = StorageLibraryImportSpaceReview::new().await?;
    review.fixture.capture_page_limit().await?;
    review.fixture.require_scoped_page_limit().await?;
    review
        .fixture
        .filesystem
        .free
        .store(256 * 1024 * 1024, Ordering::SeqCst);
    let pgn = (0..1000)
        .map(|index| format!("[White \"Admission {index}\"]\n\n1. e4 *\n\n"))
        .collect::<String>();
    let before = review.checkpoint().await?;
    let (mut session, filesystem) =
        StorageLibraryImportSpaceReview::session(&review.fixture, &pgn).await?;
    assert_eq!(session.prepare().await?.valid(), 1000);
    let confirmation = session.freeze().await?;
    let allowance = confirmation.sqlite_bytes();
    let source = confirmation.source_bytes();
    assert!(
        allowance < 256 * 1024 * 1024,
        "unusable SQLite allowance: {allowance}"
    );
    assert!(
        matches!(session.commit(confirmation).await?.as_ref(), StorageImportSessionEnumCommitOutcome::Committed(receipt) if receipt.count() == 1000)
    );
    let committed = review.files().await?;
    let checkpointed = review.checkpoint().await?;
    let database_growth = before.additional_high_water(committed, checkpointed)?;
    assert!(database_growth <= allowance);
    assert!(
        database_growth
            .checked_add(filesystem.additional_high_water(source)?)
            .ok_or("scale growth overflow")?
            <= 256 * 1024 * 1024
    );
    review.fixture.assert_page_limit_restored().await?;
    review.assert_retained(false).await?;
    drop(session);
    review.fixture.close().await
}

#[tokio::test]
async fn retained_wal_file_high_water_does_not_hide_checkpoint_backlog()
-> StorageLibraryImportTestResult {
    let review = StorageLibraryImportSpaceReview::new().await?;
    review.fixture.sql("INSERT INTO puzzle_collections(collection_id, schema_version, digest, body) VALUES ('high-water', 1, zeroblob(32), json_object('padding', hex(zeroblob(1048576)))); PRAGMA wal_checkpoint(PASSIVE)").await?;
    let before = review.files().await?;
    assert!(before.wal > 1024 * 1024);
    let (mut session, _) = StorageLibraryImportSpaceReview::session(
        &review.fixture,
        StorageLibraryImportSpaceReview::IMPORT_PGN,
    )
    .await?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    let allowance = confirmation.sqlite_bytes();
    assert!(allowance < before.wal);
    session.commit(confirmation).await?;
    let committed = review.files().await?;
    let checkpointed = review.checkpoint().await?;
    assert!(before.additional_high_water(committed, checkpointed)? <= allowance);
    drop(session);
    review.fixture.close().await
}

#[tokio::test]
async fn scoped_page_ceiling_restores_after_observed_sql_rollback() -> StorageLibraryImportTestResult
{
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    fixture.capture_page_limit().await?;
    fixture.require_scoped_page_limit().await?;
    fixture.sql("CREATE TEMP TRIGGER import_scope_abort BEFORE UPDATE ON library_state BEGIN SELECT RAISE(ABORT, 'scope rollback observed'); END").await?;
    let mut session = fixture.session("1. e4 *\n\n1. d4 *")?;
    session.prepare().await?;
    let ids = fixture.ids(&mut session).await?;
    let confirmation = session.freeze().await?;
    let error = session
        .commit(confirmation)
        .await
        .err()
        .ok_or("expected SQL rollback")?;
    assert!(
        error.to_string().contains("scope rollback observed"),
        "{error}"
    );
    fixture.assert_missing(&ids).await?;
    fixture.assert_page_limit_restored().await?;
    session.cancel_retained().await?;
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn a_stricter_existing_page_ceiling_is_preserved_after_sqlite_full()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    fixture.sql("PRAGMA max_page_count = 1").await?;
    fixture.capture_page_limit().await?;
    let mut session = fixture.session(&format!("1. e4 {{ {} }} *", "λ".repeat(100_000)))?;
    session.prepare().await?;
    let ids = fixture.ids(&mut session).await?;
    let confirmation = session.freeze().await?;
    let error = session
        .commit(confirmation)
        .await
        .err()
        .ok_or("expected SQLITE_FULL")?;
    assert!(error.to_string().contains("full"), "{error}");
    fixture.assert_missing(&ids).await?;
    fixture.assert_page_limit_restored().await?;
    session.cancel_retained().await?;
    drop(session);
    fixture.close().await
}

#[test]
fn page_limit_uses_sqlites_returned_ceiling_and_restores_a_stricter_limit()
-> StorageLibraryImportTestResult {
    use crate::storage::library_import::StorageLibraryImportPageLimit;

    let mut connection = rusqlite::Connection::open_in_memory()?;
    connection.execute_batch("CREATE TABLE scope_payload(body BLOB); PRAGMA max_page_count = 8")?;
    let transaction = connection.transaction()?;
    let mut scope = None;
    StorageLibraryImportPageLimit::install(&transaction, 100, &mut scope)?;
    let actual = u64::try_from(
        transaction.pragma_query_value(None, "max_page_count", |row| row.get::<_, i64>(0))?,
    )?;
    assert_eq!(actual, 8);
    transaction.execute("INSERT INTO scope_payload VALUES (zeroblob(8))", [])?;
    transaction.commit()?;
    StorageLibraryImportPageLimit::finish(&connection, scope, Ok(()))?;
    let restored =
        u64::try_from(
            connection.pragma_query_value(None, "max_page_count", |row| row.get::<_, i64>(0))?,
        )?;
    assert_eq!(restored, 8);
    Ok(())
}

#[test]
fn page_limit_refuses_growth_before_sqlite_can_exceed_the_scoped_ceiling()
-> StorageLibraryImportTestResult {
    use crate::storage::library_import::StorageLibraryImportPageLimit;

    let mut connection = rusqlite::Connection::open_in_memory()?;
    connection.execute_batch("CREATE TABLE scope_payload(body BLOB)")?;
    let previous =
        u64::try_from(
            connection.pragma_query_value(None, "max_page_count", |row| row.get::<_, i64>(0))?,
        )?;
    let transaction = connection.transaction()?;
    let mut scope = None;
    StorageLibraryImportPageLimit::install(&transaction, 4, &mut scope)?;
    let error = transaction
        .execute("INSERT INTO scope_payload VALUES (zeroblob(65536))", [])
        .err()
        .ok_or("expected real SQLITE_FULL")?;
    assert!(
        matches!(error, rusqlite::Error::SqliteFailure(ref failure, _) if failure.code == rusqlite::ErrorCode::DiskFull)
    );
    let pages = u64::try_from(
        transaction.pragma_query_value(None, "page_count", |row| row.get::<_, i64>(0))?,
    )?;
    assert!(pages <= 4);
    if !transaction.is_autocommit() {
        transaction.rollback()?;
    } else {
        drop(transaction);
    }
    StorageLibraryImportPageLimit::finish(&connection, scope, Ok(()))?;
    let restored =
        u64::try_from(
            connection.pragma_query_value(None, "max_page_count", |row| row.get::<_, i64>(0))?,
        )?;
    assert_eq!(restored, previous);
    Ok(())
}

#[test]
fn failed_limit_restore_retains_primary_and_rollback_causes_and_retires_connection()
-> StorageLibraryImportTestResult {
    use crate::storage::library_import::StorageLibraryImportPageLimit;
    use crate::storage::library_repository::StorageLibraryError;

    let mut connection = rusqlite::Connection::open_in_memory()?;
    connection.execute_batch("CREATE TABLE scope_payload(body BLOB); PRAGMA max_page_count = 8")?;
    let transaction = connection.transaction()?;
    let mut scope = None;
    StorageLibraryImportPageLimit::install(&transaction, 4, &mut scope)?;
    transaction.rollback()?;
    let rollback = connection
        .execute_batch("ROLLBACK")
        .err()
        .ok_or("expected actual rollback error")?;
    connection.execute_batch(
        "PRAGMA max_page_count = 100; INSERT INTO scope_payload VALUES (zeroblob(65536))",
    )?;
    let primary = StorageLibraryError::Rollback {
        primary: Box::new(StorageLibraryError::ImportProof),
        rollback,
    };
    let error = StorageLibraryImportPageLimit::finish::<()>(&connection, scope, Err(primary))
        .err()
        .ok_or("expected limit restore failure")?;
    assert!(error.requires_connection_retirement());
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("projection proof"), "{diagnostic}");
    assert!(diagnostic.contains("rollback"), "{diagnostic}");
    assert!(diagnostic.contains("ceiling"), "{diagnostic}");
    Ok(())
}

#[test]
fn unresolved_transaction_keeps_the_limit_scope_and_reports_primary_cause()
-> StorageLibraryImportTestResult {
    use crate::storage::library_import::StorageLibraryImportPageLimit;
    use crate::storage::library_repository::StorageLibraryError;

    let mut connection = rusqlite::Connection::open_in_memory()?;
    connection.execute_batch("CREATE TABLE scope_payload(body BLOB)")?;
    let transaction = connection.transaction()?;
    let mut scope = None;
    StorageLibraryImportPageLimit::install(&transaction, 4, &mut scope)?;
    let error = StorageLibraryImportPageLimit::finish::<()>(
        &transaction,
        scope,
        Err(StorageLibraryError::ImportProof),
    )
    .err()
    .ok_or("expected unresolved scope")?;
    assert!(error.requires_connection_retirement());
    assert!(error.to_string().contains("projection proof"));
    let actual = u64::try_from(
        transaction.pragma_query_value(None, "max_page_count", |row| row.get::<_, i64>(0))?,
    )?;
    assert_eq!(actual, 4);
    transaction.rollback()?;
    Ok(())
}

#[test]
fn rejected_page_ceiling_restores_after_sqlite_returns_its_actual_minimum()
-> StorageLibraryImportTestResult {
    use crate::storage::library_import::StorageLibraryImportPageLimit;
    use crate::storage::library_repository::StorageLibraryError;

    let mut connection = rusqlite::Connection::open_in_memory()?;
    connection.execute_batch("CREATE TABLE scope_payload(body BLOB); PRAGMA max_page_count = 8")?;
    let transaction = connection.transaction()?;
    let pages = u64::try_from(
        transaction.pragma_query_value(None, "page_count", |row| row.get::<_, i64>(0))?,
    )?;
    assert!(pages > 1);
    let mut scope = None;
    let error = StorageLibraryImportPageLimit::install(&transaction, 1, &mut scope)
        .err()
        .ok_or("expected SQLite to clamp the ceiling")?;
    assert!(
        matches!(error, StorageLibraryError::ImportPageCeiling { requested: 1, actual } if actual == pages)
    );
    transaction.rollback()?;
    let error = StorageLibraryImportPageLimit::finish::<()>(&connection, scope, Err(error))
        .err()
        .ok_or("lost primary ceiling refusal")?;
    assert!(!error.requires_connection_retirement());
    let restored =
        u64::try_from(
            connection.pragma_query_value(None, "max_page_count", |row| row.get::<_, i64>(0))?,
        )?;
    assert_eq!(restored, 8);
    Ok(())
}

#[tokio::test]
async fn a_small_import_into_a_checkpointed_populated_library_has_a_bounded_allowance()
-> StorageLibraryImportTestResult {
    let review = StorageLibraryImportSpaceReview::new().await?;
    let pgn = (0..1000)
        .map(|index| format!("[White \"Existing {index}\"]\n\n1. d4 *\n\n"))
        .collect::<String>();
    review.fixture.committed(&pgn).await?;
    let before = review.checkpoint().await?;
    review
        .fixture
        .filesystem
        .free
        .store(16 * 1024 * 1024, Ordering::SeqCst);
    let (mut session, _) = StorageLibraryImportSpaceReview::session(
        &review.fixture,
        StorageLibraryImportSpaceReview::IMPORT_PGN,
    )
    .await?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    let allowance = confirmation.sqlite_bytes();
    assert!(
        allowance < 16 * 1024 * 1024,
        "small import requires {allowance}"
    );
    session.commit(confirmation).await?;
    let committed = review.files().await?;
    let checkpointed = review.checkpoint().await?;
    assert!(before.additional_high_water(committed, checkpointed)? <= allowance);
    review.assert_retained(false).await?;
    drop(session);
    review.fixture.close().await
}

#[tokio::test]
async fn cancelled_imports_with_real_wal_spills_fit_the_confirmed_allowance()
-> StorageLibraryImportTestResult {
    let fixture = StorageLibraryImportTestFixture::new().await?;
    fixture.certify().await?;
    fixture.capture_page_limit().await?;
    fixture.sql("PRAGMA cache_size = 4; PRAGMA cache_spill = ON; PRAGMA wal_autocheckpoint = 0; PRAGMA wal_checkpoint(TRUNCATE)").await?;
    let before = fixture.space_files().await?;
    let pgn = format!(
        "[White \"{}\"]\n\n1. e4 {{ {} }} *\n\n1. c4 *",
        "İΣﬃ".repeat(16_000),
        "uncommitted λ ".repeat(20_000)
    );
    let mut session = fixture.session(&pgn)?;
    assert_eq!(session.prepare().await?.valid(), 2);
    let ids = fixture.ids(&mut session).await?;
    let confirmation = session.freeze().await?;
    let allowance = confirmation.sqlite_bytes();
    fixture.filesystem.block_late_read()?;
    {
        let commit = session.commit(confirmation);
        tokio::pin!(commit);
        tokio::select! {
            result = &mut commit => { result?; return Err("commit finished before observing the spill".into()); }
            () = fixture.filesystem.entered.notified() => {}
        }
    }
    fixture.port.cancel()?;
    fixture.filesystem.release_read()?;
    session.cancel_retained().await?;
    let spilled = fixture.space_files().await?;
    fixture.sql("PRAGMA wal_checkpoint(TRUNCATE)").await?;
    let checkpointed = fixture.space_files().await?;
    assert!(
        spilled.wal > 32,
        "the real pager did not spill before commit"
    );
    assert!(before.additional_high_water(spilled, checkpointed)? <= allowance);
    fixture.assert_missing(&ids).await?;
    fixture.assert_page_limit_restored().await?;
    drop(session);
    fixture.close().await
}

#[tokio::test]
async fn checkpointed_unrelated_overflow_does_not_refuse_a_sufficient_import_allowance()
-> StorageLibraryImportTestResult {
    let baseline = StorageLibraryImportSpaceReview::new().await?;
    let baseline_allowance = baseline.baseline_allowance().await?;
    let allowance = StorageLibraryImportSpaceReview::TINY_IMPORT_ALLOWANCE_BYTES;
    baseline
        .fixture
        .filesystem
        .free
        .store(allowance, Ordering::SeqCst);
    let baseline_growth = baseline.control().await?;
    assert!(baseline_growth <= allowance);
    baseline.assert_retained(false).await?;
    baseline.fixture.close().await?;
    assert!(baseline_allowance <= allowance);

    let control = StorageLibraryImportSpaceReview::new().await?;
    control.add_checkpointed_ballast().await?;
    let control_files = control.files().await?;
    let ballast_growth = control.control().await?;
    assert!(ballast_growth <= allowance);
    control.assert_retained(true).await?;
    control.fixture.close().await?;

    let review = StorageLibraryImportSpaceReview::new().await?;
    review.add_checkpointed_ballast().await?;
    assert_eq!(review.files().await?, control_files);
    review
        .fixture
        .filesystem
        .free
        .store(allowance, Ordering::SeqCst);
    let (mut session, filesystem) = StorageLibraryImportSpaceReview::session(
        &review.fixture,
        StorageLibraryImportSpaceReview::IMPORT_PGN,
    )
    .await?;
    assert_eq!(session.prepare().await?.valid(), 1);
    let game = StorageLibraryImportSpaceReview::game(&mut session).await?;
    let admission = match session.freeze().await {
        Ok(confirmation) => session.commit(confirmation).await,
        Err(error) => Err(error),
    };
    if let Ok(outcome) = &admission {
        assert!(
            matches!(outcome.as_ref(), StorageImportSessionEnumCommitOutcome::Committed(receipt) if receipt.count() == 1)
        );
        assert_eq!(
            review
                .fixture
                .store
                .client()
                .read(game.as_uuid())
                .await?
                .ok_or("missing admitted import")?
                .revision(),
            1
        );
    } else {
        review.fixture.assert_missing(&[game]).await?;
        session.cancel_retained().await?;
    }
    review
        .fixture
        .executor
        .submit_waiting(move |_| {
            assert!(
                filesystem
                    .files
                    .lock()
                    .map_err(|_poisoned| std::io::Error::other("poisoned space review files"))?
                    .is_empty()
            );
            Ok::<_, std::io::Error>(())
        })
        .await?
        .join()
        .await??;
    review.assert_retained(true).await?;
    drop(session);
    review.fixture.close().await?;
    if let Err(error) = &admission {
        assert!(
            matches!(error.as_ref(), StorageImportSessionError::Space { required, available } if required > available && *available == allowance),
            "unexpected admission failure: {error}"
        );
    }
    assert!(
        admission.is_ok(),
        "checkpointed unrelated ballast refused allowance={allowance} despite measured baseline_growth={baseline_growth}, ballast_growth={ballast_growth}: {admission:?}"
    );
    Ok(())
}
