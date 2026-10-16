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
use chess::engine::options::{EngineOptionsEnumValue, EngineOptionsError, EngineOptionsSetting};
use chess::engine::package::host::{EnginePackageHost, EnginePackageHostError};
use chess::engine::package::selector::{EnginePackageSelector, EnginePackageSelectorError};
use chess::engine::process_provider::{
    EngineNativeProcess, EngineNativeProcessProvider, EngineProcessLimits,
};
use chess::engine::resource_config::{EngineResourceConfig, EngineResourceConfigError};
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchIdentityError, EngineSearchRevision,
    EngineSearchTag,
};
use chess::engine::search::limit::{EngineSearchLimit, EngineSearchLimitError};
use chess::engine::search::position::{EngineSearchPosition, EngineSearchPositionError};
use chess::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumCompletion, EngineSearchSessionEnumEvent,
    EngineSearchSessionError, EngineSearchSessionRequest, EngineSearchSessionTerminal,
};
use chess::engine::search::stop::{EngineSearchStop, EngineSearchStopError};
use chess::engine::session::{EngineSession, EngineSessionError};

#[derive(Debug, Error)]
pub(super) enum NativeEngineAnalysisSessionError {
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
    Options(#[from] EngineOptionsError),
    #[error(transparent)]
    Session(#[from] EngineSessionError),
    #[error(transparent)]
    Search(#[from] EngineSearchSessionError),
    #[error(transparent)]
    Position(#[from] EngineSearchPositionError),
    #[error(transparent)]
    Identity(#[from] EngineSearchIdentityError),
    #[error(transparent)]
    Limit(#[from] EngineSearchLimitError),
    #[error(transparent)]
    Stop(#[from] EngineSearchStopError),
    #[error("Native analysis acceptance exceeded its deadline: {0}")]
    Deadline(#[source] tokio::time::error::Elapsed),
    #[error("Native analysis acceptance violated: {0}")]
    Invariant(&'static str),
    #[error("Analysis acceptance failed ({operation}); cleanup also failed ({cleanup})")]
    Cleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
}

type NativeEngineAnalysisSessionResult<T = ()> = Result<T, NativeEngineAnalysisSessionError>;

pub(super) struct NativeEngineAnalysisSessionHarness;

impl NativeEngineAnalysisSessionHarness {
    const THREADS: usize = 1;
    const HASH_MIB: u32 = 16;
    const CANDIDATES: u32 = 2;
    const DEPTH: u32 = 8;
    const DEADLINE: Duration = Duration::from_secs(20);

    fn finish(
        operation: NativeEngineAnalysisSessionResult,
        cleanup: NativeEngineAnalysisSessionResult,
    ) -> NativeEngineAnalysisSessionResult {
        match (operation, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(operation), Err(cleanup)) => Err(NativeEngineAnalysisSessionError::Cleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            }),
        }
    }

    async fn configure(
        engine: &mut EngineSession<EngineNativeProcess>,
        lease: &CapacityLease,
    ) -> NativeEngineAnalysisSessionResult {
        engine
            .configure_resources(EngineResourceConfig::new(
                lease.workers(),
                Self::HASH_MIB,
                ClockDuration::ZERO,
            )?)
            .await?;
        let setting = EngineOptionsSetting::new(
            EngineOptionsEnumValue::MultiPv(Self::CANDIDATES),
            engine.identity().capabilities(),
        )?;
        engine.configure_option(setting).await?;
        Ok(())
    }

    fn request(
        revision: u64,
        limit: EngineSearchLimit,
        lease: CapacityLease,
    ) -> NativeEngineAnalysisSessionResult<EngineSearchSessionRequest> {
        Ok(EngineSearchSessionRequest::new(
            EngineSearchContext::new(
                EngineSearchGameId::try_from(Uuid::from_u128(1))?,
                EngineSearchRevision::new(revision),
            ),
            EngineSearchPosition::new(BoardPosition::standard(), std::iter::empty::<&str>())?,
            limit,
            lease,
        ))
    }

    async fn terminal(
        session: &mut EngineSearchSession<EngineNativeProcess>,
        tag: EngineSearchTag,
    ) -> NativeEngineAnalysisSessionResult<EngineSearchSessionTerminal> {
        loop {
            match session.next_event(tag).await? {
                EngineSearchSessionEnumEvent::Finished(terminal) => return Ok(terminal),
                EngineSearchSessionEnumEvent::Snapshot(_)
                | EngineSearchSessionEnumEvent::Diagnostic { .. }
                | EngineSearchSessionEnumEvent::Readiness { .. } => {}
            }
        }
    }

    fn verify(
        terminal: &EngineSearchSessionTerminal,
        tag: EngineSearchTag,
    ) -> NativeEngineAnalysisSessionResult {
        match terminal.completion() {
            EngineSearchSessionEnumCompletion::Failed(error) => {
                return Err(NativeEngineAnalysisSessionError::Search((**error).clone()));
            }
            EngineSearchSessionEnumCompletion::Cancelled => {
                return Err(NativeEngineAnalysisSessionError::Invariant(
                    "unexpected cancellation",
                ));
            }
            EngineSearchSessionEnumCompletion::BestMove(_)
            | EngineSearchSessionEnumCompletion::Stopped => {}
        }
        if terminal.snapshot().metadata().tag() != tag
            || terminal.snapshot().lines().len()
                != usize::try_from(Self::CANDIDATES).map_err(|_conversion| {
                    NativeEngineAnalysisSessionError::Invariant("candidate count representation")
                })?
        {
            return Err(NativeEngineAnalysisSessionError::Invariant(
                "tagged final candidate set",
            ));
        }
        for (index, line) in terminal.snapshot().lines().iter().enumerate() {
            if line
                .rank()
                .and_then(|rank| usize::try_from(rank.get()).ok())
                != index.checked_add(1)
                || line.score().is_none()
                || line.variation().is_none()
            {
                return Err(NativeEngineAnalysisSessionError::Invariant(
                    "complete candidates in rank order",
                ));
            }
        }
        Ok(())
    }

    async fn exercise(
        session: &mut EngineSearchSession<EngineNativeProcess>,
        budget: &CapacityBudget,
        first: EngineSearchSessionRequest,
    ) -> NativeEngineAnalysisSessionResult {
        let old = session.start(first).await?;
        let terminal = Self::terminal(session, old).await?;
        Self::verify(&terminal, old)?;
        if !matches!(
            terminal.completion(),
            EngineSearchSessionEnumCompletion::BestMove(_)
        ) || budget.available() != budget.workers()
            || session.engine().is_shutdown_complete()
        {
            return Err(NativeEngineAnalysisSessionError::Invariant(
                "natural completion returns capacity while the ready process stays owned",
            ));
        }
        let current = session
            .start(Self::request(
                2,
                EngineSearchLimit::infinite(),
                budget.try_lease(Self::THREADS)?,
            )?)
            .await?;
        if current.id() == old.id() || current.generation() != old.generation() {
            return Err(NativeEngineAnalysisSessionError::Invariant(
                "replacement correlation without process restart",
            ));
        }
        if !matches!(
            session
                .stop(EngineSearchStop::received_at(old, Instant::now())?)
                .await,
            Err(EngineSearchSessionError::StaleSearch)
        ) {
            return Err(NativeEngineAnalysisSessionError::Invariant(
                "stale stop refused",
            ));
        }
        loop {
            match session.next_event(current).await? {
                EngineSearchSessionEnumEvent::Snapshot(snapshot)
                    if snapshot.lines().len()
                        == usize::try_from(Self::CANDIDATES).map_err(|_conversion| {
                            NativeEngineAnalysisSessionError::Invariant(
                                "candidate count representation",
                            )
                        })? =>
                {
                    break;
                }
                EngineSearchSessionEnumEvent::Snapshot(_)
                | EngineSearchSessionEnumEvent::Readiness { .. }
                | EngineSearchSessionEnumEvent::Diagnostic { .. } => {}
                EngineSearchSessionEnumEvent::Finished(terminal) => {
                    Self::verify(&terminal, current)?;
                    return Err(NativeEngineAnalysisSessionError::Invariant(
                        "infinite analysis ended without stop",
                    ));
                }
            }
        }
        let stopped = session
            .stop(EngineSearchStop::received_at(current, Instant::now())?)
            .await?;
        Self::verify(&stopped, current)?;
        if !matches!(
            stopped.completion(),
            EngineSearchSessionEnumCompletion::Stopped
        ) || budget.available() != budget.workers()
        {
            return Err(NativeEngineAnalysisSessionError::Invariant(
                "observed stop barrier returns the CPU allocation",
            ));
        }
        Ok(())
    }

    pub(super) fn run(directory: &Path) -> NativeEngineAnalysisSessionResult {
        let package = EnginePackageSelector::load(directory, EnginePackageHost::detect()?)?;
        let budget = CapacityBudget::discover()?;
        let runtime = Builder::new_current_thread()
            .max_blocking_threads(budget.workers())
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let lease = budget.try_lease(Self::THREADS)?;
            let mut engine = EngineSession::start(&EngineNativeProcessProvider, package).await?;
            let configured = Self::configure(&mut engine, &lease).await;
            if configured.is_err() {
                let cleanup = engine
                    .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
                    .await
                    .map_err(NativeEngineAnalysisSessionError::Session);
                return Self::finish(configured, cleanup);
            }
            let request = Self::request(1, EngineSearchLimit::depth(Self::DEPTH)?, lease)?;
            let mut session = EngineSearchSession::new(engine);
            let operation = timeout(
                Self::DEADLINE,
                Self::exercise(&mut session, &budget, request),
            )
            .await
            .map_err(NativeEngineAnalysisSessionError::Deadline)
            .and_then(|result| result);
            let cleanup = session
                .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
                .await
                .map_err(NativeEngineAnalysisSessionError::Search);
            let result = Self::finish(operation, cleanup);
            if !session.engine().is_shutdown_complete() || budget.available() != budget.workers() {
                return Err(NativeEngineAnalysisSessionError::Invariant(
                    "observed child/reader cleanup and full capacity recovery",
                ));
            }
            let _full = budget.try_lease(budget.workers())?;
            result
        })?;
        println!(
            "Pinned Reckless analysis session passed: two ranked candidates, natural completion, same-process replacement, stale stop refusal, infinite-analysis stop barrier, full CPU lease recovery and observed process/reader cleanup"
        );
        Ok(())
    }
}
