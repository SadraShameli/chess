use std::collections::HashSet;
use std::num::NonZeroUsize;

use thiserror::Error;

use crate::board::enums::{BoardEnumPieceKind, BoardEnumVariant};
use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::BoardPosition;
use crate::opening::corpus::OpeningCorpusGameId;
use crate::storage::opening_repository::{
    StorageOpeningError, StorageOpeningGameIndex, StorageOpeningOccurrence,
    StorageOpeningPositionKey, StorageOpeningSource,
};

#[derive(Debug, Error)]
pub enum OpeningIndexError {
    #[error("The opening index operation was cancelled")]
    Cancelled,
    #[error("The opening index operation exceeded a representable count")]
    Overflow,
    #[error("An opening index depth needs between {minimum} and {maximum} plies")]
    Depth { minimum: u16, maximum: u16 },
    #[error("A batch may hold between 1 and {0} bytes")]
    BatchBytes(usize),
    #[error("One game needs {required} bytes but a batch holds {limit}")]
    GameExceedsBatch { required: usize, limit: usize },
    #[error("The position cannot be keyed: {0}")]
    Position(String),
    #[error(transparent)]
    Move(#[from] BoardMoveError),
    #[error(transparent)]
    Storage(#[from] StorageOpeningError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OpeningIndexDepth(u16);

impl OpeningIndexDepth {
    pub const BASE_PLIES: u16 = 40;
    pub const MAX_PLIES: u16 = StorageOpeningGameIndex::MAX_PLIES;
    pub const MAX_EXTENSION_PLIES: u16 = Self::MAX_PLIES - Self::BASE_PLIES;

    pub const fn base() -> Self {
        Self(Self::BASE_PLIES)
    }

    pub const fn new(plies: u16) -> Result<Self, OpeningIndexError> {
        if plies < Self::BASE_PLIES || plies > Self::MAX_PLIES {
            return Err(OpeningIndexError::Depth {
                minimum: Self::BASE_PLIES,
                maximum: Self::MAX_PLIES,
            });
        }
        Ok(Self(plies))
    }

    pub const fn extended(extension: u16) -> Result<Self, OpeningIndexError> {
        match Self::BASE_PLIES.checked_add(extension) {
            Some(plies) => Self::new(plies),
            None => Err(OpeningIndexError::Overflow),
        }
    }

    pub const fn plies(self) -> u16 {
        self.0
    }
}

pub struct OpeningIndexKey;

impl OpeningIndexKey {
    pub fn of(position: &BoardPosition) -> Result<StorageOpeningPositionKey, OpeningIndexError> {
        let fen = position.fen();
        let mut fields = fen.split_ascii_whitespace();
        let (Some(placement), Some(turn), Some(castling)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return Err(OpeningIndexError::Position(fen));
        };
        let variant = match position.variant() {
            BoardEnumVariant::Standard => "s",
            BoardEnumVariant::Chess960 => "c",
        };
        let en_passant = Self::legal_en_passant(position);
        Ok(StorageOpeningPositionKey::new(format!(
            "{variant}|{placement}|{turn}|{castling}|{en_passant}"
        ))?)
    }

    fn legal_en_passant(position: &BoardPosition) -> String {
        let pawns: Vec<_> = position
            .pieces()
            .filter(|(_, piece)| piece.kind() == BoardEnumPieceKind::Pawn)
            .map(|(square, _)| square)
            .collect();
        let occupied: HashSet<_> = position.pieces().map(|(square, _)| square).collect();
        position
            .legal_moves()
            .into_iter()
            .find_map(|movement| {
                let origin = movement.origin()?;
                let destination = movement.destination();
                let capture = pawns.contains(&origin)
                    && !occupied.contains(&destination)
                    && origin.to_string().as_bytes().first()
                        != destination.to_string().as_bytes().first();
                capture.then(|| destination.to_string())
            })
            .unwrap_or_else(|| "-".to_owned())
    }
}

pub struct OpeningIndexGameInput<'a> {
    pub source: &'a StorageOpeningSource,
    pub id: OpeningCorpusGameId,
    pub initial: &'a BoardPosition,
    pub mainline: &'a [BoardMove],
    pub recorded: Option<OpeningIndexDepth>,
}

pub struct OpeningIndexer;

impl OpeningIndexer {
    pub fn index(
        game: &OpeningIndexGameInput<'_>,
        target: OpeningIndexDepth,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<StorageOpeningGameIndex>, OpeningIndexError> {
        if game.recorded.is_some_and(|recorded| recorded >= target) {
            return Ok(None);
        }
        let floor = game.recorded.map(OpeningIndexDepth::plies);
        let last = game.mainline.len().min(usize::from(target.plies()));
        let mut board = game.initial.clone();
        let mut seen = HashSet::new();
        let mut occurrences = Vec::new();
        for ply in 0..=last {
            if cancelled() {
                return Err(OpeningIndexError::Cancelled);
            }
            let key = OpeningIndexKey::of(&board)?;
            let movement = game.mainline.get(ply).copied();
            if !seen.contains(&key) {
                let ply_number =
                    u16::try_from(ply).map_err(|_error| OpeningIndexError::Overflow)?;
                if floor.is_none_or(|floor| ply_number > floor) {
                    occurrences.push(StorageOpeningOccurrence::new(
                        key.clone(),
                        ply_number,
                        movement.map(|movement| movement.uci(board.variant())),
                    )?);
                }
                seen.insert(key);
            }
            if ply < last
                && let Some(movement) = movement
            {
                board = board.play(movement)?;
            }
        }
        Ok(Some(StorageOpeningGameIndex::new(
            game.source.clone(),
            game.id.value(),
            target.plies(),
            occurrences,
        )?))
    }
}

#[derive(Debug)]
pub struct OpeningIndexBatch {
    games: Vec<StorageOpeningGameIndex>,
    bytes: usize,
}

impl OpeningIndexBatch {
    pub fn games(&self) -> &[StorageOpeningGameIndex] {
        &self.games
    }

    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn into_games(self) -> Vec<StorageOpeningGameIndex> {
        self.games
    }
}

pub struct OpeningIndexBatcher {
    max_bytes: usize,
    games: Vec<StorageOpeningGameIndex>,
    bytes: usize,
}

impl OpeningIndexBatcher {
    pub const MAX_BATCH_BYTES: usize = 128 * 1024 * 1024;

    pub fn new(max_bytes: NonZeroUsize) -> Result<Self, OpeningIndexError> {
        if max_bytes.get() > Self::MAX_BATCH_BYTES {
            return Err(OpeningIndexError::BatchBytes(Self::MAX_BATCH_BYTES));
        }
        Ok(Self {
            max_bytes: max_bytes.get(),
            games: Vec::new(),
            bytes: 0,
        })
    }

    pub fn push(
        &mut self,
        game: StorageOpeningGameIndex,
    ) -> Result<Option<OpeningIndexBatch>, OpeningIndexError> {
        let required = game.transport_bytes();
        if required > self.max_bytes {
            return Err(OpeningIndexError::GameExceedsBatch {
                required,
                limit: self.max_bytes,
            });
        }
        let total = self
            .bytes
            .checked_add(required)
            .ok_or(OpeningIndexError::Overflow)?;
        if total <= self.max_bytes {
            self.games.push(game);
            self.bytes = total;
            return Ok(None);
        }
        let completed = self.take();
        self.games.push(game);
        self.bytes = required;
        Ok(completed)
    }

    pub fn finish(mut self) -> Option<OpeningIndexBatch> {
        self.take()
    }

    fn take(&mut self) -> Option<OpeningIndexBatch> {
        if self.games.is_empty() {
            return None;
        }
        let bytes = std::mem::take(&mut self.bytes);
        Some(OpeningIndexBatch {
            games: std::mem::take(&mut self.games),
            bytes,
        })
    }
}

pub struct OpeningIndexRun;

impl OpeningIndexRun {
    pub fn execute<'a>(
        games: impl IntoIterator<Item = OpeningIndexGameInput<'a>>,
        target: OpeningIndexDepth,
        mut batcher: OpeningIndexBatcher,
        mut cancelled: impl FnMut() -> bool,
        mut sink: impl FnMut(OpeningIndexBatch) -> Result<(), OpeningIndexError>,
    ) -> Result<usize, OpeningIndexError> {
        let mut indexed = 0_usize;
        for game in games {
            if let Some(index) = OpeningIndexer::index(&game, target, &mut cancelled)? {
                indexed = indexed.checked_add(1).ok_or(OpeningIndexError::Overflow)?;
                if let Some(batch) = batcher.push(index)? {
                    sink(batch)?;
                }
            }
        }
        if let Some(batch) = batcher.finish() {
            sink(batch)?;
        }
        Ok(indexed)
    }
}
