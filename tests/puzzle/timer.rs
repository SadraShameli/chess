use std::num::NonZeroU64;
use std::time::Duration;

use chess::board::position::BoardPosition;
use chess::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use chess::puzzle::attempt::PuzzleAttemptId;
use chess::puzzle::definition::PuzzleDefinition;
use chess::puzzle::enums::{
    PuzzleEnumCountdownMode, PuzzleEnumMoveFeedback, PuzzleEnumPrimaryOutcome,
};
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::puzzle::metadata::PuzzleDefinitionMetadata;
use chess::puzzle::random_provider::PuzzleRandomProviderSeeded;
use chess::puzzle::session::{PuzzleSession, PuzzleSessionConfig, PuzzleSessionId};
use chess::puzzle::timer::{
    PuzzleTimer, PuzzleTimerEnumExpiry, PuzzleTimerEnumPauseReason, PuzzleTimerEnumPhase,
    PuzzleTimerEnumReadiness, PuzzleTimerEnumStatus, PuzzleTimerEnumTransition, PuzzleTimerError,
    PuzzleTimerSnapshot, PuzzleTimerSnapshotOptions,
};

#[test]
fn save_backpressure_is_a_distinct_persistable_pause_and_requires_explicit_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let mut timer = PuzzleTimerFixture::running(PuzzleEnumCountdownMode::Session)?;
    timer.pause(
        ClockInstant::from_nanos(20),
        PuzzleTimerEnumPauseReason::SaveBackpressure,
    )?;
    let snapshot = timer.snapshot(ClockInstant::from_nanos(1000))?;
    let validated = PuzzleTimerSnapshot::from_persisted(snapshot.persisted())?;
    assert_eq!(validated, snapshot);
    assert_eq!(snapshot.persisted().elapsed, ClockDuration::from_nanos(10));
    assert!(matches!(
        snapshot.persisted().status,
        PuzzleTimerEnumStatus::Paused {
            reason: PuzzleTimerEnumPauseReason::SaveBackpressure,
            phase: PuzzleTimerEnumPhase::Active,
        }
    ));
    timer.update_readiness(
        ClockInstant::from_nanos(2000),
        PuzzleTimerEnumReadiness::Ready,
    )?;
    assert_eq!(timer.snapshot(ClockInstant::from_nanos(2000))?, snapshot);
    timer.resume(
        ClockInstant::from_nanos(2000),
        PuzzleTimerEnumReadiness::Ready,
    )?;
    assert_eq!(
        timer.view(ClockInstant::from_nanos(2001))?.elapsed(),
        ClockDuration::from_nanos(11)
    );
    Ok(())
}

struct PuzzleTimerFixture;

impl PuzzleTimerFixture {
    fn config(
        mode: PuzzleEnumCountdownMode,
    ) -> Result<PuzzleSessionConfig, Box<dyn std::error::Error>> {
        Ok(PuzzleSessionConfig::new(mode, Duration::from_nanos(100))?)
    }

    fn running(mode: PuzzleEnumCountdownMode) -> Result<PuzzleTimer, Box<dyn std::error::Error>> {
        let mut timer = PuzzleTimer::new(Self::config(mode)?);
        assert_eq!(
            timer
                .update_readiness(
                    ClockInstant::from_nanos(10),
                    PuzzleTimerEnumReadiness::Ready
                )?
                .transition(),
            PuzzleTimerEnumTransition::Started
        );
        Ok(timer)
    }

    fn session(mode: PuzzleEnumCountdownMode) -> Result<PuzzleSession, Box<dyn std::error::Error>> {
        let initial = BoardPosition::standard();
        let mut position = initial.clone();
        let mut line = Vec::new();
        for value in ["e2e4", "e7e5", "g1f3"] {
            let movement = position.parse_uci(value)?;
            position = position.play(movement)?;
            line.push(movement);
        }
        let definition = PuzzleDefinition::new(
            PuzzleDefinitionIdentity::new("timer-fixture".into(), "v1".into(), "tactic-1".into())?,
            initial,
            vec![line],
            PuzzleDefinitionMetadata::default(),
        )?;
        let identity = NonZeroU64::new(1).ok_or("Missing fixture identity")?;
        Ok(PuzzleSession::new(
            PuzzleSessionId::new(identity),
            PuzzleAttemptId::new(identity),
            definition,
            Self::config(mode)?,
        ))
    }

