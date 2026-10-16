use std::num::{NonZeroU64, NonZeroUsize};

use chess::board::position::BoardPosition;
use chess::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use chess::puzzle::attempt::PuzzleAttemptId;
use chess::puzzle::checkpoint::{
    PuzzleCheckpoint, PuzzleCheckpointEnumAdvance, PuzzleCheckpointEnumCompletion,
    PuzzleCheckpointEnumFailure, PuzzleCheckpointEnumFreezeCause, PuzzleCheckpointEnumStatus,
    PuzzleCheckpointEnumTrigger, PuzzleCheckpointError, PuzzleCheckpointObservation,
    PuzzleCheckpointSnapshot,
};
use chess::puzzle::definition::PuzzleDefinition;
use chess::puzzle::enums::PuzzleEnumCountdownMode;
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::puzzle::metadata::PuzzleDefinitionMetadata;
use chess::puzzle::random_provider::PuzzleRandomProvider;
use chess::puzzle::run::{PuzzleRun, PuzzleRunEnumContent, PuzzleRunEnumStatus, PuzzleRunSnapshot};
use chess::puzzle::selection::{PuzzleSelection, PuzzleSelectionDataset, PuzzleSelectionFilter};
use chess::puzzle::session::{
    PuzzleSessionConfig, PuzzleSessionContext, PuzzleSessionId, PuzzleSessionRevision,
};
use chess::puzzle::timer::{
    PuzzleTimerEnumPauseReason, PuzzleTimerEnumPhase, PuzzleTimerEnumReadiness,
};

struct PuzzleCheckpointFixture;

impl PuzzleCheckpointFixture {
    fn time(millis: u64) -> ClockInstant {
        ClockInstant::from_nanos(millis * 1_000_000)
    }

    fn observation(
        millis: u64,
        elapsed: u64,
        revision: u64,
        status: PuzzleRunEnumStatus,
    ) -> Result<PuzzleCheckpointObservation, Box<dyn std::error::Error>> {
        Ok(PuzzleCheckpointObservation::new(
            PuzzleSessionContext::new(
                PuzzleSessionId::new(NonZeroU64::MIN),
                PuzzleAttemptId::new(NonZeroU64::MIN),
                PuzzleDefinitionIdentity::new("checkpoint".into(), "v1".into(), "a".into())?,
                PuzzleSessionRevision::new(revision),
            ),
            Self::time(millis),
            ClockDuration::from_millis(elapsed)?,
            status,
        ))
    }

    fn snapshot(
        millis: u64,
        elapsed: u64,
        revision: u64,
        status: PuzzleRunEnumStatus,
    ) -> Result<PuzzleCheckpointSnapshot<Vec<u64>>, Box<dyn std::error::Error>> {
        Ok(PuzzleCheckpointSnapshot::new(
            Self::observation(millis, elapsed, revision, status)?,
            vec![revision, elapsed],
        ))
    }

    fn active() -> Result<PuzzleCheckpoint<Vec<u64>>, Box<dyn std::error::Error>> {
        let mut checkpoint = PuzzleCheckpoint::new(Self::snapshot(
            0,
            0,
            0,
            PuzzleRunEnumStatus::AwaitingReadiness,
        )?)?;
        let token = checkpoint.begin_write(Self::time(0))?;
        checkpoint.complete(
            Self::time(0),
            &token,
            PuzzleCheckpointEnumCompletion::Committed,
        )?;
        checkpoint.record(
            Self::snapshot(0, 0, 1, PuzzleRunEnumStatus::Active)?,
            PuzzleCheckpointEnumTrigger::AcceptedTransition,
        )?;
        Ok(checkpoint)
    }

    fn paused() -> PuzzleRunEnumStatus {
        PuzzleRunEnumStatus::Paused {
            reason: PuzzleTimerEnumPauseReason::Manual,
            phase: PuzzleTimerEnumPhase::Active,
        }
    }

