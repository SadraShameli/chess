use std::io::Cursor;
use std::num::NonZeroUsize;

use diesel::SqliteConnection;
use thiserror::Error;

use crate::notation::pgn_import::{NotationPgnImport, NotationPgnImportEnumRecovery};
use crate::opening::book::{OpeningBook, OpeningBookError, OpeningBookLimits};
use crate::package::digest::PackageDigest;
use crate::puzzle::csv_import::{PuzzleCsvImport, PuzzleCsvImportEnumContinuation};
use crate::puzzle::identity::{PuzzleDefinitionIdentity, PuzzleDefinitionIdentityError};
use crate::puzzle::selection::PuzzleSelectionDataset;
use crate::storage::dataset_package::{
    StorageDatasetPackageEnumFormat, StorageDatasetPackageEnumValidation,
    StorageDatasetPackageError, StorageDatasetPackageSourceIdentity,
};
use crate::storage::dataset_registry::{
    StorageDatasetId, StorageDatasetRegistry, StorageDatasetRegistryEnumKind,
    StorageDatasetRegistryEnumOrigin, StorageDatasetRegistryError, StorageDatasetRegistryNew,
};

#[derive(Debug, Error)]
pub enum StorageDatasetImportError {
    #[error("The dataset import requires explicit confirmation")]
    ConfirmationRequired,
    #[error("The dataset file is empty")]
    Empty,
    #[error("The dataset needs {required} bytes but only {available} are available")]
    Space { required: u64, available: u64 },
    #[error("The dataset is larger than the {limit} byte import limit")]
    TooLarge { limit: u64 },
    #[error("The dataset import was cancelled")]
    Cancelled,
    #[error("The dataset contains no valid records")]
    NoValidRecords,
    #[error("The dataset format does not match the selected collection kind")]
    FormatMismatch,
    #[error(transparent)]
    Book(#[from] OpeningBookError),
    #[error(transparent)]
    Package(#[from] StorageDatasetPackageError),
    #[error(transparent)]
    Identity(#[from] PuzzleDefinitionIdentityError),
    #[error(transparent)]
    Registry(#[from] StorageDatasetRegistryError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageDatasetImportEnumConfirmation {
    Unconfirmed,
    Confirmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDatasetImportRequest {
    pub kind: StorageDatasetRegistryEnumKind,
    pub name: String,
    pub version: String,
    pub license: String,
    pub format: StorageDatasetPackageEnumFormat,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDatasetImportIssue {
    pub record: u64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDatasetImportPreview {
    request: StorageDatasetImportRequest,
    checksum: PackageDigest,
    bytes: u64,
    accepted: u64,
    rejected: u64,
    issues: Vec<StorageDatasetImportIssue>,
    replaces: Option<StorageDatasetId>,
}

impl StorageDatasetImportPreview {
    pub const fn request(&self) -> &StorageDatasetImportRequest {
        &self.request
    }
    pub const fn checksum(&self) -> PackageDigest {
        self.checksum
    }
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }
    pub const fn accepted(&self) -> u64 {
        self.accepted
    }
    pub const fn rejected(&self) -> u64 {
        self.rejected
    }
    pub fn issues(&self) -> &[StorageDatasetImportIssue] {
        &self.issues
    }
    pub const fn replaces(&self) -> Option<StorageDatasetId> {
        self.replaces
    }
}

struct StorageDatasetImportCounts {
    accepted: u64,
    rejected: u64,
    issues: Vec<StorageDatasetImportIssue>,
}

impl StorageDatasetImportCounts {
    const MAX_ISSUES: usize = 50;

    const fn new() -> Self {
        Self {
            accepted: 0,
            rejected: 0,
            issues: Vec::new(),
        }
    }

    fn reject(&mut self, record: u64, message: String) {
        self.rejected = self.rejected.saturating_add(1);
        if self.issues.len() < Self::MAX_ISSUES {
            self.issues.push(StorageDatasetImportIssue { record, message });
        }
    }
}

pub struct StorageDatasetImport;

impl StorageDatasetImport {
    pub const MAX_BYTES: u64 = 256 * 1024 * 1024;

    pub fn preview(
        request: StorageDatasetImportRequest,
        bytes: &[u8],
        available_bytes: u64,
        replaces: Option<StorageDatasetId>,
        cancelled: impl Fn() -> bool,
    ) -> Result<StorageDatasetImportPreview, StorageDatasetImportError> {
        let length = u64::try_from(bytes.len()).map_err(|_error| {
            StorageDatasetImportError::TooLarge {
                limit: Self::MAX_BYTES,
            }
        })?;
        if length == 0 {
            return Err(StorageDatasetImportError::Empty);
        }
        if length > Self::MAX_BYTES {
            return Err(StorageDatasetImportError::TooLarge {
                limit: Self::MAX_BYTES,
            });
        }
        if length > available_bytes {
            return Err(StorageDatasetImportError::Space {
                required: length,
                available: available_bytes,
            });
        }
        Self::check_kind(request.kind, request.format)?;
        let checksum = PackageDigest::hash_slice(bytes);
        let counts = match request.format {
            StorageDatasetPackageEnumFormat::Pgn => Self::validate_pgn(bytes, &cancelled)?,
            StorageDatasetPackageEnumFormat::Polyglot => {
                Self::validate_book(&request, bytes, checksum, &cancelled)?
            }
            StorageDatasetPackageEnumFormat::PuzzleCsv => {
                Self::validate_csv(&request, bytes, &cancelled)?
            }
            StorageDatasetPackageEnumFormat::PreparedPuzzleCatalogV1 => {
                return Err(StorageDatasetImportError::FormatMismatch);
            }
        };
        if counts.accepted == 0 {
            return Err(StorageDatasetImportError::NoValidRecords);
        }
        Ok(StorageDatasetImportPreview {
            request,
            checksum,
            bytes: length,
            accepted: counts.accepted,
            rejected: counts.rejected,
            issues: counts.issues,
            replaces,
        })
    }

    pub fn cancel(preview: StorageDatasetImportPreview) {
        drop(preview);
    }

    pub fn apply(
        connection: &mut SqliteConnection,
        preview: &StorageDatasetImportPreview,
        confirmation: StorageDatasetImportEnumConfirmation,
    ) -> Result<StorageDatasetId, StorageDatasetImportError> {
        if confirmation != StorageDatasetImportEnumConfirmation::Confirmed {
            return Err(StorageDatasetImportError::ConfirmationRequired);
        }
        let dataset = StorageDatasetRegistryNew {
            kind: preview.request.kind,
            origin: StorageDatasetRegistryEnumOrigin::Personal,
            name: preview.request.name.clone(),
            version: preview.request.version.clone(),
            format: preview.request.format,
            license: preview.request.license.clone(),
            checksum: preview.checksum,
            location: preview.request.location.clone(),
            validation: StorageDatasetPackageEnumValidation::IntegrityVerified,
            records: Some(u32::try_from(preview.accepted).unwrap_or(u32::MAX)),
        };
        Ok(match preview.replaces {
            Some(old) => StorageDatasetRegistry::replace(connection, old, &dataset)?,
            None => StorageDatasetRegistry::register(connection, &dataset)?,
        })
    }

    pub fn delete(
        connection: &mut SqliteConnection,
        dataset: StorageDatasetId,
        confirmation: StorageDatasetImportEnumConfirmation,
    ) -> Result<(), StorageDatasetImportError> {
        if confirmation != StorageDatasetImportEnumConfirmation::Confirmed {
            return Err(StorageDatasetImportError::ConfirmationRequired);
        }
        Ok(StorageDatasetRegistry::remove(connection, dataset)?)
    }

    const fn check_kind(
        kind: StorageDatasetRegistryEnumKind,
        format: StorageDatasetPackageEnumFormat,
    ) -> Result<(), StorageDatasetImportError> {
        let matches = matches!(
            (kind, format),
            (
                StorageDatasetRegistryEnumKind::Corpus,
                StorageDatasetPackageEnumFormat::Pgn
            ) | (
                StorageDatasetRegistryEnumKind::Book,
                StorageDatasetPackageEnumFormat::Polyglot
            ) | (
                StorageDatasetRegistryEnumKind::Puzzles,
                StorageDatasetPackageEnumFormat::PuzzleCsv
            )
        );
        if matches {
            Ok(())
        } else {
            Err(StorageDatasetImportError::FormatMismatch)
        }
    }

    fn validate_pgn(
        bytes: &[u8],
        cancelled: &impl Fn() -> bool,
    ) -> Result<StorageDatasetImportCounts, StorageDatasetImportError> {
        let mut node: u128 = 0;
        let mut parser = NotationPgnImport::new(Cursor::new(bytes), move || {
            node += 1;
            uuid::Uuid::from_u128(node).to_string().parse()
        });
        let mut counts = StorageDatasetImportCounts::new();
        let mut ordinal: u64 = 0;
        loop {
            if cancelled() {
                return Err(StorageDatasetImportError::Cancelled);
            }
            ordinal = ordinal.saturating_add(1);
            match parser.next_game(cancelled) {
                Ok(Some(_game)) => counts.accepted = counts.accepted.saturating_add(1),
                Ok(None) => break,
                Err(error) => {
                    counts.reject(error.game_ordinal(), error.to_string());
                    if error.recovery() == NotationPgnImportEnumRecovery::Terminated {
                        break;
                    }
                }
            }
        }
        Ok(counts)
    }

    fn validate_book(
        request: &StorageDatasetImportRequest,
        bytes: &[u8],
        checksum: PackageDigest,
        cancelled: &impl Fn() -> bool,
    ) -> Result<StorageDatasetImportCounts, StorageDatasetImportError> {
        let limits = OpeningBookLimits::new(
            NonZeroUsize::new(OpeningBookLimits::MAX_DECODED_BYTES)
                .ok_or(StorageDatasetImportError::Empty)?,
        )?;
        let source = StorageDatasetPackageSourceIdentity::new(
            request.name.clone(),
            request.version.clone(),
            checksum,
        )?;
        let book = OpeningBook::validate(bytes, source, checksum, limits, cancelled)?;
        let mut counts = StorageDatasetImportCounts::new();
        counts.accepted = u64::try_from(book.record_count()).unwrap_or(u64::MAX);
        Ok(counts)
    }

    fn validate_csv(
        request: &StorageDatasetImportRequest,
        bytes: &[u8],
        cancelled: &impl Fn() -> bool,
    ) -> Result<StorageDatasetImportCounts, StorageDatasetImportError> {
        let identity = PuzzleDefinitionIdentity::new(
            request.name.clone(),
            request.version.clone(),
            "preview".to_owned(),
        )?;
        let mut import = PuzzleCsvImport::new(
            Cursor::new(bytes),
            PuzzleSelectionDataset::from_identity(&identity),
        );
        let mut counts = StorageDatasetImportCounts::new();
        loop {
            match import.next_row(cancelled) {
                Ok(Some(_row)) => counts.accepted = counts.accepted.saturating_add(1),
                Ok(None) => break,
                Err(error) => {
                    let row = u64::try_from(error.logical_row()).unwrap_or(u64::MAX);
                    counts.reject(row, error.to_string());
                    if error.continuation() == PuzzleCsvImportEnumContinuation::Terminated {
                        break;
                    }
                }
            }
        }
        Ok(counts)
    }
}
