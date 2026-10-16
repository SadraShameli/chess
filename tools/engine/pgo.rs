use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use chess::board::enums::BoardEnumColor;
use chess::board::position::{BoardPosition, BoardPositionError};
use chess::capacity::budget::{CapacityBudget, CapacityBudgetError};
use chess::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use chess::engine::capabilities::{EngineCapabilities, EngineCapabilitiesError};
use chess::engine::diagnostics::EngineDiagnostics;
use chess::engine::package::host::{EnginePackageHost, EnginePackageHostError};
use chess::engine::package::manifest::{
    EnginePackageEnumTarget, EnginePackageManifest, EnginePackageManifestError,
};
use chess::engine::package::selector::{EnginePackageSelector, EnginePackageSelectorError};
use chess::engine::process_provider::EngineProcessLimits;
use chess::engine::search::position::{EngineSearchPosition, EngineSearchPositionError};
use chess::package::digest::{PackageDigest, PackageDigestError};
use chess::workbench::parameters::{WorkbenchBenchParameters, WorkbenchParametersError};
use serde::Serialize;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::watch;
use tokio::time::Instant;

#[derive(Debug, Error)]
pub(crate) enum ToolsEnginePgoError {
    #[error(transparent)]
    Capacity(#[from] CapacityBudgetError),
    #[error(transparent)]
    Executor(#[from] CapacityExecutorError),
    #[error(transparent)]
    Manifest(#[from] EnginePackageManifestError),
    #[error(transparent)]
    Host(#[from] EnginePackageHostError),
    #[error(transparent)]
    Package(#[from] EnginePackageSelectorError),
    #[error(transparent)]
    Digest(#[from] PackageDigestError),
    #[error(transparent)]
    Capabilities(#[from] EngineCapabilitiesError),
    #[error(transparent)]
    Position(#[from] EngineSearchPositionError),
    #[error(transparent)]
    Board(#[from] BoardPositionError),
    #[error(transparent)]
    Parameters(#[from] WorkbenchParametersError),
    #[error("Developer workflow I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Developer workflow report encoding failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Paths and arguments must be UTF-8 without control characters")]
    InvalidPath,
    #[error("The diagnostic output directory must be outside the source tree")]
    OutputOverlap,
    #[error("Source HEAD, clean tree, source root or model differs from the approved inputs")]
    SourceMismatch,
    #[error("The pinned Cargo.lock changed during the workflow")]
    LockChanged,
    #[error("The compiler differs from the project's pinned stable version")]
    CompilerMismatch,
    #[error("Diagnostic output exceeded its bound or was not valid UTF-8")]
    OutputBound,
    #[error("Developer workflow was cancelled")]
    Cancelled,
    #[error("Developer workflow exceeded its stage deadline")]
    Deadline,
    #[error(
        "Diagnostic command {stage:?} failed with {status} (exit code {code:?}); see its exact receipt"
    )]
    Command {
        stage: ToolsEnginePgoEnumStage,
        code: Option<i32>,
        status: String,
    },
    #[error("Child cleanup failed: {0}")]
    Cleanup(String),
    #[error("The engine response did not satisfy the controlled comparison")]
    Comparison,
    #[error("No nonempty raw profile was produced by instrumented training")]
    MissingProfile,
    #[error("The diagnostic child or its unacknowledged receipt is still owned")]
    ActiveProcess,
    #[error("Diagnostic command {stage:?} failed: {message}")]
    Operation {
        stage: ToolsEnginePgoEnumStage,
        message: String,
    },
    #[error("Workflow failed ({operation}); retained cleanup also failed ({cleanup})")]
    WorkflowCleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
    #[error("Workflow failed ({operation}); report publication also failed ({publication})")]
    ReportPublication {
        #[source]
        operation: Box<Self>,
        publication: Box<Self>,
    },
    #[error("Build failed ({operation}); input revalidation also failed ({validation})")]
    InputValidation {
        #[source]
        operation: Box<Self>,
        validation: Box<Self>,
    },
    #[error("Forced cleanup recorded {observations} failed observations: {first}")]
    ForcedDrain {
        observations: u64,
        #[source]
        first: Box<Self>,
    },
}

impl ToolsEnginePgoError {
    fn publication(operation: Option<Self>, publication: Self) -> Self {
        match operation {
            Some(operation) => Self::ReportPublication {
                operation: Box::new(operation),
                publication: Box::new(publication),
            },
            None => publication,
        }
    }
}

pub(crate) struct ToolsEnginePgoCancellation(watch::Sender<bool>);

impl ToolsEnginePgoCancellation {
    pub(crate) fn new() -> Self {
        Self(watch::channel(false).0)
    }

    pub(crate) fn cancel(&self) {
        self.0.send_replace(true);
    }

    async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        drop(receiver.wait_for(|cancelled| *cancelled).await);
    }

    fn check(&self) -> Result<(), ToolsEnginePgoError> {
        if *self.0.borrow() {
            Err(ToolsEnginePgoError::Cancelled)
        } else {
            Ok(())
        }
    }
}

impl Drop for ToolsEnginePgoCancellation {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ToolsEnginePgoResources<'a> {
    budget: &'a CapacityBudget,
    executor: &'a CapacityExecutor,
    cancellation: &'a ToolsEnginePgoCancellation,
}

impl<'a> ToolsEnginePgoResources<'a> {
    pub(crate) fn new(
        budget: &'a CapacityBudget,
        executor: &'a CapacityExecutor,
        cancellation: &'a ToolsEnginePgoCancellation,
    ) -> Self {
        Self {
            budget,
            executor,
            cancellation,
        }
    }
}

pub(crate) struct ToolsEnginePgoInputs {
    source: PathBuf,
    model: PathBuf,
    output: PathBuf,
    instrumentation: ToolsEnginePgoEnumInstrumentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolsEnginePgoEnumInstrumentation {
    Full,
    CountsOnly,
}

impl ToolsEnginePgoInputs {
    pub(crate) fn new(source: PathBuf, model: PathBuf, output: PathBuf) -> Self {
        Self {
            source,
            model,
            output,
            instrumentation: ToolsEnginePgoEnumInstrumentation::Full,
        }
    }

    pub(crate) fn with_instrumentation(
        mut self,
        instrumentation: ToolsEnginePgoEnumInstrumentation,
    ) -> Self {
        self.instrumentation = instrumentation;
        self
    }

    pub(crate) fn instrumentation(&self) -> ToolsEnginePgoEnumInstrumentation {
        self.instrumentation
    }

    pub(crate) async fn claim_output(&self) -> Result<PathBuf, ToolsEnginePgoError> {
        ToolsEnginePgoProcess::text(self.output.as_os_str())?;
        if tokio::fs::try_exists(&self.output).await? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "Diagnostic output already exists",
            )
            .into());
        }
        let source = tokio::fs::canonicalize(&self.source).await?;
        ToolsEnginePgoProcess::text(source.as_os_str())?;
        let output = self
            .output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = tokio::fs::canonicalize(output).await?;
        if parent.starts_with(&source) {
            return Err(ToolsEnginePgoError::OutputOverlap);
        }
        tokio::fs::create_dir(&self.output).await?;
        Ok(tokio::fs::canonicalize(&self.output).await?)
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolsEnginePgoEnumStage {
    SourceHead,
    SourceRoot,
    SourceStatus,
    RustcVersion,
    CargoVersion,
    ClangVersion,
    ProfdataVersion,
    BaselineBuild,
    InstrumentBuild,
    Training,
    Merge,
    OptimizeBuild,
    SpsaBuild,
    QueuedAutoExit,
    QueuedTransition,
    Identity,
    Warmup,
    Sample,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolsEnginePgoEnumBuild {
    Baseline,
    Instrument,
    Optimize,
    Spsa,
}

impl ToolsEnginePgoEnumBuild {
    pub(crate) fn flags(
        self,
        target: EnginePackageEnumTarget,
        profile: &Path,
        instrumentation: ToolsEnginePgoEnumInstrumentation,
    ) -> Result<String, ToolsEnginePgoError> {
        let mut flags = target.portable_flags().to_owned();
        let option = match self {
            Self::Baseline | Self::Spsa => None,
            Self::Instrument => Some("profile-generate"),
            Self::Optimize => Some("profile-use"),
        };
        if let Some(option) = option {
            let path = ToolsEnginePgoProcess::text(profile.as_os_str())?;
            flags.push_str(&format!("\u{1f}-C\u{1f}{option}={path}"));
        }
        if matches!(self, Self::Optimize) {
            flags.push_str("\u{1f}-C\u{1f}llvm-args=-pgo-warn-missing-function");
        }
        if instrumentation == ToolsEnginePgoEnumInstrumentation::CountsOnly
            && matches!(self, Self::Instrument | Self::Optimize)
        {
            flags.push_str("\u{1f}-C\u{1f}llvm-args=--disable-vp");
        }
        Ok(flags)
    }

    fn directory(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Instrument => "instrumented",
            Self::Optimize => "optimized",
            Self::Spsa => "spsa",
        }
    }

    fn stage(self) -> ToolsEnginePgoEnumStage {
        match self {
            Self::Baseline => ToolsEnginePgoEnumStage::BaselineBuild,
            Self::Instrument => ToolsEnginePgoEnumStage::InstrumentBuild,
            Self::Optimize => ToolsEnginePgoEnumStage::OptimizeBuild,
            Self::Spsa => ToolsEnginePgoEnumStage::SpsaBuild,
        }
    }
}

pub(crate) enum ToolsEnginePgoEnumInput {
    HoldOpen,
    AfterUci(Vec<u8>),
}

pub(crate) struct ToolsEnginePgoProcessRequest {
    stage: ToolsEnginePgoEnumStage,
    workers: usize,
    input: ToolsEnginePgoEnumInput,
    deadline: Duration,
}

impl ToolsEnginePgoProcessRequest {
    pub(crate) fn new(
        stage: ToolsEnginePgoEnumStage,
        workers: usize,
        input: ToolsEnginePgoEnumInput,
        deadline: Duration,
    ) -> Self {
        Self {
            stage,
            workers,
            input,
            deadline,
        }
    }
}

#[derive(Serialize)]
enum ToolsEnginePgoEnumExit {
    Exited {
        success: bool,
        code: Option<i32>,
        status: String,
    },
    Cancelled,
    Deadline,
    OutputBound,
    SpawnFailed {
        code: Option<i32>,
        message: String,
    },
    Failed {
        code: Option<i32>,
        message: String,
    },
}

impl ToolsEnginePgoEnumExit {
    fn observed(status: ExitStatus) -> Self {
        Self::Exited {
            success: status.success(),
            code: status.code(),
            status: status.to_string(),
        }
    }

    fn require_success(&self, stage: ToolsEnginePgoEnumStage) -> Result<(), ToolsEnginePgoError> {
        match self {
            Self::Exited { success: true, .. } => Ok(()),
            Self::Exited {
                success: false,
                code,
                status,
            } => Err(ToolsEnginePgoError::Command {
                stage,
                code: *code,
                status: status.clone(),
            }),
            Self::Cancelled => Err(ToolsEnginePgoError::Cancelled),
            Self::Deadline => Err(ToolsEnginePgoError::Deadline),
            Self::OutputBound => Err(ToolsEnginePgoError::OutputBound),
            Self::SpawnFailed { code, message } => Err(ToolsEnginePgoError::Io(match code {
                Some(code) => std::io::Error::from_raw_os_error(*code),
                None => std::io::Error::other(message.clone()),
            })),
            Self::Failed {
                code: Some(code), ..
            } => Err(ToolsEnginePgoError::Io(std::io::Error::from_raw_os_error(
                *code,
            ))),
            Self::Failed {
                code: None,
                message,
            } => Err(ToolsEnginePgoError::Operation {
                stage,
                message: message.clone(),
            }),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct ToolsEnginePgoReceipt {
    #[serde(flatten)]
    invocation: ToolsEnginePgoInvocation,
    elapsed_microseconds: u128,
    exit: ToolsEnginePgoEnumExit,
    stdout: String,
    stderr: String,
}

#[derive(Serialize)]
struct ToolsEnginePgoInvocation {
    stage: ToolsEnginePgoEnumStage,
    program: String,
    arguments: Vec<String>,
    directory: Option<PathBuf>,
    environment: Vec<ToolsEnginePgoEnvironment>,
    workers: usize,
}

#[derive(Serialize)]
struct ToolsEnginePgoEnvironment {
    name: String,
    value: Option<String>,
}

impl ToolsEnginePgoReceipt {
    pub(crate) fn require_success(&self) -> Result<(), ToolsEnginePgoError> {
        self.exit.require_success(self.invocation.stage)
    }

    pub(crate) fn stderr(&self) -> &str {
        &self.stderr
    }
}

pub(crate) struct ToolsEnginePgoProcess<'a> {
    resources: ToolsEnginePgoResources<'a>,
    active: Option<ToolsEnginePgoChild>,
    completed: Option<ToolsEnginePgoReceipt>,
}

struct ToolsEnginePgoChild {
    child: Child,
    capacity: Option<chess::capacity::lease::CapacityLease>,
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
    stdin: Option<tokio::process::ChildStdin>,
    output: Vec<u8>,
    errors: Vec<u8>,
    input: ToolsEnginePgoEnumInput,
    written: usize,
    ready: watch::Sender<bool>,
    pid: Option<u32>,
    started: Instant,
    deadline: Instant,
    cleanup_deadline: Option<Instant>,
    cleanup: ToolsEnginePgoEnumCleanup,
    invocation: ToolsEnginePgoInvocation,
    outcome: Option<ToolsEnginePgoEnumExit>,
    force_failures: ToolsEnginePgoDrainFailures,
}

enum ToolsEnginePgoEnumCleanup {
    NotStarted,
    Tree(Child),
    ReapChild,
    FinalHelper(Child),
    FinalSignal,
    FinalTree(Child),
    FinalChild,
}

impl ToolsEnginePgoEnumCleanup {
    fn into_final(self) -> Self {
        match self {
            Self::Tree(helper) => Self::FinalHelper(helper),
            Self::NotStarted | Self::ReapChild => Self::FinalSignal,
            state @ (Self::FinalHelper(_)
            | Self::FinalSignal
            | Self::FinalTree(_)
            | Self::FinalChild) => state,
        }
    }

    fn is_final(&self) -> bool {
        matches!(
            self,
            Self::FinalHelper(_) | Self::FinalSignal | Self::FinalTree(_) | Self::FinalChild
        )
    }
}

#[derive(Default)]
struct ToolsEnginePgoDrainFailures {
    first: Option<ToolsEnginePgoError>,
    observations: u64,
}

impl ToolsEnginePgoDrainFailures {
    fn record(&mut self, error: ToolsEnginePgoError) {
        self.observations = self.observations.saturating_add(1);
        if self.first.is_none() {
            self.first = Some(error);
        }
    }

    fn into_result(self) -> Result<(), ToolsEnginePgoError> {
        match self.first {
            Some(first) => Err(ToolsEnginePgoError::ForcedDrain {
                observations: self.observations,
                first: Box::new(first),
            }),
            None => Ok(()),
        }
    }
}

impl<'a> ToolsEnginePgoProcess<'a> {
    const REAP_OBSERVATION_DELAY: Duration = Duration::from_millis(100);
    pub(crate) fn new(resources: ToolsEnginePgoResources<'a>) -> Self {
        Self {
            resources,
            active: None,
            completed: None,
        }
    }

    fn text(value: &OsStr) -> Result<String, ToolsEnginePgoError> {
        value
            .to_str()
            .filter(|value| !value.chars().any(char::is_control))
            .map(str::to_owned)
            .ok_or(ToolsEnginePgoError::InvalidPath)
    }

    pub(crate) async fn execute(
        &mut self,
        command: &mut Command,
        request: ToolsEnginePgoProcessRequest,
    ) -> Result<&ToolsEnginePgoReceipt, ToolsEnginePgoError> {
        self.start(command, request)?;
        self.observe().await
    }

    fn start(
        &mut self,
        command: &mut Command,
        request: ToolsEnginePgoProcessRequest,
    ) -> Result<(), ToolsEnginePgoError> {
        let ToolsEnginePgoProcessRequest {
            stage,
            workers,
            input,
            deadline,
        } = request;
        if self.active.is_some() || self.completed.is_some() {
            return Err(ToolsEnginePgoError::ActiveProcess);
        }
        self.resources.cancellation.check()?;
        let standard = command.as_std();
        let invocation = ToolsEnginePgoInvocation {
            stage,
            program: Self::text(standard.get_program())?,
            arguments: standard
                .get_args()
                .map(Self::text)
                .collect::<Result<_, _>>()?,
            environment: standard
                .get_envs()
                .map(|(name, value)| {
                    Ok(ToolsEnginePgoEnvironment {
                        name: Self::text(name)?,
                        value: value
                            .map(|value| {
                                value
                                    .to_str()
                                    .map(str::to_owned)
                                    .ok_or(ToolsEnginePgoError::InvalidPath)
                            })
                            .transpose()?,
                    })
                })
                .collect::<Result<_, ToolsEnginePgoError>>()?,
            directory: standard.get_current_dir().map(Path::to_path_buf),
            workers,
        };
        let capacity = self.resources.budget.try_lease(workers)?;
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let started = Instant::now();
        let spawned = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        let child = match spawned {
            Ok(child) => child,
            Err(error) => {
                self.completed = Some(ToolsEnginePgoReceipt {
                    invocation,
                    elapsed_microseconds: started.elapsed().as_micros(),
                    exit: ToolsEnginePgoEnumExit::SpawnFailed {
                        code: error.raw_os_error(),
                        message: error.to_string(),
                    },
                    stdout: String::new(),
                    stderr: String::new(),
                });
                return Ok(());
            }
        };
        self.active = Some(ToolsEnginePgoChild {
            pid: child.id(),
            child,
            capacity: Some(capacity),
            stdout: None,
            stderr: None,
            stdin: None,
            output: Vec::new(),
            errors: Vec::new(),
            input,
            written: 0,
            ready: watch::channel(false).0,
            started,
            deadline: started + deadline,
            cleanup_deadline: None,
            cleanup: ToolsEnginePgoEnumCleanup::NotStarted,
            invocation,
            outcome: None,
            force_failures: ToolsEnginePgoDrainFailures::default(),
        });
        let active = self
            .active
            .as_mut()
            .ok_or(ToolsEnginePgoError::ActiveProcess)?;
        active.stdout = active.child.stdout.take();
        active.stderr = active.child.stderr.take();
        active.stdin = active.child.stdin.take();
        Ok(())
    }

    pub(crate) async fn observe(&mut self) -> Result<&ToolsEnginePgoReceipt, ToolsEnginePgoError> {
        if let Some(active) = self.active.as_mut()
            && active.outcome.is_none()
        {
            let operation = async {
                let wait = async { active.child.wait().await.map_err(ToolsEnginePgoError::Io) };
                let (_, _, (), status) = tokio::try_join!(
                    Self::read(&mut active.stdout, &mut active.output, Some(&active.ready)),
                    Self::read(&mut active.stderr, &mut active.errors, None),
                    Self::send(
                        &mut active.stdin,
                        &active.input,
                        &mut active.written,
                        &active.ready
                    ),
                    wait
                )?;
                Ok::<_, ToolsEnginePgoError>(ToolsEnginePgoEnumExit::observed(status))
            };
            let outcome = tokio::select! {
                biased;
                () = self.resources.cancellation.cancelled() => ToolsEnginePgoEnumExit::Cancelled,
                result = tokio::time::timeout_at(active.deadline, operation) => match result {
                    Ok(Ok(exit)) => exit,
                    Ok(Err(ToolsEnginePgoError::OutputBound)) => ToolsEnginePgoEnumExit::OutputBound,
                    Ok(Err(error)) => ToolsEnginePgoEnumExit::Failed { code: if let ToolsEnginePgoError::Io(source) = &error { source.raw_os_error() } else { None }, message: error.to_string() },
                    Err(_elapsed) => ToolsEnginePgoEnumExit::Deadline,
                }
            };
            active.outcome = Some(outcome);
        }
        let operation = self.active.as_ref().and_then(|active| {
            active
                .outcome
                .as_ref()
                .and_then(|outcome| outcome.require_success(active.invocation.stage).err())
        });
        if let Err(cleanup) = self
            .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
            .await
        {
            return Err(match operation {
                Some(operation) => ToolsEnginePgoError::WorkflowCleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                },
                None => cleanup,
            });
        }
        self.completed
            .as_ref()
            .ok_or(ToolsEnginePgoError::ActiveProcess)
    }

    async fn send(
        stdin: &mut Option<tokio::process::ChildStdin>,
        input: &ToolsEnginePgoEnumInput,
        written: &mut usize,
        ready: &watch::Sender<bool>,
    ) -> Result<(), ToolsEnginePgoError> {
        match input {
            ToolsEnginePgoEnumInput::HoldOpen => Ok(()),
            ToolsEnginePgoEnumInput::AfterUci(bytes) => {
                let mut acknowledged = ready.subscribe();
                drop(
                    acknowledged
                        .wait_for(|ready| *ready)
                        .await
                        .map_err(|_closed| ToolsEnginePgoError::Comparison)?,
                );
                let stdin = stdin.as_mut().ok_or(ToolsEnginePgoError::OutputBound)?;
                while *written < bytes.len() {
                    let count = stdin.write(&bytes[*written..]).await?;
                    if count == 0 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::WriteZero,
                            "Diagnostic stdin stopped accepting bytes",
                        )
                        .into());
                    }
                    *written = written
                        .checked_add(count)
                        .ok_or(ToolsEnginePgoError::OutputBound)?;
                }
                stdin.flush().await?;
                Ok(())
            }
        }
    }

    async fn read<P: AsyncRead + Unpin>(
        pipe: &mut Option<P>,
        output: &mut Vec<u8>,
        ready: Option<&watch::Sender<bool>>,
    ) -> Result<(), ToolsEnginePgoError> {
        let pipe = pipe.as_mut().ok_or(ToolsEnginePgoError::OutputBound)?;
        let mut buffer = vec![0_u8; 8192];
        loop {
            let count = pipe.read(&mut buffer).await?;
            if count == 0 {
                return Ok(());
            }
            if output
                .len()
                .checked_add(count)
                .is_none_or(|size| size > EngineDiagnostics::MAX_OWNED_BYTES / 2)
            {
                return Err(ToolsEnginePgoError::OutputBound);
            }
            output.extend_from_slice(&buffer[..count]);
            if let Some(ready) = ready
                && !*ready.borrow()
                && output
                    .split(|byte| *byte == b'\n')
                    .any(|line| line == b"uciok" || line == b"uciok\r")
            {
                ready.send_replace(true);
            }
        }
    }

    pub(crate) async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), ToolsEnginePgoError> {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.cleanup.is_final())
        {
            return self.force_reap_retained().await;
        }
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        let deadline = active
            .cleanup_deadline
            .map_or(deadline, |existing| existing.min(deadline));
        active.cleanup_deadline = Some(deadline);
        if active.outcome.is_none() {
            active.outcome = Some(ToolsEnginePgoEnumExit::Cancelled);
        }
        if matches!(active.cleanup, ToolsEnginePgoEnumCleanup::NotStarted) {
            let exited = active.child.try_wait()?.is_some();
            if exited && matches!(active.outcome, Some(ToolsEnginePgoEnumExit::Exited { .. })) {
                active.cleanup = ToolsEnginePgoEnumCleanup::ReapChild;
            } else if let Some(pid) = active.pid {
                let terminator = Self::tree_terminator(pid).spawn()?;
                active.cleanup = ToolsEnginePgoEnumCleanup::Tree(terminator);
            } else {
                active.cleanup = ToolsEnginePgoEnumCleanup::ReapChild;
            }
        }
        if let ToolsEnginePgoEnumCleanup::Tree(terminator) = &mut active.cleanup {
            let status = tokio::time::timeout_at(deadline, terminator.wait())
                .await
                .map_err(|error| ToolsEnginePgoError::Cleanup(error.to_string()))??;
            if !status.success() && active.child.try_wait()?.is_none() {
                active.child.start_kill()?;
                return Err(ToolsEnginePgoError::Cleanup(format!(
                    "Process-tree termination exited with {status}"
                )));
            }
            active.cleanup = ToolsEnginePgoEnumCleanup::ReapChild;
        }
        if active.child.try_wait()?.is_none() {
            active.child.start_kill()?;
        }
        tokio::time::timeout_at(deadline, active.child.wait())
            .await
            .map_err(|error| ToolsEnginePgoError::Cleanup(error.to_string()))??;
        self.complete_reaped()
    }

    fn tree_terminator(pid: u32) -> Command {
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("/bin/kill");
            command.args(["-KILL", "--"]).arg(format!("-{pid}"));
            command
        };
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("taskkill");
            command.args(["/F", "/T", "/PID"]).arg(pid.to_string());
            command
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        command
    }

    async fn force_reap_retained(&mut self) -> Result<(), ToolsEnginePgoError> {
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        active.cleanup =
            std::mem::replace(&mut active.cleanup, ToolsEnginePgoEnumCleanup::FinalSignal)
                .into_final();
        if active.outcome.is_none() {
            active.outcome = Some(ToolsEnginePgoEnumExit::Cancelled);
        }
        loop {
            match &mut active.cleanup {
                ToolsEnginePgoEnumCleanup::FinalHelper(helper) => {
                    let needs_signal = match helper.try_wait() {
                        Ok(status) => status.is_none(),
                        Err(error) => {
                            active.force_failures.record(error.into());
                            true
                        }
                    };
                    if needs_signal && let Err(error) = helper.start_kill() {
                        active.force_failures.record(error.into());
                    }
                    match helper.wait().await {
                        Ok(_status) => active.cleanup = ToolsEnginePgoEnumCleanup::FinalSignal,
                        Err(error) => {
                            active.force_failures.record(error.into());
                            tokio::time::sleep(Self::REAP_OBSERVATION_DELAY).await;
                        }
                    }
                }
                ToolsEnginePgoEnumCleanup::FinalSignal => {
                    if let Some(pid) = active.pid {
                        match Self::tree_terminator(pid).spawn() {
                            Ok(helper) => {
                                active.cleanup = ToolsEnginePgoEnumCleanup::FinalTree(helper)
                            }
                            Err(error) => {
                                active.force_failures.record(error.into());
                                tokio::time::sleep(Self::REAP_OBSERVATION_DELAY).await;
                            }
                        }
                    } else {
                        active.cleanup = ToolsEnginePgoEnumCleanup::FinalChild;
                    }
                }
                ToolsEnginePgoEnumCleanup::FinalTree(helper) => {
                    match helper.wait().await {
                        Ok(status) => {
                            if !status.success() {
                                active.force_failures.record(ToolsEnginePgoError::Cleanup(
                                    format!("Final process-tree termination exited with {status}"),
                                ));
                            }
                            active.cleanup = ToolsEnginePgoEnumCleanup::FinalChild;
                        }
                        Err(error) => {
                            active.force_failures.record(error.into());
                            tokio::time::sleep(Self::REAP_OBSERVATION_DELAY).await;
                        }
                    }
                }
                ToolsEnginePgoEnumCleanup::FinalChild => {
                    let needs_signal = match active.child.try_wait() {
                        Ok(status) => status.is_none(),
                        Err(error) => {
                            active.force_failures.record(error.into());
                            true
                        }
                    };
                    if needs_signal && let Err(error) = active.child.start_kill() {
                        active.force_failures.record(error.into());
                    }
                    match active.child.wait().await {
                        Ok(_status) => break,
                        Err(error) => {
                            active.force_failures.record(error.into());
                            tokio::time::sleep(Self::REAP_OBSERVATION_DELAY).await;
                        }
                    }
                }
                ToolsEnginePgoEnumCleanup::NotStarted
                | ToolsEnginePgoEnumCleanup::Tree(_)
                | ToolsEnginePgoEnumCleanup::ReapChild => {
                    active.cleanup = std::mem::replace(
                        &mut active.cleanup,
                        ToolsEnginePgoEnumCleanup::FinalSignal,
                    )
                    .into_final();
                }
            }
        }
        self.complete_reaped()
    }

    fn complete_reaped(&mut self) -> Result<(), ToolsEnginePgoError> {
        let mut active = self
            .active
            .take()
            .ok_or(ToolsEnginePgoError::ActiveProcess)?;
        let exit = active
            .outcome
            .take()
            .ok_or(ToolsEnginePgoError::ActiveProcess)?;
        let stdout =
            String::from_utf8(active.output).map_err(|_error| ToolsEnginePgoError::OutputBound)?;
        let stderr =
            String::from_utf8(active.errors).map_err(|_error| ToolsEnginePgoError::OutputBound)?;
        self.completed = Some(ToolsEnginePgoReceipt {
            invocation: active.invocation,
            elapsed_microseconds: active.started.elapsed().as_micros(),
            exit,
            stdout,
            stderr,
        });
        drop(active.capacity.take());
        active.force_failures.into_result()
    }

    fn take_receipt(&mut self) -> Result<ToolsEnginePgoReceipt, ToolsEnginePgoError> {
        self.completed
            .take()
            .ok_or(ToolsEnginePgoError::ActiveProcess)
    }
}

#[derive(Serialize)]
struct ToolsEnginePgoProvenance {
    engine_version: &'static str,
    source_commit: &'static str,
    source: PathBuf,
    lock_sha256: PackageDigest,
    model: PathBuf,
    model_bytes: u64,
    model_sha256: PackageDigest,
    target: EnginePackageEnumTarget,
    workers: usize,
    instrumentation: ToolsEnginePgoEnumInstrumentation,
}

#[derive(Serialize)]
struct ToolsEnginePgoBinary {
    path: PathBuf,
    bytes: u64,
    sha256: PackageDigest,
}

#[derive(Serialize)]
struct ToolsEnginePgoApplication {
    version: &'static str,
    source_commit: String,
    source_status: String,
    executable: ToolsEnginePgoBinary,
}

#[derive(Serialize)]
struct ToolsEnginePgoQueuedReceipt {
    engine_version: &'static str,
    source_commit: &'static str,
    model_sha256: &'static str,
    model_bytes: u64,
    target: EnginePackageEnumTarget,
    binary: ToolsEnginePgoBinary,
    observation: ToolsEnginePgoQueuedObservation,
}

#[derive(Serialize, PartialEq, Eq)]
pub(crate) enum ToolsEnginePgoEnumFullmove {
    Exact {
        fullmove: u32,
    },
    UpstreamAfterWhite {
        expected: u32,
        observed: u32,
        evidence: ToolsEnginePgoCounterEvidence,
    },
}

#[derive(Serialize, PartialEq, Eq)]
pub(crate) struct ToolsEnginePgoCounterEvidence {
    source_commit: &'static str,
    counter_owner: &'static str,
    caller: &'static str,
}

#[derive(Serialize, PartialEq, Eq)]
pub(crate) struct ToolsEnginePgoQueuedPosition {
    expected_fen: String,
    observed_fen: String,
    fullmove: ToolsEnginePgoEnumFullmove,
}

impl ToolsEnginePgoQueuedPosition {
    pub(crate) fn from_output(
        text: &str,
        expected: &BoardPosition,
    ) -> Result<Self, ToolsEnginePgoError> {
        let mut positions = text.lines().filter_map(|line| line.strip_prefix("FEN: "));
        let fen = positions.next().ok_or(ToolsEnginePgoError::Comparison)?;
        if positions.next().is_some() {
            return Err(ToolsEnginePgoError::Comparison);
        }
        let observed = BoardPosition::from_fen(fen, expected.variant())?;
        if observed.identity() != expected.identity()
            || observed.halfmoves() != expected.halfmoves()
        {
            return Err(ToolsEnginePgoError::Comparison);
        }
        let fullmove = if observed.fullmoves() == expected.fullmoves() {
            ToolsEnginePgoEnumFullmove::Exact {
                fullmove: expected.fullmoves(),
            }
        } else if expected.turn() == BoardEnumColor::Black
            && expected.fullmoves().checked_add(1) == Some(observed.fullmoves())
        {
            ToolsEnginePgoEnumFullmove::UpstreamAfterWhite {
                expected: expected.fullmoves(),
                observed: observed.fullmoves(),
                evidence: ToolsEnginePgoCounterEvidence {
                    source_commit: EnginePackageManifest::COMMIT,
                    counter_owner: "src/board.rs:202-206",
                    caller: "src/uci.rs:277-282",
                },
            }
        } else {
            return Err(ToolsEnginePgoError::Comparison);
        };
        Ok(Self {
            expected_fen: expected.fen(),
            observed_fen: observed.fen(),
            fullmove,
        })
    }

    pub(crate) fn fullmove(&self) -> &ToolsEnginePgoEnumFullmove {
        &self.fullmove
    }

    fn has_counter_discrepancy(&self) -> bool {
        match self.fullmove() {
            ToolsEnginePgoEnumFullmove::Exact { .. } => false,
            ToolsEnginePgoEnumFullmove::UpstreamAfterWhite { .. } => true,
        }
    }
}

#[derive(Serialize, PartialEq, Eq)]
struct ToolsEnginePgoQueuedObservation {
    auto_exit: ToolsEnginePgoQueuedPosition,
    interactive: ToolsEnginePgoQueuedPosition,
    behavior: String,
}

impl ToolsEnginePgoQueuedObservation {
    fn disposition(&self) -> ToolsEnginePgoEnumDisposition {
        if self.auto_exit.has_counter_discrepancy() || self.interactive.has_counter_discrepancy() {
            ToolsEnginePgoEnumDisposition::QueuedWithUpstreamCounterDiscrepancy
        } else {
            ToolsEnginePgoEnumDisposition::QueuedVerified
        }
    }
}

#[derive(Serialize)]
struct ToolsEnginePgoSample {
    workers: usize,
    nodes: u64,
    nps: u64,
    elapsed_microseconds: u128,
}

#[derive(Serialize)]
struct ToolsEnginePgoComparison {
    positions: u32,
    depth: u32,
    baseline: ToolsEnginePgoBinary,
    optimized: ToolsEnginePgoBinary,
    baseline_samples: Vec<ToolsEnginePgoSample>,
    optimized_samples: Vec<ToolsEnginePgoSample>,
    deterministic_one_worker_nodes: u64,
    behavior: ToolsEnginePgoQueuedObservation,
}

#[derive(Serialize)]
pub(crate) struct ToolsEnginePgoJournal<'a> {
    #[serde(skip)]
    resources: ToolsEnginePgoResources<'a>,
    #[serde(skip)]
    process: ToolsEnginePgoProcess<'a>,
    output: PathBuf,
    application: Option<ToolsEnginePgoApplication>,
    provenance: Option<ToolsEnginePgoProvenance>,
    receipts: Vec<PathBuf>,
    unpublished_receipt: Option<ToolsEnginePgoUnpublishedReceipt>,
    artifacts: Vec<ToolsEnginePgoBinary>,
    comparison: Option<ToolsEnginePgoComparison>,
    disposition: Option<ToolsEnginePgoEnumDisposition>,
    failure: Option<String>,
}

#[derive(Serialize)]
struct ToolsEnginePgoUnpublishedReceipt {
    path: PathBuf,
    receipt: Box<ToolsEnginePgoReceipt>,
}

#[derive(Clone, Copy, Serialize)]
pub(crate) enum ToolsEnginePgoEnumDisposition {
    #[serde(rename = "controlled_comparison_completed_no_performance_acceptance")]
    ControlledComparisonCompleted,
    #[serde(rename = "queued_auto_exit_and_interactive_transition_verified")]
    QueuedVerified,
    #[serde(rename = "queued_replay_verified_with_upstream_fullmove_discrepancy")]
    QueuedWithUpstreamCounterDiscrepancy,
    #[serde(rename = "spsa_compile_succeeded_runtime_tuning_not_verified")]
    SpsaCompileSucceeded,
    #[serde(rename = "spsa_unavailable_in_pinned_upstream")]
    SpsaUpstreamUnavailable,
}

impl<'a> ToolsEnginePgoJournal<'a> {
    pub(crate) async fn new(
        inputs: &ToolsEnginePgoInputs,
        resources: ToolsEnginePgoResources<'a>,
    ) -> Result<Self, ToolsEnginePgoError> {
        resources.cancellation.check()?;
        let output = inputs.claim_output().await?;
        Ok(Self {
            resources,
            process: ToolsEnginePgoProcess::new(resources),
            output,
            application: None,
            provenance: None,
            receipts: Vec::new(),
            unpublished_receipt: None,
            artifacts: Vec::new(),
            comparison: None,
            disposition: None,
            failure: None,
        })
    }

