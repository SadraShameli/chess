use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::desktop::game_transition::{
    DesktopGameTransition, DesktopGameTransitionEnumEffect, DesktopGameTransitionOptions,
};
use chess::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use chess::game::draw_claim::{
    GameDrawClaimEnumAvailability, GameDrawClaimEnumIntent, GameDrawClaimEnumKind,
};
use chess::game::enums::{
    GameEnumEngineReadiness, GameEnumMode, GameEnumPhase, GameEnumSeat, GameMoveId,
};
use chess::game::pause::{GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseEnumFault};
use chess::game::premove::GamePremoveIntent;
use chess::game::result::{GameResultEnumLocalKind, GameResultLocalConsent};
use chess::game::save::{GameSaveEnumStatus, GameSaveFailureRequest};
use chess::game::session::{
    GameMetadata, GameSeats, GameSession, GameSessionEnumInterruption, GameSessionOptions,
};
use chess::game::takeback::GameTakebackEnumResponse;

struct DesktopGameTransitionFixture;

impl GameAdjudicationMateProofProvider for DesktopGameTransitionFixture {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

impl DesktopGameTransitionFixture {
    fn id(value: u128) -> Result<GameMoveId, Box<dyn std::error::Error>> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn setup(engine: bool, start: BoardStart) -> Result<GameSession, Box<dyn std::error::Error>> {
        Ok(GameSession::new(GameSessionOptions {
            id: uuid::Uuid::from_u128(100).to_string().parse()?,
            root: Self::id(1)?,
            start,
            seats: GameSeats::new(
                if engine {
                    GameEnumMode::HumanVersusEngine
                } else {
                    GameEnumMode::LocalTwoPlayer
                },
                GameEnumSeat::Human,
                if engine {
                    GameEnumSeat::Reckless
                } else {
                    GameEnumSeat::Human
                },
            )?,
            clock: ClockState::new(
                ClockTimeControl::sudden_death(ClockDuration::from_millis(60_000)?)?,
                BoardEnumColor::White,
            ),
            metadata: GameMetadata::default(),
        })?)
    }

    fn submit(
        session: &mut GameSession,
        action: DesktopGameCommandsEnumAction,
        millis: u64,
    ) -> Result<DesktopGameTransitionEnumEffect, Box<dyn std::error::Error>> {
        Ok(DesktopGameTransition::apply(
            session,
            DesktopGameTransitionOptions {
                request: DesktopGameCommandsRequest {
                    request_id: uuid::Uuid::from_u128(200).to_string().parse()?,
                    expected_revision: session.revision(),
                    received_at: ClockInstant::from_millis(millis)?,
                    action,
                },
                engine: GamePauseEnumEngineBarrier::NoEngine,
                readiness: GameEnumEngineReadiness::Ready,
            },
            &Self,
        )?)
    }

    fn session(engine: bool) -> Result<GameSession, Box<dyn std::error::Error>> {
        let mut session = Self::setup(engine, BoardStart::standard())?;
        assert_eq!(
            Self::submit(&mut session, DesktopGameCommandsEnumAction::Start, 0)?,
            DesktopGameTransitionEnumEffect::Autosave
        );
        Ok(session)
    }

    fn play(
        session: &mut GameSession,
        value: u128,
        origin: &str,
        destination: &str,
        millis: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let color = session.tree().node(session.live())?.position().turn();
        assert_eq!(
            Self::submit(
                session,
                DesktopGameCommandsEnumAction::Play {
                    node: Self::id(value)?,
                    origin: origin.parse()?,
                    destination: destination.parse()?,
                    promotion: None,
                    color,
                },
                millis
            )?,
            DesktopGameTransitionEnumEffect::Autosave
        );
        Ok(())
    }
}

#[test]
fn native_play_uses_authoritative_position_and_navigation_never_changes_live_clock()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameTransitionFixture::session(false)?;
    let root = session.live();
    DesktopGameTransitionFixture::play(&mut session, 2, "e2", "e4", 1_000)?;
    let live = session.live();
    let revision = session.revision();
    let clock = session.clock().clone();
    assert_eq!(
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::Navigate { node: root },
            2_000
        )?,
        DesktopGameTransitionEnumEffect::SnapshotOnly
    );
    assert_eq!(session.live(), live);
    assert_eq!(session.view(), root);
    assert_eq!(session.revision(), revision);
    assert_eq!(session.clock(), &clock);
    DesktopGameTransitionFixture::play(&mut session, 3, "e7", "e5", 3_000)?;
    assert_eq!(session.tree().node(session.live())?.parent(), Some(live));
    assert_eq!(session.view(), root);
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::ReturnToLive,
        4_000,
    )?;
    assert_eq!(session.view(), session.live());
    Ok(())
}

