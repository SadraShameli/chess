use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use strum::IntoEnumIterator;

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::ClockDuration;
use chess::desktop::application::DesktopApplicationWork;
use chess::desktop::engine_bridge::{
    DesktopEngineBridgeEnumEvent, DesktopEngineBridgeEnumResult, DesktopEngineBridgeFuture,
    DesktopEngineBridgePort, DesktopEngineBridgeProvider, DesktopEngineBridgeStreams,
};
use chess::desktop::engine_commands::{
    DesktopEngineCommands, DesktopEngineCommandsAdmissionProvider, DesktopEngineCommandsAllocation,
    DesktopEngineCommandsContext, DesktopEngineCommandsEnumOperation,
    DesktopEngineCommandsEnumWorkload, DesktopEngineCommandsResourceRequest,
    DesktopEngineCommandsSelectionProvider, DesktopEngineCommandsServices,
};
use chess::desktop::engine_service::DesktopEngineService;
use chess::desktop::engine_tablebases::DesktopEngineTablebasesPolicy;
use chess::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use chess::desktop::identity::DesktopRevision;
use chess::desktop::messages::DesktopMessagesEntityId;
use chess::desktop::workbench_commands::{
    DesktopWorkbenchCommandsEnumOperation, DesktopWorkbenchCommandsResult,
};
use chess::engine::options::{EngineOptionsEnumValue, EngineOptionsSetting};
use chess::engine::package::selector::EnginePackageSelected;
use chess::engine::process_provider::{
    EngineProcess, EngineProcessError, EngineProcessLine, EngineProcessProvider,
};
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
use chess::workbench::enums::{WorkbenchEnumJobState, WorkbenchEnumParameters};
use chess::workbench::job::{WorkbenchJobDeadline, WorkbenchJobId};
use tokio::sync::Notify;
use tokio::time::Instant;
use uuid::Uuid;

use crate::desktop_engine_commands::{
    DesktopEngineCommandsTestApplication, DesktopEngineCommandsTestClock,
    DesktopEngineCommandsTestObserver, DesktopEngineCommandsTestResult,
};
use crate::desktop_workbench_commands::{
    DesktopWorkbenchCommandsTestProcess, DesktopWorkbenchCommandsTestProvider,
    DesktopWorkbenchCommandsTestState,
};
use crate::engine_package_selection::EnginePackageFixture;

#[derive(Default)]
struct DesktopEngineActorDiagnosticsGate {
    blocked: AtomicBool,
    entered: Notify,
    changed: Notify,
}

impl DesktopEngineActorDiagnosticsGate {
    fn block(&self) {
        self.blocked.store(true, Ordering::Release);
    }
    fn release(&self) {
        self.blocked.store(false, Ordering::Release);
        self.changed.notify_waiters();
    }
    async fn wait(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            self.entered.notify_one();
            if !self.blocked.load(Ordering::Acquire) {
                return;
            }
            changed.await;
        }
    }
}

#[derive(Default)]
struct DesktopEngineActorDiagnosticsControls {
    setup: DesktopEngineActorDiagnosticsGate,
    output: DesktopEngineActorDiagnosticsGate,
    cleanup: DesktopEngineActorDiagnosticsGate,
    stop: DesktopEngineActorDiagnosticsGate,
    configuration: DesktopEngineActorDiagnosticsGate,
    admission: DesktopEngineActorDiagnosticsGate,
    admission_workload: Mutex<Option<DesktopEngineCommandsEnumWorkload>>,
    spawned: AtomicUsize,
    option_writes: AtomicUsize,
    search_writes: AtomicUsize,
    position_writes: AtomicUsize,
    diagnostic_writes: AtomicUsize,
    stop_writes: AtomicUsize,
}

struct DesktopEngineActorDiagnosticsProvider {
    inner: DesktopWorkbenchCommandsTestProvider,
    controls: Arc<DesktopEngineActorDiagnosticsControls>,
}

impl EngineProcessProvider for DesktopEngineActorDiagnosticsProvider {
    type Process = DesktopEngineActorDiagnosticsProcess;
    fn spawn(&self, package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        let ordinal = self.controls.spawned.fetch_add(1, Ordering::AcqRel);
        Ok(DesktopEngineActorDiagnosticsProcess {
            inner: self.inner.spawn(package)?,
            controls: Arc::clone(&self.controls),
            ordinal,
            output_pending: false,
            stop_pending: false,
        })
    }
}

struct DesktopEngineActorDiagnosticsProcess {
    inner: DesktopWorkbenchCommandsTestProcess,
    controls: Arc<DesktopEngineActorDiagnosticsControls>,
    ordinal: usize,
    output_pending: bool,
    stop_pending: bool,
}

impl DesktopEngineActorDiagnosticsProcess {
    async fn cleanup(&self, deadline: Instant) -> Result<(), EngineProcessError> {
        tokio::time::timeout_at(deadline, self.controls.cleanup.wait())
            .await
            .map_err(EngineProcessError::ShutdownDeadline)
    }
}

impl EngineProcess for DesktopEngineActorDiagnosticsProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        if command.starts_with("setoption ") {
            self.controls.option_writes.fetch_add(1, Ordering::AcqRel);
        }
        if command.starts_with("go ") {
            self.controls.search_writes.fetch_add(1, Ordering::AcqRel);
        }
        if command.starts_with("position ") {
            self.controls.position_writes.fetch_add(1, Ordering::AcqRel);
        }
        if matches!(command, "compiler" | "eval") {
            self.controls
                .diagnostic_writes
                .fetch_add(1, Ordering::AcqRel);
            self.output_pending = true;
        }
        if command == "stop" {
            self.controls.stop_writes.fetch_add(1, Ordering::AcqRel);
            self.stop_pending = true;
        }
        self.inner.send(command).await?;
        if self.ordinal == 0 && command == "setoption name MultiPV value 1" {
            self.controls.configuration.wait().await;
        }
        if self.ordinal > 0 && command == "setoption name Threads value 1" {
            self.controls.setup.wait().await;
        }
        Ok(())
    }
    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        if self.stop_pending {
            self.controls.stop.wait().await;
            self.stop_pending = false;
            return EngineProcessLine::new(
                chess::engine::process_provider::EngineProcessEnumPipe::Stdout,
                "bestmove e2e4".to_owned(),
            );
        }
        if self.output_pending {
            self.controls.output.wait().await;
        }
        self.inner.receive().await
    }
    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.shutdown_retained(
            Instant::now() + chess::engine::process_provider::EngineProcessLimits::SHUTDOWN,
        )
        .await
    }
    async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        self.cleanup(deadline).await?;
        self.inner.shutdown_retained(deadline).await
    }
    async fn terminate(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        self.cleanup(deadline).await?;
        self.inner.terminate(deadline).await
    }
    fn is_shutdown_complete(&self) -> bool {
        self.inner.is_shutdown_complete()
    }
}

