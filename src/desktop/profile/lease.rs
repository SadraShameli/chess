use std::fs::File;
use std::io::{Read, Seek, Write};

use same_file::Handle;
use tempfile::NamedTempFile;

use crate::desktop::profile::directory::DesktopProfileDirectory;
use crate::desktop::profile::error::DesktopProfileError;
use crate::desktop::profile::identity::{DesktopProfileId, DesktopProfileIdentity};
use crate::desktop::profile::paths::DesktopProfilePaths;
use crate::desktop::profile::security::DesktopProfileSecurity;

#[derive(Debug)]
pub struct DesktopProfileLease {
    lock: Handle,
    data: DesktopProfileDirectory,
    control: DesktopProfileDirectory,
    identity: DesktopProfileIdentity,
    security: DesktopProfileSecurity,
}

pub(super) struct DesktopProfileLeaseParts {
    pub(super) paths: DesktopProfilePaths,
    pub(super) data: DesktopProfileDirectory,
    pub(super) control: DesktopProfileDirectory,
    pub(super) lock: File,
    pub(super) security: DesktopProfileSecurity,
}

impl DesktopProfileLease {
    pub fn identity(&self) -> &DesktopProfileIdentity {
        &self.identity
    }

    pub(super) fn security(&self) -> &DesktopProfileSecurity {
        &self.security
    }

    pub fn verify_identity(&self) -> Result<(), DesktopProfileError> {
        self.data.verify_identity(&self.security)?;
        self.control.verify_identity(&self.security)?;
        self.security
            .validate_data_directory(self.data.file(), self.data.path())?;
        self.security
            .validate_control_directory(self.control.file(), self.control.path())?;
        self.security
            .validate_control_file(self.lock.as_file(), self.identity.lock_path())?;
        let named_file = self
            .security
            .open_existing_control_file(self.control.file(), self.identity.lock_path())?;
        let named_anchor = same_file::Handle::from_file(named_file)
            .map_err(|error| DesktopProfileError::io(self.identity.lock_path(), error))?;
        if self.lock != named_anchor {
            return Err(DesktopProfileError::IdentityChanged {
                path: self.identity.lock_path().to_path_buf(),
            });
        }
        Ok(())
    }

    pub(super) fn new(
        parts: DesktopProfileLeaseParts,
        proposed_profile_id: DesktopProfileId,
    ) -> Result<Self, DesktopProfileError> {
        let DesktopProfileLeaseParts {
            paths,
            data,
            control,
            lock,
            security,
        } = parts;
        data.verify_identity(&security)?;
        control.verify_identity(&security)?;
        let lock = Handle::from_file(lock)
            .map_err(|error| DesktopProfileError::io(paths.lock(), error))?;
        let profile_id = Self::load_or_create_id(&paths, &control, &security, proposed_profile_id)?;
        let lease = Self {
            lock,
            data,
            control,
            identity: DesktopProfileIdentity::new(paths, profile_id),
            security,
        };
        lease.verify_identity()?;
        Ok(lease)
    }

    fn load_or_create_id(
        paths: &DesktopProfilePaths,
        control: &DesktopProfileDirectory,
        security: &DesktopProfileSecurity,
        proposed_profile_id: DesktopProfileId,
    ) -> Result<DesktopProfileId, DesktopProfileError> {
        match security.open_existing_control_file(control.file(), paths.identity()) {
            Ok(file) => Self::read_id(file, paths),
            Err(DesktopProfileError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                let mut temporary = NamedTempFile::new_in(paths.control())
                    .map_err(|error| DesktopProfileError::io(paths.control(), error))?;
                let temporary_path = temporary.path().to_path_buf();
                security.protect_created_file(temporary.as_file_mut(), &temporary_path)?;
                temporary
                    .write_all(proposed_profile_id.to_string().as_bytes())
                    .map_err(|error| DesktopProfileError::io(paths.identity(), error))?;
                temporary
                    .as_file()
                    .sync_all()
                    .map_err(|error| DesktopProfileError::io(paths.identity(), error))?;
                let mut file =
                    security.persist_control_file(temporary, control.file(), paths.identity())?;
                security.validate_control_file(&file, paths.identity())?;
                file.rewind()
                    .map_err(|error| DesktopProfileError::io(paths.identity(), error))?;
                Self::read_id(file, paths)
            }
            Err(error) => Err(error),
        }
    }

    fn read_id(
        file: File,
        paths: &DesktopProfilePaths,
    ) -> Result<DesktopProfileId, DesktopProfileError> {
        let mut bytes = Vec::new();
        file.take(DesktopProfilePaths::IDENTITY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| DesktopProfileError::io(paths.identity(), error))?;
        let value = std::str::from_utf8(&bytes).map_err(|error| {
            DesktopProfileError::InvalidIdentityRecord {
                path: paths.identity().to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
            }
        })?;
        value.parse::<DesktopProfileId>().map_err(|error| {
            DesktopProfileError::InvalidIdentityRecord {
                path: paths.identity().to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
            }
        })
    }
}
