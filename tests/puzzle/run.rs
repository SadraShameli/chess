use std::num::{NonZeroU64, NonZeroUsize};
use std::time::Duration;

use chess::board::position::BoardPosition;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::puzzle::attempt::PuzzleAttemptId;
use chess::puzzle::definition::PuzzleDefinition;
use chess::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumEvent, PuzzleEnumMoveFeedback,
    PuzzleEnumPractice, PuzzleEnumPrimaryOutcome,
};
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::puzzle::metadata::PuzzleDefinitionMetadata;
use chess::puzzle::random_provider::PuzzleRandomProvider;
use chess::puzzle::run::{
    PuzzleRun, PuzzleRunEnumAction, PuzzleRunEnumConfirmation, PuzzleRunEnumContent,
    PuzzleRunEnumEndReason, PuzzleRunEnumStatus, PuzzleRunError,
};
use chess::puzzle::selection::{PuzzleSelection, PuzzleSelectionDataset, PuzzleSelectionFilter};
use chess::puzzle::session::{
    PuzzleSessionConfig, PuzzleSessionContext, PuzzleSessionError, PuzzleSessionId,
    PuzzleSessionRevision,
};
use chess::puzzle::timer::{
    PuzzleTimerEnumExpiry, PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness,
};

use crate::puzzle_fixture::PuzzleFixture;

#[derive(Clone)]
struct PuzzleRunFixtureRandom;

impl PuzzleRandomProvider for PuzzleRunFixtureRandom {
    fn index(&mut self, _alternatives: NonZeroUsize) -> usize {
        0
    }
}

struct PuzzleRunFixture;

enum PuzzleRunFixtureEnumTimedAction {
    Hint,
    Retry,
    Reply,
}

impl PuzzleRunFixture {
    fn time(seconds: u64) -> ClockInstant {
        ClockInstant::from_nanos(seconds * 1_000_000_000)
    }

    fn definitions(
        count: usize,
        line: &[&str],
    ) -> Result<Vec<PuzzleDefinition>, Box<dyn std::error::Error>> {
        let position = BoardPosition::standard();
        (0..count)
            .map(|index| {
                Ok(PuzzleDefinition::new(
                    PuzzleDefinitionIdentity::new("run".into(), "v1".into(), index.to_string())?,
                    position.clone(),
                    vec![PuzzleFixture::line(&position, line)?],
                    PuzzleDefinitionMetadata::default(),
                )?)
            })
            .collect()
    }

    fn run(
        mode: PuzzleEnumCountdownMode,
        count: usize,
        line: &[&str],
    ) -> Result<PuzzleRun<PuzzleRunFixtureRandom>, Box<dyn std::error::Error>> {
        let definitions = Self::definitions(count, line)?;
        let dataset = PuzzleSelectionDataset::from_identity(definitions[0].identity());
        let selection = PuzzleSelection::new(
            dataset,
            definitions,
            PuzzleSelectionFilter::default(),
            PuzzleRunFixtureRandom,
        )?;
        Ok(PuzzleRun::new(
            PuzzleSessionId::new(NonZeroU64::MIN),
            PuzzleSessionConfig::new(mode, Duration::from_secs(10))?,
            selection,
        )?)
    }

    fn ready(
        run: &mut PuzzleRun<PuzzleRunFixtureRandom>,
        seconds: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        run.update_readiness(
            &run.context(),
            Self::time(seconds),
            PuzzleTimerEnumReadiness::Ready,
        )?;
        Ok(())
    }

    fn movement(
        run: &mut PuzzleRun<PuzzleRunFixtureRandom>,
        seconds: u64,
        uci: &str,
    ) -> Result<PuzzleRunEnumAction<PuzzleEnumMoveFeedback>, Box<dyn std::error::Error>> {
        let view = run.view(Self::time(seconds))?;
        let movement = match view.content() {
            PuzzleRunEnumContent::Board(attempt) | PuzzleRunEnumContent::Review(attempt) => {
                attempt.position().parse_uci(uci)?
            }
            PuzzleRunEnumContent::Hidden => return Err("The fixture board is hidden".into()),
        };
        Ok(run.submit_move(&run.context(), Self::time(seconds), movement)?)
    }
}

