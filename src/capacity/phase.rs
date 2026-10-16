use thiserror::Error;

use crate::capacity::budget::{CapacityBudget, CapacityBudgetError, CapacityBudgetPhaseAdmission};
use crate::capacity::lease::CapacityLease;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapacityPhaseGeneration(u64);

impl CapacityPhaseGeneration {
    pub fn value(self) -> u64 {
        self.0
    }

    fn next(self) -> Result<Self, CapacityPhaseError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(CapacityPhaseError::GenerationOverflow)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityPhaseEnumState {
    Quiescing,
    Preparing,
    Draining,
    Interrupted,
    Steady,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityPhaseEnumInterruption {
    Cancelled,
    Failed,
}

#[derive(Debug, Error)]
pub enum CapacityPhaseError {
    #[error(
        "Native presentation needs at least {required} usable CPU threads; only {available} are available"
    )]
    BelowNativeMinimum { available: usize, required: usize },
    #[error("Cannot reserve shared CPU capacity: {0}")]
    Budget(#[from] CapacityBudgetError),
    #[error("The operation is unavailable during the {0:?} capacity phase")]
    InvalidPhase(CapacityPhaseEnumState),
    #[error("Engine searches have not acknowledged stopping")]
    SearchStopPending,
    #[error("Existing CPU work has not released all preparation capacity")]
    QuiescencePending,
    #[error("Preparation generation {actual:?} is stale; the active generation is {expected:?}")]
    StaleGeneration {
        expected: Option<CapacityPhaseGeneration>,
        actual: CapacityPhaseGeneration,
    },
    #[error("Preparation generation counter is exhausted")]
    GenerationOverflow,
    #[error("Requested {requested} workload threads; the permitted range is 1 through {maximum}")]
    WorkloadRange { requested: usize, maximum: usize },
}

#[derive(Clone, Copy)]
enum CapacityPhaseEnumSearchStop {
    Pending,
    Acknowledged,
}

enum CapacityPhaseEnumRuntimeState {
    Quiescing {
        generation: CapacityPhaseGeneration,
        searches: CapacityPhaseEnumSearchStop,
    },
    Preparing {
        generation: CapacityPhaseGeneration,
        guard: CapacityLease,
    },
    Draining {
        generation: CapacityPhaseGeneration,
        guard: CapacityLease,
        reason: CapacityPhaseEnumInterruption,
    },
    Interrupted {
        reason: CapacityPhaseEnumInterruption,
    },
    Steady,
}

impl CapacityPhaseEnumRuntimeState {
    fn state(&self) -> CapacityPhaseEnumState {
        match self {
            Self::Quiescing { .. } => CapacityPhaseEnumState::Quiescing,
            Self::Preparing { .. } => CapacityPhaseEnumState::Preparing,
            Self::Draining { .. } => CapacityPhaseEnumState::Draining,
            Self::Interrupted { .. } => CapacityPhaseEnumState::Interrupted,
            Self::Steady => CapacityPhaseEnumState::Steady,
        }
    }

    fn generation(&self) -> Option<CapacityPhaseGeneration> {
        match self {
            Self::Quiescing { generation, .. }
            | Self::Preparing { generation, .. }
            | Self::Draining { generation, .. } => Some(*generation),
            Self::Interrupted { .. } | Self::Steady => None,
        }
    }

    fn interruption(&self) -> Option<CapacityPhaseEnumInterruption> {
        match self {
            Self::Draining { reason, .. } | Self::Interrupted { reason } => Some(*reason),
            Self::Quiescing { .. } | Self::Preparing { .. } | Self::Steady => None,
        }
    }
}

pub struct CapacityPhase {
    budget: CapacityBudget,
    steady_reservation: CapacityLease,
    last_generation: CapacityPhaseGeneration,
    state: CapacityPhaseEnumRuntimeState,
    admission: CapacityBudgetPhaseAdmission,
}

impl CapacityPhase {
    pub const MINIMUM_WORKERS: usize = 4;
    pub const STEADY_RESERVED_WORKERS: usize = 2;

