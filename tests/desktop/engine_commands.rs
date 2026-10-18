use std::num::NonZeroUsize;
use std::sync::Arc;
use strum::IntoEnumIterator;

use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::ClockDuration;
use chess::desktop::engine_commands::{
    DesktopEngineCommands, DesktopEngineCommandsAdmissionProvider, DesktopEngineCommandsAllocation,
    DesktopEngineCommandsContext, DesktopEngineCommandsResourceRequest,
    DesktopEngineCommandsSelectionProvider, DesktopEngineCommandsServices,
};
use chess::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use chess::desktop::identity::DesktopRevision;
use chess::desktop::messages::DesktopMessagesEntityId;
use chess::engine::options::{EngineOptionsEnumValue, EngineOptionsSetting};
use chess::engine::resource_config::EngineResourceConfig;
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
};
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::search::session::{EngineSearchSession, EngineSearchSessionEnumCompletion};
use tokio::time::Instant;
use uuid::Uuid;

use crate::engine_lifecycle::EngineLifecycleProvider;
use crate::engine_package_selection::EnginePackageFixture;

type DesktopEngineCommandsTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[path = "engine_actor_diagnostics.rs"]
mod diagnostics;

pub(super) struct DesktopEngineCommandsTestAdmission(pub(super) CapacityBudget);

impl DesktopEngineCommandsAdmissionProvider for DesktopEngineCommandsTestAdmission {
    type Reservation = ();

    async fn admit(
        &mut self,
        request: DesktopEngineCommandsResourceRequest,
    ) -> Result<DesktopEngineCommandsAllocation<Self::Reservation>, DesktopFailure> {
        let workers = usize::try_from(request.resources().threads()).map_err(|error| {
            DesktopFailure::new(
                DesktopEnumFailureCode::ResourceRefusal,
                error.to_string(),
                false,
            )
        })?;
        let lease = self.0.try_lease(workers).map_err(|error| {
            DesktopFailure::new(
                DesktopEnumFailureCode::ResourceRefusal,
                error.to_string(),
                true,
            )
        })?;
        Ok(DesktopEngineCommandsAllocation::new(lease, ()))
    }

    async fn acknowledge(&mut self, _reservation: &()) -> Result<(), DesktopFailure> {
        Ok(())
    }
}

pub(super) struct DesktopEngineCommandsTestSelection {
    pub(super) context: EngineSearchContext,
    pub(super) position: EngineSearchPosition,
}

impl DesktopEngineCommandsSelectionProvider for DesktopEngineCommandsTestSelection {
    fn authorize_context(&self, context: EngineSearchContext) -> Result<(), DesktopFailure> {
        if context == self.context {
            Ok(())
        } else {
            Err(DesktopFailure::new(
                DesktopEnumFailureCode::StaleRevision,
                "The authoritative selection changed".to_owned(),
                true,
            ))
        }
    }
    fn authorize(
        &self,
        context: EngineSearchContext,
        position: &EngineSearchPosition,
    ) -> Result<(), DesktopFailure> {
        self.authorize_context(context)?;
        if position == &self.position {
            Ok(())
        } else {
            Err(DesktopFailure::new(
                DesktopEnumFailureCode::StaleRevision,
                "The authoritative selection changed".to_owned(),
                true,
            ))
        }
    }
}

struct DesktopEngineCommandsTestFixture {
    provider: EngineLifecycleProvider,
    commands: DesktopEngineCommands<
        EngineLifecycleProvider,
        DesktopEngineCommandsTestAdmission,
        DesktopEngineCommandsTestSelection,
    >,
    budget: CapacityBudget,
    executor: Arc<CapacityExecutor>,
    context: DesktopEngineCommandsContext,
    position: EngineSearchPosition,
}

