use std::collections::VecDeque;
use std::error::Error;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Waker};
use std::time::Duration;

use chess::board::position::BoardPosition;
use chess::clock::duration::ClockDuration;
use chess::engine::capabilities::EngineIdentity;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::selector::EnginePackageSelected;
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
    EngineProcessProvider,
};
use chess::engine::resource_config::EngineResourceConfig;
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
};
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::session::{EngineSession, EngineSessionError};
use tokio::sync::Notify;
use tokio::time::Instant;
use uuid::Uuid;

use crate::engine_package_selection::EnginePackageFixture;

#[derive(Clone, Copy)]
enum EngineRetainedShutdownEnumCompletion {
    Success,
    Failure,
    Unobserved,
    UnobservedForced,
}

#[derive(Default)]
struct EngineRetainedShutdownObservations {
    live: AtomicUsize,
    shutdown: AtomicUsize,
    terminate: AtomicUsize,
    stop: AtomicUsize,
    permitted: AtomicBool,
    ready: Notify,
    deadlines: Mutex<Vec<Instant>>,
}

impl EngineRetainedShutdownObservations {
    fn release(&self) {
        self.permitted.store(true, Ordering::SeqCst);
        self.ready.notify_waiters();
    }

    fn deadlines(&self) -> Result<Vec<Instant>, Box<dyn Error>> {
        Ok(self
            .deadlines
            .try_lock()
            .map_err(|_error| "Deadline observations are unavailable")?
            .clone())
    }
}

struct EngineRetainedShutdownProvider {
    observations: Arc<EngineRetainedShutdownObservations>,
    completion: EngineRetainedShutdownEnumCompletion,
    stop_delay: Duration,
}

impl EngineRetainedShutdownProvider {
    fn new(completion: EngineRetainedShutdownEnumCompletion) -> Self {
        Self {
            observations: Arc::new(EngineRetainedShutdownObservations::default()),
            completion,
            stop_delay: Duration::ZERO,
        }
    }
}

impl EngineProcessProvider for EngineRetainedShutdownProvider {
    type Process = EngineRetainedShutdownProcess;

    fn spawn(&self, _package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        let mut lines = VecDeque::from([
            "id name Reckless 0.9.0".to_owned(),
            format!("id author {}", EngineIdentity::AUTHOR),
        ]);
        lines.extend(
            EngineEnumOption::ALL.map(|option| {
                format!("option name {} type {}", option.name(), option.descriptor())
            }),
        );
        lines.extend(["uciok".to_owned(), "readyok".to_owned()]);
        self.observations.live.fetch_add(1, Ordering::SeqCst);
        Ok(EngineRetainedShutdownProcess {
            observations: Arc::clone(&self.observations),
            lines,
            completion: self.completion,
            stop_delay: self.stop_delay,
            complete: false,
        })
    }
}

struct EngineRetainedShutdownProcess {
    observations: Arc<EngineRetainedShutdownObservations>,
    lines: VecDeque<String>,
    completion: EngineRetainedShutdownEnumCompletion,
    stop_delay: Duration,
    complete: bool,
}

impl EngineRetainedShutdownProcess {
    async fn finish(&mut self, deadline: Instant, forced: bool) -> Result<(), EngineProcessError> {
        self.observations
            .deadlines
            .try_lock()
            .map_err(|_error| std::io::Error::other("Deadline observations are unavailable"))?
            .push(deadline);
        loop {
            let notified = self.observations.ready.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.observations.permitted.load(Ordering::SeqCst) {
                break;
            }
            notified.await;
        }
        self.complete = match self.completion {
            EngineRetainedShutdownEnumCompletion::UnobservedForced => {
                self.observations.terminate.load(Ordering::SeqCst) > 1
            }
            EngineRetainedShutdownEnumCompletion::Unobserved => forced,
            EngineRetainedShutdownEnumCompletion::Success
            | EngineRetainedShutdownEnumCompletion::Failure => true,
        };
        match self.completion {
            EngineRetainedShutdownEnumCompletion::Success
            | EngineRetainedShutdownEnumCompletion::Unobserved
            | EngineRetainedShutdownEnumCompletion::UnobservedForced => Ok(()),
            EngineRetainedShutdownEnumCompletion::Failure => {
                Err(std::io::Error::other("Controlled engine cleanup failure").into())
            }
        }
    }

