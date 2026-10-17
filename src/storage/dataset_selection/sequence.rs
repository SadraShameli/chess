use std::collections::{BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::package::digest::PackageDigest;
use crate::storage::dataset_selection::{
    StorageDatasetSelection, StorageDatasetSelectionError, StorageDatasetSelectionGameStratum,
    StorageDatasetSelectionIdentity, StorageDatasetSelectionPuzzleStratum,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumSubset {
    Master,
    Broad,
    Puzzles,
}

impl StorageDatasetSelectionEnumSubset {
    pub const ORDER: [Self; 3] = [Self::Master, Self::Broad, Self::Puzzles];

    pub const fn upper_target(self) -> usize {
        match self {
            Self::Master => 100_000,
            Self::Broad => 250_000,
            Self::Puzzles => 200_000,
        }
    }

    pub(super) fn validate_target(self, count: u64) -> Result<(), StorageDatasetSelectionError> {
        let target = u64::try_from(self.upper_target())
            .map_err(|_error| StorageDatasetSelectionError::LengthOverflow)?;
        if count > target {
            return Err(StorageDatasetSelectionError::TargetExceeded {
                subset: self,
                requested: count,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumStratum {
    Master,
    Game(StorageDatasetSelectionGameStratum),
    Puzzle(StorageDatasetSelectionPuzzleStratum),
}

impl StorageDatasetSelectionEnumStratum {
    pub const fn subset(&self) -> StorageDatasetSelectionEnumSubset {
        match self {
            Self::Master => StorageDatasetSelectionEnumSubset::Master,
            Self::Game(_) => StorageDatasetSelectionEnumSubset::Broad,
            Self::Puzzle(_) => StorageDatasetSelectionEnumSubset::Puzzles,
        }
    }

    fn requires_game_identity(&self) -> bool {
        matches!(self, Self::Master | Self::Game(_))
    }
}

#[derive(Debug)]
pub struct StorageDatasetSelectionCandidate<H> {
    handle: H,
    identity: StorageDatasetSelectionIdentity,
    stratum: StorageDatasetSelectionEnumStratum,
}

impl<H> StorageDatasetSelectionCandidate<H> {
    pub fn new(
        handle: H,
        identity: StorageDatasetSelectionIdentity,
        stratum: StorageDatasetSelectionEnumStratum,
    ) -> Result<Self, StorageDatasetSelectionError> {
        if identity.is_game() != stratum.requires_game_identity() {
            return Err(StorageDatasetSelectionError::IdentityKindMismatch);
        }
        Ok(Self {
            handle,
            identity,
            stratum,
        })
    }

    pub const fn handle(&self) -> &H {
        &self.handle
    }

    pub const fn identity(&self) -> &StorageDatasetSelectionIdentity {
        &self.identity
    }

    pub const fn stratum(&self) -> &StorageDatasetSelectionEnumStratum {
        &self.stratum
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StorageDatasetSelectionStratumCountRecord")]
pub struct StorageDatasetSelectionStratumCount {
    stratum: StorageDatasetSelectionEnumStratum,
    available: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetSelectionStratumCountRecord {
    stratum: StorageDatasetSelectionEnumStratum,
    available: u64,
}

impl TryFrom<StorageDatasetSelectionStratumCountRecord> for StorageDatasetSelectionStratumCount {
    type Error = StorageDatasetSelectionError;

    fn try_from(record: StorageDatasetSelectionStratumCountRecord) -> Result<Self, Self::Error> {
        Self::new(record.stratum, record.available)
    }
}

impl StorageDatasetSelectionStratumCount {
    pub fn new(
        stratum: StorageDatasetSelectionEnumStratum,
        available: u64,
    ) -> Result<Self, StorageDatasetSelectionError> {
        if available == 0 {
            return Err(StorageDatasetSelectionError::EmptyStratum);
        }
        Ok(Self { stratum, available })
    }

    pub const fn stratum(&self) -> &StorageDatasetSelectionEnumStratum {
        &self.stratum
    }

    pub const fn available(&self) -> u64 {
        self.available
    }
}

#[derive(Debug)]
pub struct StorageDatasetSelectionSequence<H> {
    subset: StorageDatasetSelectionEnumSubset,
    candidates: Vec<StorageDatasetSelectionCandidate<H>>,
    strata: Vec<StorageDatasetSelectionStratumCount>,
}

struct StorageDatasetSelectionBucket<H> {
    stratum: StorageDatasetSelectionEnumStratum,
    candidates: VecDeque<StorageDatasetSelectionCandidate<H>>,
}

impl<H> StorageDatasetSelectionBucket<H> {
    fn new(candidate: StorageDatasetSelectionCandidate<H>) -> Self {
        Self {
            stratum: candidate.stratum.clone(),
            candidates: VecDeque::from([candidate]),
        }
    }
}

impl<H> StorageDatasetSelectionSequence<H> {
    pub fn new(
        subset: StorageDatasetSelectionEnumSubset,
        mut candidates: Vec<StorageDatasetSelectionCandidate<H>>,
    ) -> Result<Self, StorageDatasetSelectionError> {
        Self::validate_candidates(subset, &candidates)?;
        candidates.sort_unstable_by(|left, right| {
            left.stratum.cmp(&right.stratum).then_with(|| {
                StorageDatasetSelection::compare_order(
                    left.identity.digest(),
                    left.identity.bytes(),
                    right.identity.digest(),
                    right.identity.bytes(),
                )
            })
        });
        let limit = candidates.len().min(subset.upper_target());
        let mut buckets: Vec<StorageDatasetSelectionBucket<H>> = Vec::new();
        for candidate in candidates {
            if let Some(bucket) = buckets.last_mut()
                && bucket.stratum == candidate.stratum
            {
                bucket.candidates.push_back(candidate);
            } else {
                buckets.push(StorageDatasetSelectionBucket::new(candidate));
            }
        }
        let strata = buckets
            .iter()
            .map(|bucket| {
                StorageDatasetSelectionStratumCount::new(
                    bucket.stratum.clone(),
                    u64::try_from(bucket.candidates.len())
                        .map_err(|_error| StorageDatasetSelectionError::LengthOverflow)?,
                )
            })
            .collect::<Result<Vec<_>, StorageDatasetSelectionError>>()?;
        let mut candidates = Vec::with_capacity(limit);
        while candidates.len() < limit && !buckets.is_empty() {
            buckets.retain_mut(|bucket| {
                if candidates.len() < limit
                    && let Some(candidate) = bucket.candidates.pop_front()
                {
                    candidates.push(candidate);
                }
                !bucket.candidates.is_empty()
            });
        }
        Ok(Self {
            subset,
            candidates,
            strata,
        })
    }

    fn validate_candidates(
        subset: StorageDatasetSelectionEnumSubset,
        candidates: &[StorageDatasetSelectionCandidate<H>],
    ) -> Result<(), StorageDatasetSelectionError> {
        let mut identities = BTreeSet::new();
        for candidate in candidates {
            if candidate.stratum.subset() != subset {
                return Err(StorageDatasetSelectionError::SubsetMismatch);
            }
            if !identities.insert(candidate.identity.bytes()) {
                return Err(StorageDatasetSelectionError::UnresolvedDuplicate);
            }
        }
        Ok(())
    }

    pub fn prefix(
        &self,
        requested: usize,
    ) -> Result<StorageDatasetSelectionPrefix<'_, H>, StorageDatasetSelectionError> {
        self.subset.validate_target(
            u64::try_from(requested)
                .map_err(|_error| StorageDatasetSelectionError::LengthOverflow)?,
        )?;
        let candidates = &self.candidates[..requested.min(self.candidates.len())];
        let digest = StorageDatasetSelection::sequence_digest(
            candidates
                .iter()
                .map(StorageDatasetSelectionCandidate::identity),
        )?;
        Ok(StorageDatasetSelectionPrefix { candidates, digest })
    }

    pub fn strata(&self) -> &[StorageDatasetSelectionStratumCount] {
        &self.strata
    }
}

#[derive(Debug)]
pub struct StorageDatasetSelectionPrefix<'a, H> {
    candidates: &'a [StorageDatasetSelectionCandidate<H>],
    digest: PackageDigest,
}

impl<'a, H> StorageDatasetSelectionPrefix<'a, H> {
    pub const fn candidates(&self) -> &'a [StorageDatasetSelectionCandidate<H>] {
        self.candidates
    }

    pub const fn digest(&self) -> PackageDigest {
        self.digest
    }
}
