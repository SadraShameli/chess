use thiserror::Error;

use crate::clock::duration::ClockInstant;
use crate::desktop::puzzle_snapshot::{DesktopPuzzleSnapshot, DesktopPuzzleSnapshotSave};
use crate::puzzle::checkpoint::{
    PuzzleCheckpointEnumAdvance, PuzzleCheckpointEnumFreezeCause, PuzzleCheckpointEnumStatus,
    PuzzleCheckpointEnumTrigger, PuzzleCheckpointError, PuzzleCheckpointFreeze,
    PuzzleCheckpointObservation, PuzzleCheckpointSnapshot,
};
use crate::puzzle::persistence::{
    PuzzlePersistenceEnumCommand, PuzzlePersistenceError, PuzzlePersistenceRun,
    PuzzlePersistenceStorageRevision,
};
use crate::puzzle::run::{PuzzleRunEnumStatus, PuzzleRunError};
use crate::puzzle::session::PuzzleSessionContext;
use crate::puzzle::storage_repository::{
    PuzzleStorageDriver, PuzzleStorageDriverError, PuzzleStorageRepository,
};
use crate::puzzle::summary::PuzzleSummary;
use crate::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness, PuzzleTimerView};

#[cfg(test)]
#[path = "../../tests/desktop/puzzle_commands.rs"]
mod tests;