    fn accept(timer: &mut PuzzleTimer, now: u64) -> Result<(), PuzzleTimerError> {
        let update = timer.check_action(ClockInstant::from_nanos(now))?;
        assert_eq!(
            update.transition(),
            PuzzleTimerEnumTransition::ActionAccepted
        );
        assert_eq!(
            update.view().deadline(),
            Some(ClockInstant::from_nanos(110))
        );
        Ok(())
    }

    fn expiry(mode: PuzzleEnumCountdownMode) -> PuzzleTimerEnumExpiry {
        match mode {
            PuzzleEnumCountdownMode::Session => PuzzleTimerEnumExpiry::SessionEnded,
            PuzzleEnumCountdownMode::PerPuzzle => PuzzleTimerEnumExpiry::PuzzleTimedOut,
        }
    }

    fn assert_time(
        timer: &PuzzleTimer,
        now: u64,
        remaining: u64,
        elapsed: u64,
        puzzle_elapsed: u64,
    ) -> Result<(), PuzzleTimerError> {
        let view = timer.view(ClockInstant::from_nanos(now))?;
        assert_eq!(view.remaining(), ClockDuration::from_nanos(remaining));
        assert_eq!(view.elapsed(), ClockDuration::from_nanos(elapsed));
        assert_eq!(
            view.puzzle_elapsed(),
            ClockDuration::from_nanos(puzzle_elapsed)
        );
        Ok(())
    }
}

#[test]
fn uses_the_exclusive_real_session_configuration_and_defaults()
-> Result<(), Box<dyn std::error::Error>> {
    for mode in [
        PuzzleEnumCountdownMode::Session,
        PuzzleEnumCountdownMode::PerPuzzle,
    ] {
        let config = PuzzleSessionConfig::defaults(mode);
        let timer = PuzzleTimer::new(config);
        assert_eq!(timer.config(), config);
        let view = timer.view(ClockInstant::from_nanos(0))?;
        assert_eq!(view.remaining(), config.duration());
        assert_eq!(view.elapsed(), ClockDuration::ZERO);
        assert_eq!(view.status(), PuzzleTimerEnumStatus::AwaitingReadiness);
        assert_eq!(view.deadline(), None);
    }
    Ok(())
}

#[test]
fn requires_the_complete_initial_readiness_gate_before_charging()
-> Result<(), Box<dyn std::error::Error>> {
    for mode in [
        PuzzleEnumCountdownMode::Session,
        PuzzleEnumCountdownMode::PerPuzzle,
    ] {
        let mut timer = PuzzleTimer::new(PuzzleTimerFixture::config(mode)?);
        for (now, readiness) in [
            (10, PuzzleTimerEnumReadiness::BoardNotReady),
            (200, PuzzleTimerEnumReadiness::InputBlocked),
        ] {
            assert_eq!(
                timer
                    .update_readiness(ClockInstant::from_nanos(now), readiness)?
                    .transition(),
                PuzzleTimerEnumTransition::Unchanged
            );
            PuzzleTimerFixture::assert_time(&timer, now, 100, 0, 0)?;
        }
        assert!(matches!(
            timer.check_action(ClockInstant::from_nanos(200)),
            Err(PuzzleTimerError::NotActive { .. })
        ));
        timer.update_readiness(
            ClockInstant::from_nanos(500),
            PuzzleTimerEnumReadiness::Ready,
        )?;
        let view = timer.view(ClockInstant::from_nanos(525))?;
        assert_eq!(view.status(), PuzzleTimerEnumStatus::Running);
        assert_eq!(view.deadline(), Some(ClockInstant::from_nanos(600)));
        PuzzleTimerFixture::assert_time(&timer, 525, 75, 25, 25)?;
    }
    Ok(())
}

#[test]
fn initial_hidden_board_requires_explicit_resume_even_after_becoming_ready()
-> Result<(), Box<dyn std::error::Error>> {
    let mut timer = PuzzleTimer::new(PuzzleTimerFixture::config(
        PuzzleEnumCountdownMode::Session,
    )?);
    timer.update_readiness(
        ClockInstant::from_nanos(1),
        PuzzleTimerEnumReadiness::Hidden,
    )?;
    timer.update_readiness(
        ClockInstant::from_nanos(200),
        PuzzleTimerEnumReadiness::Ready,
    )?;
    let view = timer.view(ClockInstant::from_nanos(200))?;
    assert_eq!(
        view.status(),
        PuzzleTimerEnumStatus::Paused {
            reason: PuzzleTimerEnumPauseReason::WindowHidden,
            phase: PuzzleTimerEnumPhase::AwaitingReadiness,
        }
    );
    PuzzleTimerFixture::assert_time(&timer, 200, 100, 0, 0)?;
    timer.resume(
        ClockInstant::from_nanos(300),
        PuzzleTimerEnumReadiness::Ready,
    )?;
    PuzzleTimerFixture::assert_time(&timer, 320, 80, 20, 20)?;
    Ok(())
}

