use std::collections::VecDeque;
use std::error::Error;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::ClockInstant;
use chess::engine::capabilities::{EngineCapabilities, EngineIdentity};
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::selector::EnginePackageSelected;
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLimits,
    EngineProcessLine, EngineProcessProvider,
};
use chess::engine::session::EngineSession;
use chess::workbench::enums::{
    WorkbenchEnumJobState, WorkbenchEnumOperation, WorkbenchEnumOutcome, WorkbenchEnumParameters,
    WorkbenchEnumResult,
};
use chess::workbench::executor::WorkbenchExecutor;
use chess::workbench::job::{WorkbenchJob, WorkbenchJobDeadline, WorkbenchJobId};
use chess::workbench::parameters::{
    WorkbenchBenchParameters, WorkbenchPerftDepth, WorkbenchPerftParameters,
};
use tokio::time::Instant;
use uuid::Uuid;

use crate::engine_package_selection::EnginePackageFixture;

struct WorkbenchExecutorFixture {
    package: EnginePackageFixture,
    budget: CapacityBudget,
}

impl WorkbenchExecutorFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            package: EnginePackageFixture::new()?,
            budget: CapacityBudget::new(NonZeroUsize::new(2).ok_or("capacity")?)?,
        })
    }

    fn position() -> Result<EngineSearchPosition, Box<dyn Error>> {
        Ok(EngineSearchPosition::new(
            BoardPosition::standard(),
            std::iter::empty::<&str>(),
        )?)
    }

    fn parameters(
        operation: WorkbenchEnumOperation,
    ) -> Result<WorkbenchEnumParameters, Box<dyn Error>> {
        Ok(match operation {
            WorkbenchEnumOperation::Inspector => {
                WorkbenchEnumParameters::Inspector(Self::position()?)
            }
            WorkbenchEnumOperation::RawEvaluation => {
                WorkbenchEnumParameters::RawEvaluation(Self::position()?)
            }
            WorkbenchEnumOperation::Compiler => WorkbenchEnumParameters::Compiler,
            WorkbenchEnumOperation::Perft => WorkbenchEnumParameters::Perft(
                WorkbenchPerftParameters::new(Self::position()?, WorkbenchPerftDepth::new(1)?),
            ),
            WorkbenchEnumOperation::Bench => {
                let mut capabilities = EngineCapabilities::default();
                for option in EngineEnumOption::ALL {
                    capabilities.discover(&format!(
                        "option name {} type {}",
                        option.name(),
                        option.descriptor()
                    ))?;
                }
                WorkbenchEnumParameters::Bench(WorkbenchBenchParameters::new(
                    &capabilities,
                    None,
                    None,
                    None,
                )?)
            }
        })
    }

    fn executor(
        &self,
        operation: WorkbenchEnumOperation,
    ) -> Result<WorkbenchExecutor<WorkbenchExecutorProcess>, Box<dyn Error>> {
        let mut job = WorkbenchJob::new(
            WorkbenchJobId::try_from(Uuid::from_u128(1))?,
            Self::parameters(operation)?,
            self.package.select()?,
            WorkbenchJobDeadline::default(),
            ClockInstant::from_nanos(0),
        )?;
        job.admit(
            job.id(),
            self.budget.try_lease(job.parameters().workers())?,
            ClockInstant::from_nanos(0),
        )?;
        Ok(WorkbenchExecutor::new(job, ClockInstant::from_nanos(0))?)
    }

    fn transcript(operation: WorkbenchEnumOperation) -> Result<Vec<String>, Box<dyn Error>> {
        Ok(match operation {
            WorkbenchEnumOperation::Inspector => {
                let mut lines = Vec::new();
                let separator = " +---+---+---+---+---+---+---+---+".to_owned();
                lines.push(separator.clone());
                for (rank, pieces) in [
                    (8, "rnbqkbnr"),
                    (7, "pppppppp"),
                    (6, "        "),
                    (5, "        "),
                    (4, "        "),
                    (3, "        "),
                    (2, "PPPPPPPP"),
                    (1, "RNBQKBNR"),
                ] {
                    let cells = pieces
                        .chars()
                        .map(|piece| format!(" {piece} |"))
                        .collect::<String>();
                    lines.push(format!(" |{cells} {rank}"));
                    lines.push(separator.clone());
                }
                lines.extend([
                    "   a   b   c   d   e   f   g   h".to_owned(),
                    String::new(),
                    format!("FEN: {}", BoardPosition::standard().fen()),
                ]);
                lines
            }
            WorkbenchEnumOperation::RawEvaluation => vec!["-37".to_owned()],
            WorkbenchEnumOperation::Compiler => vec![
                "Compiler Version: rustc 1.99.0".to_owned(),
                "Compiler Target: aarch64-apple-darwin".to_owned(),
                "Compiler Features: neon".to_owned(),
            ],
            WorkbenchEnumOperation::Perft => {
                let mut lines = vec![
                    "-".repeat(60),
                    "Move Nodes Elapsed NPS".to_owned(),
                    "-".repeat(60),
                ];
                for (index, movement) in BoardPosition::standard().legal_moves().iter().enumerate()
                {
                    lines.push(format!(
                        "{} {} 1 0.000s 25.000 kN/s",
                        index + 1,
                        movement.uci(BoardPosition::standard().variant())
                    ));
                }
                lines.extend([
                    "-".repeat(60),
                    "Total 20 0.001s 20.000 kN/s".to_owned(),
                    "-".repeat(60),
                ]);
                lines
            }
            WorkbenchEnumOperation::Bench => {
                let mut lines = vec![
                    "-".repeat(50),
                    "Nodes Elapsed NPS".to_owned(),
                    "-".repeat(50),
                ];
                lines.extend((0..45).map(|index| format!("{index} 10 0.001s 10000 N/s")));
                lines.extend([
                    "-".repeat(50),
                    "450 0.045s 10000 N/s".to_owned(),
                    "-".repeat(50),
                ]);
                lines
            }
        })
    }
}

