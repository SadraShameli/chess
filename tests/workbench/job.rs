use std::error::Error;
use std::num::NonZeroUsize;
use std::time::Duration;

use chess::board::position::BoardPosition;
use chess::capacity::budget::{CapacityBudget, CapacityBudgetError};
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::engine::analysis::EngineAnalysisStaticEvaluation;
use chess::engine::capabilities::EngineCapabilities;
use chess::engine::diagnostics::EngineDiagnostics;
use chess::engine::enums::{EngineEnumOption, EngineOptionEnumDescriptor};
use chess::engine::process_provider::{
    EngineProcessEnumPipe, EngineProcessError, EngineProcessLimits, EngineProcessLine,
};
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::EngineSearchPosition;
use chess::workbench::enums::{
    WorkbenchEnumCancellation, WorkbenchEnumCompletion, WorkbenchEnumJobState,
    WorkbenchEnumOperation, WorkbenchEnumOutcome, WorkbenchEnumParameters, WorkbenchEnumResult,
};
use chess::workbench::job::{
    WorkbenchJob, WorkbenchJobDeadline, WorkbenchJobError, WorkbenchJobFailure, WorkbenchJobId,
};
use chess::workbench::parameters::{
    WorkbenchBenchParameters, WorkbenchPerftDepth, WorkbenchPerftParameters,
};
use chess::workbench::result::{
    WorkbenchBenchResult, WorkbenchCompilerResult, WorkbenchPerftResult, WorkbenchPerftRoot,
};
use uuid::Uuid;

use crate::engine_package_selection::EnginePackageFixture;

struct WorkbenchJobFixture {
    package: EnginePackageFixture,
    budget: CapacityBudget,
}

impl WorkbenchJobFixture {
    fn new(workers: usize) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            package: EnginePackageFixture::new()?,
            budget: CapacityBudget::new(NonZeroUsize::new(workers).ok_or("Empty budget")?)?,
        })
    }

    fn job(&self, parameters: WorkbenchEnumParameters) -> Result<WorkbenchJob, Box<dyn Error>> {
        Ok(WorkbenchJob::new(
            WorkbenchJobId::try_from(Uuid::from_u128(1))?,
            parameters,
            self.package.select()?,
            WorkbenchJobDeadline::default(),
            ClockInstant::from_nanos(0),
        )?)
    }

    fn position() -> Result<EngineSearchPosition, Box<dyn Error>> {
        Ok(EngineSearchPosition::new(
            BoardPosition::standard(),
            ["e2e4", "e7e5"],
        )?)
    }

    fn perft(depth: u32) -> Result<WorkbenchPerftParameters, Box<dyn Error>> {
        Ok(WorkbenchPerftParameters::new(
            Self::position()?,
            WorkbenchPerftDepth::new(depth)?,
        ))
    }

    fn capabilities() -> Result<EngineCapabilities, Box<dyn Error>> {
        let mut capabilities = EngineCapabilities::default();
        for option in EngineEnumOption::ALL {
            capabilities.discover(&format!(
                "option name {} type {}",
                option.name(),
                option.descriptor()
            ))?;
        }
        capabilities.validate()?;
        Ok(capabilities)
    }

    fn bench(
        hash_mib: Option<u32>,
        threads: Option<usize>,
        depth: Option<u32>,
    ) -> Result<WorkbenchBenchParameters, Box<dyn Error>> {
        Ok(WorkbenchBenchParameters::new(
            &Self::capabilities()?,
            hash_mib,
            threads,
            depth,
        )?)
    }

    fn start(&self, job: &mut WorkbenchJob) -> Result<(), Box<dyn Error>> {
        job.admit(
            job.id(),
            self.budget.try_lease(job.parameters().workers())?,
            ClockInstant::from_nanos(1),
        )?;
        job.start(job.id(), ClockInstant::from_nanos(2))?;
        Ok(())
    }
}

