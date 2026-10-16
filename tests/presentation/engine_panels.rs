use chess::board::start::BoardStart;
use chess::clock::duration::ClockInstant;
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::game_snapshot::DesktopGameSnapshot;
use chess::game::enums::{GameEnumMode, GameEnumSeat};
use chess::game::session::{GameMetadata, GameSeats, GameSession, GameSessionOptions};
use chess::presentation::engine_panels::PresentationEnginePanels;

type PresentationEnginePanelsTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PresentationEnginePanelsFixture;

impl PresentationEnginePanelsFixture {
    fn branched_session() -> PresentationEnginePanelsTestResult<GameSession> {
        let mut session = Self::session()?;
        session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
        let movement = session
            .tree()
            .node(session.view())?
            .position()
            .parse_uci("e2e4")?;
        session.append_study_variation(
            session.revision(),
            "123e4567-e89b-42d3-a456-426614174002".parse()?,
            movement,
        )?;
        Ok(session)
    }

    fn session() -> PresentationEnginePanelsTestResult<GameSession> {
        let start = BoardStart::standard();
        Ok(GameSession::new(GameSessionOptions {
            id: "123e4567-e89b-42d3-a456-426614174000".parse()?,
            root: "123e4567-e89b-42d3-a456-426614174001".parse()?,
            clock: ClockState::new(ClockTimeControl::untimed(), start.position().turn()),
            start,
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            metadata: GameMetadata::default(),
        })?)
    }
}

#[test]
fn authoritative_analysis_selection_keeps_original_history_and_view()
-> PresentationEnginePanelsTestResult {
    let mut session = PresentationEnginePanelsFixture::session()?;
    session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
    let root = session.view();
    for (node, uci) in [
        ("123e4567-e89b-42d3-a456-426614174002", "e2e4"),
        ("123e4567-e89b-42d3-a456-426614174003", "e7e5"),
    ] {
        let movement = session
            .tree()
            .node(session.view())?
            .position()
            .parse_uci(uci)?;
        session.append_study_variation(session.revision(), node.parse()?, movement)?;
    }
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let selection =
        PresentationEnginePanels::selection_from(&snapshot)?.ok_or("missing analysis selection")?;
    assert_eq!(
        selection.position().command(),
        "position startpos moves e2e4 e7e5"
    );
    assert_eq!(selection.context().game().value(), session.id().as_uuid());
    session.select_view(root)?;
    let root_snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let root_selection = PresentationEnginePanels::selection_from(&root_snapshot)?
        .ok_or("missing root selection")?;
    assert_eq!(root_selection.position().command(), "position startpos");
    assert_eq!(root_selection.context(), selection.context());
    Ok(())
}

#[test]
fn setup_and_live_game_history_never_enable_engine_assistance() -> PresentationEnginePanelsTestResult
{
    let session = PresentationEnginePanelsFixture::session()?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    assert!(PresentationEnginePanels::selection_from(&snapshot)?.is_none());
    let active = crate::game_transition_fixture::GameTransitionFixture::session(
        ClockTimeControl::untimed(),
        None,
    )?;
    assert_eq!(active.phase(), chess::game::enums::GameEnumPhase::Active);
    let active_snapshot = DesktopGameSnapshot::capture(&active, ClockInstant::from_nanos(0))?;
    assert!(PresentationEnginePanels::selection_from(&active_snapshot)?.is_none());
    Ok(())
}

use std::collections::VecDeque;
use std::convert::Infallible;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bevy_egui::egui;
use egui_kittest::{Harness, kittest::Queryable};
use tokio::sync::watch;
use tokio::time::Instant;

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::time_provider::ClockTimeProviderMonotonic;
use chess::desktop::application::{DesktopApplication, DesktopApplicationPort};
use chess::desktop::dto::DesktopEnumValue;
use chess::desktop::engine_bridge::{
    DesktopEngineBridgeEnumResult, DesktopEngineBridgePort, DesktopEngineBridgeProvider,
    DesktopEngineBridgeRequest,
};
use chess::desktop::engine_commands::DesktopEngineCommandsEnumOperation;
use chess::desktop::file_handle::{
    DesktopFileEnumPurpose, DesktopFileHandleRegistry, DesktopFileHandleRequest,
};
use chess::desktop::messages::DesktopEnumCommand;
use chess::desktop::native::engine_runtime::{
    DesktopNativeEngineRuntime, DesktopNativeEngineRuntimeOptions,
};
use chess::desktop::native::event_loop::DesktopNativeEventLoopWake;
use chess::desktop::native::game_resources::{
    DesktopNativeGameResources, DesktopNativeGameResourcesMemoryProvider,
    DesktopNativeGameResourcesOptions, DesktopNativeGameResourcesPolicy,
};
use chess::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation;
use chess::engine::capabilities::EngineIdentity;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::host::EnginePackageHost;
use chess::engine::package::manifest::{
    EnginePackageEntry, EnginePackageEnumTarget, EnginePackageManifest,
};
use chess::engine::package::selector::{EnginePackageSelected, EnginePackageSelector};
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
    EngineProcessProvider,
};
use chess::engine::search::session::EngineSearchSessionEnumCompletion;
use chess::package::digest::PackageDigest;
use chess::presentation::engine_panels::PresentationEnginePanelsConnection;
use chess::presentation::theme::font::PresentationThemeFont;

struct PresentationEnginePanelsMemory;