#[derive(Debug, Error)]
pub enum DesktopPuzzleCommandsError<E: std::error::Error + 'static> {
    #[error("The puzzle action context {expected:?} does not match {actual:?}")]
    StaleContext {
        expected: Box<PuzzleSessionContext>,
        actual: Box<PuzzleSessionContext>,
    },
    #[error("Puzzle saving blocks this action: {0:?}")]
    SaveRequired(PuzzleCheckpointEnumStatus),
    #[error(transparent)]
    Persistence(#[from] PuzzlePersistenceError),
    #[error(transparent)]
    Run(#[from] PuzzleRunError),
    #[error(transparent)]
    Checkpoint(#[from] PuzzleCheckpointError),
    #[error(transparent)]
    Storage(#[from] PuzzleStorageDriverError<E>),
}

pub struct DesktopPuzzleCommands<R: PuzzleStorageRepository> {
    run: PuzzlePersistenceRun,
    storage: PuzzleStorageDriver<R>,
    timer: PuzzleTimerView,
    snapshot: Option<DesktopPuzzleSnapshot>,
}

impl<R: PuzzleStorageRepository> DesktopPuzzleCommands<R> {
    pub fn new(
        mut run: PuzzlePersistenceRun,
        repository: R,
        now: ClockInstant,
        storage_revision: PuzzlePersistenceStorageRevision,
        persisted_event_count: usize,
    ) -> Result<Self, DesktopPuzzleCommandsError<R::Error>> {
        let view = run.run().view(now)?;
        if view.status() == PuzzleRunEnumStatus::Active {
            return Err(PuzzleCheckpointError::ActiveInitialState.into());
        }
        let timer = view.timer();
        let payload = run.capture(now, persisted_event_count)?;
        let observation = PuzzleCheckpointObservation::from_run(run.run(), now)?;
        let storage = PuzzleStorageDriver::new(
            repository,
            PuzzleCheckpointSnapshot::new(observation, payload),
            storage_revision,
            persisted_event_count,
        )?;
        Ok(Self {
            run,
            storage,
            timer,
            snapshot: None,
        })
    }

    pub fn context(&self) -> PuzzleSessionContext {
        self.run.run().context()
    }

    pub fn next_deadline(
        &self,
    ) -> Result<Option<ClockInstant>, DesktopPuzzleCommandsError<R::Error>> {
        Ok(
            match (
                self.storage.checkpoint().next_deadline()?,
                self.timer.deadline(),
            ) {
                (Some(checkpoint), Some(expiry)) => Some(checkpoint.min(expiry)),
                (checkpoint, None) => checkpoint,
                (None, expiry) => expiry,
            },
        )
    }

    pub const fn has_retained_write(&self) -> bool {
        self.storage.has_retained_write()
    }

    pub fn execute(
        &mut self,
        expected: &PuzzleSessionContext,
        command: PuzzlePersistenceEnumCommand,
        now: ClockInstant,
    ) -> Result<(), DesktopPuzzleCommandsError<R::Error>> {
        self.guard(now)?;
        self.validate_context(expected)?;
        if matches!(command, PuzzlePersistenceEnumCommand::Poll) {
            self.poll_and_capture(now)?;
            return self.start_queued(now);
        }
        let gated = self.storage.checkpoint().status() != PuzzleCheckpointEnumStatus::Open;
        let resume = matches!(command, PuzzlePersistenceEnumCommand::Resume(_));
        let interruption = matches!(
            command,
            PuzzlePersistenceEnumCommand::Pause(_)
                | PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Hidden)
        );
        if gated && !resume && !interruption {
            self.start_queued(now)?;
            return Err(DesktopPuzzleCommandsError::SaveRequired(
                self.storage.checkpoint().status(),
            ));
        }
        if gated && resume {
            self.storage.checkpoint().ensure_can_resume(now)?;
        }
        self.run.execute_transition(command, now)?;
        if gated && resume {
            self.storage.resume(now)?;
        }
        self.capture_if_needed(now)?;
        self.start_queued(now)
    }

    pub fn snapshot(
        &mut self,
        now: ClockInstant,
    ) -> Result<DesktopPuzzleSnapshot, DesktopPuzzleCommandsError<R::Error>> {
        self.guard(now)?;
        self.poll_and_capture(now)?;
        self.start_queued(now)?;
        let checkpoint = self.storage.checkpoint();
        let snapshot = DesktopPuzzleSnapshot::capture(
            &self.run,
            now,
            DesktopPuzzleSnapshotSave {
                status: checkpoint.status(),
                durable: checkpoint.durable(),
                retained_write: self.has_retained_write(),
                unsaved_state: checkpoint.pending().is_some() || checkpoint.coalesced().is_some(),
            },
            self.snapshot.as_ref(),
        )?;
        self.snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }

    pub fn summary(
        &mut self,
        now: ClockInstant,
    ) -> Result<PuzzleSummary, DesktopPuzzleCommandsError<R::Error>> {
        self.guard(now)?;
        self.poll_and_capture(now)?;
        self.start_queued(now)?;
        Ok(self.run.run().summary(now)?)
    }

    pub fn begin_write(
        &mut self,
        now: ClockInstant,
    ) -> Result<(), DesktopPuzzleCommandsError<R::Error>> {
        self.guard(now)?;
        self.start_queued(now)
    }

    pub fn retry_save(
        &mut self,
        expected: &PuzzleSessionContext,
        now: ClockInstant,
    ) -> Result<(), DesktopPuzzleCommandsError<R::Error>> {
        self.guard(now)?;
        self.validate_context(expected)?;
        let result = self.storage.retry_save(now);
        self.guard(now)?;
        Ok(result?)
    }

    pub async fn wait(&mut self) -> Result<(), DesktopPuzzleCommandsError<R::Error>> {
        Ok(self.storage.wait().await?)
    }

    pub fn settle(
        &mut self,
        now: ClockInstant,
    ) -> Result<(), DesktopPuzzleCommandsError<R::Error>> {
        self.guard(now)?;
        let result = self.storage.settle(now);
        self.guard(now)?;
        result?;
        if let PuzzleCheckpointEnumStatus::AwaitingResume(freeze) =
            self.storage.checkpoint().status()
            && freeze.cause() == PuzzleCheckpointEnumFreezeCause::InitialSaveRequired
            && self.storage.checkpoint().observation().status()
                == PuzzleRunEnumStatus::AwaitingReadiness
        {
            self.storage.release_initial(now)?;
        }
        self.poll_and_capture(now)?;
        self.start_queued(now)
    }

    fn validate_context(
        &self,
        expected: &PuzzleSessionContext,
    ) -> Result<(), DesktopPuzzleCommandsError<R::Error>> {
        let actual = self.context();
        if *expected != actual {
            return Err(DesktopPuzzleCommandsError::StaleContext {
                expected: Box::new(expected.clone()),
                actual: Box::new(actual),
            });
        }
        Ok(())
    }

    fn guard(&mut self, now: ClockInstant) -> Result<(), DesktopPuzzleCommandsError<R::Error>> {
        if let PuzzleCheckpointEnumAdvance::FreezeRequired(freeze) =
            self.storage.before_advance(now)?
        {
            self.freeze(freeze, now)?;
        }
        Ok(())
    }

    fn freeze(
        &mut self,
        freeze: PuzzleCheckpointFreeze,
        now: ClockInstant,
    ) -> Result<(), DesktopPuzzleCommandsError<R::Error>> {
        match self.storage.checkpoint().observation().status() {
            PuzzleRunEnumStatus::Paused { .. } | PuzzleRunEnumStatus::Ended(_) => return Ok(()),
            PuzzleRunEnumStatus::AwaitingReadiness
                if freeze.cause() == PuzzleCheckpointEnumFreezeCause::InitialSaveRequired =>
            {
                return Ok(());
            }
            PuzzleRunEnumStatus::AwaitingReadiness
            | PuzzleRunEnumStatus::Active
            | PuzzleRunEnumStatus::Review
            | PuzzleRunEnumStatus::Exhausted
            | PuzzleRunEnumStatus::Expired(_) => {}
        }
        self.run.execute(
            PuzzlePersistenceEnumCommand::Pause(PuzzleTimerEnumPauseReason::SaveBackpressure),
            freeze.pause_at(),
        )?;
        self.capture_if_needed(now)
    }

    fn poll_and_capture(
        &mut self,
        now: ClockInstant,
    ) -> Result<(), DesktopPuzzleCommandsError<R::Error>> {
        if self.storage.checkpoint().durable().is_some()
            || self.storage.checkpoint().observation().status()
                != PuzzleRunEnumStatus::AwaitingReadiness
        {
            self.run.poll(now)?;
        }
        self.capture_if_needed(now)
    }

    fn capture_if_needed(
        &mut self,
        now: ClockInstant,
    ) -> Result<(), DesktopPuzzleCommandsError<R::Error>> {
        let observation = PuzzleCheckpointObservation::from_run(self.run.run(), now)?;
        self.timer = self.run.run().view(now)?.timer();
        let trigger = if observation.context() != self.storage.checkpoint().observation().context()
        {
            PuzzleCheckpointEnumTrigger::AcceptedTransition
        } else {
            PuzzleCheckpointEnumTrigger::Countdown
        };
        if self.storage.needs_snapshot(&observation, trigger)? {
            let payload = self
                .run
                .capture(now, self.storage.persisted_event_count())?;
            self.storage
                .record(PuzzleCheckpointSnapshot::new(observation, payload), trigger)?;
        }
        Ok(())
    }

    fn start_queued(
        &mut self,
        now: ClockInstant,
    ) -> Result<(), DesktopPuzzleCommandsError<R::Error>> {
        if self.storage.checkpoint().has_queued_write() && !self.has_retained_write() {
            let result = self.storage.begin_write(now);
            self.guard(now)?;
            result?;
        }
        Ok(())
    }
}