    fn run(
        mode: PuzzleEnumCountdownMode,
    ) -> Result<PuzzleRun<PuzzleCheckpointFixtureRandom>, Box<dyn std::error::Error>> {
        let position = BoardPosition::standard();
        let movement = position.parse_uci("e2e4")?;
        let definition = PuzzleDefinition::new(
            PuzzleDefinitionIdentity::new("real-run".into(), "v1".into(), "a".into())?,
            position,
            vec![vec![movement]],
            PuzzleDefinitionMetadata::default(),
        )?;
        let selection = PuzzleSelection::new(
            PuzzleSelectionDataset::from_identity(definition.identity()),
            vec![definition],
            PuzzleSelectionFilter::default(),
            PuzzleCheckpointFixtureRandom,
        )?;
        Ok(PuzzleRun::new(
            PuzzleSessionId::new(NonZeroU64::MIN),
            PuzzleSessionConfig::defaults(mode),
            selection,
        )?)
    }
}

#[derive(Clone)]
struct PuzzleCheckpointFixtureRandom;

impl PuzzleRandomProvider for PuzzleCheckpointFixtureRandom {
    fn index(&mut self, _alternatives: NonZeroUsize) -> usize {
        0
    }
}

struct PuzzleCheckpointRunFixture {
    run: PuzzleRun<PuzzleCheckpointFixtureRandom>,
    checkpoint: PuzzleCheckpoint<PuzzleRunSnapshot>,
}

impl PuzzleCheckpointRunFixture {
    fn new(mode: PuzzleEnumCountdownMode) -> Result<Self, Box<dyn std::error::Error>> {
        let mut run = PuzzleCheckpointFixture::run(mode)?;
        let zero = PuzzleCheckpointFixture::time(0);
        let payload = run.snapshot(zero)?;
        let checkpoint = PuzzleCheckpoint::new(PuzzleCheckpointSnapshot::new(
            PuzzleCheckpointObservation::from_run(&run, zero)?,
            payload,
        ))?;
        let mut fixture = Self { run, checkpoint };
        fixture.commit(zero)?;
        fixture.run.update_readiness(
            &fixture.run.context(),
            zero,
            PuzzleTimerEnumReadiness::Ready,
        )?;
        fixture.capture(zero)?;
        Ok(fixture)
    }

    fn capture(&mut self, now: ClockInstant) -> Result<(), Box<dyn std::error::Error>> {
        let payload = self.run.snapshot(now)?;
        self.checkpoint.record(
            PuzzleCheckpointSnapshot::new(
                PuzzleCheckpointObservation::from_run(&self.run, now)?,
                payload,
            ),
            PuzzleCheckpointEnumTrigger::AcceptedTransition,
        )?;
        Ok(())
    }

    fn commit(&mut self, now: ClockInstant) -> Result<(), Box<dyn std::error::Error>> {
        let token = self.checkpoint.begin_write(now)?;
        assert_eq!(
            self.checkpoint
                .complete(now, &token, PuzzleCheckpointEnumCompletion::Committed)?,
            PuzzleCheckpointEnumAdvance::Proceed
        );
        Ok(())
    }
}