#[test]
fn starts_only_when_ready_and_resume_remains_explicit_after_every_interruption()
-> Result<(), Box<dyn std::error::Error>> {
    for reason in [
        PuzzleTimerEnumPauseReason::Manual,
        PuzzleTimerEnumPauseReason::FocusLost,
        PuzzleTimerEnumPauseReason::WindowHidden,
        PuzzleTimerEnumPauseReason::Sleep,
    ] {
        let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::Session, 2, &["e2e4"])?;
        run.update_readiness(
            &run.context(),
            PuzzleRunFixture::time(20),
            PuzzleTimerEnumReadiness::InputBlocked,
        )?;
        assert_eq!(
            run.view(PuzzleRunFixture::time(20))?.timer().elapsed(),
            ClockDuration::ZERO
        );
        PuzzleRunFixture::ready(&mut run, 20)?;
        run.pause(&run.context(), PuzzleRunFixture::time(22), reason)?;
        assert!(matches!(
            run.view(PuzzleRunFixture::time(100))?.content(),
            PuzzleRunEnumContent::Hidden
        ));
        PuzzleRunFixture::ready(&mut run, 100)?;
        assert!(matches!(
            run.view(PuzzleRunFixture::time(100))?.content(),
            PuzzleRunEnumContent::Hidden
        ));
        assert!(
            run.request_hint(&run.context(), PuzzleRunFixture::time(100))
                .is_err()
        );
        run.resume(
            &run.context(),
            PuzzleRunFixture::time(100),
            PuzzleTimerEnumReadiness::Ready,
        )?;
        assert_eq!(
            run.view(PuzzleRunFixture::time(101))?
                .timer()
                .elapsed()
                .as_duration(),
            Duration::from_secs(3)
        );
    }
    Ok(())
}

#[test]
fn review_waits_for_next_and_the_next_board_needs_a_fresh_readiness_acknowledgement()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::Session, 2, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    assert_eq!(
        PuzzleRunFixture::movement(&mut run, 2, "e2e4")?,
        PuzzleRunEnumAction::Applied(PuzzleEnumMoveFeedback::Completed)
    );
    assert!(matches!(
        run.view(PuzzleRunFixture::time(50))?.content(),
        PuzzleRunEnumContent::Review(_)
    ));
    assert_eq!(
        run.view(PuzzleRunFixture::time(50))?
            .timer()
            .remaining()
            .as_duration(),
        Duration::from_secs(8)
    );
    run.next(&run.context(), PuzzleRunFixture::time(50))?;
    assert_eq!(
        run.view(PuzzleRunFixture::time(80))?.status(),
        PuzzleRunEnumStatus::AwaitingReadiness
    );
    assert!(
        run.request_hint(&run.context(), PuzzleRunFixture::time(80))
            .is_err()
    );
    PuzzleRunFixture::ready(&mut run, 80)?;
    assert_eq!(
        run.view(PuzzleRunFixture::time(81))?
            .timer()
            .remaining()
            .as_duration(),
        Duration::from_secs(7)
    );
    assert_eq!(run.previous_attempts().len(), 1);
    assert_eq!(
        run.previous_attempts()[0].elapsed_active().as_duration(),
        Duration::from_secs(2)
    );
    Ok(())
}

