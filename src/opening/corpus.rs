use std::num::NonZeroU64;

use thiserror::Error;

use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::BoardPosition;
use crate::game::session::{GameMetadata, GameMetadataText};
use crate::notation::game_identity::{
    NotationGameIdentity, NotationGameIdentityEnumResult, NotationGameIdentityError,
};
use crate::storage::dataset_package::StorageDatasetPackageSourceIdentity;

mod limits;

pub(crate) use crate::opening::corpus::limits::OpeningCorpusBudget;
pub use crate::opening::corpus::limits::OpeningCorpusLimits;

#[derive(Debug, Error)]
pub enum OpeningCorpusError {
    #[error("A source game identifier must be nonzero")]
    GameId,
    #[error("A source contains duplicate game identifier {0:?}")]
    DuplicateGameId(OpeningCorpusGameId),
    #[error("The selected source identity is repeated or bound to conflicting bytes")]
    SourceBinding,
    #[error("The opening operation was cancelled")]
    Cancelled,
    #[error("The opening operation exhausted its injected work limit")]
    WorkLimit,
    #[error("The opening operation needs {required} reserved bytes but its limit is {limit}")]
    MemoryLimit { required: usize, limit: usize },
    #[error("The opening operation needs {actual} records but its limit is {limit}")]
    RecordLimit { actual: usize, limit: usize },
    #[error("The source game has {actual} plies but its limit is {limit}")]
    PlyLimit { actual: usize, limit: usize },
    #[error("The opening operation exceeded a representable count")]
    Overflow,
    #[error("Cannot allocate bounded opening storage: {0}")]
    Allocation(#[from] std::collections::TryReserveError),
    #[error(transparent)]
    Identity(#[from] NotationGameIdentityError),
    #[error(transparent)]
    Move(#[from] BoardMoveError),
    #[error("A date interval requires complete ordered calendar dates")]
    DateInterval,
    #[error("A rating range requires an ordered lower and upper bound")]
    RatingRange,
    #[error("A player filter requires an available canonical player name")]
    PlayerFilter,
    #[error("A time-control filter requires an available source tag")]
    TimeControlFilter,
    #[error("The sample offset exceeds the matching game count")]
    SampleOffset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpeningCorpusGameId(NonZeroU64);

impl OpeningCorpusGameId {
    pub fn new(value: u64) -> Result<Self, OpeningCorpusError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(OpeningCorpusError::GameId)
    }

    pub const fn value(self) -> u64 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningCorpusEnumMetadata {
    Players,
    Date,
    Ratings,
    TimeControl,
    Result,
}

impl OpeningCorpusEnumMetadata {
    const fn bit(self) -> u8 {
        match self {
            Self::Players => 1,
            Self::Date => 2,
            Self::Ratings => 4,
            Self::TimeControl => 8,
            Self::Result => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpeningCorpusMetadataSupport(u8);

impl OpeningCorpusMetadataSupport {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(
        OpeningCorpusEnumMetadata::Players.bit()
            | OpeningCorpusEnumMetadata::Date.bit()
            | OpeningCorpusEnumMetadata::Ratings.bit()
            | OpeningCorpusEnumMetadata::TimeControl.bit()
            | OpeningCorpusEnumMetadata::Result.bit(),
    );

    pub const fn with(self, field: OpeningCorpusEnumMetadata) -> Self {
        Self(self.0 | field.bit())
    }

    pub const fn supports(self, field: OpeningCorpusEnumMetadata) -> bool {
        self.0 & field.bit() != 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningCorpusEnumValidation {
    Empty,
    LegalMainlines,
}

pub struct OpeningCorpusRecordInput<'a> {
    pub id: OpeningCorpusGameId,
    pub initial: &'a BoardPosition,
    pub mainline: &'a [BoardMove],
    pub metadata: &'a GameMetadata,
    pub result: Option<NotationGameIdentityEnumResult>,
    pub time_control: Option<&'a GameMetadataText>,
}

#[derive(Debug)]
pub struct OpeningCorpusRecord<'a> {
    input: OpeningCorpusRecordInput<'a>,
    identity: NotationGameIdentity,
    reserved_bytes: usize,
}

impl std::fmt::Debug for OpeningCorpusRecordInput<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpeningCorpusRecordInput")
            .field("id", &self.id)
            .field("plies", &self.mainline.len())
            .finish_non_exhaustive()
    }
}

impl<'a> OpeningCorpusRecord<'a> {
    pub fn new(
        input: OpeningCorpusRecordInput<'a>,
        limits: OpeningCorpusLimits,
        cancelled: impl FnMut() -> bool,
    ) -> Result<Self, OpeningCorpusError> {
        let mut budget = OpeningCorpusBudget::new(limits, cancelled);
        budget.checkpoint()?;
        limits.validate_plies(input.mainline.len())?;
        let known = input.metadata.known();
        let player_bytes = known
            .white
            .as_ref()
            .map_or(0, |name| name.as_str().len())
            .checked_add(known.black.as_ref().map_or(0, |name| name.as_str().len()))
            .ok_or(OpeningCorpusError::Overflow)?;
        let move_bytes = input
            .mainline
            .len()
            .checked_mul(
                std::mem::size_of::<String>()
                    + OpeningCorpusLimits::CANONICAL_MOVE_RESERVATION_BYTES,
            )
            .ok_or(OpeningCorpusError::Overflow)?;
        let reserved_bytes = OpeningCorpusLimits::canonical_text_bytes(player_bytes)?
            .checked_add(move_bytes)
            .and_then(|bytes| {
                bytes.checked_add(BoardPosition::MAX_FEN_BYTES + std::mem::size_of::<Self>())
            })
            .ok_or(OpeningCorpusError::Overflow)?;
        budget.charge(reserved_bytes)?;
        let identity =
            NotationGameIdentity::new(input.initial, input.mainline, input.metadata, input.result)?;
        budget.checkpoint()?;
        Ok(Self {
            input,
            identity,
            reserved_bytes,
        })
    }

    pub const fn id(&self) -> OpeningCorpusGameId {
        self.input.id
    }
    pub const fn initial(&self) -> &BoardPosition {
        self.input.initial
    }
    pub const fn mainline(&self) -> &[BoardMove] {
        self.input.mainline
    }
    pub const fn metadata(&self) -> &GameMetadata {
        self.input.metadata
    }
    pub const fn identity(&self) -> &NotationGameIdentity {
        &self.identity
    }
    pub const fn result(&self) -> Option<NotationGameIdentityEnumResult> {
        self.input.result
    }
    pub const fn time_control(&self) -> Option<&GameMetadataText> {
        self.input.time_control
    }
}

#[derive(Debug)]
pub struct OpeningCorpus<'a> {
    source: &'a StorageDatasetPackageSourceIdentity,
    supported: OpeningCorpusMetadataSupport,
    records: Vec<OpeningCorpusRecord<'a>>,
}

impl<'a> OpeningCorpus<'a> {
    pub fn new(
        source: &'a StorageDatasetPackageSourceIdentity,
        supported: OpeningCorpusMetadataSupport,
        records: Vec<OpeningCorpusRecord<'a>>,
        limits: OpeningCorpusLimits,
        cancelled: impl FnMut() -> bool,
    ) -> Result<Self, OpeningCorpusError> {
        let mut budget = OpeningCorpusBudget::new(limits, cancelled);
        budget.checkpoint()?;
        limits.validate_records(records.len())?;
        budget.charge(
            records
                .capacity()
                .checked_mul(std::mem::size_of::<OpeningCorpusRecord<'_>>())
                .ok_or(OpeningCorpusError::Overflow)?,
        )?;
        let mut ids = Vec::new();
        budget.reserve(&mut ids, records.len())?;
        for record in &records {
            budget.checkpoint()?;
            limits.validate_plies(record.mainline().len())?;
            budget.charge(record.reserved_bytes)?;
            ids.push(record.id());
        }
        budget.sort(&mut ids, Ord::cmp)?;
        for pair in ids.windows(2) {
            budget.checkpoint()?;
            if pair[0] == pair[1] {
                return Err(OpeningCorpusError::DuplicateGameId(pair[0]));
            }
        }
        Ok(Self {
            source,
            supported,
            records,
        })
    }

    pub const fn source(&self) -> &StorageDatasetPackageSourceIdentity {
        self.source
    }
    pub const fn supported_metadata(&self) -> OpeningCorpusMetadataSupport {
        self.supported
    }
    pub fn records(&self) -> &[OpeningCorpusRecord<'a>] {
        &self.records
    }
    pub fn validation(&self) -> OpeningCorpusEnumValidation {
        if self.records.is_empty() {
            OpeningCorpusEnumValidation::Empty
        } else {
            OpeningCorpusEnumValidation::LegalMainlines
        }
    }
}
