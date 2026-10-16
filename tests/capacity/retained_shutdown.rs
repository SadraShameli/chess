use std::error::Error;
use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::mpsc;
use std::task::{Context, Poll, Waker};

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use tokio::sync::watch;

struct CapacityExecutorShutdownTestResult {
    entered: watch::Sender<bool>,
    release: mpsc::Receiver<()>,
    finished: watch::Sender<bool>,
}

impl Drop for CapacityExecutorShutdownTestResult {
    fn drop(&mut self) {
        self.entered.send_replace(true);
        self.finished.send_replace(self.release.recv().is_ok());
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
            .map(|()| CapacityExecutorShutdownTestResult {
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