impl DesktopNativeGameResourcesMemoryProvider for PresentationEnginePanelsMemory {
    type Error = Infallible;
    async fn available_bytes(&self) -> Result<u64, Infallible> {
        Ok(8 * 1024 * 1024 * 1024)
    }
}

struct PresentationEnginePanelsProcess {
    replies: VecDeque<EngineProcessLine>,
    commands: Arc<Mutex<Vec<String>>>,
    closed: bool,
    black_to_move: bool,
    stalled: bool,
    fail_configuration: bool,
    changed: Arc<tokio::sync::Notify>,
}

struct PresentationEnginePanelsProvider {
    commands: Arc<Mutex<Vec<String>>>,
    spawns: AtomicUsize,
    stall_initial: bool,
    stall_diagnostics: bool,
    fail_configuration: bool,
    changed: Arc<tokio::sync::Notify>,
}

impl EngineProcessProvider for PresentationEnginePanelsProvider {
    type Process = PresentationEnginePanelsProcess;
    fn spawn(&self, _package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        Ok(PresentationEnginePanelsProcess {
            replies: VecDeque::new(),
            commands: Arc::clone(&self.commands),
            closed: false,
            black_to_move: false,
            fail_configuration: self.fail_configuration,
            stalled: if self.spawns.fetch_add(1, Ordering::SeqCst) == 0 {
                self.stall_initial
            } else {
                self.stall_diagnostics
            },
            changed: Arc::clone(&self.changed),
        })
    }
}

impl PresentationEnginePanelsProcess {
    fn reply(&mut self, text: impl Into<String>) -> Result<(), EngineProcessError> {
        self.replies.push_back(EngineProcessLine::new(
            EngineProcessEnumPipe::Stdout,
            text.into(),
        )?);
        Ok(())
    }
}

impl EngineProcess for PresentationEnginePanelsProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        self.commands
            .lock()
            .map_err(|error| std::io::Error::other(error.to_string()))?
            .push(command.to_owned());
        self.changed.notify_one();
        if self.fail_configuration && command.starts_with("setoption name Threads value ") {
            return Err(EngineProcessError::InvalidCommand);
        }
        if command == "uci" {
            if self.stalled {
                return Ok(());
            }
            self.reply("id name Reckless 0.9.0")?;
            self.reply(format!("id author {}", EngineIdentity::AUTHOR))?;
            for option in EngineEnumOption::ALL {
                self.reply(format!(
                    "option name {} type {}",
                    option.name(),
                    option.descriptor()
                ))?;
            }
            self.reply("uciok")?;
        } else if command == "isready" {
            self.reply("readyok")?;
        } else if let Some(option) = command.strip_prefix("setoption name ") {
            if option == "Clear Hash" {
                self.reply("info string Hash cleared")?;
            } else if let Some((name, value)) = option.split_once(" value ") {
                let unit = if name == EngineEnumOption::Hash.name() {
                    " MB"
                } else if name == EngineEnumOption::MoveOverhead.name() {
                    " ms"
                } else {
                    ""
                };
                self.reply(format!("info string set {name} to {value}{unit}"))?;
            }
        } else if command.starts_with("position ") {
            self.black_to_move = command.ends_with("moves e2e4");
        } else if command == "stop" {
            self.reply(if self.black_to_move {
                "bestmove e7e5"
            } else {
                "bestmove e2e4"
            })?;
        } else if command.starts_with("go ") {
            self.reply(if self.black_to_move {
                "info depth 7 score cp 21 nodes 24 pv e7e5 g1f3"
            } else {
                "info depth 7 score cp 21 nodes 24 pv e2e4 e7e5"
            })?;
        } else if command == "compiler" {
            self.reply("Compiler Version: fixture compiler")?;
            self.reply("Compiler Target: aarch64-apple-darwin")?;
            self.reply("Compiler Features: neon")?;
        }
        Ok(())
    }
    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        if let Some(line) = self.replies.pop_front() {
            Ok(line)
        } else {
            std::future::pending().await
        }
    }
    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.closed = true;
        Ok(())
    }
    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }
    async fn terminate(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }
    fn is_shutdown_complete(&self) -> bool {
        self.closed
    }
}

struct PresentationEnginePanelsComposition {
    runtime: DesktopNativeEngineRuntime<
        PresentationEnginePanelsProvider,
        PresentationEnginePanelsMemory,
    >,
    bridge: Option<DesktopEngineBridgePort>,
    application: DesktopApplication<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
    budget: CapacityBudget,
    executor: Arc<CapacityExecutor>,
    commands: Arc<Mutex<Vec<String>>>,
    resources: Arc<DesktopNativeGameResources<PresentationEnginePanelsMemory>>,
    changed: Arc<tokio::sync::Notify>,
    snapshots: watch::Sender<Vec<DesktopGameSnapshot>>,
    files: tokio::sync::Mutex<Option<DesktopFileHandleRegistry>>,
    previewed: tokio::sync::Mutex<Option<Vec<u8>>>,
    wake: Arc<DesktopNativeEventLoopWake>,
    directory: tempfile::TempDir,
}

impl PresentationEnginePanelsComposition {
    async fn new(
        snapshot: DesktopGameSnapshot,
    ) -> PresentationEnginePanelsTestResult<(Self, PresentationEnginePanelsConnection)> {
        Self::build(snapshot, false).await
    }

