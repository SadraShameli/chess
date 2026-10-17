mod caps;
mod classification;
mod identity;
mod sequence;

pub use caps::{
    StorageDatasetSelectionEnumComponent, StorageDatasetSelectionEnumRebuild,
    StorageDatasetSelectionEnumTransition, StorageDatasetSelectionFootprint,
    StorageDatasetSelectionInstalledArtifact, StorageDatasetSelectionPrefixes,
    StorageDatasetSelectionReduction,
};
pub use classification::{
    StorageDatasetSelectionEnumGameRatingBand, StorageDatasetSelectionEnumMasterEligibility,
    StorageDatasetSelectionEnumPrimaryTheme, StorageDatasetSelectionEnumPuzzleRatingBand,
    StorageDatasetSelectionEnumRating, StorageDatasetSelectionEnumRatingReason,
    StorageDatasetSelectionEnumSource, StorageDatasetSelectionEnumThemeReason,
    StorageDatasetSelectionEnumTimeClass, StorageDatasetSelectionEnumTimeReason,
    StorageDatasetSelectionGameClassification, StorageDatasetSelectionGameStratum,
    StorageDatasetSelectionPuzzleClassification, StorageDatasetSelectionPuzzleRatingBand,
    StorageDatasetSelectionPuzzleStratum, StorageDatasetSelectionTheme,
};
pub use identity::{StorageDatasetSelectionEnumDuplicate, StorageDatasetSelectionIdentity};
pub use sequence::{
    StorageDatasetSelectionCandidate, StorageDatasetSelectionEnumStratum,
    StorageDatasetSelectionEnumSubset, StorageDatasetSelectionPrefix,
    StorageDatasetSelectionSequence, StorageDatasetSelectionStratumCount,
};

use thiserror::Error;

use crate::package::digest::PackageDigest;

#[derive(Debug, Error)]
pub enum StorageDatasetSelectionError {
    #[error("Cannot encode the canonical selection identity: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("Selection arithmetic exceeds its checked integer range")]
    ArithmeticOverflow,
    #[error("Selection length cannot be represented as an unsigned 64-bit count")]
    LengthOverflow,
    #[error("The identity iterator declared {expected} records but returned {actual}")]
    CountMismatch { expected: u64, actual: u64 },
    #[error("Puzzle rating band index {0} has no unsigned 32-bit rating")]
    InvalidPuzzleBand(u32),
    #[error("A primary theme must contain exactly one nonempty ASCII-whitespace-free token")]
    InvalidTheme,
    #[error("An observed selection stratum must contain at least one candidate")]
    EmptyStratum,
    #[error("Conflicting source records share puzzle identity digest {identity}")]
    PuzzleConflict { identity: PackageDigest },
    #[error("Resolve duplicate source records before constructing the selection sequence")]
    UnresolvedDuplicate,
    #[error("The candidate stratum does not belong to the requested subset")]
    SubsetMismatch,
    #[error("The selection identity kind does not match its stratum")]
    IdentityKindMismatch,
    #[error("Requested {requested} records exceeds the {subset:?} target")]
    TargetExceeded {
        subset: StorageDatasetSelectionEnumSubset,
        requested: u64,
    },
    #[error("The required {0:?} selection must not be empty")]
    RequiredEmpty(StorageDatasetSelectionEnumSubset),
    #[error("An installed artifact must have a nonempty identity")]
    EmptyArtifactIdentity,
    #[error("Installed artifact {0} was assigned more than once")]
    DuplicateArtifact(String),
    #[error("Installed artifact {0} has no component assignment")]
    UnclassifiedArtifact(String),
    #[error("Fixed overhead exceeds the installed artifact length")]
    InvalidFixedBytes,
    #[error("Labels and manifests use {actual} bytes, exceeding their ceiling")]
    LabelsOverflow { actual: u64 },
    #[error("Fixed overhead for {0:?} cannot fit its component ceiling")]
    NonreducibleOverflow(StorageDatasetSelectionEnumComponent),
    #[error("Installed starter data uses {actual} bytes, exceeding the total ceiling")]
    TotalOverflow { actual: u64 },
}

pub struct StorageDatasetSelection;

impl StorageDatasetSelection {
    pub const POLICY_VERSION: u32 = 1;
    pub const TOTAL_CEILING_BYTES: u64 = 2_000_000_000;
}