#[test]
fn deadlines_and_perft_depths_validate_defaults_and_boundaries() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        WorkbenchJobDeadline::default().duration(),
        Duration::from_secs(60)
    );
    assert!(WorkbenchJobDeadline::new(Duration::ZERO).is_err());
    assert!(WorkbenchJobDeadline::new(Duration::from_nanos(1)).is_ok());
    assert!(WorkbenchJobDeadline::new(Duration::from_secs(600)).is_ok());
    assert!(WorkbenchJobDeadline::new(Duration::from_secs(600) + Duration::from_nanos(1)).is_err());
    assert_eq!(WorkbenchPerftDepth::default().value(), 4);
    for depth in [1, 12] {
        assert_eq!(WorkbenchPerftDepth::new(depth)?.value(), depth);
    }
    for depth in [0, 13, u32::MAX] {
        assert!(WorkbenchPerftDepth::new(depth).is_err());
    }
    assert!(WorkbenchJobId::try_from(Uuid::nil()).is_err());
    let fixture = WorkbenchJobFixture::new(1)?;
    assert!(
        WorkbenchJob::new(
            WorkbenchJobId::try_from(Uuid::from_u128(1))?,
            WorkbenchEnumParameters::Compiler,
            fixture.package.select()?,
            WorkbenchJobDeadline::default(),
            ClockInstant::from_nanos(u64::MAX),
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn benchmark_preserves_upstream_defaults_and_canonical_bounds() -> Result<(), Box<dyn Error>> {
    let defaults = WorkbenchJobFixture::bench(None, None, None)?;
    assert_eq!(
        (defaults.hash_mib(), defaults.threads(), defaults.depth()),
        (16, 1, 12)
    );
    for option in [EngineEnumOption::Hash, EngineEnumOption::Threads] {
        let EngineOptionEnumDescriptor::Spin(range) = option.typed_descriptor() else {
            return Err("Resource descriptor changed".into());
        };
        for bound in [range.minimum(), range.maximum()] {
            let parameters = match option {
                EngineEnumOption::Hash => WorkbenchJobFixture::bench(Some(bound), None, None)?,
                EngineEnumOption::Threads => {
                    WorkbenchJobFixture::bench(None, Some(usize::try_from(bound)?), None)?
                }
                EngineEnumOption::MoveOverhead
                | EngineEnumOption::Minimal
                | EngineEnumOption::ClearHash
                | EngineEnumOption::Chess960
                | EngineEnumOption::MultiPv
                | EngineEnumOption::SyzygyPath => return Err("Unexpected resource option".into()),
            };
            assert_eq!(
                match option {
                    EngineEnumOption::Hash => parameters.hash_mib(),
                    EngineEnumOption::Threads => parameters.threads(),
                    EngineEnumOption::MoveOverhead
                    | EngineEnumOption::Minimal
                    | EngineEnumOption::ClearHash
                    | EngineEnumOption::Chess960
                    | EngineEnumOption::MultiPv
                    | EngineEnumOption::SyzygyPath =>
                        return Err("Unexpected resource option".into()),
                },
                bound
            );
        }
        for invalid in [0, range.maximum() + 1] {
            assert!(
                match option {
                    EngineEnumOption::Hash => WorkbenchJobFixture::bench(Some(invalid), None, None),
                    EngineEnumOption::Threads =>
                        WorkbenchJobFixture::bench(None, Some(usize::try_from(invalid)?), None),
                    EngineEnumOption::MoveOverhead
                    | EngineEnumOption::Minimal
                    | EngineEnumOption::ClearHash
                    | EngineEnumOption::Chess960
                    | EngineEnumOption::MultiPv
                    | EngineEnumOption::SyzygyPath =>
                        return Err("Unexpected resource option".into()),
                }
                .is_err()
            );
        }
    }
    assert!(WorkbenchJobFixture::bench(None, None, Some(0)).is_err());
    assert!(
        WorkbenchJobFixture::bench(None, None, Some(EngineSearchLimit::MAX_DEPTH + 1)).is_err()
    );
    assert!(WorkbenchJobFixture::bench(None, None, Some(EngineSearchLimit::MAX_DEPTH)).is_ok());
    assert!(WorkbenchJobFixture::bench(None, Some(usize::MAX), None).is_err());
    Ok(())
}

#[test]
fn benchmark_uses_discovered_thread_bounds_and_requires_resource_descriptors()
-> Result<(), Box<dyn Error>> {
    let mut capabilities = EngineCapabilities::default();
    assert!(WorkbenchBenchParameters::new(&capabilities, None, None, None).is_err());
    capabilities.discover(&format!(
        "option name {} type {}",
        EngineEnumOption::Hash.name(),
        EngineEnumOption::Hash.descriptor()
    ))?;
    assert!(WorkbenchBenchParameters::new(&capabilities, None, None, None).is_err());
    capabilities.discover("option name Threads type spin default 1 min 1 max 1024")?;
    assert_eq!(
        WorkbenchBenchParameters::new(&capabilities, None, Some(1024), None)?.threads(),
        1024
    );
    assert!(WorkbenchBenchParameters::new(&capabilities, None, Some(1025), None).is_err());
    Ok(())
}

#[test]
fn every_operation_requires_admission_and_preserves_context_on_success()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchJobFixture::new(2)?;
    for operation in WorkbenchEnumOperation::ALL {
        let parameters = match operation {
            WorkbenchEnumOperation::Inspector => {
                WorkbenchEnumParameters::Inspector(WorkbenchJobFixture::position()?)
            }
            WorkbenchEnumOperation::RawEvaluation => {
                WorkbenchEnumParameters::RawEvaluation(WorkbenchJobFixture::position()?)
            }
            WorkbenchEnumOperation::Compiler => WorkbenchEnumParameters::Compiler,
            WorkbenchEnumOperation::Perft => {
                WorkbenchEnumParameters::Perft(WorkbenchJobFixture::perft(1)?)
            }
            WorkbenchEnumOperation::Bench => {
                WorkbenchEnumParameters::Bench(WorkbenchJobFixture::bench(None, Some(2), None)?)
            }
        };
        let mut job = fixture.job(parameters)?;
        let package = fixture.package.select()?;
        assert_eq!(job.parameters().operation(), operation);
        assert_eq!(job.package().digest(), package.digest());
        assert_eq!(job.package().profile(), package.profile());
        assert_eq!(job.package().target(), package.target());
        assert_eq!(job.state(), WorkbenchEnumJobState::Queued);
        assert!(job.start(job.id(), ClockInstant::from_nanos(0)).is_err());
        let deadline = job.deadline();
        fixture.start(&mut job)?;
        assert_eq!(job.allocated_workers(), Some(job.parameters().workers()));
        assert_eq!(job.state(), WorkbenchEnumJobState::Running);
        let result = match job.parameters() {
            WorkbenchEnumParameters::Inspector(position) => {
                WorkbenchEnumResult::Inspector(position.current().clone())
            }
            WorkbenchEnumParameters::RawEvaluation(_position) => {
                WorkbenchEnumResult::RawEvaluation(EngineAnalysisStaticEvaluation::parse(
                    "-2147483648",
                )?)
            }
            WorkbenchEnumParameters::Compiler => {
                WorkbenchEnumResult::Compiler(WorkbenchCompilerResult::new(
                    "rustc 1.99.0".to_owned(),
                    package.target(),
                    "+neon".to_owned(),
                )?)
            }
            WorkbenchEnumParameters::Perft(parameters) => {
                let roots = parameters
                    .position()
                    .current()
                    .legal_moves()
                    .into_iter()
                    .map(|movement| WorkbenchPerftRoot::new(movement, 1))
                    .collect::<Vec<_>>();
                let nodes = u64::try_from(roots.len())?;
                WorkbenchEnumResult::Perft(WorkbenchPerftResult::new(
                    parameters,
                    roots,
                    nodes,
                    ClockDuration::from_nanos(10),
                    nodes,
                )?)
            }
            WorkbenchEnumParameters::Bench(_parameters) => WorkbenchEnumResult::Bench(
                WorkbenchBenchResult::new(45, 100, ClockDuration::from_nanos(10), 10)?,
            ),
        };
        job.finish_reaped(
            job.id(),
            WorkbenchEnumCompletion::Result(result),
            ClockInstant::from_nanos(3),
        )?;
        assert_eq!(job.state(), WorkbenchEnumJobState::Succeeded);
        assert!(
            matches!(job.outcome(), Some(WorkbenchEnumOutcome::Succeeded(result)) if result.operation() == operation)
        );
        assert_eq!(job.progress().completed(), job.progress().total());
        assert_eq!(job.deadline(), deadline);
        assert_eq!(job.id().value(), Uuid::from_u128(1));
        assert_eq!(job.allocated_workers(), None);
        assert_eq!(fixture.budget.available(), fixture.budget.workers());
    }
    Ok(())
}

#[test]
fn admission_rejects_thread_mismatch_and_never_bypasses_shared_budget() -> Result<(), Box<dyn Error>>
{
    let fixture = WorkbenchJobFixture::new(2)?;
    let mut job = fixture.job(WorkbenchEnumParameters::Bench(WorkbenchJobFixture::bench(
        None,
        Some(2),
        None,
    )?))?;
    assert!(matches!(
        job.admit(
            job.id(),
            fixture.budget.try_lease(1)?,
            ClockInstant::from_nanos(1)
        ),
        Err(WorkbenchJobError::Allocation {
            expected: 2,
            actual: 1
        })
    ));
    assert_eq!(job.state(), WorkbenchEnumJobState::Queued);
    assert_eq!(fixture.budget.available(), 2);
    let other = fixture.budget.try_lease(1)?;
    assert!(matches!(
        fixture.budget.try_lease(job.parameters().workers()),
        Err(CapacityBudgetError::Unavailable)
    ));
    assert!(job.start(job.id(), ClockInstant::from_nanos(1)).is_err());
    drop(other);
    fixture.start(&mut job)?;
    assert_eq!(fixture.budget.available(), 0);
    assert!(
        job.admit(
            job.id(),
            CapacityBudget::new(NonZeroUsize::new(2).ok_or("Empty budget")?)?.try_lease(2)?,
            ClockInstant::from_nanos(3)
        )
        .is_err()
    );
    assert_eq!(job.allocated_workers(), Some(2));
    job.finish_reaped(
        job.id(),
        WorkbenchEnumCompletion::Failed(WorkbenchJobFailure::UnexpectedTermination),
        ClockInstant::from_nanos(4),
    )?;
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}

#[test]
fn running_cancellation_retains_lease_and_cleanup_failure_until_reaped()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchJobFixture::new(1)?;
    let mut job = fixture.job(WorkbenchEnumParameters::Compiler)?;
    fixture.start(&mut job)?;
    job.cancel(job.id(), ClockInstant::from_nanos(3))?;
    assert_eq!(job.state(), WorkbenchEnumJobState::Cancelling);
    assert_eq!(
        job.cancellation().map(|value| value.reason()),
        Some(WorkbenchEnumCancellation::User)
    );
    assert_eq!(fixture.budget.available(), 0);
    assert!(job.outcome().is_none());
    assert!(job.cancel(job.id(), ClockInstant::from_nanos(4)).is_err());
    assert!(
        job.advance(job.id(), 1, ClockInstant::from_nanos(4))
            .is_err()
    );
    let line = EngineProcessLine::new(EngineProcessEnumPipe::Stderr, "cleanup context".to_owned())?;
    job.record(job.id(), &line, ClockInstant::from_nanos(4))?;
    assert_eq!(fixture.budget.available(), 0);
    job.finish_reaped(
        job.id(),
        WorkbenchEnumCompletion::Failed(WorkbenchJobFailure::Process(EngineProcessError::Io(
            std::io::Error::other("controlled reap failure"),
        ))),
        ClockInstant::from_nanos(5),
    )?;
    assert_eq!(job.state(), WorkbenchEnumJobState::Cancelled);
    assert!(
        matches!(job.outcome(), Some(WorkbenchEnumOutcome::Cancelled { completion: Some(WorkbenchEnumCompletion::Failed(error)), .. }) if error.source().is_some())
    );
    assert_eq!(
        job.diagnostics()
            .entries()
            .next_back()
            .map(|entry| entry.text()),
        Some("cleanup context")
    );
    assert_eq!(fixture.budget.available(), 1);
    assert!(
        job.finish_reaped(
            job.id(),
            WorkbenchEnumCompletion::Terminated,
            ClockInstant::from_nanos(6)
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn queued_and_admitted_cancellation_release_only_unstarted_allocations()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchJobFixture::new(1)?;
    for admit in [false, true] {
        let mut job = fixture.job(WorkbenchEnumParameters::Compiler)?;
        if admit {
            job.admit(
                job.id(),
                fixture.budget.try_lease(1)?,
                ClockInstant::from_nanos(1),
            )?;
        }
        job.cancel(job.id(), ClockInstant::from_nanos(2))?;
        assert_eq!(job.state(), WorkbenchEnumJobState::Cancelled);
        assert_eq!(fixture.budget.available(), 1);
        assert!(matches!(
            job.outcome(),
            Some(WorkbenchEnumOutcome::Cancelled {
                completion: None,
                ..
            })
        ));
        assert!(job.start(job.id(), ClockInstant::from_nanos(3)).is_err());
        assert!(job.cancel(job.id(), ClockInstant::from_nanos(3)).is_err());
    }
    Ok(())
}

#[test]
fn deadline_expiration_keeps_live_allocation_and_rejects_late_success() -> Result<(), Box<dyn Error>>
{
    let fixture = WorkbenchJobFixture::new(1)?;
    for started in [false, true] {
        let mut job = fixture.job(WorkbenchEnumParameters::Compiler)?;
        if started {
            fixture.start(&mut job)?;
        }
        assert!(job.expire(job.id(), ClockInstant::from_nanos(3)).is_err());
        let deadline = job.deadline();
        job.expire(job.id(), deadline)?;
        assert_eq!(
            job.cancellation().map(|value| value.reason()),
            Some(WorkbenchEnumCancellation::Deadline)
        );
        if started {
            assert_eq!(job.state(), WorkbenchEnumJobState::Cancelling);
            assert_eq!(fixture.budget.available(), 0);
            job.finish_reaped(job.id(), WorkbenchEnumCompletion::Terminated, deadline)?;
        }
        assert_eq!(job.state(), WorkbenchEnumJobState::TimedOut);
        assert_eq!(fixture.budget.available(), 1);
        assert!(job.expire(job.id(), deadline).is_err());
    }
    let mut late = fixture.job(WorkbenchEnumParameters::RawEvaluation(
        WorkbenchJobFixture::position()?,
    ))?;
    fixture.start(&mut late)?;
    late.finish_reaped(
        late.id(),
        WorkbenchEnumCompletion::Result(WorkbenchEnumResult::RawEvaluation(
            EngineAnalysisStaticEvaluation::parse("-7")?,
        )),
        late.deadline(),
    )?;
    assert_eq!(late.state(), WorkbenchEnumJobState::TimedOut);
    assert!(
        matches!(late.outcome(), Some(WorkbenchEnumOutcome::Cancelled { completion: Some(WorkbenchEnumCompletion::Result(WorkbenchEnumResult::RawEvaluation(value))), .. }) if value.white_value() == -7)
    );
    Ok(())
}

#[test]
fn exact_deadline_blocks_admission_start_and_progress_without_releasing_live_capacity()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchJobFixture::new(1)?;
    let mut queued = fixture.job(WorkbenchEnumParameters::Compiler)?;
    assert!(matches!(
        queued.admit(queued.id(), fixture.budget.try_lease(1)?, queued.deadline()),
        Err(WorkbenchJobError::DeadlineExpired(_))
    ));
    assert_eq!(queued.state(), WorkbenchEnumJobState::Queued);
    assert_eq!(fixture.budget.available(), 1);
    let mut admitted = fixture.job(WorkbenchEnumParameters::Compiler)?;
    admitted.admit(
        admitted.id(),
        fixture.budget.try_lease(1)?,
        ClockInstant::from_nanos(1),
    )?;
    assert!(matches!(
        admitted.start(admitted.id(), admitted.deadline()),
        Err(WorkbenchJobError::DeadlineExpired(_))
    ));
    admitted.expire(admitted.id(), admitted.deadline())?;
    assert_eq!(fixture.budget.available(), 1);
    let mut running = fixture.job(WorkbenchEnumParameters::Compiler)?;
    fixture.start(&mut running)?;
    assert!(matches!(
        running.advance(running.id(), 1, running.deadline()),
        Err(WorkbenchJobError::DeadlineExpired(_))
    ));
    assert_eq!(fixture.budget.available(), 0);
    assert!(
        running
            .fail_before_start(
                running.id(),
                WorkbenchJobFailure::UnexpectedTermination,
                ClockInstant::from_nanos(3)
            )
            .is_err()
    );
    running.expire(running.id(), running.deadline())?;
    running.finish_reaped(
        running.id(),
        WorkbenchEnumCompletion::Terminated,
        running.deadline(),
    )?;
    assert_eq!(fixture.budget.available(), 1);
    Ok(())
}

#[test]
fn stale_identity_time_progress_and_result_transitions_fail_without_mutation()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchJobFixture::new(1)?;
    let mut job = fixture.job(WorkbenchEnumParameters::Bench(WorkbenchJobFixture::bench(
        None, None, None,
    )?))?;
    let wrong_id = WorkbenchJobId::try_from(Uuid::from_u128(2))?;
    assert!(
        job.admit(
            wrong_id,
            fixture.budget.try_lease(1)?,
            ClockInstant::from_nanos(1)
        )
        .is_err()
    );
    assert_eq!(fixture.budget.available(), 1);
    fixture.start(&mut job)?;
    assert!(job.start(job.id(), ClockInstant::from_nanos(3)).is_err());
    assert!(
        job.advance(wrong_id, 1, ClockInstant::from_nanos(3))
            .is_err()
    );
    assert!(
        job.advance(job.id(), 1, ClockInstant::from_nanos(1))
            .is_err()
    );
    job.advance(job.id(), 1, ClockInstant::from_nanos(3))?;
    assert!(
        job.advance(job.id(), 1, ClockInstant::from_nanos(4))
            .is_err()
    );
    assert!(
        job.advance(job.id(), 0, ClockInstant::from_nanos(4))
            .is_err()
    );
    assert!(
        job.advance(job.id(), 46, ClockInstant::from_nanos(4))
            .is_err()
    );
    assert_eq!(job.progress().completed(), 1);
    assert!(
        job.finish_reaped(
            job.id(),
            WorkbenchEnumCompletion::Result(WorkbenchEnumResult::RawEvaluation(
                EngineAnalysisStaticEvaluation::parse("0")?
            )),
            ClockInstant::from_nanos(4)
        )
        .is_err()
    );
    assert_eq!(job.state(), WorkbenchEnumJobState::Running);
    assert_eq!(fixture.budget.available(), 0);
    job.finish_reaped(
        job.id(),
        WorkbenchEnumCompletion::Terminated,
        ClockInstant::from_nanos(4),
    )?;
    assert_eq!(job.state(), WorkbenchEnumJobState::Failed);
    assert!(matches!(
        job.outcome(),
        Some(WorkbenchEnumOutcome::Failed(
            WorkbenchJobFailure::UnexpectedTermination
        ))
    ));
    assert!(
        job.advance(job.id(), 2, ClockInstant::from_nanos(5))
            .is_err()
    );
    assert_eq!(fixture.budget.available(), 1);
    Ok(())
}

