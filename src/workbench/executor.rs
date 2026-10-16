use std::sync::Arc;

use thiserror::Error;
use tokio::time::{Instant, timeout_at};

use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::engine::capabilities::EngineIdentity;
use crate::engine::diagnostic::EngineDiagnosticEnumCommand;
use crate::engine::diagnostics::EngineDiagnostics;
use crate::engine::enums::EngineEnumOption;
use crate::engine::options::EngineOptionsApplied;
use crate::engine::package::selector::EnginePackageSelected;
use crate::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessLimits, EngineProcessProvider,
};
use crate::engine::resource_config::EngineResourceConfig;
use crate::engine::search::identity::EngineSearchGeneration;
use crate::engine::session::{EngineSession, EngineSessionError, EngineSessionStartup};
use crate::workbench::enums::{
    WorkbenchEnumCompletion, WorkbenchEnumJobState, WorkbenchEnumParameters,
};
use crate::workbench::job::{WorkbenchJob, WorkbenchJobError, WorkbenchJobFailure, WorkbenchJobId};
use crate::workbench::output::WorkbenchOutput;

#[derive(Debug, Error)]
pub enum WorkbenchExecutorError {
    #[error(transparent)]
    Job(#[from] WorkbenchJobError),
    #[error(transparent)]
    Time(#[from] ClockDurationError),
    #[error("The workbench executor cannot perform {0} in its current state")]
    State(&'static str),
    #[error("The isolated child and pipe readers have not been reaped: {0}")]
    Cleanup(#[source] Arc<EngineSessionError>),
}

enum WorkbenchExecutorEnumChild<P: EngineProcess> {
    Empty,
    Startup(EngineSessionStartup<P>),
    Session(Box<EngineSession<P>>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkbenchExecutorEnumState {
    Ready,
    SettingUp,
    Running,
    Cleaning,
    Finished,
}

#[derive(Debug, Clone)]
pub struct WorkbenchExecutorObservation {
    job_id: WorkbenchJobId,
    package: EnginePackageSelected,
    identity: EngineIdentity,
    applied_options: EngineOptionsApplied,
    generation: EngineSearchGeneration,
}

impl WorkbenchExecutorObservation {
    pub const fn job_id(&self) -> WorkbenchJobId {
        self.job_id
    }

    pub fn package(&self) -> &EnginePackageSelected {
        &self.package
    }

    pub fn identity(&self) -> &EngineIdentity {
        &self.identity
    }

    pub fn applied_options(&self) -> &EngineOptionsApplied {
        &self.applied_options
    }

    pub const fn generation(&self) -> EngineSearchGeneration {
        self.generation
    }
}

pub struct WorkbenchExecutor<P: EngineProcess> {
    job: WorkbenchJob,
    child: WorkbenchExecutorEnumChild<P>,
    state: WorkbenchExecutorEnumState,
    anchor: Instant,
    observed: ClockInstant,
    deadline: Instant,
    output: WorkbenchOutput,
    completion: Option<WorkbenchEnumCompletion>,
    cleanup_failure: Option<Arc<EngineSessionError>>,
    run_cleanup_deadline: Option<Instant>,
    observation: Option<WorkbenchExecutorObservation>,
}

impl<P: EngineProcess> WorkbenchExecutor<P> {
    pub fn new(job: WorkbenchJob, now: ClockInstant) -> Result<Self, WorkbenchExecutorError> {
        if job.state() != WorkbenchEnumJobState::Admitted {
            return Err(WorkbenchExecutorError::State("admit before constructing"));
        }
        now.elapsed_since(job.observed_at())?;
        let anchor = Instant::now();
        let remaining = if now.has_reached(job.deadline()) {
            ClockDuration::ZERO
        } else {
            job.deadline().elapsed_since(now)?
        };
        Ok(Self {
            job,
            child: WorkbenchExecutorEnumChild::Empty,
            state: WorkbenchExecutorEnumState::Ready,
            anchor,
            observed: now,
            deadline: anchor + remaining.as_duration(),
            output: WorkbenchOutput::default(),
            completion: None,
            cleanup_failure: None,
            run_cleanup_deadline: None,
            observation: None,
        })
    }

    pub fn job(&self) -> &WorkbenchJob {
        &self.job
    }

    pub fn observation(&self) -> Option<&WorkbenchExecutorObservation> {
        self.observation.as_ref()
    }

    pub fn diagnostics(&self) -> &EngineDiagnostics {
        match &self.child {
            WorkbenchExecutorEnumChild::Empty => self.job.diagnostics(),
            WorkbenchExecutorEnumChild::Startup(owner) => owner.diagnostics(),
            WorkbenchExecutorEnumChild::Session(session) => session.diagnostics(),
        }
    }

    pub fn is_shutdown_complete(&self) -> bool {
        match &self.child {
            WorkbenchExecutorEnumChild::Empty => true,
            WorkbenchExecutorEnumChild::Startup(owner) => owner.is_shutdown_complete(),
            WorkbenchExecutorEnumChild::Session(session) => session.is_shutdown_complete(),
        }
    }

    pub async fn start(
        &mut self,
        provider: &impl EngineProcessProvider<Process = P>,
    ) -> Result<(), WorkbenchExecutorError> {
        if self.state != WorkbenchExecutorEnumState::Ready {
            return Err(WorkbenchExecutorError::State("start"));
        }
        let now = self.now()?;
        if now.has_reached(self.job.deadline()) {
            self.job.expire(self.job.id(), now)?;
            self.state = WorkbenchExecutorEnumState::Finished;
            return Ok(());
        }
        self.job.start(self.job.id(), now)?;
        self.state = WorkbenchExecutorEnumState::SettingUp;
        match EngineSessionStartup::new(provider, self.job.package().clone()) {
            Ok(owner) => self.child = WorkbenchExecutorEnumChild::Startup(owner),
            Err(error) => {
                self.completion = Some(WorkbenchEnumCompletion::Failed(error.into()));
                return self
                    .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
                    .await;
            }
        }
        match timeout_at(self.deadline, self.setup()).await {
            Ok(Ok(())) => {
                self.state = WorkbenchExecutorEnumState::Running;
                Ok(())
            }
            Ok(Err(error)) => {
                self.completion = Some(WorkbenchEnumCompletion::Failed(error.into()));
                self.shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
                    .await
            }
            Err(_elapsed) => {
                self.job.expire(self.job.id(), self.now()?)?;
                self.shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
                    .await
            }
        }
    }

    async fn setup(&mut self) -> Result<(), EngineSessionError> {
        let WorkbenchExecutorEnumChild::Startup(owner) = &mut self.child else {
            return Err(EngineSessionError::StartupUnavailable);
        };
        let session = owner.initialize().await?;
        self.child = WorkbenchExecutorEnumChild::Session(Box::new(session));
        let WorkbenchExecutorEnumChild::Session(session) = &mut self.child else {
            return Err(EngineSessionError::StartupUnavailable);
        };
        let capabilities = session.identity().capabilities();
        let default_hash =
            EngineResourceConfig::advertised_default(capabilities, EngineEnumOption::Hash)?;
        let overhead = ClockDuration::from_millis(u64::from(
            EngineResourceConfig::advertised_default(capabilities, EngineEnumOption::MoveOverhead)?,
        ))
        .map_err(crate::engine::resource_config::EngineResourceConfigError::from)?;
        let (hash_mib, workers) = match self.job.parameters() {
            WorkbenchEnumParameters::Bench(parameters) => {
                (parameters.hash_mib(), parameters.workers())
            }
            WorkbenchEnumParameters::Inspector(_)
            | WorkbenchEnumParameters::RawEvaluation(_)
            | WorkbenchEnumParameters::Compiler
            | WorkbenchEnumParameters::Perft(_) => (default_hash, self.job.parameters().workers()),
        };
        session
            .configure_resources(EngineResourceConfig::new(workers, hash_mib, overhead)?)
            .await?;
        let command = match self.job.parameters() {
            WorkbenchEnumParameters::Inspector(position) => {
                EngineDiagnosticEnumCommand::Inspector(position)
            }
            WorkbenchEnumParameters::RawEvaluation(position) => {
                EngineDiagnosticEnumCommand::RawEvaluation(position)
            }
            WorkbenchEnumParameters::Compiler => EngineDiagnosticEnumCommand::Compiler,
            WorkbenchEnumParameters::Perft(parameters) => EngineDiagnosticEnumCommand::Perft {
                position: parameters.position(),
                depth: parameters.depth().value(),
            },
            WorkbenchEnumParameters::Bench(parameters) => EngineDiagnosticEnumCommand::Bench {
                hash_mib: parameters.hash_mib(),
                threads: parameters.threads(),
                depth: parameters.depth(),
            },
        };
        let applied_options = session.begin_diagnostic(command).await?;
        self.observation = Some(WorkbenchExecutorObservation {
            job_id: self.job.id(),
            package: session.package().clone(),
            identity: session.identity().clone(),
            applied_options,
            generation: session.generation(),
        });
        Ok(())
    }

    pub async fn run(&mut self) -> Result<(), WorkbenchExecutorError> {
        if self.state == WorkbenchExecutorEnumState::Cleaning {
            let deadline = self
                .run_cleanup_deadline
                .ok_or(WorkbenchExecutorError::State("resume result cleanup"))?;
            return self.shutdown_retained(deadline).await;
        }
        if self.state != WorkbenchExecutorEnumState::Running {
            return Err(WorkbenchExecutorError::State("run"));
        }
        loop {
            let WorkbenchExecutorEnumChild::Session(session) = &mut self.child else {
                return Err(WorkbenchExecutorError::State("receive"));
            };
            match timeout_at(self.deadline, session.receive_diagnostic()).await {
                Ok(Ok(line)) => {
                    let now = self.now()?;
                    self.job.observe(self.job.id(), now)?;
                    if now.has_reached(self.job.deadline()) {
                        self.job.expire(self.job.id(), now)?;
                        break;
                    }
                    if line.pipe() == EngineProcessEnumPipe::Stderr {
                        continue;
                    }
                    match self.output.consume(
                        self.job.parameters(),
                        self.job.package().target(),
                        line.text(),
                    ) {
                        Ok(Some(result)) => {
                            self.completion = Some(WorkbenchEnumCompletion::Result(result));
                            let WorkbenchExecutorEnumChild::Session(session) = &mut self.child
                            else {
                                return Err(WorkbenchExecutorError::State("complete"));
                            };
                            session.complete_diagnostic().map_err(|error| {
                                WorkbenchExecutorError::Cleanup(Arc::new(error))
                            })?;
                            break;
                        }
                        Ok(None) => {
                            let positions = self.output.positions();
                            if positions > self.job.progress().completed() {
                                self.job.advance(self.job.id(), positions, now)?;
                            }
                        }
                        Err(error) => {
                            self.completion = Some(WorkbenchEnumCompletion::Failed(error));
                            break;
                        }
                    }
                }
                Ok(Err(error)) => {
                    self.completion = Some(WorkbenchEnumCompletion::Failed(error.into()));
                    break;
                }
                Err(_elapsed) => {
                    self.job.expire(self.job.id(), self.now()?)?;
                    break;
                }
            }
        }
        let deadline = *self
            .run_cleanup_deadline
            .get_or_insert_with(|| Instant::now() + EngineProcessLimits::SHUTDOWN);
        self.shutdown_retained(deadline).await
    }

    pub async fn cancel(&mut self, id: WorkbenchJobId) -> Result<(), WorkbenchExecutorError> {
        self.job.cancel(id, self.now()?)?;
        if self.is_shutdown_complete() && self.job.outcome().is_some() {
            self.state = WorkbenchExecutorEnumState::Finished;
            return Ok(());
        }
        self.shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
            .await
    }

    pub async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), WorkbenchExecutorError> {
        if self.state == WorkbenchExecutorEnumState::Finished {
            return Ok(());
        }
        if self.job.state() == WorkbenchEnumJobState::Admitted {
            self.job.cancel(self.job.id(), self.now()?)?;
            self.state = WorkbenchExecutorEnumState::Finished;
            return Ok(());
        }
        self.state = WorkbenchExecutorEnumState::Cleaning;
        let deadline = deadline.min(Instant::now() + EngineProcessLimits::SHUTDOWN);
        if self.completion.is_none()
            && let WorkbenchExecutorEnumChild::Session(session) = &mut self.child
            && let Some(failure) = session.take_pending_failure()
        {
            self.completion = Some(WorkbenchEnumCompletion::Failed(failure.into()));
        }
        let graceful = self.job.cancellation().is_none()
            && matches!(self.completion, Some(WorkbenchEnumCompletion::Result(_)));
        let result = match &mut self.child {
            WorkbenchExecutorEnumChild::Empty => Ok(()),
            WorkbenchExecutorEnumChild::Startup(owner) => owner.terminate_retained(deadline).await,
            WorkbenchExecutorEnumChild::Session(session) => {
                if graceful {
                    session.shutdown_retained(deadline).await
                } else {
                    session.terminate_retained(deadline).await
                }
            }
        };
        if let Err(error) = result {
            self.cleanup_failure.get_or_insert_with(|| Arc::new(error));
        }
        if !self.is_shutdown_complete() {
            let failure = self
                .cleanup_failure
                .get_or_insert_with(|| Arc::new(EngineSessionError::ShutdownUnobserved));
            return Err(WorkbenchExecutorError::Cleanup(Arc::clone(failure)));
        }
        self.transfer_diagnostics()?;
        let mut completion = self
            .completion
            .take()
            .unwrap_or(WorkbenchEnumCompletion::Terminated);
        if let Some(cleanup) = self.cleanup_failure.take() {
            let cleanup = EngineSessionError::ShutdownFailed(cleanup);
            completion = WorkbenchEnumCompletion::Failed(match completion {
                WorkbenchEnumCompletion::Failed(operation) => WorkbenchJobFailure::SessionCleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                },
                WorkbenchEnumCompletion::Result(_) | WorkbenchEnumCompletion::Terminated => {
                    WorkbenchJobFailure::Session(cleanup)
                }
            });
        }
        self.job
            .finish_reaped(self.job.id(), completion, self.now()?)?;
        self.child = WorkbenchExecutorEnumChild::Empty;
        self.state = WorkbenchExecutorEnumState::Finished;
        Ok(())
    }

    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        self.observed
            .checked_add(ClockDuration::try_from(self.anchor.elapsed())?)
    }

    fn transfer_diagnostics(&mut self) -> Result<(), WorkbenchExecutorError> {
        let now = self.now()?;
        let diagnostics = match &mut self.child {
            WorkbenchExecutorEnumChild::Empty => return Ok(()),
            WorkbenchExecutorEnumChild::Startup(owner) => owner.take_diagnostics(),
            WorkbenchExecutorEnumChild::Session(session) => session.take_diagnostics(),
        };
        self.job
            .retain_diagnostics(self.job.id(), diagnostics, now)?;
        Ok(())
    }
}