    async fn build(
        snapshot: DesktopGameSnapshot,
        stall_diagnostics: bool,
    ) -> PresentationEnginePanelsTestResult<(Self, PresentationEnginePanelsConnection)> {
        let mut composition = Self::prepare(snapshot, false, stall_diagnostics, false)?;
        let (bridge, connection) = composition.runtime.initialize().await?;
        composition.bridge = Some(bridge);
        assert_eq!(composition.budget.available(), composition.budget.workers());
        Ok((composition, connection))
    }

    fn prepare(
        snapshot: DesktopGameSnapshot,
        stall_initial: bool,
        stall_diagnostics: bool,
        fail_configuration: bool,
    ) -> PresentationEnginePanelsTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let target = EnginePackageEnumTarget::MacosArm;
        let bytes = b"hermetic retained native engine composition";
        std::fs::create_dir(directory.path().join("bin"))?;
        let executable = directory.path().join(target.relative_path());
        std::fs::write(&executable, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        }
        let manifest = EnginePackageManifest::new(EnginePackageEntry::portable(
            target,
            u64::try_from(bytes.len())?,
            PackageDigest::read(&bytes[..])?,
            "fixture compiler".into(),
        )?)?;
        let package = EnginePackageSelector::select(
            directory.path(),
            &manifest,
            EnginePackageHost::from_observation(target, "14.0", "", true)?,
        )?;
        let budget = CapacityBudget::new(NonZeroUsize::new(4).ok_or("zero CPU budget")?)?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let resources = Arc::new(DesktopNativeGameResources::new(
            DesktopNativeGameResourcesOptions {
                budget: budget.clone(),
                policy: DesktopNativeGameResourcesPolicy::new(2, 128)?,
                memory: Arc::new(PresentationEnginePanelsMemory),
            },
        )?);
        let clock = Arc::new(ClockTimeProviderMonotonic::new());
        let wake = Arc::new(DesktopNativeEventLoopWake::new());
        let application = DesktopApplication::new(Arc::clone(&clock), Arc::clone(&wake));
        let (snapshots, receiver) = watch::channel(vec![snapshot]);
        let commands = Arc::new(Mutex::new(Vec::new()));
        let changed = Arc::new(tokio::sync::Notify::new());
        let runtime = DesktopNativeEngineRuntime::new(DesktopNativeEngineRuntimeOptions {
            provider: PresentationEnginePanelsProvider {
                commands: Arc::clone(&commands),
                spawns: AtomicUsize::new(0),
                stall_initial,
                stall_diagnostics,
                fail_configuration,
                changed: Arc::clone(&changed),
            },
            package,
            resources: Arc::clone(&resources),
            executor: Arc::clone(&executor),
            clock,
            wake: Arc::clone(&wake),
            snapshots: receiver,
            budget: budget.clone(),
            tablebase_parent: directory.path().to_path_buf(),
        });
        Ok(Self {
            runtime,
            bridge: None,
            application,
            budget,
            executor,
            commands,
            resources,
            changed,
            snapshots,
            files: tokio::sync::Mutex::new(Some(DesktopFileHandleRegistry::new())),
            previewed: tokio::sync::Mutex::new(None),
            wake,
            directory,
        })
    }

    async fn route(&self) -> PresentationEnginePanelsTestResult {
        if self.route_next().await? {
            Ok(())
        } else {
            Err("missing mounted native request".into())
        }
    }

    async fn route_next(&self) -> PresentationEnginePanelsTestResult<bool> {
        self.route_next_with_receipt_barrier(None).await
    }

    async fn route_before_receipt(
        &self,
        before_receipt: impl FnOnce() -> PresentationEnginePanelsTestResult,
    ) -> PresentationEnginePanelsTestResult {
        let barrier = tokio::sync::Barrier::new(2);
        let (routed, changed) = tokio::join!(
            self.route_next_with_receipt_barrier(Some(&barrier)),
            async {
                barrier.wait().await;
                let result = before_receipt();
                barrier.wait().await;
                result
            }
        );
        assert!(routed?);
        changed
    }

    async fn route_next_with_receipt_barrier(
        &self,
        barrier: Option<&tokio::sync::Barrier>,
    ) -> PresentationEnginePanelsTestResult<bool> {
        let Some(work) = self.application.try_next()? else {
            return Ok(false);
        };
        if let DesktopEnumCommand::FileSelect { owner, purpose } =
            work.request().command().operation()
        {
            let path = match purpose {
                DesktopFileEnumPurpose::WriteFile => self.directory.path().join("diagnostic.json"),
                DesktopFileEnumPurpose::Directory => self.directory.path().to_path_buf(),
                DesktopFileEnumPurpose::ReadFile => {
                    return Err("this fixture does not select source files".into());
                }
            };
            let mut files = self
                .files
                .lock()
                .await
                .take()
                .ok_or("native file registry already in use")?;
            let selected = files
                .register_selection(
                    path,
                    DesktopFileHandleRequest::new(
                        owner.file_handle_owner(),
                        *purpose,
                        self.application.port().now()?,
                    ),
                    &self.executor,
                )
                .await;
            *self.files.lock().await = Some(files);
            self.application.begin_commit(&work)?;
            self.application.complete_value(
                &work,
                DesktopEnumValue::FileSelect(Some(selected?)),
                None,
            )?;
            return Ok(true);
        }
        let DesktopEnumCommand::Engine(operation) = work.request().command().operation() else {
            return Err("expected typed engine command".into());
        };
        let control = operation.action().is_control();
        if !control {
            self.application.begin_commit(&work)?;
        }
        let selected = match operation.action() {
            DesktopEngineCommandsEnumOperation::PrepareTablebases { directory } => {
                Some((*directory, DesktopFileEnumPurpose::Directory))
            }
            DesktopEngineCommandsEnumOperation::Workbench(
                DesktopWorkbenchCommandsEnumOperation::ApplyExport { destination, .. },
            ) => Some((*destination, DesktopFileEnumPurpose::WriteFile)),
            DesktopEngineCommandsEnumOperation::Snapshot
            | DesktopEngineCommandsEnumOperation::Start { .. }
            | DesktopEngineCommandsEnumOperation::Stop { .. }
            | DesktopEngineCommandsEnumOperation::Configure(_)
            | DesktopEngineCommandsEnumOperation::Retry
            | DesktopEngineCommandsEnumOperation::LoadTablebases
            | DesktopEngineCommandsEnumOperation::Workbench(
                DesktopWorkbenchCommandsEnumOperation::Start { .. }
                | DesktopWorkbenchCommandsEnumOperation::Cancel { .. }
                | DesktopWorkbenchCommandsEnumOperation::Result { .. }
                | DesktopWorkbenchCommandsEnumOperation::PreviewExport { .. },
            ) => None,
        };
        let selection = if let Some((handle, purpose)) = selected {
            Some(
                self.files
                    .lock()
                    .await
                    .as_mut()
                    .ok_or("missing native file registry")?
                    .consume(
                        handle,
                        operation.context().owner().file_handle_owner(),
                        purpose,
                        self.application.port().now()?,
                    )?,
            )
        } else {
            None
        };
        let result = self
            .bridge
            .as_ref()
            .ok_or("missing native bridge")?
            .execute(DesktopEngineBridgeRequest {
                operation: operation.clone(),
                receipt: work.receipt().clone(),
                selection,
            })
            .await;
        if let Some(barrier) = barrier {
            barrier.wait().await;
            barrier.wait().await;
        }
        if control {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    match self.application.complete_engine_control(&work, result.clone()) {
                        Err(chess::desktop::application::DesktopApplicationError::CancellationUnacknowledged) => {
                            tokio::task::yield_now().await;
                        }
                        completed => return completed,
                    }
                }
            })
            .await??;
        } else {
            match result {
                Ok(result) => {
                    if let DesktopEngineBridgeEnumResult::ExportPreview(preview) = &result {
                        *self.previewed.lock().await = Some(preview.bytes().to_vec());
                    }
                    let revision =
                        if let DesktopEngineBridgeEnumResult::Snapshot(snapshot) = &result {
                            Some(snapshot.configuration())
                        } else {
                            None
                        };
                    self.application.complete_value(
                        &work,
                        DesktopEnumValue::Engine(Box::new(result)),
                        revision,
                    )?;
                }
                Err(failure) => self.application.complete_failure(&work, failure)?,
            }
        }
        Ok(true)
    }

    fn count(&self, matches: impl Fn(&str) -> bool) -> PresentationEnginePanelsTestResult<usize> {
        Ok(self
            .commands
            .lock()
            .map_err(|error| error.to_string())?
            .iter()
            .filter(|command| matches(command))
            .count())
    }

    fn search_count(&self) -> PresentationEnginePanelsTestResult<usize> {
        self.count(|command| command.starts_with("go "))
    }

    async fn diagnostic_started(&self) -> PresentationEnginePanelsTestResult {
        loop {
            let changed = self.changed.notified();
            if self.count(|command| command == "uci")? > 1 {
                return Ok(());
            }
            changed.await;
        }
    }

    async fn settle(
        &self,
        harness: &mut Harness<'_, PresentationEnginePanelsMounted>,
        ready: impl Fn(&PresentationEnginePanels) -> bool,
    ) -> PresentationEnginePanelsTestResult {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                self.wake.acknowledge();
                self.application.port().acknowledge_wake();
                harness.run();
                if ready(&harness.state().panels) {
                    return Ok::<_, Box<dyn std::error::Error>>(());
                }
                if !self.route_next().await? {
                    self.wake.notified().await;
                }
            }
        })
        .await?
    }

    async fn close(mut self) -> PresentationEnginePanelsTestResult {
        self.runtime
            .shutdown_retained(Instant::now() + std::time::Duration::from_secs(3))
            .await?;
        assert!(self.runtime.is_shutdown_complete());
        assert_eq!(self.budget.available(), self.budget.workers());
        drop(self.runtime);
        Arc::try_unwrap(self.executor)
            .map_err(|executor| {
                format!("Retained {} executor owners", Arc::strong_count(&executor))
            })?
            .shutdown()
            .await?;
        Ok(())
    }
}