    pub(crate) async fn finish(
        mut self,
        outcome: Result<ToolsEnginePgoEnumDisposition, ToolsEnginePgoError>,
    ) -> Result<PathBuf, ToolsEnginePgoError> {
        let graceful_cleanup = self
            .process
            .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
            .await;
        let cleanup = match graceful_cleanup {
            Ok(()) => Ok(()),
            Err(operation) => match self.process.force_reap_retained().await {
                Ok(()) => Err(operation),
                Err(cleanup) => Err(ToolsEnginePgoError::WorkflowCleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
            },
        };
        let mut outcome = match (outcome, cleanup) {
            (outcome, Ok(())) => outcome,
            (Ok(_disposition), Err(error)) => Err(error),
            (Err(operation), Err(cleanup)) => Err(ToolsEnginePgoError::WorkflowCleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            }),
        };
        if self.process.completed.is_some() {
            let terminal_error = self
                .process
                .completed
                .as_ref()
                .and_then(|receipt| receipt.require_success().err());
            match self.publish_completed().await {
                Ok(receipt) => {
                    if let Ok(disposition) = outcome {
                        outcome = receipt.require_success().map(|()| disposition);
                    }
                }
                Err(publication) => {
                    outcome = Err(ToolsEnginePgoError::publication(
                        outcome.err().or(terminal_error),
                        publication,
                    ));
                }
            }
        }
        match &outcome {
            Ok(disposition) => self.disposition = Some(*disposition),
            Err(error) => self.failure = Some(error.to_string()),
        }
        let report = self.output.join("report.json");
        let publication = Self::write_json(&report, &self).await;
        match (outcome, publication) {
            (Ok(_disposition), Ok(())) => Ok(report),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
            (Err(operation), Err(publication)) => Err(ToolsEnginePgoError::publication(
                Some(operation),
                publication,
            )),
        }
    }