#[test]
fn cadence_uses_active_elapsed_and_transitions_request_immediately()
-> Result<(), Box<dyn std::error::Error>> {
    let mut checkpoint = PuzzleCheckpointFixture::active()?;
    let token = checkpoint.begin_write(PuzzleCheckpointFixture::time(0))?;
    for millis in [1, 249] {
        assert!(!checkpoint.needs_snapshot(
            &PuzzleCheckpointFixture::observation(millis, millis, 1, PuzzleRunEnumStatus::Active)?,
            PuzzleCheckpointEnumTrigger::Countdown,
        )?);
    }
    assert!(checkpoint.needs_snapshot(
        &PuzzleCheckpointFixture::observation(250, 250, 1, PuzzleRunEnumStatus::Active)?,
        PuzzleCheckpointEnumTrigger::Countdown,
    )?);
    checkpoint.record(
        PuzzleCheckpointFixture::snapshot(250, 250, 1, PuzzleRunEnumStatus::Active)?,
        PuzzleCheckpointEnumTrigger::Countdown,
    )?;
    checkpoint.record(
        PuzzleCheckpointFixture::snapshot(251, 251, 2, PuzzleRunEnumStatus::Active)?,
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    assert_eq!(
        checkpoint.pending().map(|value| value.payload()),
        Some(&vec![1, 0])
    );
    assert_eq!(
        checkpoint.coalesced().map(|value| value.payload()),
        Some(&vec![2, 251])
    );
    checkpoint.complete(
        PuzzleCheckpointFixture::time(251),
        &token,
        PuzzleCheckpointEnumCompletion::Committed,
    )?;
    assert_eq!(
        checkpoint.pending().map(|value| value.payload()),
        Some(&vec![2, 251])
    );
    assert!(checkpoint.coalesced().is_none());
    Ok(())
}

#[test]
fn a_stalled_write_retains_one_original_and_only_the_latest_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    let mut checkpoint = PuzzleCheckpointFixture::active()?;
    let token = checkpoint.begin_write(PuzzleCheckpointFixture::time(0))?;
    for millis in [250, 500, 750] {
        checkpoint.record(
            PuzzleCheckpointFixture::snapshot(millis, millis, 1, PuzzleRunEnumStatus::Active)?,
            PuzzleCheckpointEnumTrigger::Countdown,
        )?;
    }
    assert_eq!(
        checkpoint.pending().map(|value| value.payload()),
        Some(&vec![1, 0])
    );
    assert_eq!(
        checkpoint.coalesced().map(|value| value.payload()),
        Some(&vec![1, 750])
    );
    assert_eq!(checkpoint.write_token(), Some(&token));
    assert!(
        checkpoint
            .begin_write(PuzzleCheckpointFixture::time(750))
            .is_err()
    );
    assert_eq!(
        checkpoint.before_advance(PuzzleCheckpointFixture::time(999))?,
        PuzzleCheckpointEnumAdvance::Proceed
    );
    let PuzzleCheckpointEnumAdvance::FreezeRequired(freeze) =
        checkpoint.before_advance(PuzzleCheckpointFixture::time(1000))?
    else {
        return Err("The exact durability boundary must freeze".into());
    };
    assert_eq!(freeze.cause(), PuzzleCheckpointEnumFreezeCause::DurableLag);
    assert_eq!(freeze.pause_at(), PuzzleCheckpointFixture::time(1000));
    assert!(
        checkpoint
            .durable()
            .is_some_and(|value| value.elapsed_active() == ClockDuration::ZERO)
    );
    Ok(())
}

#[test]
fn late_success_observes_the_old_durable_boundary_before_advancing_it()
-> Result<(), Box<dyn std::error::Error>> {
    let mut checkpoint = PuzzleCheckpointFixture::active()?;
    let token = checkpoint.begin_write(PuzzleCheckpointFixture::time(0))?;
    let result = checkpoint.complete(
        PuzzleCheckpointFixture::time(9000),
        &token,
        PuzzleCheckpointEnumCompletion::Committed,
    )?;
    let PuzzleCheckpointEnumAdvance::FreezeRequired(freeze) = result else {
        return Err("A late success cannot erase the missed freeze boundary".into());
    };
    assert_eq!(freeze.pause_at(), PuzzleCheckpointFixture::time(1000));
    assert!(matches!(
        checkpoint.status(),
        PuzzleCheckpointEnumStatus::AwaitingResume(_)
    ));
    assert!(
        checkpoint
            .resume(PuzzleCheckpointFixture::time(9000))
            .is_err()
    );
    Ok(())
}

#[test]
fn failure_and_cancel_retry_the_exact_snapshot_under_a_fresh_write_identity()
-> Result<(), Box<dyn std::error::Error>> {
    for failure in [
        PuzzleCheckpointEnumFailure::WriteFailed,
        PuzzleCheckpointEnumFailure::Cancelled,
    ] {
        let mut checkpoint = PuzzleCheckpointFixture::active()?;
        let original = checkpoint.begin_write(PuzzleCheckpointFixture::time(0))?;
        checkpoint.record(
            PuzzleCheckpointFixture::snapshot(250, 250, 2, PuzzleRunEnumStatus::Active)?,
            PuzzleCheckpointEnumTrigger::AcceptedTransition,
        )?;
        let pending_address = checkpoint.pending().map(|value| value.payload().as_ptr());
        checkpoint.complete(
            PuzzleCheckpointFixture::time(300),
            &original,
            PuzzleCheckpointEnumCompletion::Failed(failure),
        )?;
        assert!(
            matches!(checkpoint.status(), PuzzleCheckpointEnumStatus::RetryRequired { failure: actual, .. } if actual == failure)
        );
        assert_eq!(
            checkpoint.durable().map(|value| value.context().revision()),
            Some(PuzzleSessionRevision::ZERO)
        );
        assert!(
            checkpoint
                .begin_write(PuzzleCheckpointFixture::time(300))
                .is_err()
        );
        let retry = checkpoint.retry_save(PuzzleCheckpointFixture::time(300))?;
        assert_ne!(retry, original);
        assert_eq!(retry.context(), original.context());
        assert_eq!(
            checkpoint.pending().map(|value| value.payload().as_ptr()),
            pending_address
        );
        assert!(
            checkpoint
                .complete(
                    PuzzleCheckpointFixture::time(301),
                    &original,
                    PuzzleCheckpointEnumCompletion::Committed
                )
                .is_err()
        );
        checkpoint.complete(
            PuzzleCheckpointFixture::time(301),
            &retry,
            PuzzleCheckpointEnumCompletion::Committed,
        )?;
        assert!(matches!(
            checkpoint.status(),
            PuzzleCheckpointEnumStatus::Frozen(_)
        ));
        assert_eq!(
            checkpoint.pending().map(|value| value.payload()),
            Some(&vec![2, 250])
        );
    }
    Ok(())
}

