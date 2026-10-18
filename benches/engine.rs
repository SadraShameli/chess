use std::error::Error;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Duration;

use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::ClockDuration;
use chess::engine::package::host::EnginePackageHost;
use chess::engine::package::selector::EnginePackageSelector;
use chess::engine::process_provider::EngineNativeProcessProvider;
use chess::engine::resource_config::EngineResourceConfig;
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
};
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumEvent, EngineSearchSessionRequest,
};
use chess::engine::search::stop::EngineSearchStop;
use chess::engine::session::EngineSession;
use tokio::time::Instant;
use uuid::Uuid;

type EngineBenchResult<T = ()> = Result<T, Box<dyn Error>>;

struct EngineBenchFixture;

impl EngineBenchFixture {
    const RUNS: usize = 5;
    const SAMPLES: usize = 200;
    const DIRECTORY_VARIABLE: &'static str = "CHESS_BENCH_ENGINE_DIRECTORY";
    const SNAPSHOT_EVENTS: usize = 8;
}

struct EngineBenchRunner;

impl EngineBenchRunner {
    async fn run(directory: PathBuf, workers: NonZeroUsize) -> EngineBenchResult<Vec<Duration>> {
        let package = EnginePackageSelector::load(&directory, EnginePackageHost::detect()?)?;
        let mut engine = EngineSession::start(&EngineNativeProcessProvider, package).await?;
        engine
            .configure_resources(EngineResourceConfig::new(
                workers.get(),
                16,
                ClockDuration::ZERO,
            )?)
            .await?;
        let mut session = EngineSearchSession::new(engine);
        let capacity = CapacityBudget::new(workers)?;
        let mut samples = Vec::with_capacity(EngineBenchFixture::SAMPLES);
        for sample in 0..EngineBenchFixture::SAMPLES {
            let context = EngineSearchContext::new(
                EngineSearchGameId::try_from(Uuid::from_u128(1))?,
                EngineSearchRevision::new(u64::try_from(sample)?.saturating_add(1)),
            );
            let tag = session
                .start(EngineSearchSessionRequest::new(
                    context,
                    EngineSearchPosition::new(
                        BoardPosition::standard(),
                        std::iter::empty::<&str>(),
                    )?,
                    EngineSearchLimit::infinite(),
                    capacity.try_lease(workers.get())?,
                ))
                .await?;
            for _event in 0..EngineBenchFixture::SNAPSHOT_EVENTS {
                if matches!(
                    session.next_event(tag).await?,
                    EngineSearchSessionEnumEvent::Snapshot(_)
                ) {
                    break;
                }
            }
            let requested = Instant::now();
            let stop = EngineSearchStop::received_at(tag, requested)?;
            session.stop(stop).await?;
            samples.push(requested.elapsed());
        }
        session
            .shutdown_retained(Instant::now() + Duration::from_secs(3))
            .await?;
        Ok(samples)
    }

    fn milliseconds(samples: &[Duration]) -> String {
        let values: Vec<String> = samples
            .iter()
            .map(|sample| format!("{:.3}", sample.as_secs_f64() * 1_000.0))
            .collect();
        format!("[{}]", values.join(","))
    }
}

fn main() -> EngineBenchResult {
    let directory = std::env::var_os(EngineBenchFixture::DIRECTORY_VARIABLE)
        .map(PathBuf::from)
        .ok_or_else(|| {
            format!(
                "set {} to the engine package directory",
                EngineBenchFixture::DIRECTORY_VARIABLE
            )
        })?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let workers = std::thread::available_parallelism()?;
    let mut runs = Vec::with_capacity(EngineBenchFixture::RUNS);
    for _run in 0..EngineBenchFixture::RUNS {
        runs.push(EngineBenchRunner::milliseconds(
            &runtime.block_on(EngineBenchRunner::run(directory.clone(), workers))?,
        ));
    }
    println!("{{\"stop_ms\":[{}]}}", runs.join(","));
    Ok(())
}
