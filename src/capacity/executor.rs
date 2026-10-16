use std::any::Any;
use std::future::Future;
use std::panic::{UnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{Builder, JoinHandle as NativeJoinHandle};

use rayon::{ThreadBuilder, ThreadPool, ThreadPoolBuildError, ThreadPoolBuilder};
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot, watch};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::{Instant, timeout_at};

use crate::capacity::budget::{CapacityBudget, CapacityBudgetError};
use crate::capacity::lease::CapacityLease;

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
    #[error("The CPU completion channel closed before cleanup finished: {0}")]
    CompletionChannel(#[from] watch::error::RecvError),
    #[error("CPU executor initialization already started or shutdown has begun")]
    InitializationUnavailable,
    #[error("CPU executor shutdown exceeded its absolute deadline: {0}")]
    ShutdownDeadline(#[source] tokio::time::error::Elapsed),
    #[error("The CPU shutdown task failed: {0}")]
    ShutdownTask(#[source] JoinError),
    #[error("Native CPU worker {index} failed: {source}")]
    NativeThread {
        index: usize,
        #[source]
        source: CapacityExecutorThreadError,
    },
    #[error("CPU shutdown failed ({operation}); additional cleanup failed ({cleanup})")]
    ShutdownCleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
    #[error("CPU executor shutdown previously failed: {0}")]
    ShutdownFailed(#[source] Arc<Self>),
}

impl CapacityExecutorError {
    fn combine(operation: Result<(), Self>, cleanup: Result<(), Self>) -> Result<(), Self> {
        match (operation, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(operation), Err(cleanup)) => Err(Self::ShutdownCleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            }),
        }
    }
}

#[derive(Debug, Error)]
pub enum CapacityExecutorThreadError {
    #[error("The worker panicked: {0}")]
    Panic(String),
    #[error("The worker panicked with a non-text payload")]
    NonTextPanic,
}

impl CapacityExecutorThreadError {
    fn from_payload(payload: Box<dyn Any + Send>) -> Self {
        match payload.downcast::<String>() {
            Ok(message) => Self::Panic(*message),
            Err(payload) => match payload.downcast::<&'static str>() {
                Ok(message) => Self::Panic((*message).to_owned()),
                Err(_payload) => Self::NonTextPanic,
            },
        }
    }
}

#[derive(Clone)]
pub struct CapacityExecutorCancellation {
    cancelled: Arc<AtomicBool>,
    accepting: Arc<AtomicBool>,
    budget: CapacityBudget,
}

impl CapacityExecutorCancellation {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
            || !self.accepting.load(Ordering::SeqCst)
            || !self.budget.is_workload_admitted()
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

pub struct CapacityExecutorJob<R> {
    result: oneshot::Receiver<Result<R, CapacityExecutorError>>,
    completion: watch::Receiver<bool>,
    cancellation: CapacityExecutorCancellation,
}

impl<R> CapacityExecutorJob<R> {
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
    pub async fn join(mut self) -> Result<R, CapacityExecutorError> {
        let result = (&mut self.result).await?;
        drop(self.completion.wait_for(|completed| *completed).await?);
        result
    }
}

impl<R> Drop for CapacityExecutorJob<R> {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

pub struct CapacityExecutor {
    budget: CapacityBudget,
    pool: Option<ThreadPool>,
    workers: Vec<NativeJoinHandle<()>>,
    joining: Option<JoinHandle<Result<(), CapacityExecutorError>>>,
    slots: Arc<Semaphore>,
    accepting: Arc<AtomicBool>,
    lifecycle: CapacityExecutorEnumLifecycle,
    shutdown_failure: Option<Arc<CapacityExecutorError>>,
}

enum CapacityExecutorEnumLifecycle {
    Uninitialized,
    Running,
    Stopping,
    Complete,
}

impl CapacityExecutor {
    pub fn new(budget: CapacityBudget) -> Result<Self, CapacityExecutorError> {
        let mut executor = Self::uninitialized(budget);
        if let Err(operation) = executor.initialize() {
            if let Err(cleanup) = Self::join_workers(std::mem::take(&mut executor.workers)) {
                return Err(CapacityExecutorError::ShutdownCleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                });
            }
            return Err(operation);
        }
        Ok(executor)
    }

    pub fn uninitialized(budget: CapacityBudget) -> Self {
        let slots = Arc::new(Semaphore::new(budget.workers()));
        Self {
            workers: Vec::with_capacity(budget.workers()),
            budget,
            pool: None,
            joining: None,
            slots,
            accepting: Arc::new(AtomicBool::new(false)),
            lifecycle: CapacityExecutorEnumLifecycle::Uninitialized,
            shutdown_failure: None,
        }
    }

