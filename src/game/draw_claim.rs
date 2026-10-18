use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::board::r#move::BoardMove;
use crate::board::position::BoardPosition;
use crate::game::adjudication::{
    GameAdjudicationCandidate, GameAdjudicationEnumDecision, GameAdjudicationError,
    GameAdjudicationHistory,
};
use crate::game::enums::GameEnumOutcomeReason;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameDrawClaimEnumKind {
    ThreefoldRepetition,
    FiftyMove,
}

impl GameDrawClaimEnumKind {
    pub const fn reason(self) -> GameEnumOutcomeReason {
        match self {
            Self::ThreefoldRepetition => GameEnumOutcomeReason::ThreefoldRepetition,
            Self::FiftyMove => GameEnumOutcomeReason::FiftyMove,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameDrawClaimEnumIntent {
    Current,
    Intended(BoardMove),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameDrawClaimEnumAvailability {
    None,
    Threefold,
    FiftyMove,
    Both,
}

impl GameDrawClaimEnumAvailability {
    const CLAIM_REPETITIONS: u32 = 3;
    const CLAIM_HALFMOVES: u32 = 100;

    pub const fn contains(self, kind: GameDrawClaimEnumKind) -> bool {
        match (self, kind) {
            (Self::Both, GameDrawClaimEnumKind::ThreefoldRepetition)
            | (Self::Both, GameDrawClaimEnumKind::FiftyMove) => true,
            (Self::Threefold, GameDrawClaimEnumKind::ThreefoldRepetition)
            | (Self::FiftyMove, GameDrawClaimEnumKind::FiftyMove) => true,
            (Self::None, GameDrawClaimEnumKind::ThreefoldRepetition)
            | (Self::None, GameDrawClaimEnumKind::FiftyMove)
            | (Self::Threefold, GameDrawClaimEnumKind::FiftyMove)
            | (Self::FiftyMove, GameDrawClaimEnumKind::ThreefoldRepetition) => false,
        }
    }

    pub const fn preferred(self) -> Option<GameDrawClaimEnumKind> {
        match self {
            Self::None => None,
            Self::Threefold | Self::Both => Some(GameDrawClaimEnumKind::ThreefoldRepetition),
            Self::FiftyMove => Some(GameDrawClaimEnumKind::FiftyMove),
        }
    }

    fn from_position(
        position: &BoardPosition,
        occurrences: u32,
        terminal: Option<GameAdjudicationEnumDecision>,
    ) -> Self {
        if terminal.is_some() {
            return Self::None;
        }
        match (
            occurrences >= Self::CLAIM_REPETITIONS,
            position.halfmoves() >= Self::CLAIM_HALFMOVES,
        ) {
            (false, false) => Self::None,
            (true, false) => Self::Threefold,
            (false, true) => Self::FiftyMove,
            (true, true) => Self::Both,
        }
    }
}

impl GameAdjudicationHistory {
    pub fn claimable(&self) -> GameDrawClaimEnumAvailability {
        GameDrawClaimEnumAvailability::from_position(
            self.position(),
            self.occurrences(),
            self.direct_terminal(),
        )
    }

    pub fn claim(
        &self,
        claimant: BoardEnumColor,
        kind: GameDrawClaimEnumKind,
        intent: GameDrawClaimEnumIntent,
    ) -> Result<GameAdjudicationEnumDecision, GameDrawClaimError> {
        self.validate_claimant(claimant)?;
        let availability = match intent {
            GameDrawClaimEnumIntent::Current => self.claimable(),
            GameDrawClaimEnumIntent::Intended(movement) => {
                let candidate = self.prepare(movement)?;
                if candidate.direct_terminal().is_some() {
                    return Err(GameDrawClaimError::TerminalPosition);
                }
                candidate.claimable()
            }
        };
        if !availability.contains(kind) {
            return Err(GameDrawClaimError::Unavailable(kind));
        }
        Ok(GameAdjudicationEnumDecision::draw(kind.reason()))
    }

    pub fn reckless_claim(
        &self,
        claimant: BoardEnumColor,
        intended: Option<BoardMove>,
    ) -> Result<Option<GameAdjudicationEnumDecision>, GameDrawClaimError> {
        self.validate_claimant(claimant)?;
        let available = if let Some(kind) = self.claimable().preferred() {
            Some(kind)
        } else if let Some(movement) = intended {
            self.prepare(movement)?.claimable().preferred()
        } else {
            None
        };
        Ok(available.map(|kind| GameAdjudicationEnumDecision::draw(kind.reason())))
    }

    fn validate_claimant(&self, claimant: BoardEnumColor) -> Result<(), GameDrawClaimError> {
        if self.position().turn() != claimant {
            return Err(GameDrawClaimError::WrongTurn);
        }
        if self.direct_terminal().is_some() {
            return Err(GameDrawClaimError::TerminalPosition);
        }
        Ok(())
    }
}

impl GameAdjudicationCandidate {
    pub fn claimable(&self) -> GameDrawClaimEnumAvailability {
        GameDrawClaimEnumAvailability::from_position(
            self.position(),
            self.occurrences(),
            self.direct_terminal(),
        )
    }
}

#[derive(Debug, Error)]
pub enum GameDrawClaimError {
    #[error("Only the side to move can claim a draw")]
    WrongTurn,
    #[error("A terminal position cannot be replaced by a draw claim")]
    TerminalPosition,
    #[error("The requested draw claim is unavailable: {0:?}")]
    Unavailable(GameDrawClaimEnumKind),
    #[error(transparent)]
    Adjudication(#[from] GameAdjudicationError),
}
