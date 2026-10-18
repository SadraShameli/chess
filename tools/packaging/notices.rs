use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::packaging::plan::ToolsPackagingError;

#[derive(Deserialize)]
struct ToolsPackagingNoticesEngine {
    name: String,
    tag: String,
    commit: String,
    network: String,
    network_sha256: String,
    network_terms_reviewed: bool,
}

#[derive(Deserialize)]
struct ToolsPackagingNoticesComponent {
    name: String,
    license_file: Option<String>,
}

#[derive(Deserialize)]
struct ToolsPackagingNoticesManifest {
    schema_version: u32,
    engine: ToolsPackagingNoticesEngine,
    components: Vec<ToolsPackagingNoticesComponent>,
}

#[derive(Deserialize)]
struct ToolsPackagingNoticesMetadata {
    packages: Vec<ToolsPackagingNoticesPackage>,
}

#[derive(Deserialize)]
struct ToolsPackagingNoticesPackage {
    name: String,
    license: Option<String>,
}

pub(crate) struct ToolsPackagingNotices;

impl ToolsPackagingNotices {
    pub(crate) fn check(root: &Path) -> Result<Vec<String>, ToolsPackagingError> {
        let manifest: ToolsPackagingNoticesManifest = serde_json::from_slice(&std::fs::read(
            root.join("resources/notices/manifest.json"),
        )?)?;
        let document = std::fs::read_to_string(root.join("docs/third-party-notices.md"))?;
        let mut warnings = Vec::new();
        Self::check_manifest(&manifest, &document, root)?;
        if !manifest.engine.network_terms_reviewed {
            warnings
                .push("network terms are unreviewed: external conveyance stays blocked".to_owned());
        }
        let output = Command::new("cargo")
            .current_dir(root)
            .args(["metadata", "--locked", "--format-version", "1"])
            .output()?;
        if !output.status.success() {
            return Err(ToolsPackagingError::Notices(
                "cargo metadata failed".to_owned(),
            ));
        }
        let missing = Self::unlicensed(&output.stdout)?;
        if !missing.is_empty() {
            return Err(ToolsPackagingError::Notices(format!(
                "crates without a license expression: {}",
                missing.join(", ")
            )));
        }
        Ok(warnings)
    }

    fn check_manifest(
        manifest: &ToolsPackagingNoticesManifest,
        document: &str,
        root: &Path,
    ) -> Result<(), ToolsPackagingError> {
        if manifest.schema_version != 1 {
            return Err(ToolsPackagingError::Notices(
                "unsupported schema version".to_owned(),
            ));
        }
        let engine = &manifest.engine;
        for needed in [
            engine.name.as_str(),
            engine.tag.as_str(),
            engine.commit.as_str(),
            engine.network.as_str(),
            engine.network_sha256.as_str(),
        ] {
            if !document.contains(needed) {
                return Err(ToolsPackagingError::Notices(format!(
                    "docs/third-party-notices.md does not mention '{needed}'"
                )));
            }
        }
        for component in &manifest.components {
            if !document.contains(&component.name) {
                return Err(ToolsPackagingError::Notices(format!(
                    "docs/third-party-notices.md does not mention '{}'",
                    component.name
                )));
            }
            if let Some(file) = &component.license_file
                && !root.join(file).is_file()
            {
                return Err(ToolsPackagingError::Notices(format!(
                    "license file {file} for '{}' is missing",
                    component.name
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn unlicensed(metadata: &[u8]) -> Result<Vec<String>, ToolsPackagingError> {
        let metadata: ToolsPackagingNoticesMetadata = serde_json::from_slice(metadata)?;
        Ok(metadata
            .packages
            .into_iter()
            .filter(|package| package.license.as_deref().is_none_or(str::is_empty))
            .map(|package| package.name)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::ToolsPackagingNotices;

    #[test]
    fn crates_without_a_license_expression_are_reported() -> Result<(), Box<dyn std::error::Error>>
    {
        let metadata = br#"{"packages":[{"name":"a","license":"MIT"},{"name":"b","license":null},{"name":"c","license":""}]}"#;
        assert_eq!(
            ToolsPackagingNotices::unlicensed(metadata)?,
            vec!["b".to_owned(), "c".to_owned()]
        );
        Ok(())
    }
}
