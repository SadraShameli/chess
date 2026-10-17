use std::path::Path;
use std::time::Duration;

use thiserror::Error;
use tokio::runtime::Builder;
use tokio::time::{Instant, timeout};
use uuid::Uuid;

use chess::board::position::BoardPosition;
use chess::capacity::budget::{CapacityBudget, CapacityBudgetError};
use chess::capacity::lease::CapacityLease;
use chess::clock::duration::ClockDuration;
use chess::engine::package::host::{EnginePackageHost, EnginePackageHostError};
use chess::engine::package::selector::{EnginePackageSelector, EnginePackageSelectorError};
use chess::engine::process_provider::{
    EngineNativeProcess, EngineNativeProcessProvider, EngineProcessLimits,
};
use chess::engine::resource_config::{EngineResourceConfig, EngineResourceConfigError};
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchIdentityError, EngineSearchRevision,
};
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::{EngineSearchPosition, EngineSearchPositionError};
use chess::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumCompletion, EngineSearchSessionEnumEvent,
    EngineSearchSessionError, EngineSearchSessionRequest,
};
use chess::engine::search::stop::{EngineSearchStop, EngineSearchStopError};
use chess::engine::session::{EngineSession, EngineSessionError};

#[derive(Debug, Error)]
pub(super) enum NativeEngineLifecycleError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Capacity(#[from] CapacityBudgetError),
    #[error(transparent)]
    Host(#[from] EnginePackageHostError),
    #[error(transparent)]
    Package(#[from] EnginePackageSelectorError),
    #[error(transparent)]
    Resources(#[from] EngineResourceConfigError),
    #[error(transparent)]
    Session(#[from] EngineSessionError),
    #[error(transparent)]
    Search(#[from] EngineSearchSessionError),
    #[error(transparent)]
    Position(#[from] EngineSearchPositionError),
    #[error(transparent)]
    Identity(#[from] EngineSearchIdentityError),
    #[error(transparent)]
    Stop(#[from] EngineSearchStopError),
    #[error("Native lifecycle cycle {cycle} exceeded its deadline: {source}")]
    Deadline {
        cycle: usize,
        #[source]
        source: tokio::time::error::Elapsed,
    },
    #[error("Native lifecycle acceptance violated: {0}")]
    Invariant(&'static str),
    #[error("Stop p95 {p95:?} exceeds {limit:?}")]
    StopP95 { p95: Duration, limit: Duration },
    #[error("Stop sample {sample:?} reached the {limit:?} hard-stop bound")]
    HardStop { sample: Duration, limit: Duration },
    #[error("At least {required} stop samples are required per run, observed {observed}")]
    TooFewSamples { required: usize, observed: usize },
    #[error("Lifecycle acceptance failed ({operation}); cleanup also failed ({cleanup})")]
    Cleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
}

type NativeEngineLifecycleResult<T = ()> = Result<T, NativeEngineLifecycleError>;

pub(super) struct NativeEngineLifecycleHarness;

impl NativeEngineLifecycleHarness {
    const THREADS: usize = 1;
    const HASH_MIB: u32 = 16;
    const MINIMUM_SAMPLES: usize = 200;
    const CYCLE_DEADLINE: Duration = Duration::from_secs(20);
    const STOP_P95: Duration = Duration::from_secs(1);
    const HARD_STOP: Duration = Duration::from_secs(2);

    fn request(
        revision: u64,
        lease: CapacityLease,
    ) -> NativeEngineLifecycleResult<EngineSearchSessionRequest> {
        Ok(EngineSearchSessionRequest::new(
            EngineSearchContext::new(
                EngineSearchGameId::try_from(Uuid::from_u128(1))?,
                EngineSearchRevision::new(revision),
            ),
            EngineSearchPosition::new(BoardPosition::standard(), std::iter::empty::<&str>())?,
            EngineSearchLimit::infinite(),
            lease,
        ))
    }

    async fn cycle(
        session: &mut EngineSearchSession<EngineNativeProcess>,
        budget: &CapacityBudget,
        revision: u64,
        previous: Option<chess::engine::search::identity::EngineSearchTag>,
    ) -> NativeEngineLifecycleResult<(chess::engine::search::identity::EngineSearchTag, Duration)>
    {
        let current = session
            .start(Self::request(revision, budget.try_lease(Self::THREADS)?)?)
            .await?;
        loop {
            match session.next_event(current).await? {
                EngineSearchSessionEnumEvent::Snapshot(_) => break,
                EngineSearchSessionEnumEvent::Readiness { .. }
                | EngineSearchSessionEnumEvent::Diagnostic { .. } => {}
                EngineSearchSessionEnumEvent::Finished(_) => {
                    return Err(NativeEngineLifecycleError::Invariant(
                        "infinite analysis ended without stop",
                    ));
                }
            }
        }
        let received = Instant::now();
        let stopped = session
            .stop(EngineSearchStop::received_at(current, received)?)
            .await?;
        let elapsed = received.elapsed();
        if !matches!(
            stopped.completion(),
            EngineSearchSessionEnumCompletion::Stopped
        ) || budget.available() != budget.workers()
        {
            return Err(NativeEngineLifecycleError::Invariant(
                "observed stop barrier returns the CPU allocation",
            ));
        }
        if let Some(old) = previous
            && !matches!(
                session
                    .stop(EngineSearchStop::received_at(old, Instant::now())?)
                    .await,
                Err(EngineSearchSessionError::StaleSearch)
            )
        {
            return Err(NativeEngineLifecycleError::Invariant(
                "a stale stop must be refused with zero mutation",
            ));
        }
        Ok((current, elapsed))
    }

    fn percentile95(samples: &mut [Duration]) -> Duration {
        samples.sort_unstable();
        let rank = samples.len().saturating_mul(95).div_ceil(100);
        samples[rank.saturating_sub(1)]
    }

    async fn exercise(
        session: &mut EngineSearchSession<EngineNativeProcess>,
        budget: &CapacityBudget,
        cycles: usize,
    ) -> NativeEngineLifecycleResult<(usize, Duration, Duration)> {
        let mut samples = Vec::with_capacity(cycles);
        let mut previous = None;
        for index in 0..cycles {
            let revision = u64::try_from(index)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or(NativeEngineLifecycleError::Invariant("cycle count"))?;
            let (tag, elapsed) = timeout(
                Self::CYCLE_DEADLINE,
                Self::cycle(session, budget, revision, previous),
            )
            .await
            .map_err(|source| NativeEngineLifecycleError::Deadline {
                cycle: index,
                source,
            })??;
            if elapsed >= Self::HARD_STOP {
                return Err(NativeEngineLifecycleError::HardStop {
                    sample: elapsed,
                    limit: Self::HARD_STOP,
                });
            }
            previous = Some(tag);
            samples.push(elapsed);
        }
        if samples.len() < Self::MINIMUM_SAMPLES {
            return Err(NativeEngineLifecycleError::TooFewSamples {
                required: Self::MINIMUM_SAMPLES,
                observed: samples.len(),
            });
        }
        let maximum = samples.iter().copied().max().unwrap_or(Duration::ZERO);
        let p95 = Self::percentile95(&mut samples);
        if p95 > Self::STOP_P95 {
            return Err(NativeEngineLifecycleError::StopP95 {
                p95,
                limit: Self::STOP_P95,
            });
        }
        Ok((samples.len(), p95, maximum))
    }

    pub(super) fn run(directory: &Path, cycles: usize) -> NativeEngineLifecycleResult {
        let package = EnginePackageSelector::load(directory, EnginePackageHost::detect()?)?;
        let budget = CapacityBudget::discover()?;
        let runtime = Builder::new_current_thread()
            .max_blocking_threads(budget.workers())
            .enable_all()
            .build()?;
        let (samples, p95, maximum) = runtime.block_on(async {
            let lease = budget.try_lease(Self::THREADS)?;
            let mut engine = EngineSession::start(&EngineNativeProcessProvider, package).await?;
            let configured = engine
                .configure_resources(EngineResourceConfig::new(
                    lease.workers(),
                    Self::HASH_MIB,
                    ClockDuration::ZERO,
                )?)
                .await;
            drop(lease);
            let mut session = EngineSearchSession::new(engine);
            let operation = match configured {
                Ok(()) => Self::exercise(&mut session, &budget, cycles).await,
                Err(error) => Err(NativeEngineLifecycleError::Session(error)),
            };
            let cleanup = session
                .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
                .await
                .map_err(NativeEngineLifecycleError::Search);
            match (operation, cleanup) {
                (Ok(outcome), Ok(())) => {
                    if !session.engine().is_shutdown_complete()
                        || budget.available() != budget.workers()
                    {
                        return Err(NativeEngineLifecycleError::Invariant(
                            "observed child/reader cleanup and full capacity recovery",
                        ));
                    }
                    Ok(outcome)
                }
                (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
                (Err(operation), Err(cleanup)) => Err(NativeEngineLifecycleError::Cleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
            }
        })?;
        println!(
            "Pinned Reckless lifecycle passed: {samples} stop cycles, p95 {p95:?}, maximum {maximum:?}, zero stale mutation, full CPU recovery and observed process/reader cleanup"
        );
        Ok(())
    }
}
