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
    #[error("A request identifier cannot be reused with a different payload")]
    RequestConflict,
    #[error("The subscription must resynchronize before accepting more events")]
    ResyncRequired,
    #[error("Acknowledgement is outside the delivered sequence")]
    InvalidAcknowledgement,
    #[error("Monotonic time moved backwards")]
    TimeOrder,
}
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopEnumFailureCode {
    InvalidInput,
    StaleRevision,
    RequestConflict,
    ResyncRequired,
    InvalidAcknowledgement,
    ResourceRefusal,
    FileUnavailable,
    InternalFailure,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopFailure {
    code: DesktopEnumFailureCode,
    message: String,
    retryable: bool,
}

impl DesktopFailure {
    pub fn code(&self) -> DesktopEnumFailureCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn is_retryable(&self) -> bool {
        self.retryable
    }

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

impl From<crate::clock::duration::ClockDurationError> for DesktopFailure {
    fn from(error: crate::clock::duration::ClockDurationError) -> Self {
        Self::new(
            DesktopEnumFailureCode::InternalFailure,
            error.to_string(),
            false,
        )
    }
}
