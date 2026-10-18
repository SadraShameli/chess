use crate::capacity::lease::CapacityLease;
use crate::engine::process_provider::EngineProcess;
use crate::engine::search::identity::{EngineSearchGeneration, EngineSearchTag};
use crate::engine::search::session::{EngineSearchSession, EngineSearchSessionRequest};
use crate::report::job::{ReportJobAttempt, ReportJobError, ReportJobId};
use crate::report::scheduler::ReportSchedulerError;

#[must_use]
pub struct ReportSchedulerDispatch {
    job: ReportJobId,
    index: u32,
    sequence: u64,
    generation: EngineSearchGeneration,
    request: Option<EngineSearchSessionRequest>,
}

impl ReportSchedulerDispatch {
    pub(super) fn new<P: EngineProcess>(
        attempt: &ReportJobAttempt,
        session: &EngineSearchSession<P>,
        lease: CapacityLease,
    ) -> Self {
        Self {
            job: attempt.job(),
            index: attempt.index(),
            sequence: attempt.sequence(),
            generation: session.engine().generation(),
            request: Some(EngineSearchSessionRequest::new(
                attempt.context(),
                attempt.position().clone(),
                attempt.limit(),
                lease,
            )),
        }
    }

    pub async fn start<P: EngineProcess>(
        &mut self,
        session: &mut EngineSearchSession<P>,
    ) -> Result<EngineSearchTag, ReportSchedulerError> {
        if session.engine().generation() != self.generation {
            return Err(ReportJobError::SearchMismatch.into());
        }
        let request = self.request.take().ok_or(ReportJobError::StaleAttempt)?;
        Ok(session.start(request).await?)
    }

    pub(super) fn matches(
        &self,
        attempt: &ReportJobAttempt,
        generation: EngineSearchGeneration,
    ) -> bool {
        self.job == attempt.job()
            && self.index == attempt.index()
            && self.sequence == attempt.sequence()
            && self.generation == generation
    }

    pub(super) fn withdraw_request(&mut self) {
        self.request = None;
    }

    pub(super) fn is_unstarted(&self) -> bool {
        self.request.is_some()
    }
}
