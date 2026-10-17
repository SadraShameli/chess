use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use strum::IntoEnumIterator;

use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::ClockInstant;
use chess::desktop::engine_commands::DesktopEngineCommandsAllocation;
use chess::desktop::workbench_commands::{
    DesktopWorkbenchCommands, DesktopWorkbenchCommandsRequest,
};
use chess::engine::capabilities::EngineIdentity;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::selector::EnginePackageSelected;
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
    EngineProcessProvider,
};
use chess::workbench::enums::{
    WorkbenchEnumJobState, WorkbenchEnumOutcome, WorkbenchEnumParameters,
};
use chess::workbench::job::{WorkbenchJobDeadline, WorkbenchJobId};
use tokio::time::Instant;
use uuid::Uuid;

use crate::engine_package_selection::EnginePackageFixture;

type DesktopWorkbenchCommandsTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
pub(super) struct DesktopWorkbenchCommandsTestState {
    pub(super) live: AtomicUsize,
    blocked_cleanup: AtomicBool,
    searches: AtomicUsize,
    pub(super) malformed_search: AtomicBool,
}

pub(super) struct DesktopWorkbenchCommandsTestProvider(
    pub(super) Arc<DesktopWorkbenchCommandsTestState>,
);

impl EngineProcessProvider for DesktopWorkbenchCommandsTestProvider {
    type Process = DesktopWorkbenchCommandsTestProcess;

    fn spawn(&self, package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        self.0.live.fetch_add(1, Ordering::SeqCst);
        Ok(DesktopWorkbenchCommandsTestProcess {
            state: Arc::clone(&self.0),
            package: package.clone(),
            lines: VecDeque::new(),
            reaped: false,
        })
    }
}

pub(super) struct DesktopWorkbenchCommandsTestProcess {
    state: Arc<DesktopWorkbenchCommandsTestState>,
    package: EnginePackageSelected,
    lines: VecDeque<String>,
    reaped: bool,
}

impl DesktopWorkbenchCommandsTestProcess {
    async fn reap(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        if self.state.blocked_cleanup.load(Ordering::SeqCst) {
            tokio::time::timeout_at(deadline, std::future::pending::<()>())
                .await
                .map_err(EngineProcessError::ShutdownDeadline)?;
        }
        if !self.reaped {
            self.state.live.fetch_sub(1, Ordering::SeqCst);
            self.reaped = true;
        }
        Ok(())
    }
}

impl EngineProcess for DesktopWorkbenchCommandsTestProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        match command {
            "uci" => {
                self.lines.push_back("id name Reckless 0.9.0".to_owned());
                self.lines
                    .push_back(format!("id author {}", EngineIdentity::AUTHOR));
                self.lines.extend(EngineEnumOption::iter().map(|option| {
                    format!("option name {} type {}", option.name(), option.descriptor())
                }));
                self.lines.push_back("uciok".to_owned());
            }
            "isready" => self.lines.push_back("readyok".to_owned()),
            "setoption name Threads value 1" => self
                .lines
                .push_back("info string set Threads to 1".to_owned()),
            "setoption name Hash value 16" => self
                .lines
                .push_back("info string set Hash to 16 MB".to_owned()),
            "setoption name MoveOverhead value 100" => self
                .lines
                .push_back("info string set MoveOverhead to 100 ms".to_owned()),
            "setoption name UCI_Chess960 value false" => self
                .lines
                .push_back("info string set UCI_Chess960 to false".to_owned()),
            "setoption name MultiPV value 1" => self
                .lines
                .push_back("info string set MultiPV to 1".to_owned()),
            "go infinite" => {
                self.state.searches.fetch_add(1, Ordering::SeqCst);
                if self.state.malformed_search.load(Ordering::SeqCst) {
                    self.lines.push_back("bestmove invalid".to_owned());
                }
            }
            "compiler" => self.lines.extend([
                "Compiler Version: rustc 1.99.0".to_owned(),
                format!("Compiler Target: {}", self.package.target().triple()),
                "Compiler Features: fixture".to_owned(),
            ]),
            "eval" => self.lines.push_back("-17".to_owned()),
            _ => {}
        }
        Ok(())
    }
    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        match self.lines.pop_front() {
            Some(text) => EngineProcessLine::new(EngineProcessEnumPipe::Stdout, text),
            None => std::future::pending().await,
        }
    }
    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.reap(Instant::now() + chess::engine::process_provider::EngineProcessLimits::SHUTDOWN)
            .await
    }
    async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        self.reap(deadline).await
    }
    async fn terminate(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        self.reap(deadline).await
    }
    fn is_shutdown_complete(&self) -> bool {
        self.reaped
    }
}