use chess::engine::search::position::EngineSearchPosition;

struct WorkbenchExecutorProvider {
    transcript: Vec<String>,
    commands: Arc<Mutex<Vec<String>>>,
    blocked_cleanup: Arc<AtomicBool>,
    fail_cleanup: Arc<AtomicBool>,
    spawned: Arc<AtomicUsize>,
    eof: bool,
    block_command: Option<String>,
    fail_spawn: bool,
}

impl WorkbenchExecutorProvider {
    fn new(transcript: Vec<String>) -> Self {
        Self {
            transcript,
            commands: Arc::default(),
            blocked_cleanup: Arc::default(),
            fail_cleanup: Arc::default(),
            spawned: Arc::default(),
            eof: false,
            block_command: None,
            fail_spawn: false,
        }
    }

    fn commands(&self) -> Result<Vec<String>, Box<dyn Error>> {
        Ok(self
            .commands
            .lock()
            .map_err(|_error| "commands poisoned")?
            .clone())
    }
}

impl EngineProcessProvider for WorkbenchExecutorProvider {
    type Process = WorkbenchExecutorProcess;

    fn spawn(&self, package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        self.spawned.fetch_add(1, Ordering::SeqCst);
        if self.fail_spawn {
            return Err(std::io::Error::other("fixture spawn failed").into());
        }
        Ok(WorkbenchExecutorProcess {
            transcript: self.transcript.clone(),
            lines: VecDeque::new(),
            commands: Arc::clone(&self.commands),
            blocked_cleanup: Arc::clone(&self.blocked_cleanup),
            fail_cleanup: Arc::clone(&self.fail_cleanup),
            complete: false,
            eof: self.eof,
            package: package.clone(),
            block_command: self.block_command.clone(),
        })
    }
}

struct WorkbenchExecutorProcess {
    transcript: Vec<String>,
    lines: VecDeque<String>,
    commands: Arc<Mutex<Vec<String>>>,
    blocked_cleanup: Arc<AtomicBool>,
    fail_cleanup: Arc<AtomicBool>,
    complete: bool,
    eof: bool,
    package: EnginePackageSelected,
    block_command: Option<String>,
}

