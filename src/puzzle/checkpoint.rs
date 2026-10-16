use thiserror::Error;

use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::puzzle::random_provider::PuzzleRandomProvider;
use crate::puzzle::run::{PuzzleRun, PuzzleRunEnumStatus, PuzzleRunError};
use crate::puzzle::session::PuzzleSessionContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleCheckpointEnumTrigger {
    AcceptedTransition,
    Countdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleCheckpointEnumFailure {
    WriteFailed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleCheckpointEnumCompletion {
    Committed,
    Failed(PuzzleCheckpointEnumFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleCheckpointEnumFreezeCause {
    InitialSaveRequired,
    DurableLag,
    SaveFailed(PuzzleCheckpointEnumFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleCheckpointFreeze {
    cause: PuzzleCheckpointEnumFreezeCause,
    pause_at: ClockInstant,
}

impl PuzzleCheckpointFreeze {
    pub const fn cause(self) -> PuzzleCheckpointEnumFreezeCause {
        self.cause
    }

    pub const fn pause_at(self) -> ClockInstant {
        self.pause_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleCheckpointEnumAdvance {
    Proceed,
    FreezeRequired(PuzzleCheckpointFreeze),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleCheckpointEnumStatus {
    Open,
    Frozen(PuzzleCheckpointFreeze),
    RetryRequired {
        freeze: PuzzleCheckpointFreeze,
        failure: PuzzleCheckpointEnumFailure,
    },
    AwaitingResume(PuzzleCheckpointFreeze),
}

impl PuzzleCheckpointEnumStatus {
    const fn freeze(self) -> Option<PuzzleCheckpointFreeze> {
        match self {
            Self::Open => None,
            Self::Frozen(freeze)
            | Self::RetryRequired { freeze, .. }
            | Self::AwaitingResume(freeze) => Some(freeze),
        }
    }

    const fn advancement(self) -> PuzzleCheckpointEnumAdvance {
        match self.freeze() {
            Some(freeze) => PuzzleCheckpointEnumAdvance::FreezeRequired(freeze),
            None => PuzzleCheckpointEnumAdvance::Proceed,
        }
    }
}

#[derive(Debug, Error)]
pub enum PuzzleCheckpointError {
    #[error("The first puzzle checkpoint must be captured before active countdown starts")]
    ActiveInitialState,
    #[error("The checkpoint belongs to a different puzzle session")]
    ForeignSession,
    #[error("The checkpoint context does not follow the last accepted puzzle revision")]
    InvalidRevision,
    #[error("A countdown checkpoint cannot replace an unrecorded puzzle transition")]
    UnrecordedTransition,
    #[error("The checkpoint active elapsed counter exceeds its observed active interval")]
    InvalidElapsed,
    #[error("The puzzle has advanced beyond its maximum durable countdown lag")]
    DurableLagExceeded,
    #[error("The countdown checkpoint is not due")]
    NotDue,
    #[error("The puzzle checkpoint is frozen; pause the run before recording its state")]
    PauseRequired,
    #[error("There is no queued puzzle checkpoint to write")]
    NoQueuedWrite,
    #[error("Retry Save is required for the retained puzzle checkpoint")]
    RetryRequired,
    #[error("The completion does not match the current immutable puzzle write request")]
    CompletionMismatch,
    #[error("The puzzle write identity counter is exhausted")]
    WriteIdOverflow,
    #[error("Explicit Resume requires the newest paused snapshot to be durably saved")]
    ResumeUnavailable,
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleCheckpointObservation {
    context: PuzzleSessionContext,
    observed_at: ClockInstant,
    elapsed_active: ClockDuration,
    status: PuzzleRunEnumStatus,
}

impl PuzzleCheckpointObservation {
    pub const fn new(
        context: PuzzleSessionContext,
        observed_at: ClockInstant,
        elapsed_active: ClockDuration,
        status: PuzzleRunEnumStatus,
    ) -> Self {
        Self {
            context,
            observed_at,
            elapsed_active,
            status,
        }
    }

    pub fn from_run<R: PuzzleRandomProvider>(
        run: &PuzzleRun<R>,
        now: ClockInstant,
    ) -> Result<Self, PuzzleRunError> {
        let view = run.view(now)?;
        Ok(Self::new(
            run.context(),
            now,
            view.timer().elapsed(),
            view.status(),
        ))
    }

    pub const fn context(&self) -> &PuzzleSessionContext {
        &self.context
    }

    pub const fn observed_at(&self) -> ClockInstant {
        self.observed_at
    }

    pub const fn elapsed_active(&self) -> ClockDuration {
        self.elapsed_active
    }

    pub const fn status(&self) -> PuzzleRunEnumStatus {
        self.status
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct PuzzleCheckpointSnapshot<S> {
    observation: PuzzleCheckpointObservation,
    payload: S,
}

impl<S> PuzzleCheckpointSnapshot<S> {
    pub const fn new(observation: PuzzleCheckpointObservation, payload: S) -> Self {
        Self {
            observation,
            payload,
        }
    }

    pub const fn observation(&self) -> &PuzzleCheckpointObservation {
        &self.observation
    }

    pub const fn payload(&self) -> &S {
        &self.payload
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PuzzleCheckpointWriteId(u64);

impl PuzzleCheckpointWriteId {
    pub const fn value(self) -> u64 {
        self.0
    }

    fn next(self) -> Result<Self, PuzzleCheckpointError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(PuzzleCheckpointError::WriteIdOverflow)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleCheckpointWriteToken {
    id: PuzzleCheckpointWriteId,
    context: PuzzleSessionContext,
    elapsed_active: ClockDuration,
}

impl PuzzleCheckpointWriteToken {
    pub const fn id(&self) -> PuzzleCheckpointWriteId {
        self.id
    }

    pub const fn context(&self) -> &PuzzleSessionContext {
        &self.context
    }

    pub const fn elapsed_active(&self) -> ClockDuration {
        self.elapsed_active
    }
}

enum PuzzleCheckpointEnumWrite<S> {
    Empty,
    Queued(PuzzleCheckpointSnapshot<S>),
    Writing {
        token: PuzzleCheckpointWriteToken,
        snapshot: PuzzleCheckpointSnapshot<S>,
    },
    Failed {
        token: PuzzleCheckpointWriteToken,
        snapshot: PuzzleCheckpointSnapshot<S>,
    },
}

impl<S> PuzzleCheckpointEnumWrite<S> {
    fn snapshot(&self) -> Option<&PuzzleCheckpointSnapshot<S>> {
        match self {
            Self::Empty => None,
            Self::Queued(snapshot)
            | Self::Writing { snapshot, .. }
            | Self::Failed { snapshot, .. } => Some(snapshot),
        }
    }

    fn token(&self) -> Option<&PuzzleCheckpointWriteToken> {
        match self {
            Self::Empty | Self::Queued(_) => None,
            Self::Writing { token, .. } | Self::Failed { token, .. } => Some(token),
        }
    }
}

pub struct PuzzleCheckpoint<S> {
    observed: PuzzleCheckpointObservation,
    durable: Option<PuzzleCheckpointObservation>,
    write: PuzzleCheckpointEnumWrite<S>,
    coalesced: Option<PuzzleCheckpointSnapshot<S>>,
    status: PuzzleCheckpointEnumStatus,
    last_write_id: PuzzleCheckpointWriteId,
    last_event_at: ClockInstant,
}

impl<S> PuzzleCheckpoint<S> {
    pub const CADENCE: ClockDuration = ClockDuration::from_nanos(250_000_000);
    pub const MAX_DURABLE_LAG: ClockDuration = ClockDuration::from_nanos(1_000_000_000);

    pub fn new(initial: PuzzleCheckpointSnapshot<S>) -> Result<Self, PuzzleCheckpointError> {
        if initial.observation.status == PuzzleRunEnumStatus::Active {
            return Err(PuzzleCheckpointError::ActiveInitialState);
        }
        Ok(Self {
            observed: initial.observation.clone(),
            durable: None,
            last_event_at: initial.observation.observed_at,
            write: PuzzleCheckpointEnumWrite::Queued(initial),
            coalesced: None,
            status: PuzzleCheckpointEnumStatus::Open,
            last_write_id: PuzzleCheckpointWriteId(0),
        })
    }

    pub const fn status(&self) -> PuzzleCheckpointEnumStatus {
        self.status
    }

    pub fn pending(&self) -> Option<&PuzzleCheckpointSnapshot<S>> {
        self.write.snapshot()
    }

    pub const fn coalesced(&self) -> Option<&PuzzleCheckpointSnapshot<S>> {
        self.coalesced.as_ref()
    }

    pub fn write_token(&self) -> Option<&PuzzleCheckpointWriteToken> {
        self.write.token()
    }

    pub const fn durable(&self) -> Option<&PuzzleCheckpointObservation> {
        self.durable.as_ref()
    }

    pub fn before_advance(
        &mut self,
        now: ClockInstant,
    ) -> Result<PuzzleCheckpointEnumAdvance, PuzzleCheckpointError> {
        self.validate_event_time(now)?;
        let freeze = if self.durable.is_none() && self.status.freeze().is_none() {
            Some(PuzzleCheckpointFreeze {
                cause: PuzzleCheckpointEnumFreezeCause::InitialSaveRequired,
                pause_at: self.observed.observed_at,
            })
        } else {
            self.freeze_at(now)?
        };
        if self.status == PuzzleCheckpointEnumStatus::Open
            && let Some(freeze) = freeze
        {
            self.status = PuzzleCheckpointEnumStatus::Frozen(freeze);
        }
        self.last_event_at = now;
        Ok(self.status.advancement())
    }

    pub fn needs_snapshot(
        &self,
        observation: &PuzzleCheckpointObservation,
        trigger: PuzzleCheckpointEnumTrigger,
    ) -> Result<bool, PuzzleCheckpointError> {
        self.validate_observation(observation, trigger)?;
        Ok(match trigger {
            PuzzleCheckpointEnumTrigger::AcceptedTransition => true,
            PuzzleCheckpointEnumTrigger::Countdown => {
                observation.status == PuzzleRunEnumStatus::Active
                    && observation
                        .elapsed_active
                        .checked_sub(self.observed.elapsed_active)?
                        >= Self::CADENCE
            }
        })
    }

    pub fn record(
        &mut self,
        snapshot: PuzzleCheckpointSnapshot<S>,
        trigger: PuzzleCheckpointEnumTrigger,
    ) -> Result<(), PuzzleCheckpointError> {
        if !self.needs_snapshot(&snapshot.observation, trigger)? {
            return Err(PuzzleCheckpointError::NotDue);
        }
        self.observed = snapshot.observation.clone();
        self.last_event_at = snapshot.observation.observed_at;
        match self.write {
            PuzzleCheckpointEnumWrite::Empty => {
                self.write = PuzzleCheckpointEnumWrite::Queued(snapshot);
            }
            PuzzleCheckpointEnumWrite::Queued(_)
            | PuzzleCheckpointEnumWrite::Writing { .. }
            | PuzzleCheckpointEnumWrite::Failed { .. } => self.coalesced = Some(snapshot),
        }
        if let PuzzleCheckpointEnumStatus::AwaitingResume(freeze) = self.status {
            self.status = PuzzleCheckpointEnumStatus::Frozen(freeze);
        }
        Ok(())
    }

    pub fn begin_write(
        &mut self,
        now: ClockInstant,
    ) -> Result<PuzzleCheckpointWriteToken, PuzzleCheckpointError> {
        match &self.write {
            PuzzleCheckpointEnumWrite::Queued(snapshot) => {
                let token = self.next_token(snapshot, now)?;
                self.start_write(token, now)
            }
            PuzzleCheckpointEnumWrite::Failed { .. } => Err(PuzzleCheckpointError::RetryRequired),
            PuzzleCheckpointEnumWrite::Empty | PuzzleCheckpointEnumWrite::Writing { .. } => {
                Err(PuzzleCheckpointError::NoQueuedWrite)
            }
        }
    }

    pub fn retry_save(
        &mut self,
        now: ClockInstant,
    ) -> Result<PuzzleCheckpointWriteToken, PuzzleCheckpointError> {
        let PuzzleCheckpointEnumWrite::Failed { snapshot, .. } = &self.write else {
            return Err(PuzzleCheckpointError::RetryRequired);
        };
        let token = self.next_token(snapshot, now)?;
        self.start_write(token, now)
    }

    pub fn complete(
        &mut self,
        now: ClockInstant,
        received: &PuzzleCheckpointWriteToken,
        completion: PuzzleCheckpointEnumCompletion,
    ) -> Result<PuzzleCheckpointEnumAdvance, PuzzleCheckpointError> {
        let PuzzleCheckpointEnumWrite::Writing { token, .. } = &self.write else {
            return Err(PuzzleCheckpointError::CompletionMismatch);
        };
        if token != received {
            return Err(PuzzleCheckpointError::CompletionMismatch);
        }
        self.validate_event_time(now)?;
        let freeze = self.freeze_at(now)?;
        let PuzzleCheckpointEnumWrite::Writing { token, snapshot } =
            std::mem::replace(&mut self.write, PuzzleCheckpointEnumWrite::Empty)
        else {
            return Err(PuzzleCheckpointError::CompletionMismatch);
        };
        match completion {
            PuzzleCheckpointEnumCompletion::Committed => {
                self.durable = Some(snapshot.observation);
                if let Some(latest) = self.coalesced.take() {
                    self.write = PuzzleCheckpointEnumWrite::Queued(latest);
                }
                if let Some(freeze) = freeze {
                    self.status = if self.write.snapshot().is_some() {
                        PuzzleCheckpointEnumStatus::Frozen(freeze)
                    } else {
                        PuzzleCheckpointEnumStatus::AwaitingResume(freeze)
                    };
                }
            }
            PuzzleCheckpointEnumCompletion::Failed(failure) => {
                self.write = PuzzleCheckpointEnumWrite::Failed { token, snapshot };
                self.status = PuzzleCheckpointEnumStatus::RetryRequired {
                    freeze: PuzzleCheckpointFreeze {
                        cause: PuzzleCheckpointEnumFreezeCause::SaveFailed(failure),
                        pause_at: freeze.map_or(now, PuzzleCheckpointFreeze::pause_at),
                    },
                    failure,
                };
            }
        }
        self.last_event_at = now;
        Ok(self.status.advancement())
    }

    pub fn resume(&mut self, now: ClockInstant) -> Result<(), PuzzleCheckpointError> {
        self.validate_event_time(now)?;
        if !matches!(self.status, PuzzleCheckpointEnumStatus::AwaitingResume(_))
            || !matches!(self.observed.status, PuzzleRunEnumStatus::Paused { .. })
            || self.write.snapshot().is_some()
            || self.coalesced.is_some()
            || self.durable.as_ref() != Some(&self.observed)
        {
            return Err(PuzzleCheckpointError::ResumeUnavailable);
        }
        self.status = PuzzleCheckpointEnumStatus::Open;
        self.last_event_at = now;
        Ok(())
    }

    fn validate_event_time(&self, now: ClockInstant) -> Result<(), PuzzleCheckpointError> {
        now.elapsed_since(self.last_event_at)?;
        Ok(())
    }

    fn validate_observation(
        &self,
        observation: &PuzzleCheckpointObservation,
        trigger: PuzzleCheckpointEnumTrigger,
    ) -> Result<(), PuzzleCheckpointError> {
        self.validate_event_time(observation.observed_at)?;
        if observation.context.session_id() != self.observed.context.session_id() {
            return Err(PuzzleCheckpointError::ForeignSession);
        }
        match trigger {
            PuzzleCheckpointEnumTrigger::AcceptedTransition => {
                if observation.context.revision() <= self.observed.context.revision() {
                    return Err(PuzzleCheckpointError::InvalidRevision);
                }
            }
            PuzzleCheckpointEnumTrigger::Countdown => {
                if observation.context != self.observed.context
                    || observation.status != self.observed.status
                {
                    return Err(PuzzleCheckpointError::UnrecordedTransition);
                }
            }
        }
        let elapsed = observation
            .elapsed_active
            .checked_sub(self.observed.elapsed_active)?;
        let wall_interval = observation
            .observed_at
            .elapsed_since(self.observed.observed_at)?;
        if (self.observed.status != PuzzleRunEnumStatus::Active && elapsed != ClockDuration::ZERO)
            || elapsed > wall_interval
        {
            return Err(PuzzleCheckpointError::InvalidElapsed);
        }
        if let Some(durable) = &self.durable {
            if observation
                .elapsed_active
                .checked_sub(durable.elapsed_active)?
                > Self::MAX_DURABLE_LAG
            {
                return Err(PuzzleCheckpointError::DurableLagExceeded);
            }
        } else if observation.elapsed_active != self.observed.elapsed_active {
            return Err(PuzzleCheckpointError::DurableLagExceeded);
        }
        if self.status.freeze().is_some() && observation.status == PuzzleRunEnumStatus::Active {
            return Err(PuzzleCheckpointError::PauseRequired);
        }
        Ok(())
    }

    fn freeze_at(
        &self,
        now: ClockInstant,
    ) -> Result<Option<PuzzleCheckpointFreeze>, PuzzleCheckpointError> {
        if let Some(freeze) = self.status.freeze() {
            return Ok(Some(freeze));
        }
        if self.observed.status != PuzzleRunEnumStatus::Active {
            return Ok(None);
        }
        let Some(durable) = &self.durable else {
            return Ok(Some(PuzzleCheckpointFreeze {
                cause: PuzzleCheckpointEnumFreezeCause::InitialSaveRequired,
                pause_at: self.observed.observed_at,
            }));
        };
        let lag = self
            .observed
            .elapsed_active
            .checked_sub(durable.elapsed_active)?;
        let remaining = Self::MAX_DURABLE_LAG.checked_sub(lag)?;
        let deadline = self.observed.observed_at.checked_add(remaining)?;
        Ok(now.has_reached(deadline).then_some(PuzzleCheckpointFreeze {
            cause: PuzzleCheckpointEnumFreezeCause::DurableLag,
            pause_at: deadline,
        }))
    }

    fn next_token(
        &self,
        snapshot: &PuzzleCheckpointSnapshot<S>,
        now: ClockInstant,
    ) -> Result<PuzzleCheckpointWriteToken, PuzzleCheckpointError> {
        self.validate_event_time(now)?;
        Ok(PuzzleCheckpointWriteToken {
            id: self.last_write_id.next()?,
            context: snapshot.observation.context.clone(),
            elapsed_active: snapshot.observation.elapsed_active,
        })
    }

    fn start_write(
        &mut self,
        token: PuzzleCheckpointWriteToken,
        now: ClockInstant,
    ) -> Result<PuzzleCheckpointWriteToken, PuzzleCheckpointError> {
        let previous = std::mem::replace(&mut self.write, PuzzleCheckpointEnumWrite::Empty);
        match previous {
            PuzzleCheckpointEnumWrite::Queued(snapshot)
            | PuzzleCheckpointEnumWrite::Failed { snapshot, .. } => {
                self.last_write_id = token.id;
                self.last_event_at = now;
                self.write = PuzzleCheckpointEnumWrite::Writing {
                    token: token.clone(),
                    snapshot,
                };
                Ok(token)
            }
            PuzzleCheckpointEnumWrite::Empty | PuzzleCheckpointEnumWrite::Writing { .. } => {
                self.write = previous;
                Err(PuzzleCheckpointError::NoQueuedWrite)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use crate::clock::duration::{ClockDuration, ClockInstant};
    use crate::puzzle::attempt::PuzzleAttemptId;
    use crate::puzzle::checkpoint::{
        PuzzleCheckpoint, PuzzleCheckpointEnumCompletion, PuzzleCheckpointEnumFailure,
        PuzzleCheckpointError, PuzzleCheckpointObservation, PuzzleCheckpointSnapshot,
        PuzzleCheckpointWriteId,
    };
    use crate::puzzle::identity::PuzzleDefinitionIdentity;
    use crate::puzzle::run::PuzzleRunEnumStatus;
    use crate::puzzle::session::{PuzzleSessionContext, PuzzleSessionId, PuzzleSessionRevision};

    struct PuzzleCheckpointBoundaryPayload(NonZeroU64);

    struct PuzzleCheckpointBoundaryFixture;

    impl PuzzleCheckpointBoundaryFixture {
        fn context(
            revision: PuzzleSessionRevision,
        ) -> Result<PuzzleSessionContext, Box<dyn std::error::Error>> {
            Ok(PuzzleSessionContext::new(
                PuzzleSessionId::new(NonZeroU64::MIN),
                PuzzleAttemptId::new(NonZeroU64::MIN),
                PuzzleDefinitionIdentity::new("boundary".into(), "v1".into(), "a".into())?,
                revision,
            ))
        }

        fn owner()
        -> Result<PuzzleCheckpoint<PuzzleCheckpointBoundaryPayload>, Box<dyn std::error::Error>>
        {
            Ok(PuzzleCheckpoint::new(PuzzleCheckpointSnapshot::new(
                PuzzleCheckpointObservation::new(
                    Self::context(PuzzleSessionRevision::ZERO)?,
                    ClockInstant::from_nanos(0),
                    ClockDuration::ZERO,
                    PuzzleRunEnumStatus::AwaitingReadiness,
                ),
                PuzzleCheckpointBoundaryPayload(NonZeroU64::MIN),
            ))?)
        }
    }

    #[test]
    fn write_identity_overflow_keeps_the_unsent_nonclone_payload()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut checkpoint = PuzzleCheckpointBoundaryFixture::owner()?;
        checkpoint.last_write_id = PuzzleCheckpointWriteId(u64::MAX);
        assert!(matches!(
            checkpoint.begin_write(ClockInstant::from_nanos(0)),
            Err(PuzzleCheckpointError::WriteIdOverflow)
        ));
        assert_eq!(
            checkpoint.pending().map(|value| value.payload().0),
            Some(NonZeroU64::MIN)
        );
        assert!(checkpoint.durable().is_none());
        assert!(checkpoint.write_token().is_none());
        Ok(())
    }

    #[test]
    fn retry_identity_overflow_keeps_the_failed_request_and_does_not_acknowledge_it()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut checkpoint = PuzzleCheckpointBoundaryFixture::owner()?;
        let token = checkpoint.begin_write(ClockInstant::from_nanos(0))?;
        checkpoint.complete(
            ClockInstant::from_nanos(0),
            &token,
            PuzzleCheckpointEnumCompletion::Failed(PuzzleCheckpointEnumFailure::Cancelled),
        )?;
        checkpoint.last_write_id = PuzzleCheckpointWriteId(u64::MAX);
        let status = checkpoint.status();
        assert!(matches!(
            checkpoint.retry_save(ClockInstant::from_nanos(0)),
            Err(PuzzleCheckpointError::WriteIdOverflow)
        ));
        assert_eq!(checkpoint.write_token(), Some(&token));
        assert_eq!(checkpoint.status(), status);
        assert!(checkpoint.durable().is_none());
        assert_eq!(
            checkpoint.pending().map(|value| value.payload().0),
            Some(NonZeroU64::MIN)
        );
        Ok(())
    }

    #[test]
    fn completion_must_match_original_request_revision_and_elapsed_even_with_the_same_write_id()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut checkpoint = PuzzleCheckpointBoundaryFixture::owner()?;
        let original = checkpoint.begin_write(ClockInstant::from_nanos(0))?;
        let mut forged = original.clone();
        forged.context = PuzzleCheckpointBoundaryFixture::context(PuzzleSessionRevision::new(1))?;
        assert!(matches!(
            checkpoint.complete(
                ClockInstant::from_nanos(0),
                &forged,
                PuzzleCheckpointEnumCompletion::Committed
            ),
            Err(PuzzleCheckpointError::CompletionMismatch)
        ));
        forged.context = original.context.clone();
        forged.elapsed_active = ClockDuration::from_nanos(1);
        assert!(matches!(
            checkpoint.complete(
                ClockInstant::from_nanos(0),
                &forged,
                PuzzleCheckpointEnumCompletion::Failed(PuzzleCheckpointEnumFailure::WriteFailed)
            ),
            Err(PuzzleCheckpointError::CompletionMismatch)
        ));
        assert_eq!(checkpoint.write_token(), Some(&original));
        assert!(checkpoint.durable().is_none());
        Ok(())
    }
}
