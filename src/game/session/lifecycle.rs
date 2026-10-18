use crate::clock::duration::ClockInstant;
use crate::clock::state::{ClockState, ClockStateEnumStatus, ClockStateSnapshot, ClockStateUpdate};
use crate::game::adjudication::GameAdjudicationMateProofProvider;
use crate::game::enums::{
    GameEnumPhase, GameEnumTransitionKind, GameId, GameMoveId, GameRevision, GameRevisionError,
};
use crate::game::outcome::GameOutcome;
use crate::game::pause::{GamePauseEnumCause, GamePauseRequest};
use crate::game::save::GameSaveEnumStatus;
use crate::game::session::{
    GameSession, GameSessionClockPoll, GameSessionEnumInterruption, GameSessionEnumState,
    GameSessionError, GameSessionTransition,
};

pub enum GameSessionLifecycleEnumRequest {
    Pause(GamePauseRequest),
    PollDeadline {
        expected_revision: GameRevision,
        received_at: ClockInstant,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSessionLifecycleEnumContentChange {
    Checkpoint,
    Result,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSessionLifecycleEnumCommit {
    Accept,
    DeferResult,
}

#[derive(Debug)]
struct GameSessionLifecycleContext {
    game: GameId,
    revision: GameRevision,
    live: GameMoveId,
    state: GameSessionEnumState,
    clock: ClockState,
}

impl GameSessionLifecycleContext {
    fn capture(session: &GameSession) -> Self {
        Self {
            game: session.id,
            revision: session.revision,
            live: session.live,
            state: session.state,
            clock: session.clock.clone(),
        }
    }

    fn validate(&self, session: &GameSession) -> Result<(), GameSessionError> {
        if self.game != session.id {
            return Err(GameSessionError::ForeignLifecyclePreparation {
                expected: session.id,
                received: self.game,
            });
        }
        session.revision.check_expected(self.revision)?;
        if self.live != session.live || self.state != session.state || self.clock != session.clock {
            return Err(GameRevisionError::Stale {
                expected: self.revision,
                actual: session.revision,
            }
            .into());
        }
        Ok(())
    }
}

#[derive(Debug)]
struct GameSessionLifecycleCandidate {
    clock: ClockState,
    update: ClockStateUpdate,
    state: GameSessionEnumState,
    revision: GameRevision,
    kind: Option<GameEnumTransitionKind>,
    received_at: ClockInstant,
}

#[derive(Debug)]
pub struct GameSessionLifecyclePrepared {
    context: GameSessionLifecycleContext,
    candidate: GameSessionLifecycleCandidate,
    snapshot: ClockStateSnapshot,
    content_change: GameSessionLifecycleEnumContentChange,
}

impl GameSessionLifecyclePrepared {
    fn new(
        session: &GameSession,
        candidate: GameSessionLifecycleCandidate,
    ) -> Result<Self, GameSessionError> {
        let snapshot = candidate.clock.snapshot(candidate.received_at)?;
        let content_change = if session.outcome() == candidate.state.outcome() {
            GameSessionLifecycleEnumContentChange::Checkpoint
        } else {
            GameSessionLifecycleEnumContentChange::Result
        };
        Ok(Self {
            context: GameSessionLifecycleContext::capture(session),
            candidate,
            snapshot,
            content_change,
        })
    }

    pub const fn content_change(&self) -> GameSessionLifecycleEnumContentChange {
        self.content_change
    }

    fn validate(
        &self,
        session: &GameSession,
        mode: GameSessionLifecycleEnumCommit,
    ) -> Result<(), GameSessionError> {
        self.context.validate(session)?;
        if mode == GameSessionLifecycleEnumCommit::DeferResult
            && (self.content_change != GameSessionLifecycleEnumContentChange::Result
                || !matches!(self.candidate.state, GameSessionEnumState::Completed(_))
                || self.context.state.interruption()
                    == Some(GameSessionEnumInterruption::PendingResult))
        {
            return Err(GameSessionError::LifecycleResultRequired);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct GameSessionLifecyclePendingResult {
    context: GameSessionLifecycleContext,
    outcome: GameOutcome,
    observed_at: ClockInstant,
}

impl GameSessionLifecyclePendingResult {
    pub const fn game(&self) -> GameId {
        self.context.game
    }

    pub const fn revision(&self) -> GameRevision {
        self.context.revision
    }

    pub const fn live(&self) -> GameMoveId {
        self.context.live
    }

    pub const fn observed_at(&self) -> ClockInstant {
        self.observed_at
    }
}

#[derive(Debug)]
pub struct GameSessionLifecycleCommitted {
    transition: Option<GameSessionTransition>,
    update: ClockStateUpdate,
    pending_result: Option<GameSessionLifecyclePendingResult>,
}

impl GameSessionLifecycleCommitted {
    pub fn transition(&self) -> Option<&GameSessionTransition> {
        self.transition.as_ref()
    }

    pub const fn update(&self) -> &ClockStateUpdate {
        &self.update
    }

    pub fn pending_result(&self) -> Option<&GameSessionLifecyclePendingResult> {
        self.pending_result.as_ref()
    }

    pub fn into_parts(
        self,
    ) -> (
        Option<GameSessionTransition>,
        ClockStateUpdate,
        Option<GameSessionLifecyclePendingResult>,
    ) {
        (self.transition, self.update, self.pending_result)
    }
}

struct GameSessionLifecycleAccepted {
    transition: GameSessionTransition,
    update: ClockStateUpdate,
    pending_result: Option<GameSessionLifecyclePendingResult>,
}

impl GameSessionLifecycleAccepted {
    fn into_committed(self) -> GameSessionLifecycleCommitted {
        GameSessionLifecycleCommitted {
            transition: Some(self.transition),
            update: self.update,
            pending_result: self.pending_result,
        }
    }
}

impl GameSession {
    pub fn prepare_lifecycle<P: GameAdjudicationMateProofProvider + ?Sized>(
        &self,
        request: GameSessionLifecycleEnumRequest,
        proof: &P,
    ) -> Result<GameSessionLifecyclePrepared, GameSessionError> {
        let mut candidate = match request {
            GameSessionLifecycleEnumRequest::Pause(request) => {
                self.revision.check_expected(request.expected_revision)?;
                if self.phase() != GameEnumPhase::Active
                    && self.interruption() != Some(GameSessionEnumInterruption::PendingResult)
                {
                    return Err(GameSessionError::WrongPhase(self.phase()));
                }
                request.engine.validate(
                    self.id,
                    self.revision,
                    self.seats.mode(),
                    Some(request.cause),
                )?;
                if self.interruption() == Some(GameSessionEnumInterruption::PendingResult) {
                    return self.prepare_stopped_lifecycle(
                        request.received_at,
                        self.revision,
                        self.state,
                        Some(GameEnumTransitionKind::Interrupted),
                    );
                }
                let revision = self.revision.next()?;
                let mut clock = self.clock.clone();
                let update = clock.pause(request.received_at)?;
                let interruption = match request.cause {
                    GamePauseEnumCause::Fault(fault) => {
                        GameSessionEnumInterruption::RecoveryRequired(fault)
                    }
                    cause @ (GamePauseEnumCause::Manual
                    | GamePauseEnumCause::ComputerSleep
                    | GamePauseEnumCause::Relaunch
                    | GamePauseEnumCause::TabClosed
                    | GamePauseEnumCause::ApplicationQuit
                    | GamePauseEnumCause::UpdateInstall
                    | GamePauseEnumCause::AutosaveFailure) => {
                        GameSessionEnumInterruption::Paused(cause)
                    }
                };
                GameSessionLifecycleCandidate {
                    clock,
                    update,
                    state: GameSessionEnumState::Interrupted(interruption),
                    revision,
                    kind: Some(GameEnumTransitionKind::Interrupted),
                    received_at: request.received_at,
                }
            }
            GameSessionLifecycleEnumRequest::PollDeadline {
                expected_revision,
                received_at,
            } => {
                self.revision.check_expected(expected_revision)?;
                if self.interruption() == Some(GameSessionEnumInterruption::PendingResult) {
                    return self.prepare_stopped_lifecycle(
                        received_at,
                        self.revision,
                        self.state,
                        None,
                    );
                }
                if self.phase() != GameEnumPhase::Active {
                    return Err(GameSessionError::WrongPhase(self.phase()));
                }
                let mut clock = self.clock.clone();
                let update = clock.poll(received_at)?;
                if update.view().status() == ClockStateEnumStatus::Paused {
                    return Err(GameSessionError::WrongPhase(self.phase()));
                }
                GameSessionLifecycleCandidate {
                    clock,
                    update,
                    state: self.state,
                    revision: self.revision,
                    kind: None,
                    received_at,
                }
            }
        };
        if let ClockStateEnumStatus::Flagged(flagged) = candidate.update.view().status() {
            if candidate.kind.is_none() {
                candidate.revision = self.revision.next()?;
            }
            candidate.kind = Some(GameEnumTransitionKind::FlagFall);
            candidate.state = GameSessionEnumState::from_decision(
                self.history.flag_fall(flagged, proof),
                self.live,
            )?;
        }
        GameSessionLifecyclePrepared::new(self, candidate)
    }

    pub fn commit_lifecycle(
        &mut self,
        prepared: GameSessionLifecyclePrepared,
        mode: GameSessionLifecycleEnumCommit,
    ) -> Result<GameSessionLifecycleCommitted, GameSessionError> {
        if prepared.candidate.kind.is_some() {
            return Ok(self
                .commit_lifecycle_transition(prepared, mode)?
                .into_committed());
        }
        prepared.validate(self, mode)?;
        self.clock = prepared.candidate.clock;
        Ok(GameSessionLifecycleCommitted {
            transition: None,
            update: prepared.candidate.update,
            pending_result: None,
        })
    }

    fn commit_lifecycle_transition(
        &mut self,
        prepared: GameSessionLifecyclePrepared,
        mode: GameSessionLifecycleEnumCommit,
    ) -> Result<GameSessionLifecycleAccepted, GameSessionError> {
        prepared.validate(self, mode)?;
        let mut kind = prepared
            .candidate
            .kind
            .ok_or(GameSessionError::WrongPhase(self.phase()))?;
        let mut state = prepared.candidate.state;
        let pending_result = match mode {
            GameSessionLifecycleEnumCommit::Accept => None,
            GameSessionLifecycleEnumCommit::DeferResult => {
                let GameSessionEnumState::Completed(outcome) = state else {
                    return Err(GameSessionError::LifecycleResultRequired);
                };
                state =
                    GameSessionEnumState::Interrupted(GameSessionEnumInterruption::PendingResult);
                kind = GameEnumTransitionKind::Interrupted;
                Some(GameSessionLifecyclePendingResult {
                    context: GameSessionLifecycleContext {
                        game: self.id,
                        revision: prepared.candidate.revision,
                        live: self.live,
                        state,
                        clock: prepared.candidate.clock.clone(),
                    },
                    outcome,
                    observed_at: prepared.candidate.received_at,
                })
            }
        };
        let transition =
            self.transition(prepared.candidate.revision, kind, &state, prepared.snapshot);
        let transition = self.accept_state(transition, prepared.candidate.clock, state);
        if pending_result.is_some() {
            self.save_status = GameSaveEnumStatus::Required;
        }
        Ok(GameSessionLifecycleAccepted {
            transition,
            update: prepared.candidate.update,
            pending_result,
        })
    }

    pub fn prepare_pending_result(
        &self,
        pending: &GameSessionLifecyclePendingResult,
        received_at: ClockInstant,
    ) -> Result<GameSessionLifecyclePrepared, GameSessionError> {
        pending.context.validate(self)?;
        if self.interruption() != Some(GameSessionEnumInterruption::PendingResult) {
            return Err(GameSessionError::PendingResult);
        }
        let revision = self.revision.next()?;
        self.prepare_stopped_lifecycle(
            received_at,
            revision,
            GameSessionEnumState::Completed(pending.outcome),
            Some(GameEnumTransitionKind::ResultAccepted),
        )
    }

    fn prepare_stopped_lifecycle(
        &self,
        received_at: ClockInstant,
        revision: GameRevision,
        state: GameSessionEnumState,
        kind: Option<GameEnumTransitionKind>,
    ) -> Result<GameSessionLifecyclePrepared, GameSessionError> {
        let mut observation = self.clock.clone();
        let update = observation.poll(received_at)?;
        GameSessionLifecyclePrepared::new(
            self,
            GameSessionLifecycleCandidate {
                clock: self.clock.clone(),
                update,
                state,
                revision,
                kind,
                received_at,
            },
        )
    }

    pub fn pause<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        request: GamePauseRequest,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        let prepared =
            self.prepare_lifecycle(GameSessionLifecycleEnumRequest::Pause(request), proof)?;
        Ok(self
            .commit_lifecycle_transition(prepared, GameSessionLifecycleEnumCommit::Accept)?
            .transition)
    }

    pub fn poll_deadline<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        expected_revision: GameRevision,
        now: ClockInstant,
        proof: &P,
    ) -> Result<GameSessionClockPoll, GameSessionError> {
        let prepared = self.prepare_lifecycle(
            GameSessionLifecycleEnumRequest::PollDeadline {
                expected_revision,
                received_at: now,
            },
            proof,
        )?;
        let committed = self.commit_lifecycle(prepared, GameSessionLifecycleEnumCommit::Accept)?;
        Ok(GameSessionClockPoll {
            update: committed.update,
            transition: committed.transition,
        })
    }
}