#[test]
fn active_replies_hints_retries_and_practice_completion_share_one_continuous_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(
        PuzzleEnumCountdownMode::Session,
        1,
        &["e2e4", "e7e5", "g1f3"],
    )?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    run.request_hint(&run.context(), PuzzleRunFixture::time(1))?;
    PuzzleRunFixture::movement(&mut run, 2, "d2d4")?;
    run.retry_practice(&run.context(), PuzzleRunFixture::time(3))?;
    PuzzleRunFixture::movement(&mut run, 4, "e2e4")?;
    run.play_opponent_reply(&run.context(), PuzzleRunFixture::time(6))?;
    PuzzleRunFixture::movement(&mut run, 8, "g1f3")?;
    let summary = run.summary(PuzzleRunFixture::time(100))?;
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Failed), 1);
    assert_eq!(
        summary.primary_count(PuzzleEnumPrimaryOutcome::Solved(
            PuzzleEnumAssistance::Assisted
        )),
        0
    );
    assert_eq!(summary.practice_completions(), 1);
    assert_eq!(summary.hints(), 1);
    assert_eq!(summary.elapsed_active(), Duration::from_secs(8));
    assert!(
        matches!(run.view(PuzzleRunFixture::time(100))?.content(), PuzzleRunEnumContent::Review(attempt) if attempt.practice() == PuzzleEnumPractice::Completed)
    );
    Ok(())
}

#[test]
fn skip_preserves_failure_and_session_time_without_revealing_the_skipped_solution()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::Session, 2, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    PuzzleRunFixture::movement(&mut run, 1, "d2d4")?;
    run.skip(&run.context(), PuzzleRunFixture::time(3))?;
    let previous = &run.previous_attempts()[0];
    assert_eq!(
        previous.attempt().primary_outcome(),
        Some(PuzzleEnumPrimaryOutcome::Failed)
    );
    assert!(
        previous
            .attempt()
            .events()
            .contains(&PuzzleEnumEvent::Skipped)
    );
    assert_eq!(
        previous.elapsed_active().as_duration(),
        Duration::from_secs(3)
    );
    assert_eq!(
        run.view(PuzzleRunFixture::time(100))?.status(),
        PuzzleRunEnumStatus::AwaitingReadiness
    );
    assert_eq!(
        run.view(PuzzleRunFixture::time(100))?
            .timer()
            .remaining()
            .as_duration(),
        Duration::from_secs(7)
    );
    Ok(())
}

#[test]
fn session_expiry_wins_at_the_action_deadline_and_applies_once()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::Session, 2, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    let movement = BoardPosition::standard().parse_uci("e2e4")?;
    assert_eq!(
        run.submit_move(&run.context(), PuzzleRunFixture::time(10), movement)?,
        PuzzleRunEnumAction::Expired(PuzzleTimerEnumExpiry::SessionEnded)
    );
    assert_eq!(run.poll(PuzzleRunFixture::time(11))?, None);
    assert_eq!(
        run.view(PuzzleRunFixture::time(11))?.status(),
        PuzzleRunEnumStatus::Ended(PuzzleRunEnumEndReason::SessionExpired)
    );
    let summary = run.summary(PuzzleRunFixture::time(11))?;
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::TimedOut), 1);
    assert_eq!(summary.timeout_events(PuzzleEnumCountdownMode::Session), 1);
    assert_eq!(summary.elapsed_active(), Duration::from_secs(10));
    Ok(())
}

#[test]
fn expiry_checks_precede_hints_retries_and_random_opponent_replies()
-> Result<(), Box<dyn std::error::Error>> {
    for operation in [
        PuzzleRunFixtureEnumTimedAction::Hint,
        PuzzleRunFixtureEnumTimedAction::Retry,
        PuzzleRunFixtureEnumTimedAction::Reply,
    ] {
        let mut run = PuzzleRunFixture::run(
            PuzzleEnumCountdownMode::Session,
            1,
            &["e2e4", "e7e5", "g1f3"],
        )?;
        PuzzleRunFixture::ready(&mut run, 0)?;
        let outcome = match operation {
            PuzzleRunFixtureEnumTimedAction::Hint => run
                .request_hint(&run.context(), PuzzleRunFixture::time(10))?
                .map(|_| ()),
            PuzzleRunFixtureEnumTimedAction::Retry => {
                PuzzleRunFixture::movement(&mut run, 1, "d2d4")?;
                run.retry_practice(&run.context(), PuzzleRunFixture::time(10))?
            }
            PuzzleRunFixtureEnumTimedAction::Reply => {
                PuzzleRunFixture::movement(&mut run, 1, "e2e4")?;
                run.play_opponent_reply(&run.context(), PuzzleRunFixture::time(10))?
                    .map(|_| ())
            }
        };
        assert_eq!(
            outcome,
            PuzzleRunEnumAction::Expired(PuzzleTimerEnumExpiry::SessionEnded)
        );
        assert_eq!(run.summary(PuzzleRunFixture::time(11))?.hints(), 0);
    }
    Ok(())
}

