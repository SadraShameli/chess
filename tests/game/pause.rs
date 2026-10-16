use chess::board::enums::BoardEnumColor;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::time_control::ClockTimeControl;
use chess::game::enums::{GameEnumEngineReadiness, GameEnumPhase, GameEnumTransitionKind};
use chess::game::pause::{
    GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseEnumFault, GamePauseRequest,
    GameResumeRequest,
};
use chess::game::premove::{GamePremoveEnumClearReason, GamePremoveIntent};
use chess::game::session::GameSessionEnumInterruption;

use crate::game_transition_fixture::{
    GameTransitionFixture, GameTransitionFixtureProof, GameTransitionFixtureUnresolvedProof,
};

#[test]
fn adjudication_retry_preserves_the_paused_position_until_proof_resolves()
-> Result<(), Box<dyn std::error::Error>> {
    use chess::clock::state::ClockStateEnumStatus;
    use chess::game::adjudication::GameAdjudicationEnumMateQuestion;
    use chess::game::session::GameSessionError;

    let proof =
        GameTransitionFixtureUnresolvedProof::new(GameAdjudicationEnumMateQuestion::AnySide);
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    let stale_revision = session.revision();
    GameTransitionFixture::play_with_proof(&mut session, 2, "e2e4", 1_000, &proof)?;
    let live = session.live();
    let position = session.tree().node(live)?.position().clone();
    let clock = session.clock().clone();
    let interruption = session.interruption();
    assert_eq!(clock.status(), ClockStateEnumStatus::Paused);
    assert!(matches!(
        interruption,
        Some(GameSessionEnumInterruption::Adjudication { .. })
    ));
    assert!(matches!(
        session.resume(
            GameResumeRequest {
                expected_revision: stale_revision,
                received_at: ClockInstant::from_millis(50_000)?,
                readiness: GameEnumEngineReadiness::NotRequired,
            },
            &proof,
        ),
        Err(GameSessionError::Revision(_))
    ));
    assert_eq!(proof.calls(), 1);
    for (attempt, millis) in [(2, 50_000), (3, 100_000)] {
        let revision = session.revision();
        let transition = session.resume(
            GameResumeRequest {
                expected_revision: revision,
                received_at: ClockInstant::from_millis(millis)?,
                readiness: GameEnumEngineReadiness::NotRequired,
            },
            &proof,
        )?;
        assert_eq!(proof.calls(), attempt);
        assert_eq!(transition.kind(), GameEnumTransitionKind::Resumed);
        assert_eq!(transition.previous_revision(), revision);
        assert_eq!(transition.revision(), revision.next()?);
        assert_eq!(transition.phase(), GameEnumPhase::Interrupted);
        assert_eq!(transition.interruption(), interruption);
        assert_eq!(transition.outcome(), None);
        assert_eq!(transition.live(), live);
        assert!(transition.accepted_node().is_none());
        assert_eq!(session.live(), live);
        assert_eq!(session.tree().node(live)?.position(), &position);
        assert_eq!(session.clock(), &clock);
        assert_eq!(session.outcome(), None);
    }
    session.resume(
        GameResumeRequest {
            expected_revision: session.revision(),
            received_at: ClockInstant::from_millis(150_000)?,
            readiness: GameEnumEngineReadiness::NotRequired,
        },
        &GameTransitionFixtureProof,
    )?;
    assert_eq!(session.phase(), GameEnumPhase::Active);
    assert_eq!(session.outcome(), None);
    assert_eq!(session.tree().node(session.live())?.position(), &position);
    assert_eq!(
        session
            .clock()
            .view(ClockInstant::from_millis(150_000)?)?
            .elapsed(),
        clock.view(ClockInstant::from_millis(1_000)?)?.elapsed()
    );
    Ok(())
}