#[test]
fn replies_animations_hints_illegal_input_and_practice_keep_the_existing_countdown()
-> Result<(), Box<dyn std::error::Error>> {
    for mode in [
        PuzzleEnumCountdownMode::Session,
        PuzzleEnumCountdownMode::PerPuzzle,
    ] {
        let mut session = PuzzleTimerFixture::session(mode)?;
        let mut timer = PuzzleTimer::new(session.config());
        timer.update_readiness(
            ClockInstant::from_nanos(10),
            PuzzleTimerEnumReadiness::Ready,
        )?;
        PuzzleTimerFixture::accept(&mut timer, 20)?;
        let hint = session.request_hint(&session.context())?;
        assert_eq!(hint.stage(), session.attempt().hint_stage());
        let illegal = BoardPosition::standard()
            .play_uci("e2e4")?
            .play_uci("e7e5")?
            .parse_uci("f1b5")?;
        PuzzleTimerFixture::accept(&mut timer, 25)?;
        assert!(session.submit_move(&session.context(), illegal).is_err());
        let unlisted = session.attempt().position().parse_uci("d2d4")?;
        PuzzleTimerFixture::accept(&mut timer, 30)?;
        assert_eq!(
            session.submit_move(&session.context(), unlisted)?,
            PuzzleEnumMoveFeedback::LegalUnlisted
        );
        PuzzleTimerFixture::accept(&mut timer, 40)?;
        session.retry_practice(&session.context())?;
        let movement = session.attempt().position().parse_uci("e2e4")?;
        PuzzleTimerFixture::accept(&mut timer, 50)?;
        session.submit_move(&session.context(), movement)?;
        timer.update_readiness(
            ClockInstant::from_nanos(55),
            PuzzleTimerEnumReadiness::InputBlocked,
        )?;
        PuzzleTimerFixture::accept(&mut timer, 60)?;
        let mut random = PuzzleRandomProviderSeeded::new(1);
        session.play_opponent_reply(&session.context(), &mut random)?;
        timer.update_readiness(
            ClockInstant::from_nanos(65),
            PuzzleTimerEnumReadiness::BoardNotReady,
        )?;
        PuzzleTimerFixture::accept(&mut timer, 70)?;
        session.request_hint(&session.context())?;
        let movement = session.attempt().position().parse_uci("g1f3")?;
        PuzzleTimerFixture::accept(&mut timer, 80)?;
        assert_eq!(
            session.submit_move(&session.context(), movement)?,
            PuzzleEnumMoveFeedback::Completed
        );
        timer.enter_review(ClockInstant::from_nanos(80))?;
        assert_eq!(
            session.attempt().primary_outcome(),
            Some(PuzzleEnumPrimaryOutcome::Failed)
        );
        PuzzleTimerFixture::assert_time(&timer, 1000, 30, 70, 70)?;
    }
    Ok(())
}

#[test]
fn all_interruptions_preserve_earned_time_and_never_resume_automatically()
-> Result<(), Box<dyn std::error::Error>> {
    for reason in [
        PuzzleTimerEnumPauseReason::Manual,
        PuzzleTimerEnumPauseReason::FocusLost,
        PuzzleTimerEnumPauseReason::WindowHidden,
        PuzzleTimerEnumPauseReason::Sleep,
    ] {
        let mut timer = PuzzleTimerFixture::running(PuzzleEnumCountdownMode::PerPuzzle)?;
        assert_eq!(
            timer
                .pause(ClockInstant::from_nanos(40), reason)?
                .transition(),
            PuzzleTimerEnumTransition::Paused(reason)
        );
        timer.update_readiness(
            ClockInstant::from_nanos(500),
            PuzzleTimerEnumReadiness::Ready,
        )?;
        assert_eq!(timer.view(ClockInstant::from_nanos(500))?.deadline(), None);
        PuzzleTimerFixture::assert_time(&timer, 500, 70, 30, 30)?;
        assert!(matches!(
            timer.check_action(ClockInstant::from_nanos(500)),
            Err(PuzzleTimerError::NotActive { .. })
        ));
        timer.resume(
            ClockInstant::from_nanos(600),
            PuzzleTimerEnumReadiness::InputBlocked,
        )?;
        assert_eq!(
            timer.view(ClockInstant::from_nanos(610))?.deadline(),
            Some(ClockInstant::from_nanos(670))
        );
        PuzzleTimerFixture::assert_time(&timer, 610, 60, 40, 40)?;
        timer.pause(ClockInstant::from_nanos(620), reason)?;
        timer.resume(
            ClockInstant::from_nanos(900),
            PuzzleTimerEnumReadiness::Ready,
        )?;
        PuzzleTimerFixture::assert_time(&timer, 910, 40, 60, 60)?;
    }
    Ok(())
}

