use std::collections::BTreeSet;
use std::io;

use serde::Deserialize;
use thiserror::Error;

use crate::package::digest::PackageDigest;
use crate::storage::dataset_package::StorageDatasetPackageEnumSourceKind;

#[derive(Debug, Error)]
pub enum StorageDatasetSourcesError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("Unsupported dataset source manifest schema version {0}")]
    Schema(u32),
    #[error("The source manifest must name one standard-game, puzzle and openings source")]
    Kinds,
    #[error("The source manifest repeats the identity {0}")]
    Duplicate(String),
    #[error("The source {0} is not licensed CC0-1.0")]
    License(String),
    #[error("The source {0} has no recorded SHA-256")]
    Unrecorded(String),
    #[error("The acquired source {0} is missing")]
    Missing(String),
    #[error("The acquired source {id} changed: expected {expected}, read {actual}")]
    Changed {
        id: String,
        expected: PackageDigest,
        actual: PackageDigest,
    },
    #[error("Reading the acquired source {id} failed: {cause}")]
    Io {
        id: String,
        #[source]
        cause: io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDatasetSourcesEntry {
    pub id: String,
    pub kind: StorageDatasetPackageEnumSourceKind,
    pub version: String,
    pub url: String,
    pub file: String,
    pub license: String,
    pub sha256: Option<PackageDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDatasetSources {
    schema_version: u32,
    sources: Vec<StorageDatasetSourcesEntry>,
}

impl StorageDatasetSources {
    pub const SCHEMA_VERSION: u32 = 1;
    pub const LICENSE: &'static str = "CC0-1.0";

    pub fn from_json(bytes: &[u8]) -> Result<Self, StorageDatasetSourcesError> {
        let parsed: Self = serde_json::from_slice(bytes)?;
        if parsed.schema_version != Self::SCHEMA_VERSION {
            return Err(StorageDatasetSourcesError::Schema(parsed.schema_version));
        }
        let mut ids = BTreeSet::new();
        let mut kinds = BTreeSet::new();
        for source in &parsed.sources {
            if !ids.insert(source.id.as_str()) {
                return Err(StorageDatasetSourcesError::Duplicate(source.id.clone()));
            }
            if !kinds.insert(source.kind) {
                return Err(StorageDatasetSourcesError::Duplicate(format!(
                    "{:?}",
                    source.kind
                )));
            }
            if source.license != Self::LICENSE {
                return Err(StorageDatasetSourcesError::License(source.id.clone()));
            }
        }
        let expected = BTreeSet::from([
            StorageDatasetPackageEnumSourceKind::StandardGames,
            StorageDatasetPackageEnumSourceKind::Puzzles,
            StorageDatasetPackageEnumSourceKind::Openings,
        ]);
        if kinds != expected {
            return Err(StorageDatasetSourcesError::Kinds);
        }
        Ok(parsed)
    }

    pub fn sources(&self) -> &[StorageDatasetSourcesEntry] {
        &self.sources
    }

    pub fn verify(
        &self,
        mut measure: impl FnMut(&StorageDatasetSourcesEntry) -> io::Result<Option<PackageDigest>>,
    ) -> Result<(), StorageDatasetSourcesError> {
        for source in &self.sources {
            let expected = source
                .sha256
                .ok_or_else(|| StorageDatasetSourcesError::Unrecorded(source.id.clone()))?;
            let actual = measure(source)
                .map_err(|error| StorageDatasetSourcesError::Io {
                    id: source.id.clone(),
                    cause: error,
                })?
                .ok_or_else(|| StorageDatasetSourcesError::Missing(source.id.clone()))?;
            if actual != expected {
                return Err(StorageDatasetSourcesError::Changed {
                    id: source.id.clone(),
                    expected,
                    actual,
                });
            }
        }
        Ok(())
    }
}