impl WorkbenchExecutorProcess {
    async fn cleanup(&mut self, command: &str) -> Result<(), EngineProcessError> {
        self.commands
            .lock()
            .map_err(|_error| std::io::Error::other("commands poisoned"))?
            .push(command.to_owned());
        if self.fail_cleanup.swap(false, Ordering::SeqCst) {
            return Err(EngineProcessError::ShutdownTimeout);
        }
        if self.blocked_cleanup.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        self.complete = true;
        Ok(())
    }
}

impl EngineProcess for WorkbenchExecutorProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        assert!(self.package.executable().is_absolute());
        self.commands
            .lock()
            .map_err(|_error| std::io::Error::other("commands poisoned"))?
            .push(command.to_owned());
        if self.block_command.as_deref() == Some(command) {
            std::future::pending::<()>().await;
        }
        match command {
            "uci" => {
                self.lines.extend([
                    "id name Reckless 0.9.0".to_owned(),
                    format!("id author {}", EngineIdentity::AUTHOR),
                ]);
                self.lines.extend(EngineEnumOption::ALL.map(|option| {
                    format!("option name {} type {}", option.name(), option.descriptor())
                }));
                self.lines.push_back("uciok".to_owned());
            }
            "isready" => self.lines.push_back("readyok".to_owned()),
            "setoption name Threads value 1" => self
                .lines
                .push_back("info string set Threads to 1".to_owned()),
            "setoption name Threads value 2" => self
                .lines
                .push_back("info string set Threads to 2".to_owned()),
            "setoption name Hash value 32" => self
                .lines
                .push_back("info string set Hash to 32 MB".to_owned()),
            "setoption name Hash value 16" => self
                .lines
                .push_back("info string set Hash to 16 MB".to_owned()),
            "setoption name MoveOverhead value 100" => self
                .lines
                .push_back("info string set MoveOverhead to 100 ms".to_owned()),
            "setoption name UCI_Chess960 value false" => self
                .lines
                .push_back("info string set UCI_Chess960 to false".to_owned()),
            "d" | "eval" | "compiler" | "perft 1" | "bench 16 1 12" | "bench 32 2 1" => {
                self.lines.extend(self.transcript.iter().cloned())
            }
            _ => {}
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        match self.lines.pop_front() {
            Some(text) => EngineProcessLine::new(EngineProcessEnumPipe::Stdout, text),
            None if self.eof => Err(EngineProcessError::Closed(EngineProcessEnumPipe::Stdout)),
            None => std::future::pending().await,
        }
    }

    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.cleanup("shutdown").await
    }
    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.cleanup("shutdown").await
    }
    fn is_shutdown_complete(&self) -> bool {
        self.complete
    }
    async fn terminate(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.cleanup("terminate").await
    }
}

