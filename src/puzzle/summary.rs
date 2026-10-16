use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::num::NonZeroU64;
use std::time::Duration;

use thiserror::Error;

use crate::puzzle::attempt::{PuzzleAttempt, PuzzleAttemptId};
use crate::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumEvent, PuzzleEnumPrimaryOutcome,
};
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::session::PuzzleSessionId;

#[derive(Debug, Error)]
pub enum PuzzleSummaryError {
    #[error("The puzzle attempt {attempt:?} in session {session:?} was supplied more than once")]
    DuplicateAttempt {
        session: PuzzleSessionId,
        attempt: PuzzleAttemptId,
    },
    #[error(
        "The puzzle attempt {attempt:?} in session {session:?} has conflicting source identities"
    )]
    DefinitionConflict {
        session: PuzzleSessionId,
        attempt: PuzzleAttemptId,
    },
    #[error("A puzzle summary count exceeds the unsigned 64-bit range")]
    CountOverflow,
    #[error("The total elapsed active puzzle time exceeds the duration range")]
    ElapsedOverflow,
}

#[derive(Debug, Clone, Copy)]
pub struct PuzzleSummaryAttempt<'a> {
    session: PuzzleSessionId,
    attempt: &'a PuzzleAttempt,
    elapsed_active: Duration,
}

