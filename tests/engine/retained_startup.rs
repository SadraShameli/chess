use std::collections::VecDeque;
use std::error::Error;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Waker};
use std::time::Duration;

use chess::engine::capabilities::EngineIdentity;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::selector::EnginePackageSelected;
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
    EngineProcessProvider,
};
use chess::engine::session::{EngineSessionError, EngineSessionStartup};
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::engine_package_selection::EnginePackageFixture;

#[derive(Clone, Copy)]
enum EngineSessionStartupTestEnumHandshake {
    Ready,
    Rejected,
    Waiting,
}

#[derive(Clone, Copy)]
enum EngineSessionStartupTestEnumCleanup {
    Success,
    Failure,
    Unobserved,
}

#[derive(Default)]
struct EngineSessionStartupTestObservations {
    live: AtomicUsize,
    incomplete_drop: AtomicBool,
    shutdown: AtomicUsize,
    terminate: AtomicUsize,
    permitted: AtomicBool,
    ready: Notify,
    deadlines: Mutex<Vec<Instant>>,
}

impl EngineSessionStartupTestObservations {
    fn release(&self) {
        self.permitted.store(true, Ordering::SeqCst);
        self.ready.notify_waiters();
    }

    fn deadlines(&self) -> Result<Vec<Instant>, Box<dyn Error>> {
        Ok(self
            .deadlines
            .try_lock()
            .map_err(|_error| "Startup cleanup deadlines are unavailable")?
            .clone())
    }
}

struct EngineSessionStartupTestProvider {
    observations: Arc<EngineSessionStartupTestObservations>,
    handshake: EngineSessionStartupTestEnumHandshake,
    cleanup: EngineSessionStartupTestEnumCleanup,
}

impl EngineSessionStartupTestProvider {
    fn new(
        handshake: EngineSessionStartupTestEnumHandshake,
        cleanup: EngineSessionStartupTestEnumCleanup,
    ) -> Self {
        Self {
            observations: Arc::new(EngineSessionStartupTestObservations::default()),
            handshake,
            cleanup,
        }
    }
}

impl EngineProcessProvider for EngineSessionStartupTestProvider {
    type Process = EngineSessionStartupTestProcess;

    fn spawn(&self, _package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        let lines = match self.handshake {
            EngineSessionStartupTestEnumHandshake::Ready => {
                let mut lines = VecDeque::from([
                    "id name Reckless 0.9.0".to_owned(),
                    format!("id author {}", EngineIdentity::AUTHOR),
                ]);
                lines.extend(EngineEnumOption::ALL.map(|option| {
                    format!("option name {} type {}", option.name(), option.descriptor())
                }));
                lines.extend(["uciok".to_owned(), "readyok".to_owned()]);
                lines
            }
            EngineSessionStartupTestEnumHandshake::Rejected => {
                VecDeque::from(["invalid startup response".to_owned()])
            }
            EngineSessionStartupTestEnumHandshake::Waiting => VecDeque::new(),
        };
        self.observations.live.fetch_add(1, Ordering::SeqCst);
        Ok(EngineSessionStartupTestProcess {
            observations: Arc::clone(&self.observations),
            lines,
            cleanup: self.cleanup,
            complete: false,
        })
    }
}

struct EngineSessionStartupTestProcess {
    observations: Arc<EngineSessionStartupTestObservations>,
    lines: VecDeque<String>,
    cleanup: EngineSessionStartupTestEnumCleanup,
    complete: bool,
}

impl EngineSessionStartupTestProcess {
    async fn finish(&mut self, deadline: Instant, forced: bool) -> Result<(), EngineProcessError> {
        self.observations
            .deadlines
            .try_lock()
            .map_err(|_error| std::io::Error::other("Startup cleanup deadlines are unavailable"))?
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
        self.complete = forced
            || !matches!(
                self.cleanup,
                EngineSessionStartupTestEnumCleanup::Unobserved
            );
        match self.cleanup {
            EngineSessionStartupTestEnumCleanup::Success
            | EngineSessionStartupTestEnumCleanup::Unobserved => Ok(()),
            EngineSessionStartupTestEnumCleanup::Failure => {
                Err(std::io::Error::other("Controlled startup cleanup failure").into())
            }
        }
    }
}

