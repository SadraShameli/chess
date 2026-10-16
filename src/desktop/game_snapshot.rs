use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::clock::duration::ClockInstant;
use crate::clock::state::{ClockState, ClockStateError};
use crate::desktop::dto::DesktopBoardSnapshot;
use crate::engine::search::position::{EngineSearchPosition, EngineSearchPositionError};
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
    #[error("The selected game history does not reproduce its authoritative position")]
    InconsistentAnalysis,
    #[error(transparent)]
    Analysis(#[from] EngineSearchPositionError),
    #[error(transparent)]
    Tree(#[from] GameTreeError),
    #[error(transparent)]
    Clock(#[from] ClockStateError),
    #[error(transparent)]
    Session(#[from] GameSessionError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopGameSnapshotEnumAnalysisUnavailable {
    LivePlay,
    OversizedHistory,
}

#[derive(Debug, Clone)]
pub struct DesktopGameSnapshotAnalysis {
    node: GameMoveId,
    position: EngineSearchPosition,
}

impl DesktopGameSnapshotAnalysis {
    pub const fn node(&self) -> GameMoveId {
        self.node
    }

    pub const fn position(&self) -> &EngineSearchPosition {
        &self.position
    }
}

#[derive(Debug, Clone)]
pub enum DesktopGameSnapshotEnumAnalysis {
    Available(DesktopGameSnapshotAnalysis),
    Unavailable(DesktopGameSnapshotEnumAnalysisUnavailable),
}

impl DesktopGameSnapshotEnumAnalysis {
    fn capture(session: &GameSession) -> Result<Self, DesktopGameSnapshotError> {
        match session.phase() {
            GameEnumPhase::Completed | GameEnumPhase::Analysis => {}
            GameEnumPhase::Setup | GameEnumPhase::Active | GameEnumPhase::Interrupted => {
                return Ok(Self::Unavailable(
                    DesktopGameSnapshotEnumAnalysisUnavailable::LivePlay,
                ));
            }
        }
        let tree = session.tree();
        let initial = tree.node(tree.root())?.position();
        let path = tree.path_to(session.view())?;
        let mut history = Vec::with_capacity(path.len().saturating_sub(1));
        for &id in path.iter().skip(1) {
            let node = tree.node(id)?;
            let movement = node
                .movement()
                .ok_or(DesktopGameSnapshotError::IncompleteMove)?;
            history.push(movement.uci(initial.variant()));
        }
        match EngineSearchPosition::new(initial.clone(), history) {
            Ok(position) => {
                if position.current() != tree.node(session.view())?.position() {
                    return Err(DesktopGameSnapshotError::InconsistentAnalysis);
                }
                Ok(Self::Available(DesktopGameSnapshotAnalysis {
                    node: session.view(),
                    position,
                }))
            }
            Err(EngineSearchPositionError::OversizedHistory) => Ok(Self::Unavailable(
                DesktopGameSnapshotEnumAnalysisUnavailable::OversizedHistory,
            )),
            Err(error @ EngineSearchPositionError::Move(_)) => Err(error.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DesktopGameSnapshotMove {
    id: GameMoveId,
    number: u32,
    mover: BoardEnumColor,
    san: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopGameSnapshotEnumNotice {
    IllegalPremove {
        revision: GameRevision,
        intent: GamePremoveIntent,
    },
}

impl DesktopGameSnapshotEnumNotice {
    pub const fn revision(self) -> GameRevision {
        match self {
            Self::IllegalPremove { revision, .. } => revision,
        }
    }

    pub const fn message(self) -> &'static str {
        match self {
            Self::IllegalPremove { .. } => "The queued premove is no longer legal and was cleared",
        }
    }
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
    notice: Option<DesktopGameSnapshotEnumNotice>,
    analysis: DesktopGameSnapshotEnumAnalysis,
}

impl DesktopGameSnapshot {
    pub(crate) fn retain_notice(&mut self, notice: Option<DesktopGameSnapshotEnumNotice>) {
        self.notice = self.notice.or(notice);
    }

    pub const fn notice(&self) -> Option<DesktopGameSnapshotEnumNotice> {
        self.notice
    }

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
            notice: None,
            analysis: DesktopGameSnapshotEnumAnalysis::capture(session)?,
        })
    }

    pub const fn id(&self) -> GameId {
        self.id
    }

    pub const fn analysis(&self) -> &DesktopGameSnapshotEnumAnalysis {
        &self.analysis
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