#[derive(Default)]
struct DesktopEngineActorDiagnosticsLedger {
    owners: AtomicUsize,
    hash_mib: AtomicUsize,
}

struct DesktopEngineActorDiagnosticsReservation {
    ledger: Arc<DesktopEngineActorDiagnosticsLedger>,
    hash_mib: usize,
}

impl DesktopEngineActorDiagnosticsReservation {
    fn new(
        ledger: Arc<DesktopEngineActorDiagnosticsLedger>,
        hash_mib: u32,
    ) -> DesktopEngineCommandsTestResult<Self> {
        let hash_mib = usize::try_from(hash_mib)?;
        ledger.owners.fetch_add(1, Ordering::AcqRel);
        ledger.hash_mib.fetch_add(hash_mib, Ordering::AcqRel);
        Ok(Self { ledger, hash_mib })
    }
}

impl Drop for DesktopEngineActorDiagnosticsReservation {
    fn drop(&mut self) {
        self.ledger.owners.fetch_sub(1, Ordering::AcqRel);
        self.ledger
            .hash_mib
            .fetch_sub(self.hash_mib, Ordering::AcqRel);
    }
}

struct DesktopEngineActorDiagnosticsAdmission {
    budget: CapacityBudget,
    ledger: Arc<DesktopEngineActorDiagnosticsLedger>,
    controls: Arc<DesktopEngineActorDiagnosticsControls>,
}

impl DesktopEngineCommandsAdmissionProvider for DesktopEngineActorDiagnosticsAdmission {
    type Reservation = DesktopEngineActorDiagnosticsReservation;
    async fn admit(
        &mut self,
        request: DesktopEngineCommandsResourceRequest,
    ) -> Result<DesktopEngineCommandsAllocation<Self::Reservation>, DesktopFailure> {
        let workers = usize::try_from(request.resources().threads())
            .map_err(DesktopEngineActorDiagnosticsFixture::failure)?;
        let lease = self
            .budget
            .try_lease(workers)
            .map_err(DesktopEngineActorDiagnosticsFixture::failure)?;
        let reservation = DesktopEngineActorDiagnosticsReservation::new(
            Arc::clone(&self.ledger),
            request.resources().hash_mib(),
        )
        .map_err(DesktopEngineActorDiagnosticsFixture::failure)?;
        let should_wait = *self
            .controls
            .admission_workload
            .try_lock()
            .map_err(DesktopEngineActorDiagnosticsFixture::failure)?
            == Some(request.workload());
        if should_wait {
            self.controls.admission.wait().await;
        }
        Ok(DesktopEngineCommandsAllocation::new(lease, reservation))
    }
    async fn acknowledge(
        &mut self,
        _reservation: &Self::Reservation,
    ) -> Result<(), DesktopFailure> {
        Ok(())
    }
}

struct DesktopEngineActorDiagnosticsSelectionState {
    context: EngineSearchContext,
    position: EngineSearchPosition,
}

#[derive(Clone)]
struct DesktopEngineActorDiagnosticsSelection(
    Arc<Mutex<DesktopEngineActorDiagnosticsSelectionState>>,
);

impl DesktopEngineActorDiagnosticsSelection {
    fn replace(
        &self,
        context: EngineSearchContext,
        position: EngineSearchPosition,
    ) -> DesktopEngineCommandsTestResult {
        *self
            .0
            .try_lock()
            .map_err(DesktopEngineActorDiagnosticsFixture::failure)? =
            DesktopEngineActorDiagnosticsSelectionState { context, position };
        Ok(())
    }
    fn stale() -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::StaleRevision,
            "The authoritative selection changed during admission".to_owned(),
            true,
        )
    }
}

impl DesktopEngineCommandsSelectionProvider for DesktopEngineActorDiagnosticsSelection {
    fn authorize_context(&self, context: EngineSearchContext) -> Result<(), DesktopFailure> {
        let current = self
            .0
            .try_lock()
            .map_err(DesktopEngineActorDiagnosticsFixture::failure)?;
        if current.context == context {
            Ok(())
        } else {
            Err(Self::stale())
        }
    }
    fn authorize(
        &self,
        context: EngineSearchContext,
        position: &EngineSearchPosition,
    ) -> Result<(), DesktopFailure> {
        let current = self
            .0
            .try_lock()
            .map_err(DesktopEngineActorDiagnosticsFixture::failure)?;
        if current.context == context && current.position == *position {
            Ok(())
        } else {
            Err(Self::stale())
        }
    }
}

type DesktopEngineActorDiagnosticsCommands = DesktopEngineCommands<
    DesktopEngineActorDiagnosticsProvider,
    DesktopEngineActorDiagnosticsAdmission,
    DesktopEngineActorDiagnosticsSelection,
>;
type DesktopEngineActorDiagnosticsService = DesktopEngineService<
    DesktopEngineActorDiagnosticsProvider,
    DesktopEngineActorDiagnosticsAdmission,
    DesktopEngineActorDiagnosticsSelection,
    DesktopEngineCommandsTestClock,
    DesktopEngineTablebasesPolicy,
    DesktopEngineCommandsTestObserver,
>;

