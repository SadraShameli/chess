use std::path::Path;
use std::process::Command;

use crate::desktop::native::update::{DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError};

pub struct DesktopNativeUpdateWindows;

impl DesktopNativeUpdateWindows {
    pub fn validate_destination(destination: &Path) -> Result<String, DesktopNativeUpdateError> {
        let text = destination
            .to_str()
            .ok_or_else(|| DesktopNativeUpdateError::UnsafePath(destination.to_path_buf()))?;
        if !destination.is_absolute()
            || text.contains('"')
            || text.chars().any(char::is_control)
            || text.split(['\\', '/']).any(|part| part == "..")
        {
            return Err(DesktopNativeUpdateError::UnsafePath(destination.to_path_buf()));
        }
        Ok(text.to_owned())
    }

    pub fn install(
        package: &Path,
        destination: &Path,
    ) -> Result<DesktopNativeUpdateEnumOutcome, DesktopNativeUpdateError> {
        if !package.is_absolute() {
            return Err(DesktopNativeUpdateError::UnsafePath(package.to_path_buf()));
        }
        let destination = Self::validate_destination(destination)?;
        let status = Command::new(package)
            .arg("/S")
            .arg(format!("/D={destination}"))
            .status()?;
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
