use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use chess::clock::duration::ClockDuration;
use chess::engine::package::selector::EnginePackageSelected;
use chess::engine::process_provider::{
    EngineProcess, EngineProcessError, EngineProcessLine, EngineProcessProvider,
};
use chess::engine::resource_config::EngineResourceConfig;
use chess::engine::session::EngineSession;
use tokio::time::Instant;

use crate::engine_lifecycle::{EngineLifecycleProcess, EngineLifecycleProvider};
use crate::engine_package_selection::EnginePackageFixture;
use crate::engine_report_persistence_codecs::EngineReportPersistenceTestResult;

pub(super) struct EngineReportPersistenceRuntimeFixture;

impl EngineReportPersistenceRuntimeFixture {
    pub(super) async fn configure<P: EngineProcess>(
        engine: &mut EngineSession<P>,
        provider: &mut EngineLifecycleProvider,
        threads: u32,
    ) -> EngineReportPersistenceTestResult {
        for line in [
            format!("info string set Threads to {threads}"),
            "info string set Hash to 32 MB".to_owned(),
            "info string set MoveOverhead to 0 ms".to_owned(),
        ] {
            provider.emit(&line).await?;
        }
        engine
            .configure_resources(EngineResourceConfig::new(
                usize::try_from(threads)?,
                32,
                ClockDuration::ZERO,
            )?)
            .await?;
        for command in [
            format!("setoption name Threads value {threads}"),
            "setoption name Hash value 32".to_owned(),
            "setoption name MoveOverhead value 0".to_owned(),
            "isready".to_owned(),
        ] {
            provider.expect(&command).await?;
        }
        Ok(())
    }
}

pub(super) struct EngineReportPersistenceFailureProvider {
    pub(super) lifecycle: EngineLifecycleProvider,
    receive_failure: Arc<AtomicBool>,
    cleanup_failure: Arc<AtomicBool>,
}

impl EngineReportPersistenceFailureProvider {
    pub(super) fn new() -> Self {
        Self {
            lifecycle: EngineLifecycleProvider::new(),
            receive_failure: Arc::new(AtomicBool::new(false)),
            cleanup_failure: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) async fn start(
        &mut self,
        package: &EnginePackageFixture,
    ) -> EngineReportPersistenceTestResult<EngineSession<EngineReportPersistenceFailureProcess>>
    {
        let engine = EngineSession::start(self, package.select()?).await?;
        self.lifecycle.expect("uci").await?;
        self.lifecycle.expect("isready").await?;
        Ok(engine)
    }

    pub(super) fn fail_receive(&self) {
        self.receive_failure.store(true, Ordering::SeqCst);
    }
    pub(super) fn fail_cleanup(&self) {
        self.cleanup_failure.store(true, Ordering::SeqCst);
    }
}

impl EngineProcessProvider for EngineReportPersistenceFailureProvider {
    type Process = EngineReportPersistenceFailureProcess;

    fn spawn(&self, package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        Ok(EngineReportPersistenceFailureProcess {
            inner: self.lifecycle.spawn(package)?,
            receive_failure: Arc::clone(&self.receive_failure),
            cleanup_failure: Arc::clone(&self.cleanup_failure),
        })
    }
}

pub(super) struct EngineReportPersistenceFailureProcess {
    inner: EngineLifecycleProcess,
    receive_failure: Arc<AtomicBool>,
    cleanup_failure: Arc<AtomicBool>,
}

impl EngineReportPersistenceFailureProcess {
    fn cleanup_result(&self) -> Result<(), EngineProcessError> {
        if self.cleanup_failure.load(Ordering::SeqCst) {
            return Err(std::io::Error::from_raw_os_error(5).into());
        }
        Ok(())
    }
}

impl EngineProcess for EngineReportPersistenceFailureProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        self.inner.send(command).await
    }
    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        if self.receive_failure.load(Ordering::SeqCst) {
            return Err(std::io::Error::from_raw_os_error(5).into());
        }
        self.inner.receive().await
    }
    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.inner.shutdown().await?;
        self.cleanup_result()
    }
    async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        self.inner.shutdown_retained(deadline).await?;
        self.cleanup_result()
    }
    fn is_shutdown_complete(&self) -> bool {
        self.inner.is_shutdown_complete()
    }
    async fn terminate(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        self.inner.terminate(deadline).await?;
        self.cleanup_result()
    }
}
