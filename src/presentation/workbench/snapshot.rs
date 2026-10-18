use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use strum::IntoEnumIterator;

use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::engine::capabilities::{EngineCapabilities, EngineCapabilitiesError};
use crate::engine::enums::EngineEnumOption;
use crate::engine::options::{
    EngineOptionsApplied, EngineOptionsEnumTablebases, EngineOptionsEnumValue,
};
use crate::engine::package::selector::EnginePackageSelected;
use crate::engine::tablebase::status::{EngineTablebaseStatus, EngineTablebaseStatusEnumState};
use crate::presentation::workbench::configuration::PresentationWorkbenchConfiguration;
use crate::workbench::enums::{
    WorkbenchEnumCompletion, WorkbenchEnumJobState, WorkbenchEnumOperation, WorkbenchEnumOutcome,
    WorkbenchEnumParameters, WorkbenchEnumResult,
};
use crate::workbench::job::{
    WorkbenchJob, WorkbenchJobCancellation, WorkbenchJobDeadline, WorkbenchJobId,
    WorkbenchJobProgress,
};
use crate::workbench::parameters::WorkbenchBenchParameters;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentationWorkbenchTablebaseEnumState {
    Unconfigured,
    Configured,
    Loaded(NonZeroU32),
    Unavailable,
    Failed(Box<str>),
}

#[derive(Debug, Clone)]
pub struct PresentationWorkbenchTablebaseSnapshot {
    directories: Vec<PathBuf>,
    alias: Option<PathBuf>,
    state: PresentationWorkbenchTablebaseEnumState,
}

impl PresentationWorkbenchTablebaseSnapshot {
    fn capture(
        status: Option<&EngineTablebaseStatus>,
        applied: Option<&EngineOptionsApplied>,
    ) -> Self {
        if let Some(status) = status {
            let state = match status.state() {
                EngineTablebaseStatusEnumState::Configured => {
                    PresentationWorkbenchTablebaseEnumState::Configured
                }
                EngineTablebaseStatusEnumState::Loaded { pieces } => {
                    PresentationWorkbenchTablebaseEnumState::Loaded(*pieces)
                }
                EngineTablebaseStatusEnumState::Unavailable => {
                    PresentationWorkbenchTablebaseEnumState::Unavailable
                }
                EngineTablebaseStatusEnumState::Failed(error) => {
                    PresentationWorkbenchTablebaseEnumState::Failed(error.to_string().into())
                }
            };
            return Self {
                directories: status.source_directories().to_vec(),
                alias: Some(PathBuf::from(status.engine_path().as_str())),
                state,
            };
        }
        let (alias, state) = match applied.map(EngineOptionsApplied::tablebases) {
            None | Some(EngineOptionsEnumTablebases::Unconfigured) => {
                (None, PresentationWorkbenchTablebaseEnumState::Unconfigured)
            }
            Some(EngineOptionsEnumTablebases::Loaded { path, pieces }) => (
                Some(PathBuf::from(path.as_str())),
                PresentationWorkbenchTablebaseEnumState::Loaded(*pieces),
            ),
            Some(EngineOptionsEnumTablebases::Unavailable { path }) => (
                Some(PathBuf::from(path.as_str())),
                PresentationWorkbenchTablebaseEnumState::Unavailable,
            ),
        };
        Self {
            directories: Vec::new(),
            alias,
            state,
        }
    }

    pub fn directories(&self) -> &[PathBuf] {
        &self.directories
    }
    pub fn alias(&self) -> Option<&Path> {
        self.alias.as_deref()
    }
    pub const fn state(&self) -> &PresentationWorkbenchTablebaseEnumState {
        &self.state
    }
}

#[derive(Debug, Clone)]
pub struct PresentationWorkbenchSnapshot {
    package: EnginePackageSelected,
    capabilities: EngineCapabilities,
    applied: Option<EngineOptionsApplied>,
    tablebases: PresentationWorkbenchTablebaseSnapshot,
}

impl PresentationWorkbenchSnapshot {
    pub fn capture(
        package: &EnginePackageSelected,
        capabilities: &EngineCapabilities,
        applied: Option<&EngineOptionsApplied>,
        tablebases: Option<&EngineTablebaseStatus>,
    ) -> Result<Self, EngineCapabilitiesError> {
        capabilities.validate()?;
        Ok(Self {
            package: package.clone(),
            capabilities: capabilities.clone(),
            applied: applied.cloned(),
            tablebases: PresentationWorkbenchTablebaseSnapshot::capture(tablebases, applied),
        })
    }

    pub const fn package(&self) -> &EnginePackageSelected {
        &self.package
    }
    pub const fn capabilities(&self) -> &EngineCapabilities {
        &self.capabilities
    }
    pub const fn applied(&self) -> Option<&EngineOptionsApplied> {
        self.applied.as_ref()
    }
    pub const fn tablebases(&self) -> &PresentationWorkbenchTablebaseSnapshot {
        &self.tablebases
    }
    pub fn options(&self) -> impl ExactSizeIterator<Item = EngineEnumOption> {
        EngineEnumOption::iter()
    }

