use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chess::desktop::profile::{
    DesktopProfileAdmission, DesktopProfileEnumAdmission, DesktopProfileId, DesktopProfileLease,
    DesktopProfileOsLock, DesktopProfileSecurity,
};
use chess::package::digest::PackageDigest;
use chess::storage::backup::archive::BackupArchiveFormat;
use chess::storage::backup::create::{BackupArchive, BackupCreator, BackupSpaceNative};
use chess::storage::backup::error::BackupError;
use chess::storage::backup::journal::{BackupRestoreEnumPhase, BackupRestoreJournal};
use chess::storage::backup::manifest::{
    BackupEnumCategory, BackupEnumPlatform, BackupManifest, BackupManifestParts,
};
use chess::storage::backup::recovery::{BackupRecovery, BackupRecoveryEnumOutcome};
use chess::storage::backup::restore::{BackupRestoreRequest, BackupRestorer};
use diesel::prelude::*;
use diesel::sql_types::Text;
use diesel::{QueryableByName, RunQueryDsl, SqliteConnection};
use tempfile::TempDir;
use uuid::Uuid;

use crate::storage_backup::{StorageBackupFixture, StorageBackupTestResult};

#[derive(QueryableByName)]
struct StorageRestoreText {
    #[diesel(sql_type = Text)]
    value: String,
}

pub(crate) struct StorageRestoreFixture {
    pub(crate) directory: TempDir,
    pub(crate) security: DesktopProfileSecurity,
    pub(crate) profile: DesktopProfileId,
    pub(crate) lease: Box<DesktopProfileLease>,
}

impl StorageRestoreFixture {
    pub(crate) fn new() -> StorageBackupTestResult<Self> {
        let directory = TempDir::new()?;
        let security = DesktopProfileSecurity::current()?;
        let profile = DesktopProfileId::from_str("d92f840f-e575-4654-92bd-79b3ba8b8e55")?;
        let admission = DesktopProfileAdmission::new(security.clone(), DesktopProfileOsLock);
        let DesktopProfileEnumAdmission::Owned(lease) =
            admission.acquire(&directory.path().join("profile"), profile)?
        else {
            return Err("expected profile ownership".into());
        };
        Ok(Self {
            directory,
            security,
            profile,
            lease,
        })
    }

    pub(crate) fn data(&self) -> PathBuf {
        self.lease.identity().data_directory().to_path_buf()
    }

    pub(crate) fn database(&self) -> PathBuf {
        self.data().join("library.sqlite3")
    }

    pub(crate) fn backups(&self) -> PathBuf {
        self.data().join("backups")
    }

    pub(crate) fn body(database: &Path) -> StorageBackupTestResult<String> {
        let mut connection = SqliteConnection::establish(&database.to_string_lossy())?;
        Ok(
            diesel::sql_query("SELECT body AS value FROM documents LIMIT 1")
                .get_result::<StorageRestoreText>(&mut connection)?
                .value,
        )
    }

    pub(crate) fn archive(
        &self,
        category: BackupEnumCategory,
        id: u128,
        created_ms: u64,
    ) -> StorageBackupTestResult<BackupArchive> {
        let database = self.database();
        let backups = self.backups();
        let owner = StorageBackupFixture::new()?;
        Ok(BackupCreator::create_blocking(
            owner.request(&database, &backups, category, id, created_ms),
            &BackupSpaceNative,
        )?)
    }

    pub(crate) fn restore(
        &self,
        preview: &chess::storage::backup::preview::BackupPreview,
        safety: &BackupArchive,
        id: u128,
    ) -> Result<chess::storage::backup::restore::BackupRestoreOutcome, BackupError> {
        BackupRestorer::restore_blocking(BackupRestoreRequest {
            locations: &[],
            lease: &*self.lease,
            database: &self.database(),
            preview,
            confirmed_id: preview.id(),
            confirmed_digest: preview.digest(),
            safety,
            restore_id: Uuid::from_u128(id),
        })
    }

    pub(crate) fn admit(&self) -> StorageBackupTestResult<DesktopProfileEnumAdmission> {
        Ok(
            DesktopProfileAdmission::new(self.security.clone(), DesktopProfileOsLock)
                .acquire(&self.data(), self.profile)?,
        )
    }
}

