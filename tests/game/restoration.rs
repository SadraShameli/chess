use chess::board::enums::BoardEnumColor;
use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::ClockStateEnumStatus;
use chess::clock::state::ClockStateSnapshot;
use chess::clock::time_control::ClockTimeControl;
use chess::game::adjudication::{
    GameAdjudicationEnumInterruption, GameAdjudicationEnumMateProof,
    GameAdjudicationEnumMateQuestion, GameAdjudicationMateProofProvider,
};
use chess::game::enums::GameEnumEngineReadiness;
use chess::game::enums::{GameEnumOutcomeReason, GameEnumPhase, GameEnumResult};
use chess::game::move_node::GameMoveNodeAnnotations;
use chess::game::outcome::GameOutcome;
use chess::game::pause::{
    GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseEnumFault, GamePauseRequest,
    GameResumeRequest,
};
use chess::game::premove::GamePremoveIntent;
use chess::game::session::{GameSession, GameSessionEnumInterruption, GameSessionEnumState};
use chess::game::takeback::{GameTakebackEnumConsent, GameTakebackRequest};

use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

struct GameRestorationFixture;

impl GameRestorationFixture {
    fn variation() -> Result<GameSession, Box<dyn std::error::Error>> {
        let mut session =
            GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
        session
            .configure_takeback_consent(session.revision(), GameTakebackEnumConsent::NotRequired)?;
        GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
        GameTransitionFixture::play(&mut session, 3, "e7e5", 2_000)?;
        session.request_takeback(
            GameTakebackRequest {
                expected_revision: session.revision(),
                requester: BoardEnumColor::Black,
                received_at: ClockInstant::from_millis(3_000)?,
                engine: GamePauseEnumEngineBarrier::NoEngine,
            },
            &GameTransitionFixtureProof,
        )?;
        GameTransitionFixture::play(&mut session, 4, "c7c5", 4_000)?;
        session.select_view(session.tree().root())?;
        Ok(session)
    }
}

#[test]
fn complete_tree_roundtrip_preserves_ids_sibling_order_annotations_and_selection_paused()
-> Result<(), Box<dyn std::error::Error>> {
    let session = GameRestorationFixture::variation()?;
    let mut saved = session.restoration(ClockInstant::from_millis(5_000)?)?;
    saved.root_annotations =
        GameMoveNodeAnnotations::new("Root study".to_owned(), Vec::new(), Vec::new())?;
    saved.moves[1].annotations =
        GameMoveNodeAnnotations::new("Retained e5".to_owned(), Vec::new(), Vec::new())?;
    let saved_clock = saved.clock.clone();
    let restored = GameSession::restore(saved)?;
    assert_eq!(restored.id(), session.id());
    assert_eq!(restored.revision(), session.revision());
    assert_eq!(restored.live(), session.live());
    assert_eq!(restored.view(), session.view());
    assert_eq!(restored.start_position(), session.start_position());
    assert_eq!(restored.tree().len(), 4);
    assert_eq!(
        restored
            .tree()
            .node(GameTransitionFixture::id(2)?)?
            .children(),
        &[GameTransitionFixture::id(3)?, GameTransitionFixture::id(4)?]
    );
    assert_eq!(
        restored
            .tree()
            .node(restored.tree().root())?
            .annotations()
            .notes(),
        "Root study"
    );
    assert_eq!(
        restored
            .tree()
            .node(GameTransitionFixture::id(3)?)?
            .annotations()
            .notes(),
        "Retained e5"
    );
    assert_eq!(
        restored
            .tree()
            .node(GameTransitionFixture::id(3)?)?
            .before_clock(),
        session
            .tree()
            .node(GameTransitionFixture::id(3)?)?
            .before_clock()
    );
    assert_eq!(restored.phase(), GameEnumPhase::Interrupted);
    assert_eq!(
        restored.interruption(),
        Some(GameSessionEnumInterruption::Paused(
            GamePauseEnumCause::Relaunch
        ))
    );
    assert!(!restored.clock().is_running());
    assert_eq!(
        restored
            .clock()
            .snapshot(ClockInstant::from_millis(99_000)?)?
            .elapsed(),
        saved_clock.elapsed()
    );
    Ok(())
}

#[test]
fn pending_local_consent_is_not_replayed_after_restoration()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
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
    assert!(session.takeback().pending().is_some());
    let restored = GameSession::restore(session.restoration(ClockInstant::from_millis(3_000)?)?)?;
    assert!(restored.takeback().pending().is_none());
    assert_eq!(
        restored.takeback().consent(),
        GameTakebackEnumConsent::Required
    );
    Ok(())
}