impl EngineProcess for EngineSessionStartupTestProcess {
    async fn send(&mut self, _command: &str) -> Result<(), EngineProcessError> {
        Ok(())
    }

    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        match self.lines.pop_front() {
            Some(line) => EngineProcessLine::new(EngineProcessEnumPipe::Stdout, line),
            None => std::future::pending().await,
        }
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

impl Drop for EngineSessionStartupTestProcess {
    fn drop(&mut self) {
        self.observations.live.fetch_sub(1, Ordering::SeqCst);
        if !self.complete {
            self.observations
                .incomplete_drop
                .store(true, Ordering::SeqCst);
        }
    }
}

#[tokio::test(start_paused = true)]
async fn failed_handshake_retains_process_until_deadline_cleanup_is_retried()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineSessionStartupTestProvider::new(
        EngineSessionStartupTestEnumHandshake::Rejected,
        EngineSessionStartupTestEnumCleanup::Success,
    );
    let mut startup = EngineSessionStartup::new(&provider, fixture.select()?)?;
    assert!(matches!(
        startup.initialize().await,
        Err(EngineSessionError::UnexpectedResponse)
    ));
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 1);
    assert!(!startup.is_shutdown_complete());
    let deadline = Instant::now() + Duration::from_secs(3);
    let error = startup
        .shutdown_retained(deadline)
        .await
        .err()
        .ok_or("Startup cleanup unexpectedly completed")?;
    assert!(error.source().is_some());
    assert_eq!(Instant::now(), deadline);
    assert!(!startup.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 1);
    provider.observations.release();
    let retry = Instant::now() + Duration::from_secs(3);
    startup.shutdown_retained(retry).await?;
    assert!(startup.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 0);
    assert_eq!(provider.observations.shutdown.load(Ordering::SeqCst), 1);
    assert_eq!(provider.observations.terminate.load(Ordering::SeqCst), 1);
    assert_eq!(provider.observations.deadlines()?, [deadline, retry]);
    assert!(!provider.observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_handshake_preserves_owner_and_refuses_a_second_initialization()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineSessionStartupTestProvider::new(
        EngineSessionStartupTestEnumHandshake::Waiting,
        EngineSessionStartupTestEnumCleanup::Success,
    );
    let mut startup = EngineSessionStartup::new(&provider, fixture.select()?)?;
    {
        let mut initialization = std::pin::pin!(startup.initialize());
        assert!(
            initialization
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 1);
    assert!(matches!(
        startup.initialize().await,
        Err(EngineSessionError::StartupUnavailable)
    ));
    provider.observations.release();
    startup
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(startup.is_shutdown_complete());
    assert!(!provider.observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_startup_cleanup_retains_owner_and_retries_forced_cleanup()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineSessionStartupTestProvider::new(
        EngineSessionStartupTestEnumHandshake::Rejected,
        EngineSessionStartupTestEnumCleanup::Success,
    );
    let mut startup = EngineSessionStartup::new(&provider, fixture.select()?)?;
    assert!(startup.initialize().await.is_err());
    let deadline = Instant::now() + Duration::from_secs(3);
    {
        let mut cleanup = std::pin::pin!(startup.shutdown_retained(deadline));
        assert!(
            cleanup
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 1);
    provider.observations.release();
    startup.shutdown_retained(deadline).await?;
    assert_eq!(provider.observations.shutdown.load(Ordering::SeqCst), 1);
    assert_eq!(provider.observations.terminate.load(Ordering::SeqCst), 1);
    assert!(!provider.observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn successful_initialization_transfers_process_once_without_shutting_down_the_session()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let package = fixture.select()?;
    let expected_digest = package.digest();
    let provider = EngineSessionStartupTestProvider::new(
        EngineSessionStartupTestEnumHandshake::Ready,
        EngineSessionStartupTestEnumCleanup::Success,
    );
    let mut startup = EngineSessionStartup::new(&provider, package)?;
    let mut session = startup.initialize().await?;
    assert_eq!(session.identity().name(), "Reckless 0.9.0");
    assert_eq!(session.package().digest(), expected_digest);
    assert!(startup.is_shutdown_complete());
    assert!(matches!(
        startup.initialize().await,
        Err(EngineSessionError::StartupUnavailable)
    ));
    let deadline = Instant::now() + Duration::from_secs(3);
    startup.shutdown_retained(deadline).await?;
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 1);
    assert_eq!(provider.observations.shutdown.load(Ordering::SeqCst), 0);
    provider.observations.release();
    session.shutdown_retained(deadline).await?;
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 0);
    assert!(!provider.observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn completed_startup_cleanup_failure_is_stable_and_preserves_its_source()
-> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineSessionStartupTestProvider::new(
        EngineSessionStartupTestEnumHandshake::Rejected,
        EngineSessionStartupTestEnumCleanup::Failure,
    );
    let mut startup = EngineSessionStartup::new(&provider, fixture.select()?)?;
    assert!(startup.initialize().await.is_err());
    provider.observations.release();
    let deadline = Instant::now() + Duration::from_secs(3);
    for _attempt in 0..2 {
        let error = startup
            .shutdown_retained(deadline)
            .await
            .err()
            .ok_or("The startup cleanup failure was hidden")?;
        assert!(error.source().is_some());
        assert!(
            error
                .to_string()
                .contains("Controlled startup cleanup failure")
        );
        assert!(startup.is_shutdown_complete());
    }
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 0);
    assert_eq!(provider.observations.shutdown.load(Ordering::SeqCst), 1);
    assert_eq!(provider.observations.terminate.load(Ordering::SeqCst), 0);
    assert!(!provider.observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn unobserved_startup_cleanup_does_not_release_the_process() -> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineSessionStartupTestProvider::new(
        EngineSessionStartupTestEnumHandshake::Rejected,
        EngineSessionStartupTestEnumCleanup::Unobserved,
    );
    let mut startup = EngineSessionStartup::new(&provider, fixture.select()?)?;
    assert!(startup.initialize().await.is_err());
    provider.observations.release();
    let deadline = Instant::now() + Duration::from_secs(3);
    assert!(matches!(
        startup.shutdown_retained(deadline).await,
        Err(EngineSessionError::ShutdownUnobserved)
    ));
    assert!(!startup.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 1);
    startup.shutdown_retained(deadline).await?;
    assert!(startup.is_shutdown_complete());
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 0);
    assert_eq!(provider.observations.terminate.load(Ordering::SeqCst), 1);
    assert!(!provider.observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn initialization_timeout_retains_process_for_explicit_cleanup() -> Result<(), Box<dyn Error>>
{
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineSessionStartupTestProvider::new(
        EngineSessionStartupTestEnumHandshake::Waiting,
        EngineSessionStartupTestEnumCleanup::Success,
    );
    let mut startup = EngineSessionStartup::new(&provider, fixture.select()?)?;
    assert!(matches!(
        startup.initialize().await,
        Err(EngineSessionError::Timeout("uci"))
    ));
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 1);
    provider.observations.release();
    startup
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(startup.is_shutdown_complete());
    assert!(!provider.observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn closing_before_initialization_prevents_a_handshake() -> Result<(), Box<dyn Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineSessionStartupTestProvider::new(
        EngineSessionStartupTestEnumHandshake::Ready,
        EngineSessionStartupTestEnumCleanup::Success,
    );
    let mut startup = EngineSessionStartup::new(&provider, fixture.select()?)?;
    provider.observations.release();
    startup
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(matches!(
        startup.initialize().await,
        Err(EngineSessionError::StartupUnavailable)
    ));
    assert_eq!(provider.observations.live.load(Ordering::SeqCst), 0);
    assert!(!provider.observations.incomplete_drop.load(Ordering::SeqCst));
    Ok(())
}