struct DesktopEngineActorDiagnosticsFixture {
    commands: Option<DesktopEngineActorDiagnosticsCommands>,
    context: DesktopEngineCommandsContext,
    position: EngineSearchPosition,
    controls: Arc<DesktopEngineActorDiagnosticsControls>,
    process_state: Arc<DesktopWorkbenchCommandsTestState>,
    selection: DesktopEngineActorDiagnosticsSelection,
    ledger: Arc<DesktopEngineActorDiagnosticsLedger>,
    executor: Arc<CapacityExecutor>,
    budget: CapacityBudget,
    package: EnginePackageFixture,
    directory: tempfile::TempDir,
}

impl DesktopEngineActorDiagnosticsFixture {
    fn live_context()
    -> DesktopEngineCommandsTestResult<chess::game::engine_provider::GameEngineContext> {
        Ok(chess::game::engine_provider::GameEngineContext::new(
            "00000000-0000-0000-0000-000000000902".parse()?,
            chess::game::enums::GameRevision::ZERO,
        ))
    }
    async fn new() -> DesktopEngineCommandsTestResult<Self> {
        let package = EnginePackageFixture::new()?;
        let directory = tempfile::tempdir()?;
        let controls = Arc::new(DesktopEngineActorDiagnosticsControls::default());
        let process_state = Arc::new(DesktopWorkbenchCommandsTestState::default());
        let provider = DesktopEngineActorDiagnosticsProvider {
            inner: DesktopWorkbenchCommandsTestProvider(Arc::clone(&process_state)),
            controls: Arc::clone(&controls),
        };
        let budget =
            CapacityBudget::new(NonZeroUsize::new(2).ok_or("empty diagnostic CPU budget")?)?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let mut session = EngineSession::start(&provider, package.select()?).await?;
        session
            .configure_resources(EngineResourceConfig::new(
                1,
                16,
                ClockDuration::from_millis(100)?,
            )?)
            .await?;
        let position = EngineSearchPosition::new(
            chess::board::position::BoardPosition::standard(),
            ["g1f3", "g8f6", "b1c3", "b8c6"],
        )?;
        let context = DesktopEngineCommandsContext::new(
            DesktopMessagesEntityId::new(),
            session.generation(),
            DesktopRevision::ZERO,
            EngineSearchContext::new(
                EngineSearchGameId::try_from(Uuid::from_u128(901))?,
                EngineSearchRevision::new(4),
            ),
        );
        let selection = DesktopEngineActorDiagnosticsSelection(Arc::new(Mutex::new(
            DesktopEngineActorDiagnosticsSelectionState {
                context: context.selection(),
                position: position.clone(),
            },
        )));
        let ledger = Arc::new(DesktopEngineActorDiagnosticsLedger::default());
        let reservation = DesktopEngineActorDiagnosticsReservation::new(Arc::clone(&ledger), 16)?;
        let commands = DesktopEngineCommands::new(
            EngineSearchSession::new(session),
            DesktopEngineCommandsServices {
                provider,
                admission: DesktopEngineActorDiagnosticsAdmission {
                    budget: budget.clone(),
                    ledger: Arc::clone(&ledger),
                    controls: Arc::clone(&controls),
                },
                selection: selection.clone(),
                executor: Arc::clone(&executor),
                reservation,
                owner: context.owner(),
            },
        );
        Ok(Self {
            commands: Some(commands),
            context,
            position,
            controls,
            process_state,
            selection,
            ledger,
            executor,
            budget,
            package,
            directory,
        })
    }
    fn take_commands(
        &mut self,
    ) -> DesktopEngineCommandsTestResult<DesktopEngineActorDiagnosticsCommands> {
        self.commands
            .take()
            .ok_or_else(|| "diagnostic fixture commands already moved".into())
    }
    fn admission(
        &self,
        workload: DesktopEngineCommandsEnumWorkload,
    ) -> DesktopEngineCommandsTestResult {
        *self
            .controls
            .admission_workload
            .try_lock()
            .map_err(Self::failure)? = Some(workload);
        self.controls.admission.block();
        Ok(())
    }
    fn changed_context(&self) -> EngineSearchContext {
        EngineSearchContext::new(
            self.context.selection().game(),
            EngineSearchRevision::new(self.context.selection().revision().value() + 1),
        )
    }
    fn changed_history(&self) -> DesktopEngineCommandsTestResult<EngineSearchPosition> {
        let position = EngineSearchPosition::new(
            chess::board::position::BoardPosition::standard(),
            ["b1c3", "b8c6", "g1f3", "g8f6"],
        )?;
        assert_eq!(self.position.current(), position.current());
        assert_ne!(self.position.history(), position.history());
        Ok(position)
    }
    fn assert_initial_reservation(&self) {
        assert_eq!(self.budget.available(), self.budget.workers());
        assert_eq!(self.ledger.owners.load(Ordering::Acquire), 1);
        assert_eq!(self.ledger.hash_mib.load(Ordering::Acquire), 16);
    }
    async fn close(mut self) -> DesktopEngineCommandsTestResult {
        if let Some(mut commands) = self.commands.take() {
            commands
                .shutdown_retained(Instant::now() + std::time::Duration::from_secs(30))
                .await?;
        }
        assert_eq!(self.process_state.live.load(Ordering::Acquire), 0);
        assert_eq!(self.budget.available(), self.budget.workers());
        assert_eq!(self.ledger.owners.load(Ordering::Acquire), 0);
        assert_eq!(self.ledger.hash_mib.load(Ordering::Acquire), 0);
        assert!(self.package.select()?.executable().is_absolute());
        drop(self.commands);
        Arc::try_unwrap(self.executor)
            .map_err(|_owner| "diagnostic fixture retains CPU executor")?
            .shutdown()
            .await?;
        Ok(())
    }
    fn failure(error: impl std::fmt::Display) -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            error.to_string(),
            true,
        )
    }
}

struct DesktopEngineActorDiagnosticsActor {
    service: DesktopEngineActorDiagnosticsService,
    port: DesktopEngineBridgePort,
    streams: DesktopEngineBridgeStreams,
    application: DesktopEngineCommandsTestApplication,
    fixture: DesktopEngineActorDiagnosticsFixture,
}