#[test]
fn stale_requests_illegal_input_and_engine_seat_submission_leave_session_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameTransitionFixture::session(true)?;
    let revision = session.revision();
    assert!(
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::Play {
                node: DesktopGameTransitionFixture::id(2)?,
                origin: "e2".parse()?,
                destination: "e5".parse()?,
                promotion: None,
                color: BoardEnumColor::White,
            },
            1_000
        )
        .is_err()
    );
    assert_eq!(session.revision(), revision);
    DesktopGameTransitionFixture::play(&mut session, 2, "e2", "e4", 2_000)?;
    assert!(
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::Play {
                node: DesktopGameTransitionFixture::id(3)?,
                origin: "e7".parse()?,
                destination: "e5".parse()?,
                promotion: None,
                color: BoardEnumColor::Black,
            },
            3_000
        )
        .is_err()
    );
    let current = session.revision();
    assert!(
        DesktopGameTransition::apply(
            &mut session,
            DesktopGameTransitionOptions {
                request: DesktopGameCommandsRequest {
                    request_id: uuid::Uuid::from_u128(200).to_string().parse()?,
                    expected_revision: revision,
                    received_at: ClockInstant::from_millis(4_000)?,
                    action: DesktopGameCommandsEnumAction::ReturnToLive,
                },
                engine: GamePauseEnumEngineBarrier::NoEngine,
                readiness: GameEnumEngineReadiness::Ready,
            },
            &DesktopGameTransitionFixture
        )
        .is_err()
    );
    assert_eq!(session.revision(), current);
    assert_eq!(session.tree().len(), 2);
    Ok(())
}

#[test]
fn all_four_promotion_choices_cross_native_transition_without_defaulting_to_queen()
-> Result<(), Box<dyn std::error::Error>> {
    for promotion in [
        BoardEnumPieceKind::Knight,
        BoardEnumPieceKind::Bishop,
        BoardEnumPieceKind::Rook,
        BoardEnumPieceKind::Queen,
    ] {
        let mut session = DesktopGameTransitionFixture::setup(
            false,
            BoardStart::custom(BoardPosition::from_fen(
                "7k/P7/8/8/8/8/8/7K w - - 0 1",
                chess::board::enums::BoardEnumVariant::Standard,
            )?),
        )?;
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::Start,
            0,
        )?;
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::Play {
                node: DesktopGameTransitionFixture::id(2)?,
                origin: "a7".parse()?,
                destination: "a8".parse()?,
                promotion: Some(promotion),
                color: BoardEnumColor::White,
            },
            1_000,
        )?;
        assert_eq!(
            session
                .tree()
                .node(session.live())?
                .movement()
                .ok_or("missing promotion")?
                .promotion(),
            Some(promotion)
        );
    }
    Ok(())
}

