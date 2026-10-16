use std::num::NonZeroU64;
use std::time::Duration;

use thiserror::Error;

use crate::board::r#move::BoardMove;
use crate::clock::duration::{ClockDuration, ClockDurationError};
use crate::puzzle::attempt::{PuzzleAttempt, PuzzleAttemptError, PuzzleAttemptId};
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::{PuzzleEnumCountdownMode, PuzzleEnumHint, PuzzleEnumMoveFeedback};
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::random_provider::PuzzleRandomProvider;

#[derive(Debug, Error)]
pub enum PuzzleSessionConfigError {
    #[error("The puzzle countdown duration must be positive")]
    ZeroDuration,
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleSessionConfig {
    mode: PuzzleEnumCountdownMode,
    duration: ClockDuration,
}

impl PuzzleSessionConfig {
    pub const DEFAULT_SESSION_DURATION: ClockDuration = ClockDuration::from_nanos(300_000_000_000);
    pub const DEFAULT_PUZZLE_DURATION: ClockDuration = ClockDuration::from_nanos(60_000_000_000);

    pub fn new(
        mode: PuzzleEnumCountdownMode,
        duration: Duration,
    ) -> Result<Self, PuzzleSessionConfigError> {
        let duration = ClockDuration::try_from(duration)?;
        if duration == ClockDuration::ZERO {
            return Err(PuzzleSessionConfigError::ZeroDuration);
        }
        Ok(Self { mode, duration })
    }

    pub const fn defaults(mode: PuzzleEnumCountdownMode) -> Self {
        let duration = match mode {
            PuzzleEnumCountdownMode::Session => Self::DEFAULT_SESSION_DURATION,
            PuzzleEnumCountdownMode::PerPuzzle => Self::DEFAULT_PUZZLE_DURATION,
        };
        Self { mode, duration }
    }

    pub const fn mode(self) -> PuzzleEnumCountdownMode {
        self.mode
    }

    pub const fn duration(self) -> ClockDuration {
        self.duration
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PuzzleSessionId(NonZeroU64);

impl PuzzleSessionId {
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> NonZeroU64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PuzzleSessionRevision(u64);

impl PuzzleSessionRevision {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Result<Self, PuzzleSessionError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(PuzzleSessionError::RevisionOverflow)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleSessionContext {
    session: PuzzleSessionId,
    attempt: PuzzleAttemptId,
    definition: PuzzleDefinitionIdentity,
    revision: PuzzleSessionRevision,
}

impl PuzzleSessionContext {
    pub const fn new(
        session: PuzzleSessionId,
        attempt: PuzzleAttemptId,
        definition: PuzzleDefinitionIdentity,
        revision: PuzzleSessionRevision,
    ) -> Self {
        Self {
            session,
            attempt,
            definition,
            revision,
        }
    }
}

#[derive(Debug, Error)]
pub enum PuzzleSessionError {
    #[error("The puzzle input belongs to a different session or attempt")]
    IdentityMismatch,
    #[error("The puzzle source identity or dataset version changed")]
    DefinitionMismatch,
    #[error("The puzzle changed: expected revision {expected:?}, current revision {actual:?}")]
    StaleRevision {
        expected: PuzzleSessionRevision,
        actual: PuzzleSessionRevision,
    },
    #[error("The puzzle session revision counter is exhausted")]
    RevisionOverflow,
    #[error(transparent)]
    Attempt(#[from] PuzzleAttemptError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleSession {
    id: PuzzleSessionId,
    revision: PuzzleSessionRevision,
    config: PuzzleSessionConfig,
    attempt: PuzzleAttempt,
}

impl PuzzleSession {
    pub fn new(
        id: PuzzleSessionId,
        attempt_id: PuzzleAttemptId,
        definition: PuzzleDefinition,
        config: PuzzleSessionConfig,
    ) -> Self {
        Self {
            id,
            revision: PuzzleSessionRevision::ZERO,
            config,
            attempt: PuzzleAttempt::new(attempt_id, definition),
        }
    }

    pub const fn id(&self) -> PuzzleSessionId {
        self.id
    }

    pub const fn revision(&self) -> PuzzleSessionRevision {
        self.revision
    }

    pub const fn config(&self) -> PuzzleSessionConfig {
        self.config
    }

    pub fn attempt(&self) -> &PuzzleAttempt {
        &self.attempt
    }

    pub fn context(&self) -> PuzzleSessionContext {
        PuzzleSessionContext::new(
            self.id,
            self.attempt.id(),
            self.attempt.definition().identity().clone(),
            self.revision,
        )
    }

    pub fn submit_move(
        &mut self,
        expected: &PuzzleSessionContext,
        movement: BoardMove,
    ) -> Result<PuzzleEnumMoveFeedback, PuzzleSessionError> {
        self.transition(expected, |attempt| attempt.submit_move(movement))
    }

    pub fn play_opponent_reply<R: PuzzleRandomProvider + ?Sized>(
        &mut self,
        expected: &PuzzleSessionContext,
        random: &mut R,
    ) -> Result<BoardMove, PuzzleSessionError> {
        self.transition(expected, |attempt| attempt.play_opponent_reply(random))
    }

    pub fn request_hint(
        &mut self,
        expected: &PuzzleSessionContext,
    ) -> Result<PuzzleEnumHint, PuzzleSessionError> {
        self.transition(expected, PuzzleAttempt::request_hint)
    }

    pub fn retry_practice(
        &mut self,
        expected: &PuzzleSessionContext,
    ) -> Result<(), PuzzleSessionError> {
        self.transition(expected, PuzzleAttempt::retry_practice)
    }

    pub fn skip_attempt(
        &mut self,
        expected: &PuzzleSessionContext,
    ) -> Result<(), PuzzleSessionError> {
        self.transition(expected, PuzzleAttempt::skip)
    }

    pub fn record_timeout(
        &mut self,
        expected: &PuzzleSessionContext,
    ) -> Result<(), PuzzleSessionError> {
        let mode = self.config.mode();
        self.transition(expected, |attempt| attempt.record_timeout(mode))
    }

    fn transition<T>(
        &mut self,
        expected: &PuzzleSessionContext,
        operation: impl FnOnce(&mut PuzzleAttempt) -> Result<T, PuzzleAttemptError>,
    ) -> Result<T, PuzzleSessionError> {
        if expected.session != self.id || expected.attempt != self.attempt.id() {
            return Err(PuzzleSessionError::IdentityMismatch);
        }
        if &expected.definition != self.attempt.definition().identity() {
            return Err(PuzzleSessionError::DefinitionMismatch);
        }
        if expected.revision != self.revision {
            return Err(PuzzleSessionError::StaleRevision {
                expected: expected.revision,
                actual: self.revision,
            });
        }
        let revision = self.revision.next()?;
        let result = operation(&mut self.attempt)?;
        self.revision = revision;
        Ok(result)
    }
}