#[test]
fn hidden_readiness_pauses_active_time_but_hidden_resume_is_rejected_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut timer = PuzzleTimerFixture::running(PuzzleEnumCountdownMode::Session)?;
    timer.update_readiness(
        ClockInstant::from_nanos(40),
        PuzzleTimerEnumReadiness::Hidden,
    )?;
    let before = timer;
    assert!(matches!(
        timer.resume(
            ClockInstant::from_nanos(500),
            PuzzleTimerEnumReadiness::Hidden
        ),
        Err(PuzzleTimerError::HiddenResume)
    ));
    assert_eq!(timer, before);
    timer.resume(
        ClockInstant::from_nanos(600),
        PuzzleTimerEnumReadiness::BoardNotReady,
    )?;
    PuzzleTimerFixture::assert_time(&timer, 610, 60, 40, 40)?;
    Ok(())
}

#[test]
fn review_next_and_exhaustion_exclude_time_with_mode_specific_new_puzzle_budgets()
-> Result<(), Box<dyn std::error::Error>> {
    for mode in [
        PuzzleEnumCountdownMode::Session,
        PuzzleEnumCountdownMode::PerPuzzle,
    ] {
        let mut timer = PuzzleTimerFixture::running(mode)?;
        timer.enter_review(ClockInstant::from_nanos(40))?;
        let view = timer.view(ClockInstant::from_nanos(500))?;
        assert_eq!(view.status(), PuzzleTimerEnumStatus::Review);
        assert_eq!(view.deadline(), None);
        PuzzleTimerFixture::assert_time(&timer, 500, 70, 30, 30)?;
        timer.arm_next(ClockInstant::from_nanos(600))?;
        let next_remaining = match mode {
            PuzzleEnumCountdownMode::Session => 70,
            PuzzleEnumCountdownMode::PerPuzzle => 100,
        };
        PuzzleTimerFixture::assert_time(&timer, 700, next_remaining, 30, 0)?;
        timer.update_readiness(
            ClockInstant::from_nanos(800),
            PuzzleTimerEnumReadiness::InputBlocked,
        )?;
        timer.update_readiness(
            ClockInstant::from_nanos(900),
            PuzzleTimerEnumReadiness::Ready,
        )?;
        timer.exhaust_collection(ClockInstant::from_nanos(920))?;
        assert_eq!(
            timer.view(ClockInstant::from_nanos(2000))?.status(),
            PuzzleTimerEnumStatus::Exhausted
        );
        PuzzleTimerFixture::assert_time(&timer, 2000, next_remaining - 20, 50, 20)?;
        assert!(matches!(
            timer.resume(
                ClockInstant::from_nanos(2000),
                PuzzleTimerEnumReadiness::Ready
            ),
            Err(PuzzleTimerError::NotPaused { .. })
        ));
        timer.arm_next(ClockInstant::from_nanos(2100))?;
        timer.update_readiness(
            ClockInstant::from_nanos(2200),
            PuzzleTimerEnumReadiness::Ready,
        )?;
        let final_remaining = match mode {
            PuzzleEnumCountdownMode::Session => 50,
            PuzzleEnumCountdownMode::PerPuzzle => 100,
        };
        PuzzleTimerFixture::assert_time(&timer, 2210, final_remaining - 10, 60, 10)?;
    }
    Ok(())
}

#[test]
fn active_or_already_armed_puzzles_cannot_reset_their_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let mut timer = PuzzleTimerFixture::running(PuzzleEnumCountdownMode::PerPuzzle)?;
    let before = timer;
    assert!(matches!(
        timer.arm_next(ClockInstant::from_nanos(30)),
        Err(PuzzleTimerError::CannotArmNext { .. })
    ));
    assert_eq!(timer, before);
    timer.enter_review(ClockInstant::from_nanos(40))?;
    timer.arm_next(ClockInstant::from_nanos(50))?;
    let before = timer;
    assert!(matches!(
        timer.arm_next(ClockInstant::from_nanos(60)),
        Err(PuzzleTimerError::CannotArmNext { .. })
    ));
    assert_eq!(timer, before);
    Ok(())
}