#[tokio::test]
async fn confirmed_restore_replaces_data_after_a_fresh_safety_backup_with_no_acknowledged_loss()
-> StorageBackupTestResult {
    let fixture = StorageRestoreFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, r#"{"v":"original"}"#).await?;
    let archive = fixture.archive(BackupEnumCategory::Manual, 10, 1_000)?;
    StorageBackupFixture::write(&database, 1, r#"{"v":"newer"}"#).await?;
    let preview = StorageBackupFixture::inspect(archive.path())?;
    let safety = fixture.archive(BackupEnumCategory::PreRestore, 11, 2_000)?;
    let outcome = fixture.restore(&preview, &safety, 12)?;
    assert_eq!(outcome.safety_backup(), safety.manifest().id());
    assert_eq!(
        StorageRestoreFixture::body(&database)?,
        r#"{"v":"original"}"#
    );
    let safety_preview = StorageBackupFixture::inspect(safety.path())?;
    let kept = StorageBackupFixture::payload_scalar(
        &safety_preview,
        fixture.directory.path(),
        "SELECT body AS value FROM documents LIMIT 1",
    )?;
    assert_eq!(kept, r#"{"v":"newer"}"#);
    assert!(BackupRestoreJournal::read_blocking(&fixture.data())?.is_none());
    let leftovers = fs::read_dir(fixture.data())?
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.contains(".staged") || name.contains(".pre-restore-")
        })
        .count();
    assert_eq!(leftovers, 0);
    Ok(())
}

#[tokio::test]
async fn restore_requires_the_exact_previewed_archive_and_a_pre_restore_safety_backup()
-> StorageBackupTestResult {
    let fixture = StorageRestoreFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, r#"{"v":"original"}"#).await?;
    let archive = fixture.archive(BackupEnumCategory::Manual, 10, 1_000)?;
    let preview = StorageBackupFixture::inspect(archive.path())?;
    let manual = fixture.archive(BackupEnumCategory::Manual, 11, 2_000)?;
    let before = fs::read(&database)?;
    assert!(matches!(
        fixture.restore(&preview, &manual, 12),
        Err(BackupError::SafetyBackupRequired)
    ));
    let safety = fixture.archive(BackupEnumCategory::PreRestore, 13, 3_000)?;
    let wrong = BackupRestoreRequest {
        locations: &[],
        lease: &*fixture.lease,
        database: &database,
        preview: &preview,
        confirmed_id: Uuid::from_u128(999),
        confirmed_digest: preview.digest(),
        safety: &safety,
        restore_id: Uuid::from_u128(14),
    };
    assert!(matches!(
        BackupRestorer::restore_blocking(wrong),
        Err(BackupError::PreviewMismatch)
    ));
    let other = PackageDigest::hash_slice(b"other");
    let tampered = BackupRestoreRequest {
        locations: &[],
        lease: &*fixture.lease,
        database: &database,
        preview: &preview,
        confirmed_id: preview.id(),
        confirmed_digest: other,
        safety: &safety,
        restore_id: Uuid::from_u128(15),
    };
    assert!(matches!(
        BackupRestorer::restore_blocking(tampered),
        Err(BackupError::PreviewMismatch)
    ));
    assert_eq!(fs::read(&database)?, before);
    assert!(BackupRestoreJournal::read_blocking(&fixture.data())?.is_none());
    Ok(())
}