    async fn write_json(path: &Path, value: &impl Serialize) -> Result<(), ToolsEnginePgoError> {
        let mut bytes = serde_json::to_vec_pretty(value)?;
        bytes.push(b'\n');
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .await?;
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        Ok(())
    }

    pub(crate) async fn execute(
        &mut self,
        command: &mut Command,
        stage: ToolsEnginePgoEnumStage,
        workers: usize,
        input: ToolsEnginePgoEnumInput,
    ) -> Result<ToolsEnginePgoReceipt, ToolsEnginePgoError> {
        if self.unpublished_receipt.is_some() {
            return Err(ToolsEnginePgoError::ActiveProcess);
        }
        let deadline = match stage {
            ToolsEnginePgoEnumStage::BaselineBuild
            | ToolsEnginePgoEnumStage::InstrumentBuild
            | ToolsEnginePgoEnumStage::OptimizeBuild
            | ToolsEnginePgoEnumStage::SpsaBuild => Duration::from_secs(1800),
            ToolsEnginePgoEnumStage::Training
            | ToolsEnginePgoEnumStage::Warmup
            | ToolsEnginePgoEnumStage::Sample => Duration::from_secs(600),
            ToolsEnginePgoEnumStage::SourceHead
            | ToolsEnginePgoEnumStage::SourceRoot
            | ToolsEnginePgoEnumStage::SourceStatus
            | ToolsEnginePgoEnumStage::RustcVersion
            | ToolsEnginePgoEnumStage::CargoVersion
            | ToolsEnginePgoEnumStage::ClangVersion
            | ToolsEnginePgoEnumStage::ProfdataVersion
            | ToolsEnginePgoEnumStage::Merge
            | ToolsEnginePgoEnumStage::QueuedAutoExit
            | ToolsEnginePgoEnumStage::QueuedTransition
            | ToolsEnginePgoEnumStage::Identity => Duration::from_secs(30),
        };
        let receipt = self
            .process
            .execute(
                command,
                ToolsEnginePgoProcessRequest::new(stage, workers, input, deadline),
            )
            .await?;
        let operation = receipt.require_success().err();
        self.publish_completed()
            .await
            .map_err(|publication| ToolsEnginePgoError::publication(operation, publication))
    }

