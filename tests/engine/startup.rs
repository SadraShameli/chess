use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chess::engine::capabilities::{EngineCapabilities, EngineIdentity};
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::selector::EnginePackageSelected;
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
    EngineProcessProvider,
};
use chess::engine::session::{EngineSession, EngineSessionError};

use crate::engine_package_selection::EnginePackageFixture;

struct EngineStartupProvider {
    lines: VecDeque<String>,
    commands: Arc<Mutex<Vec<String>>>,
}

impl EngineStartupProvider {
    fn new() -> Self {
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
        Self {
            lines,
            commands: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn commands(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        Ok(self
            .commands
            .lock()
            .map_err(|_error| "command lock poisoned")?
            .clone())
    }
}

impl EngineProcessProvider for EngineStartupProvider {
    type Process = EngineStartupProcess;
    fn spawn(&self, _package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        Ok(EngineStartupProcess {
            lines: self.lines.clone(),
            commands: Arc::clone(&self.commands),
            shutdown_complete: false,
        })
    }
}

struct EngineStartupProcess {
    lines: VecDeque<String>,
    commands: Arc<Mutex<Vec<String>>>,
    shutdown_complete: bool,
}

impl EngineStartupProcess {
    fn record(&mut self, command: &str) -> Result<(), EngineProcessError> {
        self.commands
            .lock()
            .map_err(|_error| std::io::Error::other("command lock poisoned"))?
            .push(command.to_owned());
        Ok(())
    }
}

impl EngineProcess for EngineStartupProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        self.record(command)
    }
    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        match self.lines.pop_front() {
            Some(text) => EngineProcessLine::new(EngineProcessEnumPipe::Stdout, text),
            None => std::future::pending().await,
        }
    }
    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.record("shutdown")?;
        self.shutdown_complete = true;
        Ok(())
    }
    async fn shutdown_retained(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<(), EngineProcessError> {
        tokio::time::timeout_at(deadline, self.shutdown())
            .await
            .map_err(EngineProcessError::ShutdownDeadline)?
    }
    fn is_shutdown_complete(&self) -> bool {
        self.shutdown_complete
    }
    async fn terminate(
        &mut self,
        _deadline: tokio::time::Instant,
    ) -> Result<(), EngineProcessError> {
        self.record("terminate")?;
        self.shutdown_complete = true;
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn startup_discovers_the_full_pinned_registry_and_orders_readiness()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let provider = EngineStartupProvider::new();
    let session = EngineSession::start(&provider, fixture.select()?).await?;
    assert_eq!(session.identity().name(), "Reckless 0.9.0");
    assert_eq!(
        session.identity().capabilities().options().len(),
        EngineEnumOption::ALL.len()
    );
    assert_eq!(provider.commands()?, ["uci", "isready"]);
    session.shutdown().await?;
    assert_eq!(provider.commands()?, ["uci", "isready", "shutdown"]);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn each_handshake_has_its_own_deadline_and_observed_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    for (truncate, stage) in [(0, "uci"), (11, "readiness")] {
        let fixture = EnginePackageFixture::new()?;
        let mut provider = EngineStartupProvider::new();
        provider.lines.truncate(truncate);
        let result = EngineSession::start(&provider, fixture.select()?).await;
        assert!(
            matches!(result.as_ref().err().map(EngineSessionError::operation), Some(EngineSessionError::Timeout(actual)) if *actual == stage)
        );
        assert_eq!(
            provider.commands()?.last().map(String::as_str),
            Some("shutdown")
        );
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn startup_rejects_wrong_identity_and_incomplete_or_changed_capabilities()
-> Result<(), Box<dyn std::error::Error>> {
    for (index, replacement) in [
        (0, "id name another engine"),
        (1, "id author another author"),
        (2, "option name Hash type spin default 16 min 1 max 2048"),
        (2, "option name Unknown type check default false"),
        (2, "uciok"),
    ] {
        let fixture = EnginePackageFixture::new()?;
        let mut provider = EngineStartupProvider::new();
        *provider
            .lines
            .get_mut(index)
            .ok_or("missing transcript line")? = replacement.to_owned();
        assert!(
            EngineSession::start(&provider, fixture.select()?)
                .await
                .is_err()
        );
        assert_eq!(
            provider.commands()?.last().map(String::as_str),
            Some("shutdown")
        );
    }
    Ok(())
}

#[test]
fn duplicate_options_and_changed_multipv_bound_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let mut capabilities = EngineCapabilities::default();
    capabilities.discover("option name MultiPV type spin default 1 min 1 max 256")?;
    assert!(
        capabilities
            .discover("option name MultiPV type spin default 1 min 1 max 256")
            .is_err()
    );
    assert!(
        EngineCapabilities::default()
            .discover("option name MultiPV type spin default 1 min 1 max 218")
            .is_err()
    );
    Ok(())
}
