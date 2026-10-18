use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use directories::BaseDirs;
use uuid::Uuid;

use crate::desktop::profile::control_record::{
    DesktopProfileControlRecord, DesktopProfileControlRecordEnumKind,
};
use crate::desktop::profile::error::DesktopProfileError;
use crate::desktop::profile::paths::DesktopProfilePaths;
use crate::desktop::profile::security::DesktopProfileSecurity;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DesktopProfileId(Uuid);

impl DesktopProfileId {
    pub fn from_uuid(value: Uuid) -> Result<Self, DesktopProfileError> {
        if value.is_nil() {
            return Err(DesktopProfileError::NoncanonicalProfileId);
        }
        Ok(Self(value))
    }

    pub(super) fn from_control_record(
        path: &Path,
        bytes: &[u8],
    ) -> Result<Self, DesktopProfileError> {
        let value = std::str::from_utf8(bytes)
            .map_err(|source| Self::invalid_control_record(path, source))?;
        value
            .parse()
            .map_err(|source| Self::invalid_control_record(path, source))
    }

    fn invalid_control_record(
        path: &Path,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> DesktopProfileError {
        DesktopProfileError::InvalidIdentityRecord {
            path: path.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
        }
    }
}

impl FromStr for DesktopProfileId {
    type Err = DesktopProfileError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parsed = Self::from_uuid(Uuid::parse_str(value)?)?;
        if parsed.to_string() != value {
            return Err(DesktopProfileError::NoncanonicalProfileId);
        }
        Ok(parsed)
    }
}

impl Display for DesktopProfileId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

#[derive(Debug)]
pub struct DesktopProfileIdentity {
    paths: DesktopProfilePaths,
    profile_id: DesktopProfileId,
}

impl DesktopProfileIdentity {
    pub fn read_existing(
        data_directory: &Path,
        security: &DesktopProfileSecurity,
    ) -> Result<Option<DesktopProfileId>, DesktopProfileError> {
        DesktopProfileControlRecord::read_bytes(
            data_directory,
            security,
            DesktopProfileControlRecordEnumKind::Identity,
        )?
        .map(|(path, bytes)| DesktopProfileId::from_control_record(&path, &bytes))
        .transpose()
    }

    pub fn discover_data_directory() -> Result<PathBuf, DesktopProfileError> {
        BaseDirs::new()
            .map(|base| Self::data_directory_in(base.data_dir()))
            .ok_or(DesktopProfileError::DataDirectoryUnavailable)
    }

    pub fn data_directory_in(base_data_directory: &Path) -> PathBuf {
        base_data_directory.join(DesktopProfilePaths::APPLICATION_ID)
    }

    pub fn data_directory(&self) -> &Path {
        self.paths.data()
    }

    pub fn control_directory(&self) -> &Path {
        self.paths.control()
    }

    pub fn lock_path(&self) -> &Path {
        self.paths.lock()
    }

    pub fn profile_id(&self) -> DesktopProfileId {
        self.profile_id
    }

    pub(super) fn new(paths: DesktopProfilePaths, profile_id: DesktopProfileId) -> Self {
        Self { paths, profile_id }
    }
}
