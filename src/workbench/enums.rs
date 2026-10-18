use crate::board::position::BoardPosition;
use crate::engine::analysis::EngineAnalysisStaticEvaluation;
use crate::engine::search::position::EngineSearchPosition;
use crate::workbench::job::{WorkbenchJobCancellation, WorkbenchJobFailure};
use crate::workbench::parameters::{WorkbenchBenchParameters, WorkbenchPerftParameters};
use crate::workbench::result::{
    WorkbenchBenchResult, WorkbenchCompilerResult, WorkbenchPerftResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkbenchEnumOperation {
    Inspector,
    RawEvaluation,
    Compiler,
    Perft,
    Bench,
}

impl WorkbenchEnumOperation {
    pub const ALL: [Self; 5] = [
        Self::Inspector,
        Self::RawEvaluation,
        Self::Compiler,
        Self::Perft,
        Self::Bench,
    ];

    pub const fn work_units(self) -> u32 {
        match self {
            Self::Bench => WorkbenchBenchParameters::POSITIONS,
            Self::Inspector | Self::RawEvaluation | Self::Compiler | Self::Perft => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkbenchEnumParameters {
    Inspector(EngineSearchPosition),
    RawEvaluation(EngineSearchPosition),
    Compiler,
    Perft(WorkbenchPerftParameters),
    Bench(WorkbenchBenchParameters),
}

impl WorkbenchEnumParameters {
    pub const fn operation(&self) -> WorkbenchEnumOperation {
        match self {
            Self::Inspector(_) => WorkbenchEnumOperation::Inspector,
            Self::RawEvaluation(_) => WorkbenchEnumOperation::RawEvaluation,
            Self::Compiler => WorkbenchEnumOperation::Compiler,
            Self::Perft(_) => WorkbenchEnumOperation::Perft,
            Self::Bench(_) => WorkbenchEnumOperation::Bench,
        }
    }

    pub fn workers(&self) -> usize {
        match self {
            Self::Bench(parameters) => parameters.workers(),
            Self::Inspector(_) | Self::RawEvaluation(_) | Self::Compiler | Self::Perft(_) => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkbenchEnumJobState {
    Queued,
    Admitted,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkbenchEnumTransition {
    Admit,
    Start,
    Advance,
    Record,
    Cancel,
    Expire,
    FailBeforeStart,
    FinishReaped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkbenchEnumCancellation {
    User,
    Deadline,
}

#[derive(Debug)]
pub enum WorkbenchEnumResult {
    Inspector(BoardPosition),
    RawEvaluation(EngineAnalysisStaticEvaluation),
    Compiler(WorkbenchCompilerResult),
    Perft(WorkbenchPerftResult),
    Bench(WorkbenchBenchResult),
}

impl WorkbenchEnumResult {
    pub const fn operation(&self) -> WorkbenchEnumOperation {
        match self {
            Self::Inspector(_) => WorkbenchEnumOperation::Inspector,
            Self::RawEvaluation(_) => WorkbenchEnumOperation::RawEvaluation,
            Self::Compiler(_) => WorkbenchEnumOperation::Compiler,
            Self::Perft(_) => WorkbenchEnumOperation::Perft,
            Self::Bench(_) => WorkbenchEnumOperation::Bench,
        }
    }
}

#[derive(Debug)]
pub enum WorkbenchEnumCompletion {
    Result(WorkbenchEnumResult),
    Failed(WorkbenchJobFailure),
    Terminated,
}

#[derive(Debug)]
pub enum WorkbenchEnumOutcome {
    Succeeded(WorkbenchEnumResult),
    Failed(WorkbenchJobFailure),
    Cancelled {
        cancellation: WorkbenchJobCancellation,
        completion: Option<WorkbenchEnumCompletion>,
    },
}
