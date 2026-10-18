use chess::board::enums::BoardEnumColor;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::time_control::ClockTimeControl;
use chess::game::enums::{GameEnumPhase, GameEnumTransitionKind};
use chess::game::pause::GamePauseEnumEngineBarrier;
use chess::game::takeback::{
    GameTakebackEnumConsent, GameTakebackEnumResponse, GameTakebackRequest,
    GameTakebackResponseRequest,
};

use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

struct GameTakebackFixture;

impl GameTakebackFixture {
    fn request(
        session: &mut chess::game::session::GameSession,
        requester: BoardEnumColor,
        millis: u64,
    ) -> Result<chess::game::session::GameSessionTransition, Box<dyn std::error::Error>> {
        Ok(session.request_takeback(
            GameTakebackRequest {
                expected_revision: session.revision(),
                requester,
                received_at: ClockInstant::from_millis(millis)?,
                engine: GamePauseEnumEngineBarrier::NoEngine,
            },
            &GameTransitionFixtureProof,
        )?)
    }

    fn respond(
        session: &mut chess::game::session::GameSession,
        responder: BoardEnumColor,
        response: GameTakebackEnumResponse,
        millis: u64,
    ) -> Result<chess::game::session::GameSessionTransition, Box<dyn std::error::Error>> {
        Ok(session.respond_takeback(
            GameTakebackResponseRequest {
                expected_revision: session.revision(),
                responder,
                response,
                received_at: ClockInstant::from_millis(millis)?,
            },
            &GameTransitionFixtureProof,
        )?)
    }
}

#[test]
fn local_takeback_requires_opponent_consent_and_restores_exact_pre_move_clocks()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(
        ClockTimeControl::fischer(
            ClockDuration::from_millis(20_000)?,
            ClockDuration::from_millis(5_000)?,
        )?,
        None,
    )?;
    let root = session.live();
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    let move_id = session.live();
    let before = session
        .tree()
        .node(move_id)?
        .before_clock()
        .ok_or("missing before snapshot")?
        .clone();
    assert_eq!(
        session.takeback().consent(),
        GameTakebackEnumConsent::Required
    );
    let transition = GameTakebackFixture::request(&mut session, BoardEnumColor::White, 2_000)?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::TakebackRequested);
    assert!(session.clock().is_running());
    assert_eq!(session.live(), move_id);
    let pending = session
        .takeback()
        .pending()
        .ok_or("missing consent request")?;
    assert_eq!(pending.target(), root);
    assert_eq!(pending.live(), move_id);
    assert_eq!(pending.revision(), session.revision());
    assert!(
        GameTakebackFixture::respond(
            &mut session,
            BoardEnumColor::White,
            GameTakebackEnumResponse::Accept,
            3_000
        )
        .is_err()
    );
    let transition = GameTakebackFixture::respond(
        &mut session,
        BoardEnumColor::Black,
        GameTakebackEnumResponse::Accept,
        3_000,
    )?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::TakebackAccepted);
    assert_eq!(transition.before_clock(), Some(&before));
    assert_eq!(session.live(), root);
    assert_eq!(session.view(), root);
    assert!(session.clock().is_running());
    assert_eq!(transition.clock().active(), BoardEnumColor::White);
    assert_eq!(transition.clock().completed_moves(BoardEnumColor::White), 0);
    assert_eq!(
        session
            .clock()
            .view(ClockInstant::from_millis(3_000)?)?
            .side(BoardEnumColor::White)
            .remaining(),
        Some(ClockDuration::from_millis(19_000)?)
    );
    assert_eq!(session.tree().node(root)?.children(), &[move_id]);
    assert!(session.takeback().pending().is_none());
    Ok(())
}

#[test]
fn replay_after_takeback_reuses_retained_variation_and_replaces_both_clock_snapshots()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    session.configure_takeback_consent(session.revision(), GameTakebackEnumConsent::NotRequired)?;
    let root = session.live();
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    let original = session.live();
    let old_before = session
        .tree()
        .node(original)?
        .before_clock()
        .ok_or("missing before")?
        .clone();
    GameTakebackFixture::request(&mut session, BoardEnumColor::White, 2_000)?;
    GameTransitionFixture::play(&mut session, 3, "e2e4", 3_000)?;
    assert_eq!(session.live(), original);
    assert_eq!(session.tree().len(), 2);
    assert_eq!(session.tree().node(root)?.children(), &[original]);
    let replayed = session.tree().node(original)?;
    assert_ne!(replayed.before_clock(), Some(&old_before));
    assert_eq!(
        replayed
            .before_clock()
            .ok_or("missing replayed before")?
            .elapsed(),
        ClockDuration::from_millis(2_000)?
    );
    assert_eq!(
        replayed
            .clock()
            .ok_or("missing replayed after")?
            .completed_moves(BoardEnumColor::White),
        1
    );
    Ok(())
}