    async fn publish_completed(&mut self) -> Result<ToolsEnginePgoReceipt, ToolsEnginePgoError> {
        if self.unpublished_receipt.is_some() {
            return Err(ToolsEnginePgoError::ActiveProcess);
        }
        let receipt = self
            .process
            .completed
            .as_ref()
            .ok_or(ToolsEnginePgoError::ActiveProcess)?;
        let path = self
            .output
            .join(format!("command-{:03}.json", self.receipts.len()));
        if let Err(publication) = Self::write_json(&path, receipt).await {
            self.unpublished_receipt = Some(ToolsEnginePgoUnpublishedReceipt {
                path,
                receipt: Box::new(self.process.take_receipt()?),
            });
            return Err(publication);
        }
        self.receipts.push(path);
        self.process.take_receipt()
    }

    async fn checked(
        &mut self,
        command: &mut Command,
        stage: ToolsEnginePgoEnumStage,
        workers: usize,
    ) -> Result<String, ToolsEnginePgoError> {
        let receipt = self
            .execute(command, stage, workers, ToolsEnginePgoEnumInput::HoldOpen)
            .await?;
        receipt.require_success()?;
        Ok(receipt.stdout)
    }

    pub(crate) async fn verify(
        &mut self,
        inputs: &ToolsEnginePgoInputs,
    ) -> Result<(), ToolsEnginePgoError> {
        self.inspect_application().await?;
        let source = tokio::fs::canonicalize(&inputs.source).await?;
        let model = tokio::fs::canonicalize(&inputs.model).await?;
        ToolsEnginePgoProcess::text(source.as_os_str())?;
        ToolsEnginePgoProcess::text(model.as_os_str())?;
        let head = self
            .checked(
                Command::new("git")
                    .arg("-C")
                    .arg(&source)
                    .args(["rev-parse", "HEAD"]),
                ToolsEnginePgoEnumStage::SourceHead,
                1,
            )
            .await?;
        let root = self
            .checked(
                Command::new("git")
                    .arg("-C")
                    .arg(&source)
                    .args(["rev-parse", "--show-toplevel"]),
                ToolsEnginePgoEnumStage::SourceRoot,
                1,
            )
            .await?;
        if head.trim() != EnginePackageManifest::COMMIT
            || tokio::fs::canonicalize(root.trim()).await? != source
        {
            return Err(ToolsEnginePgoError::SourceMismatch);
        }
        self.verify_clean(&source).await?;
        let hash_source = source.clone();
        let hash_model = model.clone();
        let (lock_sha256, model_bytes, model_sha256) = self
            .resources
            .executor
            .submit(move |_cancellation| {
                let file = std::fs::File::open(hash_model)?;
                let bytes = file.metadata()?.len();
                if bytes != EnginePackageManifest::NETWORK_BYTES {
                    return Err(ToolsEnginePgoError::SourceMismatch);
                }
                let digest = PackageDigest::read(file)?;
                let lock =
                    PackageDigest::read(std::fs::File::open(hash_source.join("Cargo.lock"))?)?;
                Ok::<_, ToolsEnginePgoError>((lock, bytes, digest))
            })?
            .join()
            .await??;
        if model_bytes != EnginePackageManifest::NETWORK_BYTES
            || model_sha256.to_string() != EnginePackageManifest::NETWORK_SHA256
        {
            return Err(ToolsEnginePgoError::SourceMismatch);
        }
        self.provenance = Some(ToolsEnginePgoProvenance {
            engine_version: EnginePackageManifest::VERSION,
            source_commit: EnginePackageManifest::COMMIT,
            source,
            lock_sha256,
            model,
            model_bytes,
            model_sha256,
            target: EnginePackageEnumTarget::current()?,
            workers: self.resources.budget.workers(),
            instrumentation: inputs.instrumentation(),
        });
        let rustc = self
            .checked(
                Command::new("rustc").args(["--version", "--verbose"]),
                ToolsEnginePgoEnumStage::RustcVersion,
                1,
            )
            .await?;
        let pin = include_str!("../../rust-toolchain.toml")
            .lines()
            .find_map(|line| {
                line.strip_prefix("channel = \"")
                    .and_then(|value| value.strip_suffix('"'))
            })
            .ok_or(ToolsEnginePgoError::CompilerMismatch)?;
        if !rustc.lines().any(|line| line == format!("release: {pin}")) {
            return Err(ToolsEnginePgoError::CompilerMismatch);
        }
        self.checked(
            Command::new("cargo").args(["--version", "--verbose"]),
            ToolsEnginePgoEnumStage::CargoVersion,
            1,
        )
        .await?;
        self.checked(
            Command::new("clang").arg("--version"),
            ToolsEnginePgoEnumStage::ClangVersion,
            1,
        )
        .await?;
        Ok(())
    }

