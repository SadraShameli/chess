use std::collections::HashMap;
use std::fs;
use std::io::Read;
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
    BackupDependency, BackupEnumCategory, BackupEnumDependencyKind, BackupEnumPlatform,
    BackupManifest,
};
use crate::storage::backup::preview::{BackupPreview, BackupPreviewContext};
use crate::storage::backup::restore::{
    BackupDestinationLocation, BackupLeaseProof, BackupRestoreOutcome, BackupRestoreRequest,
    BackupRestorer,
};
use crate::storage::backup::retention::BackupRetention;
use crate::storage::backup::scheduler::{BackupScheduler, BackupState};
use crate::storage::connection::StorageConnection;
use crate::storage::dataset_package::StorageDatasetPackageEnumValidation;
use crate::storage::dataset_registry::StorageDatasetRegistry;
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
        let (dependencies, portable_paths, _locations) = self.dataset_metadata_blocking()?;
        Ok(BackupCreator::create_blocking(
            BackupRequest {
                database: &self.database,
                directory: &self.directory,
                category,
                id,
                created_ms,
                dependencies,
                portable_paths,
            },
            &self.space,
        )?)
    }

    fn dataset_metadata_blocking(
        &self,
    ) -> Result<
        (
            Vec<BackupDependency>,
            Vec<String>,
            Vec<BackupDestinationLocation>,
        ),
        DesktopBackupCommandsError,
    > {
        let mut dependencies = self.available_dependencies.clone();
        let mut paths = self.portable_paths.clone();
        let mut locations = Vec::new();
        let mut connection = StorageConnection::open(&self.database).map_err(BackupError::from)?;
        let entries =
            StorageDatasetRegistry::list(connection.diesel()).map_err(BackupError::from)?;
        for entry in entries.into_iter().filter(|entry| !entry.superseded) {
            let checksum = entry.checksum.to_string();
            dependencies.push(BackupDependency {
                kind: BackupEnumDependencyKind::Dataset,
                name: format!("{} {}", entry.name, entry.version),
                digest: Some(checksum.clone()),
            });
            if !paths.contains(&entry.location) {
                paths.push(entry.location.clone());
            }
            locations.push(BackupDestinationLocation {
                name: entry.name,
                version: entry.version,
                checksum,
                location: entry.location,
                integrity_verified: entry.validation
                    == StorageDatasetPackageEnumValidation::IntegrityVerified,
            });
        }
        Ok((dependencies, paths, locations))
    }

    pub fn import_blocking(
        &mut self,
        source: impl Read,
        id: Uuid,
    ) -> Result<
        (
            DesktopBackupCommandsPreviewHandle,
            Vec<crate::storage::backup::preview::BackupPreviewEnumFinding>,
        ),
        DesktopBackupCommandsError,
    > {
        fs::create_dir_all(&self.directory).map_err(BackupError::from)?;
        let partial = self.directory.join(format!("{id}.import"));
        let limit = BackupManifest::MAX_PAYLOAD_BYTES
            + u64::try_from(
                crate::storage::backup::archive::BackupArchiveFormat::MAX_MANIFEST_BYTES,
            )
            .unwrap_or(u64::MAX)
            + 16;
        let copied = Self::copy_bounded(source, &partial, limit);
        let inspected = copied.and_then(|()| self.inspect(&partial));
        let preview = match inspected {
            Ok(preview) => preview,
            Err(error) => {
                drop(fs::remove_file(&partial));
                return Err(error.into());
            }
        };
        let destination = self
            .directory
            .join(BackupCreator::file_name(preview.manifest()));
        if destination.exists() {
            fs::remove_file(&partial).map_err(BackupError::from)?;
        } else {
            fs::rename(&partial, &destination).map_err(BackupError::from)?;
        }
        self.preview_blocking(&destination)
    }

    fn copy_bounded(source: impl Read, target: &Path, limit: u64) -> Result<(), BackupError> {
        let mut file = fs::File::create(target)?;
        let copied = std::io::copy(&mut source.take(limit), &mut file)?;
        if copied >= limit {
            return Err(BackupError::InvalidArchive("archive size"));
        }
        file.sync_all()?;
        Ok(())
    }

    fn inspect(&self, path: &Path) -> Result<BackupPreview, BackupError> {
        let known = StorageMigrationRunner::embedded_versions()?;
        let (available, _paths, _locations) = self
            .dataset_metadata_blocking()
            .map_err(|_error| BackupError::InvalidArchive("destination registry"))?;
        BackupPreview::inspect_blocking(
            path,
            &BackupPreviewContext {
                known_schema_versions: &known,
                destination_platform: BackupEnumPlatform::current(),
                available_dependencies: &available,
            },
        )
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
        let preview = self.inspect(path)?;
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
        let safety = self.create_blocking(BackupEnumCategory::PreRestore, safety_id, now_ms)?;
        let (_dependencies, _paths, locations) = self.dataset_metadata_blocking()?;
        let outcome = BackupRestorer::restore_blocking(BackupRestoreRequest {
            locations: &locations,
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

    pub fn pre_update_blocking(
        &self,
        prerequisites: DesktopBackupCommandsPrerequisites,
        id: Uuid,
        now_ms: u64,
    ) -> Result<Uuid, DesktopBackupCommandsError> {
        if let Some(unmet) = prerequisites.first_unmet() {
            return Err(DesktopBackupCommandsError::Prerequisite(unmet));
        }
        let archive = self.create_blocking(BackupEnumCategory::PreUpdate, id, now_ms)?;
        let found = self.find_blocking(archive.manifest().id())?;
        if found.as_path() != archive.path() {
            return Err(BackupError::PreviewMismatch.into());
        }
        let known = StorageMigrationRunner::embedded_versions().map_err(BackupError::from)?;
        let verified = BackupPreview::inspect_blocking(
            &found,
            &BackupPreviewContext {
                known_schema_versions: &known,
                destination_platform: BackupEnumPlatform::current(),
                available_dependencies: &self.available_dependencies,
            },
        )?;
        if verified.manifest() != archive.manifest()
            || verified.manifest().category() != BackupEnumCategory::PreUpdate
        {
            return Err(BackupError::PreviewMismatch.into());
        }
        Ok(archive.manifest().id())
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
        let (_dependencies, _paths, locations) = self.dataset_metadata_blocking()?;
        let outcome = BackupRestorer::stage_for_restart_blocking(BackupRestoreRequest {
            locations: &locations,
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
