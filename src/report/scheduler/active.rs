use crate::engine::process_provider::EngineProcess;
use crate::engine::search::enums::EngineSearchEnumState;
use crate::engine::search::identity::{EngineSearchGeneration, EngineSearchTag};
use crate::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumCancellation, EngineSearchSessionEnumCompletion,
    EngineSearchSessionTerminal,
};
use crate::game::outcome::GameOutcome;
use crate::report::job::{
    ReportJob, ReportJobAttempt, ReportJobEnumState, ReportJobError, ReportJobSearchBinding,
};
use crate::report::scheduler::dispatch::ReportSchedulerDispatch;
use crate::report::scheduler::policy::ReportSchedulerEnumDecision;
use crate::report::scheduler::{ReportSchedulerEnumSelection, ReportSchedulerError};
use crate::report::source::ReportJobSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportSchedulerActiveEnumInterruption {
    None,
    Pause,
    Cancel,
}

#[derive(Debug)]
pub(super) struct ReportSchedulerActive {
    attempt: ReportJobAttempt,
    generation: EngineSearchGeneration,
    previous_tag: Option<EngineSearchTag>,
    binding: Option<ReportJobSearchBinding>,
    interruption: ReportSchedulerActiveEnumInterruption,
    changed_source: Option<ReportJobSource>,
}

impl ReportSchedulerActive {
    pub(super) fn new<P: EngineProcess>(
        attempt: ReportJobAttempt,
        session: &EngineSearchSession<P>,
    ) -> Self {
        Self {
            attempt,
            generation: session.engine().generation(),
            previous_tag: session.metadata().map(|metadata| metadata.tag()),
            binding: None,
            interruption: ReportSchedulerActiveEnumInterruption::None,
            changed_source: None,
        }
    }

    pub(super) fn decision(&self, job: &ReportJob) -> ReportSchedulerEnumDecision {
        match &self.binding {
            Some(binding) => {
                if self.interruption != ReportSchedulerActiveEnumInterruption::None
                    || job.state() != ReportJobEnumState::Running
                {
                    ReportSchedulerEnumDecision::Stop(binding.tag())
                } else {
                    ReportSchedulerEnumDecision::AwaitingTerminal(binding.tag())
                }
            }
            None => ReportSchedulerEnumDecision::Dispatching(self.attempt.job()),
        }
    }

    pub(super) fn bind<P: EngineProcess>(
        &mut self,
        job: &mut ReportJob,
        session: &EngineSearchSession<P>,
    ) -> Result<EngineSearchTag, ReportSchedulerError> {
        if self.binding.is_some() || session.engine().generation() != self.generation {
            return Err(ReportJobError::StaleAttempt.into());
        }
        let binding = job.bind_engine(&self.attempt, session)?;
        let tag = binding.tag();
        self.binding = Some(binding);
        self.apply_interruption(job)?;
        Ok(tag)
    }

    pub(super) fn pause(&mut self, job: &mut ReportJob) -> Result<(), ReportSchedulerError> {
        if self.interruption == ReportSchedulerActiveEnumInterruption::None {
            self.interruption = ReportSchedulerActiveEnumInterruption::Pause;
        }
        if self.binding.is_some() {
            self.apply_interruption(job)?;
        }
        Ok(())
    }

    pub(super) fn cancel(&mut self, job: &mut ReportJob) -> Result<(), ReportSchedulerError> {
        self.interruption = ReportSchedulerActiveEnumInterruption::Cancel;
        if self.binding.is_some() {
            self.apply_interruption(job)?;
        }
        Ok(())
    }

    pub(super) fn observe_source(
        &mut self,
        job: &mut ReportJob,
        source: ReportJobSource,
    ) -> Result<(), ReportSchedulerError> {
        if job.source().game() != source.game() {
            return Err(ReportJobError::WrongGame.into());
        }
        if self.binding.is_none() && !job.source().is_same_line(&source) {
            if self.changed_source.is_none() {
                self.changed_source = Some(source);
            }
            self.pause(job)
        } else {
            Ok(job.observe_source(&source)?)
        }
    }

    pub(super) fn record_terminal(
        &self,
        job: &mut ReportJob,
        terminal: &EngineSearchSessionTerminal,
    ) -> Result<ReportSchedulerEnumSelection, ReportSchedulerError> {
        let binding = self
            .binding
            .as_ref()
            .ok_or(ReportSchedulerError::DispatchPending)?;
        job.record_terminal(binding, terminal)?;
        Ok(self.selection(Some(terminal)))
    }

