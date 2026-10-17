use serde::{Deserialize, Serialize};

use crate::package::digest::PackageDigest;
use crate::storage::dataset_package::{
    StorageDatasetPackageError, StorageDatasetPackageSourceIdentity,
};
use crate::storage::dataset_selection::{
    StorageDatasetSelectionEnumComponent, StorageDatasetSelectionInstalledArtifact,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "StorageDatasetPackageArtifactReferenceRecord")]
pub struct StorageDatasetPackageArtifactReference {
    path: String,
    checksum: PackageDigest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetPackageArtifactReferenceRecord {
    path: String,
    checksum: PackageDigest,
}

impl TryFrom<StorageDatasetPackageArtifactReferenceRecord>
    for StorageDatasetPackageArtifactReference
{
    type Error = StorageDatasetPackageError;
    fn try_from(record: StorageDatasetPackageArtifactReferenceRecord) -> Result<Self, Self::Error> {
        Self::new(record.path, record.checksum)
    }
}

impl StorageDatasetPackageArtifactReference {
    pub fn new(path: String, checksum: PackageDigest) -> Result<Self, StorageDatasetPackageError> {
        if path.is_empty()
            || path.contains(['\\', ':'])
            || path.chars().any(char::is_control)
            || path.split('/').any(Self::unsafe_segment)
        {
            return Err(StorageDatasetPackageError::UnsafePath(path));
        }
        Ok(Self { path, checksum })
    }

    fn unsafe_segment(segment: &str) -> bool {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.ends_with(['.', ' '])
            || segment.contains(['<', '>', '"', '|', '?', '*'])
        {
            return true;
        }
        let stem = segment
            .split_once('.')
            .map_or(segment, |(stem, _extension)| stem);
        let upper = stem.to_ascii_uppercase();
        matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].into_iter().any(|prefix| {
                upper.strip_prefix(prefix).is_some_and(|suffix| {
                    matches!(
                        suffix,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            })
    }

    pub fn path(&self) -> &str {
        &self.path
    }
    pub const fn checksum(&self) -> PackageDigest {
        self.checksum
    }

    pub(super) fn collision_key(&self) -> String {
        let normalized = icu_normalizer::ComposingNormalizer::new_nfc().normalize(&self.path);
        icu_casemap::CaseMapper::new()
            .fold_string(&normalized)
            .into_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetPackageEnumArtifactRole {
    Dataset,
    Index,
    Provenance,
    LicenseCopy,
    Book,
    Labels,
    Manifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StorageDatasetPackageArtifactInput")]
pub struct StorageDatasetPackageArtifact {
    reference: StorageDatasetPackageArtifactReference,
    component: StorageDatasetSelectionEnumComponent,
    role: StorageDatasetPackageEnumArtifactRole,
    installed_bytes: u64,
    fixed_bytes: u64,
    sources: Vec<StorageDatasetPackageSourceIdentity>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDatasetPackageArtifactInput {
    pub reference: StorageDatasetPackageArtifactReference,
    pub component: StorageDatasetSelectionEnumComponent,
    pub role: StorageDatasetPackageEnumArtifactRole,
    pub installed_bytes: u64,
    pub fixed_bytes: u64,
    pub sources: Vec<StorageDatasetPackageSourceIdentity>,
}

impl TryFrom<StorageDatasetPackageArtifactInput> for StorageDatasetPackageArtifact {
    type Error = StorageDatasetPackageError;
    fn try_from(input: StorageDatasetPackageArtifactInput) -> Result<Self, Self::Error> {
        Self::new(input)
    }
}

impl StorageDatasetPackageArtifact {
    pub fn new(
        input: StorageDatasetPackageArtifactInput,
    ) -> Result<Self, StorageDatasetPackageError> {
        if input.fixed_bytes > input.installed_bytes {
            return Err(StorageDatasetPackageError::ArtifactSize);
        }
        StorageDatasetPackageSourceIdentity::validate_bindings(&input.sources)?;
        let valid_component = match input.role {
            StorageDatasetPackageEnumArtifactRole::Dataset
            | StorageDatasetPackageEnumArtifactRole::Provenance => matches!(
                input.component,
                StorageDatasetSelectionEnumComponent::Master
                    | StorageDatasetSelectionEnumComponent::Broad
                    | StorageDatasetSelectionEnumComponent::Puzzles
            ),
            StorageDatasetPackageEnumArtifactRole::Book => {
                input.component == StorageDatasetSelectionEnumComponent::Book
            }
            StorageDatasetPackageEnumArtifactRole::LicenseCopy
            | StorageDatasetPackageEnumArtifactRole::Labels
            | StorageDatasetPackageEnumArtifactRole::Manifest => {
                input.component == StorageDatasetSelectionEnumComponent::Labels
            }
            StorageDatasetPackageEnumArtifactRole::Index => true,
        };
        if !valid_component {
            return Err(StorageDatasetPackageError::ArtifactBinding);
        }
        Ok(Self {
            reference: input.reference,
            component: input.component,
            role: input.role,
            installed_bytes: input.installed_bytes,
            fixed_bytes: input.fixed_bytes,
            sources: input.sources,
        })
    }
    pub const fn reference(&self) -> &StorageDatasetPackageArtifactReference {
        &self.reference
    }
    pub const fn component(&self) -> StorageDatasetSelectionEnumComponent {
        self.component
    }
    pub const fn role(&self) -> StorageDatasetPackageEnumArtifactRole {
        self.role
    }
    pub const fn installed_bytes(&self) -> u64 {
        self.installed_bytes
    }
    pub const fn fixed_bytes(&self) -> u64 {
        self.fixed_bytes
    }
    pub fn sources(&self) -> &[StorageDatasetPackageSourceIdentity] {
        &self.sources
    }

    pub fn installed_artifact(
        &self,
    ) -> Result<StorageDatasetSelectionInstalledArtifact, StorageDatasetPackageError> {
        Ok(StorageDatasetSelectionInstalledArtifact::new(
            self.reference.path.clone(),
            Some(self.component),
            self.installed_bytes,
            self.fixed_bytes,
        )?)
    }
}