    async fn search(session: &mut EngineSession<Self>) -> Result<(), Box<dyn Error>> {
        let context = EngineSearchContext::new(
            EngineSearchGameId::try_from(Uuid::from_u128(1))?,
            EngineSearchRevision::new(1),
        );
        session
            .start_search(
                context,
                EngineSearchPosition::new(BoardPosition::standard(), std::iter::empty::<&str>())?,
                EngineSearchLimit::infinite(),
            )
            .await?;
        Ok(())
    }
}

impl EngineProcess for EngineRetainedShutdownProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        if command == "stop" {
            self.observations.stop.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.stop_delay).await;
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        self.lines
            .pop_front()
            .ok_or(EngineProcessError::Closed(EngineProcessEnumPipe::Stdout))
            .and_then(|text| EngineProcessLine::new(EngineProcessEnumPipe::Stdout, text))
    }

    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.shutdown_retained(Instant::now() + Duration::from_secs(3))
            .await
    }

    async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        self.observations.shutdown.fetch_add(1, Ordering::SeqCst);
        self.finish(deadline, false).await
    }

    fn is_shutdown_complete(&self) -> bool {
        self.complete
    }

    async fn terminate(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        self.observations.terminate.fetch_add(1, Ordering::SeqCst);
        self.finish(deadline, true).await
    }
}

