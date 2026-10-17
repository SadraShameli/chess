use std::num::NonZeroUsize;

use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::opening::book::{OpeningBook, OpeningBookLimits};
use chess::package::digest::PackageDigest;
use chess::storage::dataset_package::StorageDatasetPackageSourceIdentity;

pub(super) struct OpeningBookFixture;

impl OpeningBookFixture {
    pub(super) const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    pub(super) const START_KEY: u64 = 0x463b_9618_1691_fc9c;
    pub(super) const LITERAL_CHECKSUM: &str =
        "d841dbc2c738761294ef867ab94138815163a1dd8884b77bb9213199312b623d";
    pub(super) const E2E4: u16 = 0x031c;
    pub(super) const D2D4: u16 = 0x02db;
    pub(super) const G1F3: u16 = 0x0195;
    pub(super) const E7E5: u16 = 0x0d24;
    pub(super) const LITERAL_RECORD: [u8; 16] = *include_bytes!("fixtures/start-e2e4.bin");

    pub(super) fn source() -> Result<StorageDatasetPackageSourceIdentity, Box<dyn std::error::Error>>
    {
        Ok(StorageDatasetPackageSourceIdentity::new(
            "local/synthetic-polyglot-reference".into(),
            "fixture-v1".into(),
            PackageDigest::hash_slice(b"synthetic-polyglot-reference-manifest-v1"),
        )?)
    }

    pub(super) fn limits(bytes: usize) -> Result<OpeningBookLimits, Box<dyn std::error::Error>> {
        Ok(OpeningBookLimits::new(
            NonZeroUsize::new(bytes).ok_or("Expected a nonzero byte budget")?,
        )?)
    }

    pub(super) fn book(bytes: &[u8]) -> Result<OpeningBook<'_>, Box<dyn std::error::Error>> {
        Ok(OpeningBook::validate(
            bytes,
            Self::source()?,
            PackageDigest::hash_slice(bytes),
            Self::limits(bytes.len().max(1))?,
            || false,
        )?)
    }

    pub(super) fn position(fen: &str) -> Result<BoardPosition, Box<dyn std::error::Error>> {
        Ok(BoardPosition::from_fen(fen, BoardEnumVariant::Standard)?)
    }

    pub(super) fn record(key: u64, movement: u16, weight: u16, learn: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(&key.to_be_bytes());
        bytes.extend_from_slice(&movement.to_be_bytes());
        bytes.extend_from_slice(&weight.to_be_bytes());
        bytes.extend_from_slice(&learn.to_be_bytes());
        bytes
    }

    pub(super) fn records(rows: &[(u64, u16, u16)]) -> Vec<u8> {
        rows.iter()
            .flat_map(|&(key, movement, weight)| Self::record(key, movement, weight, 0))
            .collect()
    }

    pub(super) fn for_position(
        position: &BoardPosition,
        rows: &[(u16, u16)],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let key = position.standard_polyglot_key()?.value();
        Ok(Self::records(
            &rows
                .iter()
                .map(|&(movement, weight)| (key, movement, weight))
                .collect::<Vec<_>>(),
        ))
    }
}