impl DesktopEngineActorDiagnosticsActor {
    fn new(
        mut fixture: DesktopEngineActorDiagnosticsFixture,
    ) -> DesktopEngineCommandsTestResult<Self> {
        let (service, port, streams) = DesktopEngineService::new(
            fixture.take_commands()?,
            Arc::new(DesktopEngineCommandsTestClock),
            DesktopEngineTablebasesPolicy::new(fixture.directory.path().to_path_buf()),
            DesktopEngineCommandsTestObserver::default(),
        );
        Ok(Self {
            service,
            port,
            streams,
            application: DesktopEngineCommandsTestApplication::new(),
            fixture,
        })
    }
    fn request(
        &self,
        action: DesktopEngineCommandsEnumOperation,
    ) -> DesktopEngineCommandsTestResult<(DesktopApplicationWork, DesktopEngineBridgeFuture)> {
        let work = self.application.work(self.fixture.context, action)?;
        let response = self
            .port
            .execute(DesktopEngineCommandsTestApplication::bridge(&work)?);
        Ok((work, response))
    }
    async fn response(
        &mut self,
        mut response: DesktopEngineBridgeFuture,
    ) -> DesktopEngineCommandsTestResult<Result<DesktopEngineBridgeEnumResult, DesktopFailure>>
    {
        tokio::select! {
            biased;
            result = &mut response => Ok(result),
            result = self.service.run() => { result?; Ok(response.await) }
        }
    }
    fn complete(
        &self,
        work: &DesktopApplicationWork,
        result: Result<DesktopEngineBridgeEnumResult, DesktopFailure>,
    ) -> DesktopEngineCommandsTestResult {
        if work.request().command().operation().is_control() {
            self.application
                .application
                .complete_engine_control(work, result)?;
        } else {
            match result {
                Ok(value) => self.application.application.complete_value(
                    work,
                    chess::desktop::dto::DesktopEnumValue::Engine(Box::new(value)),
                    None,
                )?,
                Err(error) => self.application.application.complete_failure(work, error)?,
            }
        }
        Ok(())
    }
    async fn terminal(
        &mut self,
    ) -> DesktopEngineCommandsTestResult<DesktopWorkbenchCommandsResult> {
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        let event = tokio::select! {
            biased;
            event = self.streams.terminal.recv() => event.ok_or("diagnostic terminal stream closed")?,
            result = self.service.run() => { result?; return Err("actor exited before diagnostic completion".into()); }
            () = tokio::time::sleep_until(deadline) => return Err("diagnostic work stopped being polled".into()),
        };
        match event {
            DesktopEngineBridgeEnumEvent::Workbench(result) => Ok(result),
            DesktopEngineBridgeEnumEvent::Analysis(_)
            | DesktopEngineBridgeEnumEvent::Failure(_) => {
                Err("expected original diagnostic terminal result".into())
            }
        }
    }
    async fn close(mut self) -> DesktopEngineCommandsTestResult {
        self.service
            .shutdown_retained(Instant::now() + std::time::Duration::from_secs(30))
            .await?;
        assert!(self.service.is_shutdown_complete());
        drop(self.service);
        self.fixture.close().await
    }
}

impl DesktopEngineActorDiagnosticsActor {
    fn start(
        &self,
    ) -> DesktopEngineCommandsTestResult<(DesktopApplicationWork, DesktopEngineBridgeFuture)> {
        self.request(DesktopEngineCommandsEnumOperation::Workbench(
            DesktopWorkbenchCommandsEnumOperation::Start {
                parameters: WorkbenchEnumParameters::Compiler,
                deadline: WorkbenchJobDeadline::default(),
            },
        ))
    }
}

#[tokio::test(start_paused = true)]
async fn matching_cancel_preempts_pending_diagnostic_setup_and_reaps_original_reservation()
-> DesktopEngineCommandsTestResult {
    let fixture = DesktopEngineActorDiagnosticsFixture::new().await?;
    fixture.controls.setup.block();
    fixture.controls.cleanup.block();
    let mut actor = DesktopEngineActorDiagnosticsActor::new(fixture)?;
    let (start_work, mut start_response) = actor.start()?;
    let job = WorkbenchJobId::try_from(start_work.receipt().request_id().as_uuid())?;
    let mut running = Box::pin(actor.service.run());
    tokio::select! {
        () = actor.fixture.controls.setup.entered.notified() => {}
        result = &mut running => { result?; return Err("actor exited before diagnostic setup stalled".into()); }
    }
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 2);
    assert_eq!(actor.fixture.ledger.owners.load(Ordering::Acquire), 2);
    assert_eq!(actor.fixture.ledger.hash_mib.load(Ordering::Acquire), 32);
    assert_eq!(actor.fixture.budget.available(), 1);
    let cancel_work = actor.application.work(
        actor.fixture.context,
        DesktopEngineCommandsEnumOperation::Workbench(
            DesktopWorkbenchCommandsEnumOperation::Cancel { job },
        ),
    )?;
    assert!(start_work.receipt().is_cancellation_requested());
    let mut cancel_response = actor
        .port
        .execute(DesktopEngineCommandsTestApplication::bridge(&cancel_work)?);
    let received = Instant::now();
    tokio::select! {
        () = actor.fixture.controls.cleanup.entered.notified() => {}
        result = &mut cancel_response => return Err(format!("Cancel completed before controlled reaping: {result:?}").into()),
        result = &mut running => { result?; return Err("actor exited before Cancel acquired cleanup".into()); }
        () = tokio::time::sleep_until(received + std::time::Duration::from_secs(1)) => return Err("matching Cancel never reached cleanup while setup was stalled".into()),
    }
    assert!(
        std::future::poll_fn(|context| std::task::Poll::Ready(
            start_response.as_mut().poll(context)
        ))
        .await
        .is_pending()
    );
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 2);
    assert_eq!(actor.fixture.ledger.owners.load(Ordering::Acquire), 2);
    assert_eq!(actor.fixture.ledger.hash_mib.load(Ordering::Acquire), 32);
    assert_eq!(actor.fixture.budget.available(), 1);
    actor.fixture.controls.cleanup.release();
    let cancelled = tokio::select! {
        biased;
        result = &mut cancel_response => result?,
        result = &mut running => { result?; return Err("actor exited before matching Cancel response".into()); }
        () = tokio::time::sleep_until(received + std::time::Duration::from_secs(1)) => return Err("matching Cancel waited behind pending diagnostic setup".into()),
    };
    assert!(
        matches!(cancelled, DesktopEngineBridgeEnumResult::WorkbenchCancelled(actual) if actual == job)
    );
    let start_result = tokio::select! {
        biased;
        result = &mut start_response => result,
        result = &mut running => { result?; return Err("actor exited before settling original Start".into()); }
        () = tokio::time::sleep_until(received + std::time::Duration::from_secs(1)) => return Err("original diagnostic Start lost its receipt".into()),
    };
    assert!(start_result.is_err());
    assert_eq!(Instant::now(), received);
    assert!(actor.fixture.controls.setup.blocked.load(Ordering::Acquire));
    assert_eq!(actor.fixture.controls.spawned.load(Ordering::Acquire), 2);
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 1);
    assert_eq!(
        actor
            .fixture
            .controls
            .diagnostic_writes
            .load(Ordering::Acquire),
        0
    );
    actor.fixture.assert_initial_reservation();
    drop(running);
    actor.complete(&start_work, start_result)?;
    actor.complete(&cancel_work, Ok(cancelled))?;
    let terminal = actor.terminal().await?;
    assert_eq!(terminal.job().id(), job);
    assert_eq!(terminal.job().state(), WorkbenchEnumJobState::Cancelled);
    actor.fixture.controls.setup.release();
    Box::pin(actor.close()).await
}

