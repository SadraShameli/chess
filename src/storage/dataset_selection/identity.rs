use std::cmp::Ordering;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::notation::game_identity::{NotationGameIdentity, NotationGameIdentityEnumResult};
use crate::package::digest::PackageDigest;
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::storage::dataset_selection::{StorageDatasetSelection, StorageDatasetSelectionError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetSelectionEnumDuplicate {
    Distinct,
    Identical,
    Keep,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StorageDatasetSelectionEnumIdentityKind {
    Game,
    Puzzle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDatasetSelectionIdentity {
    kind: StorageDatasetSelectionEnumIdentityKind,
    bytes: Vec<u8>,
    digest: PackageDigest,
}

impl StorageDatasetSelectionIdentity {
    pub fn game(identity: &NotationGameIdentity) -> Result<Self, StorageDatasetSelectionError> {
        let structural = identity.structural_key();
        let date = identity.date();
        let bytes = serde_json::to_vec(&(
            structural.variant(),
            structural.initial_fen(),
            structural.mainline_uci(),
            identity.white(),
            identity.black(),
            (
                date.and_then(|date| date.year()),
                date.and_then(|date| date.month()),
                date.and_then(|date| date.day()),
            ),
            identity
                .result()
                .map(NotationGameIdentityEnumResult::pgn_token),
        ))?;
        Ok(Self::new(
            StorageDatasetSelectionEnumIdentityKind::Game,
            bytes,
        ))
    }

    pub fn puzzle(identity: &PuzzleDefinitionIdentity) -> Self {
        Self::new(
            StorageDatasetSelectionEnumIdentityKind::Puzzle,
            identity.puzzle_id().as_bytes().to_vec(),
        )
    }

    fn new(kind: StorageDatasetSelectionEnumIdentityKind, bytes: Vec<u8>) -> Self {
        let digest = PackageDigest::hash_slice(&bytes);
        Self {
            kind,
            bytes,
            digest,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub const fn digest(&self) -> PackageDigest {
        self.digest
    }

    pub(super) fn is_game(&self) -> bool {
        self.kind == StorageDatasetSelectionEnumIdentityKind::Game
    }
}

impl StorageDatasetSelection {
    pub fn compare_order(
        left_digest: PackageDigest,
        left_identity: &[u8],
        right_digest: PackageDigest,
        right_identity: &[u8],
    ) -> Ordering {
        left_digest
            .cmp(&right_digest)
            .then_with(|| left_identity.cmp(right_identity))
    }

    pub fn compare_duplicate(
        left: &StorageDatasetSelectionIdentity,
        left_record: &str,
        right: &StorageDatasetSelectionIdentity,
        right_record: &str,
    ) -> Result<StorageDatasetSelectionEnumDuplicate, StorageDatasetSelectionError> {
        if left != right {
            return Ok(StorageDatasetSelectionEnumDuplicate::Distinct);
        }
        if left_record == right_record {
            return Ok(StorageDatasetSelectionEnumDuplicate::Identical);
        }
        match left.kind {
            StorageDatasetSelectionEnumIdentityKind::Puzzle => {
                Err(StorageDatasetSelectionError::PuzzleConflict {
                    identity: left.digest,
                })
            }
            StorageDatasetSelectionEnumIdentityKind::Game => {
                Ok(if right_record.as_bytes() < left_record.as_bytes() {
                    StorageDatasetSelectionEnumDuplicate::Replace
                } else {
                    StorageDatasetSelectionEnumDuplicate::Keep
                })
            }
        }
    }

    pub fn sequence_digest<'a>(
        identities: impl ExactSizeIterator<Item = &'a StorageDatasetSelectionIdentity>,
    ) -> Result<PackageDigest, StorageDatasetSelectionError> {
        let count = u64::try_from(identities.len())
            .map_err(|_error| StorageDatasetSelectionError::LengthOverflow)?;
        let mut digest = Sha256::new();
        digest.update(count.to_be_bytes());
        let mut actual = 0_u64;
        for identity in identities {
            let length = u64::try_from(identity.bytes.len())
                .map_err(|_error| StorageDatasetSelectionError::LengthOverflow)?;
            digest.update(length.to_be_bytes());
            digest.update(&identity.bytes);
            actual = actual
                .checked_add(1)
                .ok_or(StorageDatasetSelectionError::ArithmeticOverflow)?;
        }
        if actual != count {
            return Err(StorageDatasetSelectionError::CountMismatch {
                expected: count,
                actual,
            });
        }
        Ok(PackageDigest::from_bytes(digest.finalize().into()))
    }
}
