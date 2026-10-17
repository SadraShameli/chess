use std::path::Path;

use chess::desktop::native::resources::{DesktopNativeResources, DesktopNativeResourcesError};

pub struct NativeInstallationHarness;

impl NativeInstallationHarness {
    pub fn run(resources: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../fixtures/platform-dependencies.json"
        ))?;
        let required = Self::names(&fixture, "required")?;
        for expected in DesktopNativeResources::REQUIRED {
            if !required.iter().any(|path| path == expected) {
                return Err(format!("the fixture does not list required {expected}").into());
            }
        }
        let installed = DesktopNativeResources::from_directory(resources)?;
        let report = installed.inspect()?;
        println!(
            "installed resources verified; optional absent: {:?}",
            report.missing_optional()
        );
        let broken = tempfile::tempdir()?;
        match DesktopNativeResources::from_directory(broken.path()) {
            Err(DesktopNativeResourcesError::Io { .. } | DesktopNativeResourcesError::InvalidFile(_)) => {}
            other => {
                return Err(format!(
                    "an empty directory must fail as a required failure: {other:?}"
                )
                .into());
            }
        }
        Ok(())
    }

    fn names(
        fixture: &serde_json::Value,
        section: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        Ok(fixture
            .get(section)
            .and_then(serde_json::Value::as_array)
            .ok_or("the fixture section is missing")?
            .iter()
            .filter_map(|entry| entry.get("path").and_then(serde_json::Value::as_str))
            .map(str::to_owned)
            .collect())
    }
}
