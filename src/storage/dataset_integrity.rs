use std::collections::BTreeSet;
use std::io;

use thiserror::Error;

use crate::package::digest::PackageDigest;
use crate::storage::dataset_package::StorageDatasetPackageArtifact;
use crate::storage::dataset_selection::StorageDatasetSelectionEnumComponent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StorageDatasetIntegrityEnumFeature {
    Explorer,
    Book,
    Puzzles,
}

impl StorageDatasetIntegrityEnumFeature {
    pub const fn for_component(component: StorageDatasetSelectionEnumComponent) -> Self {
        match component {
            StorageDatasetSelectionEnumComponent::Master
            | StorageDatasetSelectionEnumComponent::Broad
            | StorageDatasetSelectionEnumComponent::Labels => Self::Explorer,
            StorageDatasetSelectionEnumComponent::Book => Self::Book,
            StorageDatasetSelectionEnumComponent::Puzzles => Self::Puzzles,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageDatasetIntegrityEnumProblem {
    Missing,
    Unreadable,
    SizeMismatch { expected: u64, actual: u64 },
    ChecksumMismatch {
        expected: PackageDigest,
        actual: PackageDigest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDatasetIntegrityFinding {
    pub feature: StorageDatasetIntegrityEnumFeature,
    pub artifact: String,
    pub problem: StorageDatasetIntegrityEnumProblem,
}

#[derive(Debug, Error)]
#[error("The {feature:?} data failed integrity validation and was not used")]
pub struct StorageDatasetIntegrityRefusal {
    pub feature: StorageDatasetIntegrityEnumFeature,
    pub findings: Vec<StorageDatasetIntegrityFinding>,
}

pub trait StorageDatasetIntegrityReader {
    fn measure(&self, artifact: &str) -> io::Result<Option<(u64, PackageDigest)>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDatasetIntegrityRequirement {
    pub feature: StorageDatasetIntegrityEnumFeature,
    pub artifact: String,
    pub bytes: u64,
    pub checksum: PackageDigest,
}

impl StorageDatasetIntegrityRequirement {
    pub fn from_artifact(artifact: &StorageDatasetPackageArtifact) -> Self {
        Self {
            feature: StorageDatasetIntegrityEnumFeature::for_component(artifact.component()),
            artifact: artifact.reference().path().to_owned(),
            bytes: artifact.installed_bytes(),
            checksum: artifact.reference().checksum(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StorageDatasetIntegrityReport {
    findings: Vec<StorageDatasetIntegrityFinding>,
}

impl StorageDatasetIntegrityReport {
    pub fn findings(&self) -> &[StorageDatasetIntegrityFinding] {
        &self.findings
    }

    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn affected(&self) -> BTreeSet<StorageDatasetIntegrityEnumFeature> {
        self.findings.iter().map(|finding| finding.feature).collect()
    }

    pub fn ensure_available(
        &self,
        feature: StorageDatasetIntegrityEnumFeature,
    ) -> Result<(), StorageDatasetIntegrityRefusal> {
        let findings: Vec<_> = self
            .findings
            .iter()
            .filter(|finding| finding.feature == feature)
            .cloned()
            .collect();
        if findings.is_empty() {
            Ok(())
        } else {
            Err(StorageDatasetIntegrityRefusal { feature, findings })
        }
    }
}

pub struct StorageDatasetIntegrity;

impl StorageDatasetIntegrity {
    pub fn verify(
        requirements: &[StorageDatasetIntegrityRequirement],
        reader: &impl StorageDatasetIntegrityReader,
    ) -> StorageDatasetIntegrityReport {
        let mut findings = Vec::new();
        for requirement in requirements {
            let problem = match reader.measure(&requirement.artifact) {
                Ok(None) => Some(StorageDatasetIntegrityEnumProblem::Missing),
                Err(_error) => Some(StorageDatasetIntegrityEnumProblem::Unreadable),
                Ok(Some((bytes, _checksum))) if bytes != requirement.bytes => {
                    Some(StorageDatasetIntegrityEnumProblem::SizeMismatch {
                        expected: requirement.bytes,
                        actual: bytes,
                    })
                }
                Ok(Some((_bytes, checksum))) if checksum != requirement.checksum => {
                    Some(StorageDatasetIntegrityEnumProblem::ChecksumMismatch {
                        expected: requirement.checksum,
                        actual: checksum,
                    })
                }
                Ok(Some(_)) => None,
            };
            if let Some(problem) = problem {
                findings.push(StorageDatasetIntegrityFinding {
                    feature: requirement.feature,
                    artifact: requirement.artifact.clone(),
                    problem,
                });
            }
        }
        StorageDatasetIntegrityReport { findings }
    }
}
