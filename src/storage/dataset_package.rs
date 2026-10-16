use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::capacity::executor::{
    CapacityExecutor, CapacityExecutorCancellation, CapacityExecutorError, CapacityExecutorJob,
};
use crate::package::digest::{PackageDigest, PackageDigestError};

#[derive(Debug, Error)]
pub enum StorageDatasetPackageError {
    #[error("A dataset location must be absolute: {0}")]
    RelativeLocation(PathBuf),
    #[error("The dataset is not a regular file: {0}")]
    NotRegularFile(PathBuf),
    #[error("Dataset access failed at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("The dataset checksum does not match at {path}: expected {expected}, read {actual}")]
    Checksum {
        path: PathBuf,
        expected: PackageDigest,
        actual: PackageDigest,
    },
    #[error("Dataset integrity verification was canceled")]
    Cancelled,
    #[error(transparent)]
    Digest(#[from] PackageDigestError),
    #[error(transparent)]
    Executor(#[from] CapacityExecutorError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageDatasetPackageEnumFormat {
    Pgn,
    Polyglot,
    PuzzleCsv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageDatasetPackageEnumValidation {
    Unchecked,
    IntegrityVerified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDatasetPackage {
    format: StorageDatasetPackageEnumFormat,
    checksum: PackageDigest,
    location: PathBuf,
    validation: StorageDatasetPackageEnumValidation,
}

impl StorageDatasetPackage {
    pub fn new(
        format: StorageDatasetPackageEnumFormat,
        checksum: PackageDigest,
        location: PathBuf,
    ) -> Result<Self, StorageDatasetPackageError> {
        if !location.is_absolute() {
            return Err(StorageDatasetPackageError::RelativeLocation(location));
        }
        Ok(Self {
            format,
            checksum,
            location,
            validation: StorageDatasetPackageEnumValidation::Unchecked,
        })
    }

    pub const fn format(&self) -> StorageDatasetPackageEnumFormat {
        self.format
    }

    pub const fn checksum(&self) -> PackageDigest {
        self.checksum
    }

    pub fn location(&self) -> &Path {
        &self.location
    }

    pub const fn validation(&self) -> StorageDatasetPackageEnumValidation {
        self.validation
    }

    pub fn verify_integrity(
        self,
        executor: &CapacityExecutor,
    ) -> Result<
        CapacityExecutorJob<Result<Self, StorageDatasetPackageError>>,
        StorageDatasetPackageError,
    > {
        Ok(executor.submit(move |cancellation| self.verify(cancellation))?)
    }

    fn verify(
        mut self,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Self, StorageDatasetPackageError> {
        if cancellation.is_cancelled() {
            return Err(StorageDatasetPackageError::Cancelled);
        }
        let location = self
            .location
            .canonicalize()
            .map_err(|source| self.io(source))?;
        let metadata = location.metadata().map_err(|source| self.io(source))?;
        if !metadata.is_file() {
            return Err(StorageDatasetPackageError::NotRegularFile(location));
        }
        let file = File::open(&location).map_err(|source| self.io(source))?;
        let result = PackageDigest::read(StorageDatasetPackageReader { file, cancellation });
        if cancellation.is_cancelled() {
            return Err(StorageDatasetPackageError::Cancelled);
        }
        let actual = result?;
        if actual != self.checksum {
            return Err(StorageDatasetPackageError::Checksum {
                path: location,
                expected: self.checksum,
                actual,
            });
        }
        self.location = location;
        self.validation = StorageDatasetPackageEnumValidation::IntegrityVerified;
        Ok(self)
    }

    fn io(&self, source: std::io::Error) -> StorageDatasetPackageError {
        StorageDatasetPackageError::Io {
            path: self.location.clone(),
            source,
        }
    }
}

struct StorageDatasetPackageReader<'a> {
    file: File,
    cancellation: &'a CapacityExecutorCancellation,
}

impl Read for StorageDatasetPackageReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.cancellation.is_cancelled() {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        self.file.read(buffer)
    }
}
