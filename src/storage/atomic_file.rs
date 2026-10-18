use std::fs::{File, Metadata};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use same_file::Handle;
use tempfile::NamedTempFile;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageAtomicFileError {
    #[error("The selected location is unavailable: {0}")]
    Io(#[from] std::io::Error),
    #[error("The selected location changed; select it again")]
    StaleSelection,
    #[error("Select a regular file in an existing directory")]
    InvalidLocation,
    #[error("The staged file length exceeds the native range")]
    LengthOverflow,
    #[error("The staged file was cancelled before publication")]
    Cancelled,
    #[error("The file was published but its durable outcome is indeterminate: {0}")]
    PublicationIndeterminate(#[source] std::io::Error),
}

pub struct StorageFileIdentity {
    handle: Handle,
    length: u64,
    modified: SystemTime,
}

impl StorageFileIdentity {
    pub fn capture(path: &Path) -> Result<Self, StorageAtomicFileError> {
        let metadata = Self::regular_metadata(path)?;
        Ok(Self {
            handle: Handle::from_path(path)?,
            length: metadata.len(),
            modified: metadata.modified()?,
        })
    }

    pub fn validate(&self, path: &Path) -> Result<(), StorageAtomicFileError> {
        let metadata = Self::regular_metadata(path)?;
        if self.handle != Handle::from_path(path)?
            || self.length != metadata.len()
            || self.modified != metadata.modified()?
        {
            return Err(StorageAtomicFileError::StaleSelection);
        }
        Ok(())
    }

    fn regular_metadata(path: &Path) -> Result<Metadata, StorageAtomicFileError> {
        let metadata = path.symlink_metadata()?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(StorageAtomicFileError::InvalidLocation);
        }
        Ok(metadata)
    }
}

pub struct StorageAtomicFileTarget {
    path: PathBuf,
    parent: PathBuf,
    parent_identity: Handle,
    existing: Option<StorageFileIdentity>,
}

impl StorageAtomicFileTarget {
    pub fn capture(path: PathBuf) -> Result<Self, StorageAtomicFileError> {
        let name = path
            .file_name()
            .ok_or(StorageAtomicFileError::InvalidLocation)?;
        let parent = path
            .parent()
            .ok_or(StorageAtomicFileError::InvalidLocation)?
            .canonicalize()?;
        if !parent.is_dir() {
            return Err(StorageAtomicFileError::InvalidLocation);
        }
        let canonical_path = parent.join(name);
        let existing = match canonical_path.symlink_metadata() {
            Ok(_metadata) => Some(StorageFileIdentity::capture(&canonical_path)?),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let parent_identity = Handle::from_path(&parent)?;
        Ok(Self {
            path: canonical_path,
            parent,
            parent_identity,
            existing,
        })
    }

    fn validate(&self) -> Result<(), StorageAtomicFileError> {
        if self.parent_identity != Handle::from_path(&self.parent)? {
            return Err(StorageAtomicFileError::StaleSelection);
        }
        match &self.existing {
            Some(identity) => identity.validate(&self.path),
            None => match self.path.symlink_metadata() {
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Ok(_metadata) => Err(StorageAtomicFileError::StaleSelection),
                Err(error) => Err(error.into()),
            },
        }
    }
}

pub struct StorageAtomicFileStage {
    target: StorageAtomicFileTarget,
    temporary: NamedTempFile,
    bytes: u64,
}

impl StorageAtomicFileStage {
    pub fn new(target: StorageAtomicFileTarget) -> Result<Self, StorageAtomicFileError> {
        target.validate()?;
        let temporary = NamedTempFile::new_in(&target.parent)?;
        Ok(Self {
            target,
            temporary,
            bytes: 0,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), StorageAtomicFileError> {
        let length =
            u64::try_from(bytes.len()).map_err(|_error| StorageAtomicFileError::LengthOverflow)?;
        let total = self
            .bytes
            .checked_add(length)
            .ok_or(StorageAtomicFileError::LengthOverflow)?;
        self.temporary.write_all(bytes)?;
        self.bytes = total;
        Ok(())
    }

    pub fn commit(self) -> Result<u64, StorageAtomicFileError> {
        self.commit_if(|| false)
    }

    pub fn commit_if(
        mut self,
        cancelled: impl FnOnce() -> bool,
    ) -> Result<u64, StorageAtomicFileError> {
        self.temporary.flush()?;
        self.temporary.as_file().sync_all()?;
        self.target.validate()?;
        if cancelled() {
            return Err(StorageAtomicFileError::Cancelled);
        }
        let result = if self.target.existing.is_some() {
            self.temporary.persist(&self.target.path)
        } else {
            self.temporary.persist_noclobber(&self.target.path)
        };
        let file: File = result.map_err(|error| StorageAtomicFileError::Io(error.error))?;
        file.sync_all()
            .map_err(StorageAtomicFileError::PublicationIndeterminate)?;
        #[cfg(unix)]
        File::open(&self.target.parent)
            .and_then(|parent| parent.sync_all())
            .map_err(StorageAtomicFileError::PublicationIndeterminate)?;
        Ok(self.bytes)
    }
}
