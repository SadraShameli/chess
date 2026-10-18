use thiserror::Error;

use crate::board::r#move::BoardMoveError;
use crate::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use crate::game::adjudication::GameAdjudicationMateProofProvider;
use crate::game::enums::{GameEnumEngineReadiness, GameEnumPhase, GameEnumSeat, GameRevisionError};
use crate::game::pause::{
    GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseEnumFault, GamePauseRequest,
    GameResumeRequest,
};
use crate::game::premove::GamePremoveEnumClearReason;
use crate::game::result::{GameResultLocalRequest, GameResultResignRequest};
use crate::game::save::GameSaveFailureRequest;
use crate::game::session::{
    GameSession, GameSessionDrawClaimRequest, GameSessionEnumInterruption, GameSessionError,
    GameSessionMoveRequest,
};
use crate::game::takeback::{GameTakebackRequest, GameTakebackResponseRequest};
use crate::game::tree::GameTreeError;

pub struct DesktopGameTransitionOptions {
    pub request: DesktopGameCommandsRequest,
    pub engine: GamePauseEnumEngineBarrier,
    pub readiness: GameEnumEngineReadiness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopGameTransitionEnumEffect {
    SnapshotOnly,
    Autosave,
    Quit,
}

#[derive(Debug, Error)]
pub enum DesktopGameTransitionError {
    #[error(transparent)]
    Session(#[from] GameSessionError),
    #[error(transparent)]
    Revision(#[from] GameRevisionError),
    #[error(transparent)]
    Tree(#[from] GameTreeError),
    #[error(transparent)]
    Move(#[from] BoardMoveError),
}

pub struct DesktopGameTransition;

impl DesktopGameTransition {
    pub fn apply<P: GameAdjudicationMateProofProvider + ?Sized>(
        session: &mut GameSession,
        options: DesktopGameTransitionOptions,
        proof: &P,
    ) -> Result<DesktopGameTransitionEnumEffect, DesktopGameTransitionError> {
        let DesktopGameTransitionOptions {
            request,
            engine,
            readiness,
        } = options;
        let received_at = request.ordered_received_at(session.clock());
        let DesktopGameCommandsRequest {
            expected_revision,
            received_at: _,
            action,
            request_id: _,
        } = request;
        session.revision().check_expected(expected_revision)?;
        match action {
            DesktopGameCommandsEnumAction::Start => {
                session.start(expected_revision, received_at, readiness, proof)?;
            }
            DesktopGameCommandsEnumAction::Play {
                node,
                origin,
                destination,
                promotion,
                color,
            } => {
                let movement = session
                    .tree()
                    .node(session.live())?
                    .position()
                    .move_intent(origin, destination, promotion)?;
                session.accept_move(
                    GameSessionMoveRequest {
                        expected_revision,
                        seat: GameEnumSeat::Human,
                        color,
                        node,
                        movement,
                        received_at,
                    },
                    proof,
                )?;
            }
            DesktopGameCommandsEnumAction::Navigate { node } => {
                session.select_view(node)?;
                return Ok(DesktopGameTransitionEnumEffect::SnapshotOnly);
            }
            DesktopGameCommandsEnumAction::Branch {
                node,
                origin,
                destination,
                promotion,
            } => {
                let movement = session
                    .tree()
                    .node(session.view())?
                    .position()
                    .move_intent(origin, destination, promotion)?;
                session.append_study_variation(expected_revision, node, movement)?;
            }
            DesktopGameCommandsEnumAction::ReturnToLive => {
                session.return_to_live();
                return Ok(DesktopGameTransitionEnumEffect::SnapshotOnly);
            }
            DesktopGameCommandsEnumAction::BeginAnalysis => {
                session.begin_analysis(expected_revision, received_at)?;
            }
            DesktopGameCommandsEnumAction::Pause {
                cause:
                    GamePauseEnumCause::AutosaveFailure
                    | GamePauseEnumCause::Fault(GamePauseEnumFault::Save),
            } => {
                session.interrupt_for_save_failure(
                    GameSaveFailureRequest {
                        expected_revision,
                        received_at,
                        engine,
                    },
                    proof,
                )?;
                return Ok(DesktopGameTransitionEnumEffect::SnapshotOnly);
            }
            DesktopGameCommandsEnumAction::Pause {
                cause: GamePauseEnumCause::Fault(GamePauseEnumFault::Graphics),
            } if session.phase() != GameEnumPhase::Active => {
                return Ok(DesktopGameTransitionEnumEffect::SnapshotOnly);
            }
            DesktopGameCommandsEnumAction::Pause { cause } => {
                session.pause(
                    GamePauseRequest {
                        expected_revision,
                        cause,
                        received_at,
                        engine,
                    },
                    proof,
                )?;
            }
            DesktopGameCommandsEnumAction::Resume => {
                session.resume(
                    GameResumeRequest {
                        expected_revision,
                        received_at,
                        readiness,
                    },
                    proof,
                )?;
            }
            DesktopGameCommandsEnumAction::RequestTakeback { requester } => {
                session.request_takeback(
                    GameTakebackRequest {
                        expected_revision,
                        requester,
                        received_at,
                        engine,
                    },
                    proof,
                )?;
            }
            DesktopGameCommandsEnumAction::RespondTakeback {
                responder,
                response,
            } => {
                session.respond_takeback(
                    GameTakebackResponseRequest {
                        expected_revision,
                        responder,
                        response,
                        received_at,
                    },
                    proof,
                )?;
            }
            DesktopGameCommandsEnumAction::ConfigureTakeback { consent } => {
                session.configure_takeback_consent(expected_revision, consent)?;
            }
            DesktopGameCommandsEnumAction::Resign { side } => {
                session.resign(
                    GameResultResignRequest {
                        expected_revision,
                        resigning: side,
                        received_at,
                        engine,
                    },
                    proof,
                )?;
            }
            DesktopGameCommandsEnumAction::FinishLocal { consent } => {
                session.finish_local(
                    GameResultLocalRequest {
                        expected_revision,
                        consent,
                        received_at,
                    },
                    proof,
                )?;
            }
            DesktopGameCommandsEnumAction::ClaimDraw {
                color,
                kind,
                intent,
            } => {
                session.claim_draw(
                    GameSessionDrawClaimRequest {
                        expected_revision,
                        color,
                        kind,
                        intent,
                        received_at,
                    },
                    proof,
                )?;
            }
            DesktopGameCommandsEnumAction::ConfigurePremove { enabled } => {
                session.set_premoves_enabled(expected_revision, enabled)?;
            }
            DesktopGameCommandsEnumAction::QueuePremove { intent } => {
                session.queue_premove(expected_revision, intent)?;
                return Ok(DesktopGameTransitionEnumEffect::SnapshotOnly);
            }
            DesktopGameCommandsEnumAction::CancelPremove => {
                session.clear_premove(expected_revision, GamePremoveEnumClearReason::Explicit)?;
                return Ok(DesktopGameTransitionEnumEffect::SnapshotOnly);
            }
            DesktopGameCommandsEnumAction::RetryEngine => {
                if readiness != GameEnumEngineReadiness::Ready {
                    return Err(GameSessionError::EngineUnavailable.into());
                }
                session.mark_recovered(
                    expected_revision,
                    GamePauseEnumFault::Engine,
                    received_at,
                )?;
            }
            DesktopGameCommandsEnumAction::RetrySave => {}
            DesktopGameCommandsEnumAction::RecoverGraphics => {
                if session.interruption()
                    != Some(GameSessionEnumInterruption::RecoveryRequired(
                        GamePauseEnumFault::Graphics,
                    ))
                {
                    return Ok(DesktopGameTransitionEnumEffect::SnapshotOnly);
                }
                session.mark_recovered(
                    expected_revision,
                    GamePauseEnumFault::Graphics,
                    received_at,
                )?;
            }
            DesktopGameCommandsEnumAction::Stop => {
                if session.phase() != GameEnumPhase::Active {
                    return Ok(DesktopGameTransitionEnumEffect::SnapshotOnly);
                }
                session.pause(
                    GamePauseRequest {
                        expected_revision,
                        cause: GamePauseEnumCause::Manual,
                        received_at,
                        engine,
                    },
                    proof,
                )?;
            }
            DesktopGameCommandsEnumAction::Quit => {
                if session.phase() == GameEnumPhase::Active {
                    session.pause(
                        GamePauseRequest {
                            expected_revision,
                            cause: GamePauseEnumCause::ApplicationQuit,
                            received_at,
                            engine,
                        },
                        proof,
                    )?;
                }
                return Ok(DesktopGameTransitionEnumEffect::Quit);
            }
            DesktopGameCommandsEnumAction::QuiesceForPreparation { .. }
            | DesktopGameCommandsEnumAction::ResumeSearches { .. } => {
                return Ok(DesktopGameTransitionEnumEffect::SnapshotOnly);
            }
        }
        Ok(DesktopGameTransitionEnumEffect::Autosave)
    }
}