#[test]
fn per_puzzle_timeout_advances_once_and_waits_for_the_next_board()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::PerPuzzle, 2, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    let old = run.context();
    assert_eq!(
        run.poll(PuzzleRunFixture::time(50))?,
        Some(PuzzleTimerEnumExpiry::PuzzleTimedOut)
    );
    assert_eq!(run.poll(PuzzleRunFixture::time(100))?, None);
    assert_ne!(old, run.context());
    assert_eq!(run.previous_attempts().len(), 1);
    assert_eq!(
        run.summary(PuzzleRunFixture::time(100))?
            .primary_count(PuzzleEnumPrimaryOutcome::TimedOut),
        1
    );
    assert_eq!(
        run.view(PuzzleRunFixture::time(100))?
            .timer()
            .remaining()
            .as_duration(),
        Duration::from_secs(10)
    );
    PuzzleRunFixture::ready(&mut run, 100)?;
    assert_eq!(
        run.view(PuzzleRunFixture::time(102))?
            .timer()
            .puzzle_elapsed()
            .as_duration(),
        Duration::from_secs(2)
    );
    Ok(())
}

#[test]
fn collection_exhaustion_waits_for_explicit_reshuffle_and_carries_the_session_balance()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::Session, 1, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    run.skip(&run.context(), PuzzleRunFixture::time(3))?;
    assert_eq!(
        run.view(PuzzleRunFixture::time(100))?.status(),
        PuzzleRunEnumStatus::Exhausted
    );
    assert!(matches!(
        run.view(PuzzleRunFixture::time(100))?.content(),
        PuzzleRunEnumContent::Hidden
    ));
    run.reshuffle_and_continue(&run.context(), PuzzleRunFixture::time(100))?;
    assert_eq!(
        run.view(PuzzleRunFixture::time(200))?
            .timer()
            .remaining()
            .as_duration(),
        Duration::from_secs(7)
    );
    assert_eq!(run.previous_attempts().len(), 1);
    PuzzleRunFixture::ready(&mut run, 200)?;
    assert_eq!(
        run.view(PuzzleRunFixture::time(201))?
            .timer()
            .remaining()
            .as_duration(),
        Duration::from_secs(6)
    );
    Ok(())
}

#[test]
fn confirmed_end_at_puzzle_expiry_finishes_without_advancing_or_skipping_another_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::PerPuzzle, 2, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    let original = run.context();
    let result = run.end_run(
        &original,
        PuzzleRunFixture::time(10),
        PuzzleRunEnumConfirmation::Confirmed,
    )?;
    assert_eq!(
        result,
        PuzzleRunEnumAction::Expired(PuzzleTimerEnumExpiry::PuzzleTimedOut)
    );
    assert_eq!(
        run.view(PuzzleRunFixture::time(20))?.status(),
        PuzzleRunEnumStatus::Ended(PuzzleRunEnumEndReason::Confirmed)
    );
    assert!(run.previous_attempts().is_empty());
    let summary = run.summary(PuzzleRunFixture::time(20))?;
    assert_eq!(summary.finalized(), 1);
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::TimedOut), 1);
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Skipped), 0);
    assert_eq!(
        summary.timeout_events(PuzzleEnumCountdownMode::PerPuzzle),
        1
    );
    assert_eq!(summary.elapsed_active(), Duration::from_secs(10));
    Ok(())
}

