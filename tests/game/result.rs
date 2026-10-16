use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::ClockStateEnumStatus;
use chess::clock::time_control::ClockTimeControl;
use chess::game::adjudication::{
    GameAdjudicationEnumInterruption, GameAdjudicationEnumMateProof,
    GameAdjudicationEnumMateQuestion, GameAdjudicationMateProofProvider,
};
use chess::game::enums::{
    GameEnumOutcomeReason, GameEnumPhase, GameEnumResult, GameEnumTransitionKind,
};
use chess::game::pause::{GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseRequest};
use chess::game::result::{
    GameResultEnumLocalKind, GameResultLocalConsent, GameResultLocalRequest,
    GameResultResignRequest,
};
use chess::game::session::GameSession;
use chess::game::takeback::GameTakebackRequest;

use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

struct GameResultFixture;

impl GameResultFixture {
    fn consent(session: &GameSession, kind: GameResultEnumLocalKind) -> GameResultLocalConsent {
        GameResultLocalConsent::new(session.id(), session.revision(), session.live(), kind)
    }

    fn finish(
        session: &mut GameSession,
        consent: GameResultLocalConsent,
        millis: u64,
    ) -> Result<chess::game::session::GameSessionTransition, Box<dyn std::error::Error>> {
        Ok(session.finish_local(
            GameResultLocalRequest {
                expected_revision: session.revision(),
                consent,
                received_at: ClockInstant::from_millis(millis)?,
            },
            &GameTransitionFixtureProof,
        )?)
    }

    fn resign(
        session: &mut GameSession,
        resigning: BoardEnumColor,
        millis: u64,
    ) -> Result<chess::game::session::GameSessionTransition, Box<dyn std::error::Error>> {
        Ok(session.resign(
            GameResultResignRequest {
                expected_revision: session.revision(),
                resigning,
                received_at: ClockInstant::from_millis(millis)?,
                engine: GamePauseEnumEngineBarrier::NoEngine,
            },
            &GameTransitionFixtureProof,
        )?)
    }
}

#[test]
fn a_human_can_resign_out_of_turn_and_terminal_result_cancels_local_takeback()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    session.request_takeback(
        GameTakebackRequest {
            expected_revision: session.revision(),
            requester: BoardEnumColor::White,
            received_at: ClockInstant::from_millis(2_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let transition = GameResultFixture::resign(&mut session, BoardEnumColor::White, 3_000)?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::ResultAccepted);
    assert_eq!(
        session.outcome().ok_or("missing outcome")?.result(),
        GameEnumResult::Victory(BoardEnumColor::Black)
    );
    assert_eq!(
        session.outcome().ok_or("missing outcome")?.reason(),
        GameEnumOutcomeReason::Resignation
    );
    assert!(session.takeback().pending().is_none());
    assert!(!session.clock().is_running());
    assert!(session.takeback_target().is_err());
    Ok(())
}

#[test]
fn resignation_while_paused_does_not_resume_or_charge_downtime()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Manual,
            received_at: ClockInstant::from_millis(1_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let transition = GameResultFixture::resign(&mut session, BoardEnumColor::Black, 50_000)?;
    assert_eq!(
        transition.clock().elapsed(),
        ClockDuration::from_millis(1_000)?
    );
    assert_eq!(transition.clock().status(), ClockStateEnumStatus::Paused);
    assert_eq!(
        session.outcome().ok_or("missing result")?.result(),
        GameEnumResult::Victory(BoardEnumColor::White)
    );
    let recovered =
        GameSession::restore(session.restoration(ClockInstant::from_millis(100_000)?)?)?;
    assert_eq!(recovered.outcome(), session.outcome());
    Ok(())
}

#[test]
fn native_deadline_precedes_a_new_resignation_or_draw_agreement()
-> Result<(), Box<dyn std::error::Error>> {
    for agreement in [false, true] {
        let mut session = GameTransitionFixture::session(
            ClockTimeControl::sudden_death(ClockDuration::from_millis(1_000)?)?,
            None,
        )?;
        let transition = if agreement {
            let mut consent =
                GameResultFixture::consent(&session, GameResultEnumLocalKind::DrawAgreement);
            consent.confirm(BoardEnumColor::White);
            consent.confirm(BoardEnumColor::Black);
            GameResultFixture::finish(&mut session, consent, 1_000)?
        } else {
            GameResultFixture::resign(&mut session, BoardEnumColor::Black, 1_000)?
        };
        assert_eq!(transition.kind(), GameEnumTransitionKind::FlagFall);
        assert_eq!(
            session.outcome().ok_or("missing result")?.reason(),
            GameEnumOutcomeReason::Flagfall
        );
        assert_eq!(
            session.outcome().ok_or("missing result")?.result(),
            GameEnumResult::Victory(BoardEnumColor::Black)
        );
    }
    Ok(())
}

#[test]
fn local_draw_needs_both_distinct_current_position_confirmations()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let mut consent = GameResultFixture::consent(&session, GameResultEnumLocalKind::DrawAgreement);
    consent.confirm(BoardEnumColor::White);
    consent.confirm(BoardEnumColor::White);
    let revision = session.revision();
    assert!(GameResultFixture::finish(&mut session, consent.clone(), 1_000).is_err());
    assert_eq!(session.revision(), revision);
    assert!(session.clock().is_running());
    consent.confirm(BoardEnumColor::Black);
    assert!(consent.is_confirmed());
    GameResultFixture::finish(&mut session, consent, 2_000)?;
    assert_eq!(
        session.outcome().ok_or("missing draw")?.reason(),
        GameEnumOutcomeReason::Agreement
    );
    assert_eq!(
        session.outcome().ok_or("missing draw")?.result(),
        GameEnumResult::Draw
    );
    Ok(())
}

