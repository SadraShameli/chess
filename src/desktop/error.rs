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
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesktopEnumFailureCode {
    InvalidInput,
    UnsupportedProtocol,
    StaleRevision,
    RequestConflict,
    ResyncRequired,
    InvalidAcknowledgement,
    ResourceRefusal,
    FileUnavailable,
    InternalFailure,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopFailure {
    code: DesktopEnumFailureCode,
    #[schemars(length(max = 1024))]
    message: String,
    retryable: bool,
}

impl DesktopFailure {
    pub fn new(code: DesktopEnumFailureCode, message: String, retryable: bool) -> Self {
        Self {
            code,
            message,
            retryable,
        }
    }
}

impl From<DesktopError> for DesktopFailure {
    fn from(error: DesktopError) -> Self {
        let code = match &error {
            DesktopError::InvalidIdentity(_)
            | DesktopError::NoncanonicalIdentity
            | DesktopError::InvalidRevision => DesktopEnumFailureCode::InvalidInput,
            DesktopError::RevisionOverflow | DesktopError::TimeOrder => {
                DesktopEnumFailureCode::InternalFailure
            }
            DesktopError::StaleRevision { .. } => DesktopEnumFailureCode::StaleRevision,
            DesktopError::UnsupportedProtocol { .. } => DesktopEnumFailureCode::UnsupportedProtocol,
            DesktopError::RequestConflict => DesktopEnumFailureCode::RequestConflict,
            DesktopError::ResyncRequired => DesktopEnumFailureCode::ResyncRequired,
            DesktopError::InvalidAcknowledgement => DesktopEnumFailureCode::InvalidAcknowledgement,
        };
        let retryable = matches!(
            code,
            DesktopEnumFailureCode::ResyncRequired | DesktopEnumFailureCode::StaleRevision
        );
        Self::new(code, error.to_string(), retryable)
    }
}
