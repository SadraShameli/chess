use std::collections::HashMap;
use std::path::{Path, PathBuf};

use thiserror::Error;
use uuid::Uuid;

use crate::desktop::profile::DesktopProfileLease;
use crate::package::digest::PackageDigest;
use crate::storage::backup::create::{
    BackupArchive, BackupCreator, BackupRequest, BackupSpaceProvider,
};
use crate::storage::backup::error::BackupError;
use crate::storage::backup::journal::BackupRestoreJournal;
use crate::storage::backup::manifest::{
    BackupDependency, BackupEnumCategory, BackupEnumPlatform, BackupManifest,
};
use crate::storage::backup::preview::{BackupPreview, BackupPreviewContext};
use crate::storage::backup::restore::{
    BackupLeaseProof, BackupRestoreOutcome, BackupRestoreRequest, BackupRestorer,
};
use crate::storage::backup::retention::BackupRetention;
use crate::storage::backup::scheduler::{BackupScheduler, BackupState};
use crate::storage::connection::StorageConnection;
use crate::storage::migration::StorageMigrationRunner;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopBackupCommandsEnumPrerequisite {
    GamesPausedAndSaved,
    EditorsHaveNoUnsavedEdits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopBackupCommandsPrerequisites {
    pub games_paused_and_saved: bool,
    pub editors_have_no_unsaved_edits: bool,
}

impl DesktopBackupCommandsPrerequisites {
    pub const fn first_unmet(self) -> Option<DesktopBackupCommandsEnumPrerequisite> {
        if !self.games_paused_and_saved {
            Some(DesktopBackupCommandsEnumPrerequisite::GamesPausedAndSaved)
        } else if !self.editors_have_no_unsaved_edits {
            Some(DesktopBackupCommandsEnumPrerequisite::EditorsHaveNoUnsavedEdits)
        } else {
            None
        }
    }
}

#[derive(Debug, Error)]
pub enum DesktopBackupCommandsError {
    #[error("Restore is blocked until {0:?} is satisfied")]
    Prerequisite(DesktopBackupCommandsEnumPrerequisite),
    #[error("Preview {0} is unknown or was already used")]
    UnknownPreview(Uuid),
    #[error(transparent)]
    Backup(#[from] BackupError),
}

impl BackupLeaseProof for DesktopProfileLease {
    fn data_directory(&self) -> &Path {
        self.identity().data_directory()
    }

    fn verify(&self) -> Result<(), BackupError> {
        self.verify_identity()
            .map_err(|_error| BackupError::Lease(self.identity().data_directory().to_path_buf()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopBackupCommandsEntry {
    pub path: PathBuf,
    pub manifest: BackupManifest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopBackupCommandsPreviewHandle {
    id: Uuid,
    digest: PackageDigest,
}

impl DesktopBackupCommandsPreviewHandle {
    pub const fn id(self) -> Uuid {
        self.id
    }

    pub const fn digest(self) -> PackageDigest {
        self.digest
    }
}

pub struct DesktopBackupCommands<S> {
    database: PathBuf,
    directory: PathBuf,
    space: S,
    available_dependencies: Vec<BackupDependency>,
    portable_paths: Vec<String>,
    previews: HashMap<Uuid, BackupPreview>,
}

impl<S: BackupSpaceProvider> DesktopBackupCommands<S> {
    pub fn new(
        database: PathBuf,
        directory: PathBuf,
        space: S,
        available_dependencies: Vec<BackupDependency>,
        portable_paths: Vec<String>,
    ) -> Self {
        Self {
            database,
            directory,
            space,
            available_dependencies,
            portable_paths,
            previews: HashMap::new(),
        }
    }

    pub fn create_blocking(
        &self,
        category: BackupEnumCategory,
        id: Uuid,
        created_ms: u64,
    ) -> Result<BackupArchive, DesktopBackupCommandsError> {
        Ok(BackupCreator::create_blocking(
            BackupRequest {
                database: &self.database,
                directory: &self.directory,
                category,
                id,
                created_ms,
                dependencies: self.available_dependencies.clone(),
                portable_paths: self.portable_paths.clone(),
            },
            &self.space,
        )?)
    }

    pub fn list_blocking(
        &self,
    ) -> Result<Vec<DesktopBackupCommandsEntry>, DesktopBackupCommandsError> {
        Ok(BackupRetention::scan_blocking(&self.directory)?
            .into_iter()
            .map(|entry| DesktopBackupCommandsEntry {
                path: entry.path,
                manifest: entry.manifest,
            })
            .collect())
    }

    pub fn preview_blocking(
        &mut self,
        path: &Path,
    ) -> Result<
        (
            DesktopBackupCommandsPreviewHandle,
            Vec<crate::storage::backup::preview::BackupPreviewEnumFinding>,
        ),
        DesktopBackupCommandsError,
    > {
        let known = StorageMigrationRunner::embedded_versions().map_err(BackupError::from)?;
        let preview = BackupPreview::inspect_blocking(
            path,
            &BackupPreviewContext {
                known_schema_versions: &known,
                destination_platform: BackupEnumPlatform::current(),
                available_dependencies: &self.available_dependencies,
            },
        )?;
        let handle = DesktopBackupCommandsPreviewHandle {
            id: preview.id(),
            digest: preview.digest(),
        };
        let findings = preview.findings().to_vec();
        self.previews.insert(handle.id, preview);
        Ok((handle, findings))
    }

    pub fn restore_blocking(
        &mut self,
        lease: &DesktopProfileLease,
        handle: DesktopBackupCommandsPreviewHandle,
        prerequisites: DesktopBackupCommandsPrerequisites,
        safety_id: Uuid,
        restore_id: Uuid,
        now_ms: u64,
    ) -> Result<BackupRestoreOutcome, DesktopBackupCommandsError> {
        if let Some(unmet) = prerequisites.first_unmet() {
            return Err(DesktopBackupCommandsError::Prerequisite(unmet));
        }
        let preview = self
            .previews
            .get(&handle.id)
            .ok_or(DesktopBackupCommandsError::UnknownPreview(handle.id))?;
        if preview.digest() != handle.digest {
            return Err(BackupError::PreviewMismatch.into());
        }
        let safety = BackupCreator::create_blocking(
            BackupRequest {
                database: &self.database,
                directory: &self.directory,
                category: BackupEnumCategory::PreRestore,
                id: safety_id,
                created_ms: now_ms,
                dependencies: self.available_dependencies.clone(),
                portable_paths: self.portable_paths.clone(),
            },
            &self.space,
        )?;
        let outcome = BackupRestorer::restore_blocking(BackupRestoreRequest {
            lease,
            database: &self.database,
            preview,
            confirmed_id: handle.id,
            confirmed_digest: handle.digest,
            safety: &safety,
            restore_id,
        })?;
        self.previews.remove(&handle.id);
        Ok(outcome)
    }

    pub fn data_directory(&self) -> Option<&Path> {
        self.database.parent()
    }

    pub fn restore_pending_blocking(&self) -> Result<bool, DesktopBackupCommandsError> {
        match self.data_directory() {
            Some(directory) => Ok(BackupRestoreJournal::read_blocking(directory)?.is_some()),
            None => Ok(false),
        }
    }

    pub fn state_blocking(&self) -> Result<BackupState, DesktopBackupCommandsError> {
        let mut connection = StorageConnection::open(&self.database).map_err(BackupError::from)?;
        Ok(BackupScheduler::load_blocking(connection.diesel())?)
    }

    pub fn note_user_data_blocking(&self, now_ms: u64) -> Result<(), DesktopBackupCommandsError> {
        let mut connection = StorageConnection::open(&self.database).map_err(BackupError::from)?;
        let present = BackupCreator::has_user_data(connection.diesel())?;
        if present {
            BackupScheduler::record_user_data_blocking(connection.diesel(), now_ms)?;
        }
        Ok(())
    }

    fn protected_ids(&self) -> Result<Vec<Uuid>, DesktopBackupCommandsError> {
        let Some(directory) = self.data_directory() else {
            return Ok(Vec::new());
        };
        Ok(BackupRestoreJournal::read_blocking(directory)?
            .map(|journal| vec![journal.safety_backup()])
            .unwrap_or_default())
    }

    pub fn backup_now_blocking(
        &self,
        id: Uuid,
        now_ms: u64,
    ) -> Result<BackupArchive, DesktopBackupCommandsError> {
        let archive = self.create_blocking(BackupEnumCategory::Manual, id, now_ms)?;
        let mut connection = StorageConnection::open(&self.database).map_err(BackupError::from)?;
        BackupScheduler::record_success_blocking(
            connection.diesel(),
            BackupEnumCategory::Manual,
            now_ms,
        )?;
        BackupRetention::apply_blocking(&self.directory, &self.protected_ids()?)?;
        Ok(archive)
    }

    pub fn automatic_blocking(
        &self,
        id: Uuid,
        now_ms: u64,
    ) -> Result<BackupArchive, DesktopBackupCommandsError> {
        let created = self.create_blocking(BackupEnumCategory::Automatic, id, now_ms);
        let mut connection = StorageConnection::open(&self.database).map_err(BackupError::from)?;
        match created {
            Ok(archive) => {
                BackupScheduler::record_success_blocking(
                    connection.diesel(),
                    BackupEnumCategory::Automatic,
                    now_ms,
                )?;
                BackupRetention::apply_blocking(&self.directory, &self.protected_ids()?)?;
                Ok(archive)
            }
            Err(error) => {
                BackupScheduler::record_failure_blocking(connection.diesel(), &error.to_string())?;
                Err(error)
            }
        }
    }

    pub fn stage_restore_blocking(
        &mut self,
        lease: &DesktopProfileLease,
        handle: DesktopBackupCommandsPreviewHandle,
        prerequisites: DesktopBackupCommandsPrerequisites,
        safety_id: Uuid,
        restore_id: Uuid,
        now_ms: u64,
    ) -> Result<BackupRestoreOutcome, DesktopBackupCommandsError> {
        if let Some(unmet) = prerequisites.first_unmet() {
            return Err(DesktopBackupCommandsError::Prerequisite(unmet));
        }
        let preview = self
            .previews
            .get(&handle.id)
            .ok_or(DesktopBackupCommandsError::UnknownPreview(handle.id))?;
        if preview.digest() != handle.digest {
            return Err(BackupError::PreviewMismatch.into());
        }
        let safety = self.create_blocking(BackupEnumCategory::PreRestore, safety_id, now_ms)?;
        let outcome = BackupRestorer::stage_for_restart_blocking(BackupRestoreRequest {
            lease,
            database: &self.database,
            preview,
            confirmed_id: handle.id,
            confirmed_digest: handle.digest,
            safety: &safety,
            restore_id,
        })?;
        self.previews.remove(&handle.id);
        Ok(outcome)
    }

    pub fn find_blocking(&self, backup: Uuid) -> Result<PathBuf, DesktopBackupCommandsError> {
        BackupRetention::scan_blocking(&self.directory)?
            .into_iter()
            .find(|entry| entry.manifest.id() == backup)
            .map(|entry| entry.path)
            .ok_or(DesktopBackupCommandsError::UnknownPreview(backup))
    }
}