#[test]
fn recover_preserves_once_used_warnings_without_replaying_them()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    session.poll_deadline(
        session.revision(),
        ClockInstant::from_millis(591_001)?,
        &GameTransitionFixtureProof,
    )?;
    assert!(session.clock().warning_used(BoardEnumColor::White));
    GameTransitionFixture::play(&mut session, 2, "e2e4", 591_002)?;
    let restored = GameSession::restore(session.restoration(ClockInstant::from_millis(591_003)?)?)?;
    assert!(restored.clock().warning_used(BoardEnumColor::White));
    assert!(!restored.clock().warning_used(BoardEnumColor::Black));
    Ok(())
}

#[test]
fn duplicate_identity_duplicate_continuation_and_forward_parent_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let session = GameRestorationFixture::variation()?;
    let saved = session.restoration(ClockInstant::from_millis(5_000)?)?;
    let mut duplicate_id = saved.clone();
    duplicate_id.moves[1].id = duplicate_id.moves[0].id;
    assert!(GameSession::restore(duplicate_id).is_err());
    let mut duplicate_record = saved.clone();
    duplicate_record
        .moves
        .push(duplicate_record.moves[0].clone());
    assert!(GameSession::restore(duplicate_record).is_err());
    let mut duplicate_move = saved.clone();
    let mut copy = duplicate_move.moves[0].clone();
    copy.id = GameTransitionFixture::id(20)?;
    duplicate_move.moves.push(copy);
    assert!(GameSession::restore(duplicate_move).is_err());
    let mut forward_parent = saved;
    forward_parent.moves.swap(0, 1);
    assert!(GameSession::restore(forward_parent).is_err());
    Ok(())
}

#[test]
fn unpaired_or_rewritten_move_snapshots_and_unrelated_current_counters_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let session = GameRestorationFixture::variation()?;
    let saved = session.restoration(ClockInstant::from_millis(5_000)?)?;
    let mut missing_before = saved.clone();
    missing_before.moves[0].before_clock = None;
    assert!(GameSession::restore(missing_before).is_err());
    let mut rewritten = saved.clone();
    let mut after = rewritten.moves[0]
        .clock
        .as_ref()
        .ok_or("missing after")?
        .persisted();
    after.sides[0].remaining = ClockDuration::from_millis(1)?;
    rewritten.moves[0].clock = Some(ClockStateSnapshot::from_persisted(after)?);
    assert!(GameSession::restore(rewritten).is_err());
    let mut bad_counts = saved;
    let mut current = bad_counts.clock.persisted();
    current.sides[0].completed_moves += 1;
    current.sides[0].stage_moves += 1;
    bad_counts.clock = ClockStateSnapshot::from_persisted(current)?;
    assert!(GameSession::restore(bad_counts).is_err());
    Ok(())
}

#[test]
fn missing_cursors_wrong_clock_side_and_false_terminal_result_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let session = GameRestorationFixture::variation()?;
    let saved = session.restoration(ClockInstant::from_millis(5_000)?)?;
    let mut missing_cursor = saved.clone();
    missing_cursor.view = GameTransitionFixture::id(42)?;
    assert!(GameSession::restore(missing_cursor).is_err());
    let mut wrong_side = saved.clone();
    let mut clock = wrong_side.clock.persisted();
    clock.active = BoardEnumColor::Black;
    wrong_side.clock = ClockStateSnapshot::from_persisted(clock)?;
    assert!(GameSession::restore(wrong_side).is_err());
    let mut false_mate = saved;
    false_mate.state = GameSessionEnumState::Completed(GameOutcome::new(
        GameEnumResult::Victory(BoardEnumColor::Black),
        GameEnumOutcomeReason::Checkmate,
        false_mate.live,
    )?);
    assert!(GameSession::restore(false_mate).is_err());
    Ok(())
}

#[test]
fn completed_checkmate_outcome_and_saved_revision_are_preserved()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    for (id, uci) in [(2, "f2f3"), (3, "e7e5"), (4, "g2g4"), (5, "d8h4")] {
        GameTransitionFixture::play(&mut session, id, uci, 1_000)?;
    }
    let restored = GameSession::restore(session.restoration(ClockInstant::from_millis(2_000)?)?)?;
    assert_eq!(restored.phase(), GameEnumPhase::Completed);
    assert_eq!(restored.outcome(), session.outcome());
    assert_eq!(restored.revision(), session.revision());
    assert!(!restored.clock().is_running());
    Ok(())
}

#[test]
fn manual_pause_stays_paused_and_unadjudicated_deadline_capture_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(
        ClockTimeControl::sudden_death(ClockDuration::from_millis(1_000)?)?,
        None,
    )?;
    assert!(
        session
            .restoration(ClockInstant::from_millis(1_000)?)
            .is_err()
    );
    session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Manual,
            received_at: ClockInstant::from_millis(500)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let restored = GameSession::restore(session.restoration(ClockInstant::from_millis(50_000)?)?)?;
    assert_eq!(restored.interruption(), session.interruption());
    assert_eq!(
        restored
            .clock()
            .snapshot(ClockInstant::from_millis(100_000)?)?
            .elapsed(),
        ClockDuration::from_millis(500)?
    );
    Ok(())
}

