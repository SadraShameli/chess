use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::storage::backup::error::BackupError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupRestoreEnumPhase {
    Validated,
    Installing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupRestoreJournal {
    restore_id: Uuid,
    phase: BackupRestoreEnumPhase,
    safety_backup: Uuid,
}

impl BackupRestoreJournal {
    pub const FILE_NAME: &'static str = "restore.journal";
    pub const MAX_BYTES: u64 = 4096;

    pub const fn new(restore_id: Uuid, safety_backup: Uuid) -> Self {
        Self {
            restore_id,
            phase: BackupRestoreEnumPhase::Validated,
            safety_backup,
        }
    }

    pub const fn restore_id(&self) -> Uuid {
        self.restore_id
    }

    pub const fn phase(&self) -> BackupRestoreEnumPhase {
        self.phase
    }

    pub const fn safety_backup(&self) -> Uuid {
        self.safety_backup
    }

    pub fn with_phase(&self, phase: BackupRestoreEnumPhase) -> Self {
        Self {
            phase,
            ..self.clone()
        }
    }

    pub fn path(data_directory: &Path) -> PathBuf {
        data_directory.join(Self::FILE_NAME)
    }

    pub fn staged_path(data_directory: &Path, restore_id: Uuid) -> PathBuf {
        data_directory.join(format!("restore-{restore_id}.staged"))
    }

    pub fn aside_path(database: &Path, restore_id: Uuid) -> PathBuf {
        PathBuf::from(format!("{}.pre-restore-{restore_id}", database.display()))
    }

    pub fn write_blocking(&self, data_directory: &Path) -> Result<(), BackupError> {
        let temporary = data_directory.join(format!("{}.next", Self::FILE_NAME));
        let mut file = File::create(&temporary)?;
        file.write_all(&serde_json::to_vec(self)?)?;
        file.sync_all()?;
        fs::rename(&temporary, Self::path(data_directory))?;
        Self::sync_directory(data_directory)
    }

    pub fn read_blocking(data_directory: &Path) -> Result<Option<Self>, BackupError> {
        let path = Self::path(data_directory);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_file() || metadata.len() > Self::MAX_BYTES {
            return Err(BackupError::JournalInconsistent);
        }
        serde_json::from_slice(&fs::read(&path)?)
            .map(Some)
            .map_err(|_error| BackupError::JournalInconsistent)
    }

    pub fn remove_blocking(data_directory: &Path) -> Result<(), BackupError> {
        match fs::remove_file(Self::path(data_directory)) {
            Ok(()) => Self::sync_directory(data_directory),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    #[cfg(unix)]
    pub(crate) fn sync_directory(directory: &Path) -> Result<(), BackupError> {
        File::open(directory)?.sync_all()?;
        Ok(())
    }

    #[cfg(not(unix))]
    pub(crate) fn sync_directory(_directory: &Path) -> Result<(), BackupError> {
        Ok(())
    }
}
