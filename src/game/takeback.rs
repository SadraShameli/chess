use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::clock::duration::ClockInstant;
use crate::game::enums::{GameMoveId, GameRevision};
use crate::game::pause::GamePauseEnumEngineBarrier;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GameTakebackEnumConsent {
    #[default]
    Required,
    NotRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameTakebackEnumResponse {
    Accept,
    Decline,
    Cancel,
}

#[derive(Debug, Error)]
pub enum GameTakebackError {
    #[error("There is no earlier move available for this mode's takeback")]
    NoTarget,
    #[error("Only the human player can request an engine-game takeback")]
    HumanRequired,
    #[error("A local takeback request is already awaiting a response")]
    AlreadyPending,
    #[error("There is no local takeback request awaiting a response")]
    NotPending,
    #[error("The takeback request no longer targets the current game revision")]
    StaleRequest,
    #[error("Only the other local player can accept or decline this request")]
    OpponentRequired,
    #[error("Only the requesting player can cancel this request")]
    RequesterRequired,
    #[error("The undone move is missing its authoritative pre-move clock snapshot")]
    MissingClockSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameTakebackPending {
    requester: BoardEnumColor,
    live: GameMoveId,
    target: GameMoveId,
    revision: GameRevision,
}

impl GameTakebackPending {
    pub(super) fn new(
        requester: BoardEnumColor,
        live: GameMoveId,
        target: GameMoveId,
        revision: GameRevision,
    ) -> Self {
        Self {
            requester,
            live,
            target,
            revision,
        }
    }

    pub const fn requester(self) -> BoardEnumColor {
        self.requester
    }

    pub const fn live(self) -> GameMoveId {
        self.live
    }

    pub const fn target(self) -> GameMoveId {
        self.target
    }

    pub const fn revision(self) -> GameRevision {
        self.revision
    }

    pub(super) fn validate_response(
        self,
        responder: BoardEnumColor,
        response: GameTakebackEnumResponse,
    ) -> Result<(), GameTakebackError> {
        match response {
            GameTakebackEnumResponse::Accept | GameTakebackEnumResponse::Decline => {
                if responder == self.requester {
                    Err(GameTakebackError::OpponentRequired)
                } else {
                    Ok(())
                }
            }
            GameTakebackEnumResponse::Cancel => {
                if responder == self.requester {
                    Ok(())
                } else {
                    Err(GameTakebackError::RequesterRequired)
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct GameTakeback {
    consent: GameTakebackEnumConsent,
    pending: Option<GameTakebackPending>,
}

impl GameTakeback {
    pub const fn consent(&self) -> GameTakebackEnumConsent {
        self.consent
    }

    pub const fn pending(&self) -> Option<GameTakebackPending> {
        self.pending
    }

    pub(super) fn configure(&mut self, consent: GameTakebackEnumConsent) {
        self.consent = consent;
        self.clear();
    }

    pub(super) fn request(&mut self, pending: GameTakebackPending) {
        self.pending = Some(pending);
    }

    pub(super) fn clear(&mut self) {
        self.pending = None;
    }
}

pub struct GameTakebackRequest {
    pub expected_revision: GameRevision,
    pub requester: BoardEnumColor,
    pub received_at: ClockInstant,
    pub engine: GamePauseEnumEngineBarrier,
}

pub struct GameTakebackResponseRequest {
    pub expected_revision: GameRevision,
    pub responder: BoardEnumColor,
    pub response: GameTakebackEnumResponse,
    pub received_at: ClockInstant,
}
