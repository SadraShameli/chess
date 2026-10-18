use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::ClockStateEnumStatus;
use chess::clock::time_control::ClockTimeControl;
use chess::game::adjudication::{
    GameAdjudicationEnumInterruption, GameAdjudicationEnumMateProof,
    GameAdjudicationEnumMateQuestion, GameAdjudicationMateProofProvider,
};
use chess::game::enums::{GameEnumEngineReadiness, GameEnumPhase, GameEnumTransitionKind};
use chess::game::pause::{
    GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseEnumFault, GamePauseRequest,
    GameResumeRequest,
};
use chess::game::result::GameResultResignRequest;
use chess::game::save::{GameSaveEnumStatus, GameSaveError, GameSaveFailureRequest};
use chess::game::session::{GameSession, GameSessionEnumInterruption, GameSessionError};
use chess::game::takeback::GameTakebackRequest;

use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

struct GameSaveFixture;

impl GameSaveFixture {
    fn fail(session: &mut GameSession, millis: u64) -> Result<(), Box<dyn std::error::Error>> {
        session.interrupt_for_save_failure(
            GameSaveFailureRequest {
                expected_revision: session.revision(),
                received_at: ClockInstant::from_millis(millis)?,
                engine: GamePauseEnumEngineBarrier::NoEngine,
            },
            &GameTransitionFixtureProof,
        )?;
        Ok(())
    }
}

#[test]
fn failed_autosave_pauses_the_latest_position_and_clears_pending_consent()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(
        ClockTimeControl::sudden_death(ClockDuration::from_millis(20_000)?)?,
        None,
    )?;
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
    let previous_revision = session.revision();
    let live = session.live();
    GameSaveFixture::fail(&mut session, 3_000)?;
    assert_eq!(session.revision(), previous_revision.next()?);
    assert_eq!(session.live(), live);
    assert_eq!(session.tree().len(), 2);
    assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    assert_eq!(
        session.interruption(),
        Some(GameSessionEnumInterruption::Paused(
            GamePauseEnumCause::AutosaveFailure
        ))
    );
    assert!(session.takeback().pending().is_none());
    assert!(!session.clock().is_running());
    assert_eq!(
        session
            .clock()
            .snapshot(ClockInstant::from_millis(100_000)?)?
            .elapsed(),
        ClockDuration::from_millis(2_000)?
    );
    Ok(())
}

#[test]
fn a_failed_older_snapshot_never_rolls_back_newer_accepted_moves()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let old_revision = session.revision();
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    GameTransitionFixture::play(&mut session, 3, "e7e5", 2_000)?;
    let revision = session.revision();
    assert!(
        session
            .interrupt_for_save_failure(
                GameSaveFailureRequest {
                    expected_revision: old_revision,
                    received_at: ClockInstant::from_millis(3_000)?,
                    engine: GamePauseEnumEngineBarrier::NoEngine,
                },
                &GameTransitionFixtureProof,
            )
            .is_err()
    );
    assert_eq!(session.revision(), revision);
    GameSaveFixture::fail(&mut session, 3_000)?;
    assert_eq!(session.live(), GameTransitionFixture::id(3)?);
    assert_eq!(session.live_moves()?.len(), 2);
    Ok(())
}

#[test]
fn save_failure_preserves_existing_pause_and_blocks_live_mutations()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Manual,
            received_at: ClockInstant::from_millis(2_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let interruption = session.interruption();
    let clock = session.clock().clone();
    GameSaveFixture::fail(&mut session, 3_000)?;
    let revision = session.revision();
    assert_eq!(session.interruption(), interruption);
    assert_eq!(session.clock(), &clock);
    assert!(matches!(
        session.resume(
            GameResumeRequest {
                expected_revision: revision,
                received_at: ClockInstant::from_millis(4_000)?,
                readiness: GameEnumEngineReadiness::NotRequired,
            },
            &GameTransitionFixtureProof,
        ),
        Err(GameSessionError::Save(GameSaveError::SaveRequired))
    ));
    assert!(matches!(
        session.takeback_target(),
        Err(GameSessionError::Save(GameSaveError::SaveRequired))
    ));
    assert!(matches!(
        session.resign(
            GameResultResignRequest {
                expected_revision: revision,
                resigning: BoardEnumColor::Black,
                received_at: ClockInstant::from_millis(4_000)?,
                engine: GamePauseEnumEngineBarrier::NoEngine,
            },
            &GameTransitionFixtureProof,
        ),
        Err(GameSessionError::Save(GameSaveError::SaveRequired))
    ));
    assert_eq!(session.revision(), revision);
    Ok(())
}

struct GameSaveUnknownProof;

impl GameAdjudicationMateProofProvider for GameSaveUnknownProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Unknown(GameAdjudicationEnumInterruption::MemoryLimit)
    }
}

#[test]
fn save_failure_preserves_unknown_flagfall_question_and_frozen_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(
        ClockTimeControl::sudden_death(ClockDuration::from_millis(1_000)?)?,
        None,
    )?;
    session.poll_deadline(
        session.revision(),
        ClockInstant::from_millis(1_000)?,
        &GameSaveUnknownProof,
    )?;
    let state = session.state();
    GameSaveFixture::fail(&mut session, 50_000)?;
    assert_eq!(session.state(), state);
    assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    assert_eq!(
        session.clock().status(),
        ClockStateEnumStatus::Flagged(BoardEnumColor::White)
    );
    assert!(session.outcome().is_none());
    Ok(())
}

#[test]
fn save_failure_preserves_an_existing_engine_recovery_requirement()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::Black))?;
    session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Fault(GamePauseEnumFault::Engine),
            received_at: ClockInstant::from_millis(1_000)?,
            engine: GamePauseEnumEngineBarrier::FailedEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let state = session.state();
    GameSaveFixture::fail(&mut session, 2_000)?;
    assert_eq!(session.state(), state);
    assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    Ok(())
}

#[test]
fn failed_result_autosave_preserves_the_completed_result_and_clock()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    session.resign(
        GameResultResignRequest {
            expected_revision: session.revision(),
            resigning: BoardEnumColor::White,
            received_at: ClockInstant::from_millis(1_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    let outcome = session.outcome();
    let clock = session.clock().clone();
    GameSaveFixture::fail(&mut session, 2_000)?;
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    assert_eq!(session.outcome(), outcome);
    assert_eq!(session.clock(), &clock);
    assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    Ok(())
}

#[test]
fn authoritative_deadline_precedes_the_save_failure_pause() -> Result<(), Box<dyn std::error::Error>>
{
    let mut session = GameTransitionFixture::session(
        ClockTimeControl::sudden_death(ClockDuration::from_millis(1_000)?)?,
        None,
    )?;
    let transition = session.interrupt_for_save_failure(
        GameSaveFailureRequest {
            expected_revision: session.revision(),
            received_at: ClockInstant::from_millis(1_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::FlagFall);
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    assert_eq!(session.save_status(), GameSaveEnumStatus::Required);
    assert_eq!(
        session.clock().status(),
        ClockStateEnumStatus::Flagged(BoardEnumColor::White)
    );
    Ok(())
}