#[test]
fn local_takeback_and_confirmed_agreement_use_domain_consent()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameTransitionFixture::session(false)?;
    let root = session.live();
    DesktopGameTransitionFixture::play(&mut session, 2, "e2", "e4", 1_000)?;
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::RequestTakeback {
            requester: BoardEnumColor::White,
        },
        2_000,
    )?;
    assert!(session.clock().is_running());
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::RespondTakeback {
            responder: BoardEnumColor::Black,
            response: GameTakebackEnumResponse::Accept,
        },
        3_000,
    )?;
    assert_eq!(session.live(), root);
    assert_eq!(session.tree().len(), 2);
    let mut consent = GameResultLocalConsent::new(
        session.id(),
        session.revision(),
        session.live(),
        GameResultEnumLocalKind::DrawAgreement,
    );
    assert!(
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::FinishLocal {
                consent: consent.clone()
            },
            4_000
        )
        .is_err()
    );
    consent.confirm(BoardEnumColor::White);
    consent.confirm(BoardEnumColor::Black);
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::FinishLocal { consent },
        4_000,
    )?;
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    assert!(!session.clock().is_running());
    Ok(())
}

#[test]
fn stop_pause_resume_and_quit_preserve_explicit_native_transitions()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameTransitionFixture::session(false)?;
    DesktopGameTransitionFixture::submit(&mut session, DesktopGameCommandsEnumAction::Stop, 1_000)?;
    assert_eq!(
        session.interruption(),
        Some(GameSessionEnumInterruption::Paused(
            GamePauseEnumCause::Manual
        ))
    );
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::Resume,
        2_000,
    )?;
    assert!(session.clock().is_running());
    assert_eq!(
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::Quit,
            3_000
        )?,
        DesktopGameTransitionEnumEffect::Quit
    );
    assert_eq!(
        session.interruption(),
        Some(GameSessionEnumInterruption::Paused(
            GamePauseEnumCause::ApplicationQuit
        ))
    );
    assert!(!session.clock().is_running());
    let revision = session.revision();
    assert_eq!(
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::Quit,
            4_000
        )?,
        DesktopGameTransitionEnumEffect::Quit
    );
    assert_eq!(session.revision(), revision);
    Ok(())
}

#[test]
fn adjudication_retry_rechecks_flagfall_and_rejects_stale_revisions_without_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::game_transition_fixture::GameTransitionFixtureUnresolvedProof;
    use chess::clock::state::ClockStateEnumStatus;
    use chess::desktop::game_transition::DesktopGameTransitionError;
    use chess::game::enums::{GameEnumOutcomeReason, GameEnumResult};

    let proof = GameTransitionFixtureUnresolvedProof::new(GameAdjudicationEnumMateQuestion::Side(
        BoardEnumColor::Black,
    ));
    let mut session = DesktopGameTransitionFixture::session(false)?;
    session.poll_deadline(
        session.revision(),
        ClockInstant::from_millis(60_000)?,
        &proof,
    )?;
    let before = DesktopGameSnapshot::capture(&session, ClockInstant::from_millis(60_000)?)?;
    assert_eq!(
        before.clock().status(),
        ClockStateEnumStatus::Flagged(BoardEnumColor::White)
    );
    let options = |expected_revision, received_at| DesktopGameTransitionOptions {
        request: DesktopGameCommandsRequest {
            request_id: chess::desktop::identity::DesktopRequestId::new(),
            expected_revision,
            received_at,
            action: DesktopGameCommandsEnumAction::Resume,
        },
        engine: GamePauseEnumEngineBarrier::NoEngine,
        readiness: GameEnumEngineReadiness::NotRequired,
    };
    for (attempt, millis) in [(2, 120_000), (3, 180_000)] {
        let revision = session.revision();
        let now = ClockInstant::from_millis(millis)?;
        assert_eq!(
            DesktopGameTransition::apply(&mut session, options(revision, now), &proof)?,
            DesktopGameTransitionEnumEffect::Autosave
        );
        assert_eq!(session.revision(), revision.next()?);
        assert_eq!(session.phase(), GameEnumPhase::Interrupted);
        assert_eq!(session.interruption(), before.interruption());
        assert_eq!(session.outcome(), None);
        assert_eq!(session.live(), before.live());
        assert_eq!(
            session.tree().node(session.live())?.position().fen(),
            before.board().fen()
        );
        assert_eq!(session.clock(), before.clock());
        assert!(matches!(
            DesktopGameTransition::apply(&mut session, options(revision, now), &proof),
            Err(DesktopGameTransitionError::Revision(_))
        ));
        assert_eq!(proof.calls(), attempt);
        assert_eq!(session.revision(), revision.next()?);
    }
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::Resume,
        240_000,
    )?;
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    let outcome = session.outcome().ok_or("missing proven flagfall result")?;
    assert_eq!(
        outcome.result(),
        GameEnumResult::Victory(BoardEnumColor::Black)
    );
    assert_eq!(outcome.reason(), GameEnumOutcomeReason::Flagfall);
    assert_eq!(session.clock(), before.clock());
    Ok(())
}