    async fn verify_clean(&mut self, source: &Path) -> Result<(), ToolsEnginePgoError> {
        let status = self
            .checked(
                Command::new("git").arg("-C").arg(source).args([
                    "status",
                    "--porcelain=v1",
                    "--untracked-files=all",
                ]),
                ToolsEnginePgoEnumStage::SourceStatus,
                1,
            )
            .await?;
        if status.is_empty() {
            Ok(())
        } else {
            Err(ToolsEnginePgoError::SourceMismatch)
        }
    }

    async fn inspect_application(&mut self) -> Result<(), ToolsEnginePgoError> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let source_commit = self
            .checked(
                Command::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(["rev-parse", "HEAD"]),
                ToolsEnginePgoEnumStage::SourceHead,
                1,
            )
            .await?;
        let source_status = self
            .checked(
                Command::new("git").arg("-C").arg(root).args([
                    "status",
                    "--porcelain=v1",
                    "--untracked-files=all",
                ]),
                ToolsEnginePgoEnumStage::SourceStatus,
                1,
            )
            .await?;
        self.application = Some(ToolsEnginePgoApplication {
            version: env!("CARGO_PKG_VERSION"),
            source_commit: source_commit.trim().to_owned(),
            source_status,
            executable: self.binary(&std::env::current_exe()?).await?,
        });
        Ok(())
    }

    pub(crate) async fn verify_unchanged(&mut self) -> Result<(), ToolsEnginePgoError> {
        let provenance = self
            .provenance
            .as_ref()
            .ok_or(ToolsEnginePgoError::SourceMismatch)?;
        let source = provenance.source.clone();
        let expected_commit = provenance.source_commit;
        let expected_lock = provenance.lock_sha256;
        let model = provenance.model.clone();
        let expected_model_bytes = provenance.model_bytes;
        let expected_model_digest = provenance.model_sha256;
        let head = self
            .checked(
                Command::new("git")
                    .arg("-C")
                    .arg(&source)
                    .args(["rev-parse", "HEAD"]),
                ToolsEnginePgoEnumStage::SourceHead,
                1,
            )
            .await?;
        if head.trim() != expected_commit {
            return Err(ToolsEnginePgoError::SourceMismatch);
        }
        let lock = source.join("Cargo.lock");
        let (actual_lock, actual_model) = self
            .resources
            .executor
            .submit(move |_cancellation| {
                let model = std::fs::File::open(model)?;
                if model.metadata()?.len() != expected_model_bytes {
                    return Err(ToolsEnginePgoError::SourceMismatch);
                }
                Ok::<_, ToolsEnginePgoError>((
                    PackageDigest::read(std::fs::File::open(lock)?)?,
                    PackageDigest::read(model)?,
                ))
            })
            .map_err(ToolsEnginePgoError::from)?
            .join()
            .await??;
        if actual_lock != expected_lock {
            return Err(ToolsEnginePgoError::LockChanged);
        }
        if actual_model != expected_model_digest {
            return Err(ToolsEnginePgoError::SourceMismatch);
        }
        self.verify_clean(&source).await
    }

    pub(crate) async fn build(
        &mut self,
        build: ToolsEnginePgoEnumBuild,
    ) -> Result<(PathBuf, ToolsEnginePgoReceipt), ToolsEnginePgoError> {
        self.verify_unchanged().await?;
        let provenance = self
            .provenance
            .as_ref()
            .ok_or(ToolsEnginePgoError::SourceMismatch)?;
        let directory = self.output.join(build.directory());
        let raw = self.output.join("raw-profiles");
        let profile = match build {
            ToolsEnginePgoEnumBuild::Instrument => raw.clone(),
            ToolsEnginePgoEnumBuild::Baseline
            | ToolsEnginePgoEnumBuild::Optimize
            | ToolsEnginePgoEnumBuild::Spsa => self.output.join("merged.profdata"),
        };
        let flags = build.flags(provenance.target, &profile, provenance.instrumentation)?;
        let mut command = Command::new("cargo");
        command
            .current_dir(&provenance.source)
            .args([
                "build",
                "--locked",
                "--offline",
                "--release",
                "--no-default-features",
                "--features",
                if matches!(build, ToolsEnginePgoEnumBuild::Spsa) {
                    "syzygy,spsa"
                } else {
                    "syzygy"
                },
                "--target",
                provenance.target.triple(),
            ])
            .env("EVALFILE", &provenance.model)
            .env("CARGO_TARGET_DIR", &directory)
            .env_remove("RUSTFLAGS")
            .env("CARGO_ENCODED_RUSTFLAGS", flags)
            .env_remove("CARGO_INCREMENTAL")
            .env_remove("CARGO_MAKEFLAGS")
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env("RUSTC", "rustc")
            .env("CARGO_TERM_COLOR", "never")
            .env("CARGO_TERM_PROGRESS_WHEN", "never")
            .env(
                "CARGO_BUILD_JOBS",
                self.resources.budget.workers().to_string(),
            )
            .env("CFLAGS", "-O3");
        if provenance.target == EnginePackageEnumTarget::MacosArm {
            command.env("MACOSX_DEPLOYMENT_TARGET", "14.0");
        }
        let filename = if provenance.target == EnginePackageEnumTarget::WindowsX64 {
            "reckless.exe"
        } else {
            "reckless"
        };
        let binary = directory
            .join(provenance.target.triple())
            .join("release")
            .join(filename);
        let receipt = self
            .execute(
                &mut command,
                build.stage(),
                self.resources.budget.workers(),
                ToolsEnginePgoEnumInput::HoldOpen,
            )
            .await?;
        if let Err(validation) = self.verify_unchanged().await {
            return match receipt.require_success() {
                Ok(()) => Err(validation),
                Err(operation) => Err(ToolsEnginePgoError::InputValidation {
                    operation: Box::new(operation),
                    validation: Box::new(validation),
                }),
            };
        }
        if matches!(
            receipt.exit,
            ToolsEnginePgoEnumExit::Exited { success: true, .. }
        ) {
            self.artifacts.push(self.binary(&binary).await?);
        }
        Ok((binary, receipt))
    }

