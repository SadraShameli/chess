use std::mem::size_of;
use std::sync::Arc;

use thiserror::Error;

use crate::engine::search::identity::EngineSearchTag;
use crate::report::job::{
    ReportJob, ReportJobBinding, ReportJobEnumFreshness, ReportJobEnumState, ReportJobProgress,
};

#[derive(Debug, Error)]
pub enum ReportJobReplacementError {
    #[error("Replacement must identify a new queued report for the same game")]
    InvalidProposal,
    #[error("The previous report changed after the replacement preview")]
    StalePreview,
    #[error("The replacement preview exceeds the previous report's memory charge limit")]
    MemoryLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReportJobReplacementSignature {
    state: ReportJobEnumState,
    freshness: ReportJobEnumFreshness,
    progress: ReportJobProgress,
    sequence: u64,
    active: Option<EngineSearchTag>,
}

impl ReportJobReplacementSignature {
    fn capture(job: &ReportJob) -> Self {
        Self {
            state: job.state(),
            freshness: job.freshness(),
            progress: job.progress(),
            sequence: job.sequence(),
            active: job.active_search(),
        }
    }
}

#[derive(Debug)]
pub struct ReportJobReplacementPreview {
    previous: Arc<ReportJobBinding>,
    signature: ReportJobReplacementSignature,
    proposed: ReportJob,
}

impl ReportJobReplacementPreview {
    pub fn new(
        previous: &ReportJob,
        proposed: ReportJob,
    ) -> Result<Self, ReportJobReplacementError> {
        if previous.source().game() != proposed.source().game()
            || previous.id() == proposed.id()
            || proposed.state() != ReportJobEnumState::Queued
            || proposed.freshness() != ReportJobEnumFreshness::Current
        {
            return Err(ReportJobReplacementError::InvalidProposal);
        }
        let charge = usize::try_from(proposed.memory_charge_bytes())
            .map_err(|_error| ReportJobReplacementError::MemoryLimit)?;
        previous
            .source()
            .limits()
            .charge(previous.memory_charge_bytes(), charge)
            .map_err(|_error| ReportJobReplacementError::MemoryLimit)?;
        let combined = previous
            .memory_charge_bytes()
            .checked_add(proposed.memory_charge_bytes())
            .ok_or(ReportJobReplacementError::MemoryLimit)?;
        previous
            .source()
            .limits()
            .charge(combined, size_of::<Self>())
            .map_err(|_error| ReportJobReplacementError::MemoryLimit)?;
        Ok(Self {
            previous: Arc::clone(previous.binding()),
            signature: ReportJobReplacementSignature::capture(previous),
            proposed,
        })
    }

    pub const fn proposed(&self) -> &ReportJob {
        &self.proposed
    }

    pub fn confirm(self, current: &ReportJob) -> Result<ReportJob, ReportJobReplacementError> {
        if !Arc::ptr_eq(&self.previous, current.binding())
            || self.signature != ReportJobReplacementSignature::capture(current)
        {
            return Err(ReportJobReplacementError::StalePreview);
        }
        Ok(self.proposed)
    }
}