#[test]
fn a_recorded_live_line_cannot_continue_past_a_direct_automatic_result()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    GameTransitionFixture::play(&mut session, 2, "g1f3", 1_000)?;
    GameTransitionFixture::play(&mut session, 3, "e7e5", 2_000)?;
    let mut saved = session.restoration(ClockInstant::from_millis(3_000)?)?;
    saved.session.start = BoardStart::custom(BoardPosition::from_fen(
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 149 1",
        BoardEnumVariant::Standard,
    )?);
    assert!(GameSession::restore(saved).is_err());
    Ok(())
}

struct GameRestorationUnknownProof;

impl GameAdjudicationMateProofProvider for GameRestorationUnknownProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Unknown(GameAdjudicationEnumInterruption::MemoryLimit)
    }
}

struct GameRestorationImpossibleProof;

impl GameAdjudicationMateProofProvider for GameRestorationImpossibleProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Impossible
    }
}

#[test]
fn restored_unresolved_flagfall_preserves_uncertainty_until_an_explicit_successful_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(
        ClockTimeControl::sudden_death(ClockDuration::from_millis(1_000)?)?,
        None,
    )?;
    session.poll_deadline(
        session.revision(),
        ClockInstant::from_millis(1_000)?,
        &GameRestorationUnknownProof,
    )?;
    let interruption = session.interruption();
    let mut restored =
        GameSession::restore(session.restoration(ClockInstant::from_millis(2_000)?)?)?;
    assert_eq!(restored.interruption(), interruption);
    assert!(restored.outcome().is_none());
    restored.resume(
        GameResumeRequest {
            expected_revision: restored.revision(),
            received_at: ClockInstant::from_millis(50_000)?,
            readiness: GameEnumEngineReadiness::NotRequired,
        },
        &GameRestorationUnknownProof,
    )?;
    assert_eq!(restored.interruption(), interruption);
    assert!(restored.outcome().is_none());
    assert_eq!(
        restored.clock().status(),
        ClockStateEnumStatus::Flagged(BoardEnumColor::White)
    );
    assert_eq!(
        restored
            .clock()
            .snapshot(ClockInstant::from_millis(100_000)?)?
            .elapsed(),
        ClockDuration::from_millis(1_000)?
    );
    restored.resume(
        GameResumeRequest {
            expected_revision: restored.revision(),
            received_at: ClockInstant::from_millis(100_000)?,
            readiness: GameEnumEngineReadiness::NotRequired,
        },
        &GameRestorationImpossibleProof,
    )?;
    assert_eq!(restored.phase(), GameEnumPhase::Completed);
    assert_eq!(
        restored.outcome().ok_or("missing proven draw")?.result(),
        GameEnumResult::Draw
    );
    assert_eq!(
        restored.outcome().ok_or("missing proven draw")?.reason(),
        GameEnumOutcomeReason::Flagfall
    );
    Ok(())
}

#[test]
fn restored_engine_fault_retains_retry_gate_and_enabled_premoves_without_replaying_queue()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::Black))?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    session.set_premoves_enabled(true);
    session.queue_premove(
        session.revision(),
        GamePremoveIntent::new("g1".parse()?, "f3".parse()?, None)?,
    )?;
    session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Fault(GamePauseEnumFault::Engine),
            received_at: ClockInstant::from_millis(2_000)?,
            engine: GamePauseEnumEngineBarrier::FailedEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let mut restored =
        GameSession::restore(session.restoration(ClockInstant::from_millis(3_000)?)?)?;
    assert!(restored.premove().is_enabled());
    assert!(restored.premove().pending().is_none());
    assert!(
        restored
            .resume(
                GameResumeRequest {
                    expected_revision: restored.revision(),
                    received_at: ClockInstant::from_millis(50_000)?,
                    readiness: GameEnumEngineReadiness::Ready,
                },
                &GameTransitionFixtureProof
            )
            .is_err()
    );
    assert!(
        restored
            .mark_recovered(
                restored.revision(),
                GamePauseEnumFault::Graphics,
                ClockInstant::from_millis(50_000)?
            )
            .is_err()
    );
    assert_eq!(
        restored.interruption(),
        Some(GameSessionEnumInterruption::RecoveryRequired(
            GamePauseEnumFault::Engine
        ))
    );
    restored.mark_recovered(
        restored.revision(),
        GamePauseEnumFault::Engine,
        ClockInstant::from_millis(50_000)?,
    )?;
    assert!(!restored.clock().is_running());
    restored.resume(
        GameResumeRequest {
            expected_revision: restored.revision(),
            received_at: ClockInstant::from_millis(100_000)?,
            readiness: GameEnumEngineReadiness::Ready,
        },
        &GameTransitionFixtureProof,
    )?;
    assert_eq!(restored.phase(), GameEnumPhase::Active);
    assert_eq!(restored.live(), GameTransitionFixture::id(2)?);
    assert!(restored.premove().pending().is_none());
    Ok(())
}