impl<'a> PuzzleSummaryAttempt<'a> {
    pub const fn new(
        session: PuzzleSessionId,
        attempt: &'a PuzzleAttempt,
        elapsed_active: Duration,
    ) -> Self {
        Self {
            session,
            attempt,
            elapsed_active,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleSummaryRate {
    numerator: u64,
    denominator: NonZeroU64,
}

impl PuzzleSummaryRate {
    pub const fn numerator(self) -> u64 {
        self.numerator
    }

    pub const fn denominator(self) -> NonZeroU64 {
        self.denominator
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PuzzleSummary {
    primary_counts: [u64; Self::PRIMARY_OUTCOME_COUNT],
    finalized: u64,
    assisted_attempts: u64,
    practice_completions: u64,
    practice_retries: u64,
    hints: u64,
    skip_events: u64,
    timeout_events: [u64; Self::COUNTDOWN_MODE_COUNT],
    elapsed_active: Duration,
}

impl PuzzleSummary {
    const PRIMARY_OUTCOME_COUNT: usize = 5;
    const COUNTDOWN_MODE_COUNT: usize = 2;

    pub fn from_attempts<'a>(
        attempts: impl IntoIterator<Item = PuzzleSummaryAttempt<'a>>,
    ) -> Result<Self, PuzzleSummaryError> {
        let mut summary = Self::default();
        let mut identities: HashMap<(PuzzleSessionId, PuzzleAttemptId), &PuzzleDefinitionIdentity> =
            HashMap::new();
        for observation in attempts {
            let session = observation.session;
            let attempt = observation.attempt.id();
            let definition = observation.attempt.definition().identity();
            match identities.entry((session, attempt)) {
                Entry::Vacant(entry) => {
                    entry.insert(definition);
                }
                Entry::Occupied(entry) => {
                    return Err(if *entry.get() == definition {
                        PuzzleSummaryError::DuplicateAttempt { session, attempt }
                    } else {
                        PuzzleSummaryError::DefinitionConflict { session, attempt }
                    });
                }
            }
            summary.record_attempt(observation.attempt, observation.elapsed_active)?;
        }
        Ok(summary)
    }

    pub const fn finalized(&self) -> u64 {
        self.finalized
    }

    pub const fn primary_count(&self, outcome: PuzzleEnumPrimaryOutcome) -> u64 {
        self.primary_counts[Self::outcome_index(outcome)]
    }

    pub fn solve_rate(&self, assistance: PuzzleEnumAssistance) -> Option<PuzzleSummaryRate> {
        Some(PuzzleSummaryRate {
            numerator: self.primary_count(PuzzleEnumPrimaryOutcome::Solved(assistance)),
            denominator: NonZeroU64::new(self.finalized)?,
        })
    }

    pub const fn assisted_attempts(&self) -> u64 {
        self.assisted_attempts
    }

    pub const fn practice_completions(&self) -> u64 {
        self.practice_completions
    }

    pub const fn practice_retries(&self) -> u64 {
        self.practice_retries
    }

    pub const fn hints(&self) -> u64 {
        self.hints
    }

    pub const fn skip_events(&self) -> u64 {
        self.skip_events
    }

    pub const fn timeout_events(&self, mode: PuzzleEnumCountdownMode) -> u64 {
        self.timeout_events[Self::countdown_index(mode)]
    }

    pub const fn elapsed_active(&self) -> Duration {
        self.elapsed_active
    }

    fn record_attempt(
        &mut self,
        attempt: &PuzzleAttempt,
        elapsed_active: Duration,
    ) -> Result<(), PuzzleSummaryError> {
        if let Some(outcome) = attempt.primary_outcome() {
            Self::increment(&mut self.finalized)?;
            Self::increment(&mut self.primary_counts[Self::outcome_index(outcome)])?;
        }
        if attempt.assistance() == PuzzleEnumAssistance::Assisted {
            Self::increment(&mut self.assisted_attempts)?;
        }
        for event in attempt.events() {
            match event {
                PuzzleEnumEvent::Hint(_) => Self::increment(&mut self.hints)?,
                PuzzleEnumEvent::PracticeRetry => Self::increment(&mut self.practice_retries)?,
                PuzzleEnumEvent::PracticeCompleted => {
                    Self::increment(&mut self.practice_completions)?
                }
                PuzzleEnumEvent::Skipped => Self::increment(&mut self.skip_events)?,
                PuzzleEnumEvent::TimedOut(mode) => {
                    Self::increment(&mut self.timeout_events[Self::countdown_index(*mode)])?;
                }
                PuzzleEnumEvent::SolverMove { .. }
                | PuzzleEnumEvent::OpponentReply { .. }
                | PuzzleEnumEvent::LegalUnlisted(_) => {}
            }
        }
        self.elapsed_active = self
            .elapsed_active
            .checked_add(elapsed_active)
            .ok_or(PuzzleSummaryError::ElapsedOverflow)?;
        Ok(())
    }

    fn increment(count: &mut u64) -> Result<(), PuzzleSummaryError> {
        *count = count
            .checked_add(1)
            .ok_or(PuzzleSummaryError::CountOverflow)?;
        Ok(())
    }

    const fn outcome_index(outcome: PuzzleEnumPrimaryOutcome) -> usize {
        match outcome {
            PuzzleEnumPrimaryOutcome::Solved(PuzzleEnumAssistance::Unassisted) => 0,
            PuzzleEnumPrimaryOutcome::Solved(PuzzleEnumAssistance::Assisted) => 1,
            PuzzleEnumPrimaryOutcome::Failed => 2,
            PuzzleEnumPrimaryOutcome::TimedOut => 3,
            PuzzleEnumPrimaryOutcome::Skipped => 4,
        }
    }

    const fn countdown_index(mode: PuzzleEnumCountdownMode) -> usize {
        match mode {
            PuzzleEnumCountdownMode::Session => 0,
            PuzzleEnumCountdownMode::PerPuzzle => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::time::Duration;

    use crate::board::position::BoardPosition;
    use crate::puzzle::attempt::{PuzzleAttempt, PuzzleAttemptId};
    use crate::puzzle::definition::PuzzleDefinition;
    use crate::puzzle::enums::{PuzzleEnumAssistance, PuzzleEnumPrimaryOutcome};
    use crate::puzzle::identity::PuzzleDefinitionIdentity;
    use crate::puzzle::metadata::PuzzleDefinitionMetadata;
    use crate::puzzle::summary::{PuzzleSummary, PuzzleSummaryError};

    struct PuzzleSummaryBoundaryFixture;

    impl PuzzleSummaryBoundaryFixture {
        fn skipped_attempt() -> Result<PuzzleAttempt, Box<dyn std::error::Error>> {
            let position = BoardPosition::standard();
            let movement = position.parse_uci("e2e4")?;
            let definition = PuzzleDefinition::new(
                PuzzleDefinitionIdentity::new("boundary".into(), "v1".into(), "tactic".into())?,
                position,
                vec![vec![movement]],
                PuzzleDefinitionMetadata::default(),
            )?;
            let mut attempt = PuzzleAttempt::new(PuzzleAttemptId::new(NonZeroU64::MIN), definition);
            attempt.skip()?;
            Ok(attempt)
        }
    }

    #[test]
    fn finalization_reaches_the_counter_limit_then_reports_overflow()
    -> Result<(), Box<dyn std::error::Error>> {
        let attempt = PuzzleSummaryBoundaryFixture::skipped_attempt()?;
        let mut summary = PuzzleSummary {
            finalized: u64::MAX - 1,
            ..PuzzleSummary::default()
        };
        summary.primary_counts[PuzzleSummary::outcome_index(PuzzleEnumPrimaryOutcome::Skipped)] =
            u64::MAX - 1;
        summary.record_attempt(&attempt, Duration::ZERO)?;
        assert_eq!(summary.finalized(), u64::MAX);
        assert_eq!(
            summary.primary_count(PuzzleEnumPrimaryOutcome::Skipped),
            u64::MAX
        );
        assert!(matches!(
            summary.record_attempt(&attempt, Duration::ZERO),
            Err(PuzzleSummaryError::CountOverflow)
        ));
        Ok(())
    }

    #[test]
    fn secondary_event_counter_overflow_is_reported() -> Result<(), Box<dyn std::error::Error>> {
        let attempt = PuzzleSummaryBoundaryFixture::skipped_attempt()?;
        let mut summary = PuzzleSummary {
            skip_events: u64::MAX,
            ..PuzzleSummary::default()
        };
        assert!(matches!(
            summary.record_attempt(&attempt, Duration::ZERO),
            Err(PuzzleSummaryError::CountOverflow)
        ));
        Ok(())
    }

    #[test]
    fn solve_rates_retain_distinct_integers_at_the_unsigned_counter_limit()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut summary = PuzzleSummary {
            finalized: u64::MAX,
            ..PuzzleSummary::default()
        };
        summary.primary_counts[PuzzleSummary::outcome_index(PuzzleEnumPrimaryOutcome::Solved(
            PuzzleEnumAssistance::Unassisted,
        ))] = u64::MAX - 1;
        summary.primary_counts[PuzzleSummary::outcome_index(PuzzleEnumPrimaryOutcome::Solved(
            PuzzleEnumAssistance::Assisted,
        ))] = 1;
        let unassisted = summary
            .solve_rate(PuzzleEnumAssistance::Unassisted)
            .ok_or("Unavailable solve rate")?;
        let assisted = summary
            .solve_rate(PuzzleEnumAssistance::Assisted)
            .ok_or("Unavailable solve rate")?;
        assert_eq!(unassisted.numerator(), u64::MAX - 1);
        assert_eq!(assisted.numerator(), 1);
        assert_eq!(unassisted.denominator().get(), u64::MAX);
        assert_eq!(assisted.denominator(), unassisted.denominator());
        Ok(())
    }
}
