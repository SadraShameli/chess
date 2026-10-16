use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::clock::duration::ClockInstant;
use crate::clock::state::{ClockState, ClockStateError};
use crate::desktop::dto::DesktopBoardSnapshot;
use crate::game::draw_claim::GameDrawClaimEnumAvailability;
use crate::game::enums::{GameEnumPhase, GameId, GameMoveId, GameRevision};
use crate::game::outcome::GameOutcome;
use crate::game::pause::GamePauseError;
use crate::game::premove::GamePremoveIntent;
use crate::game::save::{GameSaveEnumStatus, GameSaveError};
use crate::game::session::{
    GameMetadata, GameSeats, GameSession, GameSessionEnumInterruption, GameSessionError,
};
use crate::game::takeback::{GameTakebackEnumConsent, GameTakebackError, GameTakebackPending};
use crate::game::tree::GameTreeError;

#[derive(Debug, Error)]
pub enum DesktopGameSnapshotError {
    #[error("A non-root game node is missing its parent or SAN notation")]
    IncompleteMove,
    #[error(transparent)]
    Tree(#[from] GameTreeError),
    #[error(transparent)]
    Clock(#[from] ClockStateError),
    #[error(transparent)]
    Session(#[from] GameSessionError),
}

#[derive(Debug, Clone)]
pub struct DesktopGameSnapshotMove {
    id: GameMoveId,
    number: u32,
    mover: BoardEnumColor,
    san: String,
}

impl DesktopGameSnapshotMove {
    pub const fn id(&self) -> GameMoveId {
        self.id
    }

    pub const fn number(&self) -> u32 {
        self.number
    }

    pub const fn mover(&self) -> BoardEnumColor {
        self.mover
    }

    pub fn san(&self) -> &str {
        &self.san
    }
}

#[derive(Debug, Clone)]
pub struct DesktopGameSnapshot {
    id: GameId,
    revision: GameRevision,
    captured_at: ClockInstant,
    seats: GameSeats,
    phase: GameEnumPhase,
    outcome: Option<GameOutcome>,
    interruption: Option<GameSessionEnumInterruption>,
    live: GameMoveId,
    root: GameMoveId,
    view: GameMoveId,
    live_turn: BoardEnumColor,
    board: DesktopBoardSnapshot,
    clock: ClockState,
    moves: Vec<DesktopGameSnapshotMove>,
    metadata: GameMetadata,
    takeback_consent: GameTakebackEnumConsent,
    takeback_pending: Option<GameTakebackPending>,
    premove: Option<GamePremoveIntent>,
    premoves_enabled: bool,
    claimable: GameDrawClaimEnumAvailability,
    takeback_target: Option<GameMoveId>,
    save_required: bool,
}

impl DesktopGameSnapshot {
    pub(crate) fn refresh_save_status(&mut self, status: GameSaveEnumStatus) {
        self.save_required = status == GameSaveEnumStatus::Required;
    }
    pub fn capture(
        session: &GameSession,
        now: ClockInstant,
    ) -> Result<Self, DesktopGameSnapshotError> {
        session.clock().view(now)?;
        let tree = session.tree();
        let path = tree.path_to(session.live())?;
        let takeback_target = match session.takeback_target() {
            Ok(target) => Some(target),
            Err(
                GameSessionError::Takeback(GameTakebackError::NoTarget)
                | GameSessionError::WrongPhase(_)
                | GameSessionError::Pause(GamePauseError::RecoveryRequired(_))
                | GameSessionError::Save(GameSaveError::SaveRequired),
            ) => None,
            Err(error) => return Err(error.into()),
        };
        let mut moves = Vec::with_capacity(path.len().saturating_sub(1));
        for &id in path.iter().skip(1) {
            let node = tree.node(id)?;
            let parent = node
                .parent()
                .ok_or(DesktopGameSnapshotError::IncompleteMove)?;
            let parent = tree.node(parent)?;
            moves.push(DesktopGameSnapshotMove {
                id,
                number: parent.position().fullmoves(),
                mover: parent.position().turn(),
                san: node
                    .san()
                    .ok_or(DesktopGameSnapshotError::IncompleteMove)?
                    .to_owned(),
            });
        }
        Ok(Self {
            id: session.id(),
            revision: session.revision(),
            captured_at: now,
            seats: session.seats(),
            phase: session.phase(),
            outcome: session.outcome(),
            interruption: session.interruption(),
            live: session.live(),
            root: tree.root(),
            view: session.view(),
            live_turn: tree.node(session.live())?.position().turn(),
            board: DesktopBoardSnapshot::from_position(tree.node(session.view())?.position()),
            clock: session.clock().clone(),
            moves,
            metadata: session.metadata().clone(),
            takeback_consent: session.takeback().consent(),
            takeback_pending: session.takeback().pending(),
            premove: session.premove().pending(),
            premoves_enabled: session.premove().is_enabled(),
            claimable: session.claimable(),
            takeback_target,
            save_required: session.save_status() == GameSaveEnumStatus::Required,
        })
    }

    pub const fn id(&self) -> GameId {
        self.id
    }

    pub const fn revision(&self) -> GameRevision {
        self.revision
    }

    pub const fn captured_at(&self) -> ClockInstant {
        self.captured_at
    }

    pub const fn seats(&self) -> GameSeats {
        self.seats
    }

    pub const fn phase(&self) -> GameEnumPhase {
        self.phase
    }

    pub const fn outcome(&self) -> Option<GameOutcome> {
        self.outcome
    }

    pub const fn interruption(&self) -> Option<GameSessionEnumInterruption> {
        self.interruption
    }

    pub const fn live(&self) -> GameMoveId {
        self.live
    }

    pub const fn root(&self) -> GameMoveId {
        self.root
    }

    pub const fn claimable(&self) -> GameDrawClaimEnumAvailability {
        self.claimable
    }

    pub const fn takeback_target(&self) -> Option<GameMoveId> {
        self.takeback_target
    }

    pub const fn is_save_required(&self) -> bool {
        self.save_required
    }

    pub const fn view(&self) -> GameMoveId {
        self.view
    }

    pub fn is_viewing_live(&self) -> bool {
        self.live == self.view
    }

    pub const fn live_turn(&self) -> BoardEnumColor {
        self.live_turn
    }

    pub const fn board(&self) -> &DesktopBoardSnapshot {
        &self.board
    }

    pub const fn clock(&self) -> &ClockState {
        &self.clock
    }

    pub fn moves(&self) -> &[DesktopGameSnapshotMove] {
        &self.moves
    }

    pub const fn metadata(&self) -> &GameMetadata {
        &self.metadata
    }

    pub const fn takeback_consent(&self) -> GameTakebackEnumConsent {
        self.takeback_consent
    }

    pub const fn takeback_pending(&self) -> Option<GameTakebackPending> {
        self.takeback_pending
    }

    pub const fn premove(&self) -> Option<GamePremoveIntent> {
        self.premove
    }

    pub const fn are_premoves_enabled(&self) -> bool {
        self.premoves_enabled
    }
}
