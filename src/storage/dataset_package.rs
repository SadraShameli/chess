use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

mod artifact;
mod manifest;
mod measurement;
mod observation;
mod provenance;
mod source;

pub use artifact::{
    StorageDatasetPackageArtifact, StorageDatasetPackageArtifactInput,
    StorageDatasetPackageArtifactReference, StorageDatasetPackageEnumArtifactRole,
};
pub use manifest::{StorageDatasetPackageManifest, StorageDatasetPackageManifestInput};
pub use measurement::{
    StorageDatasetPackageMeasurement, StorageDatasetPackageMeasurements,
    StorageDatasetPackageReduction,
};
pub use observation::{
    StorageDatasetPackageCounts, StorageDatasetPackageObservation,
    StorageDatasetPackageObservationInput,
};
pub use provenance::{
    StorageDatasetPackageEnumClassification, StorageDatasetPackageRawField,
    StorageDatasetPackageRecordProvenance, StorageDatasetPackageSourceRecord,
};
pub use source::{
    StorageDatasetPackageEnumLicense, StorageDatasetPackageEnumSourceKind,
    StorageDatasetPackageEnumUnavailableCategory, StorageDatasetPackageEnumUnavailableReason,
    StorageDatasetPackageLicense, StorageDatasetPackageLicenseId, StorageDatasetPackageSource,
    StorageDatasetPackageSourceIdentity, StorageDatasetPackageUnavailable,
};

use crate::capacity::executor::{
    CapacityExecutor, CapacityExecutorCancellation, CapacityExecutorError, CapacityExecutorJob,
};
use crate::package::digest::{PackageDigest, PackageDigestError};

#[derive(Debug, Error)]
pub enum StorageDatasetPackageError {
    #[error("Dataset text is empty, contains a control character, or has surrounding whitespace")]
    InvalidText,
    #[error("Unsafe or noncanonical installed artifact path: {0}")]
    UnsafePath(String),
    #[error("Observed counts are inconsistent or exceed their checked range")]
    Counts,
    #[error("Dataset source identities or source/version bindings are duplicated")]
    DuplicateSource,
    #[error("A source or record binding is missing or conflicts with the frozen source")]
    SourceBinding,
    #[error(
        "Starter preparation requires exactly one CC0 standard-game, puzzle, and openings source"
    )]
    SourceKinds,
    #[error("An installed artifact reference is missing, duplicated, or has a different checksum")]
    ArtifactReference,
    #[error("The artifact role, component, or source bindings do not match the reference")]
    ArtifactBinding,
    #[error("Artifact fixed bytes exceed its installed bytes")]
    ArtifactSize,
    #[error("Subset observations must occur once each in canonical order")]
    ObservationOrder,
    #[error("Observed stratum counts, order, subset, or unknown counts are inconsistent")]
    Strata,
    #[error(
        "Selected or initial prefix lengths do not match observed availability or reduction history"
    )]
    Prefix,
    #[error("Frozen inputs do not reproduce the declared selection")]
    Reproduction,
    #[error("Installed component measurements are incomplete, unordered, or inconsistent")]
    Measurements,
    #[error("The recorded byte-cap reduction does not match the canonical transition")]
    Reduction,
    #[error("Unsupported dataset preparation format, schema, or policy version")]
    Version,
    #[error("The semantic validation receipt does not bind this descriptor's format and checksum")]
    SemanticBinding,
    #[error("Raw metadata has ambiguous values for a classified field")]
    AmbiguousMetadata,
    #[error("An accepted preparation manifest cannot have an unavailable required source")]
    RequiredSourceUnavailable,
    #[error(transparent)]
    Selection(#[from] crate::storage::dataset_selection::StorageDatasetSelectionError),
    #[error(transparent)]
    RecordIdentity(#[from] crate::puzzle::identity::PuzzleDefinitionIdentityError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
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

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    strum::EnumString,
    strum::AsRefStr,
    strum::Display,
    strum::EnumIter,
    diesel::AsExpression,
    diesel::FromSqlRow,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetPackageEnumFormat {
    #[strum(serialize = "pgn")]
    Pgn,
    #[strum(serialize = "polyglot")]
    Polyglot,
    #[strum(serialize = "puzzle_csv")]
    PuzzleCsv,
    #[strum(serialize = "prepared_puzzle_catalog_v1")]
    PreparedPuzzleCatalogV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageDatasetPackageEnumSemanticValidation {
    Unchecked,
    Validated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDatasetPackageSemanticReceipt {
    format: StorageDatasetPackageEnumFormat,
    checksum: PackageDigest,
    accepted: u64,
    rejected: u64,
}

impl StorageDatasetPackageSemanticReceipt {
    pub fn new(
        format: StorageDatasetPackageEnumFormat,
        checksum: PackageDigest,
        accepted: u64,
        rejected: u64,
    ) -> Result<Self, StorageDatasetPackageError> {
        accepted
            .checked_add(rejected)
            .ok_or(StorageDatasetPackageError::Counts)?;
        Ok(Self {
            format,
            checksum,
            accepted,
            rejected,
        })
    }

    pub const fn format(&self) -> StorageDatasetPackageEnumFormat {
        self.format
    }
    pub const fn checksum(&self) -> PackageDigest {
        self.checksum
    }
    pub const fn accepted(&self) -> u64 {
        self.accepted
    }
    pub const fn rejected(&self) -> u64 {
        self.rejected
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::EnumString,
    strum::AsRefStr,
    strum::Display,
    strum::EnumIter,
    diesel::AsExpression,
    diesel::FromSqlRow,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
pub enum StorageDatasetPackageEnumValidation {
    #[strum(serialize = "unchecked")]
    Unchecked,
    #[strum(serialize = "integrity_verified")]
    IntegrityVerified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDatasetPackage {
    format: StorageDatasetPackageEnumFormat,
    checksum: PackageDigest,
    location: PathBuf,
    validation: StorageDatasetPackageEnumValidation,
    source: Option<StorageDatasetPackageSource>,
    semantic_receipt: Option<StorageDatasetPackageSemanticReceipt>,
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
            source: None,
            semantic_receipt: None,
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

    pub fn with_source(
        mut self,
        source: StorageDatasetPackageSource,
    ) -> Result<Self, StorageDatasetPackageError> {
        if self
            .source
            .as_ref()
            .is_some_and(|existing| existing != &source)
        {
            return Err(StorageDatasetPackageError::SourceBinding);
        }
        self.source = Some(source);
        Ok(self)
    }

    pub const fn source(&self) -> Option<&StorageDatasetPackageSource> {
        self.source.as_ref()
    }

    pub fn with_semantic_receipt(
        mut self,
        receipt: StorageDatasetPackageSemanticReceipt,
    ) -> Result<Self, StorageDatasetPackageError> {
        if receipt.format != self.format || receipt.checksum != self.checksum {
            return Err(StorageDatasetPackageError::SemanticBinding);
        }
        self.semantic_receipt = Some(receipt);
        Ok(self)
    }

    pub const fn semantic_validation(&self) -> StorageDatasetPackageEnumSemanticValidation {
        match self.semantic_receipt {
            Some(_) => StorageDatasetPackageEnumSemanticValidation::Validated,
            None => StorageDatasetPackageEnumSemanticValidation::Unchecked,
        }
    }

    pub const fn semantic_receipt(&self) -> Option<&StorageDatasetPackageSemanticReceipt> {
        self.semantic_receipt.as_ref()
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
