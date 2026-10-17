use std::collections::{BTreeMap, HashSet};

use thiserror::Error;

use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::{BoardPosition, BoardPositionError};
use crate::opening::book::OpeningBookError;
use crate::opening::book::record::OpeningBookRecord;

#[derive(Debug, Error)]
pub enum OpeningBookGenerationError {
    #[error(transparent)]
    Book(#[from] OpeningBookError),
    #[error(transparent)]
    Move(#[from] BoardMoveError),
    #[error(transparent)]
    Position(#[from] BoardPositionError),
    #[error("A book position count exceeds the checked integer range")]
    Count,
    #[error("The generation was cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningBookGeneratedMove {
    pub key: u64,
    pub encoded_move: u16,
    pub count: u64,
    pub weight: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningBookGenerated {
    bytes: Vec<u8>,
    moves: Vec<OpeningBookGeneratedMove>,
}

impl OpeningBookGenerated {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn moves(&self) -> &[OpeningBookGeneratedMove] {
        &self.moves
    }
}

pub struct OpeningBookGenerator;

impl OpeningBookGenerator {
    pub const MAX_PLIES: usize = 40;
    pub const MAX_WEIGHT: u64 = 65_535;

    pub fn generate<'a>(
        games: impl IntoIterator<Item = &'a [BoardMove]>,
        cancelled: impl Fn() -> bool,
    ) -> Result<OpeningBookGenerated, OpeningBookGenerationError> {
        let mut counts: BTreeMap<(u64, u16), u64> = BTreeMap::new();
        for game in games {
            if cancelled() {
                return Err(OpeningBookGenerationError::Cancelled);
            }
            let mut position = BoardPosition::standard();
            let mut seen = HashSet::new();
            for movement in game.iter().take(Self::MAX_PLIES) {
                let key = position.standard_polyglot_key()?.value();
                if seen.insert(key) {
                    let encoded = OpeningBookRecord::encode(*movement)?;
                    let count = counts.entry((key, encoded)).or_insert(0);
                    *count = count
                        .checked_add(1)
                        .ok_or(OpeningBookGenerationError::Count)?;
                }
                position = position.play(*movement)?;
            }
        }
        let mut maximum: BTreeMap<u64, u64> = BTreeMap::new();
        for ((key, _encoded), count) in &counts {
            let entry = maximum.entry(*key).or_insert(0);
            *entry = (*entry).max(*count);
        }
        let mut moves = Vec::with_capacity(counts.len());
        let mut bytes = Vec::with_capacity(counts.len() * OpeningBookRecord::BYTES);
        for ((key, encoded), count) in counts {
            let top = maximum.get(&key).copied().unwrap_or(count);
            let weight = Self::weight(count, top)?;
            bytes.extend_from_slice(&key.to_be_bytes());
            bytes.extend_from_slice(&encoded.to_be_bytes());
            bytes.extend_from_slice(&weight.to_be_bytes());
            bytes.extend_from_slice(&0_u32.to_be_bytes());
            moves.push(OpeningBookGeneratedMove {
                key,
                encoded_move: encoded,
                count,
                weight,
            });
        }
        Ok(OpeningBookGenerated { bytes, moves })
    }

    pub fn weight(count: u64, maximum: u64) -> Result<u16, OpeningBookGenerationError> {
        if maximum <= Self::MAX_WEIGHT {
            return u16::try_from(count).map_err(|_error| OpeningBookGenerationError::Count);
        }
        let numerator = count
            .checked_mul(Self::MAX_WEIGHT)
            .and_then(|value| value.checked_mul(2))
            .and_then(|value| value.checked_add(maximum))
            .ok_or(OpeningBookGenerationError::Count)?;
        let denominator = maximum
            .checked_mul(2)
            .ok_or(OpeningBookGenerationError::Count)?;
        let scaled = (numerator / denominator).max(1);
        u16::try_from(scaled).map_err(|_error| OpeningBookGenerationError::Count)
    }
}