    pub fn new(budget: CapacityBudget) -> Result<Self, CapacityPhaseError> {
        if budget.workers() < Self::MINIMUM_WORKERS {
            return Err(CapacityPhaseError::BelowNativeMinimum {
                available: budget.workers(),
                required: Self::MINIMUM_WORKERS,
            });
        }
        let steady_reservation = budget.try_lease(Self::STEADY_RESERVED_WORKERS)?;
        let admission = budget.claim_phase_admission()?;
        let generation = CapacityPhaseGeneration(1);
        Ok(Self {
            budget,
            steady_reservation,
            last_generation: generation,
            state: CapacityPhaseEnumRuntimeState::Quiescing {
                generation,
                searches: CapacityPhaseEnumSearchStop::Pending,
            },
            admission,
        })
    }

    pub fn state(&self) -> CapacityPhaseEnumState {
        self.state.state()
    }

    pub fn generation(&self) -> Option<CapacityPhaseGeneration> {
        self.state.generation()
    }

    pub fn interruption(&self) -> Option<CapacityPhaseEnumInterruption> {
        self.state.interruption()
    }

    pub fn total_workers(&self) -> usize {
        self.budget.workers()
    }

    pub fn reserved_workers(&self) -> usize {
        self.steady_reservation.workers()
    }

    pub fn workload_workers(&self) -> usize {
        self.total_workers() - self.reserved_workers()
    }

    pub fn withheld_workload_workers(&self) -> usize {
        match &self.state {
            CapacityPhaseEnumRuntimeState::Preparing { guard, .. }
            | CapacityPhaseEnumRuntimeState::Draining { guard, .. } => guard.workers(),
            CapacityPhaseEnumRuntimeState::Quiescing { .. }
            | CapacityPhaseEnumRuntimeState::Interrupted { .. }
            | CapacityPhaseEnumRuntimeState::Steady => 0,
        }
    }

    pub fn request_preparation(&mut self) -> Result<CapacityPhaseGeneration, CapacityPhaseError> {
        match &self.state {
            CapacityPhaseEnumRuntimeState::Quiescing { generation, .. }
            | CapacityPhaseEnumRuntimeState::Preparing { generation, .. } => Ok(*generation),
            CapacityPhaseEnumRuntimeState::Draining { .. } => {
                Err(CapacityPhaseError::InvalidPhase(self.state()))
            }
            CapacityPhaseEnumRuntimeState::Interrupted { .. }
            | CapacityPhaseEnumRuntimeState::Steady => {
                let generation = self.last_generation.next()?;
                self.last_generation = generation;
                self.admission.close();
                self.state = CapacityPhaseEnumRuntimeState::Quiescing {
                    generation,
                    searches: CapacityPhaseEnumSearchStop::Pending,
                };
                Ok(generation)
            }
        }
    }

    pub fn acknowledge_searches_stopped(
        &mut self,
        generation: CapacityPhaseGeneration,
    ) -> Result<(), CapacityPhaseError> {
        self.validate_generation(generation)?;
        match &mut self.state {
            CapacityPhaseEnumRuntimeState::Quiescing { searches, .. } => {
                *searches = CapacityPhaseEnumSearchStop::Acknowledged;
                Ok(())
            }
            CapacityPhaseEnumRuntimeState::Preparing { .. }
            | CapacityPhaseEnumRuntimeState::Draining { .. }
            | CapacityPhaseEnumRuntimeState::Interrupted { .. }
            | CapacityPhaseEnumRuntimeState::Steady => {
                Err(CapacityPhaseError::InvalidPhase(self.state()))
            }
        }
    }

    pub fn try_enter_preparation(
        &mut self,
        generation: CapacityPhaseGeneration,
    ) -> Result<(), CapacityPhaseError> {
        self.validate_generation(generation)?;
        match &self.state {
            CapacityPhaseEnumRuntimeState::Quiescing { searches, .. } => match searches {
                CapacityPhaseEnumSearchStop::Pending => Err(CapacityPhaseError::SearchStopPending),
                CapacityPhaseEnumSearchStop::Acknowledged => {
                    let guard =
                        self.admission
                            .try_lease(self.workload_workers())
                            .map_err(|error| match error {
                                CapacityBudgetError::Unavailable => {
                                    CapacityPhaseError::QuiescencePending
                                }
                                CapacityBudgetError::Detection(_)
                                | CapacityBudgetError::InvalidAllocation
                                | CapacityBudgetError::AdmissionClosed
                                | CapacityBudgetError::PhaseOwned
                                | CapacityBudgetError::Range(_) => {
                                    CapacityPhaseError::Budget(error)
                                }
                            })?;
                    self.state = CapacityPhaseEnumRuntimeState::Preparing { generation, guard };
                    Ok(())
                }
            },
            CapacityPhaseEnumRuntimeState::Preparing { .. }
            | CapacityPhaseEnumRuntimeState::Draining { .. }
            | CapacityPhaseEnumRuntimeState::Interrupted { .. }
            | CapacityPhaseEnumRuntimeState::Steady => {
                Err(CapacityPhaseError::InvalidPhase(self.state()))
            }
        }
    }

