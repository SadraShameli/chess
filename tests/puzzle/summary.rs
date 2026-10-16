use std::num::NonZeroU64;
use std::time::Duration;

use chess::board::position::BoardPosition;
use chess::puzzle::attempt::PuzzleAttemptId;
use chess::puzzle::definition::PuzzleDefinition;
use chess::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumPrimaryOutcome,
};
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::puzzle::metadata::PuzzleDefinitionMetadata;
use chess::puzzle::session::{PuzzleSession, PuzzleSessionConfig, PuzzleSessionId};
use chess::puzzle::summary::{PuzzleSummary, PuzzleSummaryAttempt, PuzzleSummaryError};

struct PuzzleSummaryFixture;

impl PuzzleSummaryFixture {
    fn session(session: u64, attempt: u64) -> Result<PuzzleSession, Box<dyn std::error::Error>> {
        Self::versioned_session(session, attempt, "v1", PuzzleEnumCountdownMode::Session)
    }

    fn versioned_session(
        session: u64,
        attempt: u64,
        version: &str,
        mode: PuzzleEnumCountdownMode,
    ) -> Result<PuzzleSession, Box<dyn std::error::Error>> {
        let initial = BoardPosition::standard();
        let movement = initial.parse_uci("e2e4")?;
        let definition = PuzzleDefinition::new(
            PuzzleDefinitionIdentity::new("fixture".into(), version.into(), "tactic".into())?,
            initial,
            vec![vec![movement]],
            PuzzleDefinitionMetadata::default(),
        )?;
        Ok(PuzzleSession::new(
            PuzzleSessionId::new(NonZeroU64::new(session).ok_or("Zero session identity")?),
            PuzzleAttemptId::new(NonZeroU64::new(attempt).ok_or("Zero attempt identity")?),
            definition,
            PuzzleSessionConfig::defaults(mode),
        ))
    }

    fn movement(session: &mut PuzzleSession, uci: &str) -> Result<(), Box<dyn std::error::Error>> {
        let movement = session.attempt().position().parse_uci(uci)?;
        session.submit_move(&session.context(), movement)?;
        Ok(())
    }

    fn observation(session: &PuzzleSession, elapsed_active: Duration) -> PuzzleSummaryAttempt<'_> {
        PuzzleSummaryAttempt::new(session.id(), session.attempt(), elapsed_active)
    }

    fn summary(
        session: &PuzzleSession,
        elapsed_active: Duration,
    ) -> Result<PuzzleSummary, PuzzleSummaryError> {
        PuzzleSummary::from_attempts([Self::observation(session, elapsed_active)])
    }
}

#[test]
fn primary_outcomes_partition_all_finalized_attempts_and_both_solve_denominators()
-> Result<(), Box<dyn std::error::Error>> {
    let mut unassisted = PuzzleSummaryFixture::session(1, 1)?;
    PuzzleSummaryFixture::movement(&mut unassisted, "e2e4")?;
    let mut assisted = PuzzleSummaryFixture::session(1, 2)?;
    assisted.request_hint(&assisted.context())?;
    PuzzleSummaryFixture::movement(&mut assisted, "e2e4")?;
    let mut failed = PuzzleSummaryFixture::session(1, 3)?;
    PuzzleSummaryFixture::movement(&mut failed, "d2d4")?;
    let mut timed_out = PuzzleSummaryFixture::session(1, 4)?;
    timed_out.record_timeout(&timed_out.context())?;
    let mut skipped = PuzzleSummaryFixture::session(1, 5)?;
    skipped.skip_attempt(&skipped.context())?;
    let active = PuzzleSummaryFixture::session(1, 6)?;
    let summary = PuzzleSummary::from_attempts(
        [
            &unassisted,
            &assisted,
            &failed,
            &timed_out,
            &skipped,
            &active,
        ]
        .into_iter()
        .map(|session| PuzzleSummaryFixture::observation(session, Duration::from_secs(10))),
    )?;
    assert_eq!(summary.finalized(), 5);
    for outcome in [
        PuzzleEnumPrimaryOutcome::Solved(PuzzleEnumAssistance::Unassisted),
        PuzzleEnumPrimaryOutcome::Solved(PuzzleEnumAssistance::Assisted),
        PuzzleEnumPrimaryOutcome::Failed,
        PuzzleEnumPrimaryOutcome::TimedOut,
        PuzzleEnumPrimaryOutcome::Skipped,
    ] {
        assert_eq!(summary.primary_count(outcome), 1);
    }
    for assistance in [
        PuzzleEnumAssistance::Unassisted,
        PuzzleEnumAssistance::Assisted,
    ] {
        let rate = summary
            .solve_rate(assistance)
            .ok_or("Unavailable solve rate")?;
        assert_eq!(rate.numerator(), 1);
        assert_eq!(rate.denominator().get(), 5);
    }
    assert_eq!(summary.elapsed_active(), Duration::from_secs(60));
    Ok(())
}

