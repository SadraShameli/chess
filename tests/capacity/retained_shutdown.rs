use std::cell::RefCell;
use std::error::Error;
use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::mpsc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::{CapacityExecutor, CapacityExecutorError, CapacityExecutorJob};
use tokio::sync::watch;
use tokio::time::Instant;

std::thread_local! {
    static CAPACITY_EXECUTOR_SHUTDOWN_EXIT: RefCell<Option<CapacityExecutorShutdownTestCleanup>> = const { RefCell::new(None) };
}

struct CapacityExecutorShutdownTestCleanup {
    entered: watch::Sender<bool>,
    release: mpsc::Receiver<()>,
    finished: watch::Sender<bool>,
}

impl Drop for CapacityExecutorShutdownTestCleanup {
    fn drop(&mut self) {
        self.entered.send_replace(true);
        self.finished.send_replace(self.release.recv().is_ok());
    }
}

struct CapacityExecutorShutdownTestExit {
    executor: CapacityExecutor,
    budget: CapacityBudget,
    release: mpsc::SyncSender<()>,
    entering: watch::Receiver<bool>,
    finishing: watch::Receiver<bool>,
}

impl CapacityExecutorShutdownTestExit {
    async fn new() -> Result<Self, Box<dyn Error>> {
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let (release, released) = mpsc::sync_channel(1);
        let (entered, entering) = watch::channel(false);
        let (finished, finishing) = watch::channel(false);
        let installed = executor
            .submit(move |_cancellation| {
                CAPACITY_EXECUTOR_SHUTDOWN_EXIT.with(|slot| {
                    slot.replace(Some(CapacityExecutorShutdownTestCleanup {
                        entered,
                        release: released,
                        finished,
                    }))
                    .is_none()
                })
            })?
            .join()
            .await?;
        if !installed {
            return Err("The native thread exit guard was already installed".into());
        }
        Ok(Self {
            executor,
            budget,
            release,
            entering,
            finishing,
        })
    }
}

struct CapacityExecutorShutdownTestWork {
    executor: CapacityExecutor,
    budget: CapacityBudget,
    release: mpsc::SyncSender<()>,
    job: CapacityExecutorJob<Result<bool, mpsc::RecvError>>,
}

impl CapacityExecutorShutdownTestWork {
    async fn new() -> Result<Self, Box<dyn Error>> {
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let (release, released) = mpsc::sync_channel(1);
        let (entered, mut entering) = watch::channel(false);
        let job = executor.submit(move |cancellation| {
            entered.send_replace(true);
            released.recv().map(|()| cancellation.is_cancelled())
        })?;
        entering.wait_for(|entered| *entered).await?;
        Ok(Self {
            executor,
            budget,
            release,
            job,
        })
    }
}

#[tokio::test]
async fn shutdown_retains_capacity_until_abandoned_result_cleanup_finishes()
-> Result<(), Box<dyn Error>> {
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let (return_result, returned) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let (entered, mut entering) = watch::channel(false);
    let (finished, mut finishing) = watch::channel(false);
    let job = executor.submit(move |_cancellation| {
        returned
            .recv()
            .map(|()| CapacityExecutorShutdownTestCleanup {
                entered,
                release: released,
                finished,
            })
    })?;
    drop(job);
    return_result.try_send(())?;
    entering.wait_for(|entered| *entered).await?;
    let mut shutdown = std::pin::pin!(executor.shutdown());
    let prematurely_completed = match shutdown
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(result) => {
            result?;
            true
        }
        Poll::Pending => false,
    };
    let available_during_cleanup = budget.available();
    release.try_send(())?;
    if !prematurely_completed {
        shutdown.await?;
    }
    finishing.wait_for(|finished| *finished).await?;
    assert!(!prematurely_completed);
    assert_eq!(available_during_cleanup, 0);
    assert_eq!(budget.available(), 1);
    Ok(())
}

#[tokio::test]
async fn legacy_shutdown_waits_for_the_native_thread_teardown_to_be_observed()
-> Result<(), Box<dyn Error>> {
    let mut fixture = CapacityExecutorShutdownTestExit::new().await?;
    let mut stopping = std::pin::pin!(fixture.executor.shutdown());
    let prematurely_completed = match stopping
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(result) => {
            result?;
            true
        }
        Poll::Pending => false,
    };
    fixture.entering.wait_for(|entered| *entered).await?;
    fixture.release.try_send(())?;
    if !prematurely_completed {
        stopping.await?;
    }
    fixture.finishing.wait_for(|finished| *finished).await?;
    assert!(!prematurely_completed);
    assert_eq!(fixture.budget.available(), 1);
    Ok(())
}