#[tokio::test]
async fn archive_validation_rejects_corruption_truncation_signature_and_newer_schemas()
-> StorageBackupTestResult {
    let fixture = StorageRestoreFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, "{}").await?;
    let archive = fixture.archive(BackupEnumCategory::Manual, 10, 1_000)?;
    let good = fs::read(archive.path())?;
    let scratch = fixture.directory.path();

    let mut flipped = good.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0xff;
    let path = scratch.join("flipped.chessbackup");
    fs::write(&path, &flipped)?;
    assert!(matches!(
        StorageBackupFixture::inspect(&path),
        Err(chess_checksum) if matches!(chess_checksum.downcast_ref::<BackupError>(), Some(BackupError::ChecksumMismatch))
    ));

    let path = scratch.join("truncated.chessbackup");
    fs::write(&path, &good[..good.len() - 10])?;
    assert!(StorageBackupFixture::inspect(&path).is_err());

    let mut unsigned = good.clone();
    unsigned[0] = b'X';
    let path = scratch.join("unsigned.chessbackup");
    fs::write(&path, &unsigned)?;
    assert!(StorageBackupFixture::inspect(&path).is_err());

    let mut oversized = good.clone();
    oversized[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let path = scratch.join("oversized.chessbackup");
    fs::write(&path, &oversized)?;
    assert!(StorageBackupFixture::inspect(&path).is_err());

    let payload = b"not a database".to_vec();
    let manifest = BackupManifest::new(BackupManifestParts {
        id: Uuid::from_u128(77),
        category: BackupEnumCategory::Manual,
        created_ms: 1,
        platform: BackupEnumPlatform::current(),
        schema_versions: vec!["99990101000000".to_owned()],
        catalog_revision: "0".to_owned(),
        payload_bytes: u64::try_from(payload.len())?,
        payload_sha256: PackageDigest::hash_slice(&payload),
        dependencies: Vec::new(),
        portable_paths: Vec::new(),
    })?;
    let mut future = Vec::new();
    BackupArchiveFormat::write_header(&mut future, &manifest)?;
    future.extend_from_slice(&payload);
    let path = scratch.join("future.chessbackup");
    fs::write(&path, &future)?;
    assert!(matches!(
        StorageBackupFixture::inspect(&path),
        Err(unsupported) if matches!(unsupported.downcast_ref::<BackupError>(), Some(BackupError::UnsupportedSchema { .. }))
    ));
    assert!(StorageBackupFixture::inspect(archive.path()).is_ok());
    Ok(())
}

struct StorageRestoreRecoveryFixture {
    directory: TempDir,
    id: Uuid,
}

impl StorageRestoreRecoveryFixture {
    fn new(
        phase: BackupRestoreEnumPhase,
        live: Option<&str>,
        aside: Option<&str>,
        staged: Option<&str>,
    ) -> StorageBackupTestResult<Self> {
        let directory = TempDir::new()?;
        let id = Uuid::from_u128(5);
        let fixture = Self { directory, id };
        for (path, content) in [
            (fixture.live(), live),
            (fixture.aside(), aside),
            (fixture.staged(), staged),
        ] {
            if let Some(content) = content {
                fs::write(path, content)?;
            }
        }
        BackupRestoreJournal::new(id, Uuid::from_u128(6))
            .with_phase(phase)
            .write_blocking(fixture.directory.path())?;
        Ok(fixture)
    }

    fn live(&self) -> PathBuf {
        self.directory.path().join("library.sqlite3")
    }

    fn aside(&self) -> PathBuf {
        BackupRestoreJournal::aside_path(&self.live(), self.id)
    }

    fn staged(&self) -> PathBuf {
        BackupRestoreJournal::staged_path(self.directory.path(), self.id)
    }

    fn recover(&self) -> Result<BackupRecoveryEnumOutcome, BackupError> {
        BackupRecovery::recover_blocking(self.directory.path(), &self.live())
    }

    fn journal_present(&self) -> StorageBackupTestResult<bool> {
        Ok(BackupRestoreJournal::read_blocking(self.directory.path())?.is_some())
    }
}