#[tokio::test(start_paused = true)]
async fn refused_second_diagnostic_start_keeps_first_result_and_releases_its_capacity()
-> DesktopEngineCommandsTestResult {
    let fixture = DesktopEngineActorDiagnosticsFixture::new().await?;
    fixture.controls.output.block();
    let mut actor = DesktopEngineActorDiagnosticsActor::new(fixture)?;
    let (first_work, first_response) = actor.start()?;
    let first_result = actor.response(first_response).await??;
    let DesktopEngineBridgeEnumResult::WorkbenchStarted(job) = first_result else {
        return Err("first diagnostic did not start".into());
    };
    actor.complete(
        &first_work,
        Ok(DesktopEngineBridgeEnumResult::WorkbenchStarted(job)),
    )?;
    let (second_work, second_response) = actor.start()?;
    let refused = actor.response(second_response).await?;
    assert!(
        matches!(&refused, Err(error) if matches!(error.code(), DesktopEnumFailureCode::ResourceRefusal))
    );
    actor.complete(&second_work, refused)?;
    assert_eq!(actor.fixture.controls.spawned.load(Ordering::Acquire), 2);
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 2);
    assert_eq!(actor.fixture.budget.available(), 1);
    assert_eq!(actor.fixture.ledger.owners.load(Ordering::Acquire), 2);
    actor.fixture.controls.output.release();
    let terminal = actor.terminal().await?;
    assert_eq!(terminal.job().id(), job);
    assert_eq!(terminal.job().state(), WorkbenchEnumJobState::Succeeded);
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 1);
    actor.fixture.assert_initial_reservation();
    Box::pin(actor.close()).await
}

#[tokio::test(start_paused = true)]
async fn retried_public_shutdown_awaits_its_own_completion_after_the_first_future_is_cancelled()
-> DesktopEngineCommandsTestResult {
    let fixture = DesktopEngineActorDiagnosticsFixture::new().await?;
    fixture.controls.cleanup.block();
    let mut actor = DesktopEngineActorDiagnosticsActor::new(fixture)?;
    let now = Instant::now();
    let mut first = Box::pin(
        actor
            .service
            .shutdown_retained(now + std::time::Duration::from_secs(1)),
    );
    tokio::select! {
        () = actor.fixture.controls.cleanup.entered.notified() => {}
        result = &mut first => { result?; return Err("first shutdown did not retain pending cleanup".into()); }
    }
    drop(first);
    assert!(!actor.service.is_shutdown_complete());
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 1);
    assert_eq!(actor.fixture.ledger.owners.load(Ordering::Acquire), 1);
    actor.fixture.controls.cleanup.release();
    actor
        .service
        .shutdown_retained(now + std::time::Duration::from_secs(2))
        .await?;
    assert!(
        actor.service.is_shutdown_complete(),
        "retry returned the first shutdown outcome while its own operation remained pending"
    );
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 0);
    assert_eq!(actor.fixture.ledger.owners.load(Ordering::Acquire), 0);
    assert_eq!(actor.fixture.ledger.hash_mib.load(Ordering::Acquire), 0);
    assert_eq!(Instant::now(), now);
    Box::pin(actor.close()).await
}

