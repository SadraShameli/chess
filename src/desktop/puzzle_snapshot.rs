use std::sync::Arc;

use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::desktop::dto::DesktopBoardSnapshot;
use crate::puzzle::attempt::PuzzleAttempt;
use crate::puzzle::checkpoint::{PuzzleCheckpointEnumStatus, PuzzleCheckpointObservation};
use crate::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumEvent, PuzzleEnumHint, PuzzleEnumHintStage, PuzzleEnumPhase,
    PuzzleEnumPractice, PuzzleEnumPrimaryOutcome,
};
use crate::puzzle::persistence::PuzzlePersistenceRun;
use crate::puzzle::run::{PuzzleRunEnumContent, PuzzleRunEnumStatus, PuzzleRunError};
use crate::puzzle::session::{PuzzleSessionConfig, PuzzleSessionContext};
use crate::puzzle::timer::PuzzleTimerView;

#[derive(Debug, Clone)]
pub struct DesktopPuzzleSnapshotAttempt {
    board: DesktopBoardSnapshot,
    phase: PuzzleEnumPhase,
    primary_outcome: Option<PuzzleEnumPrimaryOutcome>,
    assistance: PuzzleEnumAssistance,
    practice: PuzzleEnumPractice,
    hint_stage: PuzzleEnumHintStage,
    hint: Option<PuzzleEnumHint>,
    last_event: Option<PuzzleEnumEvent>,
}

impl DesktopPuzzleSnapshotAttempt {
    fn capture(attempt: &PuzzleAttempt) -> Self {
        let hint = if attempt.hint_stage() == PuzzleEnumHintStage::Unrequested {
            None
        } else {
            attempt.events().iter().rev().find_map(|event| match event {
                PuzzleEnumEvent::Hint(hint) => Some(*hint),
                PuzzleEnumEvent::SolverMove { .. }
                | PuzzleEnumEvent::OpponentReply { .. }
                | PuzzleEnumEvent::LegalUnlisted(_)
                | PuzzleEnumEvent::PracticeRetry
                | PuzzleEnumEvent::PracticeCompleted
                | PuzzleEnumEvent::Skipped
                | PuzzleEnumEvent::TimedOut(_) => None,
            })
        };
        Self {
            board: DesktopBoardSnapshot::from_position(attempt.position()),
            phase: attempt.phase(),
            primary_outcome: attempt.primary_outcome(),
            assistance: attempt.assistance(),
            practice: attempt.practice(),
            hint_stage: attempt.hint_stage(),
            hint,
            last_event: attempt.events().last().copied(),
        }
    }

    pub const fn board(&self) -> &DesktopBoardSnapshot {
        &self.board
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

    pub const fn practice(&self) -> PuzzleEnumPractice {
        self.practice
    }

    pub const fn hint_stage(&self) -> PuzzleEnumHintStage {
        self.hint_stage
    }

    pub const fn hint(&self) -> Option<PuzzleEnumHint> {
        self.hint
    }

    pub const fn last_event(&self) -> Option<PuzzleEnumEvent> {
        self.last_event
    }
}

#[derive(Debug, Clone)]
pub struct DesktopPuzzleSnapshot {
    context: PuzzleSessionContext,
    captured_at: ClockInstant,
    status: PuzzleRunEnumStatus,
    config: PuzzleSessionConfig,
    timer: PuzzleTimerView,
    attempt: Option<Arc<DesktopPuzzleSnapshotAttempt>>,
    archived_attempts: usize,
    checkpoint_status: PuzzleCheckpointEnumStatus,
    durable: Option<PuzzleCheckpointObservation>,
    retained_write: bool,
    unsaved_state: bool,
}

pub(crate) struct DesktopPuzzleSnapshotSave<'a> {
    pub(crate) status: PuzzleCheckpointEnumStatus,
    pub(crate) durable: Option<&'a PuzzleCheckpointObservation>,
    pub(crate) retained_write: bool,
    pub(crate) unsaved_state: bool,
}

impl DesktopPuzzleSnapshot {
    pub(crate) fn capture(
        owner: &PuzzlePersistenceRun,
        now: ClockInstant,
        save: DesktopPuzzleSnapshotSave<'_>,
        previous: Option<&Self>,
    ) -> Result<Self, PuzzleRunError> {
        let run = owner.run();
        let context = run.context();
        let view = run.view(now)?;
        let attempt = if let Some(previous) = previous
            && previous.context == context
            && previous.status == view.status()
        {
            previous.attempt.as_ref().map(Arc::clone)
        } else {
            match view.content() {
                PuzzleRunEnumContent::Board(attempt) | PuzzleRunEnumContent::Review(attempt) => {
                    Some(Arc::new(DesktopPuzzleSnapshotAttempt::capture(attempt)))
                }
                PuzzleRunEnumContent::Hidden => None,
            }
        };
        Ok(Self {
            context,
            captured_at: now,
            status: view.status(),
            config: owner.config(),
            timer: view.timer(),
            attempt,
            archived_attempts: run.previous_attempts().len(),
            checkpoint_status: save.status,
            durable: save.durable.cloned(),
            retained_write: save.retained_write,
            unsaved_state: save.unsaved_state,
        })
    }

    pub const fn context(&self) -> &PuzzleSessionContext {
        &self.context
    }

    pub const fn captured_at(&self) -> ClockInstant {
        self.captured_at
    }

    pub const fn status(&self) -> PuzzleRunEnumStatus {
        self.status
    }

    pub const fn config(&self) -> PuzzleSessionConfig {
        self.config
    }

    pub const fn timer(&self) -> PuzzleTimerView {
        self.timer
    }

    pub fn attempt(&self) -> Option<&DesktopPuzzleSnapshotAttempt> {
        self.attempt.as_deref()
    }

    pub const fn archived_attempts(&self) -> usize {
        self.archived_attempts
    }

    pub const fn checkpoint_status(&self) -> PuzzleCheckpointEnumStatus {
        self.checkpoint_status
    }

    pub fn durable_context(&self) -> Option<&PuzzleSessionContext> {
        self.durable
            .as_ref()
            .map(PuzzleCheckpointObservation::context)
    }

    pub fn durable_elapsed(&self) -> Option<ClockDuration> {
        self.durable
            .as_ref()
            .map(PuzzleCheckpointObservation::elapsed_active)
    }

    pub const fn has_retained_write(&self) -> bool {
        self.retained_write
    }

    pub const fn has_unsaved_state(&self) -> bool {
        self.unsaved_state
    }
}