#[test]
fn retry_save_requests_persistence_without_clearing_gate_or_resuming()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameTransitionFixture::session(false)?;
    session.interrupt_for_save_failure(
        GameSaveFailureRequest {
            expected_revision: session.revision(),
            received_at: ClockInstant::from_millis(1_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &DesktopGameTransitionFixture,
    )?;
    let revision = session.revision();
    assert_eq!(
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::RetrySave,
            2_000
        )?,
        DesktopGameTransitionEnumEffect::Autosave
    );
    assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    assert_eq!(session.revision(), revision);
    assert!(!session.clock().is_running());
    assert!(
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::Resume,
            3_000
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn premove_configuration_queue_and_cancel_preserve_live_position()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameTransitionFixture::session(true)?;
    DesktopGameTransitionFixture::play(&mut session, 2, "e2", "e4", 1_000)?;
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::ConfigurePremove { enabled: true },
        2_000,
    )?;
    let live = session.live();
    let revision = session.revision();
    assert_eq!(
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::QueuePremove {
                intent: GamePremoveIntent::new("g1".parse()?, "f3".parse()?, None)?
            },
            3_000
        )?,
        DesktopGameTransitionEnumEffect::SnapshotOnly
    );
    assert!(session.premove().pending().is_some());
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::CancelPremove,
        4_000,
    )?;
    assert!(session.premove().pending().is_none());
    assert_eq!(session.live(), live);
    assert_eq!(session.revision(), revision);
    Ok(())
}

#[test]
fn engine_retry_only_accepts_ready_replacement_and_leaves_explicit_resume_required()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameTransitionFixture::session(true)?;
    session.pause(
        chess::game::pause::GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Fault(GamePauseEnumFault::Engine),
            received_at: ClockInstant::from_millis(1_000)?,
            engine: GamePauseEnumEngineBarrier::FailedEngine,
        },
        &DesktopGameTransitionFixture,
    )?;
    let revision = session.revision();
    assert!(
        DesktopGameTransition::apply(
            &mut session,
            DesktopGameTransitionOptions {
                request: DesktopGameCommandsRequest {
                    request_id: uuid::Uuid::from_u128(200).to_string().parse()?,
                    expected_revision: revision,
                    received_at: ClockInstant::from_millis(2_000)?,
                    action: DesktopGameCommandsEnumAction::RetryEngine,
                },
                engine: GamePauseEnumEngineBarrier::NoEngine,
                readiness: GameEnumEngineReadiness::Unavailable,
            },
            &DesktopGameTransitionFixture
        )
        .is_err()
    );
    assert_eq!(session.revision(), revision);
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::RetryEngine,
        3_000,
    )?;
    assert_eq!(
        session.interruption(),
        Some(GameSessionEnumInterruption::Recovered(
            GamePauseEnumFault::Engine
        ))
    );
    assert!(!session.clock().is_running());
    Ok(())
}

