use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;

use crate::desktop::native::update::{DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError};

pub struct DesktopNativeUpdateLinux;

impl DesktopNativeUpdateLinux {
    pub fn validate_appimage(path: &Path) -> Result<PathBuf, DesktopNativeUpdateError> {
        let unsafe_path = || DesktopNativeUpdateError::UnsafePath(path.to_path_buf());
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || !path.is_absolute() {
            return Err(unsafe_path());
        }
        if path.components().any(|part| part.as_os_str() == "..") {
            return Err(unsafe_path());
        }
        if path.canonicalize()? != path {
            return Err(unsafe_path());
        }
        if path.extension().is_none_or(|extension| extension != "AppImage") {
            return Err(unsafe_path());
        }
        Ok(path.to_path_buf())
    }

    pub fn install_appimage(
        package: &Path,
        appimage: &Path,
    ) -> Result<DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError> {
        let destination = Self::validate_appimage(appimage)?;
        let parent = destination
            .parent()
            .ok_or_else(|| DesktopNativeUpdateError::UnsafePath(destination.clone()))?;
        let staged = parent.join(format!(".chess-update-{}.AppImage", std::process::id()));
        fs::copy(package, &staged)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
        }
        fs::File::open(&staged)?.sync_all()?;
        if let Err(error) = fs::rename(&staged, &destination) {
            fs::remove_file(&staged)?;
            return Err(error.into());
        }
        Ok(DesktopNativeUpdateEnumOutcome::Installed)
    }

    pub fn install_deb(
        package: &Path,
        version: &Version,
    ) -> Result<DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError> {
        if !package.is_absolute() || package.to_str().is_none_or(|text| text.starts_with('-')) {
            return Err(DesktopNativeUpdateError::UnsafePath(package.to_path_buf()));
        }
        let status = Command::new("pkexec")
            .arg("dpkg")
            .arg("--install")
            .arg(package)
            .status()?;
        if !status.success() {
            return Err(DesktopNativeUpdateError::Command {
                program: "pkexec dpkg",
                status: status.code(),
            });
        }
        let installed = Command::new("dpkg-query")
            .arg("--show")
            .arg("--showformat=${Version}")
            .arg("chess")
            .output()?;
        if installed.status.success()
            && String::from_utf8_lossy(&installed.stdout).trim() == version.to_string()
        {
            Ok(DesktopNativeUpdateEnumOutcome::Installed)
        } else {
            Ok(DesktopNativeUpdateEnumOutcome::UnobservableCompletion)
        }
    }
}