#[test]
fn manual_pause_freezes_clocks_until_explicit_resume_without_charging_downtime()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    let revision = session.revision();
    let transition = session.pause(
        GamePauseRequest {
            expected_revision: revision,
            cause: GamePauseEnumCause::Manual,
            received_at: ClockInstant::from_millis(1_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::Interrupted);
    assert_eq!(session.phase(), GameEnumPhase::Interrupted);
    assert_eq!(
        session
            .clock()
            .snapshot(ClockInstant::from_millis(50_000)?)?
            .elapsed(),
        ClockDuration::from_millis(1_000)?
    );
    assert!(matches!(
        session.interruption(),
        Some(GameSessionEnumInterruption::Paused(
            GamePauseEnumCause::Manual
        ))
    ));
    assert!(
        session
            .resume(
                GameResumeRequest {
                    expected_revision: revision,
                    received_at: ClockInstant::from_millis(50_000)?,
                    readiness: GameEnumEngineReadiness::NotRequired,
                },
                &GameTransitionFixtureProof
            )
            .is_err()
    );
    session.resume(
        GameResumeRequest {
            expected_revision: session.revision(),
            received_at: ClockInstant::from_millis(50_000)?,
            readiness: GameEnumEngineReadiness::NotRequired,
        },
        &GameTransitionFixtureProof,
    )?;
    assert_eq!(session.phase(), GameEnumPhase::Active);
    assert_eq!(
        session
            .clock()
            .view(ClockInstant::from_millis(51_000)?)?
            .elapsed(),
        ClockDuration::from_millis(2_000)?
    );
    Ok(())
}

#[test]
fn native_deadline_wins_over_pause_without_restoring_time() -> Result<(), Box<dyn std::error::Error>>
{
    let mut session = GameTransitionFixture::session(
        ClockTimeControl::sudden_death(ClockDuration::from_millis(1_000)?)?,
        None,
    )?;
    let transition = session.pause(
        GamePauseRequest {
            expected_revision: session.revision(),
            cause: GamePauseEnumCause::Manual,
            received_at: ClockInstant::from_millis(1_000)?,
            engine: GamePauseEnumEngineBarrier::NoEngine,
        },
        &GameTransitionFixtureProof,
    )?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::FlagFall);
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    assert!(!session.clock().is_running());
    Ok(())
}

#[test]
fn engine_fault_requires_explicit_successful_recovery_then_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::Black))?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    session.set_premoves_enabled(session.revision(), true)?;
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
    assert!(session.premove().pending().is_none());
    assert!(
        session
            .resume(
                GameResumeRequest {
                    expected_revision: session.revision(),
                    received_at: ClockInstant::from_millis(3_000)?,
                    readiness: GameEnumEngineReadiness::Ready,
                },
                &GameTransitionFixtureProof
            )
            .is_err()
    );
    session.mark_recovered(
        session.revision(),
        GamePauseEnumFault::Engine,
        ClockInstant::from_millis(3_000)?,
    )?;
    assert!(!session.clock().is_running());
    assert!(
        session
            .resume(
                GameResumeRequest {
                    expected_revision: session.revision(),
                    received_at: ClockInstant::from_millis(4_000)?,
                    readiness: GameEnumEngineReadiness::Unavailable,
                },
                &GameTransitionFixtureProof
            )
            .is_err()
    );
    session.resume(
        GameResumeRequest {
            expected_revision: session.revision(),
            received_at: ClockInstant::from_millis(4_000)?,
            readiness: GameEnumEngineReadiness::Ready,
        },
        &GameTransitionFixtureProof,
    )?;
    assert_eq!(session.phase(), GameEnumPhase::Active);
    assert!(session.premove().pending().is_none());
    Ok(())
}

#[test]
fn engine_mode_refuses_a_missing_barrier_and_failed_engine_cannot_fake_manual_pause()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session =
        GameTransitionFixture::session(ClockTimeControl::untimed(), Some(BoardEnumColor::Black))?;
    let revision = session.revision();
    for engine in [
        GamePauseEnumEngineBarrier::NoEngine,
        GamePauseEnumEngineBarrier::FailedEngine,
    ] {
        assert!(
            session
                .pause(
                    GamePauseRequest {
                        expected_revision: revision,
                        cause: GamePauseEnumCause::Manual,
                        received_at: ClockInstant::from_millis(1_000)?,
                        engine,
                    },
                    &GameTransitionFixtureProof
                )
                .is_err()
        );
    }
    assert_eq!(session.revision(), revision);
    assert!(session.clock().is_running());
    assert!(session.premove().pending().is_none());
    assert!(
        session
            .clear_premove(session.revision(), GamePremoveEnumClearReason::Relaunch)?
            .is_none()
    );
    Ok(())
}