#[test]
fn changing_phase_while_interrupted_cannot_bypass_explicit_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let mut timer = PuzzleTimerFixture::running(PuzzleEnumCountdownMode::Session)?;
    timer.pause(
        ClockInstant::from_nanos(40),
        PuzzleTimerEnumPauseReason::FocusLost,
    )?;
    timer.enter_review(ClockInstant::from_nanos(50))?;
    timer.exhaust_collection(ClockInstant::from_nanos(60))?;
    timer.arm_next(ClockInstant::from_nanos(100))?;
    timer.update_readiness(
        ClockInstant::from_nanos(200),
        PuzzleTimerEnumReadiness::Ready,
    )?;
    assert_eq!(
        timer.view(ClockInstant::from_nanos(200))?.status(),
        PuzzleTimerEnumStatus::Paused {
            reason: PuzzleTimerEnumPauseReason::FocusLost,
            phase: PuzzleTimerEnumPhase::AwaitingReadiness,
        }
    );
    PuzzleTimerFixture::assert_time(&timer, 200, 70, 30, 0)?;
    timer.resume(
        ClockInstant::from_nanos(300),
        PuzzleTimerEnumReadiness::InputBlocked,
    )?;
    assert_eq!(
        timer.view(ClockInstant::from_nanos(500))?.status(),
        PuzzleTimerEnumStatus::AwaitingReadiness
    );
    timer.update_readiness(
        ClockInstant::from_nanos(600),
        PuzzleTimerEnumReadiness::Ready,
    )?;
    PuzzleTimerFixture::assert_time(&timer, 610, 60, 40, 10)?;
    Ok(())
}

#[test]
fn expiry_wins_exact_instant_action_pause_review_and_next_races()
-> Result<(), Box<dyn std::error::Error>> {
    for mode in [
        PuzzleEnumCountdownMode::Session,
        PuzzleEnumCountdownMode::PerPuzzle,
    ] {
        let running = PuzzleTimerFixture::running(mode)?;
        let mut timer = running;
        assert_eq!(
            timer
                .check_action(ClockInstant::from_nanos(109))?
                .transition(),
            PuzzleTimerEnumTransition::ActionAccepted
        );
        assert_eq!(
            timer
                .check_action(ClockInstant::from_nanos(110))?
                .transition(),
            PuzzleTimerEnumTransition::Expired(PuzzleTimerFixture::expiry(mode))
        );
        let mut timer = running;
        let update = timer.poll(ClockInstant::from_nanos(110))?;
        assert_eq!(
            update.transition(),
            PuzzleTimerEnumTransition::Expired(PuzzleTimerFixture::expiry(mode))
        );
        PuzzleTimerFixture::assert_time(&timer, 1000, 0, 100, 100)?;
        let mut timer = running;
        assert_eq!(
            timer
                .pause(
                    ClockInstant::from_nanos(110),
                    PuzzleTimerEnumPauseReason::Manual
                )?
                .transition(),
            PuzzleTimerEnumTransition::Expired(PuzzleTimerFixture::expiry(mode))
        );
        let mut timer = running;
        assert_eq!(
            timer
                .enter_review(ClockInstant::from_nanos(110))?
                .transition(),
            PuzzleTimerEnumTransition::Expired(PuzzleTimerFixture::expiry(mode))
        );
        let mut timer = running;
        assert_eq!(
            timer.arm_next(ClockInstant::from_nanos(110))?.transition(),
            PuzzleTimerEnumTransition::Expired(PuzzleTimerFixture::expiry(mode))
        );
        assert_eq!(
            timer
                .check_action(ClockInstant::from_nanos(111))?
                .transition(),
            PuzzleTimerEnumTransition::Expired(PuzzleTimerFixture::expiry(mode))
        );
    }
    Ok(())
}