impl Drop for EngineRetainedShutdownProcess {
    fn drop(&mut self) {
        self.observations.live.fetch_sub(1, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn deadline_retains_the_process_and_retry_finishes_the_same_owner()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider =
        EngineRetainedShutdownProvider::new(EngineRetainedShutdownEnumCompletion::Success);
    let mut session = EngineSession::start(&provider, fixture.select()?).await?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let error = session
        .shutdown_retained(deadline)
        .await
        .err()
        .ok_or("Shutdown unexpectedly completed")?;
    assert!(error.source().is_some());
    assert_eq!(Instant::now(), deadline);
    assert!(!session.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 1);
    assert!(session.readiness().await.is_err());
    provider.observations.release();
    let retry_deadline = Instant::now() + Duration::from_secs(3);
    session.shutdown_retained(retry_deadline).await?;
    assert!(session.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 0);
    assert_eq!(
        provider.observations.deadlines()?,
        [deadline, retry_deadline]
    );
    assert_eq!(provider.observations.terminate.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelling_the_wait_does_not_drop_concrete_cleanup_ownership() -> Result<(), Box<dyn Error>>
{
    let fixture = EnginePackageFixture::new()?;
    let provider =
        EngineRetainedShutdownProvider::new(EngineRetainedShutdownEnumCompletion::Success);
    let mut session = EngineSession::start(&provider, fixture.select()?).await?;
    let deadline = Instant::now() + Duration::from_secs(3);
    {
        let waiting = session.shutdown_retained(deadline);
        let mut waiting = std::pin::pin!(waiting);
        assert!(
            waiting
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    assert!(!session.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 1);
    provider.observations.release();
    session.shutdown_retained(deadline).await?;
    assert!(session.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn completed_failures_preserve_sources_without_reinvoking_cleanup()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider =
        EngineRetainedShutdownProvider::new(EngineRetainedShutdownEnumCompletion::Failure);
    provider.observations.release();
    let mut session = EngineSession::start(&provider, fixture.select()?).await?;
    let deadline = Instant::now() + Duration::from_secs(3);
    for _ in 0..2 {
        let error = session
            .shutdown_retained(deadline)
            .await
            .err()
            .ok_or("Cleanup failure was hidden")?;
        assert!(error.source().is_some());
        assert!(
            error
                .to_string()
                .contains("Controlled engine cleanup failure")
        );
        assert!(session.is_shutdown_complete());
    }
    assert_eq!(provider.observations.shutdown.load(Ordering::SeqCst), 1);
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn successful_cleanup_is_repeatable_without_a_second_effect() -> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider =
        EngineRetainedShutdownProvider::new(EngineRetainedShutdownEnumCompletion::Success);
    provider.observations.release();
    let mut session = EngineSession::start(&provider, fixture.select()?).await?;
    let deadline = Instant::now() + Duration::from_secs(3);
    session.shutdown_retained(deadline).await?;
    session.shutdown_retained(deadline).await?;
    assert!(session.is_shutdown_complete());
    assert_eq!(provider.observations.shutdown.load(Ordering::SeqCst), 1);
    assert_eq!(provider.observations.terminate.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn successful_but_unobserved_cleanup_is_refused_and_remains_retryable()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider =
        EngineRetainedShutdownProvider::new(EngineRetainedShutdownEnumCompletion::Unobserved);
    provider.observations.release();
    let mut session = EngineSession::start(&provider, fixture.select()?).await?;
    let deadline = Instant::now() + Duration::from_secs(3);
    assert!(matches!(
        session.shutdown_retained(deadline).await,
        Err(EngineSessionError::ShutdownUnobserved)
    ));
    assert!(!session.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 1);
    session.shutdown_retained(deadline).await?;
    assert!(session.is_shutdown_complete());
    assert_eq!(provider.observations.terminate.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn stop_delivery_and_shutdown_share_one_absolute_deadline() -> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider =
        EngineRetainedShutdownProvider::new(EngineRetainedShutdownEnumCompletion::Success);
    provider.stop_delay = Duration::from_secs(2);
    let mut session = EngineSession::start(&provider, fixture.select()?).await?;
    EngineRetainedShutdownProcess::search(&mut session).await?;
    let deadline = Instant::now() + Duration::from_secs(3);
    assert!(session.shutdown_retained(deadline).await.is_err());
    assert_eq!(Instant::now(), deadline);
    assert_eq!(provider.observations.deadlines()?, [deadline]);
    assert_eq!(provider.observations.stop.load(Ordering::SeqCst), 1);
    assert!(!session.is_shutdown_complete());
    provider.observations.release();
    session
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(session.is_shutdown_complete());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_stop_delivery_forces_cleanup_without_repeating_the_write()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider =
        EngineRetainedShutdownProvider::new(EngineRetainedShutdownEnumCompletion::Success);
    provider.stop_delay = Duration::from_secs(2);
    let mut session = EngineSession::start(&provider, fixture.select()?).await?;
    EngineRetainedShutdownProcess::search(&mut session).await?;
    let deadline = Instant::now() + Duration::from_secs(3);
    {
        let waiting = session.shutdown_retained(deadline);
        let mut waiting = std::pin::pin!(waiting);
        assert!(
            waiting
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    provider.observations.release();
    session.shutdown_retained(deadline).await?;
    assert_eq!(provider.observations.stop.load(Ordering::SeqCst), 1);
    assert_eq!(provider.observations.shutdown.load(Ordering::SeqCst), 0);
    assert_eq!(provider.observations.terminate.load(Ordering::SeqCst), 1);
    assert!(session.is_shutdown_complete());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn failed_preparation_retains_successfully_requested_but_unobserved_forced_cleanup()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider =
        EngineRetainedShutdownProvider::new(EngineRetainedShutdownEnumCompletion::UnobservedForced);
    provider.observations.release();
    let mut session = EngineSession::start(&provider, fixture.select()?).await?;
    let error = session
        .configure_resources(EngineResourceConfig::new(1, 64, ClockDuration::ZERO)?)
        .await
        .err()
        .ok_or("missing preparation failure")?;
    assert!(!session.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 1);
    assert!(error.source().is_some());
    session
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(session.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 0);
    assert_eq!(provider.observations.terminate.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn failed_preparation_bounds_forced_cleanup_and_keeps_its_process_for_retry()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider =
        EngineRetainedShutdownProvider::new(EngineRetainedShutdownEnumCompletion::Success);
    let mut session = EngineSession::start(&provider, fixture.select()?).await?;
    let started = Instant::now();
    {
        let preparation =
            session.configure_resources(EngineResourceConfig::new(1, 64, ClockDuration::ZERO)?);
        tokio::pin!(preparation);
        assert!(
            preparation
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        tokio::time::advance(Duration::from_secs(3)).await;
        assert!(matches!(
            preparation
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            std::task::Poll::Ready(Err(_))
        ));
    }
    assert_eq!(Instant::now() - started, Duration::from_secs(3));
    assert!(!session.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 1);
    provider.observations.release();
    session
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(session.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 0);
    Ok(())
}