#[test]
fn local_consent_cannot_be_reused_after_an_accepted_move_or_for_an_engine_game()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let mut consent = GameResultFixture::consent(&session, GameResultEnumLocalKind::DrawAgreement);
    consent.confirm(BoardEnumColor::White);
    consent.confirm(BoardEnumColor::Black);
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    assert!(GameResultFixture::finish(&mut session, consent, 2_000).is_err());
    assert_eq!(session.phase(), GameEnumPhase::Active);
    let mut engine =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::Black))?;
    let mut consent = GameResultFixture::consent(&engine, GameResultEnumLocalKind::DrawAgreement);
    consent.confirm(BoardEnumColor::White);
    consent.confirm(BoardEnumColor::Black);
    assert!(GameResultFixture::finish(&mut engine, consent, 2_000).is_err());
    assert!(GameResultFixture::resign(&mut engine, BoardEnumColor::Black, 2_000).is_err());
    assert!(GameResultFixture::resign(&mut engine, BoardEnumColor::White, 2_000).is_err());
    assert_eq!(engine.phase(), GameEnumPhase::Active);
    Ok(())
}

struct GameResultUnresolvedProof;

impl GameAdjudicationMateProofProvider for GameResultUnresolvedProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Unknown(GameAdjudicationEnumInterruption::StateLimit)
    }
}

#[test]
fn explicit_consensual_result_is_limited_to_unresolved_local_adjudication()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let mut consent = GameResultFixture::consent(
        &session,
        GameResultEnumLocalKind::ConsensualAdjudication(GameEnumResult::Victory(
            BoardEnumColor::White,
        )),
    );
    consent.confirm(BoardEnumColor::White);
    consent.confirm(BoardEnumColor::Black);
    assert!(GameResultFixture::finish(&mut session, consent, 1_000).is_err());
    let position = session.tree().node(session.live())?.position();
    let request = chess::game::session::GameSessionMoveRequest {
        expected_revision: session.revision(),
        seat: chess::game::enums::GameEnumSeat::Human,
        color: BoardEnumColor::White,
        node: GameTransitionFixture::id(2)?,
        movement: position.parse_uci("e2e4")?,
        received_at: ClockInstant::from_millis(1_000)?,
    };
    session.accept_move(request, &GameResultUnresolvedProof)?;
    let mut consent = GameResultFixture::consent(
        &session,
        GameResultEnumLocalKind::ConsensualAdjudication(GameEnumResult::Victory(
            BoardEnumColor::White,
        )),
    );
    consent.confirm(BoardEnumColor::White);
    consent.confirm(BoardEnumColor::Black);
    GameResultFixture::finish(&mut session, consent, 2_000)?;
    assert_eq!(
        session.outcome().ok_or("missing result")?.reason(),
        GameEnumOutcomeReason::ConsensualAdjudication
    );
    assert_eq!(
        session.outcome().ok_or("missing result")?.result(),
        GameEnumResult::Victory(BoardEnumColor::White)
    );
    let restored = GameSession::restore(session.restoration(ClockInstant::from_millis(3_000)?)?)?;
    assert_eq!(restored.outcome(), session.outcome());
    Ok(())
}

#[test]
fn unresolved_flagfall_can_finish_by_explicit_local_consensus_preserving_clock_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(
        ClockTimeControl::sudden_death(ClockDuration::from_millis(1_000)?)?,
        None,
    )?;
    session.poll_deadline(
        session.revision(),
        ClockInstant::from_millis(1_000)?,
        &GameResultUnresolvedProof,
    )?;
    assert_eq!(session.phase(), GameEnumPhase::Interrupted);
    assert!(matches!(
        session.clock().status(),
        ClockStateEnumStatus::Flagged(BoardEnumColor::White)
    ));
    let mut consent = GameResultFixture::consent(
        &session,
        GameResultEnumLocalKind::ConsensualAdjudication(GameEnumResult::Draw),
    );
    consent.confirm(BoardEnumColor::White);
    consent.confirm(BoardEnumColor::Black);
    GameResultFixture::finish(&mut session, consent, 2_000)?;
    assert_eq!(
        session.outcome().ok_or("missing result")?.reason(),
        GameEnumOutcomeReason::ConsensualAdjudication
    );
    assert!(matches!(
        session.clock().status(),
        ClockStateEnumStatus::Flagged(BoardEnumColor::White)
    ));
    let restored = GameSession::restore(session.restoration(ClockInstant::from_millis(3_000)?)?)?;
    assert_eq!(restored.outcome(), session.outcome());
    Ok(())
}