#[test]
fn study_branches_cannot_change_live_history_and_reuse_existing_continuations()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameTransitionFixture::session(false)?;
    let root = session.live();
    DesktopGameTransitionFixture::play(&mut session, 2, "e2", "e4", 1_000)?;
    let live = session.live();
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::Resign {
            side: BoardEnumColor::Black,
        },
        2_000,
    )?;
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::BeginAnalysis,
        3_000,
    )?;
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::Navigate { node: root },
        4_000,
    )?;
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::Branch {
            node: DesktopGameTransitionFixture::id(3)?,
            origin: "d2".parse()?,
            destination: "d4".parse()?,
            promotion: None,
        },
        5_000,
    )?;
    assert_eq!(session.live(), live);
    assert_eq!(
        session.tree().node(root)?.children(),
        &[live, DesktopGameTransitionFixture::id(3)?]
    );
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::Navigate { node: root },
        6_000,
    )?;
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::Branch {
            node: DesktopGameTransitionFixture::id(4)?,
            origin: "d2".parse()?,
            destination: "d4".parse()?,
            promotion: None,
        },
        7_000,
    )?;
    assert_eq!(session.tree().len(), 3);
    assert_eq!(session.view(), DesktopGameTransitionFixture::id(3)?);
    Ok(())
}

#[test]
fn explicit_draw_claim_uses_domain_history_and_never_submits_a_move()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameTransitionFixture::setup(
        false,
        BoardStart::custom(BoardPosition::from_fen(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 100 1",
            chess::board::enums::BoardEnumVariant::Standard,
        )?),
    )?;
    DesktopGameTransitionFixture::submit(&mut session, DesktopGameCommandsEnumAction::Start, 0)?;
    DesktopGameTransitionFixture::submit(
        &mut session,
        DesktopGameCommandsEnumAction::ClaimDraw {
            color: BoardEnumColor::White,
            kind: chess::game::draw_claim::GameDrawClaimEnumKind::FiftyMove,
            intent: chess::game::draw_claim::GameDrawClaimEnumIntent::Current,
        },
        1_000,
    )?;
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    assert_eq!(session.tree().len(), 1);
    Ok(())
}

#[test]
fn intended_draw_claim_validates_the_snapshot_move_without_adding_it_to_live_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameTransitionFixture::setup(
        false,
        BoardStart::custom(BoardPosition::from_fen(
            "7k/8/5K2/8/8/8/8/R7 w - - 99 51",
            chess::board::enums::BoardEnumVariant::Standard,
        )?),
    )?;
    DesktopGameTransitionFixture::submit(&mut session, DesktopGameCommandsEnumAction::Start, 0)?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    assert_eq!(snapshot.claimable(), GameDrawClaimEnumAvailability::None);
    let movement = snapshot
        .board()
        .legal_moves()
        .iter()
        .find(|movement| movement.movement().uci(snapshot.board().variant()) == "a1a2")
        .ok_or("Missing intended fifty-move move")?
        .movement();
    let fen = snapshot.board().fen().to_owned();
    let live = session.live();
    let revision = session.revision();
    let clock = session.clock().clone();
    assert!(
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::ClaimDraw {
                color: BoardEnumColor::White,
                kind: GameDrawClaimEnumKind::ThreefoldRepetition,
                intent: GameDrawClaimEnumIntent::Intended(movement),
            },
            500,
        )
        .is_err()
    );
    assert_eq!(session.revision(), revision);
    assert_eq!(session.clock(), &clock);
    assert_eq!(session.phase(), GameEnumPhase::Active);
    assert_eq!(
        DesktopGameTransitionFixture::submit(
            &mut session,
            DesktopGameCommandsEnumAction::ClaimDraw {
                color: BoardEnumColor::White,
                kind: GameDrawClaimEnumKind::FiftyMove,
                intent: GameDrawClaimEnumIntent::Intended(movement),
            },
            1000,
        )?,
        DesktopGameTransitionEnumEffect::Autosave
    );
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    assert_eq!(session.live(), live);
    assert_eq!(session.tree().len(), 1);
    assert_eq!(session.tree().node(live)?.position().fen(), fen);
    assert_eq!(
        session
            .outcome()
            .ok_or("Missing claimed draw result")?
            .reason(),
        GameDrawClaimEnumKind::FiftyMove.reason()
    );
    assert!(!session.clock().is_running());
    Ok(())
}
