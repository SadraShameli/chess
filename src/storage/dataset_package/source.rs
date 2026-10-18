use std::collections::{BTreeMap, btree_map::Entry};

use serde::{Deserialize, Serialize};

use crate::package::digest::PackageDigest;
use crate::storage::dataset_package::{
    StorageDatasetPackageArtifactReference, StorageDatasetPackageError,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "StorageDatasetPackageSourceIdentityRecord")]
pub struct StorageDatasetPackageSourceIdentity {
    source: String,
    version: String,
    manifest_digest: PackageDigest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetPackageSourceIdentityRecord {
    source: String,
    version: String,
    manifest_digest: PackageDigest,
}

enum StorageDatasetPackageEnumSourceMultiplicity {
    Unique,
    Repeated,
}

impl TryFrom<StorageDatasetPackageSourceIdentityRecord> for StorageDatasetPackageSourceIdentity {
    type Error = StorageDatasetPackageError;
    fn try_from(record: StorageDatasetPackageSourceIdentityRecord) -> Result<Self, Self::Error> {
        Self::new(record.source, record.version, record.manifest_digest)
    }
}

impl StorageDatasetPackageSourceIdentity {
    pub fn new(
        source: String,
        version: String,
        manifest_digest: PackageDigest,
    ) -> Result<Self, StorageDatasetPackageError> {
        Self::validate_text(&source)?;
        Self::validate_text(&version)?;
        Ok(Self {
            source,
            version,
            manifest_digest,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub const fn manifest_digest(&self) -> PackageDigest {
        self.manifest_digest
    }

    pub(super) fn validate_text(value: &str) -> Result<(), StorageDatasetPackageError> {
        if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
            return Err(StorageDatasetPackageError::InvalidText);
        }
        Ok(())
    }

    pub(super) fn validate_bindings(sources: &[Self]) -> Result<(), StorageDatasetPackageError> {
        Self::validate_frozen_bindings(sources, StorageDatasetPackageEnumSourceMultiplicity::Unique)
    }

    pub(super) fn validate_record_bindings<'a>(
        sources: impl IntoIterator<Item = &'a Self>,
    ) -> Result<(), StorageDatasetPackageError> {
        Self::validate_frozen_bindings(
            sources,
            StorageDatasetPackageEnumSourceMultiplicity::Repeated,
        )
    }

    fn validate_frozen_bindings<'a>(
        sources: impl IntoIterator<Item = &'a Self>,
        multiplicity: StorageDatasetPackageEnumSourceMultiplicity,
    ) -> Result<(), StorageDatasetPackageError> {
        let mut identities = BTreeMap::new();
        for source in sources {
            match identities.entry((source.source(), source.version())) {
                Entry::Vacant(entry) => {
                    entry.insert(source.manifest_digest());
                }
                Entry::Occupied(entry) => match multiplicity {
                    StorageDatasetPackageEnumSourceMultiplicity::Unique => {
                        return Err(StorageDatasetPackageError::DuplicateSource);
                    }
                    StorageDatasetPackageEnumSourceMultiplicity::Repeated => {
                        if *entry.get() != source.manifest_digest() {
                            return Err(StorageDatasetPackageError::SourceBinding);
                        }
                    }
                },
            }
        }
        if identities.is_empty() {
            return Err(StorageDatasetPackageError::SourceBinding);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetPackageEnumSourceKind {
    StandardGames,
    Puzzles,
    Openings,
    Personal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetPackageEnumLicense {
    Cc0,
    PersonalUse,
    Other(StorageDatasetPackageLicenseId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct StorageDatasetPackageLicenseId(String);

impl StorageDatasetPackageLicenseId {
    pub fn new(value: String) -> Result<Self, StorageDatasetPackageError> {
        StorageDatasetPackageSourceIdentity::validate_text(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for StorageDatasetPackageLicenseId {
    type Error = StorageDatasetPackageError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDatasetPackageLicense {
    terms: StorageDatasetPackageEnumLicense,
    copy: StorageDatasetPackageArtifactReference,
}

impl StorageDatasetPackageLicense {
    pub const fn new(
        terms: StorageDatasetPackageEnumLicense,
        copy: StorageDatasetPackageArtifactReference,
    ) -> Self {
        Self { terms, copy }
    }
    pub const fn terms(&self) -> &StorageDatasetPackageEnumLicense {
        &self.terms
    }
    pub const fn copy(&self) -> &StorageDatasetPackageArtifactReference {
        &self.copy
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StorageDatasetPackageSourceInputRecord")]
pub struct StorageDatasetPackageSource {
    identity: StorageDatasetPackageSourceIdentity,
    kind: StorageDatasetPackageEnumSourceKind,
    origin: String,
    checksum: PackageDigest,
    license: StorageDatasetPackageLicense,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetPackageSourceInputRecord {
    identity: StorageDatasetPackageSourceIdentity,
    kind: StorageDatasetPackageEnumSourceKind,
    origin: String,
    checksum: PackageDigest,
    license: StorageDatasetPackageLicense,
}

impl TryFrom<StorageDatasetPackageSourceInputRecord> for StorageDatasetPackageSource {
    type Error = StorageDatasetPackageError;
    fn try_from(record: StorageDatasetPackageSourceInputRecord) -> Result<Self, Self::Error> {
        Self::new(
            record.identity,
            record.kind,
            record.origin,
            record.checksum,
            record.license,
        )
    }
}

impl StorageDatasetPackageSource {
    pub fn new(
        identity: StorageDatasetPackageSourceIdentity,
        kind: StorageDatasetPackageEnumSourceKind,
        origin: String,
        checksum: PackageDigest,
        license: StorageDatasetPackageLicense,
    ) -> Result<Self, StorageDatasetPackageError> {
        StorageDatasetPackageSourceIdentity::validate_text(&origin)?;
        Ok(Self {
            identity,
            kind,
            origin,
            checksum,
            license,
        })
    }
    pub const fn identity(&self) -> &StorageDatasetPackageSourceIdentity {
        &self.identity
    }
    pub const fn kind(&self) -> StorageDatasetPackageEnumSourceKind {
        self.kind
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub const fn checksum(&self) -> PackageDigest {
        self.checksum
    }
    pub const fn license(&self) -> &StorageDatasetPackageLicense {
        &self.license
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetPackageEnumUnavailableCategory {
    Source,
    Rating,
    TimeControl,
    Themes,
    Title,
    Variant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetPackageEnumUnavailableReason {
    NotProvided,
    Invalid,
    Unsupported,
    MissingFile,
    ChangedDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDatasetPackageUnavailable {
    source: StorageDatasetPackageSourceIdentity,
    category: StorageDatasetPackageEnumUnavailableCategory,
    reason: StorageDatasetPackageEnumUnavailableReason,
}

impl StorageDatasetPackageUnavailable {
    pub const fn new(
        source: StorageDatasetPackageSourceIdentity,
        category: StorageDatasetPackageEnumUnavailableCategory,
        reason: StorageDatasetPackageEnumUnavailableReason,
    ) -> Self {
        Self {
            source,
            category,
            reason,
        }
    }
    pub const fn source(&self) -> &StorageDatasetPackageSourceIdentity {
        &self.source
    }
    pub const fn category(&self) -> StorageDatasetPackageEnumUnavailableCategory {
        self.category
    }
    pub const fn reason(&self) -> StorageDatasetPackageEnumUnavailableReason {
        self.reason
    }
}