    pub fn value(&self, option: EngineEnumOption) -> Option<EngineOptionsEnumValue> {
        let applied = self.applied.as_ref()?;
        match option {
            EngineEnumOption::Hash => applied
                .resources()
                .map(|resources| EngineOptionsEnumValue::Hash(resources.hash_mib())),
            EngineEnumOption::Threads => applied
                .resources()
                .map(|resources| EngineOptionsEnumValue::Threads(resources.threads())),
            EngineEnumOption::MoveOverhead => applied
                .resources()
                .map(|resources| EngineOptionsEnumValue::MoveOverhead(resources.move_overhead())),
            EngineEnumOption::Minimal => {
                Some(EngineOptionsEnumValue::Minimal(applied.is_minimal()))
            }
            EngineEnumOption::Chess960 => Some(EngineOptionsEnumValue::Chess960(applied.variant())),
            EngineEnumOption::MultiPv => {
                Some(EngineOptionsEnumValue::MultiPv(applied.multi_pv().get()))
            }
            EngineEnumOption::ClearHash => None,
            EngineEnumOption::SyzygyPath => match applied.tablebases() {
                EngineOptionsEnumTablebases::Loaded { path, .. }
                | EngineOptionsEnumTablebases::Unavailable { path } => {
                    Some(EngineOptionsEnumValue::SyzygyPath(path.clone()))
                }
                EngineOptionsEnumTablebases::Unconfigured => None,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct PresentationWorkbenchSnapshotRow {
    label: Box<str>,
    value: Box<str>,
}

impl PresentationWorkbenchSnapshotRow {
    fn new(label: impl Into<Box<str>>, value: impl ToString) -> Self {
        Self {
            label: label.into(),
            value: value.to_string().into(),
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Debug, Clone)]
pub struct PresentationWorkbenchJobSnapshot {
    id: WorkbenchJobId,
    package: EnginePackageSelected,
    operation: WorkbenchEnumOperation,
    parameters: Box<str>,
    state: WorkbenchEnumJobState,
    progress: WorkbenchJobProgress,
    timeout: WorkbenchJobDeadline,
    created_at: ClockInstant,
    observed_at: ClockInstant,
    elapsed: ClockDuration,
    deadline: ClockInstant,
    remaining: Option<ClockDuration>,
    cancellation: Option<WorkbenchJobCancellation>,
    rows: Vec<PresentationWorkbenchSnapshotRow>,
    omitted_rows: usize,
    diagnostics: Vec<Box<str>>,
    omitted_lines: u64,
    failure: Option<Box<str>>,
}

impl PresentationWorkbenchJobSnapshot {
    pub const MAX_RESULT_ROWS: usize = 128;
    pub const MAX_DIAGNOSTIC_LINES: usize = 32;

    pub fn capture(job: &WorkbenchJob) -> Result<Self, ClockDurationError> {
        let parameters = match job.parameters() {
            WorkbenchEnumParameters::Inspector(position)
            | WorkbenchEnumParameters::RawEvaluation(position) => position.current().fen(),
            WorkbenchEnumParameters::Compiler => job.package().target().triple().to_owned(),
            WorkbenchEnumParameters::Perft(parameters) => format!(
                "Depth {}: {}",
                parameters.depth().value(),
                parameters.position().current().fen()
            ),
            WorkbenchEnumParameters::Bench(parameters) => format!(
                "{} positions, {} MiB, {} threads, depth {}",
                WorkbenchBenchParameters::POSITIONS,
                parameters.hash_mib(),
                parameters.threads(),
                parameters.depth()
            ),
        };
        let mut snapshot = Self {
            id: job.id(),
            package: job.package().clone(),
            operation: job.parameters().operation(),
            parameters: parameters.into(),
            state: job.state(),
            progress: job.progress(),
            timeout: job.timeout(),
            created_at: job.created_at(),
            observed_at: job.observed_at(),
            elapsed: job.observed_at().elapsed_since(job.created_at())?,
            deadline: job.deadline(),
            remaining: if job.observed_at() < job.deadline() {
                Some(job.deadline().elapsed_since(job.observed_at())?)
            } else {
                None
            },
            cancellation: job.cancellation(),
            rows: Vec::new(),
            omitted_rows: 0,
            diagnostics: job
                .diagnostics()
                .entries()
                .rev()
                .take(Self::MAX_DIAGNOSTIC_LINES)
                .map(|entry| format!("{:?}: {}", entry.pipe(), entry.text()).into())
                .collect(),
            omitted_lines: job
                .diagnostics()
                .evicted_lines()
                .saturating_add(u64::try_from(
                    job.diagnostics()
                        .entries()
                        .len()
                        .saturating_sub(Self::MAX_DIAGNOSTIC_LINES),
                )?),
            failure: None,
        };
        snapshot.diagnostics.reverse();
        match job.outcome() {
            None
            | Some(WorkbenchEnumOutcome::Cancelled {
                completion: None | Some(WorkbenchEnumCompletion::Terminated),
                ..
            }) => {}
            Some(WorkbenchEnumOutcome::Succeeded(result))
            | Some(WorkbenchEnumOutcome::Cancelled {
                completion: Some(WorkbenchEnumCompletion::Result(result)),
                ..
            }) => snapshot.project_result(result),
            Some(WorkbenchEnumOutcome::Failed(failure))
            | Some(WorkbenchEnumOutcome::Cancelled {
                completion: Some(WorkbenchEnumCompletion::Failed(failure)),
                ..
            }) => snapshot.failure = Some(failure.to_string().into()),
        }
        Ok(snapshot)
    }

    pub const fn id(&self) -> WorkbenchJobId {
        self.id
    }
    pub const fn package(&self) -> &EnginePackageSelected {
        &self.package
    }
    pub const fn operation(&self) -> WorkbenchEnumOperation {
        self.operation
    }
    pub fn parameters(&self) -> &str {
        &self.parameters
    }
    pub const fn state(&self) -> WorkbenchEnumJobState {
        self.state
    }
    pub const fn progress(&self) -> WorkbenchJobProgress {
        self.progress
    }
    pub const fn timeout(&self) -> WorkbenchJobDeadline {
        self.timeout
    }
    pub const fn created_at(&self) -> ClockInstant {
        self.created_at
    }
    pub const fn observed_at(&self) -> ClockInstant {
        self.observed_at
    }
    pub const fn elapsed(&self) -> ClockDuration {
        self.elapsed
    }
    pub const fn deadline(&self) -> ClockInstant {
        self.deadline
    }
    pub const fn remaining(&self) -> Option<ClockDuration> {
        self.remaining
    }
    pub const fn cancellation(&self) -> Option<WorkbenchJobCancellation> {
        self.cancellation
    }
    pub fn rows(&self) -> &[PresentationWorkbenchSnapshotRow] {
        &self.rows
    }
    pub const fn omitted_rows(&self) -> usize {
        self.omitted_rows
    }
    pub fn diagnostics(&self) -> &[Box<str>] {
        &self.diagnostics
    }
    pub const fn omitted_lines(&self) -> u64 {
        self.omitted_lines
    }
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub const fn is_terminal(&self) -> bool {
        match self.state {
            WorkbenchEnumJobState::Succeeded
            | WorkbenchEnumJobState::Failed
            | WorkbenchEnumJobState::Cancelled
            | WorkbenchEnumJobState::TimedOut => true,
            WorkbenchEnumJobState::Queued
            | WorkbenchEnumJobState::Admitted
            | WorkbenchEnumJobState::Running
            | WorkbenchEnumJobState::Cancelling => false,
        }
    }

    pub fn title(&self) -> &'static str {
        PresentationWorkbenchConfiguration::operation_label(self.operation)
    }

    fn project_result(&mut self, result: &WorkbenchEnumResult) {
        match result {
            WorkbenchEnumResult::Inspector(position) => {
                self.row("Validated FEN", position.fen());
                self.row("Side to move", format!("{:?}", position.turn()));
                self.row("Legal root moves", position.legal_moves().len());
            }
            WorkbenchEnumResult::RawEvaluation(evaluation) => self.row(
                "Raw White-perspective NNUE integer (not search cp)",
                evaluation.white_value(),
            ),
            WorkbenchEnumResult::Compiler(result) => {
                self.row("Compiler", result.compiler());
                self.row("Target", result.target().triple());
                self.row(
                    "Target features",
                    if result.target_features().is_empty() {
                        "No target features reported"
                    } else {
                        result.target_features()
                    },
                );
            }
            WorkbenchEnumResult::Perft(result) => {
                self.row("Depth (plies)", result.depth().value());
                self.row("Nodes", result.nodes());
                self.row("Elapsed (ms)", result.elapsed().as_millis());
                self.row("Nodes per second", result.nodes_per_second());
                for root in result.roots() {
                    self.row(
                        format!("Root {}", root.movement().uci(result.position().variant())),
                        root.nodes(),
                    );
                }
            }
            WorkbenchEnumResult::Bench(result) => {
                self.row("Positions", result.positions());
                self.row("Nodes", result.nodes());
                self.row("Elapsed (ms)", result.elapsed().as_millis());
                self.row("Nodes per second", result.nodes_per_second());
            }
        }
    }

    fn row(&mut self, label: impl Into<Box<str>>, value: impl ToString) {
        if self.rows.len() < Self::MAX_RESULT_ROWS {
            self.rows
                .push(PresentationWorkbenchSnapshotRow::new(label, value));
        } else {
            self.omitted_rows = self.omitted_rows.saturating_add(1);
        }
    }
}