#[test]
fn end_run_requires_confirmation_and_keeps_a_separate_end_reason()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::Session, 1, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    let before = run.context();
    assert!(matches!(
        run.end_run(
            &before,
            PuzzleRunFixture::time(2),
            PuzzleRunEnumConfirmation::Unconfirmed
        ),
        Err(PuzzleRunError::ConfirmationRequired)
    ));
    assert_eq!(run.context(), before);
    run.end_run(
        &before,
        PuzzleRunFixture::time(3),
        PuzzleRunEnumConfirmation::Confirmed,
    )?;
    assert_eq!(
        run.view(PuzzleRunFixture::time(100))?.status(),
        PuzzleRunEnumStatus::Ended(PuzzleRunEnumEndReason::Confirmed)
    );
    assert_eq!(
        run.summary(PuzzleRunFixture::time(100))?
            .primary_count(PuzzleEnumPrimaryOutcome::Skipped),
        1
    );
    assert_eq!(
        run.summary(PuzzleRunFixture::time(100))?.elapsed_active(),
        Duration::from_secs(3)
    );
    Ok(())
}

#[test]
fn rejected_illegal_or_stale_input_preserves_the_attempt_and_continuous_charge()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::Session, 1, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    let stale = run.context();
    let illegal = BoardPosition::standard()
        .play_uci("e2e4")?
        .parse_uci("e7e5")?;
    assert!(
        run.submit_move(&stale, PuzzleRunFixture::time(2), illegal)
            .is_err()
    );
    assert_eq!(run.context(), stale);
    assert_eq!(run.summary(PuzzleRunFixture::time(3))?.finalized(), 0);
    assert_eq!(
        run.summary(PuzzleRunFixture::time(3))?.elapsed_active(),
        Duration::from_secs(3)
    );
    run.request_hint(&stale, PuzzleRunFixture::time(3))?;
    assert!(matches!(
        run.request_hint(&stale, PuzzleRunFixture::time(4)),
        Err(PuzzleRunError::Session(
            PuzzleSessionError::StaleRevision { .. }
        ))
    ));
    assert_eq!(run.summary(PuzzleRunFixture::time(5))?.hints(), 1);
    Ok(())
}

#[test]
fn recovered_active_and_review_runs_hide_content_until_explicit_resume()
-> Result<(), Box<dyn std::error::Error>> {
    for solved in [false, true] {
        let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::Session, 2, &["e2e4"])?;
        PuzzleRunFixture::ready(&mut run, 0)?;
        if solved {
            PuzzleRunFixture::movement(&mut run, 2, "e2e4")?;
        }
        let stale = run.context();
        let snapshot = run.snapshot(PuzzleRunFixture::time(3))?;
        let mut recovered = PuzzleRun::recover(
            snapshot,
            PuzzleRunFixture::definitions(2, &["e2e4"])?,
            PuzzleRunFixtureRandom,
        )?;
        assert!(matches!(
            recovered.view(PuzzleRunFixture::time(100))?.content(),
            PuzzleRunEnumContent::Hidden
        ));
        assert_ne!(recovered.context(), stale);
        PuzzleRunFixture::ready(&mut recovered, 100)?;
        assert!(matches!(
            recovered.view(PuzzleRunFixture::time(100))?.content(),
            PuzzleRunEnumContent::Hidden
        ));
        recovered.resume(
            &recovered.context(),
            PuzzleRunFixture::time(100),
            PuzzleTimerEnumReadiness::Ready,
        )?;
        let expected_elapsed = if solved { 2 } else { 4 };
        assert_eq!(
            recovered
                .summary(PuzzleRunFixture::time(101))?
                .elapsed_active(),
            Duration::from_secs(expected_elapsed)
        );
    }
    Ok(())
}