struct PresentationEnginePanelsMounted {
    panels: PresentationEnginePanels,
    snapshot: DesktopGameSnapshot,
    port: DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
    initialized: bool,
    failures: Vec<String>,
}

impl PresentationEnginePanelsMounted {
    fn select_root(
        &mut self,
        session: &mut GameSession,
        composition: &PresentationEnginePanelsComposition,
    ) -> PresentationEnginePanelsTestResult {
        let previous = PresentationEnginePanels::selection_from(&self.snapshot)?
            .ok_or("missing original selection")?;
        session.select_view(session.tree().root())?;
        let snapshot = DesktopGameSnapshot::capture(session, ClockInstant::from_nanos(0))?;
        let selected =
            PresentationEnginePanels::selection_from(&snapshot)?.ok_or("missing root selection")?;
        assert_eq!(previous.context(), selected.context());
        assert_ne!(previous.position().history(), selected.position().history());
        composition.snapshots.send_replace(vec![snapshot.clone()]);
        self.snapshot = snapshot;
        Ok(())
    }

    fn mount_workbench(
        connection: PresentationEnginePanelsConnection,
        snapshot: DesktopGameSnapshot,
        composition: &PresentationEnginePanelsComposition,
    ) -> PresentationEnginePanelsTestResult<Harness<'static, Self>> {
        let mut harness = Self::mount(
            PresentationEnginePanels::new(connection),
            snapshot,
            composition.application.port(),
        );
        harness.run_ok().ok_or("mounted workbench did not settle")?;
        harness.get_by_label("Workbench").click_accesskit();
        harness.run();
        Ok(harness)
    }

    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if !state.initialized {
            ui.ctx().set_fonts(PresentationThemeFont::definitions());
            state.initialized = true;
            ui.ctx().request_repaint();
            return;
        }
        let was_pending = state.panels.has_pending_work();
        if let Err(error) = state.panels.synchronize(Some(&state.snapshot), &state.port) {
            state.failures.push(error.to_string());
        }
        if let Err(error) = state.panels.show(ui, &state.port) {
            state.failures.push(error.to_string());
        }
        if !was_pending && state.panels.has_pending_work() {
            ui.ctx()
                .request_discard("Exercise repeated mounted engine passes");
        }
    }

    fn mount(
        panels: PresentationEnginePanels,
        snapshot: DesktopGameSnapshot,
        port: DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
    ) -> Harness<'static, Self> {
        Harness::builder()
            .with_size(egui::vec2(1280.0, 2400.0))
            .build_ui_state(
                Self::show,
                Self {
                    panels,
                    snapshot,
                    port,
                    initialized: false,
                    failures: Vec::new(),
                },
            )
    }
}

