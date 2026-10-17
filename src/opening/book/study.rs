use std::fmt::{Display, Formatter};

use crate::board::enums::BoardEnumVariant;
use crate::board::r#move::BoardMove;
use crate::board::position::{BoardPosition, BoardPositionIdentity, BoardPositionPolyglotKey};
use crate::opening::book::record::OpeningBookRecord;
use crate::opening::book::{OpeningBook, OpeningBookError, OpeningBookIdentity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpeningBookShare {
    tenths: u16,
}

impl OpeningBookShare {
    const PERCENT_TENTHS: u64 = 1_000;

    pub const fn tenths(self) -> u16 {
        self.tenths
    }

    fn from_weight(weight: u16, total: u64) -> Result<Option<Self>, OpeningBookError> {
        if total == 0 {
            return Ok(None);
        }
        let numerator = u64::from(weight)
            .checked_mul(Self::PERCENT_TENTHS)
            .and_then(|value| value.checked_add(total / 2))
            .ok_or(OpeningBookError::WeightRange)?;
        let tenths =
            u16::try_from(numerator / total).map_err(|_error| OpeningBookError::WeightRange)?;
        Ok(Some(Self { tenths }))
    }
}

impl Display for OpeningBookShare {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}%", self.tenths / 10, self.tenths % 10)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningBookStudyMove {
    movement: BoardMove,
    uci: String,
    raw_weight: u16,
    learn: u32,
    share: Option<OpeningBookShare>,
}

impl OpeningBookStudyMove {
    pub const fn movement(&self) -> BoardMove {
        self.movement
    }

    pub fn uci(&self) -> &str {
        &self.uci
    }

    pub const fn raw_weight(&self) -> u16 {
        self.raw_weight
    }

    pub const fn learn(&self) -> u32 {
        self.learn
    }

    pub const fn share(&self) -> Option<OpeningBookShare> {
        self.share
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningBookStudy {
    identity: OpeningBookIdentity,
    position: BoardPositionIdentity,
    key: BoardPositionPolyglotKey,
    moves: Vec<OpeningBookStudyMove>,
    rejected_illegal_records: usize,
    total_legal_weight: u64,
}

impl OpeningBookStudy {
    pub(super) fn new(
        identity: &OpeningBookIdentity,
        position: &BoardPosition,
        key: BoardPositionPolyglotKey,
    ) -> Self {
        Self {
            identity: identity.clone(),
            position: position.identity(),
            key,
            moves: Vec::new(),
            rejected_illegal_records: 0,
            total_legal_weight: 0,
        }
    }

    pub const fn identity(&self) -> &OpeningBookIdentity {
        &self.identity
    }

    pub const fn position(&self) -> &BoardPositionIdentity {
        &self.position
    }

    pub const fn key(&self) -> BoardPositionPolyglotKey {
        self.key
    }

    pub fn moves(&self) -> &[OpeningBookStudyMove] {
        &self.moves
    }

    pub const fn rejected_illegal_records(&self) -> usize {
        self.rejected_illegal_records
    }

    pub const fn total_legal_weight(&self) -> u64 {
        self.total_legal_weight
    }

    pub(super) fn push(&mut self, record: OpeningBookRecord, movement: BoardMove) {
        self.moves.push(OpeningBookStudyMove {
            movement,
            uci: movement.uci(BoardEnumVariant::Standard),
            raw_weight: record.weight(),
            learn: record.learn(),
            share: None,
        });
    }

    pub(super) fn reject_illegal(&mut self) {
        self.rejected_illegal_records += 1;
    }

    pub(super) fn finish(
        mut self,
        cancellation: &impl Fn() -> bool,
    ) -> Result<Self, OpeningBookError> {
        for entry in &self.moves {
            OpeningBook::check_cancellation(cancellation)?;
            self.total_legal_weight = self
                .total_legal_weight
                .checked_add(u64::from(entry.raw_weight))
                .ok_or(OpeningBookError::WeightRange)?;
        }
        self.moves.sort_unstable_by(|left, right| {
            right
                .raw_weight
                .cmp(&left.raw_weight)
                .then_with(|| left.uci.cmp(&right.uci))
        });
        for entry in &mut self.moves {
            OpeningBook::check_cancellation(cancellation)?;
            entry.share = OpeningBookShare::from_weight(entry.raw_weight, self.total_legal_weight)?;
        }
        OpeningBook::check_cancellation(cancellation)?;
        Ok(self)
    }
}