#[test]
fn old_out_of_order_duplicate_and_foreign_completions_preserve_the_current_request()
-> Result<(), Box<dyn std::error::Error>> {
    let mut first = PuzzleCheckpointFixture::active()?;
    let first_token = first.begin_write(PuzzleCheckpointFixture::time(0))?;
    let mut other = PuzzleCheckpoint::new(PuzzleCheckpointSnapshot::new(
        PuzzleCheckpointObservation::new(
            PuzzleSessionContext::new(
                PuzzleSessionId::new(NonZeroU64::MAX),
                PuzzleAttemptId::new(NonZeroU64::MIN),
                PuzzleDefinitionIdentity::new("foreign".into(), "v1".into(), "a".into())?,
                PuzzleSessionRevision::ZERO,
            ),
            PuzzleCheckpointFixture::time(0),
            ClockDuration::ZERO,
            PuzzleRunEnumStatus::AwaitingReadiness,
        ),
        vec![99],
    ))?;
    let foreign = other.begin_write(PuzzleCheckpointFixture::time(0))?;
    assert!(
        first
            .complete(
                PuzzleCheckpointFixture::time(0),
                &foreign,
                PuzzleCheckpointEnumCompletion::Committed
            )
            .is_err()
    );
    first.record(
        PuzzleCheckpointFixture::snapshot(250, 250, 2, PuzzleRunEnumStatus::Active)?,
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    first.complete(
        PuzzleCheckpointFixture::time(250),
        &first_token,
        PuzzleCheckpointEnumCompletion::Committed,
    )?;
    assert!(
        first
            .complete(
                PuzzleCheckpointFixture::time(250),
                &first_token,
                PuzzleCheckpointEnumCompletion::Committed
            )
            .is_err()
    );
    let current = first.begin_write(PuzzleCheckpointFixture::time(250))?;
    assert!(
        first
            .complete(
                PuzzleCheckpointFixture::time(250),
                &first_token,
                PuzzleCheckpointEnumCompletion::Failed(PuzzleCheckpointEnumFailure::WriteFailed)
            )
            .is_err()
    );
    assert_eq!(first.write_token(), Some(&current));
    assert_eq!(current.context().revision(), PuzzleSessionRevision::new(2));
    assert_eq!(
        first.pending().map(|value| value.payload()),
        Some(&vec![2, 250])
    );
    Ok(())
}

#[test]
fn readiness_focus_and_success_never_release_the_explicit_resume_gate()
-> Result<(), Box<dyn std::error::Error>> {
    let mut checkpoint = PuzzleCheckpointFixture::active()?;
    let original = checkpoint.begin_write(PuzzleCheckpointFixture::time(0))?;
    checkpoint.complete(
        PuzzleCheckpointFixture::time(100),
        &original,
        PuzzleCheckpointEnumCompletion::Failed(PuzzleCheckpointEnumFailure::WriteFailed),
    )?;
    checkpoint.record(
        PuzzleCheckpointFixture::snapshot(100, 100, 2, PuzzleCheckpointFixture::paused())?,
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    let retry = checkpoint.retry_save(PuzzleCheckpointFixture::time(100))?;
    checkpoint.complete(
        PuzzleCheckpointFixture::time(100),
        &retry,
        PuzzleCheckpointEnumCompletion::Committed,
    )?;
    let pause = checkpoint.begin_write(PuzzleCheckpointFixture::time(100))?;
    checkpoint.complete(
        PuzzleCheckpointFixture::time(100),
        &pause,
        PuzzleCheckpointEnumCompletion::Committed,
    )?;
    assert!(matches!(
        checkpoint.status(),
        PuzzleCheckpointEnumStatus::AwaitingResume(_)
    ));
    assert!(matches!(
        checkpoint.before_advance(PuzzleCheckpointFixture::time(10000))?,
        PuzzleCheckpointEnumAdvance::FreezeRequired(_)
    ));
    assert!(!checkpoint.needs_snapshot(
        &PuzzleCheckpointFixture::observation(10000, 100, 2, PuzzleCheckpointFixture::paused())?,
        PuzzleCheckpointEnumTrigger::Countdown
    )?);
    checkpoint.resume(PuzzleCheckpointFixture::time(10000))?;
    assert_eq!(checkpoint.status(), PuzzleCheckpointEnumStatus::Open);
    checkpoint.record(
        PuzzleCheckpointFixture::snapshot(10000, 100, 3, PuzzleRunEnumStatus::Active)?,
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    assert_eq!(
        checkpoint.before_advance(PuzzleCheckpointFixture::time(10999))?,
        PuzzleCheckpointEnumAdvance::Proceed
    );
    assert!(matches!(
        checkpoint.before_advance(PuzzleCheckpointFixture::time(11000))?,
        PuzzleCheckpointEnumAdvance::FreezeRequired(_)
    ));
    Ok(())
}

#[test]
fn inactive_wall_time_does_not_schedule_checkpoints_or_consume_the_durable_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let mut checkpoint = PuzzleCheckpointFixture::active()?;
    checkpoint.record(
        PuzzleCheckpointFixture::snapshot(100, 100, 2, PuzzleRunEnumStatus::Review)?,
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    assert!(!checkpoint.needs_snapshot(
        &PuzzleCheckpointFixture::observation(100000, 100, 2, PuzzleRunEnumStatus::Review)?,
        PuzzleCheckpointEnumTrigger::Countdown
    )?);
    assert_eq!(
        checkpoint.before_advance(PuzzleCheckpointFixture::time(100000))?,
        PuzzleCheckpointEnumAdvance::Proceed
    );
    Ok(())
}

#[test]
fn backwards_time_elapsed_and_unrecorded_identity_changes_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let mut checkpoint = PuzzleCheckpointFixture::active()?;
    checkpoint.record(
        PuzzleCheckpointFixture::snapshot(250, 250, 1, PuzzleRunEnumStatus::Active)?,
        PuzzleCheckpointEnumTrigger::Countdown,
    )?;
    for observation in [
        PuzzleCheckpointFixture::observation(249, 249, 1, PuzzleRunEnumStatus::Active)?,
        PuzzleCheckpointFixture::observation(500, 249, 1, PuzzleRunEnumStatus::Active)?,
        PuzzleCheckpointFixture::observation(500, 500, 0, PuzzleRunEnumStatus::Active)?,
        PuzzleCheckpointFixture::observation(500, 500, 2, PuzzleRunEnumStatus::Active)?,
    ] {
        assert!(
            checkpoint
                .needs_snapshot(&observation, PuzzleCheckpointEnumTrigger::Countdown)
                .is_err()
        );
    }
    assert_eq!(
        checkpoint.coalesced().map(|value| value.payload()),
        Some(&vec![1, 250])
    );
    Ok(())
}

#[test]
fn real_run_freezes_at_the_bound_and_keeps_hidden_content_after_retry_success()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = PuzzleCheckpointRunFixture::new(PuzzleEnumCountdownMode::Session)?;
    let active = fixture
        .checkpoint
        .begin_write(PuzzleCheckpointFixture::time(0))?;
    let late = PuzzleCheckpointFixture::time(9000);
    let PuzzleCheckpointEnumAdvance::FreezeRequired(freeze) =
        fixture.checkpoint.before_advance(late)?
    else {
        return Err("The stalled real run must freeze before nine seconds".into());
    };
    fixture.run.pause(
        &fixture.run.context(),
        freeze.pause_at(),
        PuzzleTimerEnumPauseReason::Manual,
    )?;
    assert_eq!(
        fixture.run.view(late)?.timer().elapsed(),
        ClockDuration::from_millis(1000)?
    );
    assert_eq!(
        fixture.run.view(late)?.content(),
        PuzzleRunEnumContent::Hidden
    );
    fixture.capture(late)?;
    fixture.checkpoint.complete(
        late,
        &active,
        PuzzleCheckpointEnumCompletion::Failed(PuzzleCheckpointEnumFailure::WriteFailed),
    )?;
    let retry = fixture.checkpoint.retry_save(late)?;
    fixture
        .checkpoint
        .complete(late, &retry, PuzzleCheckpointEnumCompletion::Committed)?;
    let paused = fixture.checkpoint.begin_write(late)?;
    fixture
        .checkpoint
        .complete(late, &paused, PuzzleCheckpointEnumCompletion::Committed)?;
    fixture.run.update_readiness(
        &fixture.run.context(),
        late,
        PuzzleTimerEnumReadiness::Ready,
    )?;
    assert_eq!(
        fixture.run.view(late)?.content(),
        PuzzleRunEnumContent::Hidden
    );
    fixture.checkpoint.resume(late)?;
    fixture.run.resume(
        &fixture.run.context(),
        late,
        PuzzleTimerEnumReadiness::Ready,
    )?;
    assert_eq!(
        fixture.run.view(late)?.status(),
        PuzzleRunEnumStatus::Active
    );
    Ok(())
}

#[test]
fn deadline_overflow_is_typed_and_keeps_the_immutable_pending_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let near_end = ClockInstant::from_nanos(u64::MAX - 1);
    let initial =
        PuzzleCheckpointFixture::observation(0, 0, 0, PuzzleRunEnumStatus::AwaitingReadiness)?;
    let context = initial.context().clone();
    let mut checkpoint = PuzzleCheckpoint::new(PuzzleCheckpointSnapshot::new(
        PuzzleCheckpointObservation::new(
            context,
            near_end,
            ClockDuration::ZERO,
            PuzzleRunEnumStatus::AwaitingReadiness,
        ),
        vec![0],
    ))?;
    let token = checkpoint.begin_write(near_end)?;
    checkpoint.complete(near_end, &token, PuzzleCheckpointEnumCompletion::Committed)?;
    let active = PuzzleCheckpointFixture::observation(0, 0, 1, PuzzleRunEnumStatus::Active)?;
    checkpoint.record(
        PuzzleCheckpointSnapshot::new(
            PuzzleCheckpointObservation::new(
                active.context().clone(),
                near_end,
                ClockDuration::ZERO,
                PuzzleRunEnumStatus::Active,
            ),
            vec![1],
        ),
        PuzzleCheckpointEnumTrigger::AcceptedTransition,
    )?;
    assert!(matches!(
        checkpoint.before_advance(near_end),
        Err(PuzzleCheckpointError::Duration(
            ClockDurationError::Overflow
        ))
    ));
    assert_eq!(
        checkpoint.pending().map(|value| value.payload()),
        Some(&vec![1])
    );
    Ok(())
}