    pub(super) fn record_outcome(
        &self,
        job: &mut ReportJob,
        dispatch: &mut ReportSchedulerDispatch,
        outcome: GameOutcome,
    ) -> Result<(), ReportSchedulerError> {
        if self.binding.is_some()
            || self.interruption != ReportSchedulerActiveEnumInterruption::None
            || self.changed_source.is_some()
            || !dispatch.matches(&self.attempt, self.generation)
            || !dispatch.is_unstarted()
        {
            return Err(ReportJobError::StaleAttempt.into());
        }
        job.record_outcome(&self.attempt, outcome)?;
        dispatch.withdraw_request();
        Ok(())
    }

    pub(super) fn settle_dispatch<P: EngineProcess>(
        &mut self,
        job: &mut ReportJob,
        dispatch: &mut ReportSchedulerDispatch,
        session: &mut EngineSearchSession<P>,
    ) -> Result<Option<EngineSearchSessionTerminal>, ReportSchedulerError> {
        if self.binding.is_some()
            || session.engine().generation() != self.generation
            || !dispatch.matches(&self.attempt, self.generation)
        {
            return Err(ReportJobError::StaleAttempt.into());
        }
        dispatch.withdraw_request();
        if session.allocation().is_some() {
            return Err(ReportJobError::SearchStillOwned.into());
        }
        let terminal = if session.metadata().map(|metadata| metadata.tag()) == self.previous_tag {
            session.idle_engine()?;
            None
        } else {
            let terminal = session.terminal().ok_or(ReportJobError::SearchStillOwned)?;
            let metadata = terminal.snapshot().metadata();
            if metadata.tag().generation() != self.generation
                || metadata.tag().context() != self.attempt.context()
                || metadata.limit() != self.attempt.limit()
                || metadata.position() != self.attempt.position()
                || metadata.position().initial().fen() != self.attempt.position().initial().fen()
                || metadata.position().current().fen() != self.attempt.position().current().fen()
            {
                return Err(ReportJobError::SearchMismatch.into());
            }
            if session.engine().state() != EngineSearchEnumState::Idle
                && !session.engine().is_shutdown_complete()
            {
                return Err(ReportJobError::SearchStillOwned.into());
            }
            Some(terminal.clone())
        };
        job.pause()?;
        self.apply_interruption(job)?;
        Ok(terminal)
    }

    pub(super) fn selection(
        &self,
        terminal: Option<&EngineSearchSessionTerminal>,
    ) -> ReportSchedulerEnumSelection {
        if terminal.is_some_and(|terminal| {
            matches!(
                terminal.completion(),
                EngineSearchSessionEnumCompletion::Failed(_)
            )
        }) {
            return ReportSchedulerEnumSelection::Explicit;
        }
        if self.interruption == ReportSchedulerActiveEnumInterruption::Pause
            || self.binding.is_some()
                && terminal.is_some_and(|terminal| {
                    matches!(
                        terminal.completion(),
                        EngineSearchSessionEnumCompletion::BestMove(_)
                    ) && terminal.snapshot().cancellation()
                        == EngineSearchSessionEnumCancellation::NotRequested
                        && self.interruption == ReportSchedulerActiveEnumInterruption::None
                })
        {
            ReportSchedulerEnumSelection::Ready
        } else {
            ReportSchedulerEnumSelection::Explicit
        }
    }

    fn apply_interruption(&mut self, job: &mut ReportJob) -> Result<(), ReportSchedulerError> {
        if let Some(source) = &self.changed_source {
            job.observe_source(source)?;
            self.changed_source = None;
        }
        match self.interruption {
            ReportSchedulerActiveEnumInterruption::None => {}
            ReportSchedulerActiveEnumInterruption::Pause => {
                if matches!(
                    job.state(),
                    ReportJobEnumState::Queued | ReportJobEnumState::Running
                ) {
                    job.pause()?;
                }
            }
            ReportSchedulerActiveEnumInterruption::Cancel => {
                if job.state() != ReportJobEnumState::Cancelling {
                    job.cancel()?;
                }
            }
        }
        Ok(())
    }
}
