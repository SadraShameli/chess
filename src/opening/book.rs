mod error;
mod identity;
mod limits;
mod record;
mod study;
mod validation;

pub use crate::opening::book::error::OpeningBookError;
pub use crate::opening::book::identity::OpeningBookIdentity;
pub use crate::opening::book::limits::OpeningBookLimits;
pub use crate::opening::book::study::{OpeningBookShare, OpeningBookStudy, OpeningBookStudyMove};

use crate::board::position::BoardPosition;
use crate::opening::book::record::OpeningBookRecord;
use crate::opening::book::validation::OpeningBookValidation;
use crate::package::digest::PackageDigest;
use crate::storage::dataset_package::StorageDatasetPackageSourceIdentity;

#[derive(Debug)]
pub struct OpeningBook<'a> {
    bytes: &'a [u8],
    identity: OpeningBookIdentity,
}

impl<'a> OpeningBook<'a> {
    pub fn validate(
        bytes: &'a [u8],
        source: StorageDatasetPackageSourceIdentity,
        expected_checksum: PackageDigest,
        limits: OpeningBookLimits,
        cancellation: impl Fn() -> bool,
    ) -> Result<Self, OpeningBookError> {
        Self::check_cancellation(&cancellation)?;
        limits.validate(bytes)?;
        let checksum = OpeningBookValidation::validate(bytes, expected_checksum, &cancellation)?;
        Self::check_cancellation(&cancellation)?;
        Ok(Self {
            bytes,
            identity: OpeningBookIdentity::new(source, checksum),
        })
    }

    pub const fn identity(&self) -> &OpeningBookIdentity {
        &self.identity
    }

    pub const fn record_count(&self) -> usize {
        self.bytes.len() / OpeningBookRecord::BYTES
    }

    pub fn study(
        &self,
        position: &BoardPosition,
        cancellation: impl Fn() -> bool,
    ) -> Result<OpeningBookStudy, OpeningBookError> {
        Self::check_cancellation(&cancellation)?;
        let key = position.standard_polyglot_key()?;
        let mut study = OpeningBookStudy::new(&self.identity, position, key);
        let mut index = self.lower_bound(key.value(), &cancellation)?;
        if index == self.record_count() || self.record(index)?.key() != key.value() {
            return study.finish(&cancellation);
        }
        let mut legal = position
            .legal_moves()
            .into_iter()
            .map(|movement| {
                Self::check_cancellation(&cancellation)?;
                Ok((OpeningBookRecord::encode(movement)?, movement))
            })
            .collect::<Result<Vec<_>, OpeningBookError>>()?;
        legal.sort_unstable_by_key(|&(encoded, _movement)| encoded);
        while index < self.record_count() {
            Self::check_cancellation(&cancellation)?;
            let record = self.record(index)?;
            if record.key() != key.value() {
                break;
            }
            match legal
                .binary_search_by_key(&record.encoded_move(), |&(encoded, _movement)| encoded)
            {
                Ok(found) => study.push(record, legal[found].1),
                Err(_insertion) => study.reject_illegal(),
            }
            index += 1;
        }
        study.finish(&cancellation)
    }

    pub(super) fn check_cancellation(
        cancellation: &impl Fn() -> bool,
    ) -> Result<(), OpeningBookError> {
        if cancellation() {
            Err(OpeningBookError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn lower_bound(
        &self,
        key: u64,
        cancellation: &impl Fn() -> bool,
    ) -> Result<usize, OpeningBookError> {
        let mut low = 0;
        let mut high = self.record_count();
        while low < high {
            Self::check_cancellation(cancellation)?;
            let middle = low + (high - low) / 2;
            if self.record(middle)?.key() < key {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        Ok(low)
    }

    fn record(&self, index: usize) -> Result<OpeningBookRecord, OpeningBookError> {
        let start = index
            .checked_mul(OpeningBookRecord::BYTES)
            .ok_or(OpeningBookError::RecordRange { record: index })?;
        let end = start
            .checked_add(OpeningBookRecord::BYTES)
            .ok_or(OpeningBookError::RecordRange { record: index })?;
        let bytes = self
            .bytes
            .get(start..end)
            .ok_or(OpeningBookError::RecordRange { record: index })?;
        OpeningBookRecord::decode(bytes)
    }
}