#[test]
fn real_focus_sleep_and_puzzle_window_hiding_preserve_time_until_explicit_resume()
-> Result<(), Box<dyn std::error::Error>> {
    for reason in [
        PuzzleTimerEnumPauseReason::FocusLost,
        PuzzleTimerEnumPauseReason::Sleep,
        PuzzleTimerEnumPauseReason::WindowHidden,
    ] {
        let mut fixture = PuzzleCheckpointRunFixture::new(PuzzleEnumCountdownMode::PerPuzzle)?;
        fixture.commit(PuzzleCheckpointFixture::time(0))?;
        let interrupted = PuzzleCheckpointFixture::time(100);
        assert_eq!(
            fixture.checkpoint.before_advance(interrupted)?,
            PuzzleCheckpointEnumAdvance::Proceed
        );
        if reason == PuzzleTimerEnumPauseReason::WindowHidden {
            fixture.run.update_readiness(
                &fixture.run.context(),
                interrupted,
                PuzzleTimerEnumReadiness::Hidden,
            )?;
        } else {
            fixture
                .run
                .pause(&fixture.run.context(), interrupted, reason)?;
        }
        fixture.capture(interrupted)?;
        fixture.commit(interrupted)?;
        let refocused = PuzzleCheckpointFixture::time(10000);
        fixture.run.update_readiness(
            &fixture.run.context(),
            refocused,
            PuzzleTimerEnumReadiness::Ready,
        )?;
        assert_eq!(
            fixture.run.view(refocused)?.content(),
            PuzzleRunEnumContent::Hidden
        );
        assert_eq!(
            fixture.run.view(refocused)?.timer().elapsed(),
            ClockDuration::from_millis(100)?
        );
        assert!(!fixture.checkpoint.needs_snapshot(
            &PuzzleCheckpointObservation::from_run(&fixture.run, refocused)?,
            PuzzleCheckpointEnumTrigger::Countdown
        )?);
        fixture.run.resume(
            &fixture.run.context(),
            refocused,
            PuzzleTimerEnumReadiness::Ready,
        )?;
        assert_eq!(
            fixture.run.view(refocused)?.status(),
            PuzzleRunEnumStatus::Active
        );
    }
    Ok(())
}