#[test]
fn failed_then_practice_solved_retains_failure_and_separate_retry_hint_assistance_counts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSummaryFixture::session(1, 1)?;
    PuzzleSummaryFixture::movement(&mut session, "d2d4")?;
    session.retry_practice(&session.context())?;
    session.request_hint(&session.context())?;
    PuzzleSummaryFixture::movement(&mut session, "d2d4")?;
    session.retry_practice(&session.context())?;
    session.request_hint(&session.context())?;
    session.request_hint(&session.context())?;
    PuzzleSummaryFixture::movement(&mut session, "e2e4")?;
    let summary = PuzzleSummaryFixture::summary(&session, Duration::from_secs(47))?;
    assert_eq!(summary.finalized(), 1);
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Failed), 1);
    for assistance in [
        PuzzleEnumAssistance::Unassisted,
        PuzzleEnumAssistance::Assisted,
    ] {
        assert_eq!(
            summary.primary_count(PuzzleEnumPrimaryOutcome::Solved(assistance)),
            0
        );
        let rate = summary
            .solve_rate(assistance)
            .ok_or("Unavailable solve rate")?;
        assert_eq!(rate.numerator(), 0);
        assert_eq!(rate.denominator().get(), 1);
    }
    assert_eq!(summary.practice_completions(), 1);
    assert_eq!(summary.practice_retries(), 2);
    assert_eq!(summary.hints(), 3);
    assert_eq!(summary.assisted_attempts(), 1);
    assert_eq!(summary.elapsed_active(), Duration::from_secs(47));
    Ok(())
}

#[test]
fn failure_followed_by_skip_or_either_timeout_keeps_secondary_events_out_of_primary_counts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut skipped = PuzzleSummaryFixture::session(1, 1)?;
    PuzzleSummaryFixture::movement(&mut skipped, "d2d4")?;
    skipped.skip_attempt(&skipped.context())?;
    let mut session_timeout = PuzzleSummaryFixture::session(1, 2)?;
    PuzzleSummaryFixture::movement(&mut session_timeout, "d2d4")?;
    session_timeout.record_timeout(&session_timeout.context())?;
    let mut puzzle_timeout =
        PuzzleSummaryFixture::versioned_session(1, 3, "v1", PuzzleEnumCountdownMode::PerPuzzle)?;
    PuzzleSummaryFixture::movement(&mut puzzle_timeout, "d2d4")?;
    puzzle_timeout.record_timeout(&puzzle_timeout.context())?;
    let summary = PuzzleSummary::from_attempts(
        [&skipped, &session_timeout, &puzzle_timeout]
            .into_iter()
            .map(|session| PuzzleSummaryFixture::observation(session, Duration::from_secs(9))),
    )?;
    assert_eq!(summary.finalized(), 3);
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Failed), 3);
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Skipped), 0);
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::TimedOut), 0);
    assert_eq!(summary.skip_events(), 1);
    assert_eq!(summary.timeout_events(PuzzleEnumCountdownMode::Session), 1);
    assert_eq!(
        summary.timeout_events(PuzzleEnumCountdownMode::PerPuzzle),
        1
    );
    Ok(())
}

