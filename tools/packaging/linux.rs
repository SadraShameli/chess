use std::path::PathBuf;

use crate::packaging::plan::{
    ToolsPackagingEnumPlatform, ToolsPackagingError, ToolsPackagingPlan, ToolsPackagingStep,
};

pub(crate) struct ToolsPackagingLinux;

impl ToolsPackagingLinux {
    pub(crate) const TOOLCHAIN_VARIABLE: &str = "CHESS_APPIMAGE_TOOL_TAG";
    pub(crate) const PREREQUISITES: &str = "Ubuntu 22.04 or newer on x86-64 with working Vulkan graphics drivers (libvulkan1), X11 or Wayland, and xdg-desktop-portal for native dialogs. Prepare offline machines by downloading the .deb dependencies listed in packager.toml on a connected machine of the same release.";

    pub(crate) fn validate_toolchain_tag(tag: &str) -> Result<(), ToolsPackagingError> {
        let lowered = tag.to_lowercase();
        let rolling = ["continuous", "latest", "nightly", "main", "master"];
        let prerelease = ["alpha", "beta", "rc", "pre", "dev"];
        if tag.is_empty()
            || rolling.iter().any(|marker| lowered.contains(marker))
            || prerelease.iter().any(|marker| lowered.contains(marker))
        {
            return Err(ToolsPackagingError::AppImageToolchain(format!(
                "'{tag}' is a rolling or prerelease tag"
            )));
        }
        let version = lowered.strip_prefix('v').unwrap_or(&lowered);
        if version.is_empty()
            || !version
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(ToolsPackagingError::AppImageToolchain(format!(
                "'{tag}' is not a stable numeric release tag"
            )));
        }
        Ok(())
    }

    pub(crate) fn plan(toolchain_tag: Option<&str>) -> Result<ToolsPackagingPlan, ToolsPackagingError> {
        let tag = toolchain_tag.ok_or_else(|| {
            ToolsPackagingError::AppImageToolchain(format!(
                "set {} to a maintained stable release tag before packaging",
                Self::TOOLCHAIN_VARIABLE
            ))
        })?;
        Self::validate_toolchain_tag(tag)?;
        Ok(ToolsPackagingPlan {
            platform: ToolsPackagingEnumPlatform::Linux,
            steps: vec![
                ToolsPackagingStep::new(
                    "Build the optimized native application",
                    "cargo",
                    &[
                        "build",
                        "--release",
                        "--locked",
                        "--bin",
                        "chess",
                        "--target",
                        "x86_64-unknown-linux-gnu",
                    ],
                ),
                ToolsPackagingStep::new(
                    "Assemble the .deb and AppImage",
                    "cargo",
                    &[
                        "packager",
                        "--release",
                        "--config",
                        "packager.toml",
                        "--formats",
                        "deb,appimage",
                    ],
                ),
            ],
            artifacts: vec![PathBuf::from("target/packager")],
            guidance: Self::PREREQUISITES,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ToolsPackagingLinux;

    #[test]
    fn only_stable_numeric_appimage_toolchain_tags_are_accepted() {
        for tag in ["13", "v1.2.3", "2.0"] {
            assert!(ToolsPackagingLinux::validate_toolchain_tag(tag).is_ok(), "{tag}");
        }
        for tag in ["continuous", "latest", "v1.0-rc1", "1.0.0-beta", "", "abc"] {
            assert!(ToolsPackagingLinux::validate_toolchain_tag(tag).is_err(), "{tag}");
        }
        assert!(ToolsPackagingLinux::plan(None).is_err());
        assert!(ToolsPackagingLinux::plan(Some("13")).is_ok());
    }
}