#[test]
fn final_per_puzzle_timeout_retains_exact_attempt_time_through_exhaustion_and_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::PerPuzzle, 1, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    run.poll(PuzzleRunFixture::time(10))?;
    assert_eq!(
        run.view(PuzzleRunFixture::time(20))?.status(),
        PuzzleRunEnumStatus::Exhausted
    );
    assert_eq!(
        run.summary(PuzzleRunFixture::time(20))?.elapsed_active(),
        Duration::from_secs(10)
    );
    let snapshot = run.snapshot(PuzzleRunFixture::time(20))?;
    let mut recovered = PuzzleRun::recover(
        snapshot,
        PuzzleRunFixture::definitions(1, &["e2e4"])?,
        PuzzleRunFixtureRandom,
    )?;
    assert!(matches!(
        recovered.view(PuzzleRunFixture::time(30))?.content(),
        PuzzleRunEnumContent::Hidden
    ));
    assert_eq!(
        recovered
            .summary(PuzzleRunFixture::time(30))?
            .elapsed_active(),
        Duration::from_secs(10)
    );
    recovered.resume(
        &recovered.context(),
        PuzzleRunFixture::time(30),
        PuzzleTimerEnumReadiness::BoardNotReady,
    )?;
    assert_eq!(
        recovered.view(PuzzleRunFixture::time(30))?.status(),
        PuzzleRunEnumStatus::Exhausted
    );
    recovered.reshuffle_and_continue(&recovered.context(), PuzzleRunFixture::time(40))?;
    assert_eq!(
        recovered.previous_attempts()[0]
            .elapsed_active()
            .as_duration(),
        Duration::from_secs(10)
    );
    PuzzleRunFixture::ready(&mut recovered, 50)?;
    PuzzleRunFixture::movement(&mut recovered, 52, "e2e4")?;
    assert_eq!(
        recovered
            .summary(PuzzleRunFixture::time(100))?
            .elapsed_active(),
        Duration::from_secs(12)
    );
    assert_eq!(
        recovered.summary(PuzzleRunFixture::time(100))?.finalized(),
        2
    );
    Ok(())
}

#[test]
fn failed_primary_survives_timeout_and_confirmed_end_with_separate_secondary_events()
-> Result<(), Box<dyn std::error::Error>> {
    for mode in [
        PuzzleEnumCountdownMode::Session,
        PuzzleEnumCountdownMode::PerPuzzle,
    ] {
        for end_manually in [false, true] {
            let mut run = PuzzleRunFixture::run(mode, 1, &["e2e4"])?;
            PuzzleRunFixture::ready(&mut run, 0)?;
            PuzzleRunFixture::movement(&mut run, 1, "d2d4")?;
            if end_manually {
                run.end_run(
                    &run.context(),
                    PuzzleRunFixture::time(3),
                    PuzzleRunEnumConfirmation::Confirmed,
                )?;
            } else {
                run.poll(PuzzleRunFixture::time(10))?;
            }
            let summary = run.summary(PuzzleRunFixture::time(100))?;
            assert_eq!(summary.finalized(), 1);
            assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Failed), 1);
            assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::TimedOut), 0);
            assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Skipped), 0);
            if end_manually {
                assert_eq!(summary.skip_events(), 1);
                assert_eq!(summary.elapsed_active(), Duration::from_secs(3));
            } else {
                assert_eq!(summary.timeout_events(mode), 1);
                assert_eq!(summary.elapsed_active(), Duration::from_secs(10));
            }
        }
    }
    Ok(())
}

