use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use same_file::Handle;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::desktop::profile::directory::DesktopProfileDirectory;
use crate::desktop::profile::error::DesktopProfileError;
use crate::desktop::profile::lease::DesktopProfileLease;
use crate::desktop::profile::paths::DesktopProfilePaths;
use crate::desktop::profile::security::DesktopProfileSecurity;

#[derive(Clone, Copy)]
pub(super) enum DesktopProfileControlRecordEnumKind {
    Identity,
    Owner,
    Installation,
}

impl DesktopProfileControlRecordEnumKind {
    fn filename(self) -> &'static str {
        match self {
            Self::Identity => DesktopProfilePaths::IDENTITY_NAME,
            Self::Owner => "owner.json",
            Self::Installation => "installation.json",
        }
    }

    fn maximum_bytes(self) -> u64 {
        match self {
            Self::Identity => DesktopProfilePaths::IDENTITY_BYTES,
            Self::Owner => 4096,
            Self::Installation => 8192,
        }
    }

    fn path(self, data: &Path) -> PathBuf {
        data.join(DesktopProfilePaths::CONTROL_NAME)
            .join(self.filename())
    }

    fn check_bound(self, path: &Path, bytes: &[u8]) -> Result<(), DesktopProfileError> {
        if u64::try_from(bytes.len()).is_ok_and(|length| length <= self.maximum_bytes()) {
            Ok(())
        } else {
            Err(DesktopProfileError::ControlRecordTooLarge {
                path: path.to_path_buf(),
                maximum: self.maximum_bytes(),
            })
        }
    }
}

#[derive(Deserialize)]
struct DesktopProfileControlRecordWireHeader {
    version: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DesktopProfileControlRecordWire<T> {
    version: u32,
    record: T,
}

pub(super) struct DesktopProfileControlRecord;

impl DesktopProfileControlRecord {
    const VERSION: u32 = 1;

    pub(super) fn read<T: DeserializeOwned>(
        data: &Path,
        security: &DesktopProfileSecurity,
        kind: DesktopProfileControlRecordEnumKind,
    ) -> Result<Option<T>, DesktopProfileError> {
        Self::read_bytes(data, security, kind)?
            .map(|(path, bytes)| Self::decode(&path, &bytes))
            .transpose()
    }

