use std::path::{Path, PathBuf};

use chess::desktop::native::resources::{
    DesktopNativeResources, DesktopNativeResourcesEnumDependency, DesktopNativeResourcesEnumFailure,
    DesktopNativeResourcesError,
};

pub struct NativeInstallationHarness;

impl NativeInstallationHarness {
    pub fn run(resources: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/platform-dependencies.json"))?;
        Self::check_fixture(&fixture)?;
        Self::failure_classes_match_the_fixture(&fixture)?;
        Self::installed_resources_verify(resources)?;
        Self::missing_optional_is_feature_local(resources)?;
        Self::missing_engine_manifest_is_a_required_failure(resources)?;
        Self::corrupt_pieces_are_a_required_failure(resources)?;
        Self::empty_installation_is_a_required_failure()?;
        println!("installation scenarios passed on the main thread");
        Ok(())
    }

    fn check_fixture(fixture: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
        let required = Self::paths(fixture, "required")?;
        for expected in DesktopNativeResources::REQUIRED {
            if !required.iter().any(|path| path == expected) {
                return Err(format!("the fixture does not list required {expected}").into());
            }
        }
        let optional = Self::paths(fixture, "optional")?;
        for expected in DesktopNativeResources::OPTIONAL {
            if !optional.iter().any(|path| path == expected) {
                return Err(format!("the fixture does not list optional {expected}").into());
            }
        }
        Ok(())
    }

    fn failure_classes_match_the_fixture(
        fixture: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let classes = fixture
            .get("dependencies")
            .and_then(serde_json::Value::as_array)
            .ok_or("the fixture lists dependency failure classes")?;
        for dependency in DesktopNativeResourcesEnumDependency::ALL {
            let entry = classes
                .iter()
                .find(|entry| {
                    entry.get("name").and_then(serde_json::Value::as_str) == Some(dependency.name())
                })
                .ok_or_else(|| format!("{} is missing from the fixture", dependency.name()))?;
            let expected = match entry.get("class").and_then(serde_json::Value::as_str) {
                Some("interruption") => DesktopNativeResourcesEnumFailure::Interruption,
                Some("feature-local") => DesktopNativeResourcesEnumFailure::FeatureLocal,
                Some("dataset-local") => DesktopNativeResourcesEnumFailure::DatasetLocal,
                other => return Err(format!("unknown failure class {other:?}").into()),
            };
            if dependency.failure() != expected {
                return Err(format!(
                    "{} is classified {:?}, the fixture expects {expected:?}",
                    dependency.name(),
                    dependency.failure()
                )
                .into());
            }
        }
        Ok(())
    }

    fn installed_resources_verify(resources: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let installed = DesktopNativeResources::from_directory(resources)?;
        let report = installed.inspect()?;
        println!(
            "installed resources verified; optional absent: {:?}",
            report.missing_optional()
        );
        Ok(())
    }

    fn missing_optional_is_feature_local(
        resources: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let staged = Self::stage(resources, true, true)?;
        let installed = DesktopNativeResources::from_directory(staged.path())?;
        let report = installed.inspect()?;
        for optional in DesktopNativeResources::OPTIONAL {
            if !report.missing_optional().contains(&optional) {
                return Err(format!("{optional} should be reported as unavailable").into());
            }
        }
        Ok(())
    }

    fn missing_engine_manifest_is_a_required_failure(
        resources: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let staged = Self::stage(resources, true, false)?;
        let installed = DesktopNativeResources::from_directory(staged.path())?;
        match installed.inspect() {
            Err(DesktopNativeResourcesError::MissingRequired(name))
                if name == DesktopNativeResources::ENGINE_MANIFEST =>
            {
                Ok(())
            }
            other => Err(format!("a missing engine manifest must be required: {other:?}").into()),
        }
    }

    fn corrupt_pieces_are_a_required_failure(
        resources: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let staged = Self::stage(resources, true, true)?;
        std::fs::write(staged.path().join(DesktopNativeResources::PIECES_ASSET), b"corrupt")?;
        match DesktopNativeResources::from_directory(staged.path()) {
            Err(DesktopNativeResourcesError::Integrity(_)) => Ok(()),
            other => Err(format!("corrupt pieces must fail integrity: {other:?}").into()),
        }
    }

    fn empty_installation_is_a_required_failure() -> Result<(), Box<dyn std::error::Error>> {
        let broken = tempfile::tempdir()?;
        match DesktopNativeResources::from_directory(broken.path()) {
            Err(
                DesktopNativeResourcesError::Io { .. } | DesktopNativeResourcesError::InvalidFile(_),
            ) => Ok(()),
            other => {
                Err(format!("an empty directory must be a required failure: {other:?}").into())
            }
        }
    }

    fn stage(
        resources: &Path,
        pieces: bool,
        engine_manifest: bool,
    ) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut copies: Vec<&str> = Vec::new();
        if pieces {
            copies.push(DesktopNativeResources::PIECES_ASSET);
        }
        if engine_manifest {
            copies.push(DesktopNativeResources::ENGINE_MANIFEST);
        }
        for relative in copies {
            let destination: PathBuf = directory.path().join(relative);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(resources.join(relative), destination)?;
        }
        Ok(directory)
    }

    fn paths(
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