#[test]
fn every_interrupted_rename_boundary_completes_or_rolls_back_deterministically()
-> StorageBackupTestResult {
    use BackupRestoreEnumPhase::{Installing, Validated};
    let before_rename =
        StorageRestoreRecoveryFixture::new(Validated, Some("old"), None, Some("new"))?;
    assert_eq!(
        before_rename.recover()?,
        BackupRecoveryEnumOutcome::RolledBack
    );
    assert_eq!(fs::read_to_string(before_rename.live())?, "old");
    assert!(!before_rename.staged().exists());
    assert!(!before_rename.journal_present()?);

    let intent_only =
        StorageRestoreRecoveryFixture::new(Installing, Some("old"), None, Some("new"))?;
    assert_eq!(
        intent_only.recover()?,
        BackupRecoveryEnumOutcome::RolledBack
    );
    assert_eq!(fs::read_to_string(intent_only.live())?, "old");
    assert!(!intent_only.staged().exists());

    let between = StorageRestoreRecoveryFixture::new(Installing, None, Some("old"), Some("new"))?;
    assert_eq!(between.recover()?, BackupRecoveryEnumOutcome::Completed);
    assert_eq!(fs::read_to_string(between.live())?, "new");
    assert!(!between.aside().exists());
    assert!(!between.journal_present()?);

    let installed = StorageRestoreRecoveryFixture::new(Installing, Some("new"), Some("old"), None)?;
    assert_eq!(installed.recover()?, BackupRecoveryEnumOutcome::Completed);
    assert_eq!(fs::read_to_string(installed.live())?, "new");
    assert!(!installed.aside().exists());

    let cleaned = StorageRestoreRecoveryFixture::new(Installing, Some("new"), None, None)?;
    assert_eq!(cleaned.recover()?, BackupRecoveryEnumOutcome::Completed);
    assert!(!cleaned.journal_present()?);

    let lost_stage = StorageRestoreRecoveryFixture::new(Installing, None, Some("old"), None)?;
    assert_eq!(lost_stage.recover()?, BackupRecoveryEnumOutcome::RolledBack);
    assert_eq!(fs::read_to_string(lost_stage.live())?, "old");
    assert!(!lost_stage.journal_present()?);
    Ok(())
}

#[test]
fn contradictory_files_stop_recovery_without_changing_anything() -> StorageBackupTestResult {
    use BackupRestoreEnumPhase::Installing;
    let nothing = StorageRestoreRecoveryFixture::new(Installing, None, None, None)?;
    assert!(matches!(
        nothing.recover(),
        Err(BackupError::JournalInconsistent)
    ));
    assert!(nothing.journal_present()?);
    let everything =
        StorageRestoreRecoveryFixture::new(Installing, Some("a"), Some("b"), Some("c"))?;
    assert!(matches!(
        everything.recover(),
        Err(BackupError::JournalInconsistent)
    ));
    assert_eq!(fs::read_to_string(everything.live())?, "a");
    assert_eq!(fs::read_to_string(everything.aside())?, "b");
    assert_eq!(fs::read_to_string(everything.staged())?, "c");
    let empty = TempDir::new()?;
    assert_eq!(
        BackupRecovery::recover_blocking(empty.path(), &empty.path().join("library.sqlite3"))?,
        BackupRecoveryEnumOutcome::NothingToRecover
    );
    Ok(())
}

