mod linux;
mod macos;
mod notices;
mod plan;
mod verify;
mod windows;

use std::path::{Path, PathBuf};

pub(crate) use plan::{ToolsPackagingEnumPlatform, ToolsPackagingError};

pub(crate) enum ToolsPackagingEnumCommand {
    Plan(ToolsPackagingEnumPlatform),
    Run(ToolsPackagingEnumPlatform),
    Verify(ToolsPackagingEnumPlatform, PathBuf),
    Notices,
}

impl ToolsPackagingEnumCommand {
    pub(crate) fn parse(arguments: &[std::ffi::OsString]) -> Option<Result<Self, ToolsPackagingError>> {
        let text: Vec<&str> = arguments.iter().filter_map(|argument| argument.to_str()).collect();
        if text.len() != arguments.len() {
            return None;
        }
        let platform = |name: &str| {
            name.parse::<ToolsPackagingEnumPlatform>()
                .map_err(|_unknown| ToolsPackagingError::Usage)
        };
        match text.as_slice() {
            ["package-plan", name] => Some(platform(name).map(Self::Plan)),
            ["package-run", name] => Some(platform(name).map(Self::Run)),
            ["package-verify", name, directory] => {
                Some(platform(name).map(|platform| Self::Verify(platform, PathBuf::from(directory))))
            }
            ["notices-check"] => Some(Ok(Self::Notices)),
            _ => None,
        }
    }

    pub(crate) fn execute(self, root: &Path) -> Result<String, ToolsPackagingError> {
        match self {
            Self::Plan(platform) => Ok(Self::plan(platform)?.describe()),
            Self::Run(platform) => {
                let plan = Self::plan(platform)?;
                plan.run()?;
                Ok(plan.describe())
            }
            Self::Verify(platform, directory) => {
                let report = verify::ToolsPackagingVerify::verify(platform, &directory)?;
                Ok(format!(
                    "Package verified. Warnings:\n{}",
                    report.warnings.join("\n")
                ))
            }
            Self::Notices => {
                let warnings = notices::ToolsPackagingNotices::check(root)?;
                Ok(format!("Notices verified. Warnings:\n{}", warnings.join("\n")))
            }
        }
    }

    fn plan(
        platform: ToolsPackagingEnumPlatform,
    ) -> Result<plan::ToolsPackagingPlan, ToolsPackagingError> {
        match platform {
            ToolsPackagingEnumPlatform::Windows => Ok(windows::ToolsPackagingWindows::plan()),
            ToolsPackagingEnumPlatform::Macos => Ok(macos::ToolsPackagingMacos::plan()),
            ToolsPackagingEnumPlatform::Linux => {
                let tag = std::env::var(linux::ToolsPackagingLinux::TOOLCHAIN_VARIABLE).ok();
                linux::ToolsPackagingLinux::plan(tag.as_deref())
            }
        }
    }
}
