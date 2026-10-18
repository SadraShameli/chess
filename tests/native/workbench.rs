use std::error::Error;
use std::path::Path;

use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::ClockInstant;
use chess::engine::package::host::EnginePackageHost;
use chess::engine::package::selector::{EnginePackageSelected, EnginePackageSelector};
use chess::engine::process_provider::{
    EngineNativeProcess, EngineNativeProcessProvider, EngineProcessLimits,
};
use chess::engine::search::position::EngineSearchPosition;
use chess::workbench::enums::{
    WorkbenchEnumJobState, WorkbenchEnumOperation, WorkbenchEnumOutcome, WorkbenchEnumParameters,
    WorkbenchEnumResult,
};
use chess::workbench::executor::WorkbenchExecutor;
use chess::workbench::job::{WorkbenchJob, WorkbenchJobDeadline, WorkbenchJobId};
use chess::workbench::parameters::{
    WorkbenchBenchParameters, WorkbenchPerftDepth, WorkbenchPerftParameters,
};
use tokio::runtime::Builder;
use tokio::time::Instant;
use uuid::Uuid;

pub(super) struct NativeWorkbenchHarness;

impl NativeWorkbenchHarness {
    fn job(
        budget: &CapacityBudget,
        package: &EnginePackageSelected,
        parameters: WorkbenchEnumParameters,
    ) -> Result<WorkbenchExecutor<EngineNativeProcess>, Box<dyn Error>> {
        let mut job = WorkbenchJob::new(
            WorkbenchJobId::try_from(Uuid::new_v4())?,
            parameters,
            package.clone(),
            WorkbenchJobDeadline::default(),
            ClockInstant::from_nanos(0),
        )?;
        job.admit(
            job.id(),
            budget.try_lease(job.parameters().workers())?,
            ClockInstant::from_nanos(0),
        )?;
        Ok(WorkbenchExecutor::new(job, ClockInstant::from_nanos(0))?)
    }

    fn position() -> Result<EngineSearchPosition, Box<dyn Error>> {
        Ok(EngineSearchPosition::new(
            BoardPosition::standard(),
            ["e2e4", "e7e5"],
        )?)
    }

    pub(super) fn run(directory: &Path) -> Result<(), Box<dyn Error>> {
        let package = EnginePackageSelector::load(directory, EnginePackageHost::detect()?)?;
        let budget = CapacityBudget::discover()?;
        let runtime = Builder::new_current_thread()
            .max_blocking_threads(budget.workers())
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let mut startup = chess::engine::session::EngineSessionStartup::new(
                &EngineNativeProcessProvider,
                package.clone(),
            )?;
            let initialization = startup.initialize().await;
            let mut live = match initialization {
                Ok(session) => session,
                Err(error) => {
                    startup
                        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
                        .await?;
                    return Err(Box::<dyn Error>::from(error));
                }
            };
            let operation =
                async {
                    let generation = live.generation();
                    let defaults = WorkbenchBenchParameters::new(
                        live.identity().capabilities(),
                        None,
                        None,
                        None,
                    )?;
                    if (defaults.hash_mib(), defaults.threads(), defaults.depth()) != (16, 1, 12) {
                        return Err("upstream benchmark defaults".into());
                    }
                    for operation in WorkbenchEnumOperation::ALL {
                        let parameters = match operation {
                            WorkbenchEnumOperation::Inspector => {
                                WorkbenchEnumParameters::Inspector(Self::position()?)
                            }
                            WorkbenchEnumOperation::RawEvaluation => {
                                WorkbenchEnumParameters::RawEvaluation(Self::position()?)
                            }
                            WorkbenchEnumOperation::Compiler => WorkbenchEnumParameters::Compiler,
                            WorkbenchEnumOperation::Perft => {
                                WorkbenchEnumParameters::Perft(WorkbenchPerftParameters::new(
                                    EngineSearchPosition::new(
                                        BoardPosition::standard(),
                                        std::iter::empty::<&str>(),
                                    )?,
                                    WorkbenchPerftDepth::new(2)?,
                                ))
                            }
                            WorkbenchEnumOperation::Bench => {
                                WorkbenchEnumParameters::Bench(WorkbenchBenchParameters::new(
                                    live.identity().capabilities(),
                                    None,
                                    None,
                                    Some(1),
                                )?)
                            }
                        };
                        let mut executor = Self::job(&budget, &package, parameters)?;
                        executor.start(&EngineNativeProcessProvider).await?;
                        executor.run().await?;
                        if !executor.is_shutdown_complete()
                            || executor.job().state() != WorkbenchEnumJobState::Succeeded
                        {
                            return Err(
                                format!("{operation:?}: {:?}", executor.job().outcome()).into()
                            );
                        }
                        if let Some(WorkbenchEnumOutcome::Succeeded(WorkbenchEnumResult::Perft(
                            result,
                        ))) = executor.job().outcome()
                            && result.nodes() != 400
                        {
                            return Err("standard perft depth two".into());
                        }
                        if budget.available() != budget.workers() {
                            return Err("diagnostic lease recovery".into());
                        }
                    }
                    let position = BoardPosition::from_fen(
                        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
                        BoardEnumVariant::Standard,
                    )?;
                    let parameters = WorkbenchEnumParameters::Perft(WorkbenchPerftParameters::new(
                        EngineSearchPosition::new(position, std::iter::empty::<&str>())?,
                        WorkbenchPerftDepth::new(12)?,
                    ));
                    let mut executor = Self::job(&budget, &package, parameters)?;
                    executor.start(&EngineNativeProcessProvider).await?;
                    let run =
                        tokio::time::timeout(std::time::Duration::from_millis(10), executor.run())
                            .await;
                    if let Ok(result) = run {
                        result?;
                        return Err("cancellable perft unexpectedly completed".into());
                    }
                    executor.cancel(executor.job().id()).await?;
                    if !executor.is_shutdown_complete()
                        || executor.job().state() != WorkbenchEnumJobState::Cancelled
                        || budget.available() != budget.workers()
                    {
                        return Err("native kill/reap and exact lease recovery".into());
                    }
                    if live.generation() != generation || live.is_shutdown_complete() {
                        return Err("live engine isolation".into());
                    }
                    live.readiness().await?;
                    Ok::<(), Box<dyn Error>>(())
                }
                .await;
            let cleanup = live
                .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
                .await;
            match (operation, cleanup) {
                (Ok(()), Ok(())) => Ok(()),
                (Err(error), Ok(())) => Err(error),
                (Ok(()), Err(error)) => Err(Box::<dyn Error>::from(error)),
                (Err(operation), Err(cleanup)) => Err(format!(
                    "native operation failed ({operation}); cleanup failed ({cleanup})"
                )
                .into()),
            }
        })?;
        println!(
            "Pinned workbench acceptance passed: five isolated operations, 45 benchmark positions at depth one, default 16/1/12, perft 400, cancellable depth twelve, live readiness and exact lease recovery"
        );
        Ok(())
    }
}
