use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::{Duration, Instant};

use bevy::app::{First, Last, OnAppExitSystems, Startup, Update};
use bevy::asset::{
    AssetEvent, AssetId, AssetLoadError, AssetPlugin, AssetServer, Assets, Handle, LoadState,
    RecursiveDependencyLoadState,
};
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::ecs::system::SystemState;
use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::pbr::{MeshMaterial3d, PreparedMaterial};
use bevy::picking::mesh_picking::ray_cast::MeshRayCast;
use bevy::prelude::{
    App, AppExit, ButtonInput, Camera2d, DefaultPlugins, Entity, IntoScheduleConfigs, KeyCode,
    Mesh, Mesh3d, Or, Plugin, PluginGroup, Res, ResMut, Resource, StandardMaterial, With, World,
};
use bevy::render::erased_render_asset::ErasedRenderAssets;
use bevy::render::error_handler::{RenderError, RenderErrorHandler, RenderErrorPolicy};
use bevy::render::mesh::RenderMesh;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    CachedPipelineState, PipelineCache, SpecializedRenderPipelines,
};
use bevy::render::renderer::{
    RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue,
};
use bevy::render::settings::{RenderCreation, RenderResources};
use bevy::render::texture::GpuImage;
use bevy::render::view::window::{ExtractedWindow, SurfaceData};
use bevy::render::{Render, RenderApp, RenderPlugin, RenderStartup, RenderSystems};
use bevy::shader::ShaderCacheError;
use bevy::window::{
    ExitCondition, PrimaryWindow, RawHandleWrapper, Window, WindowCloseRequested, WindowPlugin,
    WindowResizeConstraints,
};
use bevy::winit::{EventLoopProxyWrapper, UpdateMode, WINIT_WINDOWS, WinitPlugin, WinitSettings};
use bevy_egui::render::EguiPipeline;
use bevy_egui::render::systems::{
    EguiPipelines, EguiRenderData, EguiTextureBindGroups, EguiTransforms,
};
use bevy_egui::{
    EguiContext, EguiPlugin, EguiPrimaryContextPass, EguiUserTextures, PrimaryEguiContext, egui,
};
use thiserror::Error;
use tokio::runtime::Handle as RuntimeHandle;
use tokio::sync::watch;

use crate::board::enums::BoardEnumColor;
use crate::board::position::{BoardPosition, BoardPositionError};
use crate::capacity::budget::CapacityBudget;
use crate::capacity::phase::{
    CapacityPhase, CapacityPhaseEnumState, CapacityPhaseError, CapacityPhaseGeneration,
};
use crate::clock::duration::ClockInstant;
use crate::clock::time_provider::ClockTimeProviderMonotonic;
use crate::desktop::application::{DesktopApplicationError, DesktopApplicationPort};
use crate::desktop::dto::DesktopBoardSnapshot;
use crate::desktop::messages::{DesktopCommand, DesktopEnumCommand, DesktopMessagesEnumResult};
use crate::desktop::native::dialogs::{
    DesktopNativeDialogs, DesktopNativeDialogsError, DesktopNativeDialogsPort,
    DesktopNativeDialogsRfd,
};
use crate::desktop::native::event_loop::{DesktopNativeEventLoopError, DesktopNativeEventLoopWake};
#[cfg(target_os = "macos")]
use crate::desktop::native::menu::{
    DesktopNativeMenu, DesktopNativeMenuEnumAction, DesktopNativeMenuError,
    DesktopNativeMenuRequests,
};
use crate::desktop::native::puzzle_service::DesktopNativePuzzleServicePort;
use crate::desktop::native::puzzle_window::{
    DesktopNativePuzzleWindow, DesktopNativePuzzleWindowError, DesktopNativePuzzleWindowFrame,
};
use crate::desktop::native::recovery::{
    DesktopNativeRecovery, DesktopNativeRecoveryEnumAction, DesktopNativeRecoveryEnumStage,
    DesktopNativeRecoveryError, DesktopNativeRecoveryFailure, DesktopNativeRecoveryProvider,
};
use crate::desktop::native::resources::DesktopNativeResources;
use crate::desktop::native::task_pools::{DesktopNativeTaskPools, DesktopNativeTaskPoolsError};
use crate::presentation::action::registry::PresentationActionEnumId;
use crate::presentation::app::{PresentationApp, PresentationAppError};
use crate::presentation::board::enums::PresentationBoardEnumCameraAction;
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::engine_panels::PresentationEnginePanelsConnection;
use crate::presentation::puzzle::panel::PresentationPuzzlePanelConnection;
use crate::presentation::theme::font::PresentationThemeFont;
use crate::scene::assets::{SceneAssets, SceneAssetsError};
use crate::scene::camera::{SceneCamera, SceneCameraError};
use crate::scene::picking::{ScenePickingViewport, ScenePickingViewportError};
use crate::scene::renderer::{SceneRenderer, SceneRendererError, SceneRendererGeneration};

#[cfg(test)]
#[path = "../../../tests/desktop/native/host_puzzles.rs"]
mod puzzle_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeHostEvent {
    Idle,
    PreparationRequested {
        generation: CapacityPhaseGeneration,
    },
    GraphicsPreparationStopped {
        generation: CapacityPhaseGeneration,
        received_at: ClockInstant,
    },
    FrameworkReady {
        generation: CapacityPhaseGeneration,
    },
    Interrupted {
        generation: CapacityPhaseGeneration,
    },
}

pub struct DesktopNativeHostServices {
    pub engine_panels: Option<PresentationEnginePanelsConnection>,
    pub puzzle_panel: Option<PresentationPuzzlePanelConnection>,
    pub puzzles: DesktopNativePuzzleServicePort,
    pub port: DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
    pub wake: Arc<DesktopNativeEventLoopWake>,
    pub resources: DesktopNativeResources,
    pub capacity: Arc<Mutex<CapacityPhase>>,
    pub runtime: RuntimeHandle,
    pub recovery: Arc<dyn DesktopNativeRecoveryProvider>,
    pub events: watch::Sender<DesktopNativeHostEvent>,
    pub budget: CapacityBudget,
    pub dialog_ready:
        watch::Sender<Option<DesktopNativeDialogsPort<Arc<DesktopNativeEventLoopWake>>>>,
}

