use std::num::{NonZeroU64, NonZeroUsize};

use thiserror::Error;

use crate::board::r#move::BoardMove;
use crate::board::position::BoardPosition;
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumEvent, PuzzleEnumHint,
    PuzzleEnumHintStage, PuzzleEnumMoveFeedback, PuzzleEnumPhase, PuzzleEnumPractice,
    PuzzleEnumPrimaryOutcome,
};
use crate::puzzle::random_provider::PuzzleRandomProvider;
use crate::puzzle::solution::{PuzzleSolution, PuzzleSolutionError};
use crate::puzzle::tree::PuzzleDefinitionNodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PuzzleAttemptId(NonZeroU64);

impl PuzzleAttemptId {
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> NonZeroU64 {
        self.0
    }
}

#[derive(Debug, Error)]
pub enum PuzzleAttemptError {
    #[error(transparent)]
    Solution(#[from] PuzzleSolutionError),
    #[error("This puzzle operation is unavailable in phase {0:?}")]
    InvalidPhase(PuzzleEnumPhase),
    #[error("The random provider selected reply {choice}, but only {alternatives} are listed")]
    InvalidRandomChoice { choice: usize, alternatives: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleAttempt {
    id: PuzzleAttemptId,
    solution: PuzzleSolution,
    phase: PuzzleEnumPhase,
    primary_outcome: Option<PuzzleEnumPrimaryOutcome>,
    assistance: PuzzleEnumAssistance,
    hint_stage: PuzzleEnumHintStage,
    practice: PuzzleEnumPractice,
    events: Vec<PuzzleEnumEvent>,
}

impl PuzzleAttempt {
    pub(super) fn new(id: PuzzleAttemptId, definition: PuzzleDefinition) -> Self {
        Self {
            id,
            solution: PuzzleSolution::new(definition),
            phase: PuzzleEnumPhase::SolverMove,
            primary_outcome: None,
            assistance: PuzzleEnumAssistance::Unassisted,
            hint_stage: PuzzleEnumHintStage::Unrequested,
            practice: PuzzleEnumPractice::NotStarted,
            events: Vec::new(),
        }
    }

    pub const fn id(&self) -> PuzzleAttemptId {
        self.id
    }

    pub fn definition(&self) -> &PuzzleDefinition {
        self.solution.definition()
    }

    pub fn position(&self) -> &BoardPosition {
        self.solution.position()
    }

    pub fn cursor(&self) -> PuzzleDefinitionNodeId {
        self.solution.cursor()
    }

    pub const fn phase(&self) -> PuzzleEnumPhase {
        self.phase
    }

    pub const fn primary_outcome(&self) -> Option<PuzzleEnumPrimaryOutcome> {
        self.primary_outcome
    }

    pub const fn assistance(&self) -> PuzzleEnumAssistance {
        self.assistance
    }

    pub const fn hint_stage(&self) -> PuzzleEnumHintStage {
        self.hint_stage
    }

    pub const fn practice(&self) -> PuzzleEnumPractice {
        self.practice
    }

    pub fn events(&self) -> &[PuzzleEnumEvent] {
        &self.events
    }

    pub(super) fn submit_move(
        &mut self,
        movement: BoardMove,
    ) -> Result<PuzzleEnumMoveFeedback, PuzzleAttemptError> {
        self.require_phase(PuzzleEnumPhase::SolverMove)?;
        let Some(node) = self.solution.classify(movement)? else {
            self.primary_outcome
                .get_or_insert(PuzzleEnumPrimaryOutcome::Failed);
            self.phase = PuzzleEnumPhase::PracticeReady;
            self.events.push(PuzzleEnumEvent::LegalUnlisted(movement));
            return Ok(PuzzleEnumMoveFeedback::LegalUnlisted);
        };
        let phase = self.solution.advance(node)?;
        self.hint_stage = PuzzleEnumHintStage::Unrequested;
        self.events
            .push(PuzzleEnumEvent::SolverMove { movement, node });
        self.enter_phase(phase);
        Ok(if phase == PuzzleEnumPhase::Review {
            PuzzleEnumMoveFeedback::Completed
        } else {
            PuzzleEnumMoveFeedback::Accepted
        })
    }

    pub(super) fn play_opponent_reply<R: PuzzleRandomProvider + ?Sized>(
        &mut self,
        random: &mut R,
    ) -> Result<BoardMove, PuzzleAttemptError> {
        self.require_phase(PuzzleEnumPhase::OpponentReply)?;
        let children = self.solution.children()?;
        let alternatives =
            NonZeroUsize::new(children.len()).ok_or(PuzzleSolutionError::MissingContinuation)?;
        let choice = random.index(alternatives);
        let node = *children
            .get(choice)
            .ok_or(PuzzleAttemptError::InvalidRandomChoice {
                choice,
                alternatives: alternatives.get(),
            })?;
        let movement = self
            .solution
            .node(node)?
            .movement()
            .ok_or(PuzzleSolutionError::MissingMove)?;
        let phase = self.solution.advance(node)?;
        self.events.push(PuzzleEnumEvent::OpponentReply {
            movement,
            node,
            choice,
        });
        self.enter_phase(phase);
        Ok(movement)
    }

    pub(super) fn request_hint(&mut self) -> Result<PuzzleEnumHint, PuzzleAttemptError> {
        self.require_phase(PuzzleEnumPhase::SolverMove)?;
        let hint = self.solution.next_hint(self.hint_stage)?;
        self.assistance = PuzzleEnumAssistance::Assisted;
        self.hint_stage = hint.stage();
        self.events.push(PuzzleEnumEvent::Hint(hint));
        Ok(hint)
    }

    pub(super) fn retry_practice(&mut self) -> Result<(), PuzzleAttemptError> {
        self.require_phase(PuzzleEnumPhase::PracticeReady)?;
        self.phase = PuzzleEnumPhase::SolverMove;
        self.practice = PuzzleEnumPractice::InProgress;
        self.events.push(PuzzleEnumEvent::PracticeRetry);
        Ok(())
    }

    pub(super) fn skip(&mut self) -> Result<(), PuzzleAttemptError> {
        self.finalize(PuzzleEnumPrimaryOutcome::Skipped, PuzzleEnumEvent::Skipped)
    }

    pub(super) fn record_timeout(
        &mut self,
        mode: PuzzleEnumCountdownMode,
    ) -> Result<(), PuzzleAttemptError> {
        self.finalize(
            PuzzleEnumPrimaryOutcome::TimedOut,
            PuzzleEnumEvent::TimedOut(mode),
        )
    }

    fn require_phase(&self, expected: PuzzleEnumPhase) -> Result<(), PuzzleAttemptError> {
        if self.phase != expected {
            return Err(PuzzleAttemptError::InvalidPhase(self.phase));
        }
        Ok(())
    }

    fn enter_phase(&mut self, phase: PuzzleEnumPhase) {
        self.phase = phase;
        if phase == PuzzleEnumPhase::Review {
            self.primary_outcome
                .get_or_insert(PuzzleEnumPrimaryOutcome::Solved(self.assistance));
            if self.practice == PuzzleEnumPractice::InProgress {
                self.practice = PuzzleEnumPractice::Completed;
                self.events.push(PuzzleEnumEvent::PracticeCompleted);
            }
        }
    }

    fn finalize(
        &mut self,
        outcome: PuzzleEnumPrimaryOutcome,
        event: PuzzleEnumEvent,
    ) -> Result<(), PuzzleAttemptError> {
        if !self.phase.is_unfinished() {
            return Err(PuzzleAttemptError::InvalidPhase(self.phase));
        }
        self.primary_outcome.get_or_insert(outcome);
        self.phase = PuzzleEnumPhase::Finalized;
        self.events.push(event);
        Ok(())
    }
}