#[tokio::test(start_paused = true)]
async fn all_operations_parse_pinned_transcripts_and_recover_the_exact_lease()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    for operation in WorkbenchEnumOperation::ALL {
        let provider =
            WorkbenchExecutorProvider::new(WorkbenchExecutorFixture::transcript(operation)?);
        let mut executor = fixture.executor(operation)?;
        executor.start(&provider).await?;
        assert_eq!(fixture.budget.available(), 1);
        executor.run().await?;
        assert_eq!(executor.job().state(), WorkbenchEnumJobState::Succeeded);
        assert_eq!(fixture.budget.available(), 2);
        let Some(WorkbenchEnumOutcome::Succeeded(result)) = executor.job().outcome() else {
            return Err("missing result".into());
        };
        assert_eq!(result.operation(), operation);
        match result {
            WorkbenchEnumResult::Inspector(position) => {
                assert_eq!(position, &BoardPosition::standard())
            }
            WorkbenchEnumResult::RawEvaluation(value) => assert_eq!(value.white_value(), -37),
            WorkbenchEnumResult::Compiler(value) => assert_eq!(value.compiler(), "rustc 1.99.0"),
            WorkbenchEnumResult::Perft(value) => {
                assert_eq!(value.nodes(), 20);
                assert_eq!(value.nodes_per_second(), 20_000);
            }
            WorkbenchEnumResult::Bench(value) => {
                assert_eq!(value.positions(), 45);
                assert_eq!(value.nodes(), 450);
            }
        }
        assert_eq!(provider.spawned.load(Ordering::SeqCst), 1);
        assert!(!provider.commands()?.iter().any(|command| command == "stop"));
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn diagnostic_tail_preserves_startup_and_result_order_without_duplicate_lines()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    let operation = WorkbenchEnumOperation::Inspector;
    let mut executor = fixture.executor(operation)?;
    let provider = WorkbenchExecutorProvider::new(WorkbenchExecutorFixture::transcript(operation)?);
    executor.start(&provider).await?;
    assert_eq!(
        executor
            .diagnostics()
            .entries()
            .next()
            .map(|line| line.text()),
        Some("id name Reckless 0.9.0"),
    );
    assert_eq!(executor.job().diagnostics().entries().len(), 0);
    executor.run().await?;
    assert!(std::ptr::eq(
        executor.diagnostics(),
        executor.job().diagnostics()
    ));
    let lines = executor.job().diagnostics().entries().collect::<Vec<_>>();
    assert_eq!(
        lines.first().map(|line| line.text()),
        Some("id name Reckless 0.9.0")
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.text().starts_with("FEN: "))
            .count(),
        1
    );
    assert_eq!(fixture.budget.available(), fixture.budget.workers());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn malformed_oversized_partial_and_eof_output_cannot_succeed() -> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    for operation in WorkbenchEnumOperation::ALL {
        let valid = WorkbenchExecutorFixture::transcript(operation)?;
        for transcript in [
            vec!["malformed".to_owned()],
            vec!["x".repeat(EngineProcessLimits::LINE_BYTES + 1)],
            valid[..valid.len() - 1].to_vec(),
        ] {
            let mut provider = WorkbenchExecutorProvider::new(transcript);
            provider.eof = true;
            let mut executor = fixture.executor(operation)?;
            executor.start(&provider).await?;
            executor.run().await?;
            assert_eq!(executor.job().state(), WorkbenchEnumJobState::Failed);
            assert_eq!(fixture.budget.available(), 2);
        }
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn live_diagnostics_update_observed_time_without_copying_the_owned_tail()
-> Result<(), Box<dyn Error>> {
    use std::future::Future;
    use std::task::{Context, Waker};

    let fixture = WorkbenchExecutorFixture::new()?;
    let mut executor = fixture.executor(WorkbenchEnumOperation::Compiler)?;
    let provider =
        WorkbenchExecutorProvider::new(vec!["Compiler Version: rustc 1.99.0".to_owned()]);
    executor.start(&provider).await?;
    tokio::time::advance(Duration::from_millis(10)).await;
    {
        let mut running = std::pin::pin!(executor.run());
        assert!(
            running
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    assert_eq!(executor.job().observed_at(), ClockInstant::from_millis(10)?);
    assert_eq!(executor.job().diagnostics().entries().len(), 0);
    assert_eq!(
        executor
            .diagnostics()
            .entries()
            .next_back()
            .map(|line| line.text()),
        Some("Compiler Version: rustc 1.99.0"),
    );
    executor.cancel(executor.job().id()).await?;
    assert!(executor.is_shutdown_complete());
    assert_eq!(
        executor
            .job()
            .diagnostics()
            .entries()
            .filter(|line| line.text().starts_with("Compiler Version: "))
            .count(),
        1,
    );
    assert_eq!(fixture.budget.available(), fixture.budget.workers());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn perft_rejects_illegal_duplicate_missing_and_inconsistent_roots()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    for replacement in [
        "1 e2e5 1 0.000s 1.000 kN/s",
        "1 a2a3 2 0.000s 1.000 kN/s",
        "Total 21 0.001s 1.000 kN/s",
    ] {
        let mut lines = WorkbenchExecutorFixture::transcript(WorkbenchEnumOperation::Perft)?;
        lines[3] = replacement.to_owned();
        let provider = WorkbenchExecutorProvider::new(lines);
        let mut executor = fixture.executor(WorkbenchEnumOperation::Perft)?;
        executor.start(&provider).await?;
        executor.run().await?;
        assert_eq!(executor.job().state(), WorkbenchEnumJobState::Failed);
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn blocked_output_deadline_and_cancel_force_termination_without_stdin_stop()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    for operation in [WorkbenchEnumOperation::Perft, WorkbenchEnumOperation::Bench] {
        for cancel in [false, true] {
            let provider = WorkbenchExecutorProvider::new(Vec::new());
            let mut executor = fixture.executor(operation)?;
            executor.start(&provider).await?;
            if cancel {
                executor.cancel(executor.job().id()).await?;
            } else {
                executor.run().await?;
            }
            assert_eq!(
                executor.job().state(),
                if cancel {
                    WorkbenchEnumJobState::Cancelled
                } else {
                    WorkbenchEnumJobState::TimedOut
                }
            );
            assert!(
                provider
                    .commands()?
                    .iter()
                    .any(|command| command == "terminate")
            );
            assert!(!provider.commands()?.iter().any(|command| command == "stop"));
            assert_eq!(fixture.budget.available(), 2);
        }
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn dropped_run_cancel_and_failed_cleanup_keep_the_owner_and_lease()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    let provider = WorkbenchExecutorProvider::new(Vec::new());
    let mut executor = fixture.executor(WorkbenchEnumOperation::Perft)?;
    executor.start(&provider).await?;
    assert!(
        tokio::time::timeout(Duration::from_millis(1), executor.run())
            .await
            .is_err()
    );
    provider.blocked_cleanup.store(true, Ordering::SeqCst);
    assert!(
        tokio::time::timeout(
            Duration::from_millis(1),
            executor.cancel(executor.job().id())
        )
        .await
        .is_err()
    );
    assert_eq!(fixture.budget.available(), 1);
    assert_eq!(executor.job().state(), WorkbenchEnumJobState::Cancelling);
    assert!(
        tokio::time::timeout(
            Duration::from_millis(1),
            executor.shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        )
        .await
        .is_err()
    );
    assert_eq!(fixture.budget.available(), 1);
    provider.blocked_cleanup.store(false, Ordering::SeqCst);
    provider.fail_cleanup.store(true, Ordering::SeqCst);
    assert!(
        executor
            .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
            .await
            .is_err()
    );
    assert_eq!(fixture.budget.available(), 1);
    executor
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    assert_eq!(fixture.budget.available(), 2);
    assert!(format!("{:?}", executor.job().outcome()).contains("Failed"));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn an_isolated_job_never_commands_the_live_session() -> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    let live_provider = WorkbenchExecutorProvider::new(Vec::new());
    let mut live = EngineSession::start(&live_provider, fixture.package.select()?).await?;
    let before = live_provider.commands()?;
    let provider = WorkbenchExecutorProvider::new(WorkbenchExecutorFixture::transcript(
        WorkbenchEnumOperation::Compiler,
    )?);
    let mut executor = fixture.executor(WorkbenchEnumOperation::Compiler)?;
    executor.start(&provider).await?;
    executor.run().await?;
    assert_eq!(live_provider.commands()?, before);
    assert!(!live.is_shutdown_complete());
    live.shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn dropped_setup_at_handshake_configuration_and_delivery_retains_cleanup()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    for command in ["uci", "setoption name Threads value 1", "perft 1"] {
        let mut provider = WorkbenchExecutorProvider::new(Vec::new());
        provider.block_command = Some(command.to_owned());
        let mut executor = fixture.executor(WorkbenchEnumOperation::Perft)?;
        assert!(
            tokio::time::timeout(Duration::from_millis(1), executor.start(&provider))
                .await
                .is_err()
        );
        assert_eq!(fixture.budget.available(), 1);
        executor.cancel(executor.job().id()).await?;
        assert!(executor.is_shutdown_complete());
        assert_eq!(executor.job().state(), WorkbenchEnumJobState::Cancelled);
        assert_eq!(fixture.budget.available(), 2);
        assert_eq!(
            provider.commands()?.last().map(String::as_str),
            Some("terminate")
        );
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn deadline_includes_admitted_queue_time_and_expiry_spawns_nothing()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    let provider = WorkbenchExecutorProvider::new(Vec::new());
    let mut executor = fixture.executor(WorkbenchEnumOperation::Compiler)?;
    tokio::time::advance(Duration::from_secs(60)).await;
    executor.start(&provider).await?;
    assert_eq!(executor.job().state(), WorkbenchEnumJobState::TimedOut);
    assert_eq!(provider.spawned.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn setup_deadline_and_spawn_failure_recover_only_after_observed_cleanup()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    for fail_spawn in [false, true] {
        let mut provider = WorkbenchExecutorProvider::new(Vec::new());
        provider.fail_spawn = fail_spawn;
        provider.block_command = Some("uci".to_owned());
        let mut executor = fixture.executor(WorkbenchEnumOperation::Compiler)?;
        executor.start(&provider).await?;
        assert!(executor.is_shutdown_complete());
        assert_eq!(executor.job().state(), WorkbenchEnumJobState::Failed);
        assert_eq!(fixture.budget.available(), 2);
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn operation_and_cleanup_failures_both_survive_retained_retry() -> Result<(), Box<dyn Error>>
{
    let fixture = WorkbenchExecutorFixture::new()?;
    let provider = WorkbenchExecutorProvider::new(vec!["invalid raw evaluation".to_owned()]);
    let mut executor = fixture.executor(WorkbenchEnumOperation::RawEvaluation)?;
    executor.start(&provider).await?;
    provider.fail_cleanup.store(true, Ordering::SeqCst);
    assert!(executor.run().await.is_err());
    assert_eq!(fixture.budget.available(), 1);
    executor
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    let Some(WorkbenchEnumOutcome::Failed(
        chess::workbench::job::WorkbenchJobFailure::SessionCleanup { operation, .. },
    )) = executor.job().outcome()
    else {
        return Err("combined failure missing".into());
    };
    assert!(matches!(
        operation.as_ref(),
        chess::workbench::job::WorkbenchJobFailure::Analysis(_)
    ));
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn bench_rejects_wrong_indices_missing_positions_total_overflow_and_nonfinite_metrics()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    for replacement in [
        "1 10 0.001s 10000 N/s",
        "0 18446744073709551615 0.001s 10000 N/s",
        "0 10 NaNs 10000 N/s",
        "0 10 0.001s inf N/s",
    ] {
        let mut lines = WorkbenchExecutorFixture::transcript(WorkbenchEnumOperation::Bench)?;
        lines[3] = replacement.to_owned();
        let provider = WorkbenchExecutorProvider::new(lines);
        let mut executor = fixture.executor(WorkbenchEnumOperation::Bench)?;
        executor.start(&provider).await?;
        executor.run().await?;
        assert_eq!(executor.job().state(), WorkbenchEnumJobState::Failed);
        assert_eq!(fixture.budget.available(), 2);
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn explicit_bench_resources_match_the_admitted_workers_hash_and_command()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    let WorkbenchEnumParameters::Bench(defaults) =
        WorkbenchExecutorFixture::parameters(WorkbenchEnumOperation::Bench)?
    else {
        return Err("bench fixture".into());
    };
    assert_eq!(
        (defaults.hash_mib(), defaults.threads(), defaults.depth()),
        (16, 1, 12)
    );
    let mut capabilities = EngineCapabilities::default();
    for option in EngineEnumOption::ALL {
        capabilities.discover(&format!(
            "option name {} type {}",
            option.name(),
            option.descriptor()
        ))?;
    }
    let parameters = WorkbenchBenchParameters::new(&capabilities, Some(32), Some(2), Some(1))?;
    let mut job = WorkbenchJob::new(
        WorkbenchJobId::try_from(Uuid::from_u128(2))?,
        WorkbenchEnumParameters::Bench(parameters),
        fixture.package.select()?,
        WorkbenchJobDeadline::default(),
        ClockInstant::from_nanos(0),
    )?;
    job.admit(
        job.id(),
        fixture.budget.try_lease(2)?,
        ClockInstant::from_nanos(0),
    )?;
    let provider = WorkbenchExecutorProvider::new(WorkbenchExecutorFixture::transcript(
        WorkbenchEnumOperation::Bench,
    )?);
    let mut executor = WorkbenchExecutor::new(job, ClockInstant::from_nanos(0))?;
    executor.start(&provider).await?;
    assert_eq!(fixture.budget.available(), 0);
    assert!(fixture.budget.try_lease(1).is_err());
    let commands = provider.commands()?;
    for command in [
        "setoption name Threads value 2",
        "setoption name Hash value 32",
        "bench 32 2 1",
    ] {
        assert!(commands.iter().any(|actual| actual == command));
    }
    executor.run().await?;
    assert_eq!(executor.job().state(), WorkbenchEnumJobState::Succeeded);
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn perft_missing_and_duplicate_legal_roots_and_wrong_sum_are_rejected()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    for fault in 0..3 {
        let mut lines = WorkbenchExecutorFixture::transcript(WorkbenchEnumOperation::Perft)?;
        match fault {
            0 => {
                lines.remove(22);
            }
            1 => {
                let duplicate = lines[3]
                    .split_whitespace()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join(" ");
                lines[4] = format!("2 {duplicate}");
            }
            _ => {
                lines[24] = "Total 21 0.001s 1.000 kN/s".to_owned();
            }
        }
        let provider = WorkbenchExecutorProvider::new(lines);
        let mut executor = fixture.executor(WorkbenchEnumOperation::Perft)?;
        executor.start(&provider).await?;
        executor.run().await?;
        assert_eq!(executor.job().state(), WorkbenchEnumJobState::Failed);
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn raw_eval_rejects_search_centipawns_and_compiler_rejects_a_different_target()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    for (operation, transcript) in [
        (
            WorkbenchEnumOperation::RawEvaluation,
            vec!["info depth 1 score cp -37".to_owned()],
        ),
        (
            WorkbenchEnumOperation::Compiler,
            vec![
                "Compiler Version: rustc 1.99.0".to_owned(),
                "Compiler Target: x86_64-pc-windows-msvc".to_owned(),
                "Compiler Features: avx2".to_owned(),
            ],
        ),
    ] {
        let provider = WorkbenchExecutorProvider::new(transcript);
        let mut executor = fixture.executor(operation)?;
        executor.start(&provider).await?;
        executor.run().await?;
        assert_eq!(executor.job().state(), WorkbenchEnumJobState::Failed);
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn absolute_job_deadline_also_interrupts_setup_and_preserves_its_cleanup()
-> Result<(), Box<dyn Error>> {
    let fixture = WorkbenchExecutorFixture::new()?;
    let mut provider = WorkbenchExecutorProvider::new(Vec::new());
    provider.block_command = Some("uci".to_owned());
    let mut job = WorkbenchJob::new(
        WorkbenchJobId::try_from(Uuid::from_u128(3))?,
        WorkbenchEnumParameters::Compiler,
        fixture.package.select()?,
        WorkbenchJobDeadline::new(Duration::from_millis(1))?,
        ClockInstant::from_nanos(0),
    )?;
    job.admit(
        job.id(),
        fixture.budget.try_lease(1)?,
        ClockInstant::from_nanos(0),
    )?;
    let mut executor = WorkbenchExecutor::new(job, ClockInstant::from_nanos(0))?;
    executor.start(&provider).await?;
    assert_eq!(executor.job().state(), WorkbenchEnumJobState::TimedOut);
    assert!(executor.is_shutdown_complete());
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}
