use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::desktop::error::DesktopError;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::profile::activation::DesktopProfileActivationBinding;
use crate::desktop::profile::control_record::{
    DesktopProfileControlRecord, DesktopProfileControlRecordEnumKind,
};
use crate::desktop::profile::{
    DesktopProfileError, DesktopProfileId, DesktopProfileLease, DesktopProfileSecurity,
};

#[derive(Debug, Error)]
pub enum DesktopProfileOwnerRecordError {
    #[error(transparent)]
    Profile(#[from] DesktopProfileError),
    #[error("The owner generation is invalid: {0}")]
    Generation(#[from] DesktopError),
    #[error("The activation endpoint is not a bounded local socket or local named pipe")]
    InvalidEndpoint,
    #[error("The bound endpoint does not match the leased profile and owner generation")]
    BindingMismatch,
    #[error("The owner cannot change phase from {current:?} to {requested:?}")]
    InvalidPhaseTransition {
        current: DesktopProfileEnumPhase,
        requested: DesktopProfileEnumPhase,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopProfileEnumPhase {
    Starting,
    Ready,
    Recovery,
    ShuttingDown,
    InstallPending,
}

impl DesktopProfileEnumPhase {
    fn permits(self, requested: Self, before_shutdown: Option<Self>) -> bool {
        if self == requested {
            return true;
        }
        match self {
            Self::Starting => {
                matches!(requested, Self::Ready | Self::Recovery | Self::ShuttingDown)
            }
            Self::Ready | Self::Recovery => matches!(
                requested,
                Self::Ready | Self::Recovery | Self::ShuttingDown | Self::InstallPending
            ),
            Self::ShuttingDown => before_shutdown == Some(requested),
            Self::InstallPending => requested == Self::ShuttingDown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DesktopProfileOwnerRecordEnumAddress {
    UnixSocket(PathBuf),
    WindowsPipe(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopProfileOwnerRecordEndpoint {
    address: DesktopProfileOwnerRecordEnumAddress,
}

impl DesktopProfileOwnerRecordEndpoint {
    const UNIX_PATH_BYTES: usize = 103;
    const WINDOWS_PREFIX: &str = r"\\.\pipe\";
    const WINDOWS_NAME_BYTES: usize = 128;

    pub fn unix_socket(path: &Path) -> Result<Self, DesktopProfileOwnerRecordError> {
        let text = path
            .to_str()
            .ok_or(DesktopProfileOwnerRecordError::InvalidEndpoint)?;
        if !text.starts_with('/')
            || text.len() > Self::UNIX_PATH_BYTES
            || text.contains('\0')
            || text
                .split('/')
                .any(|component| component == ".." || component == ".")
            || text.ends_with('/')
        {
            return Err(DesktopProfileOwnerRecordError::InvalidEndpoint);
        }
        Ok(Self {
            address: DesktopProfileOwnerRecordEnumAddress::UnixSocket(path.to_path_buf()),
        })
    }

    pub fn windows_pipe(name: &str) -> Result<Self, DesktopProfileOwnerRecordError> {
        let suffix = name
            .strip_prefix(Self::WINDOWS_PREFIX)
            .filter(|suffix| !suffix.is_empty() && suffix.len() <= Self::WINDOWS_NAME_BYTES)
            .ok_or(DesktopProfileOwnerRecordError::InvalidEndpoint)?;
        if !suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(DesktopProfileOwnerRecordError::InvalidEndpoint);
        }
        Ok(Self {
            address: DesktopProfileOwnerRecordEnumAddress::WindowsPipe(name.to_owned()),
        })
    }

    pub fn unix_path(&self) -> Option<&Path> {
        match &self.address {
            DesktopProfileOwnerRecordEnumAddress::UnixSocket(path) => Some(path),
            DesktopProfileOwnerRecordEnumAddress::WindowsPipe(_) => None,
        }
    }

    pub fn windows_name(&self) -> Option<&str> {
        match &self.address {
            DesktopProfileOwnerRecordEnumAddress::UnixSocket(_) => None,
            DesktopProfileOwnerRecordEnumAddress::WindowsPipe(name) => Some(name),
        }
    }
}

pub trait DesktopProfileOwnerRecordBoundEndpoint {
    fn confirm_bound(
        &self,
        binding: DesktopProfileActivationBinding,
    ) -> Result<DesktopProfileOwnerRecordEndpoint, DesktopProfileOwnerRecordError>;
}

#[derive(Deserialize, Serialize)]
#[serde(
    tag = "transport",
    content = "address",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum DesktopProfileOwnerRecordEnumWireEndpoint {
    UnixSocket(PathBuf),
    WindowsPipe(String),
}

impl From<&DesktopProfileOwnerRecordEndpoint> for DesktopProfileOwnerRecordEnumWireEndpoint {
    fn from(endpoint: &DesktopProfileOwnerRecordEndpoint) -> Self {
        match &endpoint.address {
            DesktopProfileOwnerRecordEnumAddress::UnixSocket(path) => {
                Self::UnixSocket(path.clone())
            }
            DesktopProfileOwnerRecordEnumAddress::WindowsPipe(name) => {
                Self::WindowsPipe(name.clone())
            }
        }
    }
}

impl TryFrom<DesktopProfileOwnerRecordEnumWireEndpoint> for DesktopProfileOwnerRecordEndpoint {
    type Error = DesktopProfileOwnerRecordError;

    fn try_from(endpoint: DesktopProfileOwnerRecordEnumWireEndpoint) -> Result<Self, Self::Error> {
        match endpoint {
            DesktopProfileOwnerRecordEnumWireEndpoint::UnixSocket(path) => Self::unix_socket(&path),
            DesktopProfileOwnerRecordEnumWireEndpoint::WindowsPipe(name) => {
                Self::windows_pipe(&name)
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DesktopProfileOwnerRecordWire {
    profile_id: String,
    owner_generation: String,
    endpoint: DesktopProfileOwnerRecordEnumWireEndpoint,
    diagnostic_pid: NonZeroU32,
    phase: DesktopProfileEnumPhase,
}

impl From<&DesktopProfileOwnerRecord> for DesktopProfileOwnerRecordWire {
    fn from(record: &DesktopProfileOwnerRecord) -> Self {
        Self {
            profile_id: record.binding.profile().to_string(),
            owner_generation: String::from(record.binding.generation()),
            endpoint: DesktopProfileOwnerRecordEnumWireEndpoint::from(&record.endpoint),
            diagnostic_pid: record.diagnostic_pid,
            phase: record.phase,
        }
    }
}

#[derive(Debug)]
pub struct DesktopProfileOwnerRecord {
    binding: DesktopProfileActivationBinding,
    endpoint: DesktopProfileOwnerRecordEndpoint,
    diagnostic_pid: NonZeroU32,
    phase: DesktopProfileEnumPhase,
}

impl DesktopProfileOwnerRecord {
    pub fn read(
        data: &Path,
        security: &DesktopProfileSecurity,
    ) -> Result<Option<Self>, DesktopProfileOwnerRecordError> {
        DesktopProfileControlRecord::read(
            data,
            security,
            DesktopProfileControlRecordEnumKind::Owner,
        )?
        .map(Self::from_wire)
        .transpose()
    }

    pub fn binding(&self) -> DesktopProfileActivationBinding {
        self.binding
    }

    pub fn endpoint(&self) -> &DesktopProfileOwnerRecordEndpoint {
        &self.endpoint
    }

    pub fn diagnostic_pid(&self) -> NonZeroU32 {
        self.diagnostic_pid
    }

    pub fn phase(&self) -> DesktopProfileEnumPhase {
        self.phase
    }

    fn from_wire(
        wire: DesktopProfileOwnerRecordWire,
    ) -> Result<Self, DesktopProfileOwnerRecordError> {
        Ok(Self {
            binding: DesktopProfileActivationBinding::new(
                DesktopProfileId::from_str(&wire.profile_id)?,
                DesktopRequestId::from_str(&wire.owner_generation)?,
            ),
            endpoint: DesktopProfileOwnerRecordEndpoint::try_from(wire.endpoint)?,
            diagnostic_pid: wire.diagnostic_pid,
            phase: wire.phase,
        })
    }
}

pub struct DesktopProfileOwnerRecordPublisher<'a> {
    lease: &'a DesktopProfileLease,
    record: DesktopProfileOwnerRecord,
    before_shutdown: Option<DesktopProfileEnumPhase>,
}

impl<'a> DesktopProfileOwnerRecordPublisher<'a> {
    pub fn publish(
        lease: &'a DesktopProfileLease,
        binding: DesktopProfileActivationBinding,
        bound_endpoint: &impl DesktopProfileOwnerRecordBoundEndpoint,
        diagnostic_pid: NonZeroU32,
    ) -> Result<Self, DesktopProfileOwnerRecordError> {
        if binding.profile() != lease.identity().profile_id() {
            return Err(DesktopProfileOwnerRecordError::BindingMismatch);
        }
        lease.verify_identity()?;
        let record = DesktopProfileOwnerRecord {
            binding,
            endpoint: bound_endpoint.confirm_bound(binding)?,
            diagnostic_pid,
            phase: DesktopProfileEnumPhase::Starting,
        };
        DesktopProfileControlRecord::replace(
            lease,
            DesktopProfileControlRecordEnumKind::Owner,
            &DesktopProfileOwnerRecordWire::from(&record),
        )?;
        Ok(Self {
            lease,
            record,
            before_shutdown: None,
        })
    }

    pub fn record(&self) -> &DesktopProfileOwnerRecord {
        &self.record
    }

    pub fn transition(
        &mut self,
        requested: DesktopProfileEnumPhase,
    ) -> Result<(), DesktopProfileOwnerRecordError> {
        let current = self.record.phase;
        if !current.permits(requested, self.before_shutdown) {
            return Err(DesktopProfileOwnerRecordError::InvalidPhaseTransition {
                current,
                requested,
            });
        }
        if current == requested {
            return Ok(());
        }
        let wire = DesktopProfileOwnerRecordWire {
            phase: requested,
            ..DesktopProfileOwnerRecordWire::from(&self.record)
        };
        DesktopProfileControlRecord::replace(
            self.lease,
            DesktopProfileControlRecordEnumKind::Owner,
            &wire,
        )?;
        if requested == DesktopProfileEnumPhase::ShuttingDown {
            self.before_shutdown = Some(current);
        } else if current == DesktopProfileEnumPhase::ShuttingDown {
            self.before_shutdown = None;
        }
        self.record.phase = requested;
        Ok(())
    }
}
