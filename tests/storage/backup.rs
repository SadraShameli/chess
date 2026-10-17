use std::fs;
use std::io::Read;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use chess::capacity::budget::CapacityBudget;
use chess::storage::backup::create::{
    BackupCreator, BackupRequest, BackupSpaceNative, BackupSpaceProvider,
};
use chess::storage::backup::error::BackupError;
use chess::storage::backup::manifest::{BackupEnumCategory, BackupEnumPlatform};
use chess::storage::backup::preview::{BackupPreview, BackupPreviewContext};
use chess::storage::backup::retention::BackupRetention;
use chess::storage::backup::scheduler::{BackupEnumDue, BackupScheduler, BackupState};
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::migration::StorageMigrationRunner;
use diesel::prelude::*;
use diesel::sql_types::Text;
use diesel::{QueryableByName, RunQueryDsl, SqliteConnection};
use tempfile::TempDir;
use uuid::Uuid;

pub(crate) type StorageBackupTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(QueryableByName)]
struct StorageBackupText {
    #[diesel(sql_type = Text)]
    value: String,
}

pub(crate) struct StorageBackupFixture {
    pub(crate) directory: TempDir,
}

impl StorageBackupFixture {
    pub(crate) fn new() -> StorageBackupTestResult<Self> {
        Ok(Self {
            directory: TempDir::new()?,
        })
    }

    pub(crate) fn database(&self) -> PathBuf {
        self.directory.path().join("library.sqlite3")
    }

    pub(crate) fn backups(&self) -> PathBuf {
        self.directory.path().join("backups")
    }

    pub(crate) async fn write(database: &Path, id: u128, body: &str) -> StorageBackupTestResult {
        let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("empty budget")?)?;
        let store = StorageDocumentStore::open(database.to_path_buf(), budget).await?;
        let client = store.client();
        let id = Uuid::from_u128(id);
        let revision = client
            .read(id)
            .await?
            .map_or(0, |document| document.revision());
        client.save(id, revision, body.to_owned()).await?;
        store.close().await?;
        Ok(())
    }

    pub(crate) fn contexts() -> StorageBackupTestResult<Vec<String>> {
        Ok(StorageMigrationRunner::embedded_versions()?)
    }

    pub(crate) fn request<'a>(
        &'a self,
        database: &'a Path,
        directory: &'a Path,
        category: BackupEnumCategory,
        id: u128,
        created_ms: u64,
    ) -> BackupRequest<'a> {
        let _ = self;
        BackupRequest {
            database,
            directory,
            category,
            id: Uuid::from_u128(id),
            created_ms,
            dependencies: Vec::new(),
            portable_paths: Vec::new(),
        }
    }

    pub(crate) fn inspect(path: &Path) -> StorageBackupTestResult<BackupPreview> {
        let known = Self::contexts()?;
        Ok(BackupPreview::inspect_blocking(
            path,
            &BackupPreviewContext {
                known_schema_versions: &known,
                destination_platform: BackupEnumPlatform::current(),
                available_dependencies: &[],
            },
        )?)
    }

    pub(crate) fn payload_scalar(
        preview: &BackupPreview,
        scratch: &Path,
        sql: &str,
    ) -> StorageBackupTestResult<String> {
        let target = scratch.join(format!("payload-{}.sqlite3", preview.id()));
        let mut bytes = Vec::new();
        preview.open_payload()?.read_to_end(&mut bytes)?;
        fs::write(&target, bytes)?;
        let mut connection = SqliteConnection::establish(&target.to_string_lossy())?;
        Ok(diesel::sql_query(sql)
            .get_result::<StorageBackupText>(&mut connection)?
            .value)
    }
}

struct StorageBackupLowSpace;

impl BackupSpaceProvider for StorageBackupLowSpace {
    fn available_bytes(&self, _directory: &Path) -> Result<Option<u64>, std::io::Error> {
        Ok(Some(1))
    }
}