#[tokio::test]
async fn cancelled_and_expired_shutdown_waits_retain_the_native_worker_join_owner()
-> Result<(), Box<dyn Error>> {
    let mut fixture = CapacityExecutorShutdownTestExit::new().await?;
    {
        let mut stopping = std::pin::pin!(
            fixture
                .executor
                .shutdown_retained(Instant::now() + Duration::from_secs(3))
        );
        assert!(
            stopping
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        fixture.entering.wait_for(|entered| *entered).await?;
    }
    assert!(!fixture.executor.is_shutdown_complete());
    assert_eq!(fixture.budget.available(), 0);
    assert!(matches!(
        fixture.executor.submit(|_cancellation| ()),
        Err(CapacityExecutorError::Closed)
    ));
    let error = fixture
        .executor
        .shutdown_retained(Instant::now())
        .await
        .err()
        .ok_or("The blocked native worker was reported joined")?;
    assert!(matches!(error, CapacityExecutorError::ShutdownDeadline(_)));
    assert!(error.source().is_some());
    assert!(!fixture.executor.is_shutdown_complete());
    fixture.release.try_send(())?;
    fixture.finishing.wait_for(|finished| *finished).await?;
    fixture
        .executor
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(fixture.executor.is_shutdown_complete());
    assert_eq!(fixture.budget.available(), 1);
    fixture.executor.shutdown_retained(Instant::now()).await?;
    Ok(())
}

#[tokio::test]
async fn deadline_during_work_drain_closes_admission_and_cancels_work_cooperatively()
-> Result<(), Box<dyn Error>> {
    let CapacityExecutorShutdownTestWork {
        mut executor,
        budget,
        release,
        job,
    } = CapacityExecutorShutdownTestWork::new().await?;
    let error = executor
        .shutdown_retained(Instant::now())
        .await
        .err()
        .ok_or("Running work was reported drained")?;
    assert!(matches!(error, CapacityExecutorError::ShutdownDeadline(_)));
    assert!(error.source().is_some());
    assert!(!executor.is_shutdown_complete());
    assert!(matches!(
        executor.submit(|_cancellation| ()),
        Err(CapacityExecutorError::Closed)
    ));
    release.try_send(())?;
    assert!(job.join().await??);
    executor
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(executor.is_shutdown_complete());
    assert_eq!(budget.available(), 1);
    Ok(())
}

#[tokio::test]
async fn cancellation_while_draining_keeps_the_pool_owned_until_retry() -> Result<(), Box<dyn Error>>
{
    let CapacityExecutorShutdownTestWork {
        mut executor,
        budget,
        release,
        job,
    } = CapacityExecutorShutdownTestWork::new().await?;
    {
        let mut stopping =
            std::pin::pin!(executor.shutdown_retained(Instant::now() + Duration::from_secs(3)));
        assert!(
            stopping
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    assert!(!executor.is_shutdown_complete());
    assert_eq!(budget.available(), 0);
    release.try_send(())?;
    assert!(job.join().await??);
    executor
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(executor.is_shutdown_complete());
    assert_eq!(budget.available(), 1);
    Ok(())
}

#[tokio::test]
async fn observed_shutdown_is_repeatable_for_one_and_all_available_native_workers()
-> Result<(), Box<dyn Error>> {
    for budget in [
        CapacityBudget::new(NonZeroUsize::MIN)?,
        CapacityBudget::discover()?,
    ] {
        let mut executor = CapacityExecutor::uninitialized(budget.clone());
        assert!(matches!(
            executor.submit(|_cancellation| ()),
            Err(CapacityExecutorError::Closed)
        ));
        executor.initialize()?;
        assert!(matches!(
            executor.initialize(),
            Err(CapacityExecutorError::InitializationUnavailable)
        ));
        assert!(!executor.is_shutdown_complete());
        executor
            .shutdown_retained(Instant::now() + Duration::from_secs(3))
            .await?;
        assert!(executor.is_shutdown_complete());
        executor.shutdown_retained(Instant::now()).await?;
        assert!(matches!(
            executor.initialize(),
            Err(CapacityExecutorError::InitializationUnavailable)
        ));
        assert_eq!(budget.available(), budget.workers());
    }
    Ok(())
}
