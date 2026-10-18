use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::board::square::BoardSquare;
use crate::capacity::phase::CapacityPhaseGeneration;
use crate::clock::duration::ClockInstant;
use crate::clock::state::ClockState;
use crate::desktop::identity::DesktopRequestId;
use crate::game::draw_claim::{GameDrawClaimEnumIntent, GameDrawClaimEnumKind};
use crate::game::enums::{GameMoveId, GameRevision};
use crate::game::pause::{GamePauseEnumCause, GamePauseEnumFault};
use crate::game::premove::GamePremoveIntent;
use crate::game::result::GameResultLocalConsent;
use crate::game::takeback::{GameTakebackEnumConsent, GameTakebackEnumResponse};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopGameCommandsEnumAction {
    Start,
    Play {
        node: GameMoveId,
        origin: BoardSquare,
        destination: BoardSquare,
        promotion: Option<BoardEnumPieceKind>,
        color: BoardEnumColor,
    },
    Navigate {
        node: GameMoveId,
    },
    Branch {
        node: GameMoveId,
        origin: BoardSquare,
        destination: BoardSquare,
        promotion: Option<BoardEnumPieceKind>,
    },
    ReturnToLive,
    BeginAnalysis,
    Pause {
        cause: GamePauseEnumCause,
    },
    Resume,
    RequestTakeback {
        requester: BoardEnumColor,
    },
    RespondTakeback {
        responder: BoardEnumColor,
        response: GameTakebackEnumResponse,
    },
    ConfigureTakeback {
        consent: GameTakebackEnumConsent,
    },
    Resign {
        side: BoardEnumColor,
    },
    FinishLocal {
        consent: GameResultLocalConsent,
    },
    ClaimDraw {
        color: BoardEnumColor,
        kind: GameDrawClaimEnumKind,
        intent: GameDrawClaimEnumIntent,
    },
    ConfigurePremove {
        enabled: bool,
    },
    QueuePremove {
        intent: GamePremoveIntent,
    },
    CancelPremove,
    RetryEngine,
    RetrySave,
    RecoverGraphics,
    Stop,
    Quit,
    QuiesceForPreparation {
        generation: CapacityPhaseGeneration,
    },
    ResumeSearches {
        generation: CapacityPhaseGeneration,
    },
}

impl DesktopGameCommandsEnumAction {
    pub fn is_control(&self) -> bool {
        self.control_index().is_some()
    }

    pub(crate) fn is_graphics_lifecycle(&self) -> bool {
        matches!(
            self,
            Self::Pause {
                cause: GamePauseEnumCause::Fault(GamePauseEnumFault::Graphics)
            } | Self::RecoverGraphics
        )
    }

    pub(crate) fn is_rebasable_control(&self) -> bool {
        matches!(self, Self::Quit) || self.is_graphics_lifecycle()
    }

    pub(crate) fn control_index(&self) -> Option<usize> {
        match self {
            Self::Stop => Some(0),
            Self::Pause { .. } => Some(1),
            Self::Quit => Some(2),
            Self::QuiesceForPreparation { .. } => Some(3),
            Self::ResumeSearches { .. } => Some(4),
            Self::RecoverGraphics => Some(5),
            Self::Start
            | Self::Play { .. }
            | Self::Navigate { .. }
            | Self::Branch { .. }
            | Self::ReturnToLive
            | Self::BeginAnalysis
            | Self::Resume
            | Self::RequestTakeback { .. }
            | Self::RespondTakeback { .. }
            | Self::ConfigureTakeback { .. }
            | Self::Resign { .. }
            | Self::FinishLocal { .. }
            | Self::ClaimDraw { .. }
            | Self::ConfigurePremove { .. }
            | Self::QueuePremove { .. }
            | Self::CancelPremove
            | Self::RetryEngine
            | Self::RetrySave => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DesktopGameCommandsRequest {
    pub request_id: DesktopRequestId,
    pub expected_revision: GameRevision,
    pub received_at: ClockInstant,
    pub action: DesktopGameCommandsEnumAction,
}

impl DesktopGameCommandsRequest {
    pub(crate) fn ordered_received_at(&self, clock: &ClockState) -> ClockInstant {
        if self.action.is_graphics_lifecycle() {
            clock.ordered_instant(self.received_at)
        } else {
            self.received_at
        }
    }
}
