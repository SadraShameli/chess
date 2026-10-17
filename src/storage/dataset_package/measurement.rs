use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use strum::IntoEnumIterator;

use crate::storage::dataset_package::{StorageDatasetPackageArtifact, StorageDatasetPackageError};
use crate::storage::dataset_selection::{
    StorageDatasetSelectionEnumComponent, StorageDatasetSelectionEnumTransition,
    StorageDatasetSelectionFootprint, StorageDatasetSelectionInstalledArtifact,
    StorageDatasetSelectionPrefixes, StorageDatasetSelectionReduction,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StorageDatasetPackageMeasurementRecord")]
pub struct StorageDatasetPackageMeasurement {
    component: StorageDatasetSelectionEnumComponent,
    installed_bytes: u64,
    fixed_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetPackageMeasurementRecord {
    component: StorageDatasetSelectionEnumComponent,
    installed_bytes: u64,
    fixed_bytes: u64,
}

impl TryFrom<StorageDatasetPackageMeasurementRecord> for StorageDatasetPackageMeasurement {
    type Error = StorageDatasetPackageError;
    fn try_from(record: StorageDatasetPackageMeasurementRecord) -> Result<Self, Self::Error> {
        Self::new(record.component, record.installed_bytes, record.fixed_bytes)
    }
}

impl StorageDatasetPackageMeasurement {
    pub fn new(
        component: StorageDatasetSelectionEnumComponent,
        installed_bytes: u64,
        fixed_bytes: u64,
    ) -> Result<Self, StorageDatasetPackageError> {
        if fixed_bytes > installed_bytes {
            return Err(StorageDatasetPackageError::ArtifactSize);
        }
        Ok(Self {
            component,
            installed_bytes,
            fixed_bytes,
        })
    }
    pub const fn component(&self) -> StorageDatasetSelectionEnumComponent {
        self.component
    }
    pub const fn installed_bytes(&self) -> u64 {
        self.installed_bytes
    }
    pub const fn fixed_bytes(&self) -> u64 {
        self.fixed_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StorageDatasetPackageMeasurementsRecord")]
pub struct StorageDatasetPackageMeasurements {
    components: Vec<StorageDatasetPackageMeasurement>,
    total_bytes: u64,
    #[serde(skip)]
    footprint: StorageDatasetSelectionFootprint,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetPackageMeasurementsRecord {
    components: Vec<StorageDatasetPackageMeasurement>,
    total_bytes: u64,
}

impl TryFrom<StorageDatasetPackageMeasurementsRecord> for StorageDatasetPackageMeasurements {
    type Error = StorageDatasetPackageError;
    fn try_from(record: StorageDatasetPackageMeasurementsRecord) -> Result<Self, Self::Error> {
        let measurements = Self::new(record.components)?;
        if measurements.total_bytes != record.total_bytes {
            return Err(StorageDatasetPackageError::Measurements);
        }
        Ok(measurements)
    }
}

impl StorageDatasetPackageMeasurements {
    pub fn new(
        components: Vec<StorageDatasetPackageMeasurement>,
    ) -> Result<Self, StorageDatasetPackageError> {
        if !components
            .iter()
            .map(StorageDatasetPackageMeasurement::component)
            .eq(StorageDatasetSelectionEnumComponent::iter())
        {
            return Err(StorageDatasetPackageError::Measurements);
        }
        let footprint = StorageDatasetSelectionFootprint::from_artifacts(
            components
                .iter()
                .map(|measurement| {
                    StorageDatasetSelectionInstalledArtifact::new(
                        format!("{:?}", measurement.component),
                        Some(measurement.component),
                        measurement.installed_bytes,
                        measurement.fixed_bytes,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
        )?;
        Ok(Self {
            components,
            total_bytes: footprint.total_bytes(),
            footprint,
        })
    }

    pub fn from_artifacts(
        artifacts: &[StorageDatasetPackageArtifact],
    ) -> Result<Self, StorageDatasetPackageError> {
        let footprint = StorageDatasetSelectionFootprint::from_artifacts(
            artifacts
                .iter()
                .map(StorageDatasetPackageArtifact::installed_artifact)
                .collect::<Result<Vec<_>, _>>()?,
        )?;
        let components = StorageDatasetSelectionEnumComponent::iter()
            .map(|component| {
                let fixed_bytes = artifacts
                    .iter()
                    .filter(|artifact| artifact.component() == component)
                    .try_fold(0_u64, |sum, artifact| {
                        sum.checked_add(artifact.fixed_bytes())
                            .ok_or(StorageDatasetPackageError::Measurements)
                    })?;
                StorageDatasetPackageMeasurement::new(
                    component,
                    footprint.component_bytes(component),
                    fixed_bytes,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            components,
            total_bytes: footprint.total_bytes(),
            footprint,
        })
    }
    pub fn components(&self) -> &[StorageDatasetPackageMeasurement] {
        &self.components
    }
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
    pub const fn footprint(&self) -> &StorageDatasetSelectionFootprint {
        &self.footprint
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "StorageDatasetPackageReductionRecord")]
pub struct StorageDatasetPackageReduction {
    measurements: StorageDatasetPackageMeasurements,
    decision: StorageDatasetSelectionReduction,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StorageDatasetPackagePrefixesRecord {
    master: u64,
    broad: u64,
    puzzles: u64,
}

impl StorageDatasetPackagePrefixesRecord {
    pub(super) fn validate(
        self,
    ) -> Result<StorageDatasetSelectionPrefixes, StorageDatasetPackageError> {
        Ok(StorageDatasetSelectionPrefixes::new(
            self.master,
            self.broad,
            self.puzzles,
        )?)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetPackageReductionRecord {
    measurements: StorageDatasetPackageMeasurements,
    before: StorageDatasetPackagePrefixesRecord,
    after: StorageDatasetPackagePrefixesRecord,
    component: StorageDatasetSelectionEnumComponent,
}

impl TryFrom<StorageDatasetPackageReductionRecord> for StorageDatasetPackageReduction {
    type Error = StorageDatasetPackageError;
    fn try_from(record: StorageDatasetPackageReductionRecord) -> Result<Self, Self::Error> {
        let reduction = Self::new(record.measurements, record.before.validate()?)?;
        if reduction.decision.after() != record.after.validate()?
            || reduction.decision.component() != record.component
        {
            return Err(StorageDatasetPackageError::Reduction);
        }
        Ok(reduction)
    }
}

impl Serialize for StorageDatasetPackageReduction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("StorageDatasetPackageReduction", 4)?;
        record.serialize_field("measurements", &self.measurements)?;
        record.serialize_field("before", &self.decision.before())?;
        record.serialize_field("after", &self.decision.after())?;
        record.serialize_field("component", &self.decision.component())?;
        record.end()
    }
}

impl StorageDatasetPackageReduction {
    pub fn new(
        measurements: StorageDatasetPackageMeasurements,
        before: StorageDatasetSelectionPrefixes,
    ) -> Result<Self, StorageDatasetPackageError> {
        match measurements.footprint.transition(before)? {
            StorageDatasetSelectionEnumTransition::Accept => {
                Err(StorageDatasetPackageError::Reduction)
            }
            StorageDatasetSelectionEnumTransition::Reduce(decision) => Ok(Self {
                measurements,
                decision,
            }),
        }
    }
    pub const fn measurements(&self) -> &StorageDatasetPackageMeasurements {
        &self.measurements
    }
    pub const fn decision(&self) -> StorageDatasetSelectionReduction {
        self.decision
    }
}