    async fn binary(&self, path: &Path) -> Result<ToolsEnginePgoBinary, ToolsEnginePgoError> {
        let path = path.to_path_buf();
        self.resources
            .executor
            .submit(move |_cancellation| {
                let file = std::fs::File::open(&path)?;
                let bytes = file.metadata()?.len();
                Ok::<_, ToolsEnginePgoError>(ToolsEnginePgoBinary {
                    path,
                    bytes,
                    sha256: PackageDigest::read(file)?,
                })
            })?
            .join()
            .await?
    }

    async fn identity(&mut self, binary: &Path) -> Result<EngineCapabilities, ToolsEnginePgoError> {
        let text = self
            .checked(
                Command::new(binary).args(["uci", "quit"]),
                ToolsEnginePgoEnumStage::Identity,
                1,
            )
            .await?;
        let mut capabilities = EngineCapabilities::default();
        for line in text.lines().filter(|line| line.starts_with("option name ")) {
            capabilities.discover(line)?;
        }
        capabilities.validate()?;
        if !text
            .lines()
            .any(|line| line == format!("id name Reckless {}", EnginePackageManifest::VERSION))
            || !text.lines().any(|line| line == "uciok")
        {
            return Err(ToolsEnginePgoError::Comparison);
        }
        Ok(capabilities)
    }

    async fn queued(
        &mut self,
        binary: &Path,
    ) -> Result<ToolsEnginePgoQueuedObservation, ToolsEnginePgoError> {
        let position = EngineSearchPosition::new(BoardPosition::standard(), ["e2e4"])?;
        let text = self
            .checked(
                Command::new(binary).args([position.command(), "d", "eval"]),
                ToolsEnginePgoEnumStage::QueuedAutoExit,
                1,
            )
            .await?;
        let auto_exit = ToolsEnginePgoQueuedPosition::from_output(&text, position.current())?;
        let input = format!("{}\nd\nquit\n", position.command()).into_bytes();
        let receipt = self
            .execute(
                Command::new(binary).args(["compiler", "uci"]),
                ToolsEnginePgoEnumStage::QueuedTransition,
                1,
                ToolsEnginePgoEnumInput::AfterUci(input),
            )
            .await?;
        receipt.require_success()?;
        if !receipt.stdout.lines().any(|line| line == "uciok") {
            return Err(ToolsEnginePgoError::Comparison);
        }
        let interactive =
            ToolsEnginePgoQueuedPosition::from_output(&receipt.stdout, position.current())?;
        Ok(ToolsEnginePgoQueuedObservation {
            auto_exit,
            interactive,
            behavior: text,
        })
    }
}

pub(crate) struct ToolsEnginePgo;

impl ToolsEnginePgo {
    const SAMPLES: usize = 5;

    pub(crate) async fn run(
        inputs: ToolsEnginePgoInputs,
        profdata: PathBuf,
        resources: ToolsEnginePgoResources<'_>,
    ) -> Result<PathBuf, ToolsEnginePgoError> {
        let mut journal = ToolsEnginePgoJournal::new(&inputs, resources).await?;
        let result = Self::controlled(&mut journal, &inputs, &profdata).await;
        journal
            .finish(result.map(|()| ToolsEnginePgoEnumDisposition::ControlledComparisonCompleted))
            .await
    }

    async fn controlled(
        journal: &mut ToolsEnginePgoJournal<'_>,
        inputs: &ToolsEnginePgoInputs,
        profdata: &Path,
    ) -> Result<(), ToolsEnginePgoError> {
        journal.verify(inputs).await?;
        journal
            .checked(
                Command::new(profdata).arg("--version"),
                ToolsEnginePgoEnumStage::ProfdataVersion,
                1,
            )
            .await?;
        let (baseline, built) = journal.build(ToolsEnginePgoEnumBuild::Baseline).await?;
        built.require_success()?;
        let capabilities = journal.identity(&baseline).await?;
        let baseline_behavior = journal.queued(&baseline).await?;
        let (instrumented, built) = journal.build(ToolsEnginePgoEnumBuild::Instrument).await?;
        built.require_success()?;
        let parameters = WorkbenchBenchParameters::new(
            &capabilities,
            None,
            Some(journal.resources.budget.workers()),
            None,
        )?;
        let raw = journal.output.join("raw-profiles");
        tokio::fs::create_dir(&raw).await?;
        let mut command = Self::bench(&instrumented, parameters);
        command.env("LLVM_PROFILE_FILE", raw.join("training-%m-%p.profraw"));
        journal
            .checked(
                &mut command,
                ToolsEnginePgoEnumStage::Training,
                parameters.workers(),
            )
            .await?;
        let mut files = Vec::new();
        let mut entries = tokio::fs::read_dir(&raw).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.path().extension() == Some(OsStr::new("profraw"))
                && entry.metadata().await?.len() > 0
            {
                files.push(entry.path());
            }
        }
        files.sort();
        if files.is_empty() {
            return Err(ToolsEnginePgoError::MissingProfile);
        }
        for file in &files {
            journal.artifacts.push(journal.binary(file).await?);
        }
        let merged = journal.output.join("merged.profdata");
        journal
            .checked(
                Command::new(profdata)
                    .args(["merge", "-o"])
                    .arg(&merged)
                    .args(files),
                ToolsEnginePgoEnumStage::Merge,
                journal.resources.budget.workers(),
            )
            .await?;
        if tokio::fs::metadata(&merged).await?.len() == 0 {
            return Err(ToolsEnginePgoError::MissingProfile);
        }
        journal.artifacts.push(journal.binary(&merged).await?);
        let (optimized, built) = journal.build(ToolsEnginePgoEnumBuild::Optimize).await?;
        built.require_success()?;
        journal.identity(&optimized).await?;
        if journal.queued(&optimized).await? != baseline_behavior {
            return Err(ToolsEnginePgoError::Comparison);
        }
        let mut baseline_samples = Vec::new();
        let mut optimized_samples = Vec::new();
        let mut workers = vec![1];
        if parameters.workers() != 1 {
            workers.push(parameters.workers());
        }
        for workers in workers {
            let parameters =
                WorkbenchBenchParameters::new(&capabilities, None, Some(workers), None)?;
            for binary in [&baseline, &optimized] {
                journal
                    .checked(
                        &mut Self::bench(binary, parameters),
                        ToolsEnginePgoEnumStage::Warmup,
                        workers,
                    )
                    .await?;
            }
            for _sample in 0..Self::SAMPLES {
                for (binary, samples) in [
                    (&baseline, &mut baseline_samples),
                    (&optimized, &mut optimized_samples),
                ] {
                    let receipt = journal
                        .execute(
                            &mut Self::bench(binary, parameters),
                            ToolsEnginePgoEnumStage::Sample,
                            workers,
                            ToolsEnginePgoEnumInput::HoldOpen,
                        )
                        .await?;
                    receipt.require_success()?;
                    let (nodes, nps) = Self::bench_output(&receipt.stdout)?;
                    samples.push(ToolsEnginePgoSample {
                        workers,
                        nodes,
                        nps,
                        elapsed_microseconds: receipt.elapsed_microseconds,
                    });
                }
            }
        }
        let expected = baseline_samples
            .first()
            .ok_or(ToolsEnginePgoError::Comparison)?
            .nodes;
        if baseline_samples
            .iter()
            .chain(&optimized_samples)
            .filter(|sample| sample.workers == 1)
            .any(|sample| sample.nodes != expected)
        {
            return Err(ToolsEnginePgoError::Comparison);
        }
        journal.comparison = Some(ToolsEnginePgoComparison {
            positions: WorkbenchBenchParameters::POSITIONS,
            depth: WorkbenchBenchParameters::DEFAULT_DEPTH,
            baseline: journal.binary(&baseline).await?,
            optimized: journal.binary(&optimized).await?,
            baseline_samples,
            optimized_samples,
            deterministic_one_worker_nodes: expected,
            behavior: baseline_behavior,
        });
        journal.verify_unchanged().await
    }

    fn bench(binary: &Path, parameters: WorkbenchBenchParameters) -> Command {
        let mut command = Command::new(binary);
        command.arg(format!(
            "bench {} {} {}",
            parameters.hash_mib(),
            parameters.threads(),
            parameters.depth()
        ));
        command
    }

    fn bench_output(text: &str) -> Result<(u64, u64), ToolsEnginePgoError> {
        let tokens = text.split_whitespace().collect::<Vec<_>>();
        let ["Bench:", nodes, "nodes", nps, "nps"] = tokens.as_slice() else {
            return Err(ToolsEnginePgoError::Comparison);
        };
        let nodes = nodes
            .parse()
            .map_err(|_error| ToolsEnginePgoError::Comparison)?;
        let nps = nps
            .parse()
            .map_err(|_error| ToolsEnginePgoError::Comparison)?;
        if nodes == 0 || nps == 0 {
            return Err(ToolsEnginePgoError::Comparison);
        }
        Ok((nodes, nps))
    }

    pub(crate) async fn check_queued(
        root: PathBuf,
        output: PathBuf,
        resources: ToolsEnginePgoResources<'_>,
    ) -> Result<PathBuf, ToolsEnginePgoError> {
        let inputs = ToolsEnginePgoInputs::new(root.clone(), PathBuf::new(), output);
        let mut journal = ToolsEnginePgoJournal::new(&inputs, resources).await?;
        let outcome = async {
            journal.inspect_application().await?;
            let package = resources
                .executor
                .submit(move |_cancellation| {
                    Ok::<_, ToolsEnginePgoError>(EnginePackageSelector::load(
                        &root.join("resources/engine"),
                        EnginePackageHost::detect()?,
                    )?)
                })?
                .join()
                .await??;
            let binary = journal.binary(package.executable()).await?;
            let capabilities = journal.identity(package.executable()).await?;
            capabilities.validate()?;
            let observation = journal.queued(package.executable()).await?;
            let disposition = observation.disposition();
            let receipt = ToolsEnginePgoQueuedReceipt {
                engine_version: EnginePackageManifest::VERSION,
                source_commit: EnginePackageManifest::COMMIT,
                model_sha256: EnginePackageManifest::NETWORK_SHA256,
                model_bytes: EnginePackageManifest::NETWORK_BYTES,
                target: package.target(),
                binary,
                observation,
            };
            ToolsEnginePgoJournal::write_json(&journal.output.join("package.json"), &receipt)
                .await?;
            Ok(disposition)
        }
        .await;
        journal.finish(outcome).await
    }
}

