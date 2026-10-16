use thiserror::Error;

use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::puzzle::enums::PuzzleEnumCountdownMode;
use crate::puzzle::session::PuzzleSessionConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleTimerEnumReadiness {
    BoardNotReady,
    Hidden,
    InputBlocked,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleTimerEnumPauseReason {
    Manual,
    FocusLost,
    WindowHidden,
    Sleep,
    Recovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleTimerEnumPhase {
    AwaitingReadiness,
    Active,
    Review,
    Exhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleTimerEnumExpiry {
    SessionEnded,
    PuzzleTimedOut,
}

impl PuzzleTimerEnumExpiry {
    const fn for_mode(mode: PuzzleEnumCountdownMode) -> Self {
        match mode {
            PuzzleEnumCountdownMode::Session => Self::SessionEnded,
            PuzzleEnumCountdownMode::PerPuzzle => Self::PuzzleTimedOut,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleTimerEnumStatus {
    AwaitingReadiness,
    Running,
    Paused {
        reason: PuzzleTimerEnumPauseReason,
        phase: PuzzleTimerEnumPhase,
    },
    Review,
    Exhausted,
    Expired {
        expiry: PuzzleTimerEnumExpiry,
        interruption: Option<PuzzleTimerEnumPauseReason>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PuzzleTimerEnumState {
    AwaitingReadiness,
    Running(ClockInstant),
    Paused {
        reason: PuzzleTimerEnumPauseReason,
        phase: PuzzleTimerEnumPhase,
    },
    Review,
    Exhausted,
    Expired {
        expiry: PuzzleTimerEnumExpiry,
        interruption: Option<PuzzleTimerEnumPauseReason>,
    },
}

impl PuzzleTimerEnumState {
    const fn status(self) -> PuzzleTimerEnumStatus {
        match self {
            Self::AwaitingReadiness => PuzzleTimerEnumStatus::AwaitingReadiness,
            Self::Running(_) => PuzzleTimerEnumStatus::Running,
            Self::Paused { reason, phase } => PuzzleTimerEnumStatus::Paused { reason, phase },
            Self::Review => PuzzleTimerEnumStatus::Review,
            Self::Exhausted => PuzzleTimerEnumStatus::Exhausted,
            Self::Expired {
                expiry,
                interruption,
            } => PuzzleTimerEnumStatus::Expired {
                expiry,
                interruption,
            },
        }
    }

    const fn phase(self) -> Option<PuzzleTimerEnumPhase> {
        match self {
            Self::AwaitingReadiness => Some(PuzzleTimerEnumPhase::AwaitingReadiness),
            Self::Running(_) => Some(PuzzleTimerEnumPhase::Active),
            Self::Paused { phase, .. } => Some(phase),
            Self::Review => Some(PuzzleTimerEnumPhase::Review),
            Self::Exhausted => Some(PuzzleTimerEnumPhase::Exhausted),
            Self::Expired { .. } => None,
        }
    }

    const fn from_phase(phase: PuzzleTimerEnumPhase, now: ClockInstant) -> Self {
        match phase {
            PuzzleTimerEnumPhase::AwaitingReadiness => Self::AwaitingReadiness,
            PuzzleTimerEnumPhase::Active => Self::Running(now),
            PuzzleTimerEnumPhase::Review => Self::Review,
            PuzzleTimerEnumPhase::Exhausted => Self::Exhausted,
        }
    }

    const fn with_phase(self, phase: PuzzleTimerEnumPhase, now: ClockInstant) -> Self {
        match self {
            Self::Paused { reason, .. } => Self::Paused { reason, phase },
            Self::Expired {
                interruption: Some(reason),
                ..
            } => Self::Paused { reason, phase },
            Self::AwaitingReadiness
            | Self::Running(_)
            | Self::Review
            | Self::Exhausted
            | Self::Expired {
                interruption: None, ..
            } => Self::from_phase(phase, now),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleTimerEnumTransition {
    Unchanged,
    Started,
    Armed,
    ActionAccepted,
    Paused(PuzzleTimerEnumPauseReason),
    Resumed,
    EnteredReview,
    CollectionExhausted,
    Expired(PuzzleTimerEnumExpiry),
}

#[derive(Debug, Error)]
pub enum PuzzleTimerError {
    #[error("The puzzle countdown is not active: {status:?}")]
    NotActive { status: PuzzleTimerEnumStatus },
    #[error("The puzzle countdown is not paused: {status:?}")]
    NotPaused { status: PuzzleTimerEnumStatus },
    #[error("A new puzzle cannot be armed from {status:?}")]
    CannotArmNext { status: PuzzleTimerEnumStatus },
    #[error("A hidden puzzle cannot resume")]
    HiddenResume,
    #[error("The saved puzzle countdown balances do not match its configured budget")]
    SnapshotBalance,
    #[error("The saved puzzle countdown status does not match its balances or mode")]
    SnapshotStatus,
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleTimerView {
    remaining: ClockDuration,
    elapsed: ClockDuration,
    puzzle_elapsed: ClockDuration,
    deadline: Option<ClockInstant>,
    status: PuzzleTimerEnumStatus,
}

impl PuzzleTimerView {
    pub const fn remaining(self) -> ClockDuration {
        self.remaining
    }

    pub const fn elapsed(self) -> ClockDuration {
        self.elapsed
    }

    pub const fn puzzle_elapsed(self) -> ClockDuration {
        self.puzzle_elapsed
    }

    pub const fn deadline(self) -> Option<ClockInstant> {
        self.deadline
    }

    pub const fn status(self) -> PuzzleTimerEnumStatus {
        self.status
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleTimerUpdate {
    view: PuzzleTimerView,
    transition: PuzzleTimerEnumTransition,
}

impl PuzzleTimerUpdate {
    pub const fn view(self) -> PuzzleTimerView {
        self.view
    }

    pub const fn transition(self) -> PuzzleTimerEnumTransition {
        self.transition
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleTimerSnapshotOptions {
    pub config: PuzzleSessionConfig,
    pub remaining: ClockDuration,
    pub elapsed: ClockDuration,
    pub puzzle_elapsed: ClockDuration,
    pub status: PuzzleTimerEnumStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleTimerSnapshot {
    options: PuzzleTimerSnapshotOptions,
}

impl PuzzleTimerSnapshot {
    pub fn from_persisted(options: PuzzleTimerSnapshotOptions) -> Result<Self, PuzzleTimerError> {
        let charged = match options.config.mode() {
            PuzzleEnumCountdownMode::Session => options.elapsed,
            PuzzleEnumCountdownMode::PerPuzzle => options.puzzle_elapsed,
        };
        if options.remaining.checked_add(charged)? != options.config.duration()
            || options.puzzle_elapsed > options.elapsed
        {
            return Err(PuzzleTimerError::SnapshotBalance);
        }
        match options.status {
            PuzzleTimerEnumStatus::Expired { expiry, .. } => {
                if options.remaining != ClockDuration::ZERO
                    || options.puzzle_elapsed == ClockDuration::ZERO
                    || expiry != PuzzleTimerEnumExpiry::for_mode(options.config.mode())
                {
                    return Err(PuzzleTimerError::SnapshotStatus);
                }
            }
            PuzzleTimerEnumStatus::AwaitingReadiness
            | PuzzleTimerEnumStatus::Running
            | PuzzleTimerEnumStatus::Paused { .. }
            | PuzzleTimerEnumStatus::Review
            | PuzzleTimerEnumStatus::Exhausted => {
                if options.remaining == ClockDuration::ZERO {
                    return Err(PuzzleTimerError::SnapshotStatus);
                }
            }
        }
        if matches!(
            options.status,
            PuzzleTimerEnumStatus::AwaitingReadiness
                | PuzzleTimerEnumStatus::Paused {
                    phase: PuzzleTimerEnumPhase::AwaitingReadiness,
                    ..
                }
        ) && options.puzzle_elapsed != ClockDuration::ZERO
        {
            return Err(PuzzleTimerError::SnapshotStatus);
        }
        Ok(Self { options })
    }

    pub const fn persisted(self) -> PuzzleTimerSnapshotOptions {
        self.options
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleTimer {
    config: PuzzleSessionConfig,
    remaining: ClockDuration,
    elapsed: ClockDuration,
    puzzle_elapsed: ClockDuration,
    state: PuzzleTimerEnumState,
    last_observed: Option<ClockInstant>,
}

impl PuzzleTimer {
    pub const fn new(config: PuzzleSessionConfig) -> Self {
        Self {
            config,
            remaining: config.duration(),
            elapsed: ClockDuration::ZERO,
            puzzle_elapsed: ClockDuration::ZERO,
            state: PuzzleTimerEnumState::AwaitingReadiness,
            last_observed: None,
        }
    }

    pub fn recover(snapshot: &PuzzleTimerSnapshot) -> Self {
        let saved = snapshot.persisted();
        let state = match saved.status {
            PuzzleTimerEnumStatus::AwaitingReadiness => {
                Self::recovered_phase(PuzzleTimerEnumPhase::AwaitingReadiness)
            }
            PuzzleTimerEnumStatus::Running => Self::recovered_phase(PuzzleTimerEnumPhase::Active),
            PuzzleTimerEnumStatus::Paused { phase, .. } => Self::recovered_phase(phase),
            PuzzleTimerEnumStatus::Review => Self::recovered_phase(PuzzleTimerEnumPhase::Review),
            PuzzleTimerEnumStatus::Exhausted => {
                Self::recovered_phase(PuzzleTimerEnumPhase::Exhausted)
            }
            PuzzleTimerEnumStatus::Expired { expiry, .. } => PuzzleTimerEnumState::Expired {
                expiry,
                interruption: Some(PuzzleTimerEnumPauseReason::Recovery),
            },
        };
        Self {
            config: saved.config,
            remaining: saved.remaining,
            elapsed: saved.elapsed,
            puzzle_elapsed: saved.puzzle_elapsed,
            state,
            last_observed: None,
        }
    }

    const fn recovered_phase(phase: PuzzleTimerEnumPhase) -> PuzzleTimerEnumState {
        PuzzleTimerEnumState::Paused {
            reason: PuzzleTimerEnumPauseReason::Recovery,
            phase,
        }
    }

    pub const fn config(&self) -> PuzzleSessionConfig {
        self.config
    }

    pub fn view(&self, now: ClockInstant) -> Result<PuzzleTimerView, PuzzleTimerError> {
        let mut projected = *self;
        Ok(projected.poll(now)?.view())
    }

    pub fn snapshot(&self, now: ClockInstant) -> Result<PuzzleTimerSnapshot, PuzzleTimerError> {
        let view = self.view(now)?;
        PuzzleTimerSnapshot::from_persisted(PuzzleTimerSnapshotOptions {
            config: self.config,
            remaining: view.remaining,
            elapsed: view.elapsed,
            puzzle_elapsed: view.puzzle_elapsed,
            status: view.status,
        })
    }

    pub fn poll(&mut self, now: ClockInstant) -> Result<PuzzleTimerUpdate, PuzzleTimerError> {
        self.transition(now, |_timer| Ok(PuzzleTimerEnumTransition::Unchanged))
    }

    pub fn update_readiness(
        &mut self,
        now: ClockInstant,
        readiness: PuzzleTimerEnumReadiness,
    ) -> Result<PuzzleTimerUpdate, PuzzleTimerError> {
        if readiness == PuzzleTimerEnumReadiness::Hidden {
            return self.pause(now, PuzzleTimerEnumPauseReason::WindowHidden);
        }
        self.transition(now, |timer| match readiness {
            PuzzleTimerEnumReadiness::Ready => match timer.state {
                PuzzleTimerEnumState::AwaitingReadiness => {
                    timer.state = PuzzleTimerEnumState::Running(now);
                    Ok(PuzzleTimerEnumTransition::Started)
                }
                PuzzleTimerEnumState::Running(_)
                | PuzzleTimerEnumState::Paused { .. }
                | PuzzleTimerEnumState::Review
                | PuzzleTimerEnumState::Exhausted => Ok(PuzzleTimerEnumTransition::Unchanged),
                PuzzleTimerEnumState::Expired { expiry, .. } => {
                    Ok(PuzzleTimerEnumTransition::Expired(expiry))
                }
            },
            PuzzleTimerEnumReadiness::BoardNotReady
            | PuzzleTimerEnumReadiness::InputBlocked
            | PuzzleTimerEnumReadiness::Hidden => Ok(PuzzleTimerEnumTransition::Unchanged),
        })
    }

    pub fn check_action(
        &mut self,
        now: ClockInstant,
    ) -> Result<PuzzleTimerUpdate, PuzzleTimerError> {
        self.transition(now, |timer| match timer.state {
            PuzzleTimerEnumState::Running(_) => Ok(PuzzleTimerEnumTransition::ActionAccepted),
            PuzzleTimerEnumState::Expired { expiry, .. } => {
                Ok(PuzzleTimerEnumTransition::Expired(expiry))
            }
            PuzzleTimerEnumState::AwaitingReadiness
            | PuzzleTimerEnumState::Paused { .. }
            | PuzzleTimerEnumState::Review
            | PuzzleTimerEnumState::Exhausted => Err(PuzzleTimerError::NotActive {
                status: timer.state.status(),
            }),
        })
    }

    pub fn pause(
        &mut self,
        now: ClockInstant,
        reason: PuzzleTimerEnumPauseReason,
    ) -> Result<PuzzleTimerUpdate, PuzzleTimerError> {
        let mut candidate = *self;
        let mut update = candidate.poll(now)?;
        update.transition = candidate.interrupt(reason);
        update.view = candidate.current_view()?;
        *self = candidate;
        Ok(update)
    }

    pub fn resume(
        &mut self,
        now: ClockInstant,
        readiness: PuzzleTimerEnumReadiness,
    ) -> Result<PuzzleTimerUpdate, PuzzleTimerError> {
        self.transition(now, |timer| {
            let PuzzleTimerEnumState::Paused { mut phase, .. } = timer.state else {
                return Err(PuzzleTimerError::NotPaused {
                    status: timer.state.status(),
                });
            };
            if readiness == PuzzleTimerEnumReadiness::Hidden {
                return Err(PuzzleTimerError::HiddenResume);
            }
            if phase == PuzzleTimerEnumPhase::AwaitingReadiness
                && readiness == PuzzleTimerEnumReadiness::Ready
            {
                phase = PuzzleTimerEnumPhase::Active;
            }
            timer.state = PuzzleTimerEnumState::from_phase(phase, now);
            Ok(PuzzleTimerEnumTransition::Resumed)
        })
    }

    pub fn enter_review(
        &mut self,
        now: ClockInstant,
    ) -> Result<PuzzleTimerUpdate, PuzzleTimerError> {
        self.transition(now, |timer| match timer.state.phase() {
            Some(PuzzleTimerEnumPhase::Active) => {
                timer.state = timer.state.with_phase(PuzzleTimerEnumPhase::Review, now);
                Ok(PuzzleTimerEnumTransition::EnteredReview)
            }
            Some(PuzzleTimerEnumPhase::Review) => Ok(PuzzleTimerEnumTransition::Unchanged),
            Some(PuzzleTimerEnumPhase::AwaitingReadiness | PuzzleTimerEnumPhase::Exhausted) => {
                Err(PuzzleTimerError::NotActive {
                    status: timer.state.status(),
                })
            }
            None => timer.expired_transition(),
        })
    }

    pub fn exhaust_collection(
        &mut self,
        now: ClockInstant,
    ) -> Result<PuzzleTimerUpdate, PuzzleTimerError> {
        self.transition(now, |timer| match timer.state.phase() {
            Some(PuzzleTimerEnumPhase::Exhausted) => Ok(PuzzleTimerEnumTransition::Unchanged),
            Some(
                PuzzleTimerEnumPhase::Active
                | PuzzleTimerEnumPhase::Review
                | PuzzleTimerEnumPhase::AwaitingReadiness,
            ) => {
                timer.state = timer.state.with_phase(PuzzleTimerEnumPhase::Exhausted, now);
                Ok(PuzzleTimerEnumTransition::CollectionExhausted)
            }
            None => timer.expired_transition(),
        })
    }

    pub fn arm_next(&mut self, now: ClockInstant) -> Result<PuzzleTimerUpdate, PuzzleTimerError> {
        self.transition(now, |timer| {
            match timer.state {
                PuzzleTimerEnumState::Review
                | PuzzleTimerEnumState::Exhausted
                | PuzzleTimerEnumState::Paused {
                    phase: PuzzleTimerEnumPhase::Review | PuzzleTimerEnumPhase::Exhausted,
                    ..
                }
                | PuzzleTimerEnumState::Expired {
                    expiry: PuzzleTimerEnumExpiry::PuzzleTimedOut,
                    ..
                } => {}
                PuzzleTimerEnumState::Expired {
                    expiry: PuzzleTimerEnumExpiry::SessionEnded,
                    ..
                } => {
                    return timer.expired_transition();
                }
                PuzzleTimerEnumState::AwaitingReadiness
                | PuzzleTimerEnumState::Running(_)
                | PuzzleTimerEnumState::Paused {
                    phase: PuzzleTimerEnumPhase::AwaitingReadiness | PuzzleTimerEnumPhase::Active,
                    ..
                } => {
                    return Err(PuzzleTimerError::CannotArmNext {
                        status: timer.state.status(),
                    });
                }
            }
            match timer.config.mode() {
                PuzzleEnumCountdownMode::Session => {}
                PuzzleEnumCountdownMode::PerPuzzle => timer.remaining = timer.config.duration(),
            }
            timer.puzzle_elapsed = ClockDuration::ZERO;
            timer.state = timer
                .state
                .with_phase(PuzzleTimerEnumPhase::AwaitingReadiness, now);
            Ok(PuzzleTimerEnumTransition::Armed)
        })
    }

    fn expired_transition(&self) -> Result<PuzzleTimerEnumTransition, PuzzleTimerError> {
        match self.state {
            PuzzleTimerEnumState::Expired { expiry, .. } => {
                Ok(PuzzleTimerEnumTransition::Expired(expiry))
            }
            PuzzleTimerEnumState::AwaitingReadiness
            | PuzzleTimerEnumState::Running(_)
            | PuzzleTimerEnumState::Paused { .. }
            | PuzzleTimerEnumState::Review
            | PuzzleTimerEnumState::Exhausted => Err(PuzzleTimerError::NotActive {
                status: self.state.status(),
            }),
        }
    }

    fn interrupt(&mut self, reason: PuzzleTimerEnumPauseReason) -> PuzzleTimerEnumTransition {
        let phase = match self.state {
            PuzzleTimerEnumState::Paused { .. } => return PuzzleTimerEnumTransition::Unchanged,
            PuzzleTimerEnumState::Expired {
                expiry,
                interruption,
            } => {
                self.state = PuzzleTimerEnumState::Expired {
                    expiry,
                    interruption: interruption.or(Some(reason)),
                };
                return PuzzleTimerEnumTransition::Expired(expiry);
            }
            PuzzleTimerEnumState::AwaitingReadiness => PuzzleTimerEnumPhase::AwaitingReadiness,
            PuzzleTimerEnumState::Running(_) => PuzzleTimerEnumPhase::Active,
            PuzzleTimerEnumState::Review => PuzzleTimerEnumPhase::Review,
            PuzzleTimerEnumState::Exhausted => PuzzleTimerEnumPhase::Exhausted,
        };
        self.state = PuzzleTimerEnumState::Paused { reason, phase };
        PuzzleTimerEnumTransition::Paused(reason)
    }

    fn transition(
        &mut self,
        now: ClockInstant,
        operation: impl FnOnce(&mut Self) -> Result<PuzzleTimerEnumTransition, PuzzleTimerError>,
    ) -> Result<PuzzleTimerUpdate, PuzzleTimerError> {
        let mut candidate = *self;
        let transition = match candidate.advance(now)? {
            Some(expiry) => PuzzleTimerEnumTransition::Expired(expiry),
            None => operation(&mut candidate)?,
        };
        let view = candidate.current_view()?;
        *self = candidate;
        Ok(PuzzleTimerUpdate { view, transition })
    }

    fn advance(
        &mut self,
        now: ClockInstant,
    ) -> Result<Option<PuzzleTimerEnumExpiry>, PuzzleTimerError> {
        if let Some(previous) = self.last_observed {
            now.elapsed_since(previous)?;
        }
        let mut expiry = None;
        if let PuzzleTimerEnumState::Running(since) = self.state {
            let charge = now.elapsed_since(since)?.min(self.remaining);
            self.elapsed = self.elapsed.checked_add(charge)?;
            self.puzzle_elapsed = self.puzzle_elapsed.checked_add(charge)?;
            self.remaining = self.remaining.checked_sub(charge)?;
            self.state = if self.remaining == ClockDuration::ZERO {
                let expired = PuzzleTimerEnumExpiry::for_mode(self.config.mode());
                expiry = Some(expired);
                PuzzleTimerEnumState::Expired {
                    expiry: expired,
                    interruption: None,
                }
            } else {
                PuzzleTimerEnumState::Running(now)
            };
        }
        self.last_observed = Some(now);
        Ok(expiry)
    }

    fn current_view(&self) -> Result<PuzzleTimerView, PuzzleTimerError> {
        let deadline = match self.state {
            PuzzleTimerEnumState::Running(since) => Some(since.checked_add(self.remaining)?),
            PuzzleTimerEnumState::AwaitingReadiness
            | PuzzleTimerEnumState::Paused { .. }
            | PuzzleTimerEnumState::Review
            | PuzzleTimerEnumState::Exhausted
            | PuzzleTimerEnumState::Expired { .. } => None,
        };
        Ok(PuzzleTimerView {
            remaining: self.remaining,
            elapsed: self.elapsed,
            puzzle_elapsed: self.puzzle_elapsed,
            deadline,
            status: self.state.status(),
        })
    }
}
