use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use crate::desktop::native::update::{DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError};

pub struct DesktopNativeUpdateMacos;

impl DesktopNativeUpdateMacos {
    fn single_bundle(staging: &Path) -> Result<PathBuf, DesktopNativeUpdateError> {
        let mut found = None;
        for entry in fs::read_dir(staging)? {
            let path = entry?.path();
            if path.extension().is_some_and(|extension| extension == "app") {
                if found.is_some() {
                    return Err(DesktopNativeUpdateError::MissingBundle);
                }
                found = Some(path);
            }
        }
        found.ok_or(DesktopNativeUpdateError::MissingBundle)
    }

    pub fn install(
        package: &Path,
        bundle: &Path,
    ) -> Result<DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError> {
        let parent = bundle
            .parent()
            .ok_or_else(|| DesktopNativeUpdateError::UnsafePath(bundle.to_path_buf()))?;
        if !bundle.is_absolute() || bundle.extension().is_none_or(|extension| extension != "app") {
            return Err(DesktopNativeUpdateError::UnsafePath(bundle.to_path_buf()));
        }
        let staging = parent.join(format!(".chess-update-{}", Uuid::new_v4()));
        fs::create_dir(&staging)?;
        let outcome = Self::replace(package, bundle, &staging);
        fs::remove_dir_all(&staging)?;
        outcome
    }

    fn replace(
        package: &Path,
        bundle: &Path,
        staging: &Path,
    ) -> Result<DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError> {
        let status = Command::new("/usr/bin/ditto")
            .arg("-x")
            .arg("-k")
            .arg(package)
            .arg(staging)
            .status()?;
        if !status.success() {
            return Err(DesktopNativeUpdateError::Command {
                program: "ditto",
                status: status.code(),
            });
        }
        let fresh = Self::single_bundle(staging)?;
        let retired = staging.join("previous.app");
        fs::rename(bundle, &retired)?;
        if let Err(error) = fs::rename(&fresh, bundle) {
            fs::rename(&retired, bundle)?;
            return Err(error.into());
        }
        Ok(DesktopNativeUpdateEnumOutcome::Installed)
    }
}