#[cfg(test)]
mod owner_regression_tests {
    use std::error::Error;
    use std::num::NonZeroUsize;
    use std::path::PathBuf;
    use std::process::Stdio;
    #[cfg(unix)]
    use std::time::Duration;

    use chess::capacity::budget::CapacityBudget;
    use chess::capacity::executor::CapacityExecutor;
    use chess::engine::package::manifest::{EnginePackageEnumTarget, EnginePackageManifest};
    use chess::package::digest::PackageDigest;
    #[cfg(unix)]
    use serde::Deserialize;
    #[cfg(unix)]
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;
    #[cfg(unix)]
    use tokio::time::Instant;

    use crate::engine::pgo::{
        ToolsEnginePgoCancellation, ToolsEnginePgoEnumInstrumentation, ToolsEnginePgoEnumStage,
        ToolsEnginePgoError, ToolsEnginePgoInputs, ToolsEnginePgoJournal, ToolsEnginePgoProvenance,
        ToolsEnginePgoResources,
    };
    #[cfg(unix)]
    use crate::engine::pgo::{
        ToolsEnginePgoEnumCleanup, ToolsEnginePgoEnumInput, ToolsEnginePgoProcessRequest,
    };

    struct ToolsEnginePgoRegressionFixture {
        directory: tempfile::TempDir,
        source: PathBuf,
        model: PathBuf,
        output: PathBuf,
    }

    #[cfg(unix)]
    struct ToolsEnginePgoRegressionHelper {
        stdin: tokio::process::ChildStdin,
        stdout: tokio::process::ChildStdout,
    }

    #[cfg(unix)]
    impl ToolsEnginePgoRegressionHelper {
        async fn close(mut self) -> Result<bool, Box<dyn Error>> {
            let write = self.stdin.write_all(b"cleanup fixture\n").await;
            drop(self.stdin);
            let mut drained = Vec::new();
            self.stdout.read_to_end(&mut drained).await?;
            match write {
                Ok(()) => Ok(false),
                Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(true),
                Err(error) => Err(error.into()),
            }
        }
    }

    #[cfg(unix)]
    #[derive(Deserialize)]
    struct ToolsEnginePgoRegressionReport {
        receipts: Vec<PathBuf>,
        failure: Option<String>,
        #[serde(default)]
        unpublished_receipt: Option<ToolsEnginePgoRegressionUnpublishedReceipt>,
    }

    #[cfg(unix)]
    #[derive(Deserialize)]
    struct ToolsEnginePgoRegressionUnpublishedReceipt {
        path: PathBuf,
        receipt: ToolsEnginePgoRegressionReceipt,
    }

    #[cfg(unix)]
    #[derive(Deserialize)]
    struct ToolsEnginePgoRegressionReceipt {
        program: String,
        arguments: Vec<String>,
        workers: usize,
        exit: ToolsEnginePgoRegressionEnumExit,
        stdout: String,
        stderr: String,
    }

    #[cfg(unix)]
    #[derive(Deserialize)]
    enum ToolsEnginePgoRegressionEnumExit {
        Cancelled,
        Exited {
            success: bool,
            code: Option<i32>,
            status: String,
        },
    }

    impl ToolsEnginePgoRegressionFixture {
        const LOCK: &[u8] = b"fixture lock\n";
        const MODEL: &[u8] = b"original model";
        const REPLACED_MODEL: &[u8] = b"replaced model";
        const INITIAL_COMMIT: &str = "c26079b3e03cd248df50029918262364b051b38f";
        const DATE: &str = "2000-01-01T00:00:00+0000";

        async fn new() -> Result<Self, Box<dyn Error>> {
            let scratch = std::env::temp_dir().join("claude-chess-pgo-owner-regressions-20261016");
            tokio::fs::create_dir_all(&scratch).await?;
            let directory = tempfile::Builder::new()
                .prefix("fixture-")
                .tempdir_in(scratch)?;
            let fixture = Self {
                source: directory.path().join("source"),
                model: directory.path().join("model"),
                output: directory.path().join("output"),
                directory,
            };
            tokio::fs::create_dir(&fixture.source).await?;
            tokio::fs::write(fixture.source.join("Cargo.lock"), Self::LOCK).await?;
            tokio::fs::write(&fixture.model, Self::MODEL).await?;
            fixture
                .git(&["init", "--quiet", "--object-format=sha1"], None)
                .await?;
            fixture.git(&["add", "--", "Cargo.lock"], None).await?;
            let tree = fixture.git(&["write-tree"], None).await?;
            let commit = fixture
                .git(&["commit-tree", tree.trim()], Some(b"initial fixture\n"))
                .await?;
            if commit.trim() != Self::INITIAL_COMMIT {
                return Err(std::io::Error::other("Deterministic fixture commit differs").into());
            }
            fixture
                .git(&["update-ref", "HEAD", Self::INITIAL_COMMIT], None)
                .await?;
            Ok(fixture)
        }

        async fn git(&self, args: &[&str], input: Option<&[u8]>) -> Result<String, Box<dyn Error>> {
            let mut command = Command::new("git");
            command
                .arg("-C")
                .arg(&self.source)
                .args(["-c", "core.autocrlf=false"])
                .args(args)
                .env("GIT_AUTHOR_NAME", "Fixture")
                .env("GIT_COMMITTER_NAME", "Fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .env("GIT_AUTHOR_DATE", Self::DATE)
                .env("GIT_COMMITTER_DATE", Self::DATE)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            let mut child = command.spawn()?;
            if let Some(input) = input {
                child
                    .stdin
                    .as_mut()
                    .ok_or("Fixture stdin missing")?
                    .write_all(input)
                    .await?;
            }
            drop(child.stdin.take());
            let output = child.wait_with_output().await?;
            if !output.status.success() {
                return Err(std::io::Error::other(String::from_utf8(output.stderr)?).into());
            }
            Ok(String::from_utf8(output.stdout)?)
        }

        async fn journal<'a>(
            &self,
            resources: ToolsEnginePgoResources<'a>,
        ) -> Result<ToolsEnginePgoJournal<'a>, Box<dyn Error>> {
            let inputs = ToolsEnginePgoInputs::new(
                self.source.clone(),
                self.model.clone(),
                self.output.clone(),
            );
            let mut journal = ToolsEnginePgoJournal::new(&inputs, resources).await?;
            journal.provenance = Some(ToolsEnginePgoProvenance {
                engine_version: EnginePackageManifest::VERSION,
                source_commit: Self::INITIAL_COMMIT,
                source: self.source.clone(),
                lock_sha256: PackageDigest::read(Self::LOCK)?,
                model: self.model.clone(),
                model_bytes: u64::try_from(Self::MODEL.len())?,
                model_sha256: PackageDigest::read(Self::MODEL)?,
                target: EnginePackageEnumTarget::current()?,
                workers: resources.budget.workers(),
                instrumentation: ToolsEnginePgoEnumInstrumentation::Full,
            });
            Ok(journal)
        }

        async fn change_head(&self) -> Result<(), Box<dyn Error>> {
            let tree = self.git(&["write-tree"], None).await?;
            let next = self
                .git(
                    &["commit-tree", tree.trim(), "-p", Self::INITIAL_COMMIT],
                    Some(b"different clean fixture\n"),
                )
                .await?;
            self.git(&["update-ref", "HEAD", next.trim()], None).await?;
            Ok(())
        }

        #[cfg(unix)]
        fn held_helper(
            journal: &mut ToolsEnginePgoJournal<'_>,
        ) -> Result<ToolsEnginePgoRegressionHelper, Box<dyn Error>> {
            journal.process.start(
                &mut Command::new("/bin/cat"),
                ToolsEnginePgoProcessRequest::new(
                    ToolsEnginePgoEnumStage::Training,
                    1,
                    ToolsEnginePgoEnumInput::HoldOpen,
                    Duration::from_secs(60),
                ),
            )?;
            let mut helper = Command::new("/bin/cat")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(false)
                .spawn()?;
            let stdin = helper.stdin.take().ok_or("Fixture helper stdin missing")?;
            let stdout = helper
                .stdout
                .take()
                .ok_or("Fixture helper stdout missing")?;
            let active = journal
                .process
                .active
                .as_mut()
                .ok_or("Fixture child missing")?;
            active.cleanup = ToolsEnginePgoEnumCleanup::Tree(helper);
            active.cleanup_deadline = Some(Instant::now());
            Ok(ToolsEnginePgoRegressionHelper { stdin, stdout })
        }