#[test]
fn empty_and_unfinished_attempts_have_unavailable_rates_without_losing_active_time_or_hints()
-> Result<(), Box<dyn std::error::Error>> {
    let empty = PuzzleSummary::from_attempts(std::iter::empty())?;
    let mut session = PuzzleSummaryFixture::session(1, 1)?;
    session.request_hint(&session.context())?;
    let active = PuzzleSummaryFixture::summary(&session, Duration::from_nanos(17))?;
    for summary in [&empty, &active] {
        assert_eq!(summary.finalized(), 0);
        assert_eq!(summary.solve_rate(PuzzleEnumAssistance::Unassisted), None);
        assert_eq!(summary.solve_rate(PuzzleEnumAssistance::Assisted), None);
    }
    assert_eq!(empty.elapsed_active(), Duration::ZERO);
    assert_eq!(active.elapsed_active(), Duration::from_nanos(17));
    assert_eq!(active.hints(), 1);
    assert_eq!(active.assisted_attempts(), 1);
    Ok(())
}

#[test]
fn recomputation_uses_current_attempt_state_without_recounting_the_previous_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSummaryFixture::session(1, 1)?;
    let initial = PuzzleSummaryFixture::summary(&session, Duration::from_secs(3))?;
    PuzzleSummaryFixture::movement(&mut session, "d2d4")?;
    let failed = PuzzleSummaryFixture::summary(&session, Duration::from_secs(5))?;
    session.retry_practice(&session.context())?;
    PuzzleSummaryFixture::movement(&mut session, "e2e4")?;
    let practiced = PuzzleSummaryFixture::summary(&session, Duration::from_secs(8))?;
    assert_eq!(initial.finalized(), 0);
    assert_eq!(failed.finalized(), 1);
    assert_eq!(practiced.finalized(), 1);
    assert_eq!(practiced.primary_count(PuzzleEnumPrimaryOutcome::Failed), 1);
    assert_eq!(failed.practice_completions(), 0);
    assert_eq!(practiced.practice_completions(), 1);
    assert_eq!(practiced.elapsed_active(), Duration::from_secs(8));
    Ok(())
}

#[test]
fn aggregation_is_order_independent_and_recomputes_from_a_borrowed_subset()
-> Result<(), Box<dyn std::error::Error>> {
    let mut solved = PuzzleSummaryFixture::session(1, 1)?;
    PuzzleSummaryFixture::movement(&mut solved, "e2e4")?;
    let mut skipped = PuzzleSummaryFixture::session(1, 2)?;
    skipped.skip_attempt(&skipped.context())?;
    let mut assisted = PuzzleSummaryFixture::session(2, 1)?;
    assisted.request_hint(&assisted.context())?;
    PuzzleSummaryFixture::movement(&mut assisted, "e2e4")?;
    let records = [
        (&solved, Duration::from_secs(5)),
        (&skipped, Duration::from_secs(7)),
        (&assisted, Duration::from_secs(11)),
    ];
    let full = PuzzleSummary::from_attempts(
        records
            .iter()
            .map(|(session, duration)| PuzzleSummaryFixture::observation(session, *duration)),
    )?;
    let reversed = PuzzleSummary::from_attempts(
        records
            .iter()
            .rev()
            .map(|(session, duration)| PuzzleSummaryFixture::observation(session, *duration)),
    )?;
    assert_eq!(full, reversed);
    assert_eq!(full.finalized(), 3);
    assert_eq!(full.elapsed_active(), Duration::from_secs(23));
    let retained = PuzzleSummary::from_attempts(
        records
            .iter()
            .filter(|(session, _)| session.id() == solved.id())
            .map(|(session, duration)| PuzzleSummaryFixture::observation(session, *duration)),
    )?;
    assert_eq!(retained.finalized(), 2);
    assert_eq!(retained.elapsed_active(), Duration::from_secs(12));
    let rate = retained
        .solve_rate(PuzzleEnumAssistance::Unassisted)
        .ok_or("Unavailable solve rate")?;
    assert_eq!(rate.numerator(), 1);
    assert_eq!(rate.denominator().get(), 2);
    assert_eq!(retained.assisted_attempts(), 0);
    Ok(())
}

