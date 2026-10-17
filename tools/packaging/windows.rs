use std::path::PathBuf;

use crate::packaging::plan::{ToolsPackagingEnumPlatform, ToolsPackagingPlan, ToolsPackagingStep};

pub(crate) struct ToolsPackagingWindows;

impl ToolsPackagingWindows {
    pub(crate) const UNSIGNED_GUIDANCE: &str = "The installer is unsigned. Windows SmartScreen shows an unrecognized app warning: choose More info, then Run anyway. No WebView2 runtime and no paid code-signing certificate are used. The Visual C++ runtime is linked statically so a clean offline Windows 11 install needs no extra downloads.";

    pub(crate) fn plan() -> ToolsPackagingPlan {
        ToolsPackagingPlan {
            platform: ToolsPackagingEnumPlatform::Windows,
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
                        "x86_64-pc-windows-msvc",
                    ],
                ),
                ToolsPackagingStep::new(
                    "Assemble the unsigned NSIS installer with engine resources",
                    "cargo",
                    &[
                        "packager",
                        "--release",
                        "--config",
                        "packager.toml",
                        "--formats",
                        "nsis",
                    ],
                ),
            ],
            artifacts: vec![PathBuf::from("target/packager")],
            guidance: Self::UNSIGNED_GUIDANCE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ToolsPackagingWindows;

    #[test]
    fn the_windows_plan_builds_then_packages_an_unsigned_nsis_installer() {
        let plan = ToolsPackagingWindows::plan();
        assert_eq!(plan.steps.len(), 2);
        assert!(
            plan.steps[1]
                .arguments
                .iter()
                .any(|argument| argument == "nsis")
        );
        assert!(
            !plan
                .describe()
                .to_lowercase()
                .contains("webview2 runtime is required")
        );
        assert!(plan.guidance.contains("More info"));
    }
}
