use std::num::{NonZeroU64, NonZeroUsize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::asset::{Assets, Handle};
use bevy::gltf::{GltfMaterial, GltfMesh, GltfNode, GltfPrimitive, gltf};
use bevy::platform::collections::HashMap;
use bevy::prelude::{Entity, Image, Mesh, StandardMaterial, Transform, World};
use bevy::shape::Cuboid;
use bevy::window::Window;
use bevy_egui::{EguiContext, EguiUserTextures, PrimaryEguiContext, egui};
use tokio::runtime::Handle as RuntimeHandle;
use tokio::sync::watch;
use tokio::time::{Instant, timeout_at};

use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::capacity::phase::{CapacityPhase, CapacityPhaseEnumState};
use crate::clock::duration::ClockInstant;
use crate::clock::time_provider::{ClockTimeProvider, ClockTimeProviderMonotonic};
use crate::desktop::application::{DesktopApplication, DesktopApplicationWork};
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::messages::{DesktopEnumCommand, DesktopMessagesEntityId};
use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;
use crate::desktop::native::host::{
    DesktopNativeHost, DesktopNativeHostEnumQuit, DesktopNativeHostEnumTeardown,
    DesktopNativeHostServices, DesktopNativeHostState,
};
use crate::desktop::native::puzzle_runtime::DesktopNativePuzzleRuntimeSchedulerTokio;
use crate::desktop::native::puzzle_service::{
    DesktopNativePuzzleService, DesktopNativePuzzleServiceOptions,
};
use crate::desktop::native::puzzle_window::DesktopNativePuzzleWindow;
use crate::desktop::native::recovery::{
    DesktopNativeRecoveryEnumAction, DesktopNativeRecoveryError, DesktopNativeRecoveryFailure,
    DesktopNativeRecoveryProvider,
};
use crate::desktop::native::resources::DesktopNativeResources;
use crate::desktop::puzzle_bridge::DesktopPuzzleBridgeEnumOperation;
use crate::desktop::puzzle_commands::DesktopPuzzleCommands;
use crate::desktop::puzzle_service::{
    DesktopPuzzleServiceEnumActivity, DesktopPuzzleServiceEnumOperation,
    DesktopPuzzleServiceEnumResult, DesktopPuzzleServiceState,
};
use crate::desktop::puzzle_snapshot::DesktopPuzzleSnapshot;
use crate::presentation::app::{PresentationApp, PresentationAppEnumWorkspace};
use crate::presentation::board::enums::PresentationBoardEnumCameraAction;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::puzzle::panel::PresentationPuzzlePanelConnection;
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::PuzzleEnumCountdownMode;
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::puzzle::metadata::PuzzleDefinitionMetadata;
use crate::puzzle::persistence::{
    PuzzlePersistenceEnumCommand, PuzzlePersistenceRun, PuzzlePersistenceStorageRevision,
};
use crate::puzzle::random_provider::PuzzleRandomProviderSeeded;
use crate::puzzle::run::PuzzleRunEnumStatus;
use crate::puzzle::selection::PuzzleSelectionFilter;
use crate::puzzle::session::{PuzzleSessionConfig, PuzzleSessionId};
use crate::puzzle::timer::PuzzleTimerEnumReadiness;
use crate::scene::assets::{SceneAssets, SceneAssetsPieceKey};
use crate::scene::camera::SceneCamera;
use crate::scene::picking::ScenePickingViewport;
use crate::scene::renderer::SceneRenderer;
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::puzzle_repository::StoragePuzzleRepository;

type DesktopNativeHostPuzzleTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type DesktopNativeHostPuzzleTestService = DesktopNativePuzzleService<
    ClockTimeProviderMonotonic,
    DesktopNativeEventLoopWake,
    DesktopNativePuzzleRuntimeSchedulerTokio,
>;

struct DesktopNativeHostPuzzleTestRecovery;

impl DesktopNativeRecoveryProvider for DesktopNativeHostPuzzleTestRecovery {
    fn choose(
        &self,
        _failure: &DesktopNativeRecoveryFailure,
    ) -> Result<DesktopNativeRecoveryEnumAction, DesktopNativeRecoveryError> {
        Ok(DesktopNativeRecoveryEnumAction::KeepPaused)
    }
}

#[derive(Clone, Copy)]
enum DesktopNativeHostPuzzleTestEnumDeferral {
    CapacityContended,
    PreparationUnacknowledged,
}

#[derive(Clone, Copy)]
enum DesktopNativeHostPuzzleTestEnumGeometry {
    Viewport,
    Camera,
}

struct DesktopNativeHostPuzzleTestObservation {
    awaiting_readiness: bool,
    current_board_applied: bool,
    started_ready: bool,
    preparation_deferred: bool,
    pending_view: bool,
    pending_camera: bool,
    viewport_matches: bool,
    wake_rearmed: bool,
    has_board_semantics: bool,
    ready_requested: bool,
    other_work_requested: bool,
    presentation_failed: bool,
}

struct DesktopNativeHostPuzzleFixture {
    directory: tempfile::TempDir,
    storage: StorageDocumentStore,
    repository: StoragePuzzleRepository,
    executor: Arc<CapacityExecutor>,
    service: DesktopNativeHostPuzzleTestService,
    application: DesktopApplication<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
    selected: watch::Sender<DesktopPuzzleServiceState>,
    snapshot: Arc<DesktopPuzzleSnapshot>,
    context: egui::Context,
    window: Entity,
    world: World,
    state: DesktopNativeHostState,
}

impl DesktopNativeHostPuzzleFixture {
    const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);
    const DEADLINE: Duration = Duration::from_secs(3);

    fn run() -> DesktopNativeHostPuzzleTestResult<PuzzlePersistenceRun> {
        let initial = BoardPosition::standard();
        let first = initial.parse_uci("e2e4")?;
        let second = initial.play(first)?.parse_uci("e7e5")?;
        Ok(PuzzlePersistenceRun::new(
            PuzzleSessionId::new(NonZeroU64::MIN),
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            vec![PuzzleDefinition::new(
                PuzzleDefinitionIdentity::new(
                    "host-readiness".into(),
                    "v1".into(),
                    "first".into(),
                )?,
                initial,
                vec![vec![first, second]],
                PuzzleDefinitionMetadata::default(),
            )?],
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(41),
        )?)
    }

    fn renderer(world: &mut World) -> DesktopNativeHostPuzzleTestResult<SceneRenderer> {
        let source = gltf::Gltf::from_slice(include_bytes!("../../../resources/pieces/chess.glb"))?;
        let source_node = source.document.nodes().next().ok_or("missing piece node")?;
        let geometry = world
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(0.5, 0.8, 0.5));
        let mut materials = Assets::<GltfMaterial>::default();
        let material = materials.add(GltfMaterial::default());
        let mut meshes = Assets::<GltfMesh>::default();
        let mesh = meshes.add(GltfMesh {
            index: 0,
            name: "readiness_fixture".into(),
            primitives: vec![GltfPrimitive {
                index: 0,
                parent_mesh_index: 0,
                name: "readiness_fixture".into(),
                mesh: geometry,
                material: Some(material),
                extras: None,
                material_extras: None,
            }],
            extras: None,
        });
        let mut nodes = Assets::<GltfNode>::default();
        let node = nodes.add(GltfNode::new(
            &source_node,
            Vec::new(),
            Some(mesh),
            Transform::default(),
            None,
            None,
        ));
        let names = SceneAssetsPieceKey::all()
            .map(|key| (key.name().into_boxed_str(), node.clone()))
            .collect::<HashMap<_, _>>();
        let assets = SceneAssets::resolve(
            &names,
            &nodes,
            &meshes,
            &materials,
            &mut world.resource_mut::<Assets<StandardMaterial>>(),
        )?;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0));
        let mut renderer = SceneRenderer::spawn(
            world,
            assets,
            ScenePickingViewport::new(rect, rect, 1.0, 1.0)?,
            SceneCamera::initial(BoardEnumColor::White),
        )?;
        renderer.apply_position(world, &BoardPosition::standard())?;
        renderer.mark_ready(renderer.generation())?;
        Ok(renderer)
    }

    async fn new() -> DesktopNativeHostPuzzleTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(
            NonZeroUsize::new(CapacityPhase::MINIMUM_WORKERS).ok_or("zero native capacity")?,
        )?;
        let mut phase = CapacityPhase::new(budget.clone())?;
        let generation = phase
            .generation()
            .ok_or("missing initial native generation")?;
        phase.acknowledge_searches_stopped(generation)?;
        phase.try_enter_preparation(generation)?;
        phase.complete_preparation(generation)?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let storage =
            StorageDocumentStore::open(directory.path().join("puzzles.sqlite3"), budget.clone())
                .await?;
        let repository = StoragePuzzleRepository::new(storage.client());
        let now = ClockInstant::from_nanos(0);
        let mut owner = DesktopPuzzleCommands::new(
            Self::run()?,
            repository.clone(),
            now,
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?;
        owner.begin_write(now)?;
        timeout_at(Instant::now() + Self::DEADLINE, async {
            while owner.has_retained_write() {
                owner.wait().await?;
                owner.settle(now)?;
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        })
        .await??;
        let snapshot = Arc::new(owner.snapshot(now)?);
        drop(owner);
        let (selected, _receiver) = watch::channel(DesktopPuzzleServiceState::new(
            Some(snapshot.context().session_id()),
            Some(Arc::clone(&snapshot)),
            DesktopPuzzleServiceEnumActivity::Selected,
            None,
            None,
        ));
        let clock = Arc::new(ClockTimeProviderMonotonic::new());
        let wake = Arc::new(DesktopNativeEventLoopWake::new());
        let application = DesktopApplication::new(Arc::clone(&clock), Arc::clone(&wake));
        let resources = DesktopNativeResources::from_directory(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("resources"),
        )?;
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<EguiUserTextures>();
        let mut egui = EguiContext::default();
        let context = egui.get_mut().clone();
        context.enable_accesskit();
        world.spawn((egui, PrimaryEguiContext));
        let window = world
            .spawn(Window {
                focused: true,
                visible: true,
                ..Window::default()
            })
            .id();
        let renderer = Self::renderer(&mut world)?;
        world.insert_resource(renderer);
        let mut service = DesktopNativePuzzleService::new(DesktopNativePuzzleServiceOptions {
            repository: repository.clone(),
            executor: Arc::clone(&executor),
            clock: Arc::clone(&clock),
            wake: Arc::clone(&wake),
            runtime: RuntimeHandle::current(),
            scheduler: DesktopNativePuzzleRuntimeSchedulerTokio::new(
                RuntimeHandle::current(),
                clock.now()?,
                Instant::now(),
            ),
            replacement_timeout: Self::DEADLINE,
        });
        let puzzles = service.start()?;
        let (events, _events) = DesktopNativeHost::event_channel();
        let (dialog_ready, _dialogs) = watch::channel(None);
        let state = DesktopNativeHostState {
            services: DesktopNativeHostServices {
                engine_panels: None,
                puzzle_panel: None,
                puzzles,
                port: application.port(),
                wake,
                resources,
                capacity: Arc::new(Mutex::new(phase)),
                runtime: RuntimeHandle::current(),
                recovery: Arc::new(DesktopNativeHostPuzzleTestRecovery),
                events,
                budget,
                dialog_ready,
            },
            presentation: PresentationApp::new(),
            puzzle_window: None,
            asset: Handle::default(),
            asset_reload: None,
            camera: SceneCamera::initial(BoardEnumColor::White),
            rendered_board: None,
            generation,
            ready: true,
            fonts_initialized: true,
            recovery: None,
            graphics_drain: None,
            failure: None,
            fault_received_at: None,
            interruption_announced: false,
            quit: DesktopNativeHostEnumQuit::Idle,
            teardown: DesktopNativeHostEnumTeardown::Active,
            replacement: None,
            expected_device: None,
            validated_surface: None,
            pending_view: None,
            pending_camera: None,
            dialog_ready: false,
            dialog_admission_closed: false,
            exit_authorized: false,
            readiness: None,
        };
        Ok(Self {
            directory,
            storage,
            repository,
            executor,
            service,
            application,
            selected,
            snapshot,
            context,
            window,
            world,
            state,
        })
    }

    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Self::SCREEN)),
            events,
            ..egui::RawInput::default()
        }
    }

    fn presentation_frame(
        &mut self,
        events: Vec<egui::Event>,
    ) -> DesktopNativeHostPuzzleTestResult<(egui::FullOutput, Option<ScenePickingViewport>)> {
        let mut result = None;
        let mut output = self.context.run_ui(Self::input(events), |ui| {
            let mut picker = |_point| Ok(None);
            result = Some(self.state.presentation.show(
                ui,
                &self.application.port(),
                None,
                &mut picker,
            ));
        });
        output.textures_delta.clear();
        let frame = result.ok_or("missing host fixture presentation frame")??;
        Ok((output, frame.viewport()))
    }

    fn click(
        output: &egui::FullOutput,
        label: &str,
    ) -> DesktopNativeHostPuzzleTestResult<egui::Event> {
        let (target, _node) = output
            .platform_output
            .accesskit_update
            .as_ref()
            .ok_or("missing host fixture accessibility update")?
            .nodes
            .iter()
            .find(|(_id, node)| node.label() == Some(label))
            .ok_or("missing host fixture control")?;
        Ok(egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: *target,
                data: None,
            },
        ))
    }

    fn next(&self) -> DesktopNativeHostPuzzleTestResult<DesktopApplicationWork> {
        self.application
            .try_next()?
            .ok_or_else(|| "missing host fixture application work".into())
    }

    async fn open(&mut self) -> DesktopNativeHostPuzzleTestResult<ScenePickingViewport> {
        self.state
            .presentation
            .mount_puzzles(PresentationPuzzlePanelConnection {
                owner: DesktopMessagesEntityId::new(),
                state: self.selected.subscribe(),
            });
        self.state.presentation.select_workspace(
            PresentationAppEnumWorkspace::Puzzles,
            &self.application.port(),
        )?;
        let inventory = self.next()?;
        if !matches!(
            inventory.request().command().operation(),
            DesktopEnumCommand::Puzzle {
                operation: DesktopPuzzleServiceEnumOperation::Inventory { after: None },
                ..
            }
        ) {
            return Err("expected host fixture inventory request".into());
        }
        let page = Arc::new(self.repository.inventory(None).await?);
        self.application.begin_commit(&inventory)?;
        self.application.complete_value(
            &inventory,
            DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Inventory(page))),
            None,
        )?;
        self.state
            .presentation
            .synchronize_puzzles(&self.application.port())?;
        self.presentation_frame(Vec::new())?;
        let (output, _viewport) = self.presentation_frame(Vec::new())?;
        self.presentation_frame(vec![Self::click(&output, "Open")?])?;
        let opened = self.next()?;
        if !matches!(opened.request().command().operation(), DesktopEnumCommand::Puzzle {
            operation: DesktopPuzzleServiceEnumOperation::OpenSaved { id, .. }, ..
        } if *id == self.snapshot.context().session_id())
        {
            return Err("expected host fixture saved-run open request".into());
        }
        self.application.begin_commit(&opened)?;
        self.application.complete_value(
            &opened,
            DesktopEnumValue::Puzzle(Box::new(DesktopPuzzleServiceEnumResult::Opened(
                Arc::clone(&self.snapshot),
            ))),
            None,
        )?;
        self.state
            .presentation
            .synchronize_puzzles(&self.application.port())?;
        self.presentation_frame(Vec::new())?;
        let (_output, viewport) = self.presentation_frame(Vec::new())?;
        viewport.ok_or_else(|| "missing host fixture puzzle viewport".into())
    }

    async fn observe(
        &mut self,
        geometry: DesktopNativeHostPuzzleTestEnumGeometry,
        deferral: DesktopNativeHostPuzzleTestEnumDeferral,
    ) -> DesktopNativeHostPuzzleTestResult<DesktopNativeHostPuzzleTestObservation> {
        let viewport = self.open().await?;
        let board = self
            .state
            .presentation
            .board_snapshot()
            .ok_or("missing visible host fixture board")?
            .clone();
        self.state.rendered_board = Some(board.clone());
        self.state.puzzle_window = Some(DesktopNativePuzzleWindow::new(
            self.window,
            self.state.services.puzzles.clone(),
        )?);
        if let DesktopNativeHostPuzzleTestEnumGeometry::Camera = geometry {
            let mut renderer = self
                .world
                .remove_resource::<SceneRenderer>()
                .ok_or("missing host fixture renderer")?;
            renderer.update_view(&mut self.world, viewport, self.state.camera)?;
            renderer.mark_ready(renderer.generation())?;
            self.world.insert_resource(renderer);
            self.state.pending_camera = Some(DesktopNativeHost::camera_action(
                self.state.camera,
                PresentationBoardEnumCameraAction::RotateLeft,
            ));
        }
        let awaiting_readiness = self
            .state
            .presentation
            .puzzle_panel()
            .and_then(|panel| panel.controller())
            .is_some_and(|controller| {
                controller.snapshot().status() == PuzzleRunEnumStatus::AwaitingReadiness
            });
        let current_board_applied = self
            .world
            .resource::<SceneRenderer>()
            .piece_root("e2".parse()?)
            .is_some()
            && self.state.rendered_board.as_ref().is_some_and(|rendered| {
                rendered.fen() == board.fen() && rendered.variant() == board.variant()
            });
        let started_ready = self.state.ready;
        let capacity = Arc::clone(&self.state.services.capacity);
        if let DesktopNativeHostPuzzleTestEnumDeferral::PreparationUnacknowledged = deferral {
            capacity
                .lock()
                .map_err(|_poisoned| "host fixture capacity poisoned")?
                .request_preparation()?;
        }
        self.state.services.wake.acknowledge();
        self.state.services.port.acknowledge_wake();
        let held = match deferral {
            DesktopNativeHostPuzzleTestEnumDeferral::CapacityContended => Some(
                capacity
                    .lock()
                    .map_err(|_poisoned| "host fixture capacity poisoned")?,
            ),
            DesktopNativeHostPuzzleTestEnumDeferral::PreparationUnacknowledged => None,
        };
        self.context.begin_pass(Self::input(Vec::new()));
        let outcome = DesktopNativeHost::present_state(&mut self.world, &mut self.state);
        let mut output = self.context.end_pass();
        output.textures_delta.clear();
        drop(held);
        outcome?;
        let preparation_deferred = capacity
            .lock()
            .map_err(|_poisoned| "host fixture capacity poisoned")?
            .state()
            == match deferral {
                DesktopNativeHostPuzzleTestEnumDeferral::CapacityContended => {
                    CapacityPhaseEnumState::Steady
                }
                DesktopNativeHostPuzzleTestEnumDeferral::PreparationUnacknowledged => {
                    CapacityPhaseEnumState::Quiescing
                }
            };
        let renderer = self.world.resource::<SceneRenderer>();
        let square = PresentationBoardSemanticGrid::square_id("e2".parse()?).accesskit_id();
        let has_board_semantics = output
            .platform_output
            .accesskit_update
            .as_ref()
            .is_some_and(|update| update.nodes.iter().any(|(id, _node)| *id == square));
        let mut ready_requested = false;
        let mut other_work_requested = false;
        while let Some(work) = self.application.try_next()? {
            let ready = matches!(
                work.request().command().operation(),
                DesktopEnumCommand::Puzzle {
                    operation: DesktopPuzzleServiceEnumOperation::Session {
                        operation: DesktopPuzzleBridgeEnumOperation::Command(
                            PuzzlePersistenceEnumCommand::Readiness(
                                PuzzleTimerEnumReadiness::Ready
                            )
                        ),
                        ..
                    },
                    ..
                }
            );
            ready_requested |= ready;
            other_work_requested |= !ready;
        }
        Ok(DesktopNativeHostPuzzleTestObservation {
            awaiting_readiness,
            current_board_applied,
            started_ready,
            preparation_deferred,
            pending_view: self.state.pending_view.is_some(),
            pending_camera: self.state.pending_camera.is_some(),
            viewport_matches: self
                .state
                .pending_view
                .is_none_or(|pending| pending == renderer.viewport()),
            wake_rearmed: self.state.services.wake.is_pending(),
            has_board_semantics,
            ready_requested,
            other_work_requested,
            presentation_failed: self.state.presentation.failure().is_some(),
        })
    }

    async fn close(mut self) -> DesktopNativeHostPuzzleTestResult {
        drop((
            self.state,
            self.world,
            self.context,
            self.application,
            self.selected,
            self.snapshot,
        ));
        self.service
            .shutdown_retained(Instant::now() + Self::DEADLINE)
            .await?;
        let service_closed = self.service.is_shutdown_complete();
        drop((self.service, self.repository));
        self.storage
            .close_retained(Instant::now() + Self::DEADLINE)
            .await?;
        let storage_closed = self.storage.is_closed();
        drop(self.storage);
        Arc::try_unwrap(self.executor)
            .map_err(|_executor| "host fixture retained shared executor")?
            .shutdown()
            .await?;
        self.directory.close()?;
        assert!(service_closed);
        assert!(storage_closed);
        Ok(())
    }

    async fn check(
        geometry: DesktopNativeHostPuzzleTestEnumGeometry,
        deferral: DesktopNativeHostPuzzleTestEnumDeferral,
    ) -> DesktopNativeHostPuzzleTestResult {
        let mut fixture = Self::new().await?;
        let observation = fixture.observe(geometry, deferral).await;
        let closed = Box::pin(fixture.close()).await;
        closed?;
        let observation = observation?;
        assert!(observation.awaiting_readiness);
        assert!(observation.current_board_applied);
        assert!(observation.started_ready);
        assert!(observation.preparation_deferred);
        assert!(observation.wake_rearmed);
        assert!(!observation.other_work_requested);
        assert!(!observation.presentation_failed);
        match geometry {
            DesktopNativeHostPuzzleTestEnumGeometry::Viewport => {
                assert!(observation.pending_view);
                assert!(!observation.viewport_matches);
                assert!(!observation.has_board_semantics);
            }
            DesktopNativeHostPuzzleTestEnumGeometry::Camera => {
                assert!(observation.pending_camera);
                assert!(!observation.pending_view);
                assert!(observation.viewport_matches);
            }
        }
        assert!(
            !observation.ready_requested,
            "deferred native geometry queued puzzle Ready while its applied frame was stale"
        );
        Ok(())
    }
}

#[tokio::test]
async fn puzzle_countdown_waits_when_viewport_preparation_is_capacity_contended()
-> DesktopNativeHostPuzzleTestResult {
    Box::pin(DesktopNativeHostPuzzleFixture::check(
        DesktopNativeHostPuzzleTestEnumGeometry::Viewport,
        DesktopNativeHostPuzzleTestEnumDeferral::CapacityContended,
    ))
    .await
}

#[tokio::test]
async fn puzzle_countdown_waits_when_viewport_preparation_is_unacknowledged()
-> DesktopNativeHostPuzzleTestResult {
    Box::pin(DesktopNativeHostPuzzleFixture::check(
        DesktopNativeHostPuzzleTestEnumGeometry::Viewport,
        DesktopNativeHostPuzzleTestEnumDeferral::PreparationUnacknowledged,
    ))
    .await
}

#[tokio::test]
async fn puzzle_countdown_waits_when_camera_preparation_is_capacity_contended()
-> DesktopNativeHostPuzzleTestResult {
    Box::pin(DesktopNativeHostPuzzleFixture::check(
        DesktopNativeHostPuzzleTestEnumGeometry::Camera,
        DesktopNativeHostPuzzleTestEnumDeferral::CapacityContended,
    ))
    .await
}
