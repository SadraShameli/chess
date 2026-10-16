use thiserror::Error;

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::BoardPosition;
use crate::board::square::BoardSquare;
use crate::game::enums::GameEnumMode;

#[derive(Debug, Error)]
pub enum GamePremoveError {
    #[error("Enable premoves before queueing a move")]
    Disabled,
    #[error("Premoves are available only against Reckless")]
    Mode,
    #[error("Queue a premove during the opponent's turn")]
    HumanTurn,
    #[error("A queued move can execute only on the human player's turn")]
    OpponentTurn,
    #[error("Choose one of the human player's pieces for a premove")]
    Origin,
    #[error("Choose a knight, bishop, rook or queen before queueing promotion")]
    PromotionRequired,
    #[error("Only a pawn moving to its promotion rank can promote")]
    UnexpectedPromotion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GamePremoveIntent {
    origin: BoardSquare,
    destination: BoardSquare,
    promotion: Option<BoardEnumPieceKind>,
}

impl GamePremoveIntent {
    const SQUARES_PER_RANK: u8 = 8;
    const WHITE_PROMOTION_RANK: u8 = 7;
    const BLACK_PROMOTION_RANK: u8 = 0;

    pub fn new(
        origin: BoardSquare,
        destination: BoardSquare,
        promotion: Option<BoardEnumPieceKind>,
    ) -> Result<Self, GamePremoveError> {
        if promotion.is_some_and(|piece| !piece.is_promotion()) {
            return Err(GamePremoveError::PromotionRequired);
        }
        Ok(Self {
            origin,
            destination,
            promotion,
        })
    }

    pub const fn origin(self) -> BoardSquare {
        self.origin
    }

    pub const fn destination(self) -> BoardSquare {
        self.destination
    }

    pub const fn promotion(self) -> Option<BoardEnumPieceKind> {
        self.promotion
    }

    fn validate(
        &self,
        position: &BoardPosition,
        human: BoardEnumColor,
    ) -> Result<(), GamePremoveError> {
        let piece = position
            .pieces()
            .find_map(|(square, piece)| (square == self.origin).then_some(piece))
            .filter(|piece| piece.color() == human)
            .ok_or(GamePremoveError::Origin)?;
        let requires_promotion = Self::requires_promotion(piece.kind(), human, self.destination);
        match (requires_promotion, self.promotion) {
            (true, None) => Err(GamePremoveError::PromotionRequired),
            (false, Some(_)) => Err(GamePremoveError::UnexpectedPromotion),
            (true, Some(_)) | (false, None) => Ok(()),
        }
    }

    pub fn requires_promotion(
        kind: BoardEnumPieceKind,
        color: BoardEnumColor,
        destination: BoardSquare,
    ) -> bool {
        let promotion_rank = match color {
            BoardEnumColor::White => Self::WHITE_PROMOTION_RANK,
            BoardEnumColor::Black => Self::BLACK_PROMOTION_RANK,
        };
        kind == BoardEnumPieceKind::Pawn
            && u8::from(destination) / Self::SQUARES_PER_RANK == promotion_rank
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamePremoveEnumClearReason {
    Explicit,
    Escape,
    Pause,
    Takeback,
    Fault,
    Relaunch,
}

impl GamePremoveEnumClearReason {
    pub const ALL: [Self; 6] = [
        Self::Explicit,
        Self::Escape,
        Self::Pause,
        Self::Takeback,
        Self::Fault,
        Self::Relaunch,
    ];
}

#[derive(Debug)]
pub enum GamePremoveEnumResolution {
    Empty,
    Execute(BoardMove),
    ClearedIllegal {
        intent: GamePremoveIntent,
        source: BoardMoveError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum GamePremoveEnumState {
    #[default]
    Disabled,
    Enabled(Option<GamePremoveIntent>),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GamePremove {
    state: GamePremoveEnumState,
}

impl GamePremove {
    pub const fn is_enabled(&self) -> bool {
        matches!(self.state, GamePremoveEnumState::Enabled(_))
    }

    pub const fn pending(&self) -> Option<GamePremoveIntent> {
        match self.state {
            GamePremoveEnumState::Disabled => None,
            GamePremoveEnumState::Enabled(pending) => pending,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        match (enabled, &mut self.state) {
            (true, state @ GamePremoveEnumState::Disabled) => {
                *state = GamePremoveEnumState::Enabled(None);
            }
            (false, state @ GamePremoveEnumState::Enabled(_)) => {
                *state = GamePremoveEnumState::Disabled;
            }
            (true, GamePremoveEnumState::Enabled(_)) | (false, GamePremoveEnumState::Disabled) => {}
        }
    }

    pub fn queue(
        &mut self,
        position: &BoardPosition,
        mode: GameEnumMode,
        human: BoardEnumColor,
        intent: GamePremoveIntent,
    ) -> Result<Option<GamePremoveIntent>, GamePremoveError> {
        let GamePremoveEnumState::Enabled(pending) = &mut self.state else {
            return Err(GamePremoveError::Disabled);
        };
        match mode {
            GameEnumMode::HumanVersusEngine => {}
            GameEnumMode::LocalTwoPlayer => return Err(GamePremoveError::Mode),
        }
        if position.turn() == human {
            return Err(GamePremoveError::HumanTurn);
        }
        intent.validate(position, human)?;
        Ok(pending.replace(intent))
    }

    pub fn clear(&mut self, _reason: GamePremoveEnumClearReason) -> Option<GamePremoveIntent> {
        match &mut self.state {
            GamePremoveEnumState::Disabled => None,
            GamePremoveEnumState::Enabled(pending) => pending.take(),
        }
    }

    pub fn resolve(
        &mut self,
        position: &BoardPosition,
        human: BoardEnumColor,
    ) -> Result<GamePremoveEnumResolution, GamePremoveError> {
        if self.pending().is_none() {
            return Ok(GamePremoveEnumResolution::Empty);
        }
        if position.turn() != human {
            return Err(GamePremoveError::OpponentTurn);
        }
        let Some(intent) = self.clear(GamePremoveEnumClearReason::Explicit) else {
            return Ok(GamePremoveEnumResolution::Empty);
        };
        Ok(
            match position.move_intent(intent.origin, intent.destination, intent.promotion) {
                Ok(movement) => GamePremoveEnumResolution::Execute(movement),
                Err(source) => GamePremoveEnumResolution::ClearedIllegal { intent, source },
            },
        )
    }
}
