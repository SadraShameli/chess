use std::num::NonZeroU32;
use std::str::FromStr;

use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::desktop::error::DesktopError;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::profile::control_record::{
    DesktopProfileControlRecord, DesktopProfileControlRecordEnumKind,
};
use crate::desktop::profile::{DesktopProfileError, DesktopProfileId, DesktopProfileLease};
use crate::package::digest::{PackageDigest, PackageDigestError};

#[derive(Debug, Error)]
pub enum DesktopProfileInstallationError {
    #[error(transparent)]
    Profile(#[from] DesktopProfileError),
    #[error("The installation intent identity is invalid: {0}")]
    Identity(#[from] DesktopError),
    #[error("The target package digest is invalid: {0}")]
    Digest(#[from] PackageDigestError),
    #[error("The target package version is invalid: {0}")]
    Version(#[from] semver::Error),
    #[error("The installation intent targets another profile")]
    ProfileMismatch,
    #[error("The installer helper must have its own distinct generation")]
    HelperGenerationMismatch,
    #[error("The target package must have a canonical stable release version")]
    NonstableVersion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopProfileInstallationEnumOutcome {
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum DesktopProfileInstallationEnumState {
    Prepared,
    Committed,
    Outcome {
        result: DesktopProfileInstallationEnumOutcome,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesktopProfileInstallationWireIntent {
    profile_id: String,
    owner_generation: String,
    helper_generation: String,
    package_digest: String,
    package_version: String,
    safety_backup_id: String,
    data_schema: NonZeroU32,
    state: DesktopProfileInstallationEnumState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopProfileInstallationSpec {
    pub owner_generation: DesktopRequestId,
    pub helper_generation: DesktopRequestId,
    pub package_digest: PackageDigest,
    pub package_version: Version,
    pub safety_backup_id: DesktopRequestId,
    pub data_schema: NonZeroU32,
}

#[derive(Debug)]
pub struct DesktopProfileInstallationIntent {
    profile_id: DesktopProfileId,
    owner_generation: DesktopRequestId,
    helper_generation: DesktopRequestId,
    package_digest: PackageDigest,
    package_version: Version,
    safety_backup_id: DesktopRequestId,
    data_schema: NonZeroU32,
    state: DesktopProfileInstallationEnumState,
}

#[derive(Debug)]
pub enum DesktopProfileInstallationEnumStartup {
    Normal,
    Blocked(DesktopProfileInstallationIntent),
}

impl DesktopProfileInstallationIntent {
    pub fn inspect(
        lease: &DesktopProfileLease,
    ) -> Result<DesktopProfileInstallationEnumStartup, DesktopProfileInstallationError> {
        lease.verify_identity()?;
        let wire: Option<DesktopProfileInstallationWireIntent> = DesktopProfileControlRecord::read(
            lease.identity().data_directory(),
            lease.security(),
            DesktopProfileControlRecordEnumKind::Installation,
        )?;
        lease.verify_identity()?;
        match wire {
            None => Ok(DesktopProfileInstallationEnumStartup::Normal),
            Some(wire) => {
                let intent = Self::from_wire(wire)?;
                if intent.profile_id != lease.identity().profile_id() {
                    return Err(DesktopProfileInstallationError::ProfileMismatch);
                }
                Ok(DesktopProfileInstallationEnumStartup::Blocked(intent))
            }
        }
    }

    pub fn read_current(
        lease: &DesktopProfileLease,
    ) -> Result<Option<Self>, DesktopProfileInstallationError> {
        match Self::inspect(lease)? {
            DesktopProfileInstallationEnumStartup::Normal => Ok(None),
            DesktopProfileInstallationEnumStartup::Blocked(intent) => Ok(Some(intent)),
        }
    }

    pub fn write(
        lease: &DesktopProfileLease,
        spec: &DesktopProfileInstallationSpec,
        state: DesktopProfileInstallationEnumState,
    ) -> Result<(), DesktopProfileInstallationError> {
        if spec.owner_generation == spec.helper_generation {
            return Err(DesktopProfileInstallationError::HelperGenerationMismatch);
        }
        if !spec.package_version.pre.is_empty() || !spec.package_version.build.is_empty() {
            return Err(DesktopProfileInstallationError::NonstableVersion);
        }
        let wire = DesktopProfileInstallationWireIntent {
            profile_id: lease.identity().profile_id().to_string(),
            owner_generation: String::from(spec.owner_generation),
            helper_generation: String::from(spec.helper_generation),
            package_digest: String::from(spec.package_digest),
            package_version: spec.package_version.to_string(),
            safety_backup_id: String::from(spec.safety_backup_id),
            data_schema: spec.data_schema,
            state,
        };
        DesktopProfileControlRecord::replace(
            lease,
            DesktopProfileControlRecordEnumKind::Installation,
            &wire,
        )?;
        Ok(())
    }

    pub fn clear(lease: &DesktopProfileLease) -> Result<(), DesktopProfileInstallationError> {
        DesktopProfileControlRecord::remove(
            lease,
            DesktopProfileControlRecordEnumKind::Installation,
        )?;
        Ok(())
    }

    pub fn spec(&self) -> DesktopProfileInstallationSpec {
        DesktopProfileInstallationSpec {
            owner_generation: self.owner_generation,
            helper_generation: self.helper_generation,
            package_digest: self.package_digest,
            package_version: self.package_version.clone(),
            safety_backup_id: self.safety_backup_id,
            data_schema: self.data_schema,
        }
    }

    pub fn profile_id(&self) -> DesktopProfileId {
        self.profile_id
    }

    pub fn owner_generation(&self) -> DesktopRequestId {
        self.owner_generation
    }

    pub fn helper_generation(&self) -> DesktopRequestId {
        self.helper_generation
    }

    pub fn package_digest(&self) -> PackageDigest {
        self.package_digest
    }

    pub fn package_version(&self) -> &Version {
        &self.package_version
    }

    pub fn safety_backup_id(&self) -> DesktopRequestId {
        self.safety_backup_id
    }

    pub fn data_schema(&self) -> NonZeroU32 {
        self.data_schema
    }

    pub fn state(&self) -> DesktopProfileInstallationEnumState {
        self.state
    }

    fn from_wire(
        wire: DesktopProfileInstallationWireIntent,
    ) -> Result<Self, DesktopProfileInstallationError> {
        let owner_generation = DesktopRequestId::from_str(&wire.owner_generation)?;
        let helper_generation = DesktopRequestId::from_str(&wire.helper_generation)?;
        if owner_generation == helper_generation {
            return Err(DesktopProfileInstallationError::HelperGenerationMismatch);
        }
        let package_version = Version::parse(&wire.package_version)?;
        if !package_version.pre.is_empty()
            || !package_version.build.is_empty()
            || package_version.to_string() != wire.package_version
        {
            return Err(DesktopProfileInstallationError::NonstableVersion);
        }
        Ok(Self {
            profile_id: DesktopProfileId::from_str(&wire.profile_id)?,
            owner_generation,
            helper_generation,
            package_digest: PackageDigest::from_str(&wire.package_digest)?,
            package_version,
            safety_backup_id: DesktopRequestId::from_str(&wire.safety_backup_id)?,
            data_schema: wire.data_schema,
            state: wire.state,
        })
    }
}