impl DesktopEngineCommandsTestFixture {
    async fn new() -> DesktopEngineCommandsTestResult<Self> {
        let package = EnginePackageFixture::new()?;
        let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("empty CPU budget")?)?;
        let mut provider = EngineLifecycleProvider::new();
        let mut session = provider.start(&package).await?;
        for line in [
            "info string set Threads to 1",
            "info string set Hash to 32 MB",
            "info string set MoveOverhead to 0 ms",
        ] {
            provider.emit(line).await?;
        }
        session
            .configure_resources(EngineResourceConfig::new(1, 32, ClockDuration::ZERO)?)
            .await?;
        for command in [
            "setoption name Threads value 1",
            "setoption name Hash value 32",
            "setoption name MoveOverhead value 0",
            "isready",
        ] {
            provider.expect(command).await?;
        }
        let position = EngineSearchPosition::new(BoardPosition::standard(), ["e2e4", "e7e5"])?;
        let selection = EngineSearchContext::new(
            EngineSearchGameId::try_from(Uuid::from_u128(1))?,
            EngineSearchRevision::new(3),
        );
        let context = DesktopEngineCommandsContext::new(
            DesktopMessagesEntityId::new(),
            session.generation(),
            DesktopRevision::ZERO,
            selection,
        );
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let commands = DesktopEngineCommands::new(
            EngineSearchSession::new(session),
            DesktopEngineCommandsServices {
                provider: EngineLifecycleProvider::new(),
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
        Ok(Self {
            provider,
            commands,
            budget,
            executor,
            context,
            position,
        })
    }

    async fn close(mut self) -> DesktopEngineCommandsTestResult {
        self.commands
            .shutdown_retained(Instant::now() + std::time::Duration::from_secs(3))
            .await?;
        drop(self.commands);
        Arc::try_unwrap(self.executor)
            .map_err(|_owner| "CPU executor still has an owner")?
            .shutdown()
            .await?;
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn native_start_keeps_history_refuses_stale_context_and_stops_only_its_search()
-> DesktopEngineCommandsTestResult {
    let mut fixture = DesktopEngineCommandsTestFixture::new().await?;
    let original = fixture.position.current().clone();
    let metadata = fixture
        .commands
        .start(
            fixture.context,
            fixture.position.clone(),
            EngineSearchLimit::infinite(),
        )
        .await?;
    fixture
        .provider
        .expect("position startpos moves e2e4 e7e5")
        .await?;
    fixture.provider.expect("go infinite").await?;
    assert_eq!(metadata.position(), &fixture.position);
    assert_eq!(fixture.budget.available(), 1);
    let setting = EngineOptionsSetting::new(
        EngineOptionsEnumValue::ClearHash,
        fixture.commands.snapshot().capabilities(),
    )?;
    assert!(
        fixture
            .commands
            .configure(fixture.context, setting)
            .await
            .is_err()
    );
    fixture.provider.assert_no_command();
    fixture.provider.emit("bestmove g1f3").await?;
    let terminal = fixture
        .commands
        .stop(fixture.context, metadata.tag(), Instant::now())
        .await?;
    fixture.provider.expect("stop").await?;
    assert!(matches!(
        terminal.completion(),
        EngineSearchSessionEnumCompletion::Stopped
    ));
    assert_eq!(fixture.position.current(), &original);
    assert_eq!(fixture.budget.available(), 2);
    let stale = DesktopEngineCommandsContext::new(
        fixture.context.owner(),
        fixture.context.generation(),
        fixture.context.configuration(),
        EngineSearchContext::new(
            fixture.context.selection().game(),
            EngineSearchRevision::new(2),
        ),
    );
    assert!(
        fixture
            .commands
            .start(
                stale,
                fixture.position.clone(),
                EngineSearchLimit::infinite()
            )
            .await
            .is_err()
    );
    fixture.provider.assert_no_command();
    fixture.close().await
}

#[tokio::test(start_paused = true)]
async fn native_idle_configuration_requires_current_version_and_admitted_cpu()
-> DesktopEngineCommandsTestResult {
    let mut fixture = DesktopEngineCommandsTestFixture::new().await?;
    let setting = EngineOptionsSetting::new(
        EngineOptionsEnumValue::ClearHash,
        fixture.commands.snapshot().capabilities(),
    )?;
    let lease = fixture.budget.try_lease(2)?;
    assert!(
        fixture
            .commands
            .configure(fixture.context, setting.clone())
            .await
            .is_err()
    );
    fixture.provider.assert_no_command();
    drop(lease);
    fixture.provider.emit("info string Hash cleared").await?;
    let snapshot = fixture
        .commands
        .configure(fixture.context, setting.clone())
        .await?;
    fixture.provider.expect("setoption name Clear Hash").await?;
    fixture.provider.expect("isready").await?;
    assert_eq!(snapshot.configuration(), DesktopRevision::ZERO.next()?);
    assert!(
        fixture
            .commands
            .configure(fixture.context, setting)
            .await
            .is_err()
    );
    fixture.provider.assert_no_command();
    fixture.close().await
}

struct DesktopEngineCommandsTestClock;

impl chess::clock::time_provider::ClockTimeProvider for DesktopEngineCommandsTestClock {
    fn now(
        &self,
    ) -> Result<chess::clock::duration::ClockInstant, chess::clock::duration::ClockDurationError>
    {
        Ok(chess::clock::duration::ClockInstant::from_nanos(0))
    }
}

struct DesktopEngineCommandsTestWake;

#[derive(Default)]
struct DesktopEngineCommandsTestObserver {
    observed: Vec<(
        chess::workbench::job::WorkbenchJobId,
        chess::workbench::enums::WorkbenchEnumJobState,
    )>,
}

impl chess::desktop::workbench_commands::DesktopWorkbenchCommandsObservationProvider
    for DesktopEngineCommandsTestObserver
{
    fn observe(
        &mut self,
        observation: chess::desktop::workbench_commands::DesktopWorkbenchCommandsObservation<'_>,
    ) -> Result<(), DesktopFailure> {
        self.observed
            .push((observation.job.id(), observation.job.state()));
        Ok(())
    }
}

impl chess::desktop::application::DesktopApplicationWakeProvider for DesktopEngineCommandsTestWake {
    fn wake(&self) -> Result<(), chess::desktop::application::DesktopApplicationWakeError> {
        Ok(())
    }
}

struct DesktopEngineCommandsTestApplication {
    application: chess::desktop::application::DesktopApplication<
        DesktopEngineCommandsTestClock,
        DesktopEngineCommandsTestWake,
    >,
}

impl DesktopEngineCommandsTestApplication {
    fn new() -> Self {
        Self {
            application: chess::desktop::application::DesktopApplication::new(
                Arc::new(DesktopEngineCommandsTestClock),
                Arc::new(DesktopEngineCommandsTestWake),
            ),
        }
    }
    fn work(
        &self,
        context: DesktopEngineCommandsContext,
        action: chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation,
    ) -> DesktopEngineCommandsTestResult<chess::desktop::application::DesktopApplicationWork> {
        let operation =
            chess::desktop::engine_commands::DesktopEngineCommandsOperation::new(context, action);
        let port = self.application.port();
        let request = port.request(chess::desktop::messages::DesktopCommand::new(
            chess::desktop::messages::DesktopEnumCommand::Engine(operation),
            Some(context.configuration()),
        ))?;
        let receipt = port.try_submit(&request)?;
        assert!(receipt.shares_outcome(&port.try_submit(&request)?));
        let work = self
            .application
            .try_next()?
            .ok_or("missing admitted work")?;
        assert_eq!(work.receipt().request_id(), receipt.request_id());
        if !work.request().command().operation().is_control() {
            self.application.begin_commit(&work)?;
        }
        Ok(work)
    }
    fn bridge(
        work: &chess::desktop::application::DesktopApplicationWork,
    ) -> DesktopEngineCommandsTestResult<chess::desktop::engine_bridge::DesktopEngineBridgeRequest>
    {
        let chess::desktop::messages::DesktopEnumCommand::Engine(operation) =
            work.request().command().operation()
        else {
            return Err("missing engine operation".into());
        };
        Ok(chess::desktop::engine_bridge::DesktopEngineBridgeRequest {
            operation: operation.clone(),
            receipt: work.receipt().clone(),
            selection: None,
        })
    }
}

#[tokio::test(start_paused = true)]
async fn native_service_dispatches_real_search_and_reserved_stop_after_ordinary_saturation()
-> DesktopEngineCommandsTestResult {
    use chess::desktop::engine_bridge::{
        DesktopEngineBridgeEnumResult, DesktopEngineBridgeProvider,
    };
    use chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation;
    let fixture = DesktopEngineCommandsTestFixture::new().await?;
    let mut provider = fixture.provider;
    let application = DesktopEngineCommandsTestApplication::new();
    let directory = tempfile::tempdir()?;
    let (mut service, port, mut streams) =
        chess::desktop::engine_service::DesktopEngineService::new(
            fixture.commands,
            Arc::new(DesktopEngineCommandsTestClock),
            chess::desktop::engine_tablebases::DesktopEngineTablebasesPolicy::new(
                directory.path().to_path_buf(),
            ),
            DesktopEngineCommandsTestObserver::default(),
        );
    let work = application.work(
        fixture.context,
        DesktopEngineCommandsEnumOperation::Start {
            position: fixture.position,
            limit: EngineSearchLimit::infinite(),
        },
    )?;
    let response = port.execute(DesktopEngineCommandsTestApplication::bridge(&work)?);
    let started = tokio::select! {
        biased;
        response = response => response?,
        result = service.run() => { result?; return Err("service exited before start response".into()); }
    };
    let DesktopEngineBridgeEnumResult::Started(metadata) = &started else {
        return Err("missing actual search metadata".into());
    };
    let tag = metadata.tag();
    provider.expect("position startpos moves e2e4 e7e5").await?;
    provider.expect("go infinite").await?;
    application.application.complete_value(
        &work,
        chess::desktop::dto::DesktopEnumValue::Engine(Box::new(started.clone())),
        None,
    )?;
    let mut ordinary = Vec::new();
    for _index in 0..chess::desktop::engine_bridge::DesktopEngineBridgePort::ORDINARY_CAPACITY {
        let work = application.work(
            fixture.context,
            DesktopEngineCommandsEnumOperation::Snapshot,
        )?;
        ordinary.push((
            port.execute(DesktopEngineCommandsTestApplication::bridge(&work)?),
            work,
        ));
    }
    let stop = application.work(
        fixture.context,
        DesktopEngineCommandsEnumOperation::Stop { tag },
    )?;
    let received = stop.receipt().runtime_received_at();
    tokio::time::advance(std::time::Duration::from_millis(100)).await;
    assert_eq!(received, stop.receipt().runtime_received_at());
    provider.emit("bestmove g1f3").await?;
    let response = port.execute(DesktopEngineCommandsTestApplication::bridge(&stop)?);
    let stopped = tokio::select! {
        biased;
        response = response => response?,
        result = service.run() => { result?; return Err("service exited before reserved stop".into()); }
    };
    let DesktopEngineBridgeEnumResult::Stopped(terminal) = &stopped else {
        return Err("missing terminal stop result".into());
    };
    assert_eq!(terminal.snapshot().metadata().tag(), tag);
    application
        .application
        .complete_engine_control(&stop, Ok(stopped.clone()))?;
    provider.expect("stop").await?;
    drop(port);
    service.run().await?;
    let event = streams
        .terminal
        .recv()
        .await
        .ok_or("missing retained terminal event")?;
    assert!(
        matches!(event, chess::desktop::engine_bridge::DesktopEngineBridgeEnumEvent::Analysis(event) if matches!(event.as_ref(), chess::engine::search::session::EngineSearchSessionEnumEvent::Finished(terminal) if terminal.snapshot().metadata().tag() == tag))
    );
    for (response, work) in ordinary {
        let value = response.await?;
        application.application.complete_value(
            &work,
            chess::desktop::dto::DesktopEnumValue::Engine(Box::new(value)),
            None,
        )?;
    }
    service
        .shutdown_retained(Instant::now() + std::time::Duration::from_secs(3))
        .await?;
    assert!(service.is_shutdown_complete());
    assert_eq!(fixture.budget.available(), 2);
    drop(service);
    Arc::try_unwrap(fixture.executor)
        .map_err(|_owner| "CPU executor still has an owner")?
        .shutdown()
        .await?;
    Ok(())
}

#[tokio::test]
async fn native_selected_directory_load_requires_engine_outcome_and_retains_failed_cleanup()
-> DesktopEngineCommandsTestResult {
    use chess::clock::duration::ClockInstant;
    use chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation;
    use chess::desktop::file_handle::{
        DesktopFileEnumPurpose, DesktopFileHandleOwner, DesktopFileHandleRegistry,
        DesktopFileHandleRequest,
    };
    use chess::engine::tablebase::status::EngineTablebaseStatusEnumState;
    let mut fixture = DesktopEngineCommandsTestFixture::new().await?;
    let application = DesktopEngineCommandsTestApplication::new();
    let storage = tempfile::tempdir()?;
    let tables = storage.path().join("selected tables with spaces");
    std::fs::create_dir(&tables)?;
    std::fs::write(tables.join("KPvK.rtbw"), [1])?;
    let mut registry = DesktopFileHandleRegistry::new();
    let owner = fixture.context.owner().file_handle_owner();
    let handle = registry
        .register_selection(
            tables,
            DesktopFileHandleRequest::new(
                owner,
                DesktopFileEnumPurpose::Directory,
                ClockInstant::from_nanos(0),
            ),
            &fixture.executor,
        )
        .await?;
    assert!(
        registry
            .consume(
                handle,
                DesktopFileHandleOwner::new(),
                DesktopFileEnumPurpose::Directory,
                ClockInstant::from_nanos(0)
            )
            .is_err()
    );
    assert!(
        registry
            .consume(
                handle,
                owner,
                DesktopFileEnumPurpose::ReadFile,
                ClockInstant::from_nanos(0)
            )
            .is_err()
    );
    let selected = registry.consume(
        handle,
        owner,
        DesktopFileEnumPurpose::Directory,
        ClockInstant::from_nanos(0),
    )?;
    let work = application.work(
        fixture.context,
        DesktopEngineCommandsEnumOperation::PrepareTablebases { directory: handle },
    )?;
    let policy = chess::desktop::engine_tablebases::DesktopEngineTablebasesPolicy::new(
        storage.path().to_path_buf(),
    );
    let prepared = fixture
        .commands
        .prepare_tablebases(
            fixture.context,
            selected,
            chess::desktop::engine_tablebases::DesktopEngineTablebasesPolicyProvider::policy(
                &policy,
            )?,
            work.receipt().clone(),
        )
        .await?;
    let status = prepared.tablebases().ok_or("missing prepared status")?;
    assert!(matches!(
        status.state(),
        EngineTablebaseStatusEnumState::Configured
    ));
    let alias = std::path::PathBuf::from(status.engine_path().as_str());
    assert!(alias.join("KPvK.rtbw").exists());
    fixture
        .provider
        .emit("info string Loaded Syzygy tablebases with 3 pieces")
        .await?;
    let loaded = fixture.commands.load_tablebases(fixture.context).await?;
    fixture
        .provider
        .expect(&format!(
            "setoption name SyzygyPath value {}",
            alias.display()
        ))
        .await?;
    fixture.provider.expect("isready").await?;
    assert!(
        matches!(loaded.tablebases().ok_or("missing actual load status")?.state(), EngineTablebaseStatusEnumState::Loaded { pieces } if pieces.get() == 3)
    );
    let moved = alias.with_extension("retained");
    std::fs::rename(&alias, &moved)?;
    std::fs::create_dir(&alias)?;
    assert!(
        fixture
            .commands
            .shutdown_retained(Instant::now() + std::time::Duration::from_secs(3))
            .await
            .is_err()
    );
    assert!(!fixture.commands.is_shutdown_complete());
    assert!(moved.join("KPvK.rtbw").exists());
    std::fs::remove_dir(&alias)?;
    std::fs::rename(&moved, &alias)?;
    fixture
        .commands
        .shutdown_retained(Instant::now() + std::time::Duration::from_secs(3))
        .await?;
    assert!(!alias.exists());
    fixture.close().await
}

#[tokio::test]
async fn cancelled_native_directory_preparation_creates_no_aliases()
-> DesktopEngineCommandsTestResult {
    use chess::clock::duration::ClockInstant;
    use chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation;
    use chess::desktop::file_handle::{
        DesktopFileEnumPurpose, DesktopFileHandleRegistry, DesktopFileHandleRequest,
    };
    let mut fixture = DesktopEngineCommandsTestFixture::new().await?;
    let application = DesktopEngineCommandsTestApplication::new();
    let storage = tempfile::tempdir()?;
    let sources = storage.path().join("source");
    std::fs::create_dir(&sources)?;
    std::fs::write(sources.join("KPvK.rtbw"), [1])?;
    let mut registry = DesktopFileHandleRegistry::new();
    let owner = fixture.context.owner().file_handle_owner();
    let handle = registry
        .register_selection(
            sources,
            DesktopFileHandleRequest::new(
                owner,
                DesktopFileEnumPurpose::Directory,
                ClockInstant::from_nanos(0),
            ),
            &fixture.executor,
        )
        .await?;
    let selected = registry.consume(
        handle,
        owner,
        DesktopFileEnumPurpose::Directory,
        ClockInstant::from_nanos(0),
    )?;
    let work = application.work(
        fixture.context,
        DesktopEngineCommandsEnumOperation::PrepareTablebases { directory: handle },
    )?;
    let port = application.application.port();
    let stop = port.request(chess::desktop::messages::DesktopCommand::new(
        chess::desktop::messages::DesktopEnumCommand::Stop {
            owner: fixture.context.owner(),
        },
        None,
    ))?;
    let receipt = port.try_submit(&stop)?;
    assert!(work.receipt().is_cancellation_requested());
    let policy = chess::desktop::engine_tablebases::DesktopEngineTablebasesPolicy::new(
        storage.path().to_path_buf(),
    );
    assert!(
        fixture
            .commands
            .prepare_tablebases(
                fixture.context,
                selected,
                chess::desktop::engine_tablebases::DesktopEngineTablebasesPolicyProvider::policy(
                    &policy
                )?,
                work.receipt().clone()
            )
            .await
            .is_err()
    );
    assert!(fixture.commands.snapshot().tablebases().is_none());
    assert_eq!(std::fs::read_dir(storage.path())?.count(), 1);
    assert!(receipt.response().is_none());
    fixture.close().await
}

type DesktopEngineCommandsTestService = chess::desktop::engine_service::DesktopEngineService<
    EngineLifecycleProvider,
    DesktopEngineCommandsTestAdmission,
    DesktopEngineCommandsTestSelection,
    DesktopEngineCommandsTestClock,
    chess::desktop::engine_tablebases::DesktopEngineTablebasesPolicy,
    DesktopEngineCommandsTestObserver,
>;

struct DesktopEngineCommandsTestActor {
    service: DesktopEngineCommandsTestService,
    port: chess::desktop::engine_bridge::DesktopEngineBridgePort,
    streams: chess::desktop::engine_bridge::DesktopEngineBridgeStreams,
    fixture: DesktopEngineCommandsTestActorFixture,
}

struct DesktopEngineCommandsTestActorFixture {
    provider: EngineLifecycleProvider,
    executor: Arc<CapacityExecutor>,
    budget: CapacityBudget,
    context: DesktopEngineCommandsContext,
    position: EngineSearchPosition,
    application: DesktopEngineCommandsTestApplication,
    directory: tempfile::TempDir,
}

impl DesktopEngineCommandsTestActor {
    async fn new() -> DesktopEngineCommandsTestResult<Self> {
        let fixture = DesktopEngineCommandsTestFixture::new().await?;
        let directory = tempfile::tempdir()?;
        let (service, port, streams) = chess::desktop::engine_service::DesktopEngineService::new(
            fixture.commands,
            Arc::new(DesktopEngineCommandsTestClock),
            chess::desktop::engine_tablebases::DesktopEngineTablebasesPolicy::new(
                directory.path().to_path_buf(),
            ),
            DesktopEngineCommandsTestObserver::default(),
        );
        Ok(Self {
            service,
            port,
            streams,
            fixture: DesktopEngineCommandsTestActorFixture {
                provider: fixture.provider,
                executor: fixture.executor,
                budget: fixture.budget,
                context: fixture.context,
                position: fixture.position,
                application: DesktopEngineCommandsTestApplication::new(),
                directory,
            },
        })
    }

    fn request(
        &self,
        action: chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation,
    ) -> DesktopEngineCommandsTestResult<(
        chess::desktop::application::DesktopApplicationWork,
        chess::desktop::engine_bridge::DesktopEngineBridgeFuture,
    )> {
        use chess::desktop::engine_bridge::DesktopEngineBridgeProvider;
        let work = self
            .fixture
            .application
            .work(self.fixture.context, action)?;
        let response = self
            .port
            .execute(DesktopEngineCommandsTestApplication::bridge(&work)?);
        Ok((work, response))
    }

    async fn response(
        &mut self,
        mut response: chess::desktop::engine_bridge::DesktopEngineBridgeFuture,
    ) -> DesktopEngineCommandsTestResult<chess::desktop::engine_bridge::DesktopEngineBridgeEnumResult>
    {
        tokio::select! {
            biased;
            response = &mut response => Ok(response?),
            result = self.service.run() => { result?; Ok(response.await?) }
        }
    }

    fn complete(
        &self,
        work: &chess::desktop::application::DesktopApplicationWork,
        response: chess::desktop::engine_bridge::DesktopEngineBridgeEnumResult,
    ) -> DesktopEngineCommandsTestResult {
        if work.request().command().operation().is_control() {
            self.fixture
                .application
                .application
                .complete_engine_control(work, Ok(response))?;
        } else {
            self.fixture.application.application.complete_value(
                work,
                chess::desktop::dto::DesktopEnumValue::Engine(Box::new(response)),
                None,
            )?;
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> DesktopEngineCommandsTestResult {
        let mut response = self
            .port
            .shutdown(Instant::now() + std::time::Duration::from_secs(30));
        tokio::select! {
            biased;
            response = &mut response => response?,
            result = self.service.run() => { result?; response.await?; }
        }
        Ok(())
    }

    async fn close(mut self) -> DesktopEngineCommandsTestResult {
        if !self.service.is_shutdown_complete() {
            self.shutdown().await?;
        }
        assert!(self.service.is_shutdown_complete());
        assert!(self.fixture.directory.path().exists());
        assert_eq!(
            self.fixture.budget.available(),
            self.fixture.budget.workers()
        );
        drop(self.service);
        Arc::try_unwrap(self.fixture.executor)
            .map_err(|_owner| "retained CPU executor owner")?
            .shutdown()
            .await?;
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn native_actor_shutdown_preempts_pending_start_without_releasing_its_stalled_write()
-> DesktopEngineCommandsTestResult {
    use chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation;
    use std::future::poll_fn;
    use std::task::Poll;
    let mut actor = DesktopEngineCommandsTestActor::new().await?;
    actor.fixture.provider.set_position_blocked(true);
    let (_work, mut response) = actor.request(DesktopEngineCommandsEnumOperation::Start {
        position: actor.fixture.position.clone(),
        limit: EngineSearchLimit::infinite(),
    })?;
    let mut running = Box::pin(actor.service.run());
    tokio::select! {
        result = actor.fixture.provider.expect("position startpos moves e2e4 e7e5") => result?,
        result = &mut running => { result?; return Err("actor exited during stalled Start".into()); }
    }
    let receipt = Instant::now();
    let mut shutdown = actor
        .port
        .shutdown(receipt + std::time::Duration::from_secs(30));
    tokio::select! {
        biased;
        result = &mut shutdown => result?,
        result = &mut running => { result?; shutdown.await?; }
    }
    assert_eq!(
        Instant::now(),
        receipt,
        "shutdown waited for the stalled Start handshake timeout"
    );
    assert!(
        poll_fn(|context| Poll::Ready(response.as_mut().poll(context)))
            .await
            .is_ready()
    );
    drop(running);
    actor.close().await
}

#[tokio::test(start_paused = true)]
async fn native_actor_shutdown_preempts_pending_configuration_without_an_option_ack()
-> DesktopEngineCommandsTestResult {
    use chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation;
    let mut actor = DesktopEngineCommandsTestActor::new().await?;
    let snapshot = actor.fixture.context;
    let mut capabilities = chess::engine::capabilities::EngineCapabilities::default();
    for option in chess::engine::enums::EngineEnumOption::iter() {
        capabilities.discover(&format!(
            "option name {} type {}",
            option.name(),
            option.descriptor()
        ))?;
    }
    let setting = EngineOptionsSetting::new(EngineOptionsEnumValue::ClearHash, &capabilities)?;
    let (_work, response) =
        actor.request(DesktopEngineCommandsEnumOperation::Configure(setting))?;
    let mut running = Box::pin(actor.service.run());
    tokio::select! {
        result = actor.fixture.provider.expect("setoption name Clear Hash") => result?,
        result = &mut running => { result?; return Err("actor exited during stalled configuration".into()); }
    }
    let receipt = Instant::now();
    let mut shutdown = actor
        .port
        .shutdown(receipt + std::time::Duration::from_secs(30));
    tokio::select! {
        biased;
        result = &mut shutdown => result?,
        result = &mut running => { result?; shutdown.await?; }
    }
    assert_eq!(
        Instant::now(),
        receipt,
        "shutdown waited for option readiness or timeout"
    );
    assert!(response.await.is_err());
    assert_eq!(snapshot.configuration(), DesktopRevision::ZERO);
    drop(running);
    actor.close().await
}

#[tokio::test(start_paused = true)]
async fn native_actor_abandoned_execute_and_shutdown_receivers_do_not_prevent_cleanup()
-> DesktopEngineCommandsTestResult {
    use chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation;
    use std::future::poll_fn;
    use std::task::Poll;
    let mut actor = DesktopEngineCommandsTestActor::new().await?;
    let (_work, abandoned) = actor.request(DesktopEngineCommandsEnumOperation::Snapshot)?;
    drop(abandoned);
    let abandoned_shutdown = actor
        .port
        .shutdown(Instant::now() + std::time::Duration::from_secs(30));
    drop(abandoned_shutdown);
    let result = actor.service.run().await;
    assert!(
        result.is_ok(),
        "abandoned receipt terminated the service before cleanup: {result:?}"
    );
    assert!(actor.service.is_shutdown_complete());
    let (_work, mut rejected) = actor.request(DesktopEngineCommandsEnumOperation::Snapshot)?;
    assert!(matches!(
        poll_fn(|context| Poll::Ready(rejected.as_mut().poll(context))).await,
        Poll::Ready(Err(_))
    ));
    actor.close().await
}

#[tokio::test(start_paused = true)]
async fn native_actor_shutdown_publishes_the_active_analysis_terminal_exactly_once()
-> DesktopEngineCommandsTestResult {
    use chess::desktop::engine_bridge::{
        DesktopEngineBridgeEnumEvent, DesktopEngineBridgeEnumResult,
    };
    use chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation;
    let mut actor = DesktopEngineCommandsTestActor::new().await?;
    let (work, response) = actor.request(DesktopEngineCommandsEnumOperation::Start {
        position: actor.fixture.position.clone(),
        limit: EngineSearchLimit::infinite(),
    })?;
    let started = actor.response(response).await?;
    let DesktopEngineBridgeEnumResult::Started(metadata) = &started else {
        return Err("missing Started metadata".into());
    };
    let tag = metadata.tag();
    actor.complete(&work, started)?;
    actor.shutdown().await?;
    let event = actor.streams.terminal.try_recv()?;
    assert!(
        matches!(event, DesktopEngineBridgeEnumEvent::Analysis(event) if matches!(event.as_ref(), chess::engine::search::session::EngineSearchSessionEnumEvent::Finished(terminal) if terminal.snapshot().metadata().tag() == tag && matches!(terminal.completion(), EngineSearchSessionEnumCompletion::Cancelled)))
    );
    assert!(matches!(
        actor.streams.terminal.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    actor.close().await
}

#[tokio::test(start_paused = true)]
async fn native_actor_dropped_run_retains_the_pending_request_until_owned_cleanup()
-> DesktopEngineCommandsTestResult {
    use chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation;
    use std::future::poll_fn;
    use std::task::Poll;
    let mut actor = DesktopEngineCommandsTestActor::new().await?;
    actor.fixture.provider.set_position_blocked(true);
    let (work, mut response) = actor.request(DesktopEngineCommandsEnumOperation::Start {
        position: actor.fixture.position.clone(),
        limit: EngineSearchLimit::infinite(),
    })?;
    let mut running = Box::pin(actor.service.run());
    tokio::select! {
        result = actor.fixture.provider.expect("position startpos moves e2e4 e7e5") => result?,
        result = &mut running => { result?; return Err("actor exited before its setup stalled".into()); }
    }
    drop(running);
    assert!(
        poll_fn(|context| Poll::Ready(response.as_mut().poll(context)))
            .await
            .is_pending(),
        "dropping run lost the original response owner"
    );
    assert_eq!(actor.fixture.budget.available(), 1);
    actor
        .service
        .shutdown_retained(Instant::now() + std::time::Duration::from_secs(30))
        .await?;
    assert!(response.await.is_err());
    assert!(work.receipt().response().is_none());
    actor.close().await
}

#[tokio::test(start_paused = true)]
async fn native_actor_failed_cleanup_keeps_interrupted_response_pending_until_reaping()
-> DesktopEngineCommandsTestResult {
    use chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation;
    use std::future::poll_fn;
    use std::task::Poll;
    let mut actor = DesktopEngineCommandsTestActor::new().await?;
    actor.fixture.provider.set_position_blocked(true);
    let (work, mut response) = actor.request(DesktopEngineCommandsEnumOperation::Start {
        position: actor.fixture.position.clone(),
        limit: EngineSearchLimit::infinite(),
    })?;
    let mut running = Box::pin(actor.service.run());
    tokio::select! {
        result = actor.fixture.provider.expect("position startpos moves e2e4 e7e5") => result?,
        result = &mut running => { result?; return Err("actor exited before setup stalled".into()); }
    }
    actor.fixture.provider.set_cleanup_allowed(false);
    let mut shutdown = actor.port.shutdown(Instant::now());
    let result = tokio::select! {
        biased;
        result = &mut shutdown => result,
        result = &mut running => { result?; shutdown.await }
    };
    assert!(result.is_err());
    assert!(
        poll_fn(|context| Poll::Ready(response.as_mut().poll(context)))
            .await
            .is_pending()
    );
    drop(running);
    assert!(!actor.service.is_shutdown_complete());
    assert_eq!(actor.fixture.budget.available(), 1);
    actor.fixture.provider.set_cleanup_allowed(true);
    let cleanup = actor.shutdown().await;
    assert!(cleanup.is_ok() || actor.service.is_shutdown_complete());
    assert!(response.await.is_err());
    assert!(work.receipt().response().is_none());
    actor.close().await
}

#[tokio::test(start_paused = true)]
async fn native_actor_shutdown_retains_one_terminal_under_full_delivery_and_retry()
-> DesktopEngineCommandsTestResult {
    use chess::desktop::engine_bridge::{
        DesktopEngineBridgeEnumEvent, DesktopEngineBridgeEnumResult,
    };
    use chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation;
    let mut actor = DesktopEngineCommandsTestActor::new().await?;
    for _index in 0..chess::desktop::engine_bridge::DesktopEngineBridgePort::EVENT_CAPACITY {
        let (work, response) = actor.request(DesktopEngineCommandsEnumOperation::Start {
            position: actor.fixture.position.clone(),
            limit: EngineSearchLimit::infinite(),
        })?;
        let started = actor.response(response).await?;
        let DesktopEngineBridgeEnumResult::Started(metadata) = &started else {
            return Err("missing Started metadata".into());
        };
        let tag = metadata.tag();
        actor.complete(&work, started)?;
        actor
            .fixture
            .provider
            .expect("position startpos moves e2e4 e7e5")
            .await?;
        actor.fixture.provider.expect("go infinite").await?;
        actor.fixture.provider.emit("bestmove g1f3").await?;
        let (work, response) = actor.request(DesktopEngineCommandsEnumOperation::Stop { tag })?;
        let stopped = actor.response(response).await?;
        actor.complete(&work, stopped)?;
        actor.fixture.provider.expect("stop").await?;
    }
    let (work, response) = actor.request(DesktopEngineCommandsEnumOperation::Start {
        position: actor.fixture.position.clone(),
        limit: EngineSearchLimit::infinite(),
    })?;
    let started = actor.response(response).await?;
    let DesktopEngineBridgeEnumResult::Started(metadata) = &started else {
        return Err("missing active search metadata".into());
    };
    let tag = metadata.tag();
    actor.complete(&work, started)?;
    actor
        .fixture
        .provider
        .expect("position startpos moves e2e4 e7e5")
        .await?;
    actor.fixture.provider.expect("go infinite").await?;
    let shutdown = tokio::time::timeout(std::time::Duration::from_secs(1), actor.shutdown())
        .await
        .map_err(
            |_elapsed| "shutdown stalled instead of reporting retained terminal backpressure",
        )?;
    assert!(shutdown.is_err());
    assert!(!actor.service.is_shutdown_complete());
    for _index in 0..chess::desktop::engine_bridge::DesktopEngineBridgePort::EVENT_CAPACITY {
        actor.streams.terminal.try_recv()?;
    }
    tokio::time::timeout(std::time::Duration::from_secs(1), actor.shutdown()).await
        .map_err(|_elapsed| "shutdown retry abandoned its queued lifecycle receipt after terminal delivery became writable")??;
    let event = actor.streams.terminal.try_recv()?;
    assert!(
        matches!(event, DesktopEngineBridgeEnumEvent::Analysis(event) if matches!(event.as_ref(), chess::engine::search::session::EngineSearchSessionEnumEvent::Finished(terminal) if terminal.snapshot().metadata().tag() == tag && matches!(terminal.completion(), EngineSearchSessionEnumCompletion::Cancelled)))
    );
    assert!(matches!(
        actor.streams.terminal.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    actor.close().await
}
