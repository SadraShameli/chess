use std::num::NonZeroUsize;

use crate::opening::book::OpeningBookError;
use crate::opening::book::record::OpeningBookRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpeningBookLimits {
    max_bytes: NonZeroUsize,
}

impl OpeningBookLimits {
    pub const MAX_DECODED_BYTES: usize = 64 * 1024 * 1024;

    pub fn new(max_bytes: NonZeroUsize) -> Result<Self, OpeningBookError> {
        if max_bytes.get() > Self::MAX_DECODED_BYTES {
            return Err(OpeningBookError::LimitExceedsDecodedBudget {
                limit: max_bytes.get(),
                maximum: Self::MAX_DECODED_BYTES,
            });
        }
        Ok(Self { max_bytes })
    }

    pub const fn max_bytes(self) -> NonZeroUsize {
        self.max_bytes
    }

    pub(super) fn validate(self, bytes: &[u8]) -> Result<(), OpeningBookError> {
        if bytes.len() > self.max_bytes.get() {
            return Err(OpeningBookError::ByteLimit {
                actual: bytes.len(),
                limit: self.max_bytes.get(),
            });
        }
        if bytes.is_empty() {
            return Err(OpeningBookError::Empty);
        }
        if !bytes.len().is_multiple_of(OpeningBookRecord::BYTES) {
            return Err(OpeningBookError::Truncated { bytes: bytes.len() });
        }
        Ok(())
    }
}
