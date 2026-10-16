use std::fs::File;
use std::path::{Path, PathBuf};

use same_file::Handle;

use crate::desktop::profile::error::DesktopProfileError;
use crate::desktop::profile::security::DesktopProfileSecurity;

#[derive(Debug)]
pub(super) struct DesktopProfileDirectory {
    path: PathBuf,
    handle: Handle,
}

impl DesktopProfileDirectory {
    pub(super) fn new(
        path: &Path,
        file: File,
        security: &DesktopProfileSecurity,
    ) -> Result<Self, DesktopProfileError> {
        let handle =
            Handle::from_file(file).map_err(|error| DesktopProfileError::io(path, error))?;
        let directory = Self {
            path: path.to_path_buf(),
            handle,
        };
        directory.verify_identity(security)?;
        Ok(directory)
    }

    pub(super) fn file(&self) -> &File {
        self.handle.as_file()
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn verify_identity(
        &self,
        security: &DesktopProfileSecurity,
    ) -> Result<(), DesktopProfileError> {
        let current_file = security.open_data_directory(&self.path)?;
        let current = Handle::from_file(current_file)
            .map_err(|error| DesktopProfileError::io(&self.path, error))?;
        if self.handle != current {
            return Err(DesktopProfileError::IdentityChanged {
                path: self.path.clone(),
            });
        }
        Ok(())
    }
}
