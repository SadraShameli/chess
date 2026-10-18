use std::fs::TryLockError;
use std::io;
use std::path::Path;

use crate::desktop::profile::directory::DesktopProfileDirectory;
use crate::desktop::profile::error::DesktopProfileError;
use crate::desktop::profile::identity::DesktopProfileId;
use crate::desktop::profile::lease::{DesktopProfileLease, DesktopProfileLeaseParts};
use crate::desktop::profile::lock::DesktopProfileLockProvider;
use crate::desktop::profile::paths::DesktopProfilePaths;
use crate::desktop::profile::security::DesktopProfileSecurity;

#[derive(Debug)]
pub enum DesktopProfileEnumAdmission {
    Owned(Box<DesktopProfileLease>),
    Busy,
}

#[derive(Debug)]
pub struct DesktopProfileAdmission<L> {
    security: DesktopProfileSecurity,
    lock_provider: L,
}

impl<L: DesktopProfileLockProvider> DesktopProfileAdmission<L> {
    pub fn new(security: DesktopProfileSecurity, lock_provider: L) -> Self {
        Self {
            security,
            lock_provider,
        }
    }

    pub fn acquire(
        &self,
        data_directory: &Path,
        proposed_profile_id: DesktopProfileId,
    ) -> Result<DesktopProfileEnumAdmission, DesktopProfileError> {
        let canonical = self.security.prepare_data_directory(data_directory)?;
        let paths = DesktopProfilePaths::new(canonical);
        let data_file = self.security.open_data_directory(paths.data())?;
        let data = DesktopProfileDirectory::new(paths.data(), data_file, &self.security)?;
        let control_file = self.security.open_control_directory(data.file(), &paths)?;
        let control = DesktopProfileDirectory::new(paths.control(), control_file, &self.security)?;
        let lock = self.security.open_lock_file(control.file(), &paths)?;
        match self.lock_provider.try_lock(&lock) {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(DesktopProfileEnumAdmission::Busy),
            Err(TryLockError::Error(source)) => {
                return Err(if source.kind() == io::ErrorKind::Unsupported {
                    DesktopProfileError::LockUnsupported { source }
                } else {
                    DesktopProfileError::LockUnavailable { source }
                });
            }
        }
        let lease = DesktopProfileLease::new(
            DesktopProfileLeaseParts {
                paths,
                data,
                control,
                lock,
                security: self.security.clone(),
            },
            proposed_profile_id,
        )?;
        Ok(DesktopProfileEnumAdmission::Owned(Box::new(lease)))
    }
}