#[test]
fn before_start_failure_preserves_cause_and_recovers_admitted_capacity()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchJobFixture::new(1)?;
    let mut job = fixture.job(WorkbenchEnumParameters::Compiler)?;
    job.admit(
        job.id(),
        fixture.budget.try_lease(1)?,
        ClockInstant::from_nanos(1),
    )?;
    job.fail_before_start(
        job.id(),
        WorkbenchJobFailure::Process(EngineProcessError::Io(std::io::Error::other(
            "controlled spawn failure",
        ))),
        ClockInstant::from_nanos(2),
    )?;
    assert_eq!(job.state(), WorkbenchEnumJobState::Failed);
    assert!(
        matches!(job.outcome(), Some(WorkbenchEnumOutcome::Failed(error)) if error.source().is_some())
    );
    assert_eq!(fixture.budget.available(), 1);
    assert!(
        job.fail_before_start(
            job.id(),
            WorkbenchJobFailure::UnexpectedTermination,
            ClockInstant::from_nanos(3)
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn inspector_and_compiler_context_mismatch_retain_job_until_typed_cleanup()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchJobFixture::new(1)?;
    let mut inspector = fixture.job(WorkbenchEnumParameters::Inspector(
        WorkbenchJobFixture::position()?,
    ))?;
    fixture.start(&mut inspector)?;
    assert!(matches!(
        inspector.finish_reaped(
            inspector.id(),
            WorkbenchEnumCompletion::Result(WorkbenchEnumResult::Inspector(
                BoardPosition::standard()
            )),
            ClockInstant::from_nanos(3)
        ),
        Err(WorkbenchJobError::ResultContext)
    ));
    let operation = WorkbenchJobFailure::InvalidResponse(WorkbenchEnumOperation::Inspector);
    inspector.finish_reaped(
        inspector.id(),
        WorkbenchEnumCompletion::Failed(WorkbenchJobFailure::Cleanup {
            operation: Box::new(operation),
            cleanup: EngineProcessError::Io(std::io::Error::other("controlled pipe cleanup")),
        }),
        ClockInstant::from_nanos(4),
    )?;
    assert!(
        matches!(inspector.outcome(), Some(WorkbenchEnumOutcome::Failed(WorkbenchJobFailure::Cleanup { operation, cleanup: EngineProcessError::Io(_error) })) if matches!(**operation, WorkbenchJobFailure::InvalidResponse(WorkbenchEnumOperation::Inspector)))
    );
    let mut compiler = fixture.job(WorkbenchEnumParameters::Compiler)?;
    fixture.start(&mut compiler)?;
    let result = WorkbenchCompilerResult::new(
        "rustc 1.99.0".to_owned(),
        chess::engine::package::manifest::EnginePackageEnumTarget::WindowsX64,
        String::new(),
    )?;
    assert!(matches!(
        compiler.finish_reaped(
            compiler.id(),
            WorkbenchEnumCompletion::Result(WorkbenchEnumResult::Compiler(result)),
            ClockInstant::from_nanos(3)
        ),
        Err(WorkbenchJobError::ResultContext)
    ));
    assert_eq!(fixture.budget.available(), 0);
    compiler.cancel(compiler.id(), ClockInstant::from_nanos(4))?;
    compiler.finish_reaped(
        compiler.id(),
        WorkbenchEnumCompletion::Terminated,
        ClockInstant::from_nanos(5),
    )?;
    assert_eq!(fixture.budget.available(), 1);
    Ok(())
}

#[test]
fn results_reject_invalid_divides_totals_context_and_unbounded_build_text()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchJobFixture::new(1)?;
    let parameters = WorkbenchJobFixture::perft(1)?;
    let moves = parameters.position().current().legal_moves();
    let roots = || {
        moves
            .iter()
            .map(|movement| WorkbenchPerftRoot::new(*movement, 1))
            .collect::<Vec<_>>()
    };
    let count = u64::try_from(moves.len())?;
    assert!(WorkbenchPerftResult::new(&parameters, Vec::new(), 0, ClockDuration::ZERO, 0).is_err());
    assert!(
        WorkbenchPerftResult::new(&parameters, roots(), count + 1, ClockDuration::ZERO, 0).is_err()
    );
    let first = *moves.first().ok_or("No legal moves")?;
    assert!(
        WorkbenchPerftResult::new(
            &parameters,
            vec![WorkbenchPerftRoot::new(first, 1); moves.len()],
            count,
            ClockDuration::ZERO,
            0
        )
        .is_err()
    );
    let overflowing = moves
        .iter()
        .map(|movement| WorkbenchPerftRoot::new(*movement, u64::MAX))
        .collect();
    assert!(
        WorkbenchPerftResult::new(&parameters, overflowing, 0, ClockDuration::ZERO, 0).is_err()
    );
    let package = fixture.package.select()?;
    assert!(WorkbenchCompilerResult::new(String::new(), package.target(), String::new()).is_err());
    assert!(
        WorkbenchCompilerResult::new(
            "x".repeat(EngineProcessLimits::LINE_BYTES + 1),
            package.target(),
            String::new()
        )
        .is_err()
    );
    assert!(
        WorkbenchCompilerResult::new(
            "compiler\ninjected".to_owned(),
            package.target(),
            String::new()
        )
        .is_err()
    );
    for count in [0, 44, 46, u32::MAX] {
        assert!(WorkbenchBenchResult::new(count, 0, ClockDuration::ZERO, 0).is_err());
    }
    let mut job = fixture.job(WorkbenchEnumParameters::Perft(WorkbenchJobFixture::perft(
        2,
    )?))?;
    fixture.start(&mut job)?;
    let result = WorkbenchPerftResult::new(&parameters, roots(), count, ClockDuration::ZERO, 0)?;
    assert!(
        job.finish_reaped(
            job.id(),
            WorkbenchEnumCompletion::Result(WorkbenchEnumResult::Perft(result)),
            ClockInstant::from_nanos(3)
        )
        .is_err()
    );
    assert_eq!(fixture.budget.available(), 0);
    job.finish_reaped(
        job.id(),
        WorkbenchEnumCompletion::Failed(WorkbenchJobFailure::UnexpectedTermination),
        ClockInstant::from_nanos(4),
    )?;
    Ok(())
}

#[test]
fn diagnostics_reuse_the_bounded_tail_through_cancellation() -> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchJobFixture::new(1)?;
    let mut job = fixture.job(WorkbenchEnumParameters::Compiler)?;
    fixture.start(&mut job)?;
    let line = EngineProcessLine::new(
        EngineProcessEnumPipe::Stdout,
        "x".repeat(EngineProcessLimits::LINE_BYTES),
    )?;
    for _index in 0..EngineDiagnostics::MAX_ENTRIES {
        job.record(job.id(), &line, ClockInstant::from_nanos(3))?;
    }
    assert!(job.diagnostics().owned_bytes() <= EngineDiagnostics::MAX_OWNED_BYTES);
    assert!(job.diagnostics().evicted_lines() > 0);
    job.cancel(job.id(), ClockInstant::from_nanos(4))?;
    job.finish_reaped(
        job.id(),
        WorkbenchEnumCompletion::Terminated,
        ClockInstant::from_nanos(5),
    )?;
    assert!(job.diagnostics().entries().len() > 0);
    assert!(
        job.record(job.id(), &line, ClockInstant::from_nanos(6))
            .is_err()
    );
    Ok(())
}