#[tokio::test]
async fn snapshot_contains_committed_documents_settings_tables_and_a_consistent_manifest()
-> StorageBackupTestResult {
    let fixture = StorageBackupFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, r#"{"kind":"committed"}"#).await?;
    let backups = fixture.backups();
    let archive = BackupCreator::create_blocking(
        fixture.request(&database, &backups, BackupEnumCategory::Manual, 10, 1_000),
        &BackupSpaceNative,
    )?;
    let preview = StorageBackupFixture::inspect(archive.path())?;
    assert_eq!(preview.manifest().category(), BackupEnumCategory::Manual);
    assert_eq!(
        preview.manifest().schema_versions(),
        StorageBackupFixture::contexts()?.as_slice()
    );
    let scratch = fixture.directory.path();
    let body = StorageBackupFixture::payload_scalar(
        &preview,
        scratch,
        "SELECT body AS value FROM documents LIMIT 1",
    )?;
    assert_eq!(body, r#"{"kind":"committed"}"#);
    let revision = StorageBackupFixture::payload_scalar(
        &preview,
        scratch,
        "SELECT revision AS value FROM library_state WHERE singleton = 1",
    )?;
    assert_eq!(revision, preview.manifest().catalog_revision());
    for table in [
        "preferences",
        "editor_drafts",
        "report_jobs",
        "puzzle_runs",
        "backup_state",
    ] {
        let present = StorageBackupFixture::payload_scalar(
            &preview,
            scratch,
            &format!("SELECT name AS value FROM sqlite_schema WHERE name = '{table}'"),
        )?;
        assert_eq!(present, table);
    }
    assert!(fs::read_dir(&backups)?.all(|entry| entry.is_ok_and(|item| {
        !item.file_name().to_string_lossy().ends_with(".staging")
            && !item.file_name().to_string_lossy().ends_with(".partial")
    })));
    Ok(())
}

#[tokio::test]
async fn a_backup_that_cannot_fit_is_refused_without_leaving_partial_files()
-> StorageBackupTestResult {
    let fixture = StorageBackupFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, "{}").await?;
    let backups = fixture.backups();
    let outcome = BackupCreator::create_blocking(
        fixture.request(&database, &backups, BackupEnumCategory::Manual, 10, 1_000),
        &StorageBackupLowSpace,
    );
    assert!(matches!(
        outcome,
        Err(BackupError::InsufficientSpace { .. })
    ));
    assert_eq!(fs::read_dir(&backups)?.count(), 0);
    Ok(())
}

#[test]
fn first_and_daily_due_times_follow_user_data_and_the_last_successful_automatic_backup() {
    let day = BackupScheduler::INTERVAL_MS;
    let state = BackupState::new(None, None, None);
    assert_eq!(
        BackupScheduler::due(&state, 5 * day),
        BackupEnumDue::NoUserData
    );
    let first = BackupState::new(Some(1_000), None, None);
    assert_eq!(
        BackupScheduler::due(&first, 1_000 + day - 1),
        BackupEnumDue::NotYet {
            due_ms: 1_000 + day
        }
    );
    assert_eq!(
        BackupScheduler::due(&first, 1_000 + day),
        BackupEnumDue::Due { overdue_ms: 0 }
    );
    let later = BackupState::new(Some(1_000), Some(10_000), None);
    assert_eq!(
        BackupScheduler::due(&later, 10_000 + day - 1),
        BackupEnumDue::NotYet {
            due_ms: 10_000 + day
        }
    );
    assert_eq!(
        BackupScheduler::due(&later, 10_000 + 3 * day),
        BackupEnumDue::Due {
            overdue_ms: 2 * day
        }
    );
}

#[test]
fn an_overdue_launch_attempts_once_per_session_and_never_loops() {
    let state = BackupState::new(Some(0), None, None);
    let mut scheduler = BackupScheduler::new();
    let now = BackupScheduler::INTERVAL_MS * 3;
    assert!(scheduler.take_attempt(&state, now));
    assert!(!scheduler.take_attempt(&state, now));
    assert!(!scheduler.take_attempt(&state, now + BackupScheduler::INTERVAL_MS));
}