#[test]
fn pause_at_expiry_keeps_the_following_board_hidden_until_explicit_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::PerPuzzle, 2, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    assert_eq!(
        run.pause(
            &run.context(),
            PuzzleRunFixture::time(10),
            PuzzleTimerEnumPauseReason::FocusLost
        )?,
        Some(PuzzleTimerEnumExpiry::PuzzleTimedOut)
    );
    PuzzleRunFixture::ready(&mut run, 100)?;
    assert!(matches!(
        run.view(PuzzleRunFixture::time(100))?.content(),
        PuzzleRunEnumContent::Hidden
    ));
    assert!(
        run.resume(
            &run.context(),
            PuzzleRunFixture::time(100),
            PuzzleTimerEnumReadiness::InputBlocked
        )
        .is_err()
    );
    run.resume(
        &run.context(),
        PuzzleRunFixture::time(100),
        PuzzleTimerEnumReadiness::Ready,
    )?;
    assert_eq!(
        run.view(PuzzleRunFixture::time(102))?
            .timer()
            .remaining()
            .as_duration(),
        Duration::from_secs(8)
    );
    assert_eq!(
        run.summary(PuzzleRunFixture::time(102))?.elapsed_active(),
        Duration::from_secs(12)
    );
    Ok(())
}

#[derive(Clone)]
struct PuzzleRunRejectingRandom {
    calls: usize,
}

impl PuzzleRandomProvider for PuzzleRunRejectingRandom {
    fn index(&mut self, alternatives: NonZeroUsize) -> usize {
        self.calls += 1;
        alternatives.get()
    }
}

#[test]
fn invalid_random_reply_rolls_back_the_provider_transcript_and_current_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let definitions = PuzzleRunFixture::definitions(1, &["e2e4", "e7e5", "g1f3"])?;
    let selection = PuzzleSelection::new(
        PuzzleSelectionDataset::from_identity(definitions[0].identity()),
        definitions.clone(),
        PuzzleSelectionFilter::default(),
        PuzzleRunRejectingRandom { calls: 0 },
    )?;
    let mut run = PuzzleRun::new(
        PuzzleSessionId::new(NonZeroU64::MIN),
        PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
        selection,
    )?;
    run.update_readiness(
        &run.context(),
        PuzzleRunFixture::time(0),
        PuzzleTimerEnumReadiness::Ready,
    )?;
    run.submit_move(
        &run.context(),
        PuzzleRunFixture::time(1),
        BoardPosition::standard().parse_uci("e2e4")?,
    )?;
    let context = run.context();
    let before = run.snapshot(PuzzleRunFixture::time(2))?;
    for _ in 0..2 {
        assert!(
            run.play_opponent_reply(&context, PuzzleRunFixture::time(2))
                .is_err()
        );
        assert_eq!(run.context(), context);
        assert_eq!(run.snapshot(PuzzleRunFixture::time(2))?, before);
    }
    let recovered = PuzzleRun::recover(before, definitions, PuzzleRunRejectingRandom { calls: 0 })?;
    assert_eq!(
        recovered
            .summary(PuzzleRunFixture::time(20))?
            .elapsed_active(),
        Duration::from_secs(2)
    );
    Ok(())
}

#[test]
fn hidden_resume_and_recovery_with_replaced_definition_fail_without_exposing_content()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::Session, 1, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    run.pause(
        &run.context(),
        PuzzleRunFixture::time(1),
        PuzzleTimerEnumPauseReason::Sleep,
    )?;
    let context = run.context();
    assert!(
        run.resume(
            &context,
            PuzzleRunFixture::time(2),
            PuzzleTimerEnumReadiness::Hidden
        )
        .is_err()
    );
    assert_eq!(run.context(), context);
    assert!(matches!(
        run.view(PuzzleRunFixture::time(2))?.content(),
        PuzzleRunEnumContent::Hidden
    ));
    let snapshot = run.snapshot(PuzzleRunFixture::time(2))?;
    assert!(matches!(
        PuzzleRun::recover(
            snapshot,
            PuzzleRunFixture::definitions(1, &["d2d4"])?,
            PuzzleRunFixtureRandom
        ),
        Err(PuzzleRunError::RecoveryMismatch)
    ));
    Ok(())
}

