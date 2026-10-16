use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::clock::duration::ClockInstant;
use crate::game::enums::{GameEnumResult, GameId, GameMoveId, GameRevision};
use crate::game::pause::GamePauseEnumEngineBarrier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameResultEnumLocalKind {
    DrawAgreement,
    ConsensualAdjudication(GameEnumResult),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum GameResultEnumConfirmations {
    #[default]
    Neither,
    White,
    Black,
    Both,
}

#[derive(Debug, Error)]
pub enum GameResultError {
    #[error(
        "A draw agreement or consensual result is available only between two local human players"
    )]
    LocalPlayersRequired,
    #[error("Both local players must explicitly confirm this result")]
    ConfirmationsRequired,
    #[error("The consent belongs to a different game, position or revision")]
    StaleConsent,
    #[error("An explicit consensual result can resolve only an interrupted adjudication")]
    AdjudicationInterruptionRequired,
    #[error("An unresolved flagfall requires Retry or an explicit consensual adjudication")]
    ConsensualResultRequired,
}

#[derive(Debug, Clone)]
pub struct GameResultLocalConsent {
    game: GameId,
    revision: GameRevision,
    live: GameMoveId,
    kind: GameResultEnumLocalKind,
    confirmations: GameResultEnumConfirmations,
}

impl GameResultLocalConsent {
    pub const fn new(
        game: GameId,
        revision: GameRevision,
        live: GameMoveId,
        kind: GameResultEnumLocalKind,
    ) -> Self {
        Self {
            game,
            revision,
            live,
            kind,
            confirmations: GameResultEnumConfirmations::Neither,
        }
    }

    pub const fn game(&self) -> GameId {
        self.game
    }

    pub const fn revision(&self) -> GameRevision {
        self.revision
    }

    pub const fn live(&self) -> GameMoveId {
        self.live
    }

    pub const fn kind(&self) -> GameResultEnumLocalKind {
        self.kind
    }

    pub const fn is_confirmed(&self) -> bool {
        matches!(self.confirmations, GameResultEnumConfirmations::Both)
    }

    pub fn confirm(&mut self, color: BoardEnumColor) {
        self.confirmations = match (self.confirmations, color) {
            (GameResultEnumConfirmations::Neither, BoardEnumColor::White) => {
                GameResultEnumConfirmations::White
            }
            (GameResultEnumConfirmations::Neither, BoardEnumColor::Black) => {
                GameResultEnumConfirmations::Black
            }
            (GameResultEnumConfirmations::White, BoardEnumColor::Black)
            | (GameResultEnumConfirmations::Black, BoardEnumColor::White) => {
                GameResultEnumConfirmations::Both
            }
            (state @ GameResultEnumConfirmations::White, BoardEnumColor::White)
            | (state @ GameResultEnumConfirmations::Black, BoardEnumColor::Black)
            | (
                state @ GameResultEnumConfirmations::Both,
                BoardEnumColor::White | BoardEnumColor::Black,
            ) => state,
        };
    }

    pub(super) fn validate(
        &self,
        game: GameId,
        revision: GameRevision,
        live: GameMoveId,
    ) -> Result<(), GameResultError> {
        if self.game != game || self.revision != revision || self.live != live {
            return Err(GameResultError::StaleConsent);
        }
        if !self.is_confirmed() {
            return Err(GameResultError::ConfirmationsRequired);
        }
        Ok(())
    }
}

pub struct GameResultResignRequest {
    pub expected_revision: GameRevision,
    pub resigning: BoardEnumColor,
    pub received_at: ClockInstant,
    pub engine: GamePauseEnumEngineBarrier,
}

pub struct GameResultLocalRequest {
    pub expected_revision: GameRevision,
    pub consent: GameResultLocalConsent,
    pub received_at: ClockInstant,
}
