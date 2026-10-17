pub mod linux;
pub mod macos;
pub mod windows;

use std::path::{Path, PathBuf};

use semver::Version;
use thiserror::Error;

use crate::update::manifest::UpdateTarget;

#[derive(Debug, Error)]
pub enum DesktopNativeUpdateError {
    #[error("The installer I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("The installer command {program} failed with status {status:?}")]
    Command {
        program: &'static str,
        status: Option<i32>,
    },
    #[error("The destination path {0:?} is not safe")]
    UnsafePath(PathBuf),
    #[error("The staged package contains no application bundle")]
    MissingBundle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeUpdateEnumOutcome {
    Installed,
    UnobservableCompletion,
}

pub struct DesktopNativeUpdate;

impl DesktopNativeUpdate {
    pub fn install(
        target: UpdateTarget,
        package: &Path,
        destination: &Path,
        version: &Version,
    ) -> Result<DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError> {
        match target {
            UpdateTarget::WindowsX86_64 => windows::DesktopNativeUpdateWindows::install(package, destination),
            UpdateTarget::MacosArm64 => macos::DesktopNativeUpdateMacos::install(package, destination),
            UpdateTarget::LinuxAppImage => linux::DesktopNativeUpdateLinux::install_appimage(package, destination),
            UpdateTarget::LinuxDeb => linux::DesktopNativeUpdateLinux::install_deb(package, version),
        }
    }
}
