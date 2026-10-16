use std::time::Duration;

use thiserror::Error;

use crate::clock::duration::{ClockDuration, ClockDurationError};
use crate::engine::capabilities::EngineCapabilities;
use crate::engine::enums::EngineEnumOption;
use crate::engine::options::{EngineOptionsEnumValue, EngineOptionsError, EngineOptionsSetting};
use crate::engine::search::position::EngineSearchPosition;
use crate::workbench::enums::{WorkbenchEnumOperation, WorkbenchEnumParameters};
use crate::workbench::job::{WorkbenchJobDeadline, WorkbenchJobError};
use crate::workbench::parameters::{
    WorkbenchBenchParameters, WorkbenchParametersError, WorkbenchPerftDepth,
    WorkbenchPerftParameters,
};

#[derive(Debug, Error)]
pub enum PresentationWorkbenchConfigurationError {
    #[error("{0}: enter a whole non-negative number")]
    Number(&'static str),
    #[error("{0:?} does not accept numeric input")]
    NonNumeric(EngineEnumOption),
    #[error("Select a validated position before running this diagnostic")]
    PositionRequired,
    #[error(transparent)]
    Options(#[from] EngineOptionsError),
    #[error(transparent)]
    Parameters(#[from] WorkbenchParametersError),
    #[error(transparent)]
    Deadline(#[from] WorkbenchJobError),
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
}

pub struct PresentationWorkbenchConfiguration {
    pub(super) perft_depth: String,
    pub(super) bench_hash: String,
    pub(super) bench_threads: String,
    pub(super) bench_depth: String,
    pub(super) deadline_millis: String,
}

impl Default for PresentationWorkbenchConfiguration {
    fn default() -> Self {
        Self {
            perft_depth: WorkbenchPerftDepth::default().value().to_string(),
            bench_hash: String::new(),
            bench_threads: String::new(),
            bench_depth: String::new(),
            deadline_millis: WorkbenchJobDeadline::default()
                .duration()
                .as_millis()
                .to_string(),
        }
    }
}

impl PresentationWorkbenchConfiguration {
    pub fn set_perft_depth(&mut self, value: String) {
        self.perft_depth = value;
    }

    pub fn set_bench(&mut self, hash: String, threads: String, depth: String) {
        self.bench_hash = hash;
        self.bench_threads = threads;
        self.bench_depth = depth;
    }

    pub fn set_deadline_millis(&mut self, value: String) {
        self.deadline_millis = value;
    }

    pub fn number(
        option: EngineEnumOption,
        input: &str,
        capabilities: &EngineCapabilities,
    ) -> Result<EngineOptionsSetting, PresentationWorkbenchConfigurationError> {
        let value = match option {
            EngineEnumOption::Hash => {
                EngineOptionsEnumValue::Hash(Self::parse(input, option.name())?)
            }
            EngineEnumOption::Threads => {
                EngineOptionsEnumValue::Threads(Self::parse(input, option.name())?)
            }
            EngineEnumOption::MoveOverhead => EngineOptionsEnumValue::MoveOverhead(
                ClockDuration::from_millis(Self::parse(input, option.name())?)?,
            ),
            EngineEnumOption::MultiPv => {
                EngineOptionsEnumValue::MultiPv(Self::parse(input, option.name())?)
            }
            EngineEnumOption::Minimal
            | EngineEnumOption::ClearHash
            | EngineEnumOption::Chess960
            | EngineEnumOption::SyzygyPath => {
                return Err(PresentationWorkbenchConfigurationError::NonNumeric(option));
            }
        };
        Ok(EngineOptionsSetting::new(value, capabilities)?)
    }

    pub fn deadline(
        &self,
    ) -> Result<WorkbenchJobDeadline, PresentationWorkbenchConfigurationError> {
        Ok(WorkbenchJobDeadline::new(Duration::from_millis(
            Self::parse(&self.deadline_millis, "Deadline (ms)")?,
        ))?)
    }

    pub fn parameters(
        &self,
        operation: WorkbenchEnumOperation,
        capabilities: &EngineCapabilities,
        position: Option<&EngineSearchPosition>,
    ) -> Result<WorkbenchEnumParameters, PresentationWorkbenchConfigurationError> {
        Ok(match operation {
            WorkbenchEnumOperation::Inspector => {
                WorkbenchEnumParameters::Inspector(Self::position(position)?)
            }
            WorkbenchEnumOperation::RawEvaluation => {
                WorkbenchEnumParameters::RawEvaluation(Self::position(position)?)
            }
            WorkbenchEnumOperation::Compiler => WorkbenchEnumParameters::Compiler,
            WorkbenchEnumOperation::Perft => {
                WorkbenchEnumParameters::Perft(WorkbenchPerftParameters::new(
                    Self::position(position)?,
                    WorkbenchPerftDepth::new(Self::parse(&self.perft_depth, "Perft depth")?)?,
                ))
            }
            WorkbenchEnumOperation::Bench => {
                WorkbenchEnumParameters::Bench(WorkbenchBenchParameters::new(
                    capabilities,
                    Self::optional(&self.bench_hash, "Benchmark hash (MiB)")?,
                    Self::optional(&self.bench_threads, "Benchmark threads")?,
                    Self::optional(&self.bench_depth, "Benchmark depth")?,
                )?)
            }
        })
    }

    pub const fn operation_label(operation: WorkbenchEnumOperation) -> &'static str {
        match operation {
            WorkbenchEnumOperation::Inspector => "Position inspector",
            WorkbenchEnumOperation::RawEvaluation => "Raw White NNUE evaluation",
            WorkbenchEnumOperation::Compiler => "Compiler diagnostics",
            WorkbenchEnumOperation::Perft => "Perft / divide",
            WorkbenchEnumOperation::Bench => "Built-in benchmark",
        }
    }

    pub const fn unit(option: EngineEnumOption) -> &'static str {
        match option {
            EngineEnumOption::Hash => "MiB",
            EngineEnumOption::Threads => "threads",
            EngineEnumOption::MoveOverhead => "ms",
            EngineEnumOption::MultiPv => "candidate lines",
            EngineEnumOption::Minimal
            | EngineEnumOption::ClearHash
            | EngineEnumOption::Chess960
            | EngineEnumOption::SyzygyPath => "",
        }
    }

    fn parse<T: std::str::FromStr>(
        input: &str,
        field: &'static str,
    ) -> Result<T, PresentationWorkbenchConfigurationError> {
        if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(PresentationWorkbenchConfigurationError::Number(field));
        }
        input
            .parse()
            .map_err(|_error| PresentationWorkbenchConfigurationError::Number(field))
    }

    fn optional<T: std::str::FromStr>(
        input: &str,
        field: &'static str,
    ) -> Result<Option<T>, PresentationWorkbenchConfigurationError> {
        if input.is_empty() {
            Ok(None)
        } else {
            Self::parse(input, field).map(Some)
        }
    }

    fn position(
        position: Option<&EngineSearchPosition>,
    ) -> Result<EngineSearchPosition, PresentationWorkbenchConfigurationError> {
        position
            .cloned()
            .ok_or(PresentationWorkbenchConfigurationError::PositionRequired)
    }
}
