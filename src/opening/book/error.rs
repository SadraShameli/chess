use thiserror::Error;

use crate::board::position::BoardPositionError;
use crate::package::digest::PackageDigest;

#[derive(Debug, Error)]
pub enum OpeningBookError {
    #[error("A Polyglot book must contain at least one record")]
    Empty,
    #[error("Polyglot input has {bytes} bytes and ends with an incomplete 16-byte record")]
    Truncated { bytes: usize },
    #[error("The book byte limit {limit} exceeds the decoded-memory budget {maximum}")]
    LimitExceedsDecodedBudget { limit: usize, maximum: usize },
    #[error("The book has {actual} bytes, exceeding the caller's limit {limit}")]
    ByteLimit { actual: usize, limit: usize },
    #[error("Polyglot record {record} has key {key:#018x} after larger key {previous:#018x}")]
    Unsorted {
        record: usize,
        previous: u64,
        key: u64,
    },
    #[error("Polyglot record {record} repeats move {encoded_move:#06x} for key {key:#018x}")]
    DuplicateMove {
        record: usize,
        key: u64,
        encoded_move: u16,
    },
    #[error("Polyglot record {record} has invalid reserved or promotion bits: {encoded_move:#06x}")]
    MoveEncoding { record: usize, encoded_move: u16 },
    #[error("A legal board move cannot be represented as a Standard Polyglot move")]
    BoardMoveEncoding,
    #[error("Polyglot record {record} is outside the validated byte range")]
    RecordRange { record: usize },
    #[error("Book weight arithmetic exceeds its supported range")]
    WeightRange,
    #[error("The book checksum does not match: expected {expected}, read {actual}")]
    Checksum {
        expected: PackageDigest,
        actual: PackageDigest,
    },
    #[error("Polyglot validation or lookup was cancelled")]
    Cancelled,
    #[error(transparent)]
    Position(#[from] BoardPositionError),
}