#[tokio::test]
async fn same_revision_pending_diagnostic_start_cancels_exact_job_and_releases_its_lease()
-> PresentationEnginePanelsTestResult {
    let mut session = PresentationEnginePanelsFixture::branched_session()?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let (composition, connection) =
        PresentationEnginePanelsComposition::new(snapshot.clone()).await?;
    let updates = connection.workbench.clone();
    let mut harness =
        PresentationEnginePanelsMounted::mount_workbench(connection, snapshot, &composition)?;
    harness.get_by_label("Perft / divide").click_accesskit();
    harness.run();
    harness.get_by_label("Run diagnostic").click_accesskit();
    harness.run();
    composition
        .route_before_receipt(|| {
            assert_eq!(
                composition.budget.available(),
                composition.budget.workers() - 1
            );
            harness
                .state_mut()
                .select_root(&mut session, &composition)?;
            harness.run();
            Ok(())
        })
        .await?;
    let started = updates.borrow().clone().ok_or("missing admitted job")?;
    assert!(!started.snapshot.is_terminal());
    harness.run();
    assert!(
        harness.state().failures.is_empty(),
        "{:?}",
        harness.state().failures
    );
    assert!(
        harness
            .state()
            .panels
            .workbench()
            .ok_or("missing root controller")?
            .job()
            .is_none()
    );
    assert!(harness.state().panels.has_pending_work());
    composition.route().await?;
    harness.run();
    let cancelled = updates.borrow().clone().ok_or("missing cancelled job")?;
    assert_eq!(cancelled.snapshot.id(), started.snapshot.id());
    assert_eq!(cancelled.request, started.request);
    assert_eq!(
        cancelled.snapshot.state(),
        chess::workbench::enums::WorkbenchEnumJobState::Cancelled
    );
    assert_eq!(composition.budget.available(), composition.budget.workers());
    assert!(
        harness.state().failures.is_empty(),
        "{:?}",
        harness.state().failures
    );
    drop(harness);
    composition.close().await
}

#[tokio::test]
async fn same_revision_pending_clear_hash_refreshes_revision_before_the_next_request()
-> PresentationEnginePanelsTestResult {
    let mut session = PresentationEnginePanelsFixture::branched_session()?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let (composition, connection) =
        PresentationEnginePanelsComposition::new(snapshot.clone()).await?;
    let mut harness =
        PresentationEnginePanelsMounted::mount_workbench(connection, snapshot, &composition)?;
    harness.get_by_label("Clear Hash").click_accesskit();
    harness.run();
    composition
        .route_before_receipt(|| {
            harness
                .state_mut()
                .select_root(&mut session, &composition)?;
            harness.run();
            Ok(())
        })
        .await?;
    harness.run();
    let controller = harness
        .state()
        .panels
        .workbench()
        .ok_or("missing root controller")?;
    assert_eq!(controller.context().configuration().value(), 1);
    assert_eq!(
        controller
            .position()
            .ok_or("missing root history")?
            .command(),
        "position startpos"
    );
    assert!(!controller.is_pending());
    assert!(
        harness.state().failures.is_empty(),
        "{:?}",
        harness.state().failures
    );
    harness.get_by_label("Clear Hash").click_accesskit();
    harness.run();
    composition.route().await?;
    harness.run();
    assert_eq!(
        harness
            .state()
            .panels
            .workbench()
            .ok_or("missing configured root")?
            .context()
            .configuration()
            .value(),
        2
    );
    assert_eq!(
        composition.count(|command| command == "setoption name Clear Hash")?,
        2
    );
    assert!(
        harness.state().failures.is_empty(),
        "{:?}",
        harness.state().failures
    );
    drop(harness);
    composition.close().await
}

