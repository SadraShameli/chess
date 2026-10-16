use std::sync::{Arc, OnceLock};

use crate::clock::duration::ClockInstant;
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::error::DesktopFailure;
use crate::desktop::file_handle::DesktopFileEnumPurpose;
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DesktopMessagesEntityId(DesktopRequestId);

impl DesktopMessagesEntityId {
    pub fn new() -> Self {
        Self(DesktopRequestId::new())
    }
}

impl Default for DesktopMessagesEntityId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopEnumCommand {
    Bootstrap,
    FileSelect {
        owner: DesktopMessagesEntityId,
        purpose: DesktopFileEnumPurpose,
    },
    StreamSubscribe,
    StreamAcknowledge {
        subscription_id: DesktopRequestId,
        sequence: DesktopRevision,
    },
    StreamResync {
        subscription_id: DesktopRequestId,
    },
    Stop {
        owner: DesktopMessagesEntityId,
    },
    Pause {
        owner: DesktopMessagesEntityId,
    },
    Quit,
}

impl DesktopEnumCommand {
    pub fn is_control(&self) -> bool {
        self.control_key().is_some()
    }

    pub(crate) fn control_key(&self) -> Option<DesktopMessagesEnumControlKey> {
        match self {
            Self::Stop { owner } => Some(DesktopMessagesEnumControlKey::Stop(*owner)),
            Self::Pause { owner } => Some(DesktopMessagesEnumControlKey::Pause(*owner)),
            Self::Quit => Some(DesktopMessagesEnumControlKey::Quit),
            Self::Bootstrap
            | Self::FileSelect { .. }
            | Self::StreamSubscribe
            | Self::StreamAcknowledge { .. }
            | Self::StreamResync { .. } => None,
        }
    }

    pub fn mutation_entity(&self) -> Option<DesktopMessagesEntityId> {
        match self {
            Self::FileSelect { owner, .. } => Some(*owner),
            Self::Bootstrap
            | Self::StreamSubscribe
            | Self::StreamAcknowledge { .. }
            | Self::StreamResync { .. }
            | Self::Stop { .. }
            | Self::Pause { .. }
            | Self::Quit => None,
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum DesktopMessagesEnumControlKey {
    Stop(DesktopMessagesEntityId),
    Pause(DesktopMessagesEntityId),
    Quit,
}

impl DesktopMessagesEnumControlKey {
    pub(crate) fn belongs_to(self, owner: DesktopMessagesEntityId) -> bool {
        match self {
            Self::Stop(target) | Self::Pause(target) => target == owner,
            Self::Quit => false,
        }
    }

    pub(crate) fn stopped_result(self) -> DesktopMessagesEnumResult {
        match self {
            Self::Stop(_) => DesktopMessagesEnumResult::Stopped,
            Self::Pause(_) => DesktopMessagesEnumResult::Paused,
            Self::Quit => DesktopMessagesEnumResult::QuitReady,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopCommand {
    operation: DesktopEnumCommand,
    expected_revision: Option<DesktopRevision>,
}

impl DesktopCommand {
    pub fn new(operation: DesktopEnumCommand, expected_revision: Option<DesktopRevision>) -> Self {
        Self {
            operation,
            expected_revision,
        }
    }

    pub fn operation(&self) -> &DesktopEnumCommand {
        &self.operation
    }

    pub fn expected_revision(&self) -> Option<DesktopRevision> {
        self.expected_revision
    }
}

struct DesktopMessagesRequestIdentity {
    session: DesktopRequestId,
    sequence: u64,
    request_id: DesktopRequestId,
    command: DesktopCommand,
    received_at: OnceLock<ClockInstant>,
    admitted_sequence: OnceLock<u64>,
}

#[derive(Clone)]
pub struct DesktopRequest(Arc<DesktopMessagesRequestIdentity>);

impl DesktopRequest {
    pub(crate) fn new(session: DesktopRequestId, sequence: u64, command: DesktopCommand) -> Self {
        Self(Arc::new(DesktopMessagesRequestIdentity {
            session,
            sequence,
            request_id: DesktopRequestId::new(),
            command,
            received_at: OnceLock::new(),
            admitted_sequence: OnceLock::new(),
        }))
    }

    pub fn request_id(&self) -> DesktopRequestId {
        self.0.request_id
    }

    pub fn command(&self) -> &DesktopCommand {
        &self.0.command
    }

    pub(crate) fn session(&self) -> DesktopRequestId {
        self.0.session
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.0.sequence
    }

    pub(crate) fn first_receipt(&self, now: ClockInstant) -> ClockInstant {
        *self.0.received_at.get_or_init(|| now)
    }

    pub(crate) fn admitted_sequence(&self) -> Option<u64> {
        self.0.admitted_sequence.get().copied()
    }

    pub(crate) fn retain_admission(&self, sequence: u64) {
        self.0.admitted_sequence.get_or_init(|| sequence);
    }
}

#[derive(Clone, Debug)]
pub enum DesktopMessagesEnumResult {
    Value {
        value: DesktopEnumValue,
        revision: Option<DesktopRevision>,
    },
    Failure(DesktopFailure),
    Cancelled,
    Stopped,
    Paused,
    QuitReady,
    ReconciliationRequired,
}