    pub fn initialize(&mut self) -> Result<(), CapacityExecutorError> {
        self.initialize_with(Self::spawn_worker)
    }

    fn initialize_with(
        &mut self,
        mut spawn: impl FnMut(ThreadBuilder) -> std::io::Result<NativeJoinHandle<()>>,
    ) -> Result<(), CapacityExecutorError> {
        if !matches!(self.lifecycle, CapacityExecutorEnumLifecycle::Uninitialized) {
            return Err(CapacityExecutorError::InitializationUnavailable);
        }
        self.lifecycle = CapacityExecutorEnumLifecycle::Stopping;
        let pool = ThreadPoolBuilder::new()
            .num_threads(self.budget.workers())
            .thread_name(|index| format!("chess-cpu-{index}"))
            .spawn_handler(|thread| {
                self.workers.push(spawn(thread)?);
                Ok(())
            })
            .build()?;
        self.pool = Some(pool);
        self.lifecycle = CapacityExecutorEnumLifecycle::Running;
        self.accepting.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn spawn_worker(thread: ThreadBuilder) -> std::io::Result<NativeJoinHandle<()>> {
        let mut builder = Builder::new();
        if let Some(name) = thread.name() {
            builder = builder.name(name.to_owned());
        }
        if let Some(stack_size) = thread.stack_size() {
            builder = builder.stack_size(stack_size);
        }
        builder.spawn(move || thread.run())
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
        self.spawn_job(operation, lease, slot)
    }

    pub async fn submit_waiting<F, R>(
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
        let slot = Arc::clone(&self.slots)
            .acquire_owned()
            .await
            .map_err(|_error| CapacityExecutorError::Closed)?;
        let lease = self.budget.lease(1).await?;
        self.spawn_job(operation, lease, slot)
    }

    fn spawn_job<F, R>(
        &self,
        operation: F,
        lease: CapacityLease,
        slot: OwnedSemaphorePermit,
    ) -> Result<CapacityExecutorJob<R>, CapacityExecutorError>
    where
        F: FnOnce(&CapacityExecutorCancellation) -> R + Send + UnwindSafe + 'static,
        R: Send + 'static,
    {
        if !self.accepting.load(Ordering::SeqCst) {
            return Err(CapacityExecutorError::Closed);
        }
        let pool = self.pool.as_ref().ok_or(CapacityExecutorError::Closed)?;
        let cancellation = CapacityExecutorCancellation {
            cancelled: Arc::new(AtomicBool::new(false)),
            accepting: Arc::clone(&self.accepting),
            budget: self.budget.clone(),
        };
        let worker_cancellation = cancellation.clone();
        let (sender, result) = oneshot::channel();
        let (completed, completion) = watch::channel(false);
        pool.spawn(move || {
            let outcome = catch_unwind(|| operation(&worker_cancellation))
                .map_err(|_panic| CapacityExecutorError::WorkerPanicked);
            if let Err(abandoned_result) = sender.send(outcome) {
                drop(abandoned_result);
            }
            drop((lease, slot));
            completed.send_replace(true);
        });
        Ok(CapacityExecutorJob {
            result,
            completion,
            cancellation,
        })
    }

    pub async fn shutdown(mut self) -> Result<(), CapacityExecutorError> {
        self.observe_shutdown(None).await
    }

    pub async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), CapacityExecutorError> {
        self.observe_shutdown(Some(deadline)).await
    }

    pub fn is_shutdown_complete(&self) -> bool {
        matches!(self.lifecycle, CapacityExecutorEnumLifecycle::Complete)
    }

    async fn observe_shutdown(
        &mut self,
        deadline: Option<Instant>,
    ) -> Result<(), CapacityExecutorError> {
        if self.is_shutdown_complete() {
            return self.shutdown_result();
        }
        self.accepting.store(false, Ordering::SeqCst);
        self.lifecycle = CapacityExecutorEnumLifecycle::Stopping;
        if self.pool.is_some() {
            let workers =
                u32::try_from(self.budget.workers()).map_err(CapacityBudgetError::Range)?;
            let drained = Self::within(
                deadline,
                Arc::clone(&self.slots).acquire_many_owned(workers),
            )
            .await?
            .map_err(|_error| CapacityExecutorError::Closed)?;
            drop(self.pool.take());
            drop(drained);
        }
        if !self.workers.is_empty() && self.joining.is_none() {
            let lease = Self::within(deadline, self.budget.lease(1)).await??;
            let workers = std::mem::take(&mut self.workers);
            self.joining = Some(tokio::task::spawn_blocking(move || {
                let result = Self::join_workers(workers);
                drop(lease);
                result
            }));
        }
        if let Some(joining) = &mut self.joining {
            let joined = Self::within(deadline, joining).await?;
            self.joining = None;
            let result = joined
                .map_err(CapacityExecutorError::ShutdownTask)
                .and_then(|result| result);
            if let Err(error) = result {
                self.shutdown_failure = Some(Arc::new(error));
            }
        }
        self.lifecycle = CapacityExecutorEnumLifecycle::Complete;
        self.shutdown_result()
    }

