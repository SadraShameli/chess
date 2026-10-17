use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;

use crate::desktop::native::update::{
    DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError, DesktopNativeUpdateStaging,
};
use crate::package::digest::PackageDigest;

pub struct DesktopNativeUpdateLinux;

impl DesktopNativeUpdateLinux {
    #[cfg(unix)]
    const ROOT_USER: u32 = 0;
    #[cfg(unix)]
    const GROUP_OTHER_WRITE: u32 = 0o022;
    const PRIVATE_TEMPORARY: &'static str = "/var/tmp";
    const DPKG: &'static str = "/usr/bin/dpkg";

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
        digest: PackageDigest,
    ) -> Result<DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError> {
        let destination = Self::validate_appimage(appimage)?;
        let parent = destination
            .parent()
            .ok_or_else(|| DesktopNativeUpdateError::UnsafePath(destination.clone()))?;
        let staged = parent.join(format!(".chess-update-{}.AppImage", uuid::Uuid::new_v4()));
        DesktopNativeUpdateStaging::copy_verified(package, digest, &staged)?;
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
        digest: PackageDigest,
    ) -> Result<DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError> {
        let helper = std::env::current_exe()?.canonicalize()?;
        Self::validate_privileged_helper(&helper)?;
        let plan = DesktopNativeUpdateLinuxDebPlan::new(&helper, package, digest)?;
        let status = Command::new(DesktopNativeUpdateLinuxDebPlan::PROGRAM)
            .args(plan.arguments())
            .status()?;
        if !status.success() {
            return Err(DesktopNativeUpdateError::Command {
                program: "pkexec privileged install",
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

    #[cfg(unix)]
    pub fn validate_privileged_helper(helper: &Path) -> Result<(), DesktopNativeUpdateError> {
        use std::os::unix::fs::MetadataExt;

        let unsafe_path = || DesktopNativeUpdateError::UnsafePath(helper.to_path_buf());
        if !helper.is_absolute() || helper.components().any(|part| part.as_os_str() == "..") {
            return Err(unsafe_path());
        }
        let metadata = fs::symlink_metadata(helper)?;
        if !metadata.is_file() || helper.canonicalize()? != helper {
            return Err(unsafe_path());
        }
        let trusted = |metadata: &fs::Metadata| {
            metadata.uid() == Self::ROOT_USER && metadata.mode() & Self::GROUP_OTHER_WRITE == 0
        };
        if !trusted(&metadata) {
            return Err(unsafe_path());
        }
        for ancestor in helper.ancestors().skip(1) {
            if !trusted(&fs::symlink_metadata(ancestor)?) {
                return Err(unsafe_path());
            }
        }
        Ok(())
    }

    #[cfg(not(unix))]
    pub fn validate_privileged_helper(helper: &Path) -> Result<(), DesktopNativeUpdateError> {
        Err(DesktopNativeUpdateError::UnsafePath(helper.to_path_buf()))
    }

    pub fn privileged_install_deb(
        package: &Path,
        digest: PackageDigest,
    ) -> Result<(), DesktopNativeUpdateError> {
        let directory = tempfile::Builder::new()
            .prefix("chess-update-")
            .tempdir_in(Self::PRIVATE_TEMPORARY)?;
        let copy = directory.path().join("package.deb");
        DesktopNativeUpdateStaging::copy_verified(package, digest, &copy)?;
        let status = Command::new(Self::DPKG).arg("--install").arg(&copy).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(DesktopNativeUpdateError::Command {
                program: "dpkg",
                status: status.code(),
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopNativeUpdateLinuxDebPlan {
    helper: PathBuf,
    package: PathBuf,
    digest: PackageDigest,
}

impl DesktopNativeUpdateLinuxDebPlan {
    pub const PROGRAM: &'static str = "pkexec";
    pub const ACTION: &'static str = "--privileged-install-deb";
    pub const PACKAGE_FLAG: &'static str = "--package";
    pub const DIGEST_FLAG: &'static str = "--sha256";

    pub fn new(
        helper: &Path,
        package: &Path,
        digest: PackageDigest,
    ) -> Result<Self, DesktopNativeUpdateError> {
        for path in [helper, package] {
            if !path.is_absolute()
                || path.components().any(|part| part.as_os_str() == "..")
                || path.to_str().is_none_or(|text| text.starts_with('-'))
            {
                return Err(DesktopNativeUpdateError::UnsafePath(path.to_path_buf()));
            }
        }
        Ok(Self {
            helper: helper.to_path_buf(),
            package: package.to_path_buf(),
            digest,
        })
    }

    pub fn arguments(&self) -> Vec<OsString> {
        vec![
            self.helper.clone().into_os_string(),
            OsString::from(Self::ACTION),
            OsString::from(Self::PACKAGE_FLAG),
            self.package.clone().into_os_string(),
            OsString::from(Self::DIGEST_FLAG),
            OsString::from(self.digest.to_string()),
        ]
    }
}