#[tokio::test(start_paused = true)]
async fn start_reauthorizes_complete_history_after_async_admission_before_engine_writes()
-> DesktopEngineCommandsTestResult {
    let mut fixture = DesktopEngineActorDiagnosticsFixture::new().await?;
    fixture.admission(DesktopEngineCommandsEnumWorkload::Analysis)?;
    let mut commands = fixture.take_commands()?;
    let writes = fixture.controls.search_writes.load(Ordering::Acquire);
    let positions = fixture.controls.position_writes.load(Ordering::Acquire);
    let mut start = Box::pin(commands.start(
        fixture.context,
        fixture.position.clone(),
        EngineSearchLimit::infinite(),
    ));
    tokio::select! {
        () = fixture.controls.admission.entered.notified() => {}
        result = &mut start => { result?; return Err("Start did not wait for admission".into()); }
    }
    assert_eq!(fixture.budget.available(), 1);
    fixture
        .selection
        .replace(fixture.context.selection(), fixture.changed_history()?)?;
    fixture.controls.admission.release();
    let failure = start
        .await
        .err()
        .ok_or("stale full-history Start was admitted")?;
    assert!(matches!(
        failure.code(),
        DesktopEnumFailureCode::StaleRevision
    ));
    assert_eq!(
        fixture.controls.search_writes.load(Ordering::Acquire),
        writes
    );
    assert_eq!(
        fixture.controls.position_writes.load(Ordering::Acquire),
        positions
    );
    assert_eq!(
        commands.snapshot().configuration(),
        fixture.context.configuration()
    );
    fixture.assert_initial_reservation();
    fixture.commands = Some(commands);
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn diagnostic_reauthorizes_selection_after_async_admission_before_spawning_child()
-> DesktopEngineCommandsTestResult {
    let fixture = DesktopEngineActorDiagnosticsFixture::new().await?;
    fixture.admission(DesktopEngineCommandsEnumWorkload::Diagnostic)?;
    let mut actor = DesktopEngineActorDiagnosticsActor::new(fixture)?;
    let spawned = actor.fixture.controls.spawned.load(Ordering::Acquire);
    let options = actor.fixture.controls.option_writes.load(Ordering::Acquire);
    let (work, mut response) = actor.request(DesktopEngineCommandsEnumOperation::Workbench(
        DesktopWorkbenchCommandsEnumOperation::Start {
            parameters: WorkbenchEnumParameters::RawEvaluation(actor.fixture.position.clone()),
            deadline: WorkbenchJobDeadline::default(),
        },
    ))?;
    let mut running = Box::pin(actor.service.run());
    tokio::select! {
        () = actor.fixture.controls.admission.entered.notified() => {}
        result = &mut running => { result?; return Err("actor exited before diagnostic admission".into()); }
    }
    actor.fixture.selection.replace(
        actor.fixture.changed_context(),
        actor.fixture.position.clone(),
    )?;
    actor.fixture.controls.admission.release();
    let result = tokio::select! {
        biased;
        result = &mut response => result,
        result = &mut running => { result?; return Err("actor exited before stale diagnostic refusal".into()); }
    };
    assert!(
        matches!(&result, Err(error) if matches!(error.code(), DesktopEnumFailureCode::StaleRevision))
    );
    assert_eq!(
        actor.fixture.controls.spawned.load(Ordering::Acquire),
        spawned
    );
    assert_eq!(
        actor.fixture.controls.option_writes.load(Ordering::Acquire),
        options
    );
    assert_eq!(
        actor
            .fixture
            .controls
            .diagnostic_writes
            .load(Ordering::Acquire),
        0
    );
    actor.fixture.assert_initial_reservation();
    drop(running);
    actor.complete(&work, result)?;
    Box::pin(actor.close()).await
}

#[tokio::test(start_paused = true)]
async fn configuration_reauthorizes_selection_after_async_admission_before_option_commit()
-> DesktopEngineCommandsTestResult {
    let mut fixture = DesktopEngineActorDiagnosticsFixture::new().await?;
    fixture.admission(DesktopEngineCommandsEnumWorkload::Configuration)?;
    let mut commands = fixture.take_commands()?;
    let setting = EngineOptionsSetting::new(
        EngineOptionsEnumValue::MultiPv(1),
        commands.snapshot().capabilities(),
    )?;
    let writes = fixture.controls.option_writes.load(Ordering::Acquire);
    let mut configure = Box::pin(commands.configure(fixture.context, setting));
    tokio::select! {
        () = fixture.controls.admission.entered.notified() => {}
        result = &mut configure => { result?; return Err("configuration did not wait for admission".into()); }
    }
    fixture
        .selection
        .replace(fixture.changed_context(), fixture.position.clone())?;
    fixture.controls.admission.release();
    let failure = configure
        .await
        .err()
        .ok_or("stale configuration was committed")?;
    assert!(matches!(
        failure.code(),
        DesktopEnumFailureCode::StaleRevision
    ));
    assert_eq!(
        fixture.controls.option_writes.load(Ordering::Acquire),
        writes
    );
    assert_eq!(
        commands.snapshot().configuration(),
        fixture.context.configuration()
    );
    fixture.assert_initial_reservation();
    fixture.commands = Some(commands);
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn retry_reauthorizes_selection_after_async_admission_before_replacing_generation()
-> DesktopEngineCommandsTestResult {
    let mut fixture = DesktopEngineActorDiagnosticsFixture::new().await?;
    let mut commands = fixture.take_commands()?;
    fixture
        .process_state
        .malformed_search
        .store(true, Ordering::Release);
    let metadata = commands
        .start(
            fixture.context,
            fixture.position.clone(),
            EngineSearchLimit::infinite(),
        )
        .await?;
    let terminal = commands.next_event(metadata.tag()).await?;
    assert!(
        matches!(terminal, EngineSearchSessionEnumEvent::Finished(terminal) if matches!(terminal.completion(), EngineSearchSessionEnumCompletion::Failed(_)))
    );
    assert_eq!(fixture.process_state.live.load(Ordering::Acquire), 0);
    fixture.admission(DesktopEngineCommandsEnumWorkload::Retry)?;
    let spawned = fixture.controls.spawned.load(Ordering::Acquire);
    let writes = fixture.controls.option_writes.load(Ordering::Acquire);
    let mut retry = Box::pin(commands.retry(fixture.context));
    tokio::select! {
        () = fixture.controls.admission.entered.notified() => {}
        result = &mut retry => { result?; return Err("Retry did not wait for admission".into()); }
    }
    fixture
        .selection
        .replace(fixture.changed_context(), fixture.position.clone())?;
    fixture.controls.admission.release();
    let failure = retry
        .await
        .err()
        .ok_or("stale Retry replaced the failed generation")?;
    assert!(matches!(
        failure.code(),
        DesktopEnumFailureCode::StaleRevision
    ));
    assert_eq!(fixture.controls.spawned.load(Ordering::Acquire), spawned);
    assert_eq!(
        fixture.controls.option_writes.load(Ordering::Acquire),
        writes
    );
    assert_eq!(
        commands.snapshot().generation(),
        fixture.context.generation()
    );
    fixture.assert_initial_reservation();
    fixture.commands = Some(commands);
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn retained_demand_events_do_not_replace_authoritative_current_admission()
-> DesktopEngineCommandsTestResult {
    let fixture = DesktopEngineActorDiagnosticsFixture::new().await?;
    let mut actor = DesktopEngineActorDiagnosticsActor::new(fixture)?;
    let (_demand, receiver) =
        tokio::sync::watch::channel(Some(DesktopEngineActorDiagnosticsFixture::live_context()?));
    actor.service = actor.service.with_live_demand(receiver);
    let (work, response) = actor.request(DesktopEngineCommandsEnumOperation::Start {
        position: actor.fixture.position.clone(),
        limit: EngineSearchLimit::infinite(),
    })?;
    let result = actor.response(response).await?;
    let admitted = matches!(result, Ok(DesktopEngineBridgeEnumResult::Started(_)));
    actor.complete(&work, result)?;
    Box::pin(actor.close()).await?;
    assert!(
        admitted,
        "a historical demand pulse permanently blocked current admission"
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn live_demand_interrupts_pending_admission_before_child_effect_and_keeps_idle_hash()
-> DesktopEngineCommandsTestResult {
    let fixture = DesktopEngineActorDiagnosticsFixture::new().await?;
    fixture.admission(DesktopEngineCommandsEnumWorkload::Diagnostic)?;
    let mut actor = DesktopEngineActorDiagnosticsActor::new(fixture)?;
    let (demand, receiver) = tokio::sync::watch::channel(None);
    actor.service = actor.service.with_live_demand(receiver);
    let (work, mut response) = actor.start()?;
    let mut running = Box::pin(actor.service.run());
    tokio::select! {
        () = actor.fixture.controls.admission.entered.notified() => {}
        result = &mut running => { result?; return Err("actor exited before gated admission".into()); }
    }
    assert_eq!(actor.fixture.budget.available(), 1);
    demand.send_replace(Some(DesktopEngineActorDiagnosticsFixture::live_context()?));
    let received = Instant::now();
    let result = tokio::select! {
        biased;
        result = &mut response => result,
        result = &mut running => { result?; return Err("actor exited during internal demand preemption".into()); }
        () = tokio::time::sleep_until(received + std::time::Duration::from_secs(1)) => return Err("live demand waited for low-priority admission".into()),
    };
    assert!(
        matches!(&result, Err(error) if matches!(error.code(), DesktopEnumFailureCode::Cancelled))
    );
    assert!(
        actor
            .fixture
            .controls
            .admission
            .blocked
            .load(Ordering::Acquire)
    );
    assert_eq!(actor.fixture.controls.spawned.load(Ordering::Acquire), 1);
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 1);
    assert_eq!(
        actor
            .fixture
            .controls
            .diagnostic_writes
            .load(Ordering::Acquire),
        0
    );
    actor.fixture.assert_initial_reservation();
    drop(running);
    actor.complete(&work, result)?;
    assert_eq!(actor.service.live_demand_received_at(), Some(received));
    Box::pin(actor.close()).await
}

#[tokio::test(start_paused = true)]
async fn repeated_and_coalesced_live_demand_keeps_the_original_stop_receipt_and_no_resume()
-> DesktopEngineCommandsTestResult {
    use std::future::{Future, poll_fn};
    use std::task::Poll;
    let fixture = DesktopEngineActorDiagnosticsFixture::new().await?;
    fixture.controls.stop.block();
    let mut actor = DesktopEngineActorDiagnosticsActor::new(fixture)?;
    let (demand, receiver) = tokio::sync::watch::channel(None);
    actor.service = actor.service.with_live_demand(receiver);
    let (work, response) = actor.request(DesktopEngineCommandsEnumOperation::Start {
        position: actor.fixture.position.clone(),
        limit: EngineSearchLimit::infinite(),
    })?;
    let started = actor.response(response).await??;
    let DesktopEngineBridgeEnumResult::Started(metadata) = &started else {
        return Err("missing analysis metadata".into());
    };
    let tag = metadata.tag();
    actor.complete(&work, Ok(started))?;
    let context = DesktopEngineActorDiagnosticsFixture::live_context()?;
    let received = Instant::now();
    demand.send_replace(Some(context));
    let mut running = Box::pin(actor.service.run());
    tokio::select! {
        () = actor.fixture.controls.stop.entered.notified() => {}
        result = &mut running => { result?; return Err("actor exited before priority Stop".into()); }
    }
    tokio::time::advance(std::time::Duration::from_millis(500)).await;
    demand.send_replace(Some(context));
    poll_fn(|context| {
        assert!(running.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    tokio::time::advance(std::time::Duration::from_millis(250)).await;
    demand.send_replace(None);
    demand.send_replace(Some(context));
    let last_received = Instant::now();
    poll_fn(|context| {
        assert!(running.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    actor.fixture.controls.stop.release();
    let event = tokio::select! {
        event = actor.streams.terminal.recv() => event.ok_or("missing preempted analysis terminal")?,
        result = &mut running => { result?; return Err("actor exited before priority terminal delivery".into()); }
        () = tokio::time::sleep_until(received + std::time::Duration::from_secs(1)) => return Err("priority Stop did not complete after its acknowledgment".into()),
    };
    let DesktopEngineBridgeEnumEvent::Analysis(event) = event else {
        return Err("unexpected priority terminal".into());
    };
    let EngineSearchSessionEnumEvent::Finished(terminal) = event.as_ref() else {
        return Err("priority did not publish a terminal".into());
    };
    assert_eq!(terminal.snapshot().metadata().tag(), tag);
    assert!(matches!(
        terminal.completion(),
        EngineSearchSessionEnumCompletion::Stopped
    ));
    let chess::engine::search::session::EngineSearchSessionEnumCancellation::StopRequested(stop) =
        terminal.snapshot().cancellation()
    else {
        return Err("priority Stop lost its receipt".into());
    };
    assert_eq!(
        stop.deadline(),
        received + chess::engine::search::stop::EngineSearchStop::SAFETY
    );
    assert_eq!(
        actor.fixture.controls.stop_writes.load(Ordering::Acquire),
        1
    );
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 1);
    actor.fixture.assert_initial_reservation();
    drop(running);
    assert_eq!(actor.service.live_demand_received_at(), Some(last_received));
    let (work, response) = actor.request(DesktopEngineCommandsEnumOperation::Start {
        position: actor.fixture.position.clone(),
        limit: EngineSearchLimit::infinite(),
    })?;
    let restarted = actor.response(response).await?;
    assert!(matches!(
        &restarted,
        Ok(DesktopEngineBridgeEnumResult::Started(_))
    ));
    actor.complete(&work, restarted)?;
    assert_eq!(
        actor.fixture.controls.search_writes.load(Ordering::Acquire),
        2
    );
    Box::pin(actor.close()).await
}

#[tokio::test(start_paused = true)]
async fn dropped_quiesce_and_run_waits_retain_diagnostic_cleanup_and_later_quiesce_keeps_idle_child()
-> DesktopEngineCommandsTestResult {
    use std::future::poll_fn;
    use std::task::Poll;
    let fixture = DesktopEngineActorDiagnosticsFixture::new().await?;
    fixture.controls.setup.block();
    fixture.controls.cleanup.block();
    let mut actor = DesktopEngineActorDiagnosticsActor::new(fixture)?;
    let (work, mut response) = actor.start()?;
    let mut running = Box::pin(actor.service.run());
    tokio::select! {
        () = actor.fixture.controls.setup.entered.notified() => {}
        result = &mut running => { result?; return Err("actor exited before diagnostic setup".into()); }
    }
    let received = Instant::now();
    let mut first = actor
        .port
        .quiesce(received + std::time::Duration::from_secs(1));
    tokio::select! {
        () = actor.fixture.controls.cleanup.entered.notified() => {}
        result = &mut first => { result?; return Err("quiesce completed before controlled reaping".into()); }
        result = &mut running => { result?; return Err("actor exited before quiesce reached cleanup".into()); }
    }
    assert!(
        poll_fn(|context| Poll::Ready(response.as_mut().poll(context)))
            .await
            .is_pending()
    );
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 2);
    assert_eq!(actor.fixture.ledger.owners.load(Ordering::Acquire), 2);
    assert_eq!(actor.fixture.budget.available(), 1);
    drop(first);
    drop(running);
    actor.fixture.controls.cleanup.release();
    let mut retry = actor
        .port
        .quiesce(received + std::time::Duration::from_secs(2));
    let mut running = Box::pin(actor.service.run());
    tokio::select! {
        biased;
        result = &mut retry => result?,
        result = &mut running => { result?; return Err("actor exited after an abandoned quiesce waiter".into()); }
        () = tokio::time::sleep_until(received + std::time::Duration::from_secs(1)) => return Err("quiesce retry did not observe retained diagnostic cleanup".into()),
    }
    let original = tokio::select! {
        biased;
        result = &mut response => result,
        result = &mut running => { result?; return Err("actor lost interrupted Start response".into()); }
    };
    assert!(original.is_err());
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 1);
    actor.fixture.assert_initial_reservation();
    drop(running);
    assert!(!actor.service.is_shutdown_complete());
    actor.complete(&work, original)?;
    actor.fixture.controls.setup.release();
    Box::pin(actor.close()).await
}

#[tokio::test(start_paused = true)]
async fn ambiguous_configuration_quiesce_reaps_and_requires_explicit_retry_without_resume()
-> DesktopEngineCommandsTestResult {
    let fixture = DesktopEngineActorDiagnosticsFixture::new().await?;
    fixture.controls.configuration.block();
    let mut actor = DesktopEngineActorDiagnosticsActor::new(fixture)?;
    let mut capabilities = chess::engine::capabilities::EngineCapabilities::default();
    for option in chess::engine::enums::EngineEnumOption::iter() {
        capabilities.discover(&format!(
            "option name {} type {}",
            option.name(),
            option.descriptor()
        ))?;
    }
    let setting = EngineOptionsSetting::new(EngineOptionsEnumValue::MultiPv(1), &capabilities)?;
    let (work, mut response) =
        actor.request(DesktopEngineCommandsEnumOperation::Configure(setting))?;
    let mut running = Box::pin(actor.service.run());
    tokio::select! {
        () = actor.fixture.controls.configuration.entered.notified() => {}
        result = &mut running => { result?; return Err("configuration did not reach its pending acknowledgment".into()); }
    }
    let mut quiesce = actor
        .port
        .quiesce(Instant::now() + std::time::Duration::from_secs(1));
    tokio::select! {
        biased;
        result = &mut quiesce => result?,
        result = &mut running => { result?; return Err("quiesce closed the actor".into()); }
    }
    let interrupted = tokio::select! {
        biased;
        result = &mut response => result,
        result = &mut running => { result?; return Err("quiesce lost the configuration response".into()); }
    };
    assert!(interrupted.is_err());
    drop(running);
    actor.complete(&work, interrupted)?;
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 0);
    assert_eq!(actor.fixture.ledger.owners.load(Ordering::Acquire), 0);
    assert_eq!(
        actor.fixture.budget.available(),
        actor.fixture.budget.workers()
    );
    let (work, response) = actor.request(DesktopEngineCommandsEnumOperation::Snapshot)?;
    let snapshot = actor.response(response).await??;
    let DesktopEngineBridgeEnumResult::Snapshot(current) = &snapshot else {
        return Err("missing failed-engine snapshot".into());
    };
    assert_eq!(
        current.state(),
        chess::engine::search::enums::EngineSearchEnumState::Failed
    );
    actor.complete(&work, Ok(snapshot))?;
    assert_eq!(
        actor.fixture.controls.search_writes.load(Ordering::Acquire),
        0
    );
    let (work, response) = actor.request(DesktopEngineCommandsEnumOperation::Retry)?;
    let retried = actor.response(response).await??;
    let DesktopEngineBridgeEnumResult::Snapshot(current) = &retried else {
        return Err("explicit Retry did not return a snapshot".into());
    };
    assert_eq!(
        current.state(),
        chess::engine::search::enums::EngineSearchEnumState::Idle
    );
    assert_ne!(current.generation(), actor.fixture.context.generation());
    actor.complete(&work, Ok(retried))?;
    assert_eq!(
        actor.fixture.controls.search_writes.load(Ordering::Acquire),
        0
    );
    assert_eq!(actor.fixture.process_state.live.load(Ordering::Acquire), 1);
    actor.fixture.assert_initial_reservation();
    Box::pin(actor.close()).await
}