#[test]
fn repeated_source_puzzles_are_distinct_across_attempt_and_session_identities()
-> Result<(), Box<dyn std::error::Error>> {
    let mut first = PuzzleSummaryFixture::session(1, 1)?;
    let mut next_attempt = PuzzleSummaryFixture::session(1, 2)?;
    let mut next_session = PuzzleSummaryFixture::session(2, 1)?;
    for session in [&mut first, &mut next_attempt, &mut next_session] {
        PuzzleSummaryFixture::movement(session, "e2e4")?;
    }
    let summary = PuzzleSummary::from_attempts(
        [&first, &next_attempt, &next_session]
            .into_iter()
            .map(|session| PuzzleSummaryFixture::observation(session, Duration::ZERO)),
    )?;
    assert_eq!(summary.finalized(), 3);
    assert_eq!(
        summary.primary_count(PuzzleEnumPrimaryOutcome::Solved(
            PuzzleEnumAssistance::Unassisted
        )),
        3
    );
    Ok(())
}

#[test]
fn duplicate_identity_is_rejected_even_with_different_elapsed_time()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSummaryFixture::session(1, 1)?;
    PuzzleSummaryFixture::movement(&mut session, "e2e4")?;
    assert!(matches!(
        PuzzleSummary::from_attempts([
            PuzzleSummaryFixture::observation(&session, Duration::from_secs(1)),
            PuzzleSummaryFixture::observation(&session, Duration::from_secs(2)),
        ]),
        Err(PuzzleSummaryError::DuplicateAttempt { session: id, attempt })
            if id == session.id() && attempt == session.attempt().id()
    ));
    Ok(())
}

#[test]
fn unfinished_duplicate_identity_cannot_double_count_time_or_hints()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = PuzzleSummaryFixture::session(1, 1)?;
    session.request_hint(&session.context())?;
    assert!(matches!(
        PuzzleSummary::from_attempts([
            PuzzleSummaryFixture::observation(&session, Duration::from_secs(1)),
            PuzzleSummaryFixture::observation(&session, Duration::from_secs(1)),
        ]),
        Err(PuzzleSummaryError::DuplicateAttempt { .. })
    ));
    Ok(())
}

#[test]
fn reusing_a_session_attempt_pair_for_another_source_version_is_a_conflict()
-> Result<(), Box<dyn std::error::Error>> {
    let first = PuzzleSummaryFixture::session(1, 1)?;
    let changed =
        PuzzleSummaryFixture::versioned_session(1, 1, "v2", PuzzleEnumCountdownMode::Session)?;
    assert!(matches!(
        PuzzleSummary::from_attempts([
            PuzzleSummaryFixture::observation(&first, Duration::ZERO),
            PuzzleSummaryFixture::observation(&changed, Duration::ZERO),
        ]),
        Err(PuzzleSummaryError::DefinitionConflict { session, attempt })
            if session == first.id() && attempt == first.attempt().id()
    ));
    Ok(())
}

#[test]
fn caller_supplied_active_durations_preserve_nanoseconds_above_float_precision()
-> Result<(), Box<dyn std::error::Error>> {
    let first = PuzzleSummaryFixture::session(1, 1)?;
    let second = PuzzleSummaryFixture::session(1, 2)?;
    let summary = PuzzleSummary::from_attempts([
        PuzzleSummaryFixture::observation(&first, Duration::from_nanos(9_007_199_254_740_993)),
        PuzzleSummaryFixture::observation(&second, Duration::from_nanos(1)),
    ])?;
    assert_eq!(
        summary.elapsed_active(),
        Duration::from_nanos(9_007_199_254_740_994)
    );
    Ok(())
}

#[test]
fn elapsed_totals_accept_the_full_duration_range_and_report_overflow()
-> Result<(), Box<dyn std::error::Error>> {
    let first = PuzzleSummaryFixture::session(1, 1)?;
    let second = PuzzleSummaryFixture::session(1, 2)?;
    let maximum = PuzzleSummaryFixture::summary(&first, Duration::MAX)?;
    assert_eq!(maximum.elapsed_active(), Duration::MAX);
    assert!(matches!(
        PuzzleSummary::from_attempts([
            PuzzleSummaryFixture::observation(&first, Duration::MAX),
            PuzzleSummaryFixture::observation(&second, Duration::from_nanos(1)),
        ]),
        Err(PuzzleSummaryError::ElapsedOverflow)
    ));
    Ok(())
}