#[cfg(unix)]
#[tokio::test]
async fn same_revision_pending_tablebase_preparation_refreshes_authoritative_status()
-> PresentationEnginePanelsTestResult {
    let mut session = PresentationEnginePanelsFixture::branched_session()?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let (composition, connection) =
        PresentationEnginePanelsComposition::new(snapshot.clone()).await?;
    std::fs::write(
        composition.directory.path().join("KQvK.rtbw"),
        b"local WDL fixture",
    )?;
    let mut harness =
        PresentationEnginePanelsMounted::mount_workbench(connection, snapshot, &composition)?;
    harness
        .get_by_label("Select tablebase directories")
        .click_accesskit();
    harness.run();
    composition.route().await?;
    harness.run();
    composition
        .route_before_receipt(|| {
            harness
                .state_mut()
                .select_root(&mut session, &composition)?;
            harness.run();
            Ok(())
        })
        .await?;
    harness.run();
    let controller = harness
        .state()
        .panels
        .workbench()
        .ok_or("missing root controller")?;
    let tablebases = controller
        .engine()
        .ok_or("missing authoritative engine")?
        .tablebases();
    assert_eq!(tablebases.state(), &chess::presentation::workbench::snapshot::PresentationWorkbenchTablebaseEnumState::Configured);
    assert_eq!(
        tablebases.directories(),
        &[composition.directory.path().canonicalize()?]
    );
    assert_eq!(controller.context().configuration().value(), 0);
    assert!(!controller.is_pending());
    assert!(
        harness.state().failures.is_empty(),
        "{:?}",
        harness.state().failures
    );
    drop(harness);
    composition.close().await
}

#[cfg(unix)]
#[tokio::test]
async fn same_revision_pending_tablebase_failure_does_not_reject_the_new_controller()
-> PresentationEnginePanelsTestResult {
    let mut session = PresentationEnginePanelsFixture::branched_session()?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let (composition, connection) =
        PresentationEnginePanelsComposition::new(snapshot.clone()).await?;
    let mut harness =
        PresentationEnginePanelsMounted::mount_workbench(connection, snapshot, &composition)?;
    harness
        .get_by_label("Select tablebase directories")
        .click_accesskit();
    harness.run();
    composition.route().await?;
    harness.run();
    composition
        .route_before_receipt(|| {
            harness
                .state_mut()
                .select_root(&mut session, &composition)?;
            harness.run();
            Ok(())
        })
        .await?;
    harness.run();
    let controller = harness
        .state()
        .panels
        .workbench()
        .ok_or("missing root controller")?;
    assert!(!controller.is_pending());
    assert!(controller.failure().is_none());
    assert!(
        harness.state().failures.is_empty(),
        "{:?}",
        harness.state().failures
    );
    drop(harness);
    composition.close().await
}

#[tokio::test]
async fn mounted_start_stop_uses_real_native_actor_and_never_resubmits_on_repeated_passes()
-> PresentationEnginePanelsTestResult {
    let mut session = PresentationEnginePanelsFixture::session()?;
    session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let (composition, connection) =
        PresentationEnginePanelsComposition::new(snapshot.clone()).await?;
    let mut harness = PresentationEnginePanelsMounted::mount(
        PresentationEnginePanels::new(connection),
        snapshot,
        composition.application.port(),
    );
    harness
        .run_ok()
        .ok_or("mounted engine panels did not settle")?;
    harness.get_by_label("Start Analysis").click_accesskit();
    harness.run();
    harness.run();
    assert!(harness.state().panels.has_pending_work());
    composition.route().await?;
    harness.run();
    harness.run();
    assert_eq!(composition.search_count()?, 1);
    assert!(composition.application.try_next()?.is_none());
    harness.get_by_label("Stop Analysis").click_accesskit();
    harness.run();
    composition.route().await?;
    harness.run();
    harness.run();
    assert_eq!(composition.search_count()?, 1);
    let terminal = harness
        .state()
        .panels
        .analysis()
        .and_then(|controller| controller.terminal())
        .ok_or("missing real stop completion")?;
    assert!(matches!(
        terminal.completion(),
        EngineSearchSessionEnumCompletion::Stopped
    ));
    assert!(
        harness.state().failures.is_empty(),
        "{:?}",
        harness.state().failures
    );
    drop(harness);
    composition.close().await
}