#[test]
fn initial_state_is_not_durable_until_its_observed_commit() -> Result<(), Box<dyn std::error::Error>>
{
    assert!(matches!(
        PuzzleCheckpoint::new(PuzzleCheckpointFixture::snapshot(
            0,
            0,
            1,
            PuzzleRunEnumStatus::Active
        )?),
        Err(PuzzleCheckpointError::ActiveInitialState)
    ));
    let mut checkpoint = PuzzleCheckpoint::new(PuzzleCheckpointFixture::snapshot(
        0,
        0,
        0,
        PuzzleRunEnumStatus::AwaitingReadiness,
    )?)?;
    assert!(checkpoint.durable().is_none());
    let PuzzleCheckpointEnumAdvance::FreezeRequired(freeze) =
        checkpoint.before_advance(PuzzleCheckpointFixture::time(0))?
    else {
        return Err("Unsaved initial state must block advancement".into());
    };
    assert_eq!(
        freeze.cause(),
        PuzzleCheckpointEnumFreezeCause::InitialSaveRequired
    );
    let token = checkpoint.begin_write(PuzzleCheckpointFixture::time(0))?;
    assert!(checkpoint.durable().is_none());
    checkpoint.complete(
        PuzzleCheckpointFixture::time(0),
        &token,
        PuzzleCheckpointEnumCompletion::Committed,
    )?;
    assert!(checkpoint.durable().is_some());
    assert!(matches!(
        checkpoint.status(),
        PuzzleCheckpointEnumStatus::AwaitingResume(_)
    ));
    assert!(matches!(
        checkpoint.before_advance(PuzzleCheckpointFixture::time(0))?,
        PuzzleCheckpointEnumAdvance::FreezeRequired(_)
    ));
    Ok(())
}