#[tokio::test]
async fn failure_keeps_a_persistent_warning_until_a_manual_or_automatic_success()
-> StorageBackupTestResult {
    let fixture = StorageBackupFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, "{}").await?;
    let mut connection = SqliteConnection::establish(&database.to_string_lossy())?;
    BackupScheduler::record_user_data_blocking(&mut connection, 500)?;
    BackupScheduler::record_user_data_blocking(&mut connection, 900)?;
    BackupScheduler::record_failure_blocking(&mut connection, "disk full")?;
    let state = BackupScheduler::load_blocking(&mut connection)?;
    assert_eq!(state.first_user_data_ms(), Some(500));
    assert_eq!(state.warning(), Some("disk full"));
    assert_eq!(state.last_automatic_ms(), None);
    BackupScheduler::record_success_blocking(&mut connection, BackupEnumCategory::PreRestore, 1)?;
    assert_eq!(
        BackupScheduler::load_blocking(&mut connection)?.warning(),
        Some("disk full")
    );
    BackupScheduler::record_success_blocking(&mut connection, BackupEnumCategory::Manual, 2)?;
    let after_manual = BackupScheduler::load_blocking(&mut connection)?;
    assert_eq!(after_manual.warning(), None);
    assert_eq!(after_manual.last_automatic_ms(), None);
    BackupScheduler::record_failure_blocking(&mut connection, "again")?;
    BackupScheduler::record_success_blocking(&mut connection, BackupEnumCategory::Automatic, 3)?;
    let after_automatic = BackupScheduler::load_blocking(&mut connection)?;
    assert_eq!(after_automatic.warning(), None);
    assert_eq!(after_automatic.last_automatic_ms(), Some(3));
    Ok(())
}

#[tokio::test]
async fn retention_keeps_thirty_automatic_all_manual_five_per_safety_category_and_protected()
-> StorageBackupTestResult {
    let fixture = StorageBackupFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, "{}").await?;
    let backups = fixture.backups();
    let mut next = 100_u128;
    let mut make =
        |category: BackupEnumCategory, count: u64| -> StorageBackupTestResult<Vec<Uuid>> {
            let mut ids = Vec::new();
            for index in 0..count {
                next += 1;
                let archive = BackupCreator::create_blocking(
                    fixture.request(&database, &backups, category, next, 1_000 + index),
                    &BackupSpaceNative,
                )?;
                ids.push(archive.manifest().id());
            }
            Ok(ids)
        };
    let automatic = make(BackupEnumCategory::Automatic, 33)?;
    let manual = make(BackupEnumCategory::Manual, 35)?;
    let restore = make(BackupEnumCategory::PreRestore, 8)?;
    let update = make(BackupEnumCategory::PreUpdate, 6)?;
    let protected = vec![restore[0]];
    let removed = BackupRetention::apply_blocking(&backups, &protected)?;
    assert_eq!(removed.len(), 3 + 2 + 1);
    let remaining = BackupRetention::scan_blocking(&backups)?;
    let count = |category: BackupEnumCategory| {
        remaining
            .iter()
            .filter(|entry| entry.manifest.category() == category)
            .count()
    };
    assert_eq!(count(BackupEnumCategory::Automatic), 30);
    assert_eq!(count(BackupEnumCategory::Manual), 35);
    assert_eq!(count(BackupEnumCategory::PreRestore), 5 + 1);
    assert_eq!(count(BackupEnumCategory::PreUpdate), 5);
    assert!(
        remaining
            .iter()
            .all(|entry| entry.manifest.id() != automatic[0])
    );
    assert!(
        remaining
            .iter()
            .any(|entry| entry.manifest.id() == manual[0])
    );
    assert!(
        remaining
            .iter()
            .any(|entry| entry.manifest.id() == restore[0])
    );
    assert!(
        remaining
            .iter()
            .all(|entry| entry.manifest.id() != update[0])
    );
    Ok(())
}