#[tokio::test]
async fn same_revision_history_navigation_stops_old_search_and_requires_explicit_start()
-> PresentationEnginePanelsTestResult {
    let mut session = PresentationEnginePanelsFixture::session()?;
    session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
    let root = session.view();
    let movement = session.tree().node(root)?.position().parse_uci("e2e4")?;
    session.append_study_variation(
        session.revision(),
        "123e4567-e89b-42d3-a456-426614174002".parse()?,
        movement,
    )?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let (composition, connection) =
        PresentationEnginePanelsComposition::new(snapshot.clone()).await?;
    let mut harness = PresentationEnginePanelsMounted::mount(
        PresentationEnginePanels::new(connection),
        snapshot,
        composition.application.port(),
    );
    harness
        .run_ok()
        .ok_or("mounted history panels did not settle")?;
    harness.get_by_label("Start Analysis").click_accesskit();
    harness.run();
    composition.route().await?;
    harness.run();
    session.select_view(root)?;
    let changed = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    composition.snapshots.send_replace(vec![changed.clone()]);
    harness.state_mut().snapshot = changed;
    harness.run();
    composition.route().await?;
    harness.run();
    assert_eq!(composition.search_count()?, 1);
    assert!(
        harness
            .state()
            .panels
            .analysis()
            .ok_or("missing selected analysis")?
            .metadata()
            .is_none()
    );
    assert!(
        harness.state().failures.is_empty(),
        "{:?}",
        harness.state().failures
    );
    drop(harness);
    composition.close().await
}

#[tokio::test]
async fn canonical_live_demand_preempts_mounted_analysis_before_reassigning_its_cpu()
-> PresentationEnginePanelsTestResult {
    use chess::desktop::native::game_engine::DesktopNativeGameEngineResources;
    use chess::game::engine_provider::GameEngineContext;

    let mut session = PresentationEnginePanelsFixture::session()?;
    session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let (composition, connection) =
        PresentationEnginePanelsComposition::new(snapshot.clone()).await?;
    let live = GameEngineContext::new(
        "123e4567-e89b-42d3-a456-426614174999".parse()?,
        "0".parse()?,
    );
    let registration = composition.resources.register(live).await?;
    let startup = composition.resources.allocate(live).await?;
    composition
        .resources
        .acknowledge(live, startup.config())
        .await?;
    drop(startup);
    composition
        .bridge
        .as_ref()
        .ok_or("missing native bridge")?
        .quiesce(Instant::now() + std::time::Duration::from_secs(3))
        .await?;
    let mut harness = PresentationEnginePanelsMounted::mount(
        PresentationEnginePanels::new(connection),
        snapshot,
        composition.application.port(),
    );
    harness
        .run_ok()
        .ok_or("mounted priority panel did not settle")?;
    harness.get_by_label("Start Analysis").click_accesskit();
    harness.run();
    composition.route().await?;
    harness.run();
    assert_eq!(composition.budget.available(), 2);
    let allocation = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        composition.resources.allocate(live),
    )
    .await??;
    assert_eq!(allocation.workers(), 2);
    assert_eq!(composition.budget.available(), 2);
    harness.run();
    let terminal = harness
        .state()
        .panels
        .analysis()
        .and_then(|controller| controller.terminal())
        .ok_or("live demand did not deliver its observed analysis terminal")?;
    assert!(matches!(
        terminal.completion(),
        EngineSearchSessionEnumCompletion::Stopped | EngineSearchSessionEnumCompletion::Cancelled
    ));
    assert_eq!(composition.search_count()?, 1);
    drop(allocation);
    drop(registration);
    drop(harness);
    composition.close().await
}

#[tokio::test]
async fn shutdown_keeps_the_terminal_consumer_alive_after_the_mounted_panel_is_dropped()
-> PresentationEnginePanelsTestResult {
    let mut session = PresentationEnginePanelsFixture::session()?;
    session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let (composition, connection) =
        PresentationEnginePanelsComposition::new(snapshot.clone()).await?;
    let mut harness = PresentationEnginePanelsMounted::mount(
        PresentationEnginePanels::new(connection),
        snapshot,
        composition.application.port(),
    );
    harness
        .run_ok()
        .ok_or("mounted shutdown panel did not settle")?;
    harness.get_by_label("Start Analysis").click_accesskit();
    harness.run();
    composition.route().await?;
    harness.run();
    assert_eq!(composition.search_count()?, 1);
    drop(harness);
    composition.close().await
}

#[tokio::test]
async fn mounted_cancel_setup_uses_the_real_retained_start_identity_before_job_snapshot()
-> PresentationEnginePanelsTestResult {
    let mut session = PresentationEnginePanelsFixture::session()?;
    session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let (composition, connection) =
        PresentationEnginePanelsComposition::build(snapshot.clone(), true).await?;
    let mut harness = PresentationEnginePanelsMounted::mount(
        PresentationEnginePanels::new(connection),
        snapshot,
        composition.application.port(),
    );
    harness
        .run_ok()
        .ok_or("mounted setup cancellation panel did not settle")?;
    harness.get_by_label("Workbench").click_accesskit();
    harness.run();
    harness
        .get_by_label("Compiler diagnostics")
        .click_accesskit();
    harness.run();
    harness.get_by_label("Run diagnostic").click_accesskit();
    harness.run();
    {
        let start = composition.route();
        tokio::pin!(start);
        tokio::select! {
            result = &mut start => { result?; return Err("stalled diagnostic unexpectedly completed before cancellation".into()); }
            ready = composition.diagnostic_started() => ready?,
        }
        harness.run();
        assert!(
            harness
                .state()
                .panels
                .workbench()
                .ok_or("missing workbench controller")?
                .job()
                .is_none()
        );
        harness
            .get_by_label("Cancel Diagnostic Setup")
            .click_accesskit();
        harness.run();
        harness.run();
        let cancel = composition.route();
        let (start, cancel) = tokio::join!(start, cancel);
        start?;
        cancel?;
    }
    composition
        .settle(&mut harness, |panels| !panels.has_pending_work())
        .await?;
    assert_eq!(composition.count(|command| command == "uci")?, 2);
    assert_eq!(composition.budget.available(), composition.budget.workers());
    assert!(!harness.state().panels.has_pending_work());
    assert!(
        harness.state().failures.is_empty(),
        "{:?}",
        harness.state().failures
    );
    drop(harness);
    composition.close().await
}

