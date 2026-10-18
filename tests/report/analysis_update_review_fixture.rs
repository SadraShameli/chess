use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use strum::IntoEnumIterator;

use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::ClockDuration;
use chess::engine::capabilities::EngineIdentity;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::selector::EnginePackageSelected;
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLimits,
    EngineProcessLine, EngineProcessProvider,
};
use chess::engine::resource_config::EngineResourceConfig;
use chess::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumEvent, EngineSearchSessionRequest,
    EngineSearchSessionTerminal,
};
use chess::engine::session::EngineSession;
use chess::report::job::ReportJob;
use tokio::time::{Instant, timeout};

use crate::report_job::fixture::{ReportFixture, ReportFixtureResult};

pub(super) struct ReportAnalysisUpdateReviewScript {
    pub(super) before: &'static [&'static str],
    pub(super) after: &'static [&'static str],
}

pub(super) struct ReportAnalysisUpdateReviewRun {
    pub(super) job: ReportJob,
    pub(super) terminals: [EngineSearchSessionTerminal; 2],
    pub(super) commands: Vec<String>,
    pub(super) shutdown_complete: bool,
    pub(super) allocation: Option<usize>,
    pub(super) available: usize,
}

struct ReportAnalysisUpdateReviewProvider {
    process: Mutex<Option<ReportAnalysisUpdateReviewProcess>>,
}

impl EngineProcessProvider for ReportAnalysisUpdateReviewProvider {
    type Process = ReportAnalysisUpdateReviewProcess;

    fn spawn(&self, _package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        self.process
            .lock()
            .map_err(|_error| std::io::Error::other("Review process lock poisoned"))?
            .take()
            .ok_or(EngineProcessError::MissingPipe)
    }
}

struct ReportAnalysisUpdateReviewProcess {
    identity: EngineIdentity,
    scripts: VecDeque<&'static [&'static str]>,
    lines: VecDeque<EngineProcessLine>,
    commands: Arc<Mutex<Vec<String>>>,
    shutdown_complete: bool,
}

impl ReportAnalysisUpdateReviewProcess {
    fn emit(&mut self, text: String) -> Result<(), EngineProcessError> {
        if self.lines.len() >= EngineProcessLimits::PENDING_LINES {
            return Err(std::io::Error::other("Review response queue is full").into());
        }
        self.lines
            .push_back(EngineProcessLine::new(EngineProcessEnumPipe::Stdout, text)?);
        Ok(())
    }

    fn record(&self, command: &str) -> Result<(), EngineProcessError> {
        self.commands
            .lock()
            .map_err(|_error| std::io::Error::other("Review command lock poisoned"))?
            .push(command.to_owned());
        Ok(())
    }
}

impl EngineProcess for ReportAnalysisUpdateReviewProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        self.record(command)?;
        if command == "uci" {
            self.emit(format!("id name {}", self.identity.name()))?;
            self.emit(format!("id author {}", self.identity.author()))?;
            for option in EngineEnumOption::iter() {
                self.emit(format!(
                    "option name {} type {}",
                    option.name(),
                    option.descriptor()
                ))?;
            }
            self.emit("uciok".to_owned())?;
        } else if command == "isready" {
            self.emit("readyok".to_owned())?;
        } else if let Some(setting) = command.strip_prefix("setoption name ") {
            let (name, value) = setting
                .split_once(" value ")
                .ok_or(EngineProcessError::InvalidCommand)?;
            let unit = if name == EngineEnumOption::Hash.name() {
                " MB"
            } else if name == EngineEnumOption::MoveOverhead.name() {
                " ms"
            } else {
                ""
            };
            self.emit(format!("info string set {name} to {value}{unit}"))?;
        } else if command.starts_with("go ") {
            let script = self
                .scripts
                .pop_front()
                .ok_or_else(|| std::io::Error::other("Missing review search transcript"))?;
            for line in script {
                self.emit((*line).to_owned())?;
            }
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        self.lines
            .pop_front()
            .ok_or(EngineProcessError::Closed(EngineProcessEnumPipe::Stdout))
    }

    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.record("shutdown")?;
        self.shutdown_complete = true;
        Ok(())
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }

    fn is_shutdown_complete(&self) -> bool {
        self.shutdown_complete
    }

    async fn terminate(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }
}

impl ReportAnalysisUpdateReviewRun {
    pub(super) async fn execute(
        script: ReportAnalysisUpdateReviewScript,
    ) -> ReportFixtureResult<Self> {
        let fixture = ReportFixture::new().await?;
        let setup = async {
            let job = fixture.job(&["e2e4"])?;
            let capacity =
                CapacityBudget::new(NonZeroUsize::new(1).ok_or("Empty review capacity")?)?;
            let commands = Arc::new(Mutex::new(Vec::new()));
            let provider = ReportAnalysisUpdateReviewProvider {
                process: Mutex::new(Some(ReportAnalysisUpdateReviewProcess {
                    identity: fixture.session().engine().identity().clone(),
                    scripts: VecDeque::from([script.before, script.after]),
                    lines: VecDeque::new(),
                    commands: Arc::clone(&commands),
                    shutdown_complete: false,
                })),
            };
            let engine =
                EngineSession::start(&provider, fixture.session().engine().package().clone())
                    .await?;
            let mut session = EngineSearchSession::new(engine);
            let outcome = Self::analyze(&mut session, &capacity, job).await;
            let cleanup = session
                .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
                .await;
            let shutdown_complete = session.engine().is_shutdown_complete();
            let allocation = session.allocation();
            let available = capacity.available();
            drop(session);
            drop(provider);
            cleanup?;
            let (job, terminals) = outcome?;
            let commands = commands
                .lock()
                .map_err(|_error| std::io::Error::other("Review command lock poisoned"))?
                .clone();
            Ok(Self {
                job,
                terminals,
                commands,
                shutdown_complete,
                allocation,
                available,
            })
        }
        .await;
        let cleanup = fixture.close().await;
        cleanup?;
        setup
    }

    async fn analyze(
        session: &mut EngineSearchSession<ReportAnalysisUpdateReviewProcess>,
        capacity: &CapacityBudget,
        mut job: ReportJob,
    ) -> ReportFixtureResult<(ReportJob, [EngineSearchSessionTerminal; 2])> {
        session
            .idle_engine()?
            .configure_resources(EngineResourceConfig::new(1, 16, ClockDuration::ZERO)?)
            .await?;
        job.resume()?;
        let before = Self::complete(session, capacity, &mut job, 0).await?;
        let after = Self::complete(session, capacity, &mut job, 1).await?;
        Ok((job, [before, after]))
    }

    async fn complete(
        session: &mut EngineSearchSession<ReportAnalysisUpdateReviewProcess>,
        capacity: &CapacityBudget,
        job: &mut ReportJob,
        index: u32,
    ) -> ReportFixtureResult<EngineSearchSessionTerminal> {
        let attempt = job.start_position(index)?;
        let tag = session
            .start(EngineSearchSessionRequest::new(
                attempt.context(),
                attempt.position().clone(),
                attempt.limit(),
                capacity.try_lease(1)?,
            ))
            .await?;
        let binding = job.bind_engine(&attempt, session)?;
        for _ in 0..8 {
            if let EngineSearchSessionEnumEvent::Finished(terminal) =
                timeout(EngineProcessLimits::SHUTDOWN, session.next_event(tag)).await??
            {
                job.record_terminal(&binding, &terminal)?;
                return Ok(terminal);
            }
        }
        Err("Review search did not deliver its terminal within eight events".into())
    }
}