#[test]
fn session_expiry_is_terminal_while_puzzle_expiry_allows_a_new_readiness_gate()
-> Result<(), Box<dyn std::error::Error>> {
    for mode in [
        PuzzleEnumCountdownMode::Session,
        PuzzleEnumCountdownMode::PerPuzzle,
    ] {
        let mut timer = PuzzleTimerFixture::running(mode)?;
        timer.poll(ClockInstant::from_nanos(1000))?;
        assert_eq!(timer.view(ClockInstant::from_nanos(1000))?.deadline(), None);
        let update = timer.arm_next(ClockInstant::from_nanos(2000))?;
        match mode {
            PuzzleEnumCountdownMode::Session => {
                assert_eq!(
                    update.transition(),
                    PuzzleTimerEnumTransition::Expired(PuzzleTimerEnumExpiry::SessionEnded)
                );
                PuzzleTimerFixture::assert_time(&timer, 2000, 0, 100, 100)?;
            }
            PuzzleEnumCountdownMode::PerPuzzle => {
                assert_eq!(update.transition(), PuzzleTimerEnumTransition::Armed);
                PuzzleTimerFixture::assert_time(&timer, 3000, 100, 100, 0)?;
                timer.update_readiness(
                    ClockInstant::from_nanos(4000),
                    PuzzleTimerEnumReadiness::Ready,
                )?;
                PuzzleTimerFixture::assert_time(&timer, 4025, 75, 125, 25)?;
            }
        }
    }
    Ok(())
}

#[test]
fn interruptions_at_or_after_puzzle_expiry_still_block_the_next_puzzle_until_resume()
-> Result<(), Box<dyn std::error::Error>> {
    for now in [110, 120] {
        let mut timer = PuzzleTimerFixture::running(PuzzleEnumCountdownMode::PerPuzzle)?;
        if now > 110 {
            timer.poll(ClockInstant::from_nanos(110))?;
        }
        let update = timer.update_readiness(
            ClockInstant::from_nanos(now),
            PuzzleTimerEnumReadiness::Hidden,
        )?;
        assert_eq!(
            update.transition(),
            PuzzleTimerEnumTransition::Expired(PuzzleTimerEnumExpiry::PuzzleTimedOut)
        );
        assert_eq!(
            update.view().status(),
            PuzzleTimerEnumStatus::Expired {
                expiry: PuzzleTimerEnumExpiry::PuzzleTimedOut,
                interruption: Some(PuzzleTimerEnumPauseReason::WindowHidden)
            }
        );
        timer.arm_next(ClockInstant::from_nanos(200))?;
        timer.update_readiness(
            ClockInstant::from_nanos(300),
            PuzzleTimerEnumReadiness::Ready,
        )?;
        assert_eq!(
            timer.view(ClockInstant::from_nanos(300))?.status(),
            PuzzleTimerEnumStatus::Paused {
                reason: PuzzleTimerEnumPauseReason::WindowHidden,
                phase: PuzzleTimerEnumPhase::AwaitingReadiness
            }
        );
        PuzzleTimerFixture::assert_time(&timer, 300, 100, 100, 0)?;
        timer.resume(
            ClockInstant::from_nanos(400),
            PuzzleTimerEnumReadiness::Ready,
        )?;
        PuzzleTimerFixture::assert_time(&timer, 410, 90, 110, 10)?;
    }
    Ok(())
}

#[test]
fn recovering_an_expired_puzzle_cannot_start_the_next_puzzle_without_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let timer = PuzzleTimerFixture::running(PuzzleEnumCountdownMode::PerPuzzle)?;
    let snapshot = timer.snapshot(ClockInstant::from_nanos(110))?;
    let mut restored = PuzzleTimer::recover(&snapshot);
    restored.arm_next(ClockInstant::from_nanos(0))?;
    restored.update_readiness(
        ClockInstant::from_nanos(1000),
        PuzzleTimerEnumReadiness::Ready,
    )?;
    assert_eq!(
        restored.view(ClockInstant::from_nanos(1000))?.status(),
        PuzzleTimerEnumStatus::Paused {
            reason: PuzzleTimerEnumPauseReason::Recovery,
            phase: PuzzleTimerEnumPhase::AwaitingReadiness
        }
    );
    PuzzleTimerFixture::assert_time(&restored, 1000, 100, 100, 0)?;
    restored.resume(
        ClockInstant::from_nanos(2000),
        PuzzleTimerEnumReadiness::Ready,
    )?;
    PuzzleTimerFixture::assert_time(&restored, 2025, 75, 125, 25)?;
    Ok(())
}

