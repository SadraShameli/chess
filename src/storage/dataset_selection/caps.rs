use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::storage::dataset_selection::{
    StorageDatasetSelection, StorageDatasetSelectionEnumSubset, StorageDatasetSelectionError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumComponent {
    Master,
    Broad,
    Puzzles,
    Book,
    Labels,
}

impl StorageDatasetSelectionEnumComponent {
    pub const ORDER: [Self; 5] = [
        Self::Master,
        Self::Broad,
        Self::Puzzles,
        Self::Book,
        Self::Labels,
    ];

    pub const fn ceiling_bytes(self) -> u64 {
        match self {
            Self::Master | Self::Broad => 800_000_000,
            Self::Puzzles => 300_000_000,
            Self::Book | Self::Labels => 50_000_000,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Master => 0,
            Self::Broad => 1,
            Self::Puzzles => 2,
            Self::Book => 3,
            Self::Labels => 4,
        }
    }

    const fn reducible_subset(self) -> Option<StorageDatasetSelectionEnumSubset> {
        match self {
            Self::Master | Self::Book => Some(StorageDatasetSelectionEnumSubset::Master),
            Self::Broad => Some(StorageDatasetSelectionEnumSubset::Broad),
            Self::Puzzles => Some(StorageDatasetSelectionEnumSubset::Puzzles),
            Self::Labels => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StorageDatasetSelectionPrefixes {
    master: u64,
    broad: u64,
    puzzles: u64,
}

impl StorageDatasetSelectionPrefixes {
    const REDUCTION_NUMERATOR: u64 = 9;
    const REDUCTION_DENOMINATOR: u64 = 10;

    pub fn new(
        master: u64,
        broad: u64,
        puzzles: u64,
    ) -> Result<Self, StorageDatasetSelectionError> {
        let prefixes = Self {
            master,
            broad,
            puzzles,
        };
        for subset in StorageDatasetSelectionEnumSubset::ORDER {
            subset.validate_target(prefixes.count(subset))?;
        }
        Ok(prefixes)
    }

    pub const fn master(self) -> u64 {
        self.master
    }

    pub const fn broad(self) -> u64 {
        self.broad
    }

    pub const fn puzzles(self) -> u64 {
        self.puzzles
    }

    const fn count(self, subset: StorageDatasetSelectionEnumSubset) -> u64 {
        match subset {
            StorageDatasetSelectionEnumSubset::Master => self.master,
            StorageDatasetSelectionEnumSubset::Broad => self.broad,
            StorageDatasetSelectionEnumSubset::Puzzles => self.puzzles,
        }
    }

    fn validate_required(self) -> Result<(), StorageDatasetSelectionError> {
        for subset in StorageDatasetSelectionEnumSubset::ORDER {
            if self.count(subset) == 0 {
                return Err(StorageDatasetSelectionError::RequiredEmpty(subset));
            }
        }
        Ok(())
    }

    fn reduce(
        self,
        subset: StorageDatasetSelectionEnumSubset,
    ) -> Result<Self, StorageDatasetSelectionError> {
        let count = self
            .count(subset)
            .checked_mul(Self::REDUCTION_NUMERATOR)
            .and_then(|count| count.checked_div(Self::REDUCTION_DENOMINATOR))
            .ok_or(StorageDatasetSelectionError::ArithmeticOverflow)?;
        if count == 0 {
            return Err(StorageDatasetSelectionError::RequiredEmpty(subset));
        }
        Ok(match subset {
            StorageDatasetSelectionEnumSubset::Master => Self {
                master: count,
                ..self
            },
            StorageDatasetSelectionEnumSubset::Broad => Self {
                broad: count,
                ..self
            },
            StorageDatasetSelectionEnumSubset::Puzzles => Self {
                puzzles: count,
                ..self
            },
        })
    }
}

#[derive(Debug)]
pub struct StorageDatasetSelectionInstalledArtifact {
    identity: String,
    component: Option<StorageDatasetSelectionEnumComponent>,
    installed_bytes: u64,
    fixed_bytes: u64,
}

impl StorageDatasetSelectionInstalledArtifact {
    pub fn new(
        identity: String,
        component: Option<StorageDatasetSelectionEnumComponent>,
        installed_bytes: u64,
        fixed_bytes: u64,
    ) -> Result<Self, StorageDatasetSelectionError> {
        if identity.is_empty() {
            return Err(StorageDatasetSelectionError::EmptyArtifactIdentity);
        }
        if fixed_bytes > installed_bytes {
            return Err(StorageDatasetSelectionError::InvalidFixedBytes);
        }
        Ok(Self {
            identity,
            component,
            installed_bytes,
            fixed_bytes,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StorageDatasetSelectionFootprint {
    component_bytes: [u64; StorageDatasetSelectionEnumComponent::ORDER.len()],
    fixed_bytes: [u64; StorageDatasetSelectionEnumComponent::ORDER.len()],
    total_bytes: u64,
}

impl StorageDatasetSelectionFootprint {
    pub fn from_artifacts(
        artifacts: impl IntoIterator<Item = StorageDatasetSelectionInstalledArtifact>,
    ) -> Result<Self, StorageDatasetSelectionError> {
        let mut identities = BTreeSet::new();
        let mut footprint = Self {
            component_bytes: [0; StorageDatasetSelectionEnumComponent::ORDER.len()],
            fixed_bytes: [0; StorageDatasetSelectionEnumComponent::ORDER.len()],
            total_bytes: 0,
        };
        for artifact in artifacts {
            let component = artifact.component.ok_or_else(|| {
                StorageDatasetSelectionError::UnclassifiedArtifact(artifact.identity.clone())
            })?;
            if !identities.insert(artifact.identity.clone()) {
                return Err(StorageDatasetSelectionError::DuplicateArtifact(
                    artifact.identity,
                ));
            }
            let index = component.index();
            footprint.component_bytes[index] = footprint.component_bytes[index]
                .checked_add(artifact.installed_bytes)
                .ok_or(StorageDatasetSelectionError::ArithmeticOverflow)?;
            footprint.fixed_bytes[index] = footprint.fixed_bytes[index]
                .checked_add(artifact.fixed_bytes)
                .ok_or(StorageDatasetSelectionError::ArithmeticOverflow)?;
            footprint.total_bytes = footprint
                .total_bytes
                .checked_add(artifact.installed_bytes)
                .ok_or(StorageDatasetSelectionError::ArithmeticOverflow)?;
        }
        Ok(footprint)
    }

    pub const fn component_bytes(&self, component: StorageDatasetSelectionEnumComponent) -> u64 {
        self.component_bytes[component.index()]
    }

    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub fn transition(
        &self,
        prefixes: StorageDatasetSelectionPrefixes,
    ) -> Result<StorageDatasetSelectionEnumTransition, StorageDatasetSelectionError> {
        prefixes.validate_required()?;
        for component in StorageDatasetSelectionEnumComponent::ORDER {
            let actual = self.component_bytes(component);
            if actual <= component.ceiling_bytes() {
                continue;
            }
            let subset = component
                .reducible_subset()
                .ok_or(StorageDatasetSelectionError::LabelsOverflow { actual })?;
            if self.fixed_bytes[component.index()] > component.ceiling_bytes() {
                return Err(StorageDatasetSelectionError::NonreducibleOverflow(
                    component,
                ));
            }
            let after = prefixes.reduce(subset)?;
            return Ok(StorageDatasetSelectionEnumTransition::Reduce(
                StorageDatasetSelectionReduction {
                    component,
                    before: prefixes,
                    after,
                    rebuild: StorageDatasetSelectionEnumRebuild::for_subset(subset),
                },
            ));
        }
        if self.total_bytes > StorageDatasetSelection::TOTAL_CEILING_BYTES {
            return Err(StorageDatasetSelectionError::TotalOverflow {
                actual: self.total_bytes,
            });
        }
        Ok(StorageDatasetSelectionEnumTransition::Accept)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumRebuild {
    MasterAndBook,
    Broad,
    Puzzles,
}

impl StorageDatasetSelectionEnumRebuild {
    const fn for_subset(subset: StorageDatasetSelectionEnumSubset) -> Self {
        match subset {
            StorageDatasetSelectionEnumSubset::Master => Self::MasterAndBook,
            StorageDatasetSelectionEnumSubset::Broad => Self::Broad,
            StorageDatasetSelectionEnumSubset::Puzzles => Self::Puzzles,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StorageDatasetSelectionReduction {
    component: StorageDatasetSelectionEnumComponent,
    before: StorageDatasetSelectionPrefixes,
    after: StorageDatasetSelectionPrefixes,
    rebuild: StorageDatasetSelectionEnumRebuild,
}

impl StorageDatasetSelectionReduction {
    pub const fn component(self) -> StorageDatasetSelectionEnumComponent {
        self.component
    }

    pub const fn before(self) -> StorageDatasetSelectionPrefixes {
        self.before
    }

    pub const fn after(self) -> StorageDatasetSelectionPrefixes {
        self.after
    }

    pub const fn rebuild(self) -> StorageDatasetSelectionEnumRebuild {
        self.rebuild
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumTransition {
    Accept,
    Reduce(StorageDatasetSelectionReduction),
}
