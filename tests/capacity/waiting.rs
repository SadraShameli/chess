use std::future::{Future, poll_fn};
use std::num::NonZeroUsize;
use std::task::Poll;

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::capacity::phase::CapacityPhase;

#[tokio::test]
async fn waiting_submission_retains_owned_work_until_shared_capacity_is_available()
-> Result<(), Box<dyn std::error::Error>> {
    for workers in [NonZeroUsize::MIN, std::thread::available_parallelism()?] {
        let budget = CapacityBudget::new(workers)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let held = budget.try_lease(workers.get())?;
        let owned = String::from("retained payload");
        let mut pending = Box::pin(executor.submit_waiting(move |_cancellation| owned));
        poll_fn(|context| {
            assert!(pending.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        assert_eq!(budget.available(), 0);
        drop(held);
        assert_eq!(pending.await?.join().await?, "retained payload");
        assert_eq!(budget.available(), workers.get());
        executor.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn cancelled_waiting_submission_releases_its_slot_and_preparation_waits_without_spinning()
-> Result<(), Box<dyn std::error::Error>> {
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let held = budget.try_lease(1)?;
    let mut abandoned = Box::pin(executor.submit_waiting(|_cancellation| 1));
    poll_fn(|context| {
        assert!(abandoned.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(abandoned);
    drop(held);
    assert_eq!(
        executor
            .submit_waiting(|_cancellation| 2)
            .await?
            .join()
            .await?,
        2
    );
    executor.shutdown().await?;

    let budget = CapacityBudget::new(
        NonZeroUsize::new(CapacityPhase::MINIMUM_WORKERS).ok_or("invalid native minimum")?,
    )?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let mut phase = CapacityPhase::new(budget.clone())?;
    let generation = phase.request_preparation()?;
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    let mut waiting = Box::pin(executor.submit_waiting(|_cancellation| 3));
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    phase.complete_preparation(generation)?;
    assert_eq!(waiting.await?.join().await?, 3);
    assert_eq!(budget.available(), phase.workload_workers());
    drop(phase);
    executor.shutdown().await?;
    Ok(())
}