#[test]
fn over_bound_counters_and_active_records_during_freeze_cannot_replace_pending_memory()
-> Result<(), Box<dyn std::error::Error>> {
    let mut checkpoint = PuzzleCheckpointFixture::active()?;
    assert!(matches!(
        checkpoint.record(
            PuzzleCheckpointFixture::snapshot(1001, 1001, 2, PuzzleRunEnumStatus::Active)?,
            PuzzleCheckpointEnumTrigger::AcceptedTransition
        ),
        Err(PuzzleCheckpointError::DurableLagExceeded)
    ));
    assert!(matches!(
        checkpoint.before_advance(PuzzleCheckpointFixture::time(1000))?,
        PuzzleCheckpointEnumAdvance::FreezeRequired(_)
    ));
    assert!(matches!(
        checkpoint.record(
            PuzzleCheckpointFixture::snapshot(1000, 1000, 2, PuzzleRunEnumStatus::Active)?,
            PuzzleCheckpointEnumTrigger::AcceptedTransition
        ),
        Err(PuzzleCheckpointError::PauseRequired)
    ));
    assert_eq!(
        checkpoint.pending().map(|value| value.payload()),
        Some(&vec![1, 0])
    );
    assert!(checkpoint.coalesced().is_none());
    Ok(())
}

