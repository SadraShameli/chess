use std::path::PathBuf;

use crate::packaging::plan::{ToolsPackagingEnumPlatform, ToolsPackagingPlan, ToolsPackagingStep};

pub(crate) struct ToolsPackagingMacos;

impl ToolsPackagingMacos {
    pub(crate) const APP: &str = "target/packager/Chess.app";
    pub(crate) const ENGINE: &str =
        "target/packager/Chess.app/Contents/Resources/engine/bin/reckless-aarch64-apple-darwin";
    pub(crate) const DMG: &str = "target/packager/Chess.dmg";
    pub(crate) const APPROVAL_GUIDANCE: &str = "The app is ad-hoc signed without a Developer ID, timestamp or notarization. After copying it to Applications, macOS 14 blocks the first launch: open System Settings, Privacy and Security, and choose Open Anyway for Chess, then confirm. This is a manual, per-user approval.";

    pub(crate) fn plan() -> ToolsPackagingPlan {
        ToolsPackagingPlan {
            platform: ToolsPackagingEnumPlatform::Macos,
            steps: vec![
                ToolsPackagingStep::new(
                    "Build the optimized Apple Silicon application",
                    "cargo",
                    &[
                        "build",
                        "--release",
                        "--locked",
                        "--bin",
                        "chess",
                        "--target",
                        "aarch64-apple-darwin",
                    ],
                ),
                ToolsPackagingStep::new(
                    "Assemble the app bundle",
                    "cargo",
                    &[
                        "packager",
                        "--release",
                        "--config",
                        "packager.toml",
                        "--formats",
                        "app",
                    ],
                ),
                ToolsPackagingStep::new(
                    "Ad-hoc sign the nested engine first",
                    "codesign",
                    &["--force", "--sign", "-", "--timestamp=none", Self::ENGINE],
                ),
                ToolsPackagingStep::new(
                    "Ad-hoc sign the app bundle",
                    "codesign",
                    &["--force", "--sign", "-", "--timestamp=none", Self::APP],
                ),
                ToolsPackagingStep::new(
                    "Verify the nested signatures",
                    "codesign",
                    &["--verify", "--deep", "--strict", Self::APP],
                ),
                ToolsPackagingStep::new(
                    "Require an arm64-only application binary",
                    "lipo",
                    &[
                        "-verify_arch",
                        "arm64",
                        "target/packager/Chess.app/Contents/MacOS/chess",
                    ],
                ),
                ToolsPackagingStep::new(
                    "Create the DMG from the signed app",
                    "hdiutil",
                    &[
                        "create",
                        "-volname",
                        "Chess",
                        "-srcfolder",
                        Self::APP,
                        "-ov",
                        "-format",
                        "UDZO",
                        Self::DMG,
                    ],
                ),
            ],
            artifacts: vec![PathBuf::from(Self::APP), PathBuf::from(Self::DMG)],
            guidance: Self::APPROVAL_GUIDANCE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ToolsPackagingMacos;

    #[test]
    fn the_macos_plan_signs_the_engine_before_the_app_and_builds_the_dmg_last() {
        let plan = ToolsPackagingMacos::plan();
        let position = |needle: &str| {
            plan.steps
                .iter()
                .position(|step| step.command_line().contains(needle))
        };
        let engine = position(ToolsPackagingMacos::ENGINE);
        let app = plan.steps.iter().position(|step| {
            step.program == "codesign" && step.command_line().ends_with("Chess.app")
        });
        let dmg = position("hdiutil");
        assert!(engine < app && app < dmg);
        assert!(
            plan.steps
                .iter()
                .all(|step| !step.command_line().contains("notarytool"))
        );
    }
}
