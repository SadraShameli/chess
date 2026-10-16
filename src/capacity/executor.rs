use std::panic::{UnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use rayon::{ThreadPool, ThreadPoolBuildError, ThreadPoolBuilder};
use thiserror::Error;
use tokio::sync::{Semaphore, oneshot};

use crate::capacity::budget::{CapacityBudget, CapacityBudgetError};

#[derive(Debug, Error)]
pub enum CapacityExecutorError {
    #[error(transparent)]
    Budget(#[from] CapacityBudgetError),
    #[error("Cannot construct the CPU worker pool: {0}")]
    Pool(#[from] ThreadPoolBuildError),
    #[error("The CPU executor is shutting down")]
    Closed,
    #[error("The CPU operation failed unexpectedly")]
    WorkerPanicked,
    #[error("The CPU result channel closed: {0}")]
    ResultChannel(#[from] oneshot::error::RecvError),
}

#[derive(Clone)]
pub struct CapacityExecutorCancellation {
    cancelled: Arc<AtomicBool>,
}

impl CapacityExecutorCancellation {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

pub struct CapacityExecutorJob<R> {
    result: oneshot::Receiver<Result<R, CapacityExecutorError>>,
    cancellation: CapacityExecutorCancellation,
}

impl<R> CapacityExecutorJob<R> {
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
    pub async fn join(mut self) -> Result<R, CapacityExecutorError> {
        (&mut self.result).await?
    }
}

impl<R> Drop for CapacityExecutorJob<R> {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

pub struct CapacityExecutor {
    budget: CapacityBudget,
    pool: ThreadPool,
    slots: Arc<Semaphore>,
    accepting: AtomicBool,
}

impl CapacityExecutor {
    pub fn new(budget: CapacityBudget) -> Result<Self, CapacityExecutorError> {
        let pool = ThreadPoolBuilder::new()
            .num_threads(budget.workers())
            .thread_name(|index| format!("chess-cpu-{index}"))
            .build()?;
        let slots = Arc::new(Semaphore::new(budget.workers()));
        Ok(Self {
            budget,
            pool,
            slots,
            accepting: AtomicBool::new(true),
        })
    }

    pub fn submit<F, R>(
        &self,
        operation: F,
    ) -> Result<CapacityExecutorJob<R>, CapacityExecutorError>
    where
        F: FnOnce(&CapacityExecutorCancellation) -> R + Send + UnwindSafe + 'static,
        R: Send + 'static,
    {
        if !self.accepting.load(Ordering::SeqCst) {
            return Err(CapacityExecutorError::Closed);
        }
        let lease = self.budget.try_lease(1)?;
        let slot = Arc::clone(&self.slots)
            .try_acquire_owned()
            .map_err(|_error| CapacityExecutorError::Closed)?;
        if !self.accepting.load(Ordering::SeqCst) {
            return Err(CapacityExecutorError::Closed);
        }
        let cancellation = CapacityExecutorCancellation {
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let worker_cancellation = cancellation.clone();
        let (sender, result) = oneshot::channel();
        self.pool.spawn(move || {
            let outcome = catch_unwind(|| operation(&worker_cancellation))
                .map_err(|_panic| CapacityExecutorError::WorkerPanicked);
            drop((lease, slot));
            if let Err(abandoned_result) = sender.send(outcome) {
                drop(abandoned_result);
            }
        });
        Ok(CapacityExecutorJob {
            result,
            cancellation,
        })
    }

    pub async fn shutdown(self) -> Result<(), CapacityExecutorError> {
        self.accepting.store(false, Ordering::SeqCst);
        let workers = u32::try_from(self.budget.workers()).map_err(CapacityBudgetError::Range)?;
        let drained = Arc::clone(&self.slots)
            .acquire_many_owned(workers)
            .await
            .map_err(|_error| CapacityExecutorError::Closed)?;
        drop(drained);
        Ok(())
    }
}