#[test]
fn backward_time_and_deadline_overflow_leave_every_field_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let mut timer = PuzzleTimerFixture::running(PuzzleEnumCountdownMode::Session)?;
    timer.poll(ClockInstant::from_nanos(40))?;
    let before = timer;
    assert!(matches!(
        timer.poll(ClockInstant::from_nanos(39)),
        Err(PuzzleTimerError::Duration(ClockDurationError::TimeOrder))
    ));
    assert_eq!(timer, before);
    assert!(matches!(
        timer.view(ClockInstant::from_nanos(39)),
        Err(PuzzleTimerError::Duration(ClockDurationError::TimeOrder))
    ));
    assert_eq!(timer, before);
    timer.pause(
        ClockInstant::from_nanos(50),
        PuzzleTimerEnumPauseReason::Manual,
    )?;
    let before = timer;
    assert!(matches!(
        timer.resume(
            ClockInstant::from_nanos(u64::MAX),
            PuzzleTimerEnumReadiness::Ready
        ),
        Err(PuzzleTimerError::Duration(ClockDurationError::Overflow))
    ));
    assert_eq!(timer, before);
    let mut waiting = PuzzleTimer::new(timer.config());
    let before = waiting;
    assert!(matches!(
        waiting.update_readiness(
            ClockInstant::from_nanos(u64::MAX),
            PuzzleTimerEnumReadiness::Ready
        ),
        Err(PuzzleTimerError::Duration(ClockDurationError::Overflow))
    ));
    assert_eq!(waiting, before);
    Ok(())
}

#[test]
fn aggregate_elapsed_overflow_does_not_partially_charge_the_puzzle()
-> Result<(), Box<dyn std::error::Error>> {
    let snapshot = PuzzleTimerSnapshot::from_persisted(PuzzleTimerSnapshotOptions {
        config: PuzzleTimerFixture::config(PuzzleEnumCountdownMode::PerPuzzle)?,
        remaining: ClockDuration::from_nanos(100),
        elapsed: ClockDuration::from_nanos(u64::MAX),
        puzzle_elapsed: ClockDuration::ZERO,
        status: PuzzleTimerEnumStatus::AwaitingReadiness,
    })?;
    let mut timer = PuzzleTimer::recover(&snapshot);
    timer.resume(ClockInstant::from_nanos(0), PuzzleTimerEnumReadiness::Ready)?;
    let before = timer;
    assert!(matches!(
        timer.check_action(ClockInstant::from_nanos(1)),
        Err(PuzzleTimerError::Duration(ClockDurationError::Overflow))
    ));
    assert_eq!(timer, before);
    Ok(())
}

#[test]
fn active_snapshots_recover_paused_without_downtime_or_a_new_budget()
-> Result<(), Box<dyn std::error::Error>> {
    for mode in [
        PuzzleEnumCountdownMode::Session,
        PuzzleEnumCountdownMode::PerPuzzle,
    ] {
        let timer = PuzzleTimerFixture::running(mode)?;
        let snapshot = timer.snapshot(ClockInstant::from_nanos(45))?;
        let validated = PuzzleTimerSnapshot::from_persisted(snapshot.persisted())?;
        assert_eq!(snapshot, validated);
        let mut restored = PuzzleTimer::recover(&validated);
        assert_eq!(
            restored.view(ClockInstant::from_nanos(0))?.status(),
            PuzzleTimerEnumStatus::Paused {
                reason: PuzzleTimerEnumPauseReason::Recovery,
                phase: PuzzleTimerEnumPhase::Active
            }
        );
        PuzzleTimerFixture::assert_time(&restored, 1_000_000, 65, 35, 35)?;
        restored.update_readiness(
            ClockInstant::from_nanos(1_000_000),
            PuzzleTimerEnumReadiness::Ready,
        )?;
        PuzzleTimerFixture::assert_time(&restored, 2_000_000, 65, 35, 35)?;
        restored.resume(
            ClockInstant::from_nanos(2_000_000),
            PuzzleTimerEnumReadiness::InputBlocked,
        )?;
        PuzzleTimerFixture::assert_time(&restored, 2_000_010, 55, 45, 45)?;
    }
    Ok(())
}

