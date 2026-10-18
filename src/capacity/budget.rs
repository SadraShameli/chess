use std::num::{NonZeroUsize, TryFromIntError};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use thiserror::Error;
use tokio::sync::{Notify, Semaphore};

use crate::capacity::lease::CapacityLease;

#[derive(Debug, Error)]
pub enum CapacityBudgetError {
    #[error("Cannot detect usable CPU capacity: {0}; choose an explicit budget")]
    Detection(#[source] std::io::Error),
    #[error("The requested CPU allocation is zero or exceeds the total budget")]
    InvalidAllocation,
    #[error("CPU capacity is currently assigned to other work")]
    Unavailable,
    #[error("CPU workload admission is closed for framework preparation or recovery")]
    AdmissionClosed,
    #[error("The CPU budget already has a phase controller")]
    PhaseOwned,
    #[error("CPU allocation exceeds the supported range: {0}")]
    Range(#[from] TryFromIntError),
}

#[derive(Clone)]
pub struct CapacityBudget {
    workers: NonZeroUsize,
    permits: Arc<Semaphore>,
    admission: Arc<CapacityBudgetAdmission>,
}

struct CapacityBudgetAdmission {
    is_open: AtomicBool,
    is_owned: AtomicBool,
    changed: Notify,
}

pub(super) struct CapacityBudgetPhaseAdmission {
    budget: CapacityBudget,
}

impl CapacityBudgetPhaseAdmission {
    pub(super) fn close(&self) {
        self.budget.admission.is_open.store(false, Ordering::SeqCst);
        self.budget.admission.changed.notify_waiters();
    }

    pub(super) fn open(&self) {
        self.budget.admission.is_open.store(true, Ordering::SeqCst);
        self.budget.admission.changed.notify_waiters();
    }

    pub(super) fn try_lease(&self, workers: usize) -> Result<CapacityLease, CapacityBudgetError> {
        self.budget.try_lease_reserved(workers)
    }
}

impl Drop for CapacityBudgetPhaseAdmission {
    fn drop(&mut self) {
        self.open();
        self.budget
            .admission
            .is_owned
            .store(false, Ordering::SeqCst);
    }
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
            admission: Arc::new(CapacityBudgetAdmission {
                is_open: AtomicBool::new(true),
                is_owned: AtomicBool::new(false),
                changed: Notify::new(),
            }),
        })
    }

    pub fn workers(&self) -> usize {
        self.workers.get()
    }
    pub fn available(&self) -> usize {
        self.permits.available_permits()
    }

    pub fn try_lease(&self, workers: usize) -> Result<CapacityLease, CapacityBudgetError> {
        self.validate_allocation(workers)?;
        if !self.is_workload_admitted() {
            return Err(CapacityBudgetError::AdmissionClosed);
        }
        let lease = self.try_lease_reserved(workers)?;
        if !self.is_workload_admitted() {
            return Err(CapacityBudgetError::AdmissionClosed);
        }
        Ok(lease)
    }

    pub async fn lease(&self, workers: usize) -> Result<CapacityLease, CapacityBudgetError> {
        let workers = self.validate_allocation(workers)?;
        loop {
            let changed = self.admission.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.is_workload_admitted() {
                changed.await;
                continue;
            }
            let permits = tokio::select! {
                biased;
                () = changed.as_mut() => continue,
                acquired = Arc::clone(&self.permits).acquire_many_owned(workers) => {
                    acquired.map_err(|_error| CapacityBudgetError::Unavailable)?
                }
            };
            if self.is_workload_admitted() {
                return Ok(CapacityLease::new(permits));
            }
        }
    }

    pub(super) fn claim_phase_admission(
        &self,
    ) -> Result<CapacityBudgetPhaseAdmission, CapacityBudgetError> {
        self.admission
            .is_owned
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_current| CapacityBudgetError::PhaseOwned)?;
        let admission = CapacityBudgetPhaseAdmission {
            budget: self.clone(),
        };
        admission.close();
        Ok(admission)
    }

    pub(super) fn is_workload_admitted(&self) -> bool {
        self.admission.is_open.load(Ordering::SeqCst)
    }

    fn try_lease_reserved(&self, workers: usize) -> Result<CapacityLease, CapacityBudgetError> {
        let workers = self.validate_allocation(workers)?;
        let permits = Arc::clone(&self.permits)
            .try_acquire_many_owned(workers)
            .map_err(|_error| CapacityBudgetError::Unavailable)?;
        Ok(CapacityLease::new(permits))
    }

    fn validate_allocation(&self, workers: usize) -> Result<u32, CapacityBudgetError> {
        if workers == 0 || workers > self.workers() {
            return Err(CapacityBudgetError::InvalidAllocation);
        }
        Ok(u32::try_from(workers)?)
    }
}