#[test]
fn only_committed_actual_elapsed_rebases_the_next_durability_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let mut checkpoint = PuzzleCheckpointFixture::active()?;
    let first = checkpoint.begin_write(PuzzleCheckpointFixture::time(0))?;
    checkpoint.record(
        PuzzleCheckpointFixture::snapshot(250, 250, 1, PuzzleRunEnumStatus::Active)?,
        PuzzleCheckpointEnumTrigger::Countdown,
    )?;
    checkpoint.complete(
        PuzzleCheckpointFixture::time(250),
        &first,
        PuzzleCheckpointEnumCompletion::Committed,
    )?;
    let second = checkpoint.begin_write(PuzzleCheckpointFixture::time(250))?;
    checkpoint.record(
        PuzzleCheckpointFixture::snapshot(500, 500, 1, PuzzleRunEnumStatus::Active)?,
        PuzzleCheckpointEnumTrigger::Countdown,
    )?;
    checkpoint.complete(
        PuzzleCheckpointFixture::time(600),
        &second,
        PuzzleCheckpointEnumCompletion::Committed,
    )?;
    assert_eq!(
        checkpoint.durable().map(|value| value.elapsed_active()),
        Some(ClockDuration::from_millis(250)?)
    );
    assert_eq!(
        checkpoint.before_advance(PuzzleCheckpointFixture::time(1249))?,
        PuzzleCheckpointEnumAdvance::Proceed
    );
    let PuzzleCheckpointEnumAdvance::FreezeRequired(freeze) =
        checkpoint.before_advance(PuzzleCheckpointFixture::time(1250))?
    else {
        return Err("The durable elapsed counter must set the boundary".into());
    };
    assert_eq!(freeze.pause_at(), PuzzleCheckpointFixture::time(1250));
    Ok(())
}

#[test]
fn scheduling_deadlines_share_cadence_and_the_committed_lag_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let mut checkpoint = PuzzleCheckpointFixture::active()?;
    assert_eq!(
        checkpoint.next_deadline()?,
        Some(PuzzleCheckpointFixture::time(250))
    );
    for millis in [250, 500, 750] {
        checkpoint.record(
            PuzzleCheckpointFixture::snapshot(millis, millis, 1, PuzzleRunEnumStatus::Active)?,
            PuzzleCheckpointEnumTrigger::Countdown,
        )?;
        if millis == 750 {
            checkpoint.record(
                PuzzleCheckpointFixture::snapshot(760, 760, 2, PuzzleRunEnumStatus::Active)?,
                PuzzleCheckpointEnumTrigger::AcceptedTransition,
            )?;
            assert_eq!(
                checkpoint.next_deadline()?,
                Some(PuzzleCheckpointFixture::time(1000))
            );
        }
    }
    checkpoint.before_advance(PuzzleCheckpointFixture::time(1000))?;
    assert_eq!(checkpoint.next_deadline()?, None);
    Ok(())
}

#[test]
fn fresh_initial_release_requires_the_latest_inactive_commit_and_never_opens_a_pause()
-> Result<(), Box<dyn std::error::Error>> {
    let zero = PuzzleCheckpointFixture::time(0);
    let mut checkpoint = PuzzleCheckpoint::new(PuzzleCheckpointFixture::snapshot(
        0,
        0,
        0,
        PuzzleRunEnumStatus::AwaitingReadiness,
    )?)?;
    checkpoint.before_advance(zero)?;
    assert!(checkpoint.release_initial(zero).is_err());
    let token = checkpoint.begin_write(zero)?;
    checkpoint.complete(zero, &token, PuzzleCheckpointEnumCompletion::Committed)?;
    checkpoint.release_initial(zero)?;
    assert_eq!(checkpoint.status(), PuzzleCheckpointEnumStatus::Open);
    assert_eq!(checkpoint.next_deadline()?, None);
    let mut paused = PuzzleCheckpoint::new(PuzzleCheckpointFixture::snapshot(
        0,
        0,
        0,
        PuzzleCheckpointFixture::paused(),
    )?)?;
    paused.before_advance(zero)?;
    let token = paused.begin_write(zero)?;
    paused.complete(zero, &token, PuzzleCheckpointEnumCompletion::Committed)?;
    assert!(paused.release_initial(zero).is_err());
    paused.resume(zero)?;
    Ok(())
}