struct DesktopWorkbenchCommandsTestReservation(Arc<AtomicUsize>);

impl Drop for DesktopWorkbenchCommandsTestReservation {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct DesktopWorkbenchCommandsTestFixture {
    package: EnginePackageFixture,
    provider: DesktopWorkbenchCommandsTestProvider,
    budget: CapacityBudget,
    reservations: Arc<AtomicUsize>,
    commands: DesktopWorkbenchCommands<
        DesktopWorkbenchCommandsTestProcess,
        DesktopWorkbenchCommandsTestReservation,
    >,
}

impl DesktopWorkbenchCommandsTestFixture {
    fn new() -> DesktopWorkbenchCommandsTestResult<Self> {
        Ok(Self {
            package: EnginePackageFixture::new()?,
            provider: DesktopWorkbenchCommandsTestProvider(Arc::new(
                DesktopWorkbenchCommandsTestState::default(),
            )),
            budget: CapacityBudget::new(NonZeroUsize::new(2).ok_or("empty budget")?)?,
            reservations: Arc::new(AtomicUsize::new(0)),
            commands: DesktopWorkbenchCommands::new(),
        })
    }
    async fn start(&mut self) -> DesktopWorkbenchCommandsTestResult<WorkbenchJobId> {
        let id = WorkbenchJobId::try_from(Uuid::from_u128(41))?;
        let lease = self.budget.try_lease(1)?;
        self.reservations.fetch_add(1, Ordering::SeqCst);
        self.commands
            .start(
                &self.provider,
                DesktopWorkbenchCommandsRequest {
                    package: self.package.select()?,
                    parameters: WorkbenchEnumParameters::Compiler,
                    deadline: WorkbenchJobDeadline::default(),
                    id,
                    now: ClockInstant::from_nanos(0),
                },
                DesktopEngineCommandsAllocation::new(
                    lease,
                    DesktopWorkbenchCommandsTestReservation(Arc::clone(&self.reservations)),
                ),
            )
            .await?;
        Ok(id)
    }
}

#[tokio::test(start_paused = true)]
async fn completed_native_diagnostic_delivers_the_real_job_and_isolated_observation()
-> DesktopWorkbenchCommandsTestResult {
    let mut fixture = DesktopWorkbenchCommandsTestFixture::new()?;
    let id = fixture.start().await?;
    assert_eq!(fixture.budget.available(), 1);
    assert_eq!(fixture.provider.0.live.load(Ordering::SeqCst), 1);
    assert!(fixture.commands.result(id).is_err());
    fixture.commands.run().await?;
    let result = fixture
        .commands
        .observe_completion()?
        .ok_or("missing completed job")?;
    assert_eq!(result.job().id(), id);
    assert_eq!(result.job().state(), WorkbenchEnumJobState::Succeeded);
    assert!(matches!(
        result.job().outcome(),
        Some(WorkbenchEnumOutcome::Succeeded(_))
    ));
    let observed = result
        .observation()
        .ok_or("missing actual child observation")?;
    assert_eq!(observed.job_id(), id);
    assert_eq!(observed.package().digest(), result.job().package().digest());
    let resources = observed
        .applied_options()
        .resources()
        .ok_or("missing acknowledged resources")?;
    assert_eq!(resources.threads(), 1);
    assert_eq!(resources.hash_mib(), 16);
    assert_eq!(resources.move_overhead().as_millis(), 100);
    assert_eq!(fixture.reservations.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.provider.0.live.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.budget.available(), 2);
    assert_eq!(fixture.commands.result(id)?.job().id(), id);
    assert!(
        fixture
            .commands
            .result(WorkbenchJobId::try_from(Uuid::from_u128(42))?)
            .is_err()
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_diagnostic_retains_cpu_hash_and_child_until_cleanup_is_observed()
-> DesktopWorkbenchCommandsTestResult {
    let mut fixture = DesktopWorkbenchCommandsTestFixture::new()?;
    let id = fixture.start().await?;
    let stale = WorkbenchJobId::try_from(Uuid::from_u128(42))?;
    assert!(
        fixture
            .commands
            .cancel(stale, Instant::now())
            .await
            .is_err()
    );
    assert_eq!(
        fixture.commands.active_job().ok_or("missing job")?.state(),
        WorkbenchEnumJobState::Running
    );
    fixture
        .provider
        .0
        .blocked_cleanup
        .store(true, Ordering::SeqCst);
    assert!(fixture.commands.cancel(id, Instant::now()).await.is_err());
    assert!(fixture.commands.observe_completion()?.is_none());
    assert_eq!(fixture.budget.available(), 1);
    assert_eq!(fixture.reservations.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.provider.0.live.load(Ordering::SeqCst), 1);
    fixture
        .provider
        .0
        .blocked_cleanup
        .store(false, Ordering::SeqCst);
    fixture.commands.cancel(id, Instant::now()).await?;
    let result = fixture
        .commands
        .observe_completion()?
        .ok_or("missing cancelled job")?;
    assert_eq!(result.job().state(), WorkbenchEnumJobState::Cancelled);
    assert_eq!(fixture.budget.available(), 2);
    assert_eq!(fixture.reservations.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.provider.0.live.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn explicit_native_retry_replaces_only_a_failed_generation_and_never_resumes_search()
-> DesktopWorkbenchCommandsTestResult {
    use crate::desktop_engine_commands::{
        DesktopEngineCommandsTestAdmission, DesktopEngineCommandsTestSelection,
    };
    use chess::board::position::BoardPosition;
    use chess::capacity::executor::CapacityExecutor;
    use chess::clock::duration::ClockDuration;
    use chess::desktop::engine_commands::{
        DesktopEngineCommands, DesktopEngineCommandsContext, DesktopEngineCommandsServices,
    };
    use chess::desktop::identity::DesktopRevision;
    use chess::desktop::messages::DesktopMessagesEntityId;
    use chess::engine::resource_config::EngineResourceConfig;
    use chess::engine::search::identity::{
        EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
    };
    use chess::engine::search::limit::EngineSearchLimit;
    use chess::engine::search::position::EngineSearchPosition;
    use chess::engine::search::session::{
        EngineSearchSession, EngineSearchSessionEnumCompletion, EngineSearchSessionEnumEvent,
    };
    use chess::engine::session::EngineSession;
    let package = EnginePackageFixture::new()?;
    let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("empty budget")?)?;
    let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
    let state = Arc::new(DesktopWorkbenchCommandsTestState::default());
    let provider = DesktopWorkbenchCommandsTestProvider(Arc::clone(&state));
    let mut session = EngineSession::start(&provider, package.select()?).await?;
    session
        .configure_resources(EngineResourceConfig::new(
            1,
            16,
            ClockDuration::from_millis(100)?,
        )?)
        .await?;
    let position =
        EngineSearchPosition::new(BoardPosition::standard(), std::iter::empty::<&str>())?;
    let selection = EngineSearchContext::new(
        EngineSearchGameId::try_from(Uuid::from_u128(88))?,
        EngineSearchRevision::new(0),
    );
    let context = DesktopEngineCommandsContext::new(
        DesktopMessagesEntityId::new(),
        session.generation(),
        DesktopRevision::ZERO,
        selection,
    );
    let mut commands = DesktopEngineCommands::new(
        EngineSearchSession::new(session),
        DesktopEngineCommandsServices {
            provider,
            admission: DesktopEngineCommandsTestAdmission(budget.clone()),
            selection: DesktopEngineCommandsTestSelection {
                context: selection,
                position: position.clone(),
            },
            executor: Arc::clone(&executor),
            reservation: (),
            owner: context.owner(),
        },
    );
    assert!(commands.retry(context).await.is_err());
    assert_eq!(state.live.load(Ordering::SeqCst), 1);
    state.malformed_search.store(true, Ordering::SeqCst);
    let metadata = commands
        .start(context, position.clone(), EngineSearchLimit::infinite())
        .await?;
    let failed = commands.next_event(metadata.tag()).await?;
    assert!(
        matches!(failed, EngineSearchSessionEnumEvent::Finished(terminal) if matches!(terminal.completion(), EngineSearchSessionEnumCompletion::Failed(_)))
    );
    assert_eq!(state.searches.load(Ordering::SeqCst), 1);
    let replacement = commands.retry(context).await?;
    assert_ne!(replacement.generation(), context.generation());
    assert_eq!(replacement.configuration(), context.configuration().next()?);
    assert_eq!(state.live.load(Ordering::SeqCst), 1);
    assert_eq!(state.searches.load(Ordering::SeqCst), 1);
    assert_eq!(position.current(), &BoardPosition::standard());
    assert!(
        commands
            .start(context, position, EngineSearchLimit::infinite())
            .await
            .is_err()
    );
    commands
        .shutdown_retained(
            Instant::now() + chess::engine::process_provider::EngineProcessLimits::SHUTDOWN,
        )
        .await?;
    assert_eq!(state.live.load(Ordering::SeqCst), 0);
    drop(commands);
    Arc::try_unwrap(executor)
        .map_err(|_owner| "retained CPU executor owner")?
        .shutdown()
        .await?;
    Ok(())
}
