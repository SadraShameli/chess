use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;

#[test]
fn detects_all_usable_threads_without_a_silent_fallback() -> Result<(), Box<dyn std::error::Error>>
{
    let budget = CapacityBudget::discover()?;
    assert_eq!(
        budget.workers(),
        std::thread::available_parallelism()?.get()
    );
    let lease = budget.try_lease(budget.workers())?;
    assert_eq!(budget.available(), 0);
    assert!(budget.try_lease(1).is_err());
    drop(lease);
    assert_eq!(budget.available(), budget.workers());
    assert!(budget.try_lease(0).is_err());
    Ok(())
}

#[tokio::test]
async fn executor_and_engine_style_leases_share_the_same_capacity()
-> Result<(), Box<dyn std::error::Error>> {
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid test capacity")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let engine_lease = budget.try_lease(1)?;
    assert!(executor.submit(|_cancellation| 17_u64).is_err());
    drop(engine_lease);
    assert_eq!(executor.submit(|_cancellation| 17_u64)?.join().await?, 17);
    assert_eq!(budget.available(), 1);
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn cancellation_is_cooperative_and_shutdown_drains_owned_work()
-> Result<(), Box<dyn std::error::Error>> {
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid test capacity")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let entered = Arc::new(std::sync::Barrier::new(2));
    let worker_entered = Arc::clone(&entered);
    let (release_sender, release_receiver) = mpsc::sync_channel(1);
    let observed = Arc::new(AtomicBool::new(false));
    let worker_observed = Arc::clone(&observed);
    let job = executor.submit(
        move |cancellation| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            worker_entered.wait();
            release_receiver.recv()?;
            worker_observed.store(cancellation.is_cancelled(), Ordering::SeqCst);
            Ok(())
        },
    )?;
    tokio::task::spawn_blocking(move || entered.wait()).await?;
    assert!(executor.submit(|_cancellation| 1).is_err());
    drop(job);
    release_sender.try_send(())?;
    executor.shutdown().await?;
    assert!(observed.load(Ordering::SeqCst));
    assert_eq!(budget.available(), 1);
    Ok(())
}

#[tokio::test]
async fn admits_a_full_bounded_batch_at_detected_capacity() -> Result<(), Box<dyn std::error::Error>>
{
    let budget = CapacityBudget::discover()?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let lease = budget.try_lease(budget.workers())?;
    assert_eq!(lease.workers(), budget.workers());
    drop(lease);
    let mut jobs = Vec::with_capacity(budget.workers());
    for value in 0..budget.workers() {
        jobs.push(executor.submit(move |_cancellation| value)?);
    }
    for (expected, job) in jobs.into_iter().enumerate() {
        assert_eq!(job.join().await?, expected);
    }
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn fair_capacity_waits_release_on_cancellation() -> Result<(), Box<dyn std::error::Error>> {
    use std::future::{Future, poll_fn};
    use std::task::Poll;

    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let held = budget.try_lease(1)?;
    let mut waiting = std::pin::pin!(budget.lease(1));
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(held);
    let acquired = waiting.await?;
    assert_eq!(budget.available(), 0);
    drop(acquired);
    assert_eq!(budget.available(), 1);
    assert!(budget.lease(0).await.is_err());
    let held = budget.try_lease(1)?;
    {
        let mut cancelled = Box::pin(budget.lease(1));
        poll_fn(|context| {
            assert!(cancelled.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    drop(held);
    assert_eq!(budget.available(), 1);
    Ok(())
}
