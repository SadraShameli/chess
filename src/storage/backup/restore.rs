use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use diesel::connection::SimpleConnection;
use diesel::sql_types::Text;
use diesel::{QueryableByName, RunQueryDsl, SqliteConnection};
use uuid::Uuid;

use crate::package::digest::PackageDigest;
use crate::storage::backup::create::BackupArchive;
use crate::storage::backup::error::BackupError;
use crate::storage::backup::journal::{BackupRestoreEnumPhase, BackupRestoreJournal};
use crate::storage::backup::manifest::BackupEnumCategory;
use crate::storage::backup::preview::BackupPreview;
use crate::storage::migration::StorageMigrationRunner;

#[derive(QueryableByName)]
struct BackupRestoreIntegrity {
    #[diesel(sql_type = Text)]
    integrity_check: String,
}

pub trait BackupLeaseProof {
    fn data_directory(&self) -> &Path;
    fn verify(&self) -> Result<(), BackupError>;
}

pub struct BackupRestoreRequest<'a> {
    pub lease: &'a dyn BackupLeaseProof,
    pub database: &'a Path,
    pub preview: &'a BackupPreview,
    pub confirmed_id: Uuid,
    pub confirmed_digest: PackageDigest,
    pub safety: &'a BackupArchive,
    pub restore_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupRestoreOutcome {
    restore_id: Uuid,
    safety_backup: Uuid,
}

impl BackupRestoreOutcome {
    pub const fn restore_id(self) -> Uuid {
        self.restore_id
    }

    pub const fn safety_backup(self) -> Uuid {
        self.safety_backup
    }
}

pub struct BackupRestorer;

impl BackupRestorer {
    pub fn restore_blocking(
        request: BackupRestoreRequest<'_>,
    ) -> Result<BackupRestoreOutcome, BackupError> {
        let data = request.lease.data_directory().to_path_buf();
        request.lease.verify()?;
        Self::require_inside(&data, request.database)?;
        Self::validate_confirmation(&request)?;
        Self::require_quiet_database(request.database)?;
        let staged = BackupRestoreJournal::staged_path(&data, request.restore_id);
        let prepared = Self::stage(&request, &staged);
        if let Err(error) = prepared {
            Self::discard(&staged)?;
            return Err(error);
        }
        let journal = BackupRestoreJournal::new(request.restore_id, request.safety.manifest().id());
        journal.write_blocking(&data)?;
        let installing = journal.with_phase(BackupRestoreEnumPhase::Installing);
        installing.write_blocking(&data)?;
        let aside = BackupRestoreJournal::aside_path(request.database, request.restore_id);
        if request.database.exists() {
            fs::rename(request.database, &aside)?;
        }
        fs::rename(&staged, request.database)?;
        BackupRestoreJournal::sync_directory(&data)?;
        if aside.exists() {
            fs::remove_file(&aside)?;
        }
        BackupRestoreJournal::remove_blocking(&data)?;
        Ok(BackupRestoreOutcome {
            restore_id: request.restore_id,
            safety_backup: request.safety.manifest().id(),
        })
    }

    fn require_inside(data: &Path, database: &Path) -> Result<(), BackupError> {
        let parent = database
            .parent()
            .ok_or_else(|| BackupError::Lease(database.to_path_buf()))?;
        if parent.canonicalize()? != data.canonicalize()? {
            return Err(BackupError::Lease(database.to_path_buf()));
        }
        Ok(())
    }

    fn validate_confirmation(request: &BackupRestoreRequest<'_>) -> Result<(), BackupError> {
        if request.confirmed_id != request.preview.id()
            || request.confirmed_digest != request.preview.digest()
        {
            return Err(BackupError::PreviewMismatch);
        }
        let safety = request.safety.manifest();
        if safety.category() != BackupEnumCategory::PreRestore
            || !request.safety.path().is_file()
            || safety.id() == request.preview.id()
        {
            return Err(BackupError::SafetyBackupRequired);
        }
        Ok(())
    }

    fn require_quiet_database(database: &Path) -> Result<(), BackupError> {
        for suffix in ["-wal", "-shm"] {
            let companion = PathBuf::from(format!("{}{suffix}", database.display()));
            if let Ok(metadata) = fs::metadata(&companion)
                && metadata.len() > 0
            {
                return Err(BackupError::Integrity(
                    "the live database still has an uncheckpointed write-ahead log".to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn stage(request: &BackupRestoreRequest<'_>, staged: &Path) -> Result<(), BackupError> {
        let mut source = request.preview.open_payload()?;
        let mut target = File::create(staged)?;
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            target.write_all(&buffer[..count])?;
        }
        target.sync_all()?;
        drop(target);
        if PackageDigest::read(File::open(staged)?)? != request.preview.digest() {
            return Err(BackupError::ChecksumMismatch);
        }
        Self::migrate_and_verify(staged)?;
        File::open(staged)?.sync_all()?;
        Ok(())
    }

    fn migrate_and_verify(staged: &Path) -> Result<(), BackupError> {
        use diesel::Connection;
        let mut connection = SqliteConnection::establish(&staged.to_string_lossy())?;
        connection.batch_execute("PRAGMA foreign_keys = ON; PRAGMA journal_mode = DELETE;")?;
        StorageMigrationRunner::run_embedded(&mut connection)?;
        let rows = diesel::sql_query("PRAGMA integrity_check")
            .load::<BackupRestoreIntegrity>(&mut connection)?;
        let verdict = rows
            .iter()
            .map(|row| row.integrity_check.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        if verdict != "ok" {
            return Err(BackupError::Integrity(verdict));
        }
        connection.batch_execute("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    fn discard(staged: &Path) -> Result<(), BackupError> {
        match fs::remove_file(staged) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
