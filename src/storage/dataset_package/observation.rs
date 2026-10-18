use serde::{Deserialize, Serialize};

use crate::package::digest::PackageDigest;
use crate::storage::dataset_package::{
    StorageDatasetPackageArtifactReference, StorageDatasetPackageError,
    StorageDatasetPackageSourceIdentity,
};
use crate::storage::dataset_selection::{
    StorageDatasetSelectionEnumGameRatingBand, StorageDatasetSelectionEnumPrimaryTheme,
    StorageDatasetSelectionEnumPuzzleRatingBand, StorageDatasetSelectionEnumStratum,
    StorageDatasetSelectionEnumSubset, StorageDatasetSelectionEnumTimeClass,
    StorageDatasetSelectionSequence, StorageDatasetSelectionStratumCount,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StorageDatasetPackageCountsRecord")]
pub struct StorageDatasetPackageCounts {
    observed: u64,
    available: u64,
    unknown: u64,
    rejected: u64,
    duplicates: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetPackageCountsRecord {
    observed: u64,
    available: u64,
    unknown: u64,
    rejected: u64,
    duplicates: u64,
}

impl TryFrom<StorageDatasetPackageCountsRecord> for StorageDatasetPackageCounts {
    type Error = StorageDatasetPackageError;
    fn try_from(record: StorageDatasetPackageCountsRecord) -> Result<Self, Self::Error> {
        Self::new(
            record.observed,
            record.available,
            record.unknown,
            record.rejected,
            record.duplicates,
        )
    }
}

impl StorageDatasetPackageCounts {
    pub fn new(
        observed: u64,
        available: u64,
        unknown: u64,
        rejected: u64,
        duplicates: u64,
    ) -> Result<Self, StorageDatasetPackageError> {
        if unknown > available
            || available
                .checked_add(rejected)
                .and_then(|count| count.checked_add(duplicates))
                != Some(observed)
        {
            return Err(StorageDatasetPackageError::Counts);
        }
        Ok(Self {
            observed,
            available,
            unknown,
            rejected,
            duplicates,
        })
    }
    pub const fn observed(self) -> u64 {
        self.observed
    }
    pub const fn available(self) -> u64 {
        self.available
    }
    pub const fn unknown(self) -> u64 {
        self.unknown
    }
    pub const fn rejected(self) -> u64 {
        self.rejected
    }
    pub const fn duplicates(self) -> u64 {
        self.duplicates
    }
}

#[derive(Debug)]
pub struct StorageDatasetPackageObservationInput {
    pub subset: StorageDatasetSelectionEnumSubset,
    pub sources: Vec<StorageDatasetPackageSourceIdentity>,
    pub counts: StorageDatasetPackageCounts,
    pub requested_prefix: usize,
    pub provenance: StorageDatasetPackageArtifactReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StorageDatasetPackageObservationRecord")]
pub struct StorageDatasetPackageObservation {
    subset: StorageDatasetSelectionEnumSubset,
    sources: Vec<StorageDatasetPackageSourceIdentity>,
    counts: StorageDatasetPackageCounts,
    strata: Vec<StorageDatasetSelectionStratumCount>,
    selected_prefix: u64,
    sequence_digest: PackageDigest,
    provenance: StorageDatasetPackageArtifactReference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetPackageObservationRecord {
    subset: StorageDatasetSelectionEnumSubset,
    sources: Vec<StorageDatasetPackageSourceIdentity>,
    counts: StorageDatasetPackageCounts,
    strata: Vec<StorageDatasetSelectionStratumCount>,
    selected_prefix: u64,
    sequence_digest: PackageDigest,
    provenance: StorageDatasetPackageArtifactReference,
}

impl TryFrom<StorageDatasetPackageObservationRecord> for StorageDatasetPackageObservation {
    type Error = StorageDatasetPackageError;
    fn try_from(record: StorageDatasetPackageObservationRecord) -> Result<Self, Self::Error> {
        let observation = Self {
            subset: record.subset,
            sources: record.sources,
            counts: record.counts,
            strata: record.strata,
            selected_prefix: record.selected_prefix,
            sequence_digest: record.sequence_digest,
            provenance: record.provenance,
        };
        observation.validate()?;
        Ok(observation)
    }
}

impl StorageDatasetPackageObservation {
    pub fn from_sequence<H>(
        sequence: &StorageDatasetSelectionSequence<H>,
        input: StorageDatasetPackageObservationInput,
    ) -> Result<Self, StorageDatasetPackageError> {
        let prefix = sequence.prefix(input.requested_prefix)?;
        let selected_prefix = u64::try_from(prefix.candidates().len())
            .map_err(|_error| StorageDatasetPackageError::Counts)?;
        let observation = Self {
            subset: input.subset,
            sources: input.sources,
            counts: input.counts,
            strata: sequence.strata().to_vec(),
            selected_prefix,
            sequence_digest: prefix.digest(),
            provenance: input.provenance,
        };
        observation.validate()?;
        Ok(observation)
    }

    fn validate(&self) -> Result<(), StorageDatasetPackageError> {
        StorageDatasetPackageSourceIdentity::validate_bindings(&self.sources)?;
        let target = u64::try_from(self.subset.upper_target())
            .map_err(|_error| StorageDatasetPackageError::Counts)?;
        if self.selected_prefix == 0 || self.selected_prefix > self.counts.available.min(target) {
            return Err(StorageDatasetPackageError::Prefix);
        }
        let mut available = 0_u64;
        let mut unknown = 0_u64;
        let mut previous = None;
        for stratum in &self.strata {
            if stratum.stratum().subset() != self.subset
                || previous.is_some_and(|previous| previous >= stratum.stratum())
            {
                return Err(StorageDatasetPackageError::Strata);
            }
            previous = Some(stratum.stratum());
            available = available
                .checked_add(stratum.available())
                .ok_or(StorageDatasetPackageError::Counts)?;
            if Self::has_unknown(stratum.stratum()) {
                unknown = unknown
                    .checked_add(stratum.available())
                    .ok_or(StorageDatasetPackageError::Counts)?;
            }
        }
        if available != self.counts.available || unknown != self.counts.unknown {
            return Err(StorageDatasetPackageError::Strata);
        }
        Ok(())
    }

    fn has_unknown(stratum: &StorageDatasetSelectionEnumStratum) -> bool {
        match stratum {
            StorageDatasetSelectionEnumStratum::Master => false,
            StorageDatasetSelectionEnumStratum::Game(stratum) => {
                stratum.rating_band() == StorageDatasetSelectionEnumGameRatingBand::Unknown
                    || stratum.time_class() == StorageDatasetSelectionEnumTimeClass::Unknown
            }
            StorageDatasetSelectionEnumStratum::Puzzle(stratum) => {
                stratum.rating_band() == StorageDatasetSelectionEnumPuzzleRatingBand::Unknown
                    || stratum.primary_theme() == &StorageDatasetSelectionEnumPrimaryTheme::Unknown
            }
        }
    }

    pub fn validate_reproduction<H>(
        &self,
        sequence: &StorageDatasetSelectionSequence<H>,
    ) -> Result<(), StorageDatasetPackageError> {
        let requested = usize::try_from(self.selected_prefix)
            .map_err(|_error| StorageDatasetPackageError::Counts)?;
        let prefix = sequence.prefix(requested)?;
        if sequence.strata() != self.strata.as_slice()
            || u64::try_from(prefix.candidates().len())
                .map_err(|_error| StorageDatasetPackageError::Counts)?
                != self.selected_prefix
            || prefix.digest() != self.sequence_digest
        {
            return Err(StorageDatasetPackageError::Reproduction);
        }
        Ok(())
    }
    pub const fn subset(&self) -> StorageDatasetSelectionEnumSubset {
        self.subset
    }
    pub fn sources(&self) -> &[StorageDatasetPackageSourceIdentity] {
        &self.sources
    }
    pub const fn counts(&self) -> StorageDatasetPackageCounts {
        self.counts
    }
    pub fn strata(&self) -> &[StorageDatasetSelectionStratumCount] {
        &self.strata
    }
    pub const fn selected_prefix(&self) -> u64 {
        self.selected_prefix
    }
    pub const fn sequence_digest(&self) -> PackageDigest {
        self.sequence_digest
    }
    pub const fn provenance(&self) -> &StorageDatasetPackageArtifactReference {
        &self.provenance
    }
}
