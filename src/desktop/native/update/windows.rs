use std::path::Path;
use std::process::Command;

use crate::desktop::native::update::{DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError};
use crate::package::digest::PackageDigest;

pub struct DesktopNativeUpdateWindows;

impl DesktopNativeUpdateWindows {
    #[cfg(windows)]
    const SHARE_READ_ONLY: u32 = 1;

    pub fn validate_destination(destination: &Path) -> Result<String, DesktopNativeUpdateError> {
        let text = destination
            .to_str()
            .ok_or_else(|| DesktopNativeUpdateError::UnsafePath(destination.to_path_buf()))?;
        if !destination.is_absolute()
            || text.contains('"')
            || text.chars().any(char::is_control)
            || text.split(['\\', '/']).any(|part| part == "..")
        {
            return Err(DesktopNativeUpdateError::UnsafePath(
                destination.to_path_buf(),
            ));
        }
        Ok(text.to_owned())
    }

    #[cfg(windows)]
    fn hold_verified(
        package: &Path,
        digest: PackageDigest,
    ) -> Result<std::fs::File, DesktopNativeUpdateError> {
        use std::os::windows::fs::OpenOptionsExt;

        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(Self::SHARE_READ_ONLY)
            .open(package)?;
        if !held.metadata()?.is_file() {
            return Err(DesktopNativeUpdateError::UnsafePath(package.to_path_buf()));
        }
        if PackageDigest::read(&held)? != digest {
            return Err(DesktopNativeUpdateError::DigestMismatch);
        }
        Ok(held)
    }

    #[cfg(not(windows))]
    fn hold_verified(
        package: &Path,
        digest: PackageDigest,
    ) -> Result<std::fs::File, DesktopNativeUpdateError> {
        let held = std::fs::File::open(package)?;
        if PackageDigest::read(&held)? != digest {
            return Err(DesktopNativeUpdateError::DigestMismatch);
        }
        Ok(held)
    }

    pub fn install(
        package: &Path,
        destination: &Path,
        digest: PackageDigest,
    ) -> Result<DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError> {
        if !package.is_absolute() {
            return Err(DesktopNativeUpdateError::UnsafePath(package.to_path_buf()));
        }
        let destination = Self::validate_destination(destination)?;
        let held = Self::hold_verified(package, digest)?;
        let status = Command::new(package)
            .arg("/S")
            .arg(format!("/D={destination}"))
            .status()?;
        drop(held);
        if status.success() {
            Ok(DesktopNativeUpdateEnumOutcome::Installed)
        } else {
            Err(DesktopNativeUpdateError::Command {
                program: "nsis installer",
                status: status.code(),
            })
        }
    }
}