    pub fn complete_preparation(
        &mut self,
        generation: CapacityPhaseGeneration,
    ) -> Result<(), CapacityPhaseError> {
        self.validate_generation(generation)?;
        match &self.state {
            CapacityPhaseEnumRuntimeState::Preparing { .. } => {
                self.state = CapacityPhaseEnumRuntimeState::Steady;
                self.admission.open();
                Ok(())
            }
            CapacityPhaseEnumRuntimeState::Quiescing { .. }
            | CapacityPhaseEnumRuntimeState::Draining { .. }
            | CapacityPhaseEnumRuntimeState::Interrupted { .. }
            | CapacityPhaseEnumRuntimeState::Steady => {
                Err(CapacityPhaseError::InvalidPhase(self.state()))
            }
        }
    }

    pub fn try_lease_workload(&self, workers: usize) -> Result<CapacityLease, CapacityPhaseError> {
        if self.state() != CapacityPhaseEnumState::Steady {
            return Err(CapacityPhaseError::InvalidPhase(self.state()));
        }
        if workers == 0 || workers > self.workload_workers() {
            return Err(CapacityPhaseError::WorkloadRange {
                requested: workers,
                maximum: self.workload_workers(),
            });
        }
        Ok(self.budget.try_lease(workers)?)
    }

    pub fn interrupt_preparation(
        &mut self,
        generation: CapacityPhaseGeneration,
        reason: CapacityPhaseEnumInterruption,
    ) -> Result<(), CapacityPhaseError> {
        self.validate_generation(generation)?;
        let previous = std::mem::replace(
            &mut self.state,
            CapacityPhaseEnumRuntimeState::Interrupted { reason },
        );
        match previous {
            CapacityPhaseEnumRuntimeState::Quiescing { .. } => Ok(()),
            CapacityPhaseEnumRuntimeState::Preparing { guard, .. } => {
                self.state = CapacityPhaseEnumRuntimeState::Draining {
                    generation,
                    guard,
                    reason,
                };
                Ok(())
            }
            CapacityPhaseEnumRuntimeState::Draining { .. }
            | CapacityPhaseEnumRuntimeState::Interrupted { .. }
            | CapacityPhaseEnumRuntimeState::Steady => {
                self.state = previous;
                Err(CapacityPhaseError::InvalidPhase(self.state()))
            }
        }
    }

    pub fn acknowledge_preparation_stopped(
        &mut self,
        generation: CapacityPhaseGeneration,
    ) -> Result<(), CapacityPhaseError> {
        self.validate_generation(generation)?;
        match &self.state {
            CapacityPhaseEnumRuntimeState::Draining { reason, .. } => {
                self.state = CapacityPhaseEnumRuntimeState::Interrupted { reason: *reason };
                Ok(())
            }
            CapacityPhaseEnumRuntimeState::Quiescing { .. }
            | CapacityPhaseEnumRuntimeState::Preparing { .. }
            | CapacityPhaseEnumRuntimeState::Interrupted { .. }
            | CapacityPhaseEnumRuntimeState::Steady => {
                Err(CapacityPhaseError::InvalidPhase(self.state()))
            }
        }
    }

    fn validate_generation(
        &self,
        actual: CapacityPhaseGeneration,
    ) -> Result<(), CapacityPhaseError> {
        let expected = self.generation();
        if expected != Some(actual) {
            return Err(CapacityPhaseError::StaleGeneration { expected, actual });
        }
        Ok(())
    }
}
