use thiserror::Error;

use crate::game::enums::GameId;
use crate::report::job::{ReportJob, ReportJobEnumFreshness, ReportJobEnumState, ReportJobId};
use crate::report::scheduler::{ReportScheduler, ReportSchedulerError, ReportSchedulerRejectedJob};
use crate::report::source::{ReportJobSource, ReportJobSourceError};

#[derive(Debug, Error)]
pub enum ReportServiceError {
    #[error(transparent)]
    Scheduler(#[from] ReportSchedulerError),
    #[error(transparent)]
    Rejected(#[from] ReportSchedulerRejectedJob),
    #[error(transparent)]
    Source(#[from] ReportJobSourceError),
    #[error("The replacement report identity is already retained")]
    DuplicateReplacement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportServiceEnumTrigger {
    PlayedCompletion,
    ExplicitImported,
    ExplicitVariation,
    Reopen,
    Import,
    MetadataEdit,
}

impl ReportServiceEnumTrigger {
    pub const fn creates_report(self) -> bool {
        matches!(
            self,
            Self::PlayedCompletion | Self::ExplicitImported | Self::ExplicitVariation
        )
    }
}

#[derive(Debug)]
pub enum ReportServiceEnumOutcome {
    Created(ReportJobId),
    Existing(ReportJobId),
    Declined(Box<ReportJob>),
}

pub struct ReportService {
    scheduler: ReportScheduler,
    retained: Vec<ReportJobId>,
}

impl ReportService {
    pub const fn new(scheduler: ReportScheduler) -> Self {
        Self {
            scheduler,
            retained: Vec::new(),
        }
    }

    pub const fn scheduler(&self) -> &ReportScheduler {
        &self.scheduler
    }

    pub const fn scheduler_mut(&mut self) -> &mut ReportScheduler {
        &mut self.scheduler
    }

    fn live(&mut self) {
        let scheduler = &self.scheduler;
        self.retained.retain(|id| scheduler.job(*id).is_ok());
    }

    fn existing(&self, job: &ReportJob) -> Option<ReportJobId> {
        self.retained.iter().copied().find(|id| {
            self.scheduler.job(*id).is_ok_and(|retained| {
                retained.freshness() == ReportJobEnumFreshness::Current
                    && retained.state() != ReportJobEnumState::Cancelled
                    && retained.source().is_same_line(job.source())
            })
        })
    }

    pub fn ids(&mut self) -> Vec<ReportJobId> {
        self.live();
        self.retained.clone()
    }

    pub fn replace(
        &mut self,
        previous: ReportJobId,
        replacement: ReportJob,
    ) -> Result<ReportJobId, ReportServiceError> {
        self.live();
        if self.scheduler.job(replacement.id()).is_ok() {
            return Err(ReportServiceError::DuplicateReplacement);
        }
        self.scheduler.take(previous)?;
        self.retained.retain(|id| *id != previous);
        let id = self.scheduler.enqueue(replacement)?;
        self.retained.push(id);
        Ok(id)
    }

    pub fn request(
        &mut self,
        trigger: ReportServiceEnumTrigger,
        job: ReportJob,
    ) -> Result<ReportServiceEnumOutcome, ReportServiceError> {
        if !trigger.creates_report() {
            return Ok(ReportServiceEnumOutcome::Declined(Box::new(job)));
        }
        self.live();
        if let Some(id) = self.existing(&job) {
            return Ok(ReportServiceEnumOutcome::Existing(id));
        }
        let id = self.scheduler.enqueue(job)?;
        self.retained.push(id);
        Ok(ReportServiceEnumOutcome::Created(id))
    }

    pub fn observe_game(
        &mut self,
        game: GameId,
        mut capture: impl FnMut() -> Result<ReportJobSource, ReportJobSourceError>,
    ) -> Result<usize, ReportServiceError> {
        self.live();
        let ids: Vec<ReportJobId> = self
            .retained
            .iter()
            .copied()
            .filter(|id| {
                self.scheduler
                    .job(*id)
                    .is_ok_and(|job| job.source().game() == game)
            })
            .collect();
        let mut outdated = 0;
        for id in ids {
            let was_current =
                self.scheduler.job(id)?.freshness() == ReportJobEnumFreshness::Current;
            self.scheduler.observe_source(id, capture()?)?;
            if was_current
                && self.scheduler.job(id)?.freshness() == ReportJobEnumFreshness::Outdated
            {
                outdated += 1;
            }
        }
        Ok(outdated)
    }
}