    fn shutdown_result(&self) -> Result<(), CapacityExecutorError> {
        match &self.shutdown_failure {
            Some(error) => Err(CapacityExecutorError::ShutdownFailed(Arc::clone(error))),
            None => Ok(()),
        }
    }

    async fn within<T>(
        deadline: Option<Instant>,
        operation: impl Future<Output = T>,
    ) -> Result<T, CapacityExecutorError> {
        match deadline {
            Some(deadline) => timeout_at(deadline, operation)
                .await
                .map_err(CapacityExecutorError::ShutdownDeadline),
            None => Ok(operation.await),
        }
    }

    fn join_workers(workers: Vec<NativeJoinHandle<()>>) -> Result<(), CapacityExecutorError> {
        let mut outcome = Ok(());
        for (index, worker) in workers.into_iter().enumerate() {
            if let Err(payload) = worker.join() {
                outcome = CapacityExecutorError::combine(
                    outcome,
                    Err(CapacityExecutorError::NativeThread {
                        index,
                        source: CapacityExecutorThreadError::from_payload(payload),
                    }),
                );
            }
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::Builder;
    use std::time::Duration;

    use tokio::time::Instant;

    use crate::capacity::budget::CapacityBudget;
    use crate::capacity::executor::{CapacityExecutor, CapacityExecutorError};

    #[tokio::test]
    async fn partially_created_pool_retains_native_handles_until_joined()
    -> Result<(), Box<dyn Error>> {
        let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("Invalid test capacity")?)?;
        let mut executor = CapacityExecutor::uninitialized(budget.clone());
        let result = executor.initialize_with(|thread| {
            if thread.index() == 1 {
                return Err(std::io::Error::other(
                    "Controlled native thread creation failure",
                ));
            }
            CapacityExecutor::spawn_worker(thread)
        });
        let error = result
            .err()
            .ok_or("Partial thread creation unexpectedly succeeded")?;
        assert!(matches!(error, CapacityExecutorError::Pool(_)));
        assert!(error.source().is_some());
        assert_eq!(executor.workers.len(), 1);
        assert!(executor.pool.is_none());
        assert!(!executor.is_shutdown_complete());
        assert!(matches!(
            executor.initialize(),
            Err(CapacityExecutorError::InitializationUnavailable)
        ));
        executor
            .shutdown_retained(Instant::now() + Duration::from_secs(3))
            .await?;
        assert!(executor.is_shutdown_complete());
        assert!(executor.workers.is_empty());
        assert!(executor.joining.is_none());
        assert_eq!(budget.available(), budget.workers());
        Ok(())
    }

    #[tokio::test]
    async fn native_join_failures_are_observed_aggregated_and_stable_after_completion()
    -> Result<(), Box<dyn Error>> {
        let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("Invalid test capacity")?)?;
        let allowed = Arc::new(AtomicBool::new(false));
        let mut executor = CapacityExecutor::uninitialized(budget.clone());
        executor.initialize_with(|thread| {
            let allowed = Arc::clone(&allowed);
            Builder::new().spawn(move || {
                thread.run();
                assert!(
                    allowed.load(Ordering::SeqCst),
                    "Controlled native worker exit failure"
                );
            })
        })?;
        let mut previous = None;
        for _attempt in 0..2 {
            let error = executor
                .shutdown_retained(Instant::now() + Duration::from_secs(3))
                .await
                .err()
                .ok_or("Native worker failure was hidden")?;
            assert!(error.source().is_some());
            let message = error.to_string();
            assert!(message.contains("Native CPU worker 0"));
            assert!(message.contains("Native CPU worker 1"));
            assert!(message.contains("Controlled native worker exit failure"));
            if let Some(previous) = previous.replace(message.clone()) {
                assert_eq!(message, previous);
            }
            assert!(executor.is_shutdown_complete());
            assert!(executor.pool.is_none());
            assert!(executor.workers.is_empty());
            assert!(executor.joining.is_none());
        }
        assert_eq!(budget.available(), budget.workers());
        Ok(())
    }
}