#[derive(Debug, Error)]
pub enum DesktopNativeHostError {
    #[error("The native resource directory is not valid UTF-8: {0}")]
    ResourcePath(PathBuf),
    #[error("The shared native capacity phase is busy")]
    CapacityBusy,
    #[error("The shared native capacity phase is poisoned")]
    CapacityPoisoned,
    #[error("Native initialization requires an acknowledged preparation phase")]
    PreparationRequired,
    #[error("No compiled native GPU backend is available")]
    BackendUnavailable,
    #[error("Native graphics adapter selection failed: {0}")]
    Adapter(#[source] wgpu::RequestAdapterError),
    #[error("Native graphics device initialization failed: {0}")]
    Device(#[source] wgpu::RequestDeviceError),
    #[error("The native window surface could not be created: {0}")]
    Surface(#[source] wgpu::CreateSurfaceError),
    #[error("No graphics adapter supports this native window surface")]
    IncompatibleSurface,
    #[error("The native host requires one primary window")]
    PrimaryWindow,
    #[error("The native host requires its presentation state")]
    MissingState,
    #[error("The native host requires initialized rendering resources")]
    MissingRenderer,
    #[error("The piece asset failed to load: {0}")]
    AssetLoad(#[source] Arc<AssetLoadError>),
    #[error("Asset Retry requires a fresh piece asset identity")]
    AssetRetryIdentity,
    #[error("Native graphics pipeline compilation failed: {0}")]
    Pipeline(#[source] Arc<ShaderCacheError>),
    #[error("Native graphics did not become ready within {budget:?}")]
    ReadinessTimeout {
        budget: Duration,
        #[source]
        source: Option<tokio::time::error::Elapsed>,
    },
    #[error("Native graphics failed ({kind:?}): {description}")]
    Graphics {
        kind: bevy::render::error_handler::ErrorType,
        description: String,
    },
    #[error("Native graphics startup remains paused: {source}")]
    StartupPaused {
        #[source]
        source: Box<DesktopNativeHostError>,
    },
    #[error("The native application refused a safe quit: {0:?}")]
    QuitRefused(Box<DesktopMessagesEnumResult>),
    #[error("The native event loop exited unsuccessfully: {0:?}")]
    EventLoop(AppExit),
    #[error(transparent)]
    Wake(#[from] DesktopNativeEventLoopError),
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
    #[error(transparent)]
    Capacity(#[from] CapacityPhaseError),
    #[error(transparent)]
    Recovery(#[from] DesktopNativeRecoveryError),
    #[error(transparent)]
    TaskPools(#[from] DesktopNativeTaskPoolsError),
    #[error(transparent)]
    Assets(#[from] SceneAssetsError),
    #[error(transparent)]
    Scene(#[from] SceneRendererError),
    #[error(transparent)]
    Camera(#[from] SceneCameraError),
    #[error(transparent)]
    Viewport(#[from] ScenePickingViewportError),
    #[error(transparent)]
    Position(#[from] BoardPositionError),
    #[error(transparent)]
    Presentation(#[from] PresentationAppError),
    #[error("Native system resources are unavailable: {0}")]
    System(#[from] bevy::ecs::system::SystemParamValidationError),
    #[error(transparent)]
    Dialogs(#[from] DesktopNativeDialogsError),
    #[error(transparent)]
    PuzzleWindow(#[from] DesktopNativePuzzleWindowError),
    #[cfg(target_os = "macos")]
    #[error(transparent)]
    Menu(#[from] DesktopNativeMenuError),
}

#[derive(Resource)]
struct DesktopNativeHostState {
    services: DesktopNativeHostServices,
    presentation: PresentationApp,
    puzzle_window: Option<DesktopNativePuzzleWindow<DesktopNativePuzzleServicePort>>,
    asset: Handle<Gltf>,
    asset_reload: Option<DesktopNativeHostEnumAssetReload>,
    camera: SceneCamera,
    rendered_board: Option<DesktopBoardSnapshot>,
    generation: CapacityPhaseGeneration,
    ready: bool,
    fonts_initialized: bool,
    recovery: Option<DesktopNativeRecovery>,
    graphics_drain: Option<DesktopNativeHostGraphicsDrain>,
    failure: Option<DesktopNativeHostError>,
    fault_received_at: Option<ClockInstant>,
    interruption_announced: bool,
    quit: DesktopNativeHostEnumQuit,
    teardown: DesktopNativeHostEnumTeardown,
    replacement: Option<RenderResources>,
    expected_device: Option<RenderDevice>,
    validated_surface: Option<Entity>,
    pending_view: Option<ScenePickingViewport>,
    pending_camera: Option<SceneCamera>,
    dialog_ready: bool,
    dialog_admission_closed: bool,
    exit_authorized: bool,
    readiness: Option<DesktopNativeHostReadiness>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopNativeHostEnumQuit {
    Idle,
    Requested,
    Submitted,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopNativeHostEnumTeardown {
    Active,
    Draining,
    Observed,
    Admitted,
}

impl DesktopNativeHostEnumTeardown {
    fn begin(
        &mut self,
        capacity: &Mutex<CapacityPhase>,
        port: &DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
    ) -> Result<bool, DesktopNativeHostError> {
        let Some(mut phase) = DesktopNativeHost::capacity_phase(capacity, port)? else {
            return Ok(false);
        };
        phase.request_teardown()?;
        *self = Self::Draining;
        Ok(true)
    }

    fn observe_completed_frame(&mut self) {
        if *self == Self::Draining {
            *self = Self::Observed;
        }
    }

    fn try_admit(
        &mut self,
        capacity: &Mutex<CapacityPhase>,
        port: &DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
    ) -> Result<bool, DesktopNativeHostError> {
        match self {
            Self::Active | Self::Draining => {
                port.rearm_wake()?;
                Ok(false)
            }
            Self::Observed => {
                let Some(mut phase) = DesktopNativeHost::capacity_phase(capacity, port)? else {
                    return Ok(false);
                };
                if phase.state() == CapacityPhaseEnumState::Draining
                    && let Some(generation) = phase.generation()
                {
                    phase.acknowledge_preparation_stopped(generation)?;
                }
                phase.begin_teardown()?;
                *self = Self::Admitted;
                Ok(true)
            }
            Self::Admitted => Ok(true),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopNativeHostEnumGraphicsDrainStage {
    Draining,
    Observed,
    Announced,
}

struct DesktopNativeHostGraphicsDrain {
    generation: CapacityPhaseGeneration,
    received_at: ClockInstant,
    stage: DesktopNativeHostEnumGraphicsDrainStage,
}

enum DesktopNativeHostEnumAssetReload {
    Retiring(AssetId<Gltf>),
    Loading(DesktopNativeHostAssetReload),
}

impl DesktopNativeHostEnumAssetReload {
    fn retire(asset: &mut Handle<Gltf>, previous: Option<Self>) -> Self {
        let id = match previous {
            Some(Self::Retiring(id)) => id,
            Some(Self::Loading(_)) | None => std::mem::take(asset).id(),
        };
        Self::Retiring(id)
    }

    fn start(
        &mut self,
        id: AssetId<Gltf>,
        events: &Messages<AssetEvent<Gltf>>,
        root: &LoadState,
        recursive: &RecursiveDependencyLoadState,
    ) -> Result<(), DesktopNativeHostError> {
        match self {
            Self::Retiring(previous) if *previous == id => {
                Err(DesktopNativeHostError::AssetRetryIdentity)
            }
            Self::Retiring(_) => {
                *self = Self::Loading(DesktopNativeHostAssetReload::new(events, root, recursive));
                Ok(())
            }
            Self::Loading(_) => Err(DesktopNativeHostError::MissingState),
        }
    }
}

struct DesktopNativeHostAssetReload {
    events: MessageCursor<AssetEvent<Gltf>>,
    previous_failure: Option<Arc<AssetLoadError>>,
    refreshed: bool,
}

impl DesktopNativeHostAssetReload {
    fn new(
        events: &Messages<AssetEvent<Gltf>>,
        root: &LoadState,
        recursive: &RecursiveDependencyLoadState,
    ) -> Self {
        Self {
            events: events.get_cursor_current(),
            previous_failure: DesktopNativeHost::asset_failure(root, recursive),
            refreshed: false,
        }
    }

    fn observe(
        &mut self,
        id: AssetId<Gltf>,
        events: &Messages<AssetEvent<Gltf>>,
        root: &LoadState,
        recursive: &RecursiveDependencyLoadState,
    ) -> Result<bool, DesktopNativeHostError> {
        self.refreshed |= self.events.read(events).any(|event| {
            event.is_added(id) || event.is_modified(id) || event.is_loaded_with_dependencies(id)
        });
        if let Some(error) = DesktopNativeHost::asset_failure(root, recursive) {
            if !self.refreshed
                && self
                    .previous_failure
                    .as_ref()
                    .is_some_and(|previous| Arc::ptr_eq(previous, &error))
            {
                return Ok(false);
            }
            return Err(DesktopNativeHostError::AssetLoad(error));
        }
        Ok(self.refreshed && root.is_loaded() && recursive.is_loaded())
    }
}

impl DesktopNativeHostGraphicsDrain {
    fn new(generation: CapacityPhaseGeneration, received_at: ClockInstant) -> Self {
        Self {
            generation,
            received_at,
            stage: DesktopNativeHostEnumGraphicsDrainStage::Draining,
        }
    }

    fn observe_completed_frame(&mut self) {
        if self.stage == DesktopNativeHostEnumGraphicsDrainStage::Draining {
            self.stage = DesktopNativeHostEnumGraphicsDrainStage::Observed;
        }
    }

    fn take_stopped_event(&mut self) -> Option<DesktopNativeHostEvent> {
        if self.stage != DesktopNativeHostEnumGraphicsDrainStage::Observed {
            return None;
        }
        self.stage = DesktopNativeHostEnumGraphicsDrainStage::Announced;
        Some(DesktopNativeHostEvent::GraphicsPreparationStopped {
            generation: self.generation,
            received_at: self.received_at,
        })
    }

    fn fresh_preparation(
        &self,
        capacity: &Mutex<CapacityPhase>,
        port: &DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
    ) -> Result<Option<CapacityPhaseGeneration>, DesktopNativeHostError> {
        if self.stage != DesktopNativeHostEnumGraphicsDrainStage::Announced {
            return Ok(None);
        }
        let Some(phase) = DesktopNativeHost::capacity_phase(capacity, port)? else {
            return Ok(None);
        };
        Ok(phase.generation().filter(|generation| {
            phase.state() == CapacityPhaseEnumState::Preparing && *generation != self.generation
        }))
    }
}

#[derive(Resource, Clone)]
struct DesktopNativeHostRenderProbe {
    target: Handle<bevy::prelude::Image>,
    generation: SceneRendererGeneration,
    meshes: Vec<AssetId<Mesh>>,
    materials: Vec<AssetId<StandardMaterial>>,
    ready: Arc<AtomicBool>,
    failure: Arc<OnceLock<Arc<ShaderCacheError>>>,
}

impl DesktopNativeHostRenderProbe {
    fn permanent_pipeline_failure(error: &ShaderCacheError) -> Option<ShaderCacheError> {
        match error {
            ShaderCacheError::ProcessShaderError(message) => {
                Some(ShaderCacheError::ProcessShaderError(message.clone()))
            }
            ShaderCacheError::CreateShaderModule(message) => {
                Some(ShaderCacheError::CreateShaderModule(message.clone()))
            }
            ShaderCacheError::ShaderNotLoaded(_)
            | ShaderCacheError::ShaderImportNotYetAvailable => None,
        }
    }
}

#[derive(Clone, Copy)]
struct DesktopNativeHostReadiness {
    started: Instant,
}

impl DesktopNativeHostReadiness {
    const BUDGET: Duration = Duration::from_secs(10);

    const fn new_at(started: Instant) -> Self {
        Self { started }
    }

    fn timed_out(source: Option<tokio::time::error::Elapsed>) -> DesktopNativeHostError {
        DesktopNativeHostError::ReadinessTimeout {
            budget: Self::BUDGET,
            source,
        }
    }

    fn remaining_at(self, now: Instant) -> Result<Duration, DesktopNativeHostError> {
        Self::BUDGET
            .checked_sub(now.duration_since(self.started))
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| Self::timed_out(None))
    }

    fn wait<T>(
        self,
        runtime: &RuntimeHandle,
        operation: impl Future<Output = T>,
    ) -> Result<T, DesktopNativeHostError> {
        runtime.block_on(async {
            tokio::time::timeout(self.remaining_at(Instant::now())?, operation)
                .await
                .map_err(|source| Self::timed_out(Some(source)))
        })
    }
}

type DesktopNativeHostDialogs =
    DesktopNativeDialogs<DesktopNativeDialogsRfd, Arc<DesktopNativeEventLoopWake>>;

pub struct DesktopNativeHost;

impl DesktopNativeHost {
    const TITLE: &str = "Chess";
    const RECOVERY_TITLE: &str = "Chess: graphics paused. Close window for recovery options";
    const WIDTH: u16 = 1280;
    const HEIGHT: u16 = 800;
    const INITIAL_BOARD_SIZE: f32 = 640.0;

    pub fn event_channel() -> (
        watch::Sender<DesktopNativeHostEvent>,
        watch::Receiver<DesktopNativeHostEvent>,
    ) {
        watch::channel(DesktopNativeHostEvent::Idle)
    }

    pub fn run(services: DesktopNativeHostServices) -> Result<(), DesktopNativeHostError> {
        Self::run_with_plugin(services, |_app: &mut App| {})
    }

    pub fn run_with_plugin(
        mut services: DesktopNativeHostServices,
        plugin: impl Plugin,
    ) -> Result<(), DesktopNativeHostError> {
        let generation = Self::preparing_generation(&services)?;
        let Some((render_resources, readiness)) = Self::startup_graphics(&services, generation)?
        else {
            return Ok(());
        };
        let asset_path = services
            .resources
            .root()
            .to_str()
            .ok_or_else(|| {
                DesktopNativeHostError::ResourcePath(services.resources.root().to_path_buf())
            })?
            .to_owned();
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(DesktopNativeTaskPools::plugin())
                .set(AssetPlugin {
                    file_path: asset_path,
                    watch_for_changes_override: Some(false),
                    ..AssetPlugin::default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: Self::TITLE.to_owned(),
                        resolution: (u32::from(Self::WIDTH), u32::from(Self::HEIGHT)).into(),
                        resize_constraints: WindowResizeConstraints {
                            min_width: f32::from(Self::WIDTH),
                            min_height: f32::from(Self::HEIGHT),
                            ..WindowResizeConstraints::default()
                        },
                        ..Window::default()
                    }),
                    close_when_requested: false,
                    exit_condition: ExitCondition::DontExit,
                    ..WindowPlugin::default()
                })
                .set(WinitPlugin {
                    run_on_any_thread: false,
                })
                .set(RenderPlugin {
                    render_creation: render_resources.into(),
                    synchronous_pipeline_compilation: true,
                    ..RenderPlugin::default()
                })
                .disable::<PipelinedRenderingPlugin>(),
        )
        .add_plugins(EguiPlugin {
            bindless_mode_array_size: if cfg!(target_os = "macos") {
                None
            } else {
                EguiPlugin::default().bindless_mode_array_size
            },
        })
        .insert_resource(WinitSettings::continuous())
        .insert_resource(RenderErrorHandler(Self::render_error))
        .add_systems(Startup, Self::setup)
        .add_systems(First, Self::acknowledge_wake)
        .add_systems(Update, Self::update)
        .add_systems(Last, Self::guard_exit.before(OnAppExitSystems))
        .add_systems(EguiPrimaryContextPass, Self::present);
        DesktopNativeTaskPools::validate()?;
        let proxy = std::ops::Deref::deref(app.world().resource::<EventLoopProxyWrapper>()).clone();
        services.wake.attach(proxy)?;
        let asset = app
            .world()
            .resource::<AssetServer>()
            .load(DesktopNativeResources::PIECES_ASSET);
        let mut presentation = PresentationApp::new();
        if let Some(connection) = services.engine_panels.take() {
            presentation.mount_engine(connection);
        }
        if let Some(connection) = services.puzzle_panel.take() {
            presentation.mount_puzzles(connection);
        }
        app.insert_resource(DesktopNativeHostState {
            services,
            presentation,
            puzzle_window: None,
            asset,
            asset_reload: None,
            camera: SceneCamera::initial(BoardEnumColor::White),
            rendered_board: None,
            generation,
            ready: false,
            fonts_initialized: false,
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
            readiness: Some(readiness),
        });
        let render_app = app
            .get_sub_app_mut(RenderApp)
            .ok_or(DesktopNativeHostError::MissingRenderer)?;
        let mut extract = render_app
            .take_extract()
            .ok_or(DesktopNativeHostError::MissingRenderer)?;
        render_app
            .add_systems(
                Render,
                Self::observe_render.in_set(RenderSystems::PostCleanup),
            )
            .add_systems(RenderStartup, Self::reset_graphics)
            .set_extract(move |main, render| {
                Self::validate_surface(main, render);
                if let Some(renderer) = main.get_resource::<SceneRenderer>() {
                    let current = render.get_resource::<DesktopNativeHostRenderProbe>();
                    if !current.is_some_and(|current| current.generation == renderer.generation()) {
                        let target = renderer.image().clone();
                        let generation = renderer.generation();
                        let mut meshes = main
                            .query::<&Mesh3d>()
                            .iter(main)
                            .map(|mesh| mesh.0.id())
                            .collect::<Vec<_>>();
                        meshes.sort_unstable();
                        meshes.dedup();
                        let mut materials = main
                            .query::<&MeshMaterial3d<StandardMaterial>>()
                            .iter(main)
                            .map(|material| material.0.id())
                            .collect::<Vec<_>>();
                        materials.sort_unstable();
                        materials.dedup();
                        let probe = DesktopNativeHostRenderProbe {
                            target,
                            generation,
                            meshes,
                            materials,
                            ready: Arc::new(AtomicBool::new(false)),
                            failure: Arc::new(OnceLock::new()),
                        };
                        main.insert_resource(probe.clone());
                        render.insert_resource(probe);
                    }
                }
                extract(main, render);
            });
        app.add_plugins(plugin);
        let exit = app.run();
        if exit.is_success() {
            Ok(())
        } else {
            Err(DesktopNativeHostError::EventLoop(exit))
        }
    }

    fn preparing_generation(
        services: &DesktopNativeHostServices,
    ) -> Result<CapacityPhaseGeneration, DesktopNativeHostError> {
        let capacity = services.capacity.try_lock().map_err(Self::capacity_lock)?;
        if capacity.state() != CapacityPhaseEnumState::Preparing {
            return Err(DesktopNativeHostError::PreparationRequired);
        }
        capacity
            .generation()
            .ok_or(DesktopNativeHostError::PreparationRequired)
    }

    fn capacity_lock<T>(error: TryLockError<T>) -> DesktopNativeHostError {
        match error {
            TryLockError::WouldBlock => DesktopNativeHostError::CapacityBusy,
            TryLockError::Poisoned(_error) => DesktopNativeHostError::CapacityPoisoned,
        }
    }

    fn capacity_phase<'a>(
        capacity: &'a Mutex<CapacityPhase>,
        port: &DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
    ) -> Result<Option<MutexGuard<'a, CapacityPhase>>, DesktopNativeHostError> {
        match capacity.try_lock() {
            Ok(phase) => Ok(Some(phase)),
            Err(TryLockError::WouldBlock) => {
                port.rearm_wake()?;
                Ok(None)
            }
            Err(TryLockError::Poisoned(_error)) => Err(DesktopNativeHostError::CapacityPoisoned),
        }
    }

    fn dialog_result<T>(
        port: &DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
        result: Result<T, DesktopNativeDialogsError>,
    ) -> Result<Option<T>, DesktopNativeHostError> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(DesktopNativeDialogsError::Busy) => {
                port.rearm_wake()?;
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn startup_graphics(
        services: &DesktopNativeHostServices,
        generation: CapacityPhaseGeneration,
    ) -> Result<Option<(RenderResources, DesktopNativeHostReadiness)>, DesktopNativeHostError> {
        loop {
            let readiness = DesktopNativeHostReadiness::new_at(Instant::now());
            match Self::graphics(&services.runtime, None, readiness) {
                Ok(resources) => return Ok(Some((resources, readiness))),
                Err(source) => {
                    let mut recovery =
                        DesktopNativeRecovery::new(Self::failure(&source), generation);
                    recovery.acknowledge(generation)?;
                    match recovery.choose(services.recovery.as_ref())? {
                        DesktopNativeRecoveryEnumAction::Retry => {}
                        DesktopNativeRecoveryEnumAction::SafeQuit => {
                            services
                                .runtime
                                .block_on(Self::quit_before_window(services))?;
                            return Ok(None);
                        }
                        DesktopNativeRecoveryEnumAction::KeepPaused => {
                            return Err(DesktopNativeHostError::StartupPaused {
                                source: Box::new(source),
                            });
                        }
                    }
                }
            }
        }
    }

    async fn quit_before_window(
        services: &DesktopNativeHostServices,
    ) -> Result<(), DesktopNativeHostError> {
        let mut teardown = DesktopNativeHostEnumTeardown::Active;
        while !teardown.begin(&services.capacity, &services.port)? {
            services.wake.notified().await;
        }
        teardown.observe_completed_frame();
        while !teardown.try_admit(&services.capacity, &services.port)? {
            services.wake.notified().await;
        }
        let request = services
            .port
            .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
        let receipt = loop {
            match services.port.try_submit(&request) {
                Ok(receipt) => break receipt,
                Err(DesktopApplicationError::Busy) => services.wake.notified().await,
                Err(error) => return Err(error.into()),
            }
        };
        loop {
            services.port.acknowledge_wake();
            services.wake.acknowledge();
            match receipt.response() {
                Some(DesktopMessagesEnumResult::QuitReady) => return Ok(()),
                Some(response) => {
                    return Err(DesktopNativeHostError::QuitRefused(Box::new(
                        response.clone(),
                    )));
                }
                None => services.wake.notified().await,
            }
        }
    }

    fn graphics(
        runtime: &RuntimeHandle,
        window: Option<Entity>,
        readiness: DesktopNativeHostReadiness,
    ) -> Result<RenderResources, DesktopNativeHostError> {
        let backends = wgpu::Backends::PRIMARY & wgpu::Instance::enabled_backend_features();
        if backends.is_empty() {
            return Err(DesktopNativeHostError::BackendUnavailable);
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = if let Some(window) = window {
            let adapters = readiness.wait(runtime, instance.enumerate_adapters(backends))?;
            WINIT_WINDOWS.with_borrow(|windows| {
                let window = windows
                    .get_window(window)
                    .ok_or(DesktopNativeHostError::PrimaryWindow)?;
                let surface = instance
                    .create_surface(&**window)
                    .map_err(DesktopNativeHostError::Surface)?;
                adapters
                    .into_iter()
                    .find(|adapter| {
                        adapter.is_surface_supported(&surface)
                            && !surface.get_capabilities(adapter).formats.is_empty()
                    })
                    .ok_or(DesktopNativeHostError::IncompatibleSurface)
            })?
        } else {
            readiness
                .wait(
                    runtime,
                    instance.request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::HighPerformance,
                        force_fallback_adapter: false,
                        compatible_surface: None,
                        ..wgpu::RequestAdapterOptions::default()
                    }),
                )?
                .map_err(DesktopNativeHostError::Adapter)?
        };
        let info = adapter.get_info();
        let mut features = adapter.features() - wgpu::Features::all_experimental_mask();
        if info.device_type == wgpu::DeviceType::DiscreteGpu {
            features.remove(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);
        }
        let descriptor = wgpu::DeviceDescriptor {
            label: Some("chess.native.device"),
            required_features: features,
            required_limits: adapter.limits(),
            ..wgpu::DeviceDescriptor::default()
        };
        let (device, queue) = readiness
            .wait(runtime, adapter.request_device(&descriptor))?
            .map_err(DesktopNativeHostError::Device)?;
        Ok(RenderResources(
            RenderDevice::new(device),
            RenderQueue::new(queue),
            RenderAdapterInfo::new(info),
            RenderAdapter::new(adapter),
            RenderInstance::new(instance),
        ))
    }

    fn setup(world: &mut World) {
        world.spawn(Camera2d);
        let outcome = world.resource_scope(
            |_world, mut state: bevy::prelude::Mut<'_, DesktopNativeHostState>| {
                let state = &mut *state;
                state.presentation.activate(
                    PresentationActionEnumId::Subscribe,
                    0,
                    &state.services.port,
                )
            },
        );
        if let Err(error) = outcome {
            Self::record_failure(world, DesktopNativeHostError::Presentation(error));
        }
    }

    fn acknowledge_wake(mut state: ResMut<'_, DesktopNativeHostState>) {
        state.services.wake.acknowledge();
        state.services.port.acknowledge_wake();
        state.teardown.observe_completed_frame();
        if let Some(drain) = &mut state.graphics_drain {
            drain.observe_completed_frame();
        }
    }

    fn update(world: &mut World) {
        let Some(mut state) = world.remove_resource::<DesktopNativeHostState>() else {
            return;
        };
        let outcome = Self::update_state(world, &mut state);
        world.insert_resource(state);
        if let Err(error) = outcome {
            Self::record_failure(world, error);
        }
    }

    fn update_state(
        world: &mut World,
        state: &mut DesktopNativeHostState,
    ) -> Result<(), DesktopNativeHostError> {
        state.presentation.poll();
        Self::update_puzzles(world, state)?;
        Self::activate_window(world, state);
        #[cfg(target_os = "macos")]
        let menu_requests = Self::poll_menu(world, state)?;
        let close_requested = world
            .get_resource_mut::<Messages<WindowCloseRequested>>()
            .is_some_and(|mut messages| messages.drain().next().is_some());
        let quit_shortcut = world
            .get_resource::<ButtonInput<KeyCode>>()
            .is_some_and(Self::is_quit_shortcut);
        let close_requested = close_requested || quit_shortcut;
        #[cfg(target_os = "macos")]
        let close_requested = close_requested
            || menu_requests.is_requested(DesktopNativeMenuEnumAction::Quit)
            || menu_requests.is_requested(DesktopNativeMenuEnumAction::Recovery)
                && state.failure.is_some();
        if close_requested {
            if state.failure.is_some() {
                state.recovery = None;
                state.readiness = None;
                state.replacement = None;
                state.expected_device = None;
                Self::begin_interruption(world, state)?;
            } else {
                state.presentation.request_quit();
            }
        }
        Self::poll_dialogs(world, state)?;
        if state.quit != DesktopNativeHostEnumQuit::Idle {
            return Self::poll_quit(world, state);
        }
        if let Some(readiness) = state.readiness {
            readiness.remaining_at(Instant::now())?;
            if let Some(probe) = world.get_resource::<DesktopNativeHostRenderProbe>()
                && let Some(error) = probe.failure.get()
            {
                return Err(DesktopNativeHostError::Pipeline(Arc::clone(error)));
            }
        }
        if state.failure.is_some() && state.recovery.is_none() && state.readiness.is_none() {
            Self::begin_interruption(world, state)?;
        }
        if state.graphics_drain.is_some() {
            return Self::poll_graphics_handoff(world, state);
        }
        if state.presentation.is_quit_ready() {
            Self::try_exit(world, state)?;
            return Ok(());
        }
        if state.recovery.is_some() {
            return Self::recover(world, state);
        }
        if state.presentation.snapshot().is_none() {
            return Ok(());
        }
        if !world.contains_resource::<SceneRenderer>() {
            Self::spawn_scene(world, state)?;
        }
        if let Some(mut renderer) = world.remove_resource::<SceneRenderer>() {
            let outcome = Self::update_scene(world, state, &mut renderer);
            world.insert_resource(renderer);
            outcome?;
        }
        Ok(())
    }

    fn update_puzzles(
        world: &mut World,
        state: &mut DesktopNativeHostState,
    ) -> Result<(), DesktopNativeHostError> {
        if state.puzzle_window.is_none() {
            let mut windows = world.query_filtered::<Entity, With<PrimaryWindow>>();
            let window = windows
                .single(world)
                .map_err(|_error| DesktopNativeHostError::PrimaryWindow)?;
            state.puzzle_window = Some(DesktopNativePuzzleWindow::new(
                window,
                state.services.puzzles.clone(),
            )?);
        }
        if let Some(window) = &mut state.puzzle_window {
            window.update(
                world,
                &mut state.presentation,
                &state.services.port,
                state.failure.is_some(),
            )?;
        }
        Ok(())
    }

    fn poll_quit(
        world: &mut World,
        state: &mut DesktopNativeHostState,
    ) -> Result<(), DesktopNativeHostError> {
        if state.quit == DesktopNativeHostEnumQuit::Requested {
            if !Self::begin_teardown(world, state)? {
                return Ok(());
            }
            match state.presentation.activate(
                PresentationActionEnumId::ConfirmQuit,
                0,
                &state.services.port,
            ) {
                Ok(_identity) => state.quit = DesktopNativeHostEnumQuit::Submitted,
                Err(PresentationAppError::Admission {
                    source: DesktopApplicationError::Busy,
                    ..
                }) => state.services.port.rearm_wake()?,
                Err(error) => return Err(error.into()),
            }
        }
        if state.presentation.is_quit_ready() {
            Self::try_exit(world, state)?;
        }
        Ok(())
    }

    fn begin_teardown(
        world: &mut World,
        state: &mut DesktopNativeHostState,
    ) -> Result<bool, DesktopNativeHostError> {
        if state.teardown == DesktopNativeHostEnumTeardown::Active {
            if !state
                .teardown
                .begin(&state.services.capacity, &state.services.port)?
            {
                return Ok(false);
            }
            if let Some(renderer) = world.remove_resource::<SceneRenderer>() {
                renderer.despawn(world)?;
            }
            world.remove_resource::<DesktopNativeHostRenderProbe>();
            state.ready = false;
            state.readiness = None;
            state.replacement = None;
            state.expected_device = None;
            state.pending_view = None;
            state.pending_camera = None;
            state.recovery = None;
            state.graphics_drain = None;
            state.asset_reload = None;
            world.insert_resource(WinitSettings::continuous());
            state.services.port.rearm_wake()?;
            return Ok(false);
        }
        let admitted = state
            .teardown
            .try_admit(&state.services.capacity, &state.services.port)?;
        if admitted {
            world.insert_resource(WinitSettings {
                focused_mode: UpdateMode::reactive_low_power(Duration::MAX),
                unfocused_mode: UpdateMode::reactive_low_power(Duration::MAX),
            });
        }
        Ok(admitted)
    }

    const fn quit_modifiers() -> [KeyCode; 2] {
        if cfg!(target_os = "macos") {
            [KeyCode::SuperLeft, KeyCode::SuperRight]
        } else {
            [KeyCode::ControlLeft, KeyCode::ControlRight]
        }
    }

    fn is_quit_shortcut(keys: &ButtonInput<KeyCode>) -> bool {
        keys.just_pressed(KeyCode::KeyQ)
            && Self::quit_modifiers()
                .into_iter()
                .any(|modifier| keys.pressed(modifier))
    }

    fn set_window_title(world: &mut World, title: &str) {
        let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
        if let Ok(mut window) = windows.single_mut(world)
            && window.title != title
        {
            window.title = title.to_owned();
        }
    }

    #[cfg(target_os = "macos")]
    fn poll_menu(
        world: &mut World,
        state: &DesktopNativeHostState,
    ) -> Result<DesktopNativeMenuRequests, DesktopNativeHostError> {
        if world.get_non_send::<DesktopNativeMenu>().is_none() {
            let mut query = world.query_filtered::<Entity, With<PrimaryWindow>>();
            let Ok(entity) = query.single(world) else {
                return Ok(DesktopNativeMenuRequests::default());
            };
            if !WINIT_WINDOWS.with_borrow(|windows| windows.get_window(entity).is_some()) {
                return Ok(DesktopNativeMenuRequests::default());
            }
            let mut menu = DesktopNativeMenu::install(Arc::clone(&state.services.wake))?;
            menu.set_recovery_enabled(state.failure.is_some());
            world.insert_non_send(menu);
        }
        world
            .get_non_send_mut::<DesktopNativeMenu>()
            .ok_or(DesktopNativeHostError::MissingState)?
            .poll()
            .map_err(DesktopNativeHostError::Menu)
    }

    #[cfg(target_os = "macos")]
    fn set_native_recovery_enabled(world: &mut World, enabled: bool) {
        if let Some(mut menu) = world.get_non_send_mut::<DesktopNativeMenu>() {
            menu.set_recovery_enabled(enabled);
        }
    }

    fn poll_dialogs(
        world: &mut World,
        state: &mut DesktopNativeHostState,
    ) -> Result<(), DesktopNativeHostError> {
        if !state.dialog_ready {
            let mut query = world.query_filtered::<Entity, With<PrimaryWindow>>();
            let Ok(entity) = query.single(world) else {
                return Ok(());
            };
            let backend = WINIT_WINDOWS
                .with_borrow(|windows| windows.get_window(entity).map(DesktopNativeDialogsRfd::new))
                .transpose()?;
            let Some(backend) = backend else {
                return Ok(());
            };
            let (dialogs, port) = DesktopNativeDialogs::new(
                backend,
                state.services.budget.clone(),
                Arc::clone(&state.services.wake),
            );
            world.insert_non_send(dialogs);
            state.dialog_ready = true;
            drop(state.services.dialog_ready.send_replace(Some(port)));
        }
        let result = world
            .get_non_send_mut::<DesktopNativeHostDialogs>()
            .ok_or(DesktopNativeHostError::MissingState)?
            .poll();
        Self::dialog_result(&state.services.port, result).map(|_state| ())
    }

    fn activate_window(world: &mut World, state: &DesktopNativeHostState) {
        let mut query = world.query_filtered::<Entity, With<PrimaryWindow>>();
        let Ok(entity) = query.single(world) else {
            return;
        };
        WINIT_WINDOWS.with_borrow(|windows| {
            let Some(window) = windows.get_window(entity) else {
                return;
            };
            if state.services.wake.take_activation_request() {
                window.set_visible(true);
                window.set_minimized(false);
                window.focus_window();
                window.request_user_attention(
                    (!window.has_focus())
                        .then_some(winit::window::UserAttentionType::Informational),
                );
            }
        });
    }

    fn try_exit(
        world: &mut World,
        state: &mut DesktopNativeHostState,
    ) -> Result<(), DesktopNativeHostError> {
        if state
            .puzzle_window
            .as_ref()
            .is_some_and(|window| !window.is_settled())
        {
            return Ok(());
        }
        if let Some(dialogs) = world.get_non_send::<DesktopNativeHostDialogs>() {
            if !state.dialog_admission_closed {
                if Self::dialog_result(&state.services.port, dialogs.close_admission())?.is_none() {
                    return Ok(());
                }
                state.dialog_admission_closed = true;
            }
            if Self::dialog_result(&state.services.port, dialogs.can_shutdown())? != Some(true) {
                return Ok(());
            }
        }
        state.exit_authorized = true;
        world.write_message(AppExit::Success);
        Ok(())
    }

    fn guard_exit(world: &mut World) {
        let Some(mut state) = world.remove_resource::<DesktopNativeHostState>() else {
            return;
        };
        if !state.exit_authorized
            && let Some(mut messages) = world.get_resource_mut::<Messages<AppExit>>()
            && !messages.is_empty()
        {
            messages.clear();
            state.presentation.request_quit();
        }
        world.insert_resource(state);
    }

    fn spawn_scene(
        world: &mut World,
        state: &mut DesktopNativeHostState,
    ) -> Result<(), DesktopNativeHostError> {
        let server = world.resource::<AssetServer>();
        if let Some(DesktopNativeHostEnumAssetReload::Retiring(previous)) = &state.asset_reload {
            if server.is_managed(*previous) {
                return Ok(());
            }
            let asset = server.load(DesktopNativeResources::PIECES_ASSET);
            let (root, _direct, recursive) = server
                .get_load_states(asset.id())
                .ok_or(DesktopNativeHostError::MissingState)?;
            state
                .asset_reload
                .as_mut()
                .ok_or(DesktopNativeHostError::MissingState)?
                .start(
                    asset.id(),
                    world.resource::<Messages<AssetEvent<Gltf>>>(),
                    &root,
                    &recursive,
                )?;
            state.asset = asset;
        }
        let Some((root, _direct, recursive)) = server.get_load_states(state.asset.id()) else {
            return Ok(());
        };
        if let Some(DesktopNativeHostEnumAssetReload::Loading(reload)) = &mut state.asset_reload {
            if !reload.observe(
                state.asset.id(),
                world.resource::<Messages<AssetEvent<Gltf>>>(),
                &root,
                &recursive,
            )? {
                return Ok(());
            }
            state.asset_reload = None;
        }
        if let Some(error) = Self::asset_failure(&root, &recursive) {
            return Err(DesktopNativeHostError::AssetLoad(error));
        }
        if !root.is_loaded() || !recursive.is_loaded() {
            return Ok(());
        }
        let mut assets = SystemState::<(
            Res<'_, Assets<Gltf>>,
            Res<'_, Assets<GltfNode>>,
            Res<'_, Assets<GltfMesh>>,
            Res<'_, Assets<GltfMaterial>>,
            bevy::prelude::ResMut<'_, Assets<StandardMaterial>>,
        )>::new(world);
        let (models, nodes, meshes, gltf_materials, mut materials) = assets.get_mut(world)?;
        let model = models
            .get(&state.asset)
            .ok_or(DesktopNativeHostError::MissingRenderer)?;
        let templates = SceneAssets::resolve(
            &model.named_nodes,
            &nodes,
            &meshes,
            &gltf_materials,
            &mut materials,
        )?;
        let rectangle = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::Vec2::splat(Self::INITIAL_BOARD_SIZE),
        );
        let viewport = ScenePickingViewport::new(rectangle, rectangle, 1.0, 1.0)?;
        let mut renderer = SceneRenderer::spawn(world, templates, viewport, state.camera)?;
        if let Some(snapshot) = state.presentation.board_snapshot() {
            let position = BoardPosition::from_fen(snapshot.fen(), snapshot.variant())?;
            renderer.apply_position(world, &position)?;
            state.rendered_board = Some(snapshot.clone());
        } else {
            state.rendered_board = None;
        }
        world.insert_resource(renderer);
        Ok(())
    }

    fn asset_failure(
        root: &LoadState,
        recursive: &RecursiveDependencyLoadState,
    ) -> Option<Arc<AssetLoadError>> {
        match root {
            LoadState::Failed(error) => Some(Arc::clone(error)),
            LoadState::NotLoaded | LoadState::Loading | LoadState::Loaded => match recursive {
                RecursiveDependencyLoadState::Failed(error) => Some(Arc::clone(error)),
                RecursiveDependencyLoadState::NotLoaded
                | RecursiveDependencyLoadState::Loading
                | RecursiveDependencyLoadState::Loaded => None,
            },
        }
    }

    fn update_scene(
        world: &mut World,
        state: &mut DesktopNativeHostState,
        renderer: &mut SceneRenderer,
    ) -> Result<(), DesktopNativeHostError> {
        if state.ready {
            renderer.finish_frame(world)?;
        }
        if !state.ready && !Self::is_preparing(state)? {
            return Ok(());
        }
        if !state.ready {
            world.insert_resource(WinitSettings::continuous());
        }
        if let Some(snapshot) = state.presentation.board_snapshot() {
            if !state.rendered_board.as_ref().is_some_and(|previous| {
                previous.fen() == snapshot.fen() && previous.variant() == snapshot.variant()
            }) {
                let position = BoardPosition::from_fen(snapshot.fen(), snapshot.variant())?;
                renderer.apply_position(world, &position)?;
                state.rendered_board = Some(snapshot.clone());
            }
        } else if state.rendered_board.is_some() {
            renderer.clear_position(world)?;
            state.rendered_board = None;
        }
        if state.ready {
            return Ok(());
        }
        let camera_changed = state.pending_camera.is_some();
        if let Some(camera) = state.pending_camera.take() {
            state.camera = camera;
        }
        let viewport = state
            .pending_view
            .take()
            .or_else(|| camera_changed.then_some(renderer.viewport()));
        if let Some(viewport) = viewport {
            renderer.update_view(world, viewport, state.camera)?;
        }
        let probe_ready = world
            .get_resource::<DesktopNativeHostRenderProbe>()
            .is_some_and(|probe| {
                probe.generation == renderer.generation() && probe.ready.load(Ordering::Acquire)
            });
        if !state.ready && probe_ready {
            renderer.mark_ready(renderer.generation())?;
            renderer.finish_frame(world)?;
            state.ready = true;
            state.readiness = None;
            state.failure = None;
            state.fault_received_at = None;
            Self::set_window_title(world, Self::TITLE);
            #[cfg(target_os = "macos")]
            Self::set_native_recovery_enabled(world, false);
            state
                .services
                .events
                .send_replace(DesktopNativeHostEvent::FrameworkReady {
                    generation: state.generation,
                });
            world.insert_resource(WinitSettings {
                focused_mode: UpdateMode::reactive_low_power(Duration::MAX),
                unfocused_mode: UpdateMode::reactive_low_power(Duration::MAX),
            });
        }
        Ok(())
    }

    fn present(world: &mut World) {
        let Some(mut state) = world.remove_resource::<DesktopNativeHostState>() else {
            return;
        };
        let outcome = Self::present_state(world, &mut state);
        world.insert_resource(state);
        if let Err(error) = outcome {
            Self::record_failure(world, error);
        }
    }

    fn present_state(
        world: &mut World,
        state: &mut DesktopNativeHostState,
    ) -> Result<(), DesktopNativeHostError> {
        let mut contexts = world.query_filtered::<&mut EguiContext, With<PrimaryEguiContext>>();
        let Ok(mut context) = contexts.single_mut(world) else {
            return Ok(());
        };
        let context = context.get_mut().clone();
        if !state.fonts_initialized {
            context.set_fonts(PresentationThemeFont::definitions());
            state.fonts_initialized = true;
            context.request_discard("install bundled native typography");
            return Ok(());
        }
        let renderer = world.remove_resource::<SceneRenderer>();
        let outcome = Self::show_presentation(world, state, &context, renderer.as_ref());
        if let Some(renderer) = renderer {
            world.insert_resource(renderer);
        }
        outcome?;
        if let Some(window) = &mut state.puzzle_window {
            window.report_readiness(
                DesktopNativePuzzleWindowFrame {
                    world,
                    graphics_ready: state.ready
                        && state.failure.is_none()
                        && state.quit == DesktopNativeHostEnumQuit::Idle
                        && state.pending_view.is_none()
                        && state.pending_camera.is_none()
                        && world.contains_resource::<SceneRenderer>(),
                    rendered_board: state.rendered_board.as_ref(),
                },
                &mut state.presentation,
                &state.services.port,
            )?;
        }
        Ok(())
    }

    fn show_presentation(
        world: &mut World,
        state: &mut DesktopNativeHostState,
        context: &egui::Context,
        renderer: Option<&SceneRenderer>,
    ) -> Result<(), DesktopNativeHostError> {
        let scene = renderer
            .map(|renderer| {
                let mut textures = world.resource_mut::<EguiUserTextures>();
                PresentationBoardScene::from_renderer(renderer, &mut textures, state.camera)
            })
            .transpose()?;
        let mut ray_cast_state = SystemState::<MeshRayCast<'_, '_>>::new(world);
        let mut ray_cast = ray_cast_state.get_mut(world)?;
        let mut picker = |point| match renderer {
            Some(renderer) => renderer.pick(point, renderer.generation(), &mut ray_cast),
            None => Ok(None),
        };
        let mut ui = egui::Ui::new(
            context.clone(),
            egui::Id::new("chess.native.root"),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                .max_rect(context.content_rect()),
        );
        let frame =
            state
                .presentation
                .show(&mut ui, &state.services.port, scene.as_ref(), &mut picker)?;
        if state.presentation.take_confirmed_quit_request()
            && state.quit == DesktopNativeHostEnumQuit::Idle
        {
            state.quit = DesktopNativeHostEnumQuit::Requested;
            state.services.port.rearm_wake()?;
        }
        if state.quit != DesktopNativeHostEnumQuit::Idle {
            return Ok(());
        }
        if let Some(viewport) = frame.viewport()
            && renderer.is_some_and(|renderer| renderer.viewport() != viewport)
        {
            state.pending_view = Some(viewport);
        }
        if let Some(action) = frame.camera_action() {
            state.pending_camera = Some(Self::camera_action(state.camera, action));
            context.request_repaint();
        }
        if state.ready && (state.pending_view.is_some() || state.pending_camera.is_some()) {
            let Some(generation) =
                Self::request_geometry_preparation(&state.services.capacity, &state.services.port)?
            else {
                return Ok(());
            };
            state.generation = generation;
            state.ready = false;
            state.readiness = Some(DesktopNativeHostReadiness::new_at(Instant::now()));
            state
                .services
                .events
                .send_replace(DesktopNativeHostEvent::PreparationRequested { generation });
            context.request_repaint();
        }
        Ok(())
    }

    fn request_geometry_preparation(
        capacity: &Mutex<CapacityPhase>,
        port: &DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
    ) -> Result<Option<CapacityPhaseGeneration>, DesktopNativeHostError> {
        let Some(mut phase) = Self::capacity_phase(capacity, port)? else {
            return Ok(None);
        };
        if phase.state() != CapacityPhaseEnumState::Steady {
            port.rearm_wake()?;
            return Ok(None);
        }
        Ok(Some(phase.request_preparation()?))
    }

    fn is_preparing(state: &DesktopNativeHostState) -> Result<bool, DesktopNativeHostError> {
        let Some(phase) = Self::capacity_phase(&state.services.capacity, &state.services.port)?
        else {
            return Ok(false);
        };
        Ok(phase.state() == CapacityPhaseEnumState::Preparing
            && phase.generation() == Some(state.generation))
    }

    fn camera_action(
        camera: SceneCamera,
        action: PresentationBoardEnumCameraAction,
    ) -> SceneCamera {
        match action {
            PresentationBoardEnumCameraAction::RotateLeft => camera.rotate_left(),
            PresentationBoardEnumCameraAction::RotateRight => camera.rotate_right(),
            PresentationBoardEnumCameraAction::TiltUp => camera.tilt_up(),
            PresentationBoardEnumCameraAction::TiltDown => camera.tilt_down(),
            PresentationBoardEnumCameraAction::ZoomIn => camera.zoom_in(),
            PresentationBoardEnumCameraAction::ZoomOut => camera.zoom_out(),
            PresentationBoardEnumCameraAction::Reset => camera.reset(),
            PresentationBoardEnumCameraAction::Flip => {
                camera.with_orientation(match camera.orientation() {
                    BoardEnumColor::White => BoardEnumColor::Black,
                    BoardEnumColor::Black => BoardEnumColor::White,
                })
            }
        }
    }

    fn validate_surface(main: &mut World, render: &mut World) {
        let Some(mut state) = main.remove_resource::<DesktopNativeHostState>() else {
            return;
        };
        let outcome = Self::surface(main, render, &mut state);
        main.insert_resource(state);
        if let Err(error) = outcome {
            let mut query = main.query_filtered::<Entity, With<PrimaryWindow>>();
            if let Ok(window) = query.single(main) {
                main.entity_mut(window).remove::<RawHandleWrapper>();
            }
            Self::record_failure(main, error);
        }
    }

    fn surface(
        main: &mut World,
        render: &World,
        state: &mut DesktopNativeHostState,
    ) -> Result<(), DesktopNativeHostError> {
        if let Some(expected) = &state.expected_device {
            if main
                .get_resource::<RenderDevice>()
                .is_some_and(|device| device.wgpu_device() == expected.wgpu_device())
            {
                state.expected_device = None;
            } else {
                return Ok(());
            }
        }
        let mut query =
            main.query_filtered::<Entity, (With<PrimaryWindow>, With<RawHandleWrapper>)>();
        let Ok(window) = query.single(main) else {
            return Ok(());
        };
        if state.validated_surface == Some(window) {
            return Ok(());
        }
        let Some(instance) = render.get_resource::<RenderInstance>() else {
            return Ok(());
        };
        let adapter = main
            .get_resource::<RenderAdapter>()
            .ok_or(DesktopNativeHostError::MissingRenderer)?;
        WINIT_WINDOWS.with_borrow(|windows| {
            let window = windows
                .get_window(window)
                .ok_or(DesktopNativeHostError::PrimaryWindow)?;
            let surface = instance
                .create_surface(&**window)
                .map_err(DesktopNativeHostError::Surface)?;
            if !adapter.is_surface_supported(&surface)
                || surface.get_capabilities(adapter).formats.is_empty()
            {
                return Err(DesktopNativeHostError::IncompatibleSurface);
            }
            Ok(())
        })?;
        state.validated_surface = Some(window);
        Ok(())
    }

    fn observe_render(
        probe: Option<Res<'_, DesktopNativeHostRenderProbe>>,
        images: Res<'_, RenderAssets<GpuImage>>,
        meshes: Res<'_, RenderAssets<RenderMesh>>,
        materials: Res<'_, ErasedRenderAssets<PreparedMaterial>>,
        pipelines: Res<'_, PipelineCache>,
    ) {
        let Some(probe) = probe else {
            return;
        };
        if probe.failure.get().is_some() {
            return;
        }
        for pipeline in pipelines.pipelines() {
            if let CachedPipelineState::Err(error) = &pipeline.state
                && let Some(error) = DesktopNativeHostRenderProbe::permanent_pipeline_failure(error)
            {
                probe.failure.get_or_init(|| Arc::new(error));
                return;
            }
        }
        if images.get(probe.target.id()).is_none()
            || probe.meshes.is_empty()
            || probe.materials.is_empty()
            || probe.meshes.iter().any(|id| meshes.get(*id).is_none())
            || probe
                .materials
                .iter()
                .any(|id| materials.get(*id).is_none())
            || pipelines.pipelines().next().is_none()
        {
            return;
        }
        if pipelines.waiting_pipelines().next().is_none()
            && pipelines
                .pipelines()
                .all(|pipeline| matches!(pipeline.state, CachedPipelineState::Ok(_)))
        {
            probe.ready.store(true, Ordering::Release);
        }
    }

    fn reset_graphics(world: &mut World) {
        let windows = world
            .query_filtered::<Entity, Or<(With<ExtractedWindow>, With<SurfaceData>)>>()
            .iter(world)
            .collect::<Vec<_>>();
        for window in windows {
            let mut entity = world.entity_mut(window);
            entity.remove::<ExtractedWindow>();
            entity.remove::<SurfaceData>();
            entity.remove::<RawHandleWrapper>();
        }
        world.remove_resource::<EguiPipeline>();
        world.remove_resource::<SpecializedRenderPipelines<EguiPipeline>>();
        world.remove_resource::<EguiTransforms>();
        world.remove_resource::<EguiRenderData>();
        world.remove_resource::<EguiTextureBindGroups>();
        world.remove_resource::<EguiPipelines>();
        world.init_resource::<EguiPipeline>();
        world.init_resource::<SpecializedRenderPipelines<EguiPipeline>>();
        world.init_resource::<EguiTransforms>();
        world.init_resource::<EguiRenderData>();
        world.init_resource::<EguiTextureBindGroups>();
        world.insert_resource(EguiPipelines(Default::default()));
    }

    fn failure(error: &DesktopNativeHostError) -> DesktopNativeRecoveryFailure {
        let stage = match error {
            DesktopNativeHostError::BackendUnavailable | DesktopNativeHostError::Adapter(_) => {
                DesktopNativeRecoveryEnumStage::Adapter
            }
            DesktopNativeHostError::Device(_) => DesktopNativeRecoveryEnumStage::Device,
            DesktopNativeHostError::Surface(_) | DesktopNativeHostError::IncompatibleSurface => {
                DesktopNativeRecoveryEnumStage::Surface
            }
            DesktopNativeHostError::AssetLoad(_)
            | DesktopNativeHostError::AssetRetryIdentity
            | DesktopNativeHostError::Assets(_) => DesktopNativeRecoveryEnumStage::Assets,
            DesktopNativeHostError::ResourcePath(_)
            | DesktopNativeHostError::CapacityBusy
            | DesktopNativeHostError::CapacityPoisoned
            | DesktopNativeHostError::PreparationRequired
            | DesktopNativeHostError::PrimaryWindow
            | DesktopNativeHostError::MissingState
            | DesktopNativeHostError::MissingRenderer
            | DesktopNativeHostError::StartupPaused { .. }
            | DesktopNativeHostError::QuitRefused(_)
            | DesktopNativeHostError::EventLoop(_)
            | DesktopNativeHostError::Wake(_)
            | DesktopNativeHostError::Application(_)
            | DesktopNativeHostError::Capacity(_)
            | DesktopNativeHostError::Recovery(_)
            | DesktopNativeHostError::TaskPools(_)
            | DesktopNativeHostError::Scene(_)
            | DesktopNativeHostError::Camera(_)
            | DesktopNativeHostError::Viewport(_)
            | DesktopNativeHostError::Position(_)
            | DesktopNativeHostError::PuzzleWindow(_)
            | DesktopNativeHostError::Presentation(_)
            | DesktopNativeHostError::System(_) => DesktopNativeRecoveryEnumStage::Renderer,
            DesktopNativeHostError::Pipeline(_)
            | DesktopNativeHostError::ReadinessTimeout { .. } => {
                DesktopNativeRecoveryEnumStage::Renderer
            }
            DesktopNativeHostError::Dialogs(_) => DesktopNativeRecoveryEnumStage::Renderer,
            #[cfg(target_os = "macos")]
            DesktopNativeHostError::Menu(_) => DesktopNativeRecoveryEnumStage::Renderer,
            DesktopNativeHostError::Graphics { .. } => DesktopNativeRecoveryEnumStage::Renderer,
        };
        DesktopNativeRecoveryFailure::new(stage, error.to_string())
    }

    fn record_failure(world: &mut World, error: DesktopNativeHostError) {
        let Some(mut state) = world.remove_resource::<DesktopNativeHostState>() else {
            return;
        };
        if state.readiness.take().is_some() {
            state.recovery = None;
            state.replacement = None;
            state.failure = None;
            state.fault_received_at = None;
        }
        if state.failure.is_none() {
            eprintln!("Native host failure: {error}");
            state.failure = Some(error);
        }
        if let Err(phase_error) = Self::begin_interruption(world, &mut state) {
            eprintln!(
                "Native graphics interruption could not enter its capacity phase: {phase_error}"
            );
        }
        world.insert_resource(state);
    }

    fn begin_interruption(
        world: &mut World,
        state: &mut DesktopNativeHostState,
    ) -> Result<(), DesktopNativeHostError> {
        if state.quit != DesktopNativeHostEnumQuit::Idle {
            return Ok(());
        }
        if state.recovery.is_some() || state.graphics_drain.is_some() {
            return Ok(());
        }
        if state.failure.is_none() {
            return Ok(());
        }
        let received_at = match state.fault_received_at {
            Some(received_at) => received_at,
            None => {
                let received_at = state.services.port.now()?;
                state.fault_received_at = Some(received_at);
                received_at
            }
        };
        Self::set_window_title(world, Self::RECOVERY_TITLE);
        #[cfg(target_os = "macos")]
        Self::set_native_recovery_enabled(world, true);
        let generation = {
            let Some(mut phase) =
                Self::capacity_phase(&state.services.capacity, &state.services.port)?
            else {
                return Ok(());
            };
            phase.request_preparation()?
        };
        state.generation = generation;
        state.ready = false;
        state.interruption_announced = false;
        if let Some(mut renderer) = world.remove_resource::<SceneRenderer>() {
            let outcome = renderer.fail(world);
            world.insert_resource(renderer);
            outcome?;
        }
        state.graphics_drain = Some(DesktopNativeHostGraphicsDrain::new(generation, received_at));
        world.insert_resource(WinitSettings::continuous());
        state.services.port.rearm_wake()?;
        Ok(())
    }

    fn poll_graphics_handoff(
        world: &mut World,
        state: &mut DesktopNativeHostState,
    ) -> Result<(), DesktopNativeHostError> {
        let drain = state
            .graphics_drain
            .as_mut()
            .ok_or(DesktopNativeHostError::MissingState)?;
        if let Some(event) = drain.take_stopped_event() {
            state.services.events.send_replace(event);
            world.insert_resource(WinitSettings {
                focused_mode: UpdateMode::reactive_low_power(Duration::MAX),
                unfocused_mode: UpdateMode::reactive_low_power(Duration::MAX),
            });
        }
        let Some(generation) =
            drain.fresh_preparation(&state.services.capacity, &state.services.port)?
        else {
            return Ok(());
        };
        let error = state
            .failure
            .as_ref()
            .ok_or(DesktopNativeHostError::MissingState)?;
        state.generation = generation;
        state.recovery = Some(DesktopNativeRecovery::new(Self::failure(error), generation));
        state.graphics_drain = None;
        Self::recover(world, state)
    }

    fn recover(
        world: &mut World,
        state: &mut DesktopNativeHostState,
    ) -> Result<(), DesktopNativeHostError> {
        let recovery = state
            .recovery
            .as_mut()
            .ok_or(DesktopNativeHostError::MissingState)?;
        let acknowledged = {
            let Some(phase) = Self::capacity_phase(&state.services.capacity, &state.services.port)?
            else {
                return Ok(());
            };
            phase.state() == CapacityPhaseEnumState::Preparing
                && phase.generation() == Some(recovery.generation())
        };
        if !acknowledged {
            return Ok(());
        }
        recovery.acknowledge(state.generation)?;
        match recovery.choose(state.services.recovery.as_ref())? {
            DesktopNativeRecoveryEnumAction::KeepPaused => {
                if !state.interruption_announced {
                    state
                        .services
                        .events
                        .send_replace(DesktopNativeHostEvent::Interrupted {
                            generation: state.generation,
                        });
                    state.interruption_announced = true;
                }
                world.insert_resource(WinitSettings {
                    focused_mode: UpdateMode::reactive_low_power(Duration::MAX),
                    unfocused_mode: UpdateMode::reactive_low_power(Duration::MAX),
                });
            }
            DesktopNativeRecoveryEnumAction::SafeQuit => {
                state.quit = DesktopNativeHostEnumQuit::Requested;
                state.services.port.rearm_wake()?;
            }
            DesktopNativeRecoveryEnumAction::Retry => {
                if state.replacement.is_none() {
                    let readiness = DesktopNativeHostReadiness::new_at(Instant::now());
                    let mut query = world.query_filtered::<Entity, With<PrimaryWindow>>();
                    let window = query
                        .single(world)
                        .map_err(|_error| DesktopNativeHostError::PrimaryWindow)?;
                    let replacement =
                        match Self::graphics(&state.services.runtime, Some(window), readiness) {
                            Ok(replacement) => replacement,
                            Err(error) => {
                                state.recovery = Some(DesktopNativeRecovery::new(
                                    Self::failure(&error),
                                    state.generation,
                                ));
                                state.failure = Some(error);
                                state.services.port.rearm_wake()?;
                                return Ok(());
                            }
                        };
                    state.replacement = Some(replacement);
                    state.readiness = Some(readiness);
                    if let Some(renderer) = world.remove_resource::<SceneRenderer>() {
                        renderer.despawn(world)?;
                    }
                    world.remove_resource::<DesktopNativeHostRenderProbe>();
                    state.rendered_board = None;
                    state.fonts_initialized = false;
                    state.asset_reload = Some(DesktopNativeHostEnumAssetReload::retire(
                        &mut state.asset,
                        state.asset_reload.take(),
                    ));
                    world.insert_resource(WinitSettings::continuous());
                    world
                        .get_resource::<RenderDevice>()
                        .ok_or(DesktopNativeHostError::MissingRenderer)?
                        .wgpu_device()
                        .destroy();
                    state.services.port.rearm_wake()?;
                }
            }
        }
        Ok(())
    }

    fn render_error(
        error: &RenderError,
        main: &mut World,
        _render: &mut World,
    ) -> RenderErrorPolicy {
        let Some(mut state) = main.remove_resource::<DesktopNativeHostState>() else {
            return RenderErrorPolicy::StopRendering;
        };
        if let Some(resources) = state.replacement.take() {
            state.recovery = None;
            state.validated_surface = None;
            state.expected_device = Some(resources.0.clone());
            let mut query = main.query_filtered::<Entity, With<PrimaryWindow>>();
            if let Ok(entity) = query.single(main) {
                let handle = WINIT_WINDOWS
                    .with_borrow(|windows| windows.get_window(entity).map(RawHandleWrapper::new));
                if let Some(Ok(handle)) = handle {
                    main.entity_mut(entity).insert(handle);
                }
            }
            main.insert_resource(state);
            return RenderErrorPolicy::Recover(RenderCreation::from(resources));
        }
        main.insert_resource(state);
        Self::record_failure(
            main,
            DesktopNativeHostError::Graphics {
                kind: error.ty,
                description: error.description.clone(),
            },
        );
        RenderErrorPolicy::StopRendering
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use bevy::asset::{
        AssetEvent, AssetLoadError, Handle, LoadState, RecursiveDependencyLoadState,
    };
    use bevy::ecs::message::Messages;
    use bevy::gltf::Gltf;
    use bevy::prelude::{ButtonInput, KeyCode};
    use bevy::shader::{Shader, ShaderCacheError};

    use crate::capacity::budget::CapacityBudget;
    use crate::capacity::phase::{CapacityPhase, CapacityPhaseEnumState, CapacityPhaseError};
    use crate::clock::duration::ClockInstant;
    use crate::clock::time_provider::ClockTimeProviderMonotonic;
    use crate::desktop::application::{DesktopApplication, DesktopApplicationPort};
    use crate::desktop::native::dialogs::DesktopNativeDialogsError;
    use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;
    use crate::desktop::native::host::{
        DesktopNativeHost, DesktopNativeHostAssetReload, DesktopNativeHostEnumTeardown,
        DesktopNativeHostError, DesktopNativeHostEvent, DesktopNativeHostGraphicsDrain,
        DesktopNativeHostReadiness, DesktopNativeHostRenderProbe,
    };

    struct DesktopNativeHostFixture {
        wake: Arc<DesktopNativeEventLoopWake>,
        port: DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
        capacity: Mutex<CapacityPhase>,
        budget: CapacityBudget,
    }

    impl DesktopNativeHostFixture {
        fn new() -> Result<Self, Box<dyn std::error::Error>> {
            let wake = Arc::new(DesktopNativeEventLoopWake::new());
            let application = DesktopApplication::new(
                Arc::new(ClockTimeProviderMonotonic::new()),
                Arc::clone(&wake),
            );
            let workers = NonZeroUsize::new(CapacityPhase::MINIMUM_WORKERS)
                .ok_or("native capacity minimum is zero")?;
            let budget = CapacityBudget::new(workers)?;
            Ok(Self {
                wake,
                port: application.port(),
                capacity: Mutex::new(CapacityPhase::new(budget.clone())?),
                budget,
            })
        }

        fn acknowledge_wake(&self) {
            self.wake.acknowledge();
            self.port.acknowledge_wake();
        }
    }

    #[test]
    fn native_host_capacity_contention_rearms_idle_work() -> Result<(), Box<dyn std::error::Error>>
    {
        let fixture = DesktopNativeHostFixture::new()?;
        let guard = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
            .ok_or("native capacity unexpectedly unavailable")?;
        assert!(!fixture.wake.is_pending());
        assert!(DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?.is_none());
        assert!(fixture.wake.is_pending());
        drop(guard);
        fixture.acknowledge_wake();
        assert!(DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?.is_some());
        assert!(!fixture.wake.is_pending());
        Ok(())
    }

    #[test]
    fn native_host_dialog_contention_rearms_without_hiding_other_errors()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = DesktopNativeHostFixture::new()?;
        assert!(
            DesktopNativeHost::dialog_result::<()>(
                &fixture.port,
                Err(DesktopNativeDialogsError::Busy),
            )?
            .is_none()
        );
        assert!(fixture.wake.is_pending());
        fixture.acknowledge_wake();
        assert!(matches!(
            DesktopNativeHost::dialog_result::<()>(
                &fixture.port,
                Err(DesktopNativeDialogsError::Closed),
            ),
            Err(DesktopNativeHostError::Dialogs(
                DesktopNativeDialogsError::Closed
            ))
        ));
        assert_eq!(
            DesktopNativeHost::dialog_result(&fixture.port, Ok(true))?,
            Some(true)
        );
        assert!(!fixture.wake.is_pending());
        Ok(())
    }

    #[test]
    fn native_host_geometry_waits_for_framework_ready_acknowledgement()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = DesktopNativeHostFixture::new()?;
        let generation = {
            let mut phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
                .ok_or("native capacity unexpectedly unavailable")?;
            let generation = phase.generation().ok_or("missing initial preparation")?;
            phase.acknowledge_searches_stopped(generation)?;
            phase.try_enter_preparation(generation)?;
            generation
        };
        assert_eq!(
            DesktopNativeHost::request_geometry_preparation(&fixture.capacity, &fixture.port)?,
            None
        );
        assert!(fixture.wake.is_pending());
        {
            let mut phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
                .ok_or("native capacity unexpectedly unavailable")?;
            assert_eq!(phase.state(), CapacityPhaseEnumState::Preparing);
            assert_eq!(phase.generation(), Some(generation));
            phase.complete_preparation(generation)?;
        }
        fixture.acknowledge_wake();
        let next =
            DesktopNativeHost::request_geometry_preparation(&fixture.capacity, &fixture.port)?
                .ok_or("geometry preparation was not admitted after acknowledgement")?;
        assert_ne!(next, generation);
        let phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
            .ok_or("native capacity unexpectedly unavailable")?;
        assert_eq!(phase.state(), CapacityPhaseEnumState::Quiescing);
        assert_eq!(phase.generation(), Some(next));
        Ok(())
    }

    #[test]
    fn native_host_teardown_retains_preparation_until_a_completed_render_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = DesktopNativeHostFixture::new()?;
        let workers = {
            let mut phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
                .ok_or("native capacity unexpectedly unavailable")?;
            let generation = phase.generation().ok_or("missing initial preparation")?;
            phase.acknowledge_searches_stopped(generation)?;
            phase.try_enter_preparation(generation)?;
            phase.workload_workers()
        };
        let mut teardown = DesktopNativeHostEnumTeardown::Active;
        assert!(teardown.begin(&fixture.capacity, &fixture.port)?);
        assert!(!teardown.try_admit(&fixture.capacity, &fixture.port)?);
        {
            let phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
                .ok_or("native capacity unexpectedly unavailable")?;
            assert_eq!(phase.state(), CapacityPhaseEnumState::Draining);
            assert_eq!(phase.withheld_workload_workers(), workers);
            assert!(fixture.budget.try_lease(workers).is_err());
        }
        teardown.observe_completed_frame();
        assert!(teardown.try_admit(&fixture.capacity, &fixture.port)?);
        assert!(teardown.try_admit(&fixture.capacity, &fixture.port)?);
        {
            let phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
                .ok_or("native capacity unexpectedly unavailable")?;
            assert_eq!(phase.state(), CapacityPhaseEnumState::Teardown);
            assert_eq!(phase.withheld_workload_workers(), 0);
            assert!(phase.try_lease_workload(workers).is_err());
        }
        assert_eq!(fixture.budget.try_lease(workers)?.workers(), workers);
        Ok(())
    }

    #[test]
    fn native_host_startup_quit_reopens_executor_admission_after_observed_stop()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = DesktopNativeHostFixture::new()?;
        let mut teardown = DesktopNativeHostEnumTeardown::Active;
        assert!(teardown.begin(&fixture.capacity, &fixture.port)?);
        assert!(!teardown.try_admit(&fixture.capacity, &fixture.port)?);
        {
            let phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
                .ok_or("native capacity unexpectedly unavailable")?;
            assert_eq!(phase.state(), CapacityPhaseEnumState::Interrupted);
            assert!(fixture.budget.try_lease(1).is_err());
        }
        teardown.observe_completed_frame();
        assert!(teardown.try_admit(&fixture.capacity, &fixture.port)?);
        assert_eq!(fixture.budget.try_lease(1)?.workers(), 1);
        Ok(())
    }

    #[test]
    fn native_host_terminal_drain_retires_only_its_own_preparation_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = DesktopNativeHostFixture::new()?;
        let (previous, current) = {
            let mut phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
                .ok_or("native capacity unexpectedly unavailable")?;
            let previous = phase.generation().ok_or("missing initial preparation")?;
            phase.acknowledge_searches_stopped(previous)?;
            phase.try_enter_preparation(previous)?;
            phase.complete_preparation(previous)?;
            let current = phase.request_preparation()?;
            phase.acknowledge_searches_stopped(current)?;
            phase.try_enter_preparation(current)?;
            (previous, current)
        };
        let mut teardown = DesktopNativeHostEnumTeardown::Active;
        assert!(teardown.begin(&fixture.capacity, &fixture.port)?);
        {
            let mut phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
                .ok_or("native capacity unexpectedly unavailable")?;
            assert!(!phase.accepts_preparation_event(current)?);
            assert!(matches!(
                phase.accepts_preparation_event(previous),
                Err(CapacityPhaseError::StaleGeneration { expected: Some(expected), actual })
                    if expected == current && actual == previous
            ));
            assert!(phase.request_preparation().is_err());
            assert_eq!(phase.state(), CapacityPhaseEnumState::Draining);
            assert_eq!(fixture.budget.available(), 0);
        }
        teardown.observe_completed_frame();
        assert!(teardown.try_admit(&fixture.capacity, &fixture.port)?);
        let phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
            .ok_or("native capacity unexpectedly unavailable")?;
        assert!(!phase.accepts_preparation_event(current)?);
        assert!(matches!(
            phase.accepts_preparation_event(previous),
            Err(CapacityPhaseError::StaleGeneration { .. })
        ));
        assert_eq!(phase.state(), CapacityPhaseEnumState::Teardown);
        Ok(())
    }

    #[test]
    fn native_host_terminal_quiescence_cannot_request_recovery_before_observed_teardown()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = DesktopNativeHostFixture::new()?;
        let mut teardown = DesktopNativeHostEnumTeardown::Active;
        assert!(teardown.begin(&fixture.capacity, &fixture.port)?);
        let mut phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
            .ok_or("native capacity unexpectedly unavailable")?;
        assert_eq!(phase.state(), CapacityPhaseEnumState::Interrupted);
        assert!(phase.request_preparation().is_err());
        assert!(fixture.budget.try_lease(1).is_err());
        Ok(())
    }

    #[test]
    fn native_host_graphics_recovery_requires_observed_drain_and_a_fresh_preparation()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = DesktopNativeHostFixture::new()?;
        let generation = {
            let mut phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
                .ok_or("native capacity unexpectedly unavailable")?;
            let generation = phase.generation().ok_or("missing initial preparation")?;
            phase.acknowledge_searches_stopped(generation)?;
            phase.try_enter_preparation(generation)?;
            generation
        };
        let received_at = ClockInstant::from_nanos(47_000);
        let mut drain = DesktopNativeHostGraphicsDrain::new(generation, received_at);
        assert_eq!(drain.take_stopped_event(), None);
        assert_eq!(
            drain.fresh_preparation(&fixture.capacity, &fixture.port)?,
            None
        );
        drain.observe_completed_frame();
        assert_eq!(
            drain.take_stopped_event(),
            Some(DesktopNativeHostEvent::GraphicsPreparationStopped {
                generation,
                received_at,
            })
        );
        assert_eq!(drain.take_stopped_event(), None);
        assert_eq!(
            drain.fresh_preparation(&fixture.capacity, &fixture.port)?,
            None
        );
        {
            let mut phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
                .ok_or("native capacity unexpectedly unavailable")?;
            phase.interrupt_preparation(
                generation,
                crate::capacity::phase::CapacityPhaseEnumInterruption::Failed,
            )?;
            phase.acknowledge_preparation_stopped(generation)?;
            phase.open_interrupted_admission()?;
        }
        assert_eq!(
            drain.fresh_preparation(&fixture.capacity, &fixture.port)?,
            None
        );
        let next = {
            let mut phase = DesktopNativeHost::capacity_phase(&fixture.capacity, &fixture.port)?
                .ok_or("native capacity unexpectedly unavailable")?;
            assert_eq!(phase.state(), CapacityPhaseEnumState::Steady);
            let next = phase.request_preparation()?;
            phase.acknowledge_searches_stopped(next)?;
            phase.try_enter_preparation(next)?;
            next
        };
        assert_ne!(next, generation);
        assert_eq!(
            drain.fresh_preparation(&fixture.capacity, &fixture.port)?,
            Some(next)
        );
        Ok(())
    }

    #[test]
    fn native_host_readiness_expires_at_the_shared_attempt_boundary()
    -> Result<(), Box<dyn std::error::Error>> {
        let start = Instant::now();
        let readiness = DesktopNativeHostReadiness::new_at(start);
        let before = start
            .checked_add(DesktopNativeHostReadiness::BUDGET - Duration::from_nanos(1))
            .ok_or("invalid readiness instant")?;
        let boundary = start
            .checked_add(DesktopNativeHostReadiness::BUDGET)
            .ok_or("invalid readiness instant")?;
        assert_eq!(
            readiness.remaining_at(start)?,
            DesktopNativeHostReadiness::BUDGET
        );
        assert_eq!(readiness.remaining_at(before)?, Duration::from_nanos(1));
        assert!(matches!(
            readiness.remaining_at(boundary),
            Err(DesktopNativeHostError::ReadinessTimeout { source: None, .. })
        ));
        Ok(())
    }

    #[test]
    fn native_host_pipeline_failures_keep_temporary_loads_retryable() {
        assert!(
            DesktopNativeHostRenderProbe::permanent_pipeline_failure(
                &ShaderCacheError::ShaderImportNotYetAvailable
            )
            .is_none()
        );
        assert!(
            DesktopNativeHostRenderProbe::permanent_pipeline_failure(
                &ShaderCacheError::ShaderNotLoaded(Handle::<Shader>::default().id())
            )
            .is_none()
        );
        for error in [
            ShaderCacheError::ProcessShaderError("invalid shader expression".to_owned()),
            ShaderCacheError::CreateShaderModule("unsupported shader instruction".to_owned()),
        ] {
            let fatal = DesktopNativeHostRenderProbe::permanent_pipeline_failure(&error);
            assert_eq!(
                fatal.as_ref().map(ToString::to_string),
                Some(error.to_string())
            );
        }
    }

    #[test]
    fn native_host_recursive_asset_failure_preserves_the_dependency_cause()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = Arc::new(AssetLoadError::EmptyPath("".into()));
        let failure = DesktopNativeHost::asset_failure(
            &LoadState::Loaded,
            &RecursiveDependencyLoadState::Failed(Arc::clone(&source)),
        )
        .ok_or("missing recursive dependency failure")?;
        assert!(Arc::ptr_eq(&source, &failure));
        assert!(
            DesktopNativeHost::asset_failure(
                &LoadState::Loaded,
                &RecursiveDependencyLoadState::Loaded,
            )
            .is_none()
        );
        Ok(())
    }

    #[test]
    fn native_host_asset_retry_requires_a_new_identity_and_ignores_old_completion_messages()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut asset = Handle::<Gltf>::Uuid(uuid::Uuid::from_u128(11), std::marker::PhantomData);
        let previous = asset.id();
        let fresh = Handle::<Gltf>::Uuid(uuid::Uuid::from_u128(12), std::marker::PhantomData).id();
        let mut retry = crate::desktop::native::host::DesktopNativeHostEnumAssetReload::retire(
            &mut asset, None,
        );
        let mut events = Messages::default();
        assert!(matches!(
            retry.start(
                previous,
                &events,
                &LoadState::Loaded,
                &RecursiveDependencyLoadState::Loaded
            ),
            Err(DesktopNativeHostError::AssetRetryIdentity)
        ));
        retry = crate::desktop::native::host::DesktopNativeHostEnumAssetReload::retire(
            &mut asset,
            Some(retry),
        );
        assert!(matches!(
            retry.start(
                previous,
                &events,
                &LoadState::Loaded,
                &RecursiveDependencyLoadState::Loaded
            ),
            Err(DesktopNativeHostError::AssetRetryIdentity)
        ));
        retry.start(
            fresh,
            &events,
            &LoadState::Loading,
            &RecursiveDependencyLoadState::Loading,
        )?;
        let crate::desktop::native::host::DesktopNativeHostEnumAssetReload::Loading(ref mut reload) =
            retry
        else {
            return Err("asset retry did not own its fresh load".into());
        };
        events.write(AssetEvent::LoadedWithDependencies { id: previous });
        assert!(!reload.observe(
            fresh,
            &events,
            &LoadState::Loaded,
            &RecursiveDependencyLoadState::Loaded
        )?);
        events.write(AssetEvent::LoadedWithDependencies { id: fresh });
        assert!(reload.observe(
            fresh,
            &events,
            &LoadState::Loaded,
            &RecursiveDependencyLoadState::Loaded
        )?);
        Ok(())
    }

    #[test]
    fn native_host_asset_retry_waits_for_a_fresh_root_and_all_repaired_dependencies()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = Handle::<Gltf>::default().id();
        let other = Handle::<Gltf>::Uuid(uuid::Uuid::from_u128(1), std::marker::PhantomData).id();
        let failure = Arc::new(AssetLoadError::EmptyPath("".into()));
        let failed = RecursiveDependencyLoadState::Failed(Arc::clone(&failure));
        let mut events = Messages::default();
        events.write(AssetEvent::LoadedWithDependencies { id });
        let mut reload = DesktopNativeHostAssetReload::new(&events, &LoadState::Loaded, &failed);
        assert!(!reload.observe(id, &events, &LoadState::Loaded, &failed)?);
        assert!(!reload.observe(
            id,
            &events,
            &LoadState::Loading,
            &RecursiveDependencyLoadState::Loading
        )?);
        events.write(AssetEvent::Modified { id: other });
        assert!(!reload.observe(
            id,
            &events,
            &LoadState::Loaded,
            &RecursiveDependencyLoadState::Loaded
        )?);
        events.write(AssetEvent::Modified { id });
        assert!(!reload.observe(
            id,
            &events,
            &LoadState::Loaded,
            &RecursiveDependencyLoadState::Loading
        )?);
        assert!(reload.observe(
            id,
            &events,
            &LoadState::Loaded,
            &RecursiveDependencyLoadState::Loaded
        )?);
        Ok(())
    }

    #[test]
    fn native_host_asset_retry_preserves_new_root_and_dependency_failures()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = Handle::<Gltf>::default().id();
        let previous = Arc::new(AssetLoadError::EmptyPath("".into()));
        let current = Arc::new(AssetLoadError::EmptyPath("".into()));
        for recursive in [false, true] {
            let events = Messages::default();
            let mut reload = DesktopNativeHostAssetReload::new(
                &events,
                &LoadState::Failed(Arc::clone(&previous)),
                &RecursiveDependencyLoadState::Failed(Arc::clone(&previous)),
            );
            assert!(!reload.observe(
                id,
                &events,
                &LoadState::Failed(Arc::clone(&previous)),
                &RecursiveDependencyLoadState::Failed(Arc::clone(&previous))
            )?);
            let (root, dependencies) = if recursive {
                (
                    LoadState::Loaded,
                    RecursiveDependencyLoadState::Failed(Arc::clone(&current)),
                )
            } else {
                (
                    LoadState::Failed(Arc::clone(&current)),
                    RecursiveDependencyLoadState::Loading,
                )
            };
            assert!(matches!(
                reload.observe(id, &events, &root, &dependencies),
                Err(DesktopNativeHostError::AssetLoad(error)) if Arc::ptr_eq(&error, &current)
            ));
        }
        Ok(())
    }

    #[test]
    fn native_host_asset_retry_revalidates_previously_loaded_but_invalid_templates()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = Handle::<Gltf>::default().id();
        let mut events = Messages::default();
        let mut reload = DesktopNativeHostAssetReload::new(
            &events,
            &LoadState::Loaded,
            &RecursiveDependencyLoadState::Loaded,
        );
        assert!(!reload.observe(
            id,
            &events,
            &LoadState::Loaded,
            &RecursiveDependencyLoadState::Loaded
        )?);
        events.write(AssetEvent::Added { id });
        assert!(reload.observe(
            id,
            &events,
            &LoadState::Loaded,
            &RecursiveDependencyLoadState::Loaded
        )?);
        Ok(())
    }

    #[test]
    fn native_host_quit_shortcut_requires_a_modifier_and_fresh_keypress() {
        let mut unmodified = ButtonInput::default();
        unmodified.press(KeyCode::KeyQ);
        assert!(!DesktopNativeHost::is_quit_shortcut(&unmodified));
        for modifier in DesktopNativeHost::quit_modifiers() {
            let mut keys = ButtonInput::default();
            keys.press(modifier);
            assert!(!DesktopNativeHost::is_quit_shortcut(&keys));
            keys.press(KeyCode::KeyQ);
            assert!(DesktopNativeHost::is_quit_shortcut(&keys));
            keys.clear();
            assert!(!DesktopNativeHost::is_quit_shortcut(&keys));
        }
    }
}