#[tokio::test]
async fn a_staged_restore_changes_nothing_until_the_next_launch_installs_it()
-> StorageBackupTestResult {
    let fixture = StorageRestoreFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, r#"{"v":"original"}"#).await?;
    let archive = fixture.archive(BackupEnumCategory::Manual, 10, 1_000)?;
    StorageBackupFixture::write(&database, 1, r#"{"v":"newer"}"#).await?;
    let preview = StorageBackupFixture::inspect(archive.path())?;
    let safety = fixture.archive(BackupEnumCategory::PreRestore, 11, 2_000)?;
    BackupRestorer::stage_for_restart_blocking(BackupRestoreRequest {
        locations: &[],
        lease: &*fixture.lease,
        database: &database,
        preview: &preview,
        confirmed_id: preview.id(),
        confirmed_digest: preview.digest(),
        safety: &safety,
        restore_id: Uuid::from_u128(12),
    })?;
    assert_eq!(StorageRestoreFixture::body(&database)?, r#"{"v":"newer"}"#);
    let journal = BackupRestoreJournal::read_blocking(&fixture.data())?.ok_or("missing journal")?;
    assert_eq!(journal.phase(), BackupRestoreEnumPhase::Pending);
    assert!(matches!(
        fixture.restore(&preview, &safety, 13),
        Err(BackupError::RestoreAlreadyPending)
    ));
    assert_eq!(StorageRestoreFixture::body(&database)?, r#"{"v":"newer"}"#);
    assert_eq!(
        BackupRecovery::recover_blocking(&fixture.data(), &database)?,
        BackupRecoveryEnumOutcome::Completed
    );
    assert_eq!(
        StorageRestoreFixture::body(&database)?,
        r#"{"v":"original"}"#
    );
    assert!(BackupRestoreJournal::read_blocking(&fixture.data())?.is_none());
    assert_eq!(
        BackupRecovery::recover_blocking(&fixture.data(), &database)?,
        BackupRecoveryEnumOutcome::NothingToRecover
    );
    Ok(())
}

#[test]
fn a_pending_restore_whose_staged_copy_vanished_rolls_back_to_the_untouched_database()
-> StorageBackupTestResult {
    let fixture = StorageRestoreRecoveryFixture::new(
        BackupRestoreEnumPhase::Pending,
        Some("old"),
        None,
        None,
    )?;
    assert_eq!(fixture.recover()?, BackupRecoveryEnumOutcome::RolledBack);
    assert_eq!(fs::read_to_string(fixture.live())?, "old");
    assert!(!fixture.journal_present()?);
    Ok(())
}

struct StorageRestoreDatasetFixture;

impl StorageRestoreDatasetFixture {
    fn entry(
        name: &str,
        location: &str,
    ) -> chess::storage::dataset_registry::StorageDatasetRegistryNew {
        use chess::storage::dataset_package::{
            StorageDatasetPackageEnumFormat, StorageDatasetPackageEnumValidation,
        };
        use chess::storage::dataset_registry::{
            StorageDatasetRegistryEnumKind, StorageDatasetRegistryEnumOrigin,
            StorageDatasetRegistryNew,
        };
        StorageDatasetRegistryNew {
            kind: StorageDatasetRegistryEnumKind::Book,
            origin: StorageDatasetRegistryEnumOrigin::Personal,
            name: name.to_owned(),
            version: "1".to_owned(),
            format: StorageDatasetPackageEnumFormat::Polyglot,
            license: "test".to_owned(),
            checksum: PackageDigest::hash_slice(name.as_bytes()),
            location: location.to_owned(),
            validation: StorageDatasetPackageEnumValidation::IntegrityVerified,
            records: Some(1),
        }
    }

    fn register(
        database: &Path,
        entries: &[(&str, &str)],
        lease_first: bool,
    ) -> StorageBackupTestResult {
        use chess::storage::dataset_registry::{
            StorageDatasetRegistry, StorageDatasetRegistryEnumHolder,
        };
        let mut connection = SqliteConnection::establish(&database.to_string_lossy())?;
        for (index, (name, location)) in entries.iter().enumerate() {
            let id =
                StorageDatasetRegistry::register(&mut connection, &Self::entry(name, location))?;
            if lease_first && index == 0 {
                StorageDatasetRegistry::lease(
                    &mut connection,
                    id,
                    StorageDatasetRegistryEnumHolder::Run,
                    "run-1",
                )?;
            }
        }
        Ok(())
    }

    fn scalar(database: &Path, sql: &str) -> StorageBackupTestResult<String> {
        let mut connection = SqliteConnection::establish(&database.to_string_lossy())?;
        Ok(diesel::sql_query(sql)
            .get_result::<StorageRestoreText>(&mut connection)?
            .value)
    }
}

#[tokio::test]
async fn restore_drops_leases_keeps_this_machines_dataset_locations_and_flags_missing_datasets()
-> StorageBackupTestResult {
    use chess::desktop::backup_commands::{
        DesktopBackupCommands, DesktopBackupCommandsPrerequisites,
    };
    use chess::storage::backup::preview::BackupPreviewEnumFinding;
    let fixture = StorageRestoreFixture::new()?;
    let destination = fixture.database();
    StorageBackupFixture::write(&destination, 1, r#"{"v":"destination"}"#).await?;
    StorageRestoreDatasetFixture::register(
        &destination,
        &[("shared-book", "/destination/machine/shared.bin")],
        false,
    )?;

    let source_directory = TempDir::new()?;
    let source = source_directory.path().join("library.sqlite3");
    StorageBackupFixture::write(&source, 1, r#"{"v":"source"}"#).await?;
    StorageRestoreDatasetFixture::register(
        &source,
        &[
            ("shared-book", "/source/machine/shared.bin"),
            ("source-only", "/source/machine/only.bin"),
        ],
        true,
    )?;
    let source_commands = DesktopBackupCommands::new(
        source.clone(),
        source_directory.path().join("backups"),
        BackupSpaceNative,
        Vec::new(),
        Vec::new(),
    );
    let archive =
        source_commands.create_blocking(BackupEnumCategory::Manual, Uuid::from_u128(40), 1_000)?;
    assert_eq!(archive.manifest().dependencies().len(), 2);
    assert!(
        archive
            .manifest()
            .portable_paths()
            .contains(&"/source/machine/only.bin".to_owned())
    );

    let mut commands = DesktopBackupCommands::new(
        destination.clone(),
        fixture.backups(),
        BackupSpaceNative,
        Vec::new(),
        Vec::new(),
    );
    let (handle, findings) = commands.preview_blocking(archive.path())?;
    let referenced = findings
        .iter()
        .filter(|finding| matches!(finding, BackupPreviewEnumFinding::DatasetReferenced(_)))
        .count();
    let unavailable: Vec<_> = findings
        .iter()
        .filter_map(|finding| match finding {
            BackupPreviewEnumFinding::DependencyUnavailable(dependency) => {
                Some(dependency.name.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(referenced, 2);
    assert_eq!(unavailable, vec!["source-only 1".to_owned()]);
    assert!(findings.iter().any(|finding| matches!(
        finding,
        BackupPreviewEnumFinding::DestinationLocationsKept { stored } if stored.len() == 2
    )));

    commands.restore_blocking(
        &fixture.lease,
        handle,
        DesktopBackupCommandsPrerequisites {
            games_paused_and_saved: true,
            editors_have_no_unsaved_edits: true,
        },
        Uuid::from_u128(41),
        Uuid::from_u128(42),
        2_000,
    )?;
    assert_eq!(
        StorageRestoreFixture::body(&destination)?,
        r#"{"v":"source"}"#
    );
    let location = |name: &str| {
        StorageRestoreDatasetFixture::scalar(
            &destination,
            &format!("SELECT location AS value FROM datasets WHERE name = '{name}'"),
        )
    };
    let validation = |name: &str| {
        StorageRestoreDatasetFixture::scalar(
            &destination,
            &format!("SELECT validation AS value FROM datasets WHERE name = '{name}'"),
        )
    };
    assert_eq!(location("shared-book")?, "/destination/machine/shared.bin");
    assert_eq!(validation("shared-book")?, "integrity_verified");
    assert_eq!(location("source-only")?, "/source/machine/only.bin");
    assert_eq!(validation("source-only")?, "unchecked");
    assert_eq!(
        StorageRestoreDatasetFixture::scalar(
            &destination,
            "SELECT CAST(count(*) AS TEXT) AS value FROM dataset_leases"
        )?,
        "0"
    );
    Ok(())
}

struct StorageRestoreGameFixture;

impl StorageRestoreGameFixture {
    async fn save(
        database: &Path,
        session: &chess::game::session::GameSession,
        expected: chess::game::storage_repository::GameStorageRevision,
        change: chess::game::storage_repository::GameStorageEnumContentChange,
    ) -> StorageBackupTestResult<chess::game::storage_repository::GameStorageReceipt> {
        use chess::capacity::budget::CapacityBudget;
        use chess::capacity::executor::CapacityExecutor;
        use chess::clock::duration::ClockInstant;
        use chess::game::storage_repository::{GameStorageRepository, GameStorageSaveRequest};
        use chess::storage::document_store::StorageDocumentStore;
        use chess::storage::library_played::StorageLibraryPlayedRepository;
        let budget = CapacityBudget::new(std::num::NonZeroUsize::new(2).ok_or("empty budget")?)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let store = StorageDocumentStore::open(database.to_path_buf(), budget).await?;
        let repository = StorageLibraryPlayedRepository::new(store.client(), &executor);
        let receipt = repository
            .save(GameStorageSaveRequest::capture_with_change(
                session,
                ClockInstant::from_millis(2_000)?,
                expected,
                change,
            )?)
            .await?;
        drop(repository);
        store.close().await?;
        executor.shutdown().await?;
        Ok(receipt)
    }

    async fn load(
        database: &Path,
        game: chess::game::enums::GameId,
    ) -> StorageBackupTestResult<chess::game::storage_repository::GameStorageRecovered> {
        use chess::capacity::budget::CapacityBudget;
        use chess::capacity::executor::CapacityExecutor;
        use chess::game::storage_repository::GameStorageRepository;
        use chess::storage::document_store::StorageDocumentStore;
        use chess::storage::library_played::StorageLibraryPlayedRepository;
        let budget = CapacityBudget::new(std::num::NonZeroUsize::new(2).ok_or("empty budget")?)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let store = StorageDocumentStore::open(database.to_path_buf(), budget).await?;
        let repository = StorageLibraryPlayedRepository::new(store.client(), &executor);
        let recovered = repository.load(game).await?;
        drop(repository);
        store.close().await?;
        executor.shutdown().await?;
        recovered.ok_or_else(|| "the restored game is missing".into())
    }
}

#[tokio::test]
async fn a_restored_game_reopens_paused_at_the_backed_up_position_clock_and_revision()
-> StorageBackupTestResult {
    use chess::clock::duration::ClockInstant;
    use chess::game::enums::GameEnumPhase;
    use chess::game::library_organization::GameLibraryOrganizationTimestamp;
    use chess::game::storage_repository::{
        GameStorageContentEdit, GameStorageEnumContentChange, GameStorageEnumEditCause,
        GameStorageRevision,
    };
    let fixture = StorageRestoreFixture::new()?;
    let database = fixture.database();
    let session = crate::storage_game_document::StorageGameDocumentFixture::annotated()?;
    let created = GameStorageEnumContentChange::Changed(GameStorageContentEdit::new(
        session.revision(),
        session.revision(),
        GameStorageEnumEditCause::Created,
        GameLibraryOrganizationTimestamp::new(10)?,
    )?);
    let first =
        StorageRestoreGameFixture::save(&database, &session, GameStorageRevision::INITIAL, created)
            .await?;
    let archive = fixture.archive(BackupEnumCategory::Manual, 60, 1_000)?;
    let second = StorageRestoreGameFixture::save(
        &database,
        &session,
        first.storage_revision(),
        GameStorageEnumContentChange::Checkpoint,
    )
    .await?;
    assert!(second.storage_revision().value() > first.storage_revision().value());

    let preview = StorageBackupFixture::inspect(archive.path())?;
    let safety = fixture.archive(BackupEnumCategory::PreRestore, 61, 2_000)?;
    fixture.restore(&preview, &safety, 62)?;

    let recovered = StorageRestoreGameFixture::load(&database, session.id()).await?;
    assert_eq!(recovered.storage_revision(), first.storage_revision());
    let restored = recovered.session();
    assert_eq!(restored.id(), session.id());
    assert_eq!(restored.revision(), session.revision());
    assert_eq!(restored.live(), session.live());
    assert_eq!(restored.tree().len(), session.tree().len());
    assert_eq!(
        restored.tree().node(restored.live())?.position().fen(),
        session.tree().node(session.live())?.position().fen()
    );
    assert_eq!(restored.phase(), GameEnumPhase::Interrupted);
    assert!(!restored.clock().is_running());
    let saved_elapsed = session
        .clock()
        .snapshot(ClockInstant::from_millis(2_000)?)?
        .elapsed();
    assert_eq!(
        restored
            .clock()
            .snapshot(ClockInstant::from_millis(900_000)?)?
            .elapsed(),
        saved_elapsed
    );
    let safety_preview = StorageBackupFixture::inspect(safety.path())?;
    let kept = StorageBackupFixture::payload_scalar(
        &safety_preview,
        fixture.directory.path(),
        "SELECT CAST(count(*) AS TEXT) AS value FROM documents",
    )?;
    assert_eq!(kept, "1");
    Ok(())
}

#[tokio::test]
async fn an_older_schema_backup_is_migrated_in_the_staged_copy_before_it_replaces_anything()
-> StorageBackupTestResult {
    use diesel::connection::SimpleConnection;
    let fixture = StorageRestoreFixture::new()?;
    let destination = fixture.database();
    StorageBackupFixture::write(&destination, 1, r#"{"v":"destination"}"#).await?;

    let old_directory = TempDir::new()?;
    let old = old_directory.path().join("library.sqlite3");
    StorageBackupFixture::write(&old, 1, r#"{"v":"old"}"#).await?;
    let versions = StorageBackupFixture::contexts()?;
    let newest = versions
        .get(5)
        .ok_or("the backup migration is missing")?
        .clone();
    {
        let mut connection = SqliteConnection::establish(&old.to_string_lossy())?;
        connection.batch_execute(&format!(
            "DROP TABLE backup_state; DELETE FROM __diesel_schema_migrations WHERE version = '{newest}';"
        ))?;
    }
    let owner = StorageBackupFixture::new()?;
    let backups = old_directory.path().join("backups");
    let archive = BackupCreator::create_blocking(
        owner.request(&old, &backups, BackupEnumCategory::Manual, 70, 1_000),
        &BackupSpaceNative,
    )?;
    assert!(!archive.manifest().schema_versions().contains(&newest));
    let preview = StorageBackupFixture::inspect(archive.path())?;
    assert!(preview.findings().iter().any(|finding| matches!(
        finding,
        chess::storage::backup::preview::BackupPreviewEnumFinding::OlderSchemaWillMigrate { pending }
            if pending.contains(&newest)
    )));

    let safety = fixture.archive(BackupEnumCategory::PreRestore, 71, 2_000)?;
    fixture.restore(&preview, &safety, 72)?;
    assert_eq!(StorageRestoreFixture::body(&destination)?, r#"{"v":"old"}"#);
    let applied = StorageRestoreDatasetFixture::scalar(
        &destination,
        "SELECT CAST(count(*) AS TEXT) AS value FROM __diesel_schema_migrations",
    )?;
    assert_eq!(applied, versions.len().to_string());
    let state = StorageRestoreDatasetFixture::scalar(
        &destination,
        "SELECT CAST(count(*) AS TEXT) AS value FROM backup_state",
    )?;
    assert_eq!(state, "1");
    Ok(())
}

#[tokio::test]
async fn a_full_backup_carries_every_user_table_and_saved_options_but_no_credential_or_control_state()
-> StorageBackupTestResult {
    use diesel::connection::SimpleConnection;
    const TABLES: &str = "SELECT group_concat(name, ',') AS value FROM (SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name)";
    let fixture = StorageRestoreFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, "{}").await?;
    {
        let mut connection = SqliteConnection::establish(&database.to_string_lossy())?;
        connection.batch_execute(
            "INSERT OR REPLACE INTO preferences (singleton, schema_version, revision, body) VALUES (1, 1, '3', '{\"theme\":\"light\"}');",
        )?;
    }
    let archive = fixture.archive(BackupEnumCategory::Manual, 80, 1_000)?;
    let preview = StorageBackupFixture::inspect(archive.path())?;
    let scratch = fixture.directory.path();

    let live = StorageRestoreDatasetFixture::scalar(&database, TABLES)?;
    let payload = StorageBackupFixture::payload_scalar(&preview, scratch, TABLES)?;
    assert_eq!(payload, live);
    for required in [
        "backup_state",
        "dataset_leases",
        "datasets",
        "editor_drafts",
        "library_entries",
        "preferences",
        "puzzle_runs",
        "report_jobs",
    ] {
        assert!(
            payload.split(',').any(|table| table == required),
            "{required}"
        );
    }
    for forbidden in ["credential", "secret", "token", "owner", "activation"] {
        assert!(!payload.contains(forbidden), "{forbidden}");
    }
    let options = StorageBackupFixture::payload_scalar(
        &preview,
        scratch,
        "SELECT body AS value FROM preferences WHERE singleton = 1",
    )?;
    assert_eq!(options, r#"{"theme":"light"}"#);

    let bytes = fs::read(archive.path())?;
    let control = fixture.lease.identity().control_directory().to_path_buf();
    let mut names = 0;
    for entry in fs::read_dir(control)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        names += 1;
        assert!(
            name.len() < 8
                || !bytes
                    .windows(name.len())
                    .any(|window| window == name.as_bytes()),
            "{name}"
        );
    }
    assert!(names > 0);
    assert!(
        fs::read_dir(fixture.backups())?
            .filter_map(Result::ok)
            .all(
                |entry| entry.path().extension().and_then(|value| value.to_str())
                    == Some("chessbackup")
            )
    );
    Ok(())
}