#[test]
fn recovery_preserves_waiting_review_exhaustion_and_expired_phases()
-> Result<(), Box<dyn std::error::Error>> {
    let mut timer = PuzzleTimerFixture::running(PuzzleEnumCountdownMode::Session)?;
    timer.enter_review(ClockInstant::from_nanos(40))?;
    for phase in [
        PuzzleTimerEnumPhase::Review,
        PuzzleTimerEnumPhase::Exhausted,
        PuzzleTimerEnumPhase::AwaitingReadiness,
    ] {
        match phase {
            PuzzleTimerEnumPhase::Review => {}
            PuzzleTimerEnumPhase::Exhausted => {
                timer.exhaust_collection(ClockInstant::from_nanos(50))?;
            }
            PuzzleTimerEnumPhase::AwaitingReadiness => {
                timer.arm_next(ClockInstant::from_nanos(60))?;
            }
            PuzzleTimerEnumPhase::Active => return Err("Unexpected active phase".into()),
        }
        let snapshot = timer.snapshot(ClockInstant::from_nanos(100))?;
        let mut restored = PuzzleTimer::recover(&snapshot);
        assert_eq!(
            restored.view(ClockInstant::from_nanos(0))?.status(),
            PuzzleTimerEnumStatus::Paused {
                reason: PuzzleTimerEnumPauseReason::Recovery,
                phase
            }
        );
        restored.resume(
            ClockInstant::from_nanos(200),
            PuzzleTimerEnumReadiness::InputBlocked,
        )?;
        let expected_status = match phase {
            PuzzleTimerEnumPhase::AwaitingReadiness => PuzzleTimerEnumStatus::AwaitingReadiness,
            PuzzleTimerEnumPhase::Review => PuzzleTimerEnumStatus::Review,
            PuzzleTimerEnumPhase::Exhausted => PuzzleTimerEnumStatus::Exhausted,
            PuzzleTimerEnumPhase::Active => return Err("Unexpected active phase".into()),
        };
        assert_eq!(
            restored.view(ClockInstant::from_nanos(1000))?.status(),
            expected_status
        );
        assert_eq!(
            restored.view(ClockInstant::from_nanos(1000))?.remaining(),
            snapshot.persisted().remaining
        );
    }
    let timer = PuzzleTimerFixture::running(PuzzleEnumCountdownMode::Session)?;
    let snapshot = timer.snapshot(ClockInstant::from_nanos(110))?;
    let mut restored = PuzzleTimer::recover(&snapshot);
    assert_eq!(
        restored
            .check_action(ClockInstant::from_nanos(0))?
            .transition(),
        PuzzleTimerEnumTransition::Expired(PuzzleTimerEnumExpiry::SessionEnded)
    );
    Ok(())
}

#[test]
fn invalid_snapshot_balances_statuses_and_expiry_modes_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let timer = PuzzleTimerFixture::running(PuzzleEnumCountdownMode::Session)?;
    let valid = timer.snapshot(ClockInstant::from_nanos(40))?.persisted();
    for invalid in [
        PuzzleTimerSnapshotOptions {
            remaining: ClockDuration::from_nanos(71),
            ..valid
        },
        PuzzleTimerSnapshotOptions {
            elapsed: ClockDuration::from_nanos(29),
            ..valid
        },
        PuzzleTimerSnapshotOptions {
            puzzle_elapsed: ClockDuration::from_nanos(31),
            ..valid
        },
        PuzzleTimerSnapshotOptions {
            status: PuzzleTimerEnumStatus::AwaitingReadiness,
            ..valid
        },
        PuzzleTimerSnapshotOptions {
            status: PuzzleTimerEnumStatus::Expired {
                expiry: PuzzleTimerEnumExpiry::SessionEnded,
                interruption: None,
            },
            ..valid
        },
        PuzzleTimerSnapshotOptions {
            status: PuzzleTimerEnumStatus::Paused {
                reason: PuzzleTimerEnumPauseReason::Manual,
                phase: PuzzleTimerEnumPhase::AwaitingReadiness,
            },
            ..valid
        },
    ] {
        assert!(PuzzleTimerSnapshot::from_persisted(invalid).is_err());
    }
    let expired = timer.snapshot(ClockInstant::from_nanos(110))?.persisted();
    for invalid in [
        PuzzleTimerSnapshotOptions {
            status: PuzzleTimerEnumStatus::Running,
            ..expired
        },
        PuzzleTimerSnapshotOptions {
            status: PuzzleTimerEnumStatus::Expired {
                expiry: PuzzleTimerEnumExpiry::PuzzleTimedOut,
                interruption: None,
            },
            ..expired
        },
    ] {
        assert!(matches!(
            PuzzleTimerSnapshot::from_persisted(invalid),
            Err(PuzzleTimerError::SnapshotStatus)
        ));
    }
    let timer = PuzzleTimerFixture::running(PuzzleEnumCountdownMode::PerPuzzle)?;
    let valid = timer.snapshot(ClockInstant::from_nanos(40))?.persisted();
    assert!(matches!(
        PuzzleTimerSnapshot::from_persisted(PuzzleTimerSnapshotOptions {
            puzzle_elapsed: ClockDuration::from_nanos(29),
            ..valid
        }),
        Err(PuzzleTimerError::SnapshotBalance)
    ));
    Ok(())
}