        #[cfg(unix)]
        fn primary_is_cancelled(error: &ToolsEnginePgoError) -> bool {
            if matches!(error, ToolsEnginePgoError::Cancelled) {
                return true;
            }
            if let ToolsEnginePgoError::WorkflowCleanup { operation, .. }
            | ToolsEnginePgoError::ReportPublication { operation, .. }
            | ToolsEnginePgoError::InputValidation { operation, .. } = error
            {
                Self::primary_is_cancelled(operation)
            } else {
                false
            }
        }

        #[cfg(unix)]
        async fn terminal_receipt(
            &self,
            report: &ToolsEnginePgoRegressionReport,
        ) -> Result<Option<ToolsEnginePgoRegressionReceipt>, Box<dyn Error>> {
            if let Some(path) = report.receipts.last() {
                return Ok(Some(serde_json::from_slice(&tokio::fs::read(path).await?)?));
            }
            Ok(None)
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_observe_cleanup_failure_keeps_primary_and_terminal_receipt()
    -> Result<(), Box<dyn Error>> {
        let fixture = ToolsEnginePgoRegressionFixture::new().await?;
        let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("Invalid fixture capacity")?)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let cancellation = ToolsEnginePgoCancellation::new();
        let mut journal = fixture
            .journal(ToolsEnginePgoResources::new(
                &budget,
                &executor,
                &cancellation,
            ))
            .await?;
        let helper = ToolsEnginePgoRegressionFixture::held_helper(&mut journal)?;
        cancellation.cancel();
        let observed = journal.process.observe().await;
        let observed_error = observed
            .err()
            .ok_or("Held cleanup helper unexpectedly completed")?;
        let result = journal.finish(Err(observed_error)).await;
        let helper_reaped = helper.close().await?;
        let report: ToolsEnginePgoRegressionReport =
            serde_json::from_slice(&tokio::fs::read(fixture.output.join("report.json")).await?)?;
        let terminal = fixture.terminal_receipt(&report).await?;
        executor.shutdown().await?;
        fixture.directory.close()?;
        let error = result.err().ok_or("Cancellation unexpectedly succeeded")?;
        assert!(helper_reaped);
        assert_eq!(budget.available(), budget.workers());
        assert!(
            ToolsEnginePgoRegressionFixture::primary_is_cancelled(&error),
            "Cleanup failure erased the actual Cancelled outcome"
        );
        assert!(report.failure.is_some());
        let terminal = terminal.ok_or("Actual terminal receipt was discarded")?;
        assert_eq!(terminal.program, "/bin/cat");
        assert!(terminal.arguments.is_empty());
        assert_eq!(terminal.workers, 1);
        assert!(matches!(
            terminal.exit,
            ToolsEnginePgoRegressionEnumExit::Cancelled
        ));
        assert!(terminal.stdout.is_empty());
        assert!(terminal.stderr.is_empty());
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_child_receipt_publication_keeps_typed_primary_and_inline_receipt()
    -> Result<(), Box<dyn Error>> {
        let fixture = ToolsEnginePgoRegressionFixture::new().await?;
        let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("Invalid fixture capacity")?)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let cancellation = ToolsEnginePgoCancellation::new();
        let mut journal = fixture
            .journal(ToolsEnginePgoResources::new(
                &budget,
                &executor,
                &cancellation,
            ))
            .await?;
        let conflict = journal.output.join("command-000.json");
        tokio::fs::write(&conflict, b"preserve existing command receipt").await?;
        let execution = journal
            .execute(
                &mut Command::new("/usr/bin/false"),
                ToolsEnginePgoEnumStage::Training,
                1,
                ToolsEnginePgoEnumInput::HoldOpen,
            )
            .await;
        let operation_error = execution
            .err()
            .ok_or("Conflicting command receipt unexpectedly published")?;
        let result = journal.finish(Err(operation_error)).await;
        let report: ToolsEnginePgoRegressionReport =
            serde_json::from_slice(&tokio::fs::read(fixture.output.join("report.json")).await?)?;
        let preserved = tokio::fs::read(&conflict).await?;
        executor.shutdown().await?;
        fixture.directory.close()?;
        let error = result.err().ok_or("Failed child unexpectedly succeeded")?;
        if let ToolsEnginePgoError::ReportPublication {
            operation,
            publication,
        } = error
        {
            assert!(matches!(
                *operation,
                ToolsEnginePgoError::Command { code: Some(1), .. }
            ));
            assert!(
                matches!(*publication, ToolsEnginePgoError::Io(ref source) if source.kind() == std::io::ErrorKind::AlreadyExists)
            );
        } else {
            return Err(std::io::Error::other(format!(
                "Receipt publication erased child failure: {error}"
            ))
            .into());
        }
        assert_eq!(preserved, b"preserve existing command receipt");
        assert!(report.receipts.is_empty());
        assert!(report.failure.is_some());
        let inline = report
            .unpublished_receipt
            .ok_or("Actual failed-child receipt missing inline")?;
        assert_eq!(inline.path, conflict);
        assert_eq!(inline.receipt.program, "/usr/bin/false");
        assert!(inline.receipt.arguments.is_empty());
        assert_eq!(inline.receipt.workers, 1);
        assert!(
            matches!(inline.receipt.exit, ToolsEnginePgoRegressionEnumExit::Exited { success: false, code: Some(1), ref status } if !status.is_empty())
        );
        assert!(inline.receipt.stdout.is_empty());
        assert!(inline.receipt.stderr.is_empty());
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn consuming_finish_reaps_held_helper_before_releasing_capacity()
    -> Result<(), Box<dyn Error>> {
        let fixture = ToolsEnginePgoRegressionFixture::new().await?;
        let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("Invalid fixture capacity")?)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let cancellation = ToolsEnginePgoCancellation::new();
        let mut journal = fixture
            .journal(ToolsEnginePgoResources::new(
                &budget,
                &executor,
                &cancellation,
            ))
            .await?;
        let helper = ToolsEnginePgoRegressionFixture::held_helper(&mut journal)?;
        let result = journal.finish(Err(ToolsEnginePgoError::Deadline)).await;
        let helper_reaped = helper.close().await?;
        let available = budget.available();
        executor.shutdown().await?;
        fixture.directory.close()?;
        assert!(result.is_err());
        assert!(
            helper_reaped,
            "Consuming finish returned while its cleanup helper remained live"
        );
        assert_eq!(available, budget.workers());
        Ok(())
    }

    #[tokio::test]
    async fn final_provenance_rejects_changed_clean_head_with_same_lock()
    -> Result<(), Box<dyn Error>> {
        let fixture = ToolsEnginePgoRegressionFixture::new().await?;
        let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("Invalid fixture capacity")?)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let cancellation = ToolsEnginePgoCancellation::new();
        let mut journal = fixture
            .journal(ToolsEnginePgoResources::new(
                &budget,
                &executor,
                &cancellation,
            ))
            .await?;
        journal.verify_unchanged().await?;
        fixture.change_head().await?;
        let result = journal.verify_unchanged().await;
        drop(journal);
        executor.shutdown().await?;
        fixture.directory.close()?;
        assert!(
            matches!(result, Err(ToolsEnginePgoError::SourceMismatch)),
            "Changed clean HEAD retained stale source provenance"
        );
        Ok(())
    }

    #[tokio::test]
    async fn final_provenance_rejects_same_length_model_replacement() -> Result<(), Box<dyn Error>>
    {
        let fixture = ToolsEnginePgoRegressionFixture::new().await?;
        let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("Invalid fixture capacity")?)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let cancellation = ToolsEnginePgoCancellation::new();
        let mut journal = fixture
            .journal(ToolsEnginePgoResources::new(
                &budget,
                &executor,
                &cancellation,
            ))
            .await?;
        journal.verify_unchanged().await?;
        if ToolsEnginePgoRegressionFixture::MODEL.len()
            != ToolsEnginePgoRegressionFixture::REPLACED_MODEL.len()
        {
            return Err(std::io::Error::other("Fixture model sizes differ").into());
        }
        tokio::fs::write(
            &fixture.model,
            ToolsEnginePgoRegressionFixture::REPLACED_MODEL,
        )
        .await?;
        let result = journal.verify_unchanged().await;
        drop(journal);
        executor.shutdown().await?;
        fixture.directory.close()?;
        assert!(
            matches!(result, Err(ToolsEnginePgoError::SourceMismatch)),
            "Replaced model retained stale digest provenance"
        );
        Ok(())
    }

    #[tokio::test]
    async fn report_publication_failure_preserves_original_operation_and_cleanup()
    -> Result<(), Box<dyn Error>> {
        let fixture = ToolsEnginePgoRegressionFixture::new().await?;
        let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("Invalid fixture capacity")?)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let cancellation = ToolsEnginePgoCancellation::new();
        let journal = fixture
            .journal(ToolsEnginePgoResources::new(
                &budget,
                &executor,
                &cancellation,
            ))
            .await?;
        let report = journal.output.join("report.json");
        tokio::fs::write(&report, b"preserve report").await?;
        let original = ToolsEnginePgoError::WorkflowCleanup {
            operation: Box::new(ToolsEnginePgoError::Command {
                stage: ToolsEnginePgoEnumStage::Training,
                code: Some(17),
                status: "exit status: 17".to_owned(),
            }),
            cleanup: Box::new(ToolsEnginePgoError::Cleanup(
                "controlled original cleanup error".to_owned(),
            )),
        };
        let original_message = original.to_string();
        let result = journal.finish(Err(original)).await;
        let existing_report = tokio::fs::read(&report).await?;
        executor.shutdown().await?;
        fixture.directory.close()?;
        let error = result
            .err()
            .ok_or("Report publication unexpectedly succeeded")?;
        assert!(
            error.to_string().contains(&original_message),
            "Report publication erased the original typed failure"
        );
        assert!(error.source().is_some());
        if let ToolsEnginePgoError::ReportPublication {
            operation,
            publication,
        } = error
        {
            assert!(matches!(
                *operation,
                ToolsEnginePgoError::WorkflowCleanup { .. }
            ));
            assert!(
                matches!(*publication, ToolsEnginePgoError::Io(ref source) if source.kind() == std::io::ErrorKind::AlreadyExists)
            );
            assert!(publication.source().is_some());
        } else {
            return Err(error.into());
        }
        assert_eq!(existing_report, b"preserve report");
        Ok(())
    }
}
