use std::num::NonZeroU64;

use thiserror::Error;

use crate::board::r#move::BoardMove;
use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::puzzle::attempt::{PuzzleAttempt, PuzzleAttemptId};
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::{PuzzleEnumHint, PuzzleEnumMoveFeedback, PuzzleEnumPhase};
use crate::puzzle::random_provider::PuzzleRandomProvider;
use crate::puzzle::selection::{
    PuzzleSelection, PuzzleSelectionEnumStatus, PuzzleSelectionError, PuzzleSelectionSnapshot,
};
use crate::puzzle::session::{
    PuzzleSession, PuzzleSessionConfig, PuzzleSessionContext, PuzzleSessionError, PuzzleSessionId,
};
use crate::puzzle::summary::{PuzzleSummary, PuzzleSummaryAttempt, PuzzleSummaryError};
use crate::puzzle::timer::{
    PuzzleTimer, PuzzleTimerEnumExpiry, PuzzleTimerEnumPauseReason, PuzzleTimerEnumPhase,
    PuzzleTimerEnumReadiness, PuzzleTimerEnumStatus, PuzzleTimerEnumTransition, PuzzleTimerError,
    PuzzleTimerSnapshot, PuzzleTimerUpdate, PuzzleTimerView,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleRunEnumConfirmation {
    Unconfirmed,
    Confirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleRunEnumEndReason {
    Confirmed,
    SessionExpired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleRunEnumStatus {
    AwaitingReadiness,
    Active,
    Paused {
        reason: PuzzleTimerEnumPauseReason,
        phase: PuzzleTimerEnumPhase,
    },
    Review,
    Exhausted,
    Expired(PuzzleTimerEnumExpiry),
    Ended(PuzzleRunEnumEndReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleRunEnumAction<T> {
    Applied(T),
    Expired(PuzzleTimerEnumExpiry),
}

impl<T> PuzzleRunEnumAction<T> {
    pub fn map<U>(self, operation: impl FnOnce(T) -> U) -> PuzzleRunEnumAction<U> {
        match self {
            Self::Applied(value) => PuzzleRunEnumAction::Applied(operation(value)),
            Self::Expired(expiry) => PuzzleRunEnumAction::Expired(expiry),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleRunEnumContent<'a> {
    Hidden,
    Board(&'a PuzzleAttempt),
    Review(&'a PuzzleAttempt),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleRunView<'a> {
    status: PuzzleRunEnumStatus,
    timer: PuzzleTimerView,
    content: PuzzleRunEnumContent<'a>,
}

impl<'a> PuzzleRunView<'a> {
    pub const fn status(self) -> PuzzleRunEnumStatus {
        self.status
    }

    pub const fn timer(self) -> PuzzleTimerView {
        self.timer
    }

    pub const fn content(self) -> PuzzleRunEnumContent<'a> {
        self.content
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleRunEnumAdvanceReason {
    Next,
    Skipped,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleRunEnumEvent {
    Timer(PuzzleTimerEnumTransition),
    Review,
    Advanced(PuzzleRunEnumAdvanceReason),
    Reshuffled,
    Ended(PuzzleRunEnumEndReason),
    Recovered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleRunEvent {
    context: PuzzleSessionContext,
    elapsed_active: ClockDuration,
    event: PuzzleRunEnumEvent,
}

impl PuzzleRunEvent {
    pub const fn context(&self) -> &PuzzleSessionContext {
        &self.context
    }

    pub const fn elapsed_active(&self) -> ClockDuration {
        self.elapsed_active
    }

    pub const fn event(&self) -> PuzzleRunEnumEvent {
        self.event
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleRunAttempt {
    attempt: PuzzleAttempt,
    elapsed_active: ClockDuration,
}

impl PuzzleRunAttempt {
    pub const fn attempt(&self) -> &PuzzleAttempt {
        &self.attempt
    }

    pub const fn elapsed_active(&self) -> ClockDuration {
        self.elapsed_active
    }
}

#[derive(Debug, Error)]
pub enum PuzzleRunError {
    #[error("No puzzles match the selected collection")]
    NoMatchingPuzzles,
    #[error("The run has ended: {0:?}")]
    Ended(PuzzleRunEnumEndReason),
    #[error("This puzzle run operation is unavailable in {0:?}")]
    InvalidStatus(PuzzleRunEnumStatus),
    #[error("End Run requires confirmation")]
    ConfirmationRequired,
    #[error("The puzzle attempt identity counter is exhausted")]
    AttemptIdOverflow,
    #[error("The recovered session does not match the pinned selected puzzle")]
    RecoveryMismatch,
    #[error(transparent)]
    Session(#[from] PuzzleSessionError),
    #[error(transparent)]
    Timer(#[from] PuzzleTimerError),
    #[error(transparent)]
    Selection(#[from] PuzzleSelectionError),
    #[error(transparent)]
    Summary(#[from] PuzzleSummaryError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleRunSnapshot {
    selection: PuzzleSelectionSnapshot,
    session: PuzzleSession,
    timer: PuzzleTimerSnapshot,
    previous_attempts: Vec<PuzzleRunAttempt>,
    events: Vec<PuzzleRunEvent>,
    ended: Option<PuzzleRunEnumEndReason>,
    finalized_elapsed: Option<ClockDuration>,
}

struct PuzzleRunActionCandidate {
    session: PuzzleSession,
    timer: PuzzleTimer,
    view: PuzzleTimerView,
}

impl PuzzleRunActionCandidate {
    fn apply<T>(
        &mut self,
        now: ClockInstant,
        operation: impl FnOnce(&mut PuzzleSession) -> Result<T, PuzzleSessionError>,
    ) -> Result<T, PuzzleRunError> {
        let value = operation(&mut self.session)?;
        if self.session.attempt().phase() == PuzzleEnumPhase::Review {
            self.view = self.timer.enter_review(now)?.view();
        }
        Ok(value)
    }
}

pub struct PuzzleRun<R> {
    selection: PuzzleSelection<R>,
    session: PuzzleSession,
    timer: PuzzleTimer,
    previous_attempts: Vec<PuzzleRunAttempt>,
    events: Vec<PuzzleRunEvent>,
    ended: Option<PuzzleRunEnumEndReason>,
    finalized_elapsed: Option<ClockDuration>,
}

impl<R: PuzzleRandomProvider> PuzzleRun<R> {
    pub fn new(
        id: PuzzleSessionId,
        config: PuzzleSessionConfig,
        selection: PuzzleSelection<R>,
    ) -> Result<Self, PuzzleRunError> {
        let definition = selection
            .current()
            .ok_or(PuzzleRunError::NoMatchingPuzzles)?
            .clone();
        Ok(Self {
            selection,
            session: PuzzleSession::new(
                id,
                PuzzleAttemptId::new(NonZeroU64::MIN),
                definition,
                config,
            ),
            timer: PuzzleTimer::new(config),
            previous_attempts: Vec::new(),
            events: Vec::new(),
            ended: None,
            finalized_elapsed: None,
        })
    }

    pub fn context(&self) -> PuzzleSessionContext {
        self.session.context()
    }

    pub fn previous_attempts(&self) -> &[PuzzleRunAttempt] {
        &self.previous_attempts
    }

    pub fn events(&self) -> &[PuzzleRunEvent] {
        &self.events
    }

    pub fn view(&self, now: ClockInstant) -> Result<PuzzleRunView<'_>, PuzzleRunError> {
        let timer = self.timer.view(now)?;
        let status = self.status(timer.status());
        let content = match status {
            PuzzleRunEnumStatus::AwaitingReadiness | PuzzleRunEnumStatus::Active => {
                PuzzleRunEnumContent::Board(self.session.attempt())
            }
            PuzzleRunEnumStatus::Review => PuzzleRunEnumContent::Review(self.session.attempt()),
            PuzzleRunEnumStatus::Paused { .. }
            | PuzzleRunEnumStatus::Exhausted
            | PuzzleRunEnumStatus::Expired(_)
            | PuzzleRunEnumStatus::Ended(_) => PuzzleRunEnumContent::Hidden,
        };
        Ok(PuzzleRunView {
            status,
            timer,
            content,
        })
    }

    pub fn summary(&self, now: ClockInstant) -> Result<PuzzleSummary, PuzzleRunError> {
        let current_elapsed = self.attempt_elapsed(self.timer.view(now)?);
        let previous = self.previous_attempts.iter().map(|entry| {
            PuzzleSummaryAttempt::new(
                self.session.id(),
                &entry.attempt,
                entry.elapsed_active.as_duration(),
            )
        });
        let current = PuzzleSummaryAttempt::new(
            self.session.id(),
            self.session.attempt(),
            current_elapsed.as_duration(),
        );
        Ok(PuzzleSummary::from_attempts(
            previous.chain(std::iter::once(current)),
        )?)
    }

    pub fn submit_move(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
        movement: BoardMove,
    ) -> Result<PuzzleRunEnumAction<PuzzleEnumMoveFeedback>, PuzzleRunError> {
        self.timed_action(expected, now, |session| {
            session.submit_move(expected, movement)
        })
    }

    pub fn request_hint(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
    ) -> Result<PuzzleRunEnumAction<PuzzleEnumHint>, PuzzleRunError> {
        self.timed_action(expected, now, |session| session.request_hint(expected))
    }

    pub fn retry_practice(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
    ) -> Result<PuzzleRunEnumAction<()>, PuzzleRunError> {
        self.timed_action(expected, now, |session| session.retry_practice(expected))
    }

    pub fn play_opponent_reply(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
    ) -> Result<PuzzleRunEnumAction<BoardMove>, PuzzleRunError>
    where
        R: Clone,
    {
        let mut candidate = match self.prepare_action(expected, now)? {
            PuzzleRunEnumAction::Applied(candidate) => candidate,
            PuzzleRunEnumAction::Expired(expiry) => {
                return Ok(PuzzleRunEnumAction::Expired(expiry));
            }
        };
        let value = self.selection.random_mut().transaction(|random| {
            candidate.apply(now, |session| session.play_opponent_reply(expected, random))
        })?;
        self.commit_action(candidate);
        Ok(PuzzleRunEnumAction::Applied(value))
    }

    pub fn update_readiness(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
        readiness: PuzzleTimerEnumReadiness,
    ) -> Result<Option<PuzzleTimerEnumExpiry>, PuzzleRunError> {
        self.timer_transition(expected, now, |timer| {
            timer.update_readiness(now, readiness)
        })
    }

    pub fn pause(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
        reason: PuzzleTimerEnumPauseReason,
    ) -> Result<Option<PuzzleTimerEnumExpiry>, PuzzleRunError> {
        self.timer_transition(expected, now, |timer| timer.pause(now, reason))
    }

    pub fn resume(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
        readiness: PuzzleTimerEnumReadiness,
    ) -> Result<Option<PuzzleTimerEnumExpiry>, PuzzleRunError> {
        if matches!(
            self.timer.view(now)?.status(),
            PuzzleTimerEnumStatus::Paused {
                phase: PuzzleTimerEnumPhase::Active | PuzzleTimerEnumPhase::AwaitingReadiness,
                ..
            }
        ) && matches!(
            readiness,
            PuzzleTimerEnumReadiness::BoardNotReady | PuzzleTimerEnumReadiness::InputBlocked
        ) {
            self.validate_open(expected)?;
            return Err(PuzzleRunError::InvalidStatus(self.view(now)?.status()));
        }
        self.timer_transition(expected, now, |timer| timer.resume(now, readiness))
    }

    pub fn poll(
        &mut self,
        now: ClockInstant,
    ) -> Result<Option<PuzzleTimerEnumExpiry>, PuzzleRunError> {
        let mut timer = self.timer;
        let update = timer.poll(now)?;
        if let PuzzleTimerEnumTransition::Expired(expiry) = update.transition()
            && self.ended.is_none()
            && self.selection.current().is_some()
            && self.session.attempt().phase().is_unfinished()
        {
            self.apply_expiry(timer, now, expiry)?;
            return Ok(Some(expiry));
        }
        self.timer = timer;
        Ok(None)
    }

    pub fn next(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
    ) -> Result<(), PuzzleRunError> {
        self.validate_open(expected)?;
        let view = self.view(now)?;
        if view.status() != PuzzleRunEnumStatus::Review {
            return Err(PuzzleRunError::InvalidStatus(view.status()));
        }
        self.advance(
            self.session.clone(),
            self.timer,
            now,
            PuzzleRunEnumAdvanceReason::Next,
        )
    }

    pub fn skip(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
    ) -> Result<PuzzleRunEnumAction<()>, PuzzleRunError> {
        let mut candidate = match self.prepare_action(expected, now)? {
            PuzzleRunEnumAction::Applied(candidate) => candidate,
            PuzzleRunEnumAction::Expired(expiry) => {
                return Ok(PuzzleRunEnumAction::Expired(expiry));
            }
        };
        candidate.session.skip_attempt(expected)?;
        candidate.timer.enter_review(now)?;
        self.advance(
            candidate.session,
            candidate.timer,
            now,
            PuzzleRunEnumAdvanceReason::Skipped,
        )?;
        Ok(PuzzleRunEnumAction::Applied(()))
    }

    pub fn reshuffle_and_continue(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
    ) -> Result<(), PuzzleRunError>
    where
        R: Clone,
    {
        self.validate_open(expected)?;
        let view = self.view(now)?;
        if view.status() != PuzzleRunEnumStatus::Exhausted {
            return Err(PuzzleRunError::InvalidStatus(view.status()));
        }
        let id = self.next_attempt_id()?;
        let revision = self.session.revision().next()?;
        let elapsed = self.attempt_elapsed(view.timer());
        let mut timer = self.timer;
        let update = timer.arm_next(now)?;
        self.selection.reshuffle_and_continue()?;
        if let Some(definition) = self.selection.current() {
            let previous = self
                .session
                .replace_attempt(id, definition.clone(), revision);
            self.previous_attempts.push(PuzzleRunAttempt {
                attempt: previous,
                elapsed_active: elapsed,
            });
            self.finalized_elapsed = None;
        }
        self.timer = timer;
        self.record_event(PuzzleRunEnumEvent::Reshuffled, update.view().elapsed());
        Ok(())
    }

    pub fn end_run(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
        confirmation: PuzzleRunEnumConfirmation,
    ) -> Result<PuzzleRunEnumAction<()>, PuzzleRunError> {
        self.validate_open(expected)?;
        if confirmation != PuzzleRunEnumConfirmation::Confirmed {
            return Err(PuzzleRunError::ConfirmationRequired);
        }
        let mut timer = self.timer;
        let update = timer.poll(now)?;
        let expiry = if let PuzzleTimerEnumTransition::Expired(expiry) = update.transition()
            && self.session.attempt().phase().is_unfinished()
        {
            Some(expiry)
        } else {
            None
        };
        let mut session = self.session.clone();
        if expiry.is_some() {
            session.record_timeout(expected)?;
        } else if session.attempt().phase().is_unfinished() {
            session.skip_attempt(expected)?;
        } else {
            session.set_revision(session.revision().next()?);
        }
        let update = timer.exhaust_collection(now)?;
        self.finalized_elapsed = Some(self.attempt_elapsed(update.view()));
        self.session = session;
        self.timer = timer;
        let reason = match expiry {
            Some(PuzzleTimerEnumExpiry::SessionEnded) => PuzzleRunEnumEndReason::SessionExpired,
            Some(PuzzleTimerEnumExpiry::PuzzleTimedOut) | None => PuzzleRunEnumEndReason::Confirmed,
        };
        self.ended = Some(reason);
        self.record_event(PuzzleRunEnumEvent::Ended(reason), update.view().elapsed());
        Ok(match expiry {
            Some(expiry) => PuzzleRunEnumAction::Expired(expiry),
            None => PuzzleRunEnumAction::Applied(()),
        })
    }

    pub fn snapshot(&mut self, now: ClockInstant) -> Result<PuzzleRunSnapshot, PuzzleRunError> {
        self.poll(now)?;
        Ok(PuzzleRunSnapshot {
            selection: self.selection.snapshot()?,
            session: self.session.clone(),
            timer: self.timer.snapshot(now)?,
            previous_attempts: self.previous_attempts.clone(),
            events: self.events.clone(),
            ended: self.ended,
            finalized_elapsed: self.finalized_elapsed,
        })
    }

    pub fn recover(
        snapshot: PuzzleRunSnapshot,
        definitions: Vec<PuzzleDefinition>,
        freshly_seeded_random: R,
    ) -> Result<Self, PuzzleRunError> {
        let selection =
            PuzzleSelection::restore(definitions, snapshot.selection, freshly_seeded_random)?;
        if let Some(definition) = selection.current()
            && definition != snapshot.session.attempt().definition()
        {
            return Err(PuzzleRunError::RecoveryMismatch);
        }
        let mut session = snapshot.session;
        session.set_revision(session.revision().next()?);
        let timer = PuzzleTimer::recover(&snapshot.timer);
        let mut run = Self {
            selection,
            session,
            timer,
            previous_attempts: snapshot.previous_attempts,
            events: snapshot.events,
            ended: snapshot.ended,
            finalized_elapsed: snapshot.finalized_elapsed,
        };
        run.record_event(
            PuzzleRunEnumEvent::Recovered,
            snapshot.timer.persisted().elapsed,
        );
        Ok(run)
    }

    fn status(&self, timer: PuzzleTimerEnumStatus) -> PuzzleRunEnumStatus {
        if let Some(reason) = self.ended {
            return PuzzleRunEnumStatus::Ended(reason);
        }
        match timer {
            PuzzleTimerEnumStatus::AwaitingReadiness => PuzzleRunEnumStatus::AwaitingReadiness,
            PuzzleTimerEnumStatus::Running => PuzzleRunEnumStatus::Active,
            PuzzleTimerEnumStatus::Review => PuzzleRunEnumStatus::Review,
            PuzzleTimerEnumStatus::Exhausted => PuzzleRunEnumStatus::Exhausted,
            PuzzleTimerEnumStatus::Expired { expiry, .. } => PuzzleRunEnumStatus::Expired(expiry),
            PuzzleTimerEnumStatus::Paused { reason, phase } => {
                PuzzleRunEnumStatus::Paused { reason, phase }
            }
        }
    }

    fn validate_open(&self, expected: &PuzzleSessionContext) -> Result<(), PuzzleRunError> {
        self.session.validate_context(expected)?;
        if let Some(reason) = self.ended {
            return Err(PuzzleRunError::Ended(reason));
        }
        Ok(())
    }

    fn prepare_action(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
    ) -> Result<PuzzleRunEnumAction<PuzzleRunActionCandidate>, PuzzleRunError> {
        self.validate_open(expected)?;
        let mut timer = self.timer;
        let update = timer.check_action(now)?;
        if let PuzzleTimerEnumTransition::Expired(expiry) = update.transition() {
            if self.selection.current().is_some() && self.session.attempt().phase().is_unfinished()
            {
                self.apply_expiry(timer, now, expiry)?;
                return Ok(PuzzleRunEnumAction::Expired(expiry));
            }
            return Err(PuzzleRunError::InvalidStatus(self.view(now)?.status()));
        }
        self.session.revision().next()?;
        Ok(PuzzleRunEnumAction::Applied(PuzzleRunActionCandidate {
            session: self.session.clone(),
            timer,
            view: update.view(),
        }))
    }

    fn timed_action<T>(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
        operation: impl FnOnce(&mut PuzzleSession) -> Result<T, PuzzleSessionError>,
    ) -> Result<PuzzleRunEnumAction<T>, PuzzleRunError> {
        let mut candidate = match self.prepare_action(expected, now)? {
            PuzzleRunEnumAction::Applied(candidate) => candidate,
            PuzzleRunEnumAction::Expired(expiry) => {
                return Ok(PuzzleRunEnumAction::Expired(expiry));
            }
        };
        let value = candidate.apply(now, operation)?;
        self.commit_action(candidate);
        Ok(PuzzleRunEnumAction::Applied(value))
    }

    fn commit_action(&mut self, candidate: PuzzleRunActionCandidate) {
        self.session = candidate.session;
        self.timer = candidate.timer;
        if self.session.attempt().phase() == PuzzleEnumPhase::Review {
            self.finalized_elapsed = Some(candidate.view.puzzle_elapsed());
            self.record_event(PuzzleRunEnumEvent::Review, candidate.view.elapsed());
        }
    }

    fn timer_transition(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
        operation: impl FnOnce(&mut PuzzleTimer) -> Result<PuzzleTimerUpdate, PuzzleTimerError>,
    ) -> Result<Option<PuzzleTimerEnumExpiry>, PuzzleRunError> {
        self.validate_open(expected)?;
        let mut timer = self.timer;
        let update = operation(&mut timer)?;
        if let PuzzleTimerEnumTransition::Expired(expiry) = update.transition()
            && self.selection.current().is_some()
            && self.session.attempt().phase().is_unfinished()
        {
            self.apply_expiry(timer, now, expiry)?;
            return Ok(Some(expiry));
        }
        if update.transition() != PuzzleTimerEnumTransition::Unchanged {
            self.session.set_revision(self.session.revision().next()?);
            self.record_event(
                PuzzleRunEnumEvent::Timer(update.transition()),
                update.view().elapsed(),
            );
        }
        self.timer = timer;
        Ok(None)
    }

    fn apply_expiry(
        &mut self,
        timer: PuzzleTimer,
        now: ClockInstant,
        expiry: PuzzleTimerEnumExpiry,
    ) -> Result<(), PuzzleRunError> {
        let mut session = self.session.clone();
        session.record_timeout(&session.context())?;
        match expiry {
            PuzzleTimerEnumExpiry::SessionEnded => {
                let view = timer.view(now)?;
                self.session = session;
                self.timer = timer;
                self.ended = Some(PuzzleRunEnumEndReason::SessionExpired);
                self.finalized_elapsed = Some(view.puzzle_elapsed());
                self.record_event(
                    PuzzleRunEnumEvent::Ended(PuzzleRunEnumEndReason::SessionExpired),
                    view.elapsed(),
                );
            }
            PuzzleTimerEnumExpiry::PuzzleTimedOut => {
                self.advance(session, timer, now, PuzzleRunEnumAdvanceReason::TimedOut)?;
            }
        }
        Ok(())
    }

    fn advance(
        &mut self,
        mut session: PuzzleSession,
        timer: PuzzleTimer,
        now: ClockInstant,
        reason: PuzzleRunEnumAdvanceReason,
    ) -> Result<(), PuzzleRunError> {
        let id = self.next_attempt_id()?;
        let revision = session.revision().next()?;
        let elapsed = self.attempt_elapsed(timer.view(now)?);
        let mut armed = timer;
        let armed_update = armed.arm_next(now)?;
        let mut exhausted = armed;
        let exhausted_update = exhausted.exhaust_collection(now)?;
        self.selection.advance();
        if let Some(definition) = self.selection.current() {
            let previous = session.replace_attempt(id, definition.clone(), revision);
            self.previous_attempts.push(PuzzleRunAttempt {
                attempt: previous,
                elapsed_active: elapsed,
            });
            self.timer = armed;
            self.finalized_elapsed = None;
        } else {
            session.set_revision(revision);
            self.timer = exhausted;
            self.finalized_elapsed = Some(elapsed);
        }
        self.session = session;
        self.record_event(
            PuzzleRunEnumEvent::Advanced(reason),
            armed_update.view().elapsed(),
        );
        if self.selection.status() == PuzzleSelectionEnumStatus::Exhausted {
            self.record_event(
                PuzzleRunEnumEvent::Timer(exhausted_update.transition()),
                exhausted_update.view().elapsed(),
            );
        }
        Ok(())
    }

    fn next_attempt_id(&self) -> Result<PuzzleAttemptId, PuzzleRunError> {
        self.session
            .attempt()
            .id()
            .value()
            .checked_add(1)
            .map(PuzzleAttemptId::new)
            .ok_or(PuzzleRunError::AttemptIdOverflow)
    }

    fn attempt_elapsed(&self, timer: PuzzleTimerView) -> ClockDuration {
        self.finalized_elapsed.unwrap_or(timer.puzzle_elapsed())
    }

    fn record_event(&mut self, event: PuzzleRunEnumEvent, elapsed_active: ClockDuration) {
        self.events.push(PuzzleRunEvent {
            context: self.context(),
            elapsed_active,
            event,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU64, NonZeroUsize};

    use crate::board::position::BoardPosition;
    use crate::clock::duration::ClockInstant;
    use crate::puzzle::attempt::PuzzleAttemptId;
    use crate::puzzle::definition::PuzzleDefinition;
    use crate::puzzle::enums::PuzzleEnumCountdownMode;
    use crate::puzzle::identity::PuzzleDefinitionIdentity;
    use crate::puzzle::metadata::PuzzleDefinitionMetadata;
    use crate::puzzle::random_provider::PuzzleRandomProvider;
    use crate::puzzle::run::{PuzzleRun, PuzzleRunError};
    use crate::puzzle::selection::{
        PuzzleSelection, PuzzleSelectionDataset, PuzzleSelectionFilter,
    };
    use crate::puzzle::session::{
        PuzzleSessionConfig, PuzzleSessionError, PuzzleSessionId, PuzzleSessionRevision,
    };
    use crate::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness};

    #[derive(Clone)]
    struct PuzzleRunBoundaryRandom;

    impl PuzzleRandomProvider for PuzzleRunBoundaryRandom {
        fn index(&mut self, _alternatives: NonZeroUsize) -> usize {
            0
        }
    }

    struct PuzzleRunBoundaryFixture;

    impl PuzzleRunBoundaryFixture {
        fn run() -> Result<PuzzleRun<PuzzleRunBoundaryRandom>, Box<dyn std::error::Error>> {
            let identity =
                PuzzleDefinitionIdentity::new("boundary".into(), "v1".into(), "a".into())?;
            let dataset = PuzzleSelectionDataset::from_identity(&identity);
            let position = BoardPosition::standard();
            let movement = position.parse_uci("e2e4")?;
            let definition = PuzzleDefinition::new(
                identity,
                position,
                vec![vec![movement]],
                PuzzleDefinitionMetadata::default(),
            )?;
            let selection = PuzzleSelection::new(
                dataset,
                vec![definition],
                PuzzleSelectionFilter::default(),
                PuzzleRunBoundaryRandom,
            )?;
            let mut run = PuzzleRun::new(
                PuzzleSessionId::new(NonZeroU64::MIN),
                PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
                selection,
            )?;
            run.update_readiness(
                &run.context(),
                ClockInstant::from_nanos(0),
                PuzzleTimerEnumReadiness::Ready,
            )?;
            Ok(run)
        }
    }

    #[test]
    fn revision_overflow_rejects_timed_actions_and_lifecycle_changes_atomically()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut run = PuzzleRunBoundaryFixture::run()?;
        run.session
            .set_revision(PuzzleSessionRevision::new(u64::MAX));
        let before = run.snapshot(ClockInstant::from_nanos(0))?;
        assert!(matches!(
            run.request_hint(&run.context(), ClockInstant::from_nanos(0)),
            Err(PuzzleRunError::Session(
                PuzzleSessionError::RevisionOverflow
            ))
        ));
        assert!(matches!(
            run.pause(
                &run.context(),
                ClockInstant::from_nanos(0),
                PuzzleTimerEnumPauseReason::Manual
            ),
            Err(PuzzleRunError::Session(
                PuzzleSessionError::RevisionOverflow
            ))
        ));
        assert_eq!(run.snapshot(ClockInstant::from_nanos(0))?, before);
        Ok(())
    }

    #[test]
    fn advance_counter_failures_do_not_finalize_or_consume_the_current_selection()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut run = PuzzleRunBoundaryFixture::run()?;
        run.session
            .set_revision(PuzzleSessionRevision::new(u64::MAX - 1));
        let before = run.snapshot(ClockInstant::from_nanos(0))?;
        assert!(matches!(
            run.skip(&run.context(), ClockInstant::from_nanos(0)),
            Err(PuzzleRunError::Session(
                PuzzleSessionError::RevisionOverflow
            ))
        ));
        assert_eq!(run.snapshot(ClockInstant::from_nanos(0))?, before);
        run.session.set_revision(PuzzleSessionRevision::ZERO);
        run.session.replace_attempt(
            PuzzleAttemptId::new(NonZeroU64::MAX),
            run.session.attempt().definition().clone(),
            PuzzleSessionRevision::ZERO,
        );
        let before = run.snapshot(ClockInstant::from_nanos(0))?;
        assert!(matches!(
            run.skip(&run.context(), ClockInstant::from_nanos(0)),
            Err(PuzzleRunError::AttemptIdOverflow)
        ));
        assert_eq!(run.snapshot(ClockInstant::from_nanos(0))?, before);
        Ok(())
    }
}
