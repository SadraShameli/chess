use thiserror::Error;

use crate::desktop::identity::DesktopRevision;

#[derive(Debug, Error)]
pub enum DesktopError {
    #[error("Invalid identifier: {0}")]
    InvalidIdentity(#[from] uuid::Error),
    #[error("Identifiers must be non-nil lowercase canonical UUID strings")]
    NoncanonicalIdentity,
    #[error("Revisions must be canonical unsigned decimal strings")]
    InvalidRevision,
    #[error("The revision counter is exhausted")]
    RevisionOverflow,
    #[error("State changed: expected revision {expected}, current revision {actual}")]
    StaleRevision {
        expected: DesktopRevision,
        actual: DesktopRevision,
    },
    #[error("Unsupported protocol version {received}; update the application")]
    UnsupportedProtocol { received: u32 },
    #[error("A request identifier cannot be reused with a different payload")]
    RequestConflict,
    #[error("The subscription must resynchronize before accepting more events")]
    ResyncRequired,
    #[error("Acknowledgement is outside the delivered sequence")]
    InvalidAcknowledgement,
    #[error("Monotonic time moved backwards")]
    TimeOrder,
}