#[tokio::test]
async fn dropping_initial_handshake_await_retains_cpu_and_child_until_observed_cleanup()
-> PresentationEnginePanelsTestResult {
    use std::future::{Future, poll_fn};
    use std::task::Poll;

    let mut session = PresentationEnginePanelsFixture::session()?;
    session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let mut composition =
        PresentationEnginePanelsComposition::prepare(snapshot, true, false, false)?;
    {
        let initialization = composition.runtime.initialize();
        tokio::pin!(initialization);
        poll_fn(|context| {
            assert!(initialization.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        assert_eq!(composition.budget.available(), 2);
    }
    assert!(!composition.runtime.is_shutdown_complete());
    assert_eq!(composition.budget.available(), 2);
    composition.close().await
}

#[tokio::test]
async fn mounted_clear_hash_and_isolated_diagnostic_export_write_the_exact_reviewed_artifact()
-> PresentationEnginePanelsTestResult {
    PresentationEnginePanelsFixture::export(PresentationEnginePanelsEnumExportSelection::Current)
        .await
}

#[tokio::test]
async fn same_revision_pending_export_retains_publication_and_ignores_the_old_ui_acknowledgement()
-> PresentationEnginePanelsTestResult {
    PresentationEnginePanelsFixture::export(PresentationEnginePanelsEnumExportSelection::Changed)
        .await
}

enum PresentationEnginePanelsEnumExportSelection {
    Current,
    Changed,
}

impl PresentationEnginePanelsFixture {
    fn export(
        selection: PresentationEnginePanelsEnumExportSelection,
    ) -> impl std::future::Future<Output = PresentationEnginePanelsTestResult> {
        Box::pin(async move {
            let mut session = Self::branched_session()?;
            let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
            let (composition, connection) =
                PresentationEnginePanelsComposition::new(snapshot.clone()).await?;
            let mut harness = PresentationEnginePanelsMounted::mount(
                PresentationEnginePanels::new(connection),
                snapshot,
                composition.application.port(),
            );
            harness
                .run_ok()
                .ok_or("mounted export panel did not settle")?;
            harness.get_by_label("Workbench").click_accesskit();
            harness.run();
            harness.get_by_label("Clear Hash").click_accesskit();
            harness.run();
            harness.run();
            composition.route().await?;
            harness.run();
            assert_eq!(
                composition.count(|command| command == "setoption name Clear Hash")?,
                1
            );
            assert_eq!(
                harness
                    .state()
                    .panels
                    .workbench()
                    .ok_or("missing configured workbench")?
                    .context()
                    .configuration()
                    .value(),
                1
            );
            harness
                .get_by_label("Compiler diagnostics")
                .click_accesskit();
            harness.run();
            harness.get_by_label("Run diagnostic").click_accesskit();
            harness.run();
            composition.route().await?;
            composition
                .settle(&mut harness, |panels| {
                    panels
                        .workbench()
                        .and_then(|controller| controller.job())
                        .is_some_and(|job| job.is_terminal())
                        && !panels.has_pending_work()
                })
                .await?;
            assert_eq!(composition.count(|command| command == "uci")?, 2);
            harness
                .get_by_label("Export diagnostic result")
                .click_accesskit();
            harness.run();
            composition.route().await?;
            harness.run();
            harness.get_by_label("Diagnostic export preview");
            harness
                .get_by_label("Save Diagnostic Export")
                .click_accesskit();
            harness.run();
            composition.route().await?;
            harness.run();
            match selection {
                PresentationEnginePanelsEnumExportSelection::Current => composition.route().await?,
                PresentationEnginePanelsEnumExportSelection::Changed => {
                    composition
                        .route_before_receipt(|| {
                            harness
                                .state_mut()
                                .select_root(&mut session, &composition)?;
                            harness.run();
                            Ok(())
                        })
                        .await?;
                }
            }
            harness.run();
            let bytes = std::fs::read(composition.directory.path().join("diagnostic.json"))?;
            assert_eq!(Some(bytes), *composition.previewed.lock().await);
            assert_eq!(
                harness
                    .state()
                    .panels
                    .workbench()
                    .ok_or("missing exported workbench")?
                    .exported_job()
                    .is_some(),
                matches!(
                    selection,
                    PresentationEnginePanelsEnumExportSelection::Current
                )
            );
            assert!(
                harness.state().failures.is_empty(),
                "{:?}",
                harness.state().failures
            );
            drop(harness);
            composition.close().await
        })
    }
}

#[tokio::test]
async fn configuration_failure_retains_the_actual_initial_owner_until_cleanup()
-> PresentationEnginePanelsTestResult {
    let mut session = PresentationEnginePanelsFixture::session()?;
    session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
    let snapshot = DesktopGameSnapshot::capture(&session, ClockInstant::from_nanos(0))?;
    let mut composition =
        PresentationEnginePanelsComposition::prepare(snapshot, false, false, true)?;
    assert!(composition.runtime.initialize().await.is_err());
    assert!(!composition.runtime.is_shutdown_complete());
    assert_eq!(composition.budget.available(), 2);
    composition.close().await
}
