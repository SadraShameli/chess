use std::fs::{File, TryLockError};

pub trait DesktopProfileLockProvider {
    fn try_lock(&self, file: &File) -> Result<(), TryLockError>;
}

#[derive(Debug, Clone, Copy)]
pub struct DesktopProfileOsLock;

impl DesktopProfileLockProvider for DesktopProfileOsLock {
    fn try_lock(&self, file: &File) -> Result<(), TryLockError> {
        file.try_lock()
    }
}
