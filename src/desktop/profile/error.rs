use std::io;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DesktopProfileError {
    #[error("The existing application data location is unavailable")]
    DataDirectoryUnavailable,
    #[error("The current effective operating-system user could not be established")]
    CurrentUserUnavailable,
    #[error("The current operating-system user query failed: {source}")]
    CurrentUserQuery {
        #[source]
        source: io::Error,
    },
    #[error("Invalid profile identifier: {0}")]
    InvalidProfileId(#[from] uuid::Error),
    #[error("The profile identifier must be a non-nil canonical lowercase UUID")]
    NoncanonicalProfileId,
    #[error("Profile filesystem operation failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Unsafe profile filesystem identity at {path}")]
    UnsafeIdentity { path: PathBuf },
    #[error("Unsafe profile ownership or permissions at {path}")]
    UnsafePermissions { path: PathBuf },
    #[error("The profile filesystem identity changed at {path}")]
    IdentityChanged { path: PathBuf },
    #[error("The destination-local profile identity record is invalid at {path}")]
    InvalidIdentityRecord {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("The filesystem does not support required exclusive profile locking: {source}")]
    LockUnsupported {
        #[source]
        source: io::Error,
    },
    #[error("Exclusive profile locking failed: {source}")]
    LockUnavailable {
        #[source]
        source: io::Error,
    },
    #[error("The profile must use a supported local filesystem: {path}")]
    NonlocalFilesystem { path: PathBuf },
    #[error("The profile filesystem has not been approved for safe local ownership: {path}")]
    UnsupportedFilesystem { path: PathBuf },
    #[error("The profile filesystem could not be established at {path}: {source}")]
    FilesystemQuery {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("The extended access control list could not be verified at {path}: {source}")]
    AclQuery {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("The profile control record exceeds {maximum} bytes at {path}")]
    ControlRecordTooLarge { path: PathBuf, maximum: u64 },
    #[error("The profile control record is invalid at {path}: {source}")]
    InvalidControlRecord {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("Profile control record version {received} is unsupported at {path}")]
    UnsupportedControlRecordVersion { path: PathBuf, received: u32 },
}

impl DesktopProfileError {
    pub(super) fn io(path: &std::path::Path, source: io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}