#[test]
fn a_new_accepted_move_cancels_local_consent_and_stale_acceptance_is_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    GameTakebackFixture::request(&mut session, BoardEnumColor::White, 2_000)?;
    let consent_revision = session.revision();
    GameTransitionFixture::play(&mut session, 3, "e7e5", 3_000)?;
    assert!(session.takeback().pending().is_none());
    let live = session.live();
    let revision = session.revision();
    assert!(
        session
            .respond_takeback(
                GameTakebackResponseRequest {
                    expected_revision: consent_revision,
                    responder: BoardEnumColor::Black,
                    response: GameTakebackEnumResponse::Accept,
                    received_at: ClockInstant::from_millis(4_000)?,
                },
                &GameTransitionFixtureProof
            )
            .is_err()
    );
    assert_eq!(session.live(), live);
    assert_eq!(session.revision(), revision);
    assert!(session.clock().is_running());
    Ok(())
}

#[test]
fn pending_consent_has_no_expiry_and_decline_or_requester_cancel_keeps_play_running()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    GameTakebackFixture::request(&mut session, BoardEnumColor::White, 2_000)?;
    session.poll_deadline(
        session.revision(),
        ClockInstant::from_millis(86_400_000)?,
        &GameTransitionFixtureProof,
    )?;
    assert!(session.takeback().pending().is_some());
    GameTakebackFixture::respond(
        &mut session,
        BoardEnumColor::Black,
        GameTakebackEnumResponse::Decline,
        86_400_000,
    )?;
    assert!(session.takeback().pending().is_none());
    assert!(session.clock().is_running());
    GameTakebackFixture::request(&mut session, BoardEnumColor::White, 86_400_001)?;
    assert!(
        GameTakebackFixture::respond(
            &mut session,
            BoardEnumColor::Black,
            GameTakebackEnumResponse::Cancel,
            86_400_002
        )
        .is_err()
    );
    GameTakebackFixture::respond(
        &mut session,
        BoardEnumColor::White,
        GameTakebackEnumResponse::Cancel,
        86_400_002,
    )?;
    assert!(session.takeback().pending().is_none());
    assert_eq!(session.live(), GameTransitionFixture::id(2)?);
    Ok(())
}

#[test]
fn flagfall_before_takeback_acceptance_finishes_game_and_cancels_consent()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(
        ClockTimeControl::sudden_death(ClockDuration::from_millis(1_000)?)?,
        None,
    )?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 100)?;
    GameTakebackFixture::request(&mut session, BoardEnumColor::White, 200)?;
    let transition = GameTakebackFixture::respond(
        &mut session,
        BoardEnumColor::Black,
        GameTakebackEnumResponse::Accept,
        1_100,
    )?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::FlagFall);
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    assert_eq!(session.live(), GameTransitionFixture::id(2)?);
    assert!(session.takeback().pending().is_none());
    Ok(())
}

#[test]
fn local_takeback_targets_one_ply_and_keeps_removed_moves_and_alternatives()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    session.configure_takeback_consent(session.revision(), GameTakebackEnumConsent::NotRequired)?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    GameTransitionFixture::play(&mut session, 3, "e7e5", 2_000)?;
    assert_eq!(session.takeback_target()?, GameTransitionFixture::id(2)?);
    GameTakebackFixture::request(&mut session, BoardEnumColor::Black, 3_000)?;
    GameTransitionFixture::play(&mut session, 4, "c7c5", 4_000)?;
    assert_eq!(
        session
            .tree()
            .node(GameTransitionFixture::id(2)?)?
            .children(),
        &[GameTransitionFixture::id(3)?, GameTransitionFixture::id(4)?]
    );
    assert_eq!(session.live_moves()?.len(), 2);
    Ok(())
}

#[test]
fn engine_takeback_targets_previous_human_turn_during_and_after_engine_response()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::Black))?;
    let root = session.live();
    assert!(session.takeback_target().is_err());
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    assert_eq!(session.takeback_target()?, root);
    GameTransitionFixture::play(&mut session, 3, "e7e5", 2_000)?;
    assert_eq!(session.takeback_target()?, root);
    GameTransitionFixture::play(&mut session, 4, "g1f3", 3_000)?;
    assert_eq!(session.takeback_target()?, GameTransitionFixture::id(3)?);
    GameTransitionFixture::play(&mut session, 5, "b8c6", 4_000)?;
    assert_eq!(session.takeback_target()?, GameTransitionFixture::id(3)?);
    assert!(GameTakebackFixture::request(&mut session, BoardEnumColor::White, 5_000).is_err());
    assert_eq!(session.live(), GameTransitionFixture::id(5)?);
    Ok(())
}

#[test]
fn human_black_cannot_take_back_engines_first_move_before_their_first_turn()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::White))?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    assert!(session.takeback_target().is_err());
    GameTransitionFixture::play(&mut session, 3, "e7e5", 2_000)?;
    assert_eq!(session.takeback_target()?, GameTransitionFixture::id(2)?);
    GameTransitionFixture::play(&mut session, 4, "g1f3", 3_000)?;
    assert_eq!(session.takeback_target()?, GameTransitionFixture::id(2)?);
    Ok(())
}
