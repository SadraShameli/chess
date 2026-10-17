mod active;
pub mod dispatch;
pub mod policy;

use std::collections::VecDeque;
use std::num::NonZeroUsize;

use thiserror::Error;

use crate::capacity::lease::CapacityLease;
use crate::engine::process_provider::EngineProcess;
use crate::engine::search::identity::EngineSearchTag;
use crate::engine::search::session::{
    EngineSearchSession, EngineSearchSessionError, EngineSearchSessionTerminal,
};
use crate::game::outcome::GameOutcome;
use crate::report::job::{
    ReportJob, ReportJobEnumFreshness, ReportJobEnumState, ReportJobError, ReportJobId,
};
use crate::report::scheduler::active::ReportSchedulerActive;
use crate::report::scheduler::dispatch::ReportSchedulerDispatch;
use crate::report::scheduler::policy::{
    ReportSchedulerDemand, ReportSchedulerEnumBlocker, ReportSchedulerEnumDecision,
};
use crate::report::source::ReportJobSource;

#[derive(Debug, Error)]
pub enum ReportSchedulerError {
    #[error("The report queue is full")]
    QueueFull,
    #[error("The report job {0:?} is already retained")]
    Duplicate(ReportJobId),
    #[error("The report job {0:?} is not retained")]
    UnknownJob(ReportJobId),
    #[error("The scheduler cannot dispatch from {0:?}")]
    NotReady(ReportSchedulerEnumDecision),
    #[error("The scheduler has no active report attempt")]
    NoAttempt,
    #[error("The active report dispatch has not yet been bound")]
    DispatchPending,
    #[error("The report job still owns unfinished work")]
    Unfinished,
    #[error("The actual CPU lease must match the recorded engine thread allocation")]
    AllocationMismatch,
    #[error(transparent)]
    Job(#[from] ReportJobError),
    #[error(transparent)]
    Session(#[from] EngineSearchSessionError),
}

#[derive(Debug, Error)]
#[error("{reason}")]
pub struct ReportSchedulerRejectedJob {
    #[source]
    reason: ReportSchedulerError,
    job: Box<ReportJob>,
}

impl ReportSchedulerRejectedJob {
    pub fn into_job(self) -> ReportJob {
        *self.job
    }

    pub const fn reason(&self) -> &ReportSchedulerError {
        &self.reason
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportSchedulerEnumSelection {
    Ready,
    Explicit,
}

#[derive(Debug)]
struct ReportSchedulerEntry {
    job: ReportJob,
    selection: ReportSchedulerEnumSelection,
}

impl ReportSchedulerEntry {
    fn is_ready(&self) -> bool {
        self.selection == ReportSchedulerEnumSelection::Ready
            && self.job.freshness() == ReportJobEnumFreshness::Current
            && matches!(
                self.job.state(),
                ReportJobEnumState::Queued
                    | ReportJobEnumState::Paused
                    | ReportJobEnumState::Running
            )
    }
}

pub struct ReportScheduler {
    jobs: VecDeque<ReportSchedulerEntry>,
    limit: NonZeroUsize,
    demand: ReportSchedulerDemand,
    owner: Option<ReportJobId>,
    active: Option<ReportSchedulerActive>,
}

impl ReportScheduler {
    pub fn new(limit: NonZeroUsize, demand: ReportSchedulerDemand) -> Self {
        Self {
            jobs: VecDeque::new(),
            limit,
            demand,
            owner: None,
            active: None,
        }
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    pub fn job(&self, id: ReportJobId) -> Result<&ReportJob, ReportSchedulerError> {
        Ok(&self.jobs[self.index(id)?].job)
    }

    pub fn enqueue(&mut self, job: ReportJob) -> Result<ReportJobId, ReportSchedulerRejectedJob> {
        let id = job.id();
        let refusal = if self.position(id).is_some() {
            Some(ReportSchedulerError::Duplicate(id))
        } else if self.jobs.len() == self.limit.get() {
            Some(ReportSchedulerError::QueueFull)
        } else if job.freshness() == ReportJobEnumFreshness::Outdated {
            Some(ReportJobError::Outdated.into())
        } else if !matches!(
            job.state(),
            ReportJobEnumState::Queued | ReportJobEnumState::Paused
        ) || job.active_search().is_some()
        {
            Some(ReportJobError::State(job.state()).into())
        } else {
            None
        };
        if let Some(reason) = refusal {
            return Err(ReportSchedulerRejectedJob {
                reason,
                job: Box::new(job),
            });
        }
        let selection = if job.state() == ReportJobEnumState::Queued {
            ReportSchedulerEnumSelection::Ready
        } else {
            ReportSchedulerEnumSelection::Explicit
        };
        self.jobs.push_back(ReportSchedulerEntry { job, selection });
        Ok(id)
    }

    pub fn select(&mut self, id: ReportJobId) -> Result<(), ReportSchedulerError> {
        let index = self.index(id)?;
        let job = &self.jobs[index].job;
        if job.freshness() == ReportJobEnumFreshness::Outdated {
            return Err(ReportJobError::Outdated.into());
        }
        if job.state() != ReportJobEnumState::Paused
            || self.owner == Some(id) && self.active.is_some()
        {
            return Err(ReportJobError::State(job.state()).into());
        }
        self.jobs[index].selection = ReportSchedulerEnumSelection::Ready;
        Ok(())
    }

    pub fn decision(&self) -> ReportSchedulerEnumDecision {
        if let Some(index) = self.owner_index() {
            let entry = &self.jobs[index];
            if let Some(active) = &self.active {
                return active.decision(&entry.job);
            }
        }
        if let Some(blocker) = self.demand.blocker() {
            return ReportSchedulerEnumDecision::Blocked(blocker);
        }
        self.next_index()
            .map_or(ReportSchedulerEnumDecision::Idle, |index| {
                ReportSchedulerEnumDecision::Ready(self.jobs[index].job.id())
            })
    }

    pub fn update_demand(
        &mut self,
        demand: ReportSchedulerDemand,
    ) -> Result<(), ReportSchedulerError> {
        self.demand = demand;
        if demand.blocker().is_some()
            && let Some(index) = self.owner_index()
        {
            let job = &mut self.jobs[index].job;
            if let Some(active) = &mut self.active {
                active.pause(job)?;
            } else if job.state() == ReportJobEnumState::Running {
                job.pause()?;
            }
        }
        Ok(())
    }

    pub fn workbench_admission(&self) -> Result<(), ReportSchedulerEnumBlocker> {
        if let Some(blocker) = self.demand.blocker() {
            return Err(blocker);
        }
        if self.active.is_some() || self.next_index().is_some() {
            return Err(ReportSchedulerEnumBlocker::Report);
        }
        Ok(())
    }

    pub fn prepare<P: EngineProcess>(
        &mut self,
        session: &mut EngineSearchSession<P>,
        lease: CapacityLease,
    ) -> Result<ReportSchedulerDispatch, ReportSchedulerError> {
        let decision = self.decision();
        let ReportSchedulerEnumDecision::Ready(id) = decision else {
            return Err(ReportSchedulerError::NotReady(decision));
        };
        let index = self.index(id)?;
        let job = &mut self.jobs[index].job;
        let engine = session.idle_engine()?;
        if !job.configuration().matches_engine(engine) {
            return Err(ReportJobError::SearchMismatch.into());
        }
        let threads = engine
            .resources()
            .ok_or(ReportSchedulerError::AllocationMismatch)?
            .threads();
        if usize::try_from(threads).ok() != Some(lease.workers()) {
            return Err(ReportSchedulerError::AllocationMismatch);
        }
        let position = job
            .results()
            .iter()
            .position(Option::is_none)
            .ok_or(ReportJobError::PositionIndex)?;
        let position = u32::try_from(position).map_err(|_error| ReportJobError::PositionIndex)?;
        if matches!(
            job.state(),
            ReportJobEnumState::Queued | ReportJobEnumState::Paused
        ) {
            job.resume()?;
        }
        let attempt = job.start_position(position)?;
        let dispatch = ReportSchedulerDispatch::new(&attempt, session, lease);
        self.owner = Some(id);
        self.active = Some(ReportSchedulerActive::new(attempt, session));
        Ok(dispatch)
    }

    pub fn bind<P: EngineProcess>(
        &mut self,
        session: &EngineSearchSession<P>,
    ) -> Result<EngineSearchTag, ReportSchedulerError> {
        let index = self.owner_index().ok_or(ReportSchedulerError::NoAttempt)?;
        let active = self
            .active
            .as_mut()
            .ok_or(ReportSchedulerError::NoAttempt)?;
        active.bind(&mut self.jobs[index].job, session)
    }

    pub fn record_terminal(
        &mut self,
        terminal: &EngineSearchSessionTerminal,
    ) -> Result<(), ReportSchedulerError> {
        let index = self.owner_index().ok_or(ReportSchedulerError::NoAttempt)?;
        let active = self
            .active
            .as_ref()
            .ok_or(ReportSchedulerError::NoAttempt)?;
        let entry = &mut self.jobs[index];
        entry.selection = active.record_terminal(&mut entry.job, terminal)?;
        self.active = None;
        self.release_finished_owner();
        Ok(())
    }

    pub fn record_outcome(
        &mut self,
        dispatch: &mut ReportSchedulerDispatch,
        outcome: GameOutcome,
    ) -> Result<(), ReportSchedulerError> {
        let index = self.owner_index().ok_or(ReportSchedulerError::NoAttempt)?;
        let active = self
            .active
            .as_ref()
            .ok_or(ReportSchedulerError::NoAttempt)?;
        active.record_outcome(&mut self.jobs[index].job, dispatch, outcome)?;
        self.active = None;
        self.release_finished_owner();
        Ok(())
    }

    pub fn settle_dispatch<P: EngineProcess>(
        &mut self,
        dispatch: &mut ReportSchedulerDispatch,
        session: &mut EngineSearchSession<P>,
    ) -> Result<Option<EngineSearchSessionTerminal>, ReportSchedulerError> {
        let index = self.owner_index().ok_or(ReportSchedulerError::NoAttempt)?;
        let active = self
            .active
            .as_mut()
            .ok_or(ReportSchedulerError::NoAttempt)?;
        let entry = &mut self.jobs[index];
        let terminal = active.settle_dispatch(&mut entry.job, dispatch, session)?;
        entry.selection = active.selection(terminal.as_ref());
        self.active = None;
        self.release_finished_owner();
        Ok(terminal)
    }

    pub fn observe_source(
        &mut self,
        id: ReportJobId,
        source: ReportJobSource,
    ) -> Result<(), ReportSchedulerError> {
        let index = self.index(id)?;
        let job = &mut self.jobs[index].job;
        if self.owner == Some(id)
            && let Some(active) = &mut self.active
        {
            active.observe_source(job, source)?;
        } else {
            job.observe_source(&source)?;
        }
        self.release_finished_owner();
        Ok(())
    }

    pub fn cancel(&mut self, id: ReportJobId) -> Result<(), ReportSchedulerError> {
        let index = self.index(id)?;
        let job = &mut self.jobs[index].job;
        if self.owner == Some(id)
            && let Some(active) = &mut self.active
        {
            active.cancel(job)?;
        } else {
            job.cancel()?;
        }
        self.release_finished_owner();
        Ok(())
    }

    pub fn take(&mut self, id: ReportJobId) -> Result<ReportJob, ReportSchedulerError> {
        let index = self.index(id)?;
        let job = &self.jobs[index].job;
        if self.owner == Some(id) && self.active.is_some()
            || !matches!(
                job.state(),
                ReportJobEnumState::Completed | ReportJobEnumState::Cancelled
            ) && job.freshness() != ReportJobEnumFreshness::Outdated
        {
            return Err(ReportSchedulerError::Unfinished);
        }
        let entry = self
            .jobs
            .remove(index)
            .ok_or(ReportSchedulerError::UnknownJob(id))?;
        if self.owner == Some(id) {
            self.owner = None;
        }
        Ok(entry.job)
    }

    fn index(&self, id: ReportJobId) -> Result<usize, ReportSchedulerError> {
        self.position(id)
            .ok_or(ReportSchedulerError::UnknownJob(id))
    }

    fn position(&self, id: ReportJobId) -> Option<usize> {
        self.jobs.iter().position(|entry| entry.job.id() == id)
    }

    fn owner_index(&self) -> Option<usize> {
        self.owner.and_then(|id| self.position(id))
    }

    fn next_index(&self) -> Option<usize> {
        if let Some(index) = self.owner_index()
            && self.jobs[index].is_ready()
        {
            return Some(index);
        }
        self.jobs.iter().position(ReportSchedulerEntry::is_ready)
    }

    fn release_finished_owner(&mut self) {
        if self.active.is_none()
            && let Some(index) = self.owner_index()
        {
            let job = &self.jobs[index].job;
            if job.freshness() == ReportJobEnumFreshness::Outdated
                || matches!(
                    job.state(),
                    ReportJobEnumState::Completed | ReportJobEnumState::Cancelled
                )
            {
                self.owner = None;
            }
        }
    }
}