#[test]
fn foreign_session_attempt_and_dataset_contexts_are_rejected_before_any_transition()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::Session, 1, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    let identity = PuzzleRunFixture::definitions(1, &["e2e4"])?[0]
        .identity()
        .clone();
    let foreign = NonZeroU64::MIN
        .checked_add(1)
        .ok_or("Fixture identity overflow")?;
    let contexts = [
        (
            PuzzleSessionContext::new(
                PuzzleSessionId::new(foreign),
                PuzzleAttemptId::new(NonZeroU64::MIN),
                identity.clone(),
                PuzzleSessionRevision::ZERO,
            ),
            false,
        ),
        (
            PuzzleSessionContext::new(
                PuzzleSessionId::new(NonZeroU64::MIN),
                PuzzleAttemptId::new(foreign),
                identity,
                PuzzleSessionRevision::ZERO,
            ),
            false,
        ),
        (
            PuzzleSessionContext::new(
                PuzzleSessionId::new(NonZeroU64::MIN),
                PuzzleAttemptId::new(NonZeroU64::MIN),
                PuzzleDefinitionIdentity::new("run".into(), "v2".into(), "0".into())?,
                PuzzleSessionRevision::ZERO,
            ),
            true,
        ),
    ];
    let before = run.snapshot(PuzzleRunFixture::time(0))?;
    for (context, definition_mismatch) in contexts {
        let error = run.request_hint(&context, PuzzleRunFixture::time(2));
        if definition_mismatch {
            assert!(matches!(
                error,
                Err(PuzzleRunError::Session(
                    PuzzleSessionError::DefinitionMismatch
                ))
            ));
        } else {
            assert!(matches!(
                error,
                Err(PuzzleRunError::Session(
                    PuzzleSessionError::IdentityMismatch
                ))
            ));
        }
        assert_eq!(run.snapshot(PuzzleRunFixture::time(0))?, before);
    }
    assert_eq!(
        run.summary(PuzzleRunFixture::time(3))?.elapsed_active(),
        Duration::from_secs(3)
    );
    Ok(())
}

#[test]
fn recovery_retains_previous_attempts_and_the_current_attempt_time_without_double_counting()
-> Result<(), Box<dyn std::error::Error>> {
    let mut run = PuzzleRunFixture::run(PuzzleEnumCountdownMode::Session, 2, &["e2e4"])?;
    PuzzleRunFixture::ready(&mut run, 0)?;
    run.skip(&run.context(), PuzzleRunFixture::time(3))?;
    PuzzleRunFixture::ready(&mut run, 10)?;
    run.request_hint(&run.context(), PuzzleRunFixture::time(11))?;
    let saved = run.snapshot(PuzzleRunFixture::time(12))?;
    let mut recovered = PuzzleRun::recover(
        saved,
        PuzzleRunFixture::definitions(2, &["e2e4"])?,
        PuzzleRunFixtureRandom,
    )?;
    assert_eq!(recovered.previous_attempts().len(), 1);
    assert_eq!(
        recovered.previous_attempts()[0]
            .elapsed_active()
            .as_duration(),
        Duration::from_secs(3)
    );
    assert_eq!(
        recovered
            .summary(PuzzleRunFixture::time(100))?
            .elapsed_active(),
        Duration::from_secs(5)
    );
    recovered.resume(
        &recovered.context(),
        PuzzleRunFixture::time(100),
        PuzzleTimerEnumReadiness::Ready,
    )?;
    PuzzleRunFixture::movement(&mut recovered, 102, "e2e4")?;
    let summary = recovered.summary(PuzzleRunFixture::time(200))?;
    assert_eq!(summary.finalized(), 2);
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Skipped), 1);
    assert_eq!(
        summary.primary_count(PuzzleEnumPrimaryOutcome::Solved(
            PuzzleEnumAssistance::Assisted
        )),
        1
    );
    assert_eq!(summary.elapsed_active(), Duration::from_secs(7));
    Ok(())
}
