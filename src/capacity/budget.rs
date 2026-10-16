use std::num::{NonZeroUsize, TryFromIntError};
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::Semaphore;

use crate::capacity::lease::CapacityLease;

#[derive(Debug, Error)]
pub enum CapacityBudgetError {
    #[error("Cannot detect usable CPU capacity: {0}; choose an explicit budget")]
    Detection(#[source] std::io::Error),
    #[error("The requested CPU allocation is zero or exceeds the total budget")]
    InvalidAllocation,
    #[error("CPU capacity is currently assigned to other work")]
    Unavailable,
    #[error("CPU allocation exceeds the supported range: {0}")]
    Range(#[from] TryFromIntError),
}

#[derive(Clone)]
pub struct CapacityBudget {
    workers: NonZeroUsize,
    permits: Arc<Semaphore>,
}

impl CapacityBudget {
    pub fn discover() -> Result<Self, CapacityBudgetError> {
        Self::new(std::thread::available_parallelism().map_err(CapacityBudgetError::Detection)?)
    }

    pub fn new(workers: NonZeroUsize) -> Result<Self, CapacityBudgetError> {
        u32::try_from(workers.get())?;
        if workers.get() > Semaphore::MAX_PERMITS {
            return Err(CapacityBudgetError::InvalidAllocation);
        }
        Ok(Self {
            workers,
            permits: Arc::new(Semaphore::new(workers.get())),
        })
    }

    pub fn workers(&self) -> usize {
        self.workers.get()
    }
    pub fn available(&self) -> usize {
        self.permits.available_permits()
    }

    pub fn try_lease(&self, workers: usize) -> Result<CapacityLease, CapacityBudgetError> {
        if workers == 0 || workers > self.workers() {
            return Err(CapacityBudgetError::InvalidAllocation);
        }
        let permits = Arc::clone(&self.permits)
            .try_acquire_many_owned(u32::try_from(workers)?)
            .map_err(|_error| CapacityBudgetError::Unavailable)?;
        Ok(CapacityLease::new(permits))
    }

    pub async fn lease(&self, workers: usize) -> Result<CapacityLease, CapacityBudgetError> {
        if workers == 0 || workers > self.workers() {
            return Err(CapacityBudgetError::InvalidAllocation);
        }
        let permits = Arc::clone(&self.permits)
            .acquire_many_owned(u32::try_from(workers)?)
            .await
            .map_err(|_error| CapacityBudgetError::Unavailable)?;
        Ok(CapacityLease::new(permits))
    }
}
