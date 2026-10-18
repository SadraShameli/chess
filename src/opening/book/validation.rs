use sha2::{Digest, Sha256};

use crate::opening::book::record::OpeningBookRecord;
use crate::opening::book::{OpeningBook, OpeningBookError};
use crate::package::digest::PackageDigest;

pub(super) struct OpeningBookValidation {
    previous_key: Option<u64>,
    seen: [u64; OpeningBookValidation::BITMAP_WORDS],
    touched: Vec<usize>,
}

impl OpeningBookValidation {
    const CHUNK_BYTES: usize = 64 * 1024;
    const WORD_BITS: usize = 64;
    const BITMAP_WORDS: usize = OpeningBookRecord::ENCODING_COUNT / Self::WORD_BITS;

    pub(super) fn validate(
        bytes: &[u8],
        expected: PackageDigest,
        cancellation: &impl Fn() -> bool,
    ) -> Result<PackageDigest, OpeningBookError> {
        let mut validation = Self {
            previous_key: None,
            seen: [0; Self::BITMAP_WORDS],
            touched: Vec::with_capacity(Self::BITMAP_WORDS),
        };
        let mut digest = Sha256::new();
        for (chunk_index, chunk) in bytes.chunks(Self::CHUNK_BYTES).enumerate() {
            OpeningBook::check_cancellation(cancellation)?;
            let first_record = chunk_index * (Self::CHUNK_BYTES / OpeningBookRecord::BYTES);
            for (offset, bytes) in chunk
                .as_chunks::<{ OpeningBookRecord::BYTES }>()
                .0
                .iter()
                .enumerate()
            {
                OpeningBook::check_cancellation(cancellation)?;
                validation.record(OpeningBookRecord::decode(bytes)?, first_record + offset)?;
            }
            OpeningBook::check_cancellation(cancellation)?;
            digest.update(chunk);
        }
        OpeningBook::check_cancellation(cancellation)?;
        let actual = PackageDigest::from_bytes(digest.finalize().into());
        if actual != expected {
            return Err(OpeningBookError::Checksum { expected, actual });
        }
        Ok(actual)
    }

    fn record(&mut self, record: OpeningBookRecord, index: usize) -> Result<(), OpeningBookError> {
        record.validate_encoding(index)?;
        if let Some(previous) = self.previous_key {
            if record.key() < previous {
                return Err(OpeningBookError::Unsorted {
                    record: index,
                    previous,
                    key: record.key(),
                });
            }
            if record.key() != previous {
                for word in self.touched.drain(..) {
                    self.seen[word] = 0;
                }
            }
        }
        self.previous_key = Some(record.key());
        let encoded = usize::from(record.encoded_move());
        let word = encoded / Self::WORD_BITS;
        let mask = 1_u64 << (encoded % Self::WORD_BITS);
        if self.seen[word] & mask != 0 {
            return Err(OpeningBookError::DuplicateMove {
                record: index,
                key: record.key(),
                encoded_move: record.encoded_move(),
            });
        }
        if self.seen[word] == 0 {
            self.touched.push(word);
        }
        self.seen[word] |= mask;
        Ok(())
    }
}
