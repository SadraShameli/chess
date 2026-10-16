use std::collections::HashMap;

use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::{BoardPosition, BoardPositionIdentity};
use crate::game::enums::{GameEnumOutcomeReason, GameEnumResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameAdjudicationEnumMateQuestion {
    AnySide,
    Side(BoardEnumColor),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameAdjudicationEnumInterruption {
    TimeLimit,
    StateLimit,
    MemoryLimit,
    Cancelled,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameAdjudicationEnumMateProof {
    Possible,
    Impossible,
    Unknown(GameAdjudicationEnumInterruption),
}

pub trait GameAdjudicationMateProofProvider {
    fn prove(
        &self,
        position: &BoardPosition,
        question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameAdjudicationEnumDecision {
    Ongoing,
    Complete {
        result: GameEnumResult,
        reason: GameEnumOutcomeReason,
    },
    Interrupted {
        question: GameAdjudicationEnumMateQuestion,
        cause: GameAdjudicationEnumInterruption,
    },
}

impl GameAdjudicationEnumDecision {
    pub(super) const fn draw(reason: GameEnumOutcomeReason) -> Self {
        Self::Complete {
            result: GameEnumResult::Draw,
            reason,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GameAdjudicationHistory {
    position: BoardPosition,
    counts: HashMap<BoardPositionIdentity, u32>,
    occurrences: u32,
    revision: u64,
}

impl GameAdjudicationHistory {
    pub fn new(position: BoardPosition) -> Self {
        let mut counts = HashMap::new();
        counts.insert(position.identity(), 1);
        Self {
            position,
            counts,
            occurrences: 1,
            revision: 0,
        }
    }

    pub fn from_moves(
        position: BoardPosition,
        movements: impl IntoIterator<Item = BoardMove>,
    ) -> Result<Self, GameAdjudicationError> {
        let mut history = Self::new(position);
        for movement in movements {
            let candidate = history.prepare(movement)?;
            history.commit(candidate)?;
        }
        Ok(history)
    }

    pub fn position(&self) -> &BoardPosition {
        &self.position
    }

    pub const fn occurrences(&self) -> u32 {
        self.occurrences
    }

    pub fn prepare(
        &self,
        movement: BoardMove,
    ) -> Result<GameAdjudicationCandidate, GameAdjudicationError> {
        let position = self.position.play(movement)?;
        let identity = position.identity();
        let previous_occurrences = self.counts.get(&identity).map_or(0, |count| *count);
        let occurrences = previous_occurrences
            .checked_add(1)
            .ok_or(GameAdjudicationError::HistoryCapacity)?;
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(GameAdjudicationError::HistoryCapacity)?;
        Ok(GameAdjudicationCandidate {
            previous_position: self.position.clone(),
            movement,
            position,
            identity,
            previous_occurrences,
            occurrences,
            source_revision: self.revision,
            next_revision,
        })
    }

    pub fn commit(
        &mut self,
        candidate: GameAdjudicationCandidate,
    ) -> Result<(), GameAdjudicationError> {
        if candidate.source_revision != self.revision
            || candidate.previous_position != self.position
            || self
                .counts
                .get(&candidate.identity)
                .map_or(0, |count| *count)
                != candidate.previous_occurrences
        {
            return Err(GameAdjudicationError::StaleCandidate);
        }
        self.counts
            .insert(candidate.identity, candidate.occurrences);
        self.position = candidate.position;
        self.occurrences = candidate.occurrences;
        self.revision = candidate.next_revision;
        Ok(())
    }

    pub fn adjudicate<P: GameAdjudicationMateProofProvider + ?Sized>(
        &self,
        proof: &P,
    ) -> GameAdjudicationEnumDecision {
        self.state().adjudicate(proof)
    }

    pub(super) fn direct_terminal(&self) -> Option<GameAdjudicationEnumDecision> {
        self.state().direct_terminal()
    }

    pub fn flag_fall<P: GameAdjudicationMateProofProvider + ?Sized>(
        &self,
        flagged: BoardEnumColor,
        proof: &P,
    ) -> GameAdjudicationEnumDecision {
        if let Some(decision) = self.state().direct_terminal() {
            return decision;
        }
        let opponent = flagged.opposite();
        let question = GameAdjudicationEnumMateQuestion::Side(opponent);
        match proof.prove(self.position(), question) {
            GameAdjudicationEnumMateProof::Possible => GameAdjudicationEnumDecision::Complete {
                result: GameEnumResult::Victory(opponent),
                reason: GameEnumOutcomeReason::Flagfall,
            },
            GameAdjudicationEnumMateProof::Impossible => {
                GameAdjudicationEnumDecision::draw(GameEnumOutcomeReason::Flagfall)
            }
            GameAdjudicationEnumMateProof::Unknown(cause) => {
                GameAdjudicationEnumDecision::Interrupted { question, cause }
            }
        }
    }

    pub fn resign(&self, resigning: BoardEnumColor) -> GameAdjudicationEnumDecision {
        self.state()
            .direct_terminal()
            .unwrap_or(GameAdjudicationEnumDecision::Complete {
                result: GameEnumResult::Victory(resigning.opposite()),
                reason: GameEnumOutcomeReason::Resignation,
            })
    }

    pub fn agree(
        &self,
        agreement: &GameAdjudicationLocalAgreement,
    ) -> Result<GameAdjudicationEnumDecision, GameAdjudicationError> {
        if !agreement.is_confirmed() {
            return Err(GameAdjudicationError::AgreementUnconfirmed);
        }
        Ok(self
            .state()
            .direct_terminal()
            .unwrap_or(GameAdjudicationEnumDecision::draw(
                GameEnumOutcomeReason::Agreement,
            )))
    }

    fn state(&self) -> GameAdjudicationState<'_> {
        GameAdjudicationState {
            position: &self.position,
            occurrences: self.occurrences,
        }
    }
}

#[derive(Debug)]
pub struct GameAdjudicationCandidate {
    previous_position: BoardPosition,
    movement: BoardMove,
    position: BoardPosition,
    identity: BoardPositionIdentity,
    previous_occurrences: u32,
    occurrences: u32,
    source_revision: u64,
    next_revision: u64,
}

impl GameAdjudicationCandidate {
    pub fn previous_position(&self) -> &BoardPosition {
        &self.previous_position
    }

    pub const fn movement(&self) -> BoardMove {
        self.movement
    }

    pub fn position(&self) -> &BoardPosition {
        &self.position
    }

    pub const fn occurrences(&self) -> u32 {
        self.occurrences
    }

    pub fn adjudicate<P: GameAdjudicationMateProofProvider + ?Sized>(
        &self,
        proof: &P,
    ) -> GameAdjudicationEnumDecision {
        GameAdjudicationState {
            position: &self.position,
            occurrences: self.occurrences,
        }
        .adjudicate(proof)
    }

    pub(super) fn direct_terminal(&self) -> Option<GameAdjudicationEnumDecision> {
        GameAdjudicationState {
            position: &self.position,
            occurrences: self.occurrences,
        }
        .direct_terminal()
    }
}

struct GameAdjudicationState<'a> {
    position: &'a BoardPosition,
    occurrences: u32,
}

impl GameAdjudicationState<'_> {
    const AUTOMATIC_REPETITIONS: u32 = 5;
    const AUTOMATIC_HALFMOVES: u32 = 150;

    fn direct_terminal(&self) -> Option<GameAdjudicationEnumDecision> {
        if self.position.is_checkmate() {
            Some(GameAdjudicationEnumDecision::Complete {
                result: GameEnumResult::Victory(self.position.turn().opposite()),
                reason: GameEnumOutcomeReason::Checkmate,
            })
        } else if self.position.is_stalemate() {
            Some(GameAdjudicationEnumDecision::draw(
                GameEnumOutcomeReason::Stalemate,
            ))
        } else if self.occurrences >= Self::AUTOMATIC_REPETITIONS {
            Some(GameAdjudicationEnumDecision::draw(
                GameEnumOutcomeReason::FivefoldRepetition,
            ))
        } else if self.position.halfmoves() >= Self::AUTOMATIC_HALFMOVES {
            Some(GameAdjudicationEnumDecision::draw(
                GameEnumOutcomeReason::SeventyFiveMove,
            ))
        } else {
            None
        }
    }

    fn adjudicate<P: GameAdjudicationMateProofProvider + ?Sized>(
        &self,
        proof: &P,
    ) -> GameAdjudicationEnumDecision {
        if let Some(decision) = self.direct_terminal() {
            return decision;
        }
        let question = GameAdjudicationEnumMateQuestion::AnySide;
        match proof.prove(self.position, question) {
            GameAdjudicationEnumMateProof::Possible => GameAdjudicationEnumDecision::Ongoing,
            GameAdjudicationEnumMateProof::Impossible => {
                GameAdjudicationEnumDecision::draw(GameEnumOutcomeReason::DeadPosition)
            }
            GameAdjudicationEnumMateProof::Unknown(cause) => {
                GameAdjudicationEnumDecision::Interrupted { question, cause }
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameAdjudicationLocalAgreement {
    white_confirmed: bool,
    black_confirmed: bool,
}

impl GameAdjudicationLocalAgreement {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn confirm(&mut self, color: BoardEnumColor) {
        match color {
            BoardEnumColor::White => self.white_confirmed = true,
            BoardEnumColor::Black => self.black_confirmed = true,
        }
    }

    pub const fn is_confirmed(&self) -> bool {
        self.white_confirmed && self.black_confirmed
    }
}

#[derive(Debug, Error)]
pub enum GameAdjudicationError {
    #[error(transparent)]
    Move(#[from] BoardMoveError),
    #[error("The prepared move no longer matches this game history")]
    StaleCandidate,
    #[error("The game history reached its representable capacity")]
    HistoryCapacity,
    #[error("Both local seats must confirm an agreed draw")]
    AgreementUnconfirmed,
}
