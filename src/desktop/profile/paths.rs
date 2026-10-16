use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(super) struct DesktopProfilePaths {
    data: PathBuf,
    control: PathBuf,
    lock: PathBuf,
    identity: PathBuf,
}

impl DesktopProfilePaths {
    pub(super) const APPLICATION_ID: &str = "nl.sadra.chess";
    pub(super) const CONTROL_NAME: &str = "control";
    pub(super) const LOCK_NAME: &str = "owner.lock";
    pub(super) const IDENTITY_NAME: &str = "profile.id";
    pub(super) const IDENTITY_BYTES: u64 = 36;

    pub(super) fn new(data: PathBuf) -> Self {
        let control = data.join(Self::CONTROL_NAME);
        let lock = control.join(Self::LOCK_NAME);
        let identity = control.join(Self::IDENTITY_NAME);
        Self {
            data,
            control,
            lock,
            identity,
        }
    }

    pub(super) fn data(&self) -> &Path {
        &self.data
    }

    pub(super) fn control(&self) -> &Path {
        &self.control
    }

    pub(super) fn lock(&self) -> &Path {
        &self.lock
    }

    pub(super) fn identity(&self) -> &Path {
        &self.identity
    }
}
