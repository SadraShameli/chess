use std::path::PathBuf;

use thiserror::Error;

use crate::package::digest::PackageDigestError;
use crate::storage::document_store::StorageDocumentError;

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("The backup location is unavailable: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Storage(#[from] StorageDocumentError),
    #[error("The backup database cannot be opened: {0}")]
    Connection(#[from] diesel::ConnectionError),
    #[error("The backup database operation failed: {0}")]
    Diesel(#[from] diesel::result::Error),
    #[error("The backup manifest is not valid: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Digest(#[from] PackageDigestError),
    #[error("The stored revision is not a valid number: {0}")]
    Revision(#[from] std::num::ParseIntError),
    #[error("The backup archive is invalid: {0}")]
    InvalidArchive(&'static str),
    #[error("The backup archive checksum does not match its contents")]
    ChecksumMismatch,
    #[error("The backup archive format version {found} is not supported")]
    UnsupportedFormat { found: u32 },
    #[error("The backup was written by a newer or unknown database schema version {version}")]
    UnsupportedSchema { version: String },
    #[error("The backup needs {required} bytes but only {available} bytes are available")]
    InsufficientSpace { required: u64, available: u64 },
    #[error("The database kept changing while the snapshot was taken; try again")]
    ConcurrentWrite,
    #[error("The backup destination {0} is not a usable directory")]
    Destination(PathBuf),
    #[error("A fresh safety backup must be created before restoring")]
    SafetyBackupRequired,
    #[error("The restore confirmation does not match the previewed archive")]
    PreviewMismatch,
    #[error("The restored database failed its integrity check: {0}")]
    Integrity(String),
    #[error("The restore journal is missing, malformed or contradicts the files on disk")]
    JournalInconsistent,
    #[error("A restore is already staged and waits for the next launch")]
    RestoreAlreadyPending,
    #[error("The profile lease no longer protects {0}")]
    Lease(PathBuf),
}