    pub(super) fn read_bytes(
        data: &Path,
        security: &DesktopProfileSecurity,
        kind: DesktopProfileControlRecordEnumKind,
    ) -> Result<Option<(PathBuf, Vec<u8>)>, DesktopProfileError> {
        let canonical = match data.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(DesktopProfileError::io(data, error)),
        };
        let data = DesktopProfileDirectory::new(
            &canonical,
            security.open_data_directory(&canonical)?,
            security,
        )?;
        let control_path = canonical.join(DesktopProfilePaths::CONTROL_NAME);
        let control_file = match security.open_data_directory(&control_path) {
            Ok(file) => file,
            Err(DesktopProfileError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                data.verify_identity(security)?;
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        security.validate_control_directory(&control_file, &control_path)?;
        let control = DesktopProfileDirectory::new(&control_path, control_file, security)?;
        let path = kind.path(&canonical);
        let file = match security.open_existing_control_file(control.file(), &path) {
            Ok(file) => file,
            Err(DesktopProfileError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                data.verify_identity(security)?;
                control.verify_identity(security)?;
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(kind.maximum_bytes() + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| DesktopProfileError::io(&path, error))?;
        data.verify_identity(security)?;
        control.verify_identity(security)?;
        kind.check_bound(&path, &bytes)?;
        Ok(Some((path, bytes)))
    }

    pub(super) fn replace<T: Serialize>(
        lease: &DesktopProfileLease,
        kind: DesktopProfileControlRecordEnumKind,
        record: &T,
    ) -> Result<(), DesktopProfileError> {
        let path = kind.path(lease.identity().data_directory());
        let bytes = serde_json::to_vec(&DesktopProfileControlRecordWire {
            version: Self::VERSION,
            record,
        })
        .map_err(|source| DesktopProfileError::InvalidControlRecord {
            path: path.clone(),
            source,
        })?;
        kind.check_bound(&path, &bytes)?;
        lease.verify_identity()?;
        let security = lease.security();
        let control = security.open_data_directory(lease.identity().control_directory())?;
        security.validate_control_directory(&control, lease.identity().control_directory())?;
        match security.open_existing_control_file(&control, &path) {
            Ok(file) => drop(file),
            Err(DesktopProfileError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut temporary = NamedTempFile::new_in(lease.identity().control_directory())
            .map_err(|error| DesktopProfileError::io(&path, error))?;
        let temporary_path = temporary.path().to_path_buf();
        security.protect_created_file(temporary.as_file_mut(), &temporary_path)?;
        temporary
            .write_all(&bytes)
            .map_err(|error| DesktopProfileError::io(&path, error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| DesktopProfileError::io(&path, error))?;
        lease.verify_identity()?;
        let published = Self::persist(temporary, &control, &path)?;
        security.validate_control_file(&published, &path)?;
        let actual = security.open_existing_control_file(&control, &path)?;
        if Handle::from_file(published).map_err(|error| DesktopProfileError::io(&path, error))?
            != Handle::from_file(actual).map_err(|error| DesktopProfileError::io(&path, error))?
        {
            return Err(DesktopProfileError::IdentityChanged { path });
        }
        lease.verify_identity()
    }

    fn decode<T: DeserializeOwned>(path: &Path, bytes: &[u8]) -> Result<T, DesktopProfileError> {
        let header: DesktopProfileControlRecordWireHeader =
            serde_json::from_slice(bytes).map_err(|source| {
                DesktopProfileError::InvalidControlRecord {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
        if header.version != Self::VERSION {
            return Err(DesktopProfileError::UnsupportedControlRecordVersion {
                path: path.to_path_buf(),
                received: header.version,
            });
        }
        let envelope: DesktopProfileControlRecordWire<T> =
            serde_json::from_slice(bytes).map_err(|source| {
                DesktopProfileError::InvalidControlRecord {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
        Ok(envelope.record)
    }

    #[cfg(unix)]
    fn persist(
        temporary: NamedTempFile,
        control: &File,
        path: &Path,
    ) -> Result<File, DesktopProfileError> {
        let (published, temporary_path) = temporary.into_parts();
        rustix::fs::renameat(
            control,
            Self::filename_in_control(&temporary_path)?,
            control,
            Self::filename_in_control(path)?,
        )
        .map_err(|error| DesktopProfileError::io(path, error.into()))?;
        control
            .sync_all()
            .map_err(|error| DesktopProfileError::io(path, error))?;
        Ok(published)
    }

    #[cfg(unix)]
    fn filename_in_control(path: &Path) -> Result<&std::ffi::OsStr, DesktopProfileError> {
        path.file_name()
            .ok_or_else(|| DesktopProfileError::UnsafeIdentity {
                path: path.to_path_buf(),
            })
    }

    #[cfg(windows)]
    fn persist(
        temporary: NamedTempFile,
        _control: &File,
        path: &Path,
    ) -> Result<File, DesktopProfileError> {
        let (file, temporary_path) = temporary.into_parts();
        let source = Self::windows_path(&temporary_path)?;
        let destination = Self::windows_path(path)?;
        winsafe::SetFileAttributes(source, winsafe::co::FILE_ATTRIBUTE::NORMAL)
            .map_err(|error| DesktopProfileError::io(path, std::io::Error::other(error)))?;
        winsafe::MoveFileEx(
            source,
            Some(destination),
            winsafe::co::MOVEFILE::REPLACE_EXISTING | winsafe::co::MOVEFILE::WRITE_THROUGH,
        )
        .map_err(|error| DesktopProfileError::io(path, std::io::Error::other(error)))?;
        Ok(file)
    }

    #[cfg(windows)]
    fn windows_path(path: &Path) -> Result<&str, DesktopProfileError> {
        path.to_str()
            .filter(|value| !value.contains('\0'))
            .ok_or_else(|| DesktopProfileError::UnsafeIdentity {
                path: path.to_path_buf(),
            })
    }
}
