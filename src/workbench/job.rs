use std::time::Duration;

use thiserror::Error;
use uuid::Uuid;

use crate::capacity::lease::CapacityLease;
use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::engine::analysis::EngineAnalysisError;
use crate::engine::diagnostics::EngineDiagnostics;
use crate::engine::package::selector::EnginePackageSelected;
use crate::engine::process_provider::{EngineProcessError, EngineProcessLine};
use crate::workbench::enums::{
    WorkbenchEnumCancellation, WorkbenchEnumCompletion, WorkbenchEnumJobState,
    WorkbenchEnumOperation, WorkbenchEnumOutcome, WorkbenchEnumParameters, WorkbenchEnumResult,
    WorkbenchEnumTransition,
};
use crate::workbench::result::WorkbenchResultError;

#[derive(Debug, Error)]
pub enum WorkbenchJobError {
    #[error("A workbench job identifier must not be nil")]
    NilId,
    #[error("A workbench deadline must be positive and at most ten minutes")]
    DeadlineRange,
    #[error("Job {actual:?} does not match the owned job {expected:?}")]
    Identity {
        expected: WorkbenchJobId,
        actual: WorkbenchJobId,
    },
    #[error("Cannot apply {transition:?} while the workbench job is {state:?}")]
    Transition {
        state: WorkbenchEnumJobState,
        transition: WorkbenchEnumTransition,
    },
    #[error("The workbench job requires {expected} CPU workers but received {actual}")]
    Allocation { expected: usize, actual: usize },
    #[error("The workbench job has reached its deadline {0:?}")]
    DeadlineExpired(ClockInstant),
    #[error("The workbench job has not reached its deadline {0:?}")]
    DeadlinePending(ClockInstant),
    #[error("Progress {requested} must exceed {completed} without exceeding {total}")]
    Progress {
        completed: u32,
        requested: u32,
        total: u32,
    },
    #[error("The {actual:?} result does not match the requested {expected:?} operation")]
    ResultOperation {
        expected: WorkbenchEnumOperation,
        actual: WorkbenchEnumOperation,
    },
    #[error("The diagnostic result does not match the requested position, depth or package target")]
    ResultContext,
    #[error(transparent)]
    Time(#[from] ClockDurationError),
}

#[derive(Debug, Error)]
pub enum WorkbenchJobFailure {
    #[error(transparent)]
    Process(#[from] EngineProcessError),
    #[error(transparent)]
    Analysis(#[from] EngineAnalysisError),
    #[error(transparent)]
    Result(#[from] WorkbenchResultError),
    #[error("The engine emitted malformed or incomplete {0:?} output")]
    InvalidResponse(WorkbenchEnumOperation),
    #[error("The diagnostic child terminated without a result or cancellation request")]
    UnexpectedTermination,
    #[error("The diagnostic failed ({operation}) and child cleanup also failed ({cleanup})")]
    Cleanup {
        #[source]
        operation: Box<Self>,
        cleanup: EngineProcessError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkbenchJobId(Uuid);

impl TryFrom<Uuid> for WorkbenchJobId {
    type Error = WorkbenchJobError;

    fn try_from(value: Uuid) -> Result<Self, Self::Error> {
        if value.is_nil() {
            return Err(WorkbenchJobError::NilId);
        }
        Ok(Self(value))
    }
}

impl WorkbenchJobId {
    pub const fn value(self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkbenchJobDeadline(Duration);

impl Default for WorkbenchJobDeadline {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

impl WorkbenchJobDeadline {
    pub const DEFAULT: Duration = Duration::from_secs(60);
    pub const MAXIMUM: Duration = Duration::from_secs(10 * 60);

    pub fn new(duration: Duration) -> Result<Self, WorkbenchJobError> {
        if duration.is_zero() || duration > Self::MAXIMUM {
            return Err(WorkbenchJobError::DeadlineRange);
        }
        Ok(Self(duration))
    }

    pub const fn duration(self) -> Duration {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkbenchJobProgress {
    completed: u32,
    total: u32,
}

impl WorkbenchJobProgress {
    pub const fn completed(self) -> u32 {
        self.completed
    }

    pub const fn total(self) -> u32 {
        self.total
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkbenchJobCancellation {
    reason: WorkbenchEnumCancellation,
    requested_at: ClockInstant,
}

impl WorkbenchJobCancellation {
    pub const fn reason(self) -> WorkbenchEnumCancellation {
        self.reason
    }

    pub const fn requested_at(self) -> ClockInstant {
        self.requested_at
    }
}

enum WorkbenchJobEnumLifecycle {
    Queued,
    Admitted(CapacityLease),
    Running(CapacityLease),
    Cancelling {
        lease: CapacityLease,
        cancellation: WorkbenchJobCancellation,
    },
    Finished(WorkbenchEnumOutcome),
}

pub struct WorkbenchJob {
    id: WorkbenchJobId,
    parameters: WorkbenchEnumParameters,
    package: EnginePackageSelected,
    timeout: WorkbenchJobDeadline,
    created_at: ClockInstant,
    observed_at: ClockInstant,
    deadline: ClockInstant,
    progress: WorkbenchJobProgress,
    lifecycle: WorkbenchJobEnumLifecycle,
    diagnostics: EngineDiagnostics,
}

impl WorkbenchJob {
    pub fn new(
        id: WorkbenchJobId,
        parameters: WorkbenchEnumParameters,
        package: EnginePackageSelected,
        timeout: WorkbenchJobDeadline,
        now: ClockInstant,
    ) -> Result<Self, WorkbenchJobError> {
        let deadline = now.checked_add(ClockDuration::try_from(timeout.duration())?)?;
        let total = parameters.operation().work_units();
        Ok(Self {
            id,
            parameters,
            package,
            timeout,
            created_at: now,
            observed_at: now,
            deadline,
            progress: WorkbenchJobProgress {
                completed: 0,
                total,
            },
            lifecycle: WorkbenchJobEnumLifecycle::Queued,
            diagnostics: EngineDiagnostics::default(),
        })
    }

    pub const fn id(&self) -> WorkbenchJobId {
        self.id
    }

    pub const fn parameters(&self) -> &WorkbenchEnumParameters {
        &self.parameters
    }

    pub const fn package(&self) -> &EnginePackageSelected {
        &self.package
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

    pub const fn deadline(&self) -> ClockInstant {
        self.deadline
    }

    pub const fn progress(&self) -> WorkbenchJobProgress {
        self.progress
    }

    pub fn state(&self) -> WorkbenchEnumJobState {
        match &self.lifecycle {
            WorkbenchJobEnumLifecycle::Queued => WorkbenchEnumJobState::Queued,
            WorkbenchJobEnumLifecycle::Admitted(_) => WorkbenchEnumJobState::Admitted,
            WorkbenchJobEnumLifecycle::Running(_) => WorkbenchEnumJobState::Running,
            WorkbenchJobEnumLifecycle::Cancelling { .. } => WorkbenchEnumJobState::Cancelling,
            WorkbenchJobEnumLifecycle::Finished(outcome) => match outcome {
                WorkbenchEnumOutcome::Succeeded(_) => WorkbenchEnumJobState::Succeeded,
                WorkbenchEnumOutcome::Failed(_) => WorkbenchEnumJobState::Failed,
                WorkbenchEnumOutcome::Cancelled { cancellation, .. } => match cancellation.reason {
                    WorkbenchEnumCancellation::User => WorkbenchEnumJobState::Cancelled,
                    WorkbenchEnumCancellation::Deadline => WorkbenchEnumJobState::TimedOut,
                },
            },
        }
    }

    pub fn allocated_workers(&self) -> Option<usize> {
        match &self.lifecycle {
            WorkbenchJobEnumLifecycle::Admitted(lease)
            | WorkbenchJobEnumLifecycle::Running(lease)
            | WorkbenchJobEnumLifecycle::Cancelling { lease, .. } => Some(lease.workers()),
            WorkbenchJobEnumLifecycle::Queued | WorkbenchJobEnumLifecycle::Finished(_) => None,
        }
    }

    pub fn cancellation(&self) -> Option<WorkbenchJobCancellation> {
        match &self.lifecycle {
            WorkbenchJobEnumLifecycle::Cancelling { cancellation, .. }
            | WorkbenchJobEnumLifecycle::Finished(WorkbenchEnumOutcome::Cancelled {
                cancellation,
                ..
            }) => Some(*cancellation),
            WorkbenchJobEnumLifecycle::Queued
            | WorkbenchJobEnumLifecycle::Admitted(_)
            | WorkbenchJobEnumLifecycle::Running(_)
            | WorkbenchJobEnumLifecycle::Finished(
                WorkbenchEnumOutcome::Succeeded(_) | WorkbenchEnumOutcome::Failed(_),
            ) => None,
        }
    }

    pub fn outcome(&self) -> Option<&WorkbenchEnumOutcome> {
        match &self.lifecycle {
            WorkbenchJobEnumLifecycle::Finished(outcome) => Some(outcome),
            WorkbenchJobEnumLifecycle::Queued
            | WorkbenchJobEnumLifecycle::Admitted(_)
            | WorkbenchJobEnumLifecycle::Running(_)
            | WorkbenchJobEnumLifecycle::Cancelling { .. } => None,
        }
    }

    pub const fn diagnostics(&self) -> &EngineDiagnostics {
        &self.diagnostics
    }

    pub fn admit(
        &mut self,
        id: WorkbenchJobId,
        lease: CapacityLease,
        now: ClockInstant,
    ) -> Result<(), WorkbenchJobError> {
        self.validate(id, now, WorkbenchEnumTransition::Admit)?;
        self.before_deadline(now)?;
        let expected = self.parameters.workers();
        let actual = lease.workers();
        if actual != expected {
            return Err(WorkbenchJobError::Allocation { expected, actual });
        }
        self.lifecycle = WorkbenchJobEnumLifecycle::Admitted(lease);
        self.observed_at = now;
        Ok(())
    }

    pub fn start(
        &mut self,
        id: WorkbenchJobId,
        now: ClockInstant,
    ) -> Result<(), WorkbenchJobError> {
        self.validate(id, now, WorkbenchEnumTransition::Start)?;
        self.before_deadline(now)?;
        let previous = std::mem::replace(&mut self.lifecycle, WorkbenchJobEnumLifecycle::Queued);
        self.lifecycle = match previous {
            WorkbenchJobEnumLifecycle::Admitted(lease) => WorkbenchJobEnumLifecycle::Running(lease),
            WorkbenchJobEnumLifecycle::Queued
            | WorkbenchJobEnumLifecycle::Running(_)
            | WorkbenchJobEnumLifecycle::Cancelling { .. }
            | WorkbenchJobEnumLifecycle::Finished(_) => {
                self.lifecycle = previous;
                return Err(self.transition_error(WorkbenchEnumTransition::Start));
            }
        };
        self.observed_at = now;
        Ok(())
    }

    pub fn advance(
        &mut self,
        id: WorkbenchJobId,
        completed: u32,
        now: ClockInstant,
    ) -> Result<(), WorkbenchJobError> {
        self.validate(id, now, WorkbenchEnumTransition::Advance)?;
        self.before_deadline(now)?;
        if completed <= self.progress.completed || completed > self.progress.total {
            return Err(WorkbenchJobError::Progress {
                completed: self.progress.completed,
                requested: completed,
                total: self.progress.total,
            });
        }
        self.progress.completed = completed;
        self.observed_at = now;
        Ok(())
    }

    pub fn record(
        &mut self,
        id: WorkbenchJobId,
        line: &EngineProcessLine,
        now: ClockInstant,
    ) -> Result<(), WorkbenchJobError> {
        self.validate(id, now, WorkbenchEnumTransition::Record)?;
        self.diagnostics.record(line);
        self.observed_at = now;
        Ok(())
    }

    pub fn cancel(
        &mut self,
        id: WorkbenchJobId,
        now: ClockInstant,
    ) -> Result<(), WorkbenchJobError> {
        self.validate(id, now, WorkbenchEnumTransition::Cancel)?;
        let reason = if now.has_reached(self.deadline) {
            WorkbenchEnumCancellation::Deadline
        } else {
            WorkbenchEnumCancellation::User
        };
        self.request_cancellation(reason, now, WorkbenchEnumTransition::Cancel)
    }

    pub fn expire(
        &mut self,
        id: WorkbenchJobId,
        now: ClockInstant,
    ) -> Result<(), WorkbenchJobError> {
        self.validate(id, now, WorkbenchEnumTransition::Expire)?;
        if !now.has_reached(self.deadline) {
            return Err(WorkbenchJobError::DeadlinePending(self.deadline));
        }
        self.request_cancellation(
            WorkbenchEnumCancellation::Deadline,
            now,
            WorkbenchEnumTransition::Expire,
        )
    }

    pub fn fail_before_start(
        &mut self,
        id: WorkbenchJobId,
        failure: WorkbenchJobFailure,
        now: ClockInstant,
    ) -> Result<(), WorkbenchJobError> {
        self.validate(id, now, WorkbenchEnumTransition::FailBeforeStart)?;
        self.finish(WorkbenchEnumCompletion::Failed(failure), now);
        Ok(())
    }

    pub fn finish_reaped(
        &mut self,
        id: WorkbenchJobId,
        completion: WorkbenchEnumCompletion,
        now: ClockInstant,
    ) -> Result<(), WorkbenchJobError> {
        self.validate(id, now, WorkbenchEnumTransition::FinishReaped)?;
        if let WorkbenchEnumCompletion::Result(result) = &completion {
            self.validate_result(result)?;
        }
        self.finish(completion, now);
        Ok(())
    }

    fn finish(&mut self, completion: WorkbenchEnumCompletion, now: ClockInstant) {
        let cancellation = self.cancellation().or_else(|| {
            now.has_reached(self.deadline)
                .then_some(WorkbenchJobCancellation {
                    reason: WorkbenchEnumCancellation::Deadline,
                    requested_at: self.deadline,
                })
        });
        let outcome = match cancellation {
            Some(cancellation) => WorkbenchEnumOutcome::Cancelled {
                cancellation,
                completion: Some(completion),
            },
            None => match completion {
                WorkbenchEnumCompletion::Result(result) => {
                    self.progress.completed = self.progress.total;
                    WorkbenchEnumOutcome::Succeeded(result)
                }
                WorkbenchEnumCompletion::Failed(failure) => WorkbenchEnumOutcome::Failed(failure),
                WorkbenchEnumCompletion::Terminated => {
                    WorkbenchEnumOutcome::Failed(WorkbenchJobFailure::UnexpectedTermination)
                }
            },
        };
        self.lifecycle = WorkbenchJobEnumLifecycle::Finished(outcome);
        self.observed_at = now;
    }

    fn request_cancellation(
        &mut self,
        reason: WorkbenchEnumCancellation,
        now: ClockInstant,
        transition: WorkbenchEnumTransition,
    ) -> Result<(), WorkbenchJobError> {
        let cancellation = WorkbenchJobCancellation {
            reason,
            requested_at: now,
        };
        let previous = std::mem::replace(&mut self.lifecycle, WorkbenchJobEnumLifecycle::Queued);
        self.lifecycle = match previous {
            WorkbenchJobEnumLifecycle::Queued | WorkbenchJobEnumLifecycle::Admitted(_) => {
                WorkbenchJobEnumLifecycle::Finished(WorkbenchEnumOutcome::Cancelled {
                    cancellation,
                    completion: None,
                })
            }
            WorkbenchJobEnumLifecycle::Running(lease) => WorkbenchJobEnumLifecycle::Cancelling {
                lease,
                cancellation,
            },
            WorkbenchJobEnumLifecycle::Cancelling { .. }
            | WorkbenchJobEnumLifecycle::Finished(_) => {
                self.lifecycle = previous;
                return Err(self.transition_error(transition));
            }
        };
        self.observed_at = now;
        Ok(())
    }

    fn validate_result(&self, result: &WorkbenchEnumResult) -> Result<(), WorkbenchJobError> {
        let expected = self.parameters.operation();
        let actual = result.operation();
        if expected != actual {
            return Err(WorkbenchJobError::ResultOperation { expected, actual });
        }
        let matches = match result {
            WorkbenchEnumResult::Inspector(position) => match &self.parameters {
                WorkbenchEnumParameters::Inspector(parameters) => position == parameters.current(),
                WorkbenchEnumParameters::RawEvaluation(_)
                | WorkbenchEnumParameters::Compiler
                | WorkbenchEnumParameters::Perft(_)
                | WorkbenchEnumParameters::Bench(_) => false,
            },
            WorkbenchEnumResult::RawEvaluation(_) | WorkbenchEnumResult::Bench(_) => true,
            WorkbenchEnumResult::Compiler(result) => result.target() == self.package.target(),
            WorkbenchEnumResult::Perft(result) => match &self.parameters {
                WorkbenchEnumParameters::Perft(parameters) => {
                    result.position() == parameters.position().current()
                        && result.depth() == parameters.depth()
                }
                WorkbenchEnumParameters::Inspector(_)
                | WorkbenchEnumParameters::RawEvaluation(_)
                | WorkbenchEnumParameters::Compiler
                | WorkbenchEnumParameters::Bench(_) => false,
            },
        };
        if !matches {
            return Err(WorkbenchJobError::ResultContext);
        }
        Ok(())
    }

    fn before_deadline(&self, now: ClockInstant) -> Result<(), WorkbenchJobError> {
        if now.has_reached(self.deadline) {
            return Err(WorkbenchJobError::DeadlineExpired(self.deadline));
        }
        Ok(())
    }

    fn validate(
        &self,
        id: WorkbenchJobId,
        now: ClockInstant,
        transition: WorkbenchEnumTransition,
    ) -> Result<(), WorkbenchJobError> {
        if id != self.id {
            return Err(WorkbenchJobError::Identity {
                expected: self.id,
                actual: id,
            });
        }
        now.elapsed_since(self.observed_at)?;
        let permitted = match transition {
            WorkbenchEnumTransition::Admit => self.state() == WorkbenchEnumJobState::Queued,
            WorkbenchEnumTransition::Start => self.state() == WorkbenchEnumJobState::Admitted,
            WorkbenchEnumTransition::Advance => self.state() == WorkbenchEnumJobState::Running,
            WorkbenchEnumTransition::Record | WorkbenchEnumTransition::FinishReaped => matches!(
                self.state(),
                WorkbenchEnumJobState::Running | WorkbenchEnumJobState::Cancelling
            ),
            WorkbenchEnumTransition::Cancel | WorkbenchEnumTransition::Expire => matches!(
                self.state(),
                WorkbenchEnumJobState::Queued
                    | WorkbenchEnumJobState::Admitted
                    | WorkbenchEnumJobState::Running
            ),
            WorkbenchEnumTransition::FailBeforeStart => matches!(
                self.state(),
                WorkbenchEnumJobState::Queued | WorkbenchEnumJobState::Admitted
            ),
        };
        if !permitted {
            return Err(self.transition_error(transition));
        }
        Ok(())
    }

    fn transition_error(&self, transition: WorkbenchEnumTransition) -> WorkbenchJobError {
        WorkbenchJobError::Transition {
            state: self.state(),
            transition,
        }
    }
}
