use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::Duration;

use bevy::app::{First, Last, OnAppExitSystems, Startup, Update};
use bevy::asset::{AssetId, AssetPlugin, AssetServer, Assets, Handle, LoadState};
use bevy::ecs::message::Messages;
use bevy::ecs::system::SystemState;
use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::pbr::{MeshMaterial3d, PreparedMaterial};
use bevy::picking::mesh_picking::ray_cast::MeshRayCast;
use bevy::prelude::{
    App, AppExit, Camera2d, DefaultPlugins, Entity, IntoScheduleConfigs, Mesh, Mesh3d, PluginGroup,
    Res, Resource, StandardMaterial, With, World,
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
use bevy::render::{Render, RenderApp, RenderPlugin, RenderStartup, RenderSystems};
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
use crate::clock::time_provider::ClockTimeProviderMonotonic;
use crate::desktop::application::{
    DesktopApplicationError, DesktopApplicationPort, DesktopApplicationReceipt,
};
use crate::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEnumResult, DesktopRequest,
};
use crate::desktop::native::dialogs::{
    DesktopNativeDialogs, DesktopNativeDialogsError, DesktopNativeDialogsPort,
    DesktopNativeDialogsRfd,
};
use crate::desktop::native::event_loop::{DesktopNativeEventLoopError, DesktopNativeEventLoopWake};
use crate::desktop::native::recovery::{
    DesktopNativeRecovery, DesktopNativeRecoveryEnumAction, DesktopNativeRecoveryEnumStage,
    DesktopNativeRecoveryError, DesktopNativeRecoveryFailure, DesktopNativeRecoveryProvider,
};
use crate::desktop::native::resources::DesktopNativeResources;
use crate::desktop::native::task_pools::{DesktopNativeTaskPools, DesktopNativeTaskPoolsError};
use crate::presentation::app::{PresentationApp, PresentationAppEnumAction, PresentationAppError};
use crate::presentation::board::enums::PresentationBoardEnumCameraAction;
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::theme::font::PresentationThemeFont;
use crate::scene::assets::{SceneAssets, SceneAssetsError};
use crate::scene::camera::{SceneCamera, SceneCameraError};
use crate::scene::picking::{ScenePickingViewport, ScenePickingViewportError};
use crate::scene::renderer::{SceneRenderer, SceneRendererError, SceneRendererGeneration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeHostEvent {
    Idle,
    PreparationRequested { generation: CapacityPhaseGeneration },
    FrameworkReady { generation: CapacityPhaseGeneration },
    Interrupted { generation: CapacityPhaseGeneration },
}

pub struct DesktopNativeHostServices {
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
    AssetLoad(String),
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
}

#[derive(Resource)]
struct DesktopNativeHostState {
    services: DesktopNativeHostServices,
    presentation: PresentationApp,
    asset: Handle<Gltf>,
    camera: SceneCamera,
    generation: CapacityPhaseGeneration,
    ready: bool,
    fonts_initialized: bool,
    recovery: Option<DesktopNativeRecovery>,
    failure: Option<DesktopNativeHostError>,
    interruption_announced: bool,
    quit: Option<(DesktopRequest, Option<DesktopApplicationReceipt>)>,
    replacement: Option<RenderResources>,
    expected_device: Option<RenderDevice>,
    validated_surface: Option<Entity>,
    pending_view: Option<ScenePickingViewport>,
    pending_camera: Option<SceneCamera>,
    dialog_ready: bool,
    dialog_admission_closed: bool,
    exit_authorized: bool,
}

#[derive(Resource, Clone)]
struct DesktopNativeHostRenderProbe {
    target: Handle<bevy::prelude::Image>,
    generation: SceneRendererGeneration,
    meshes: Vec<AssetId<Mesh>>,
    materials: Vec<AssetId<StandardMaterial>>,
    ready: Arc<AtomicBool>,
}

type DesktopNativeHostDialogs =
    DesktopNativeDialogs<DesktopNativeDialogsRfd, Arc<DesktopNativeEventLoopWake>>;

pub struct DesktopNativeHost;

impl DesktopNativeHost {
    const TITLE: &str = "Chess";
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
        let generation = Self::preparing_generation(&services)?;
        let Some(render_resources) = Self::startup_graphics(&services, generation)? else {
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
        app.insert_resource(DesktopNativeHostState {
            services,
            presentation: PresentationApp::new(),
            asset,
            camera: SceneCamera::initial(BoardEnumColor::White),
            generation,
            ready: false,
            fonts_initialized: false,
            recovery: None,
            failure: None,
            interruption_announced: false,
            quit: None,
            replacement: None,
            expected_device: None,
            validated_surface: None,
            pending_view: None,
            pending_camera: None,
            dialog_ready: false,
            dialog_admission_closed: false,
            exit_authorized: false,
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
            .add_systems(RenderStartup, Self::reset_egui)
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
                        };
                        main.insert_resource(probe.clone());
                        render.insert_resource(probe);
                    }
                }
                extract(main, render);
            });
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
    ) -> Result<Option<RenderResources>, DesktopNativeHostError> {
        loop {
            match Self::graphics(&services.runtime, None) {
                Ok(resources) => return Ok(Some(resources)),
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
            let adapters = runtime.block_on(instance.enumerate_adapters(backends));
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
            runtime
                .block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                    ..wgpu::RequestAdapterOptions::default()
                }))
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
        let (device, queue) = runtime
            .block_on(adapter.request_device(&descriptor))
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
                    PresentationAppEnumAction::Refresh,
                    0,
                    &state.services.port,
                )
            },
        );
        if let Err(error) = outcome {
            Self::record_failure(world, DesktopNativeHostError::Presentation(error));
        }
    }

    fn acknowledge_wake(state: Res<'_, DesktopNativeHostState>) {
        state.services.wake.acknowledge();
        state.services.port.acknowledge_wake();
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
        Self::activate_window(world, state);
        Self::poll_dialogs(world, state)?;
        if state.failure.is_some() && state.recovery.is_none() {
            Self::begin_interruption(world, state)?;
        }
        if let Some(mut messages) = world.get_resource_mut::<Messages<WindowCloseRequested>>()
            && messages.drain().next().is_some()
        {
            if state.recovery.take().is_some() {
                Self::begin_interruption(world, state)?;
            } else {
                state.presentation.request_quit();
            }
        }
        if let Some((request, receipt)) = &mut state.quit {
            if receipt.is_none() {
                match state.services.port.try_submit(request) {
                    Ok(admitted) => *receipt = Some(admitted),
                    Err(DesktopApplicationError::Busy) => state.services.port.rearm_wake()?,
                    Err(error) => return Err(error.into()),
                }
            }
            if let Some(response) = receipt
                .as_ref()
                .and_then(DesktopApplicationReceipt::response)
            {
                if matches!(response, DesktopMessagesEnumResult::QuitReady) {
                    Self::try_exit(world, state)?;
                    return Ok(());
                }
                return Err(DesktopNativeHostError::QuitRefused(Box::new(
                    response.clone(),
                )));
            }
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
        state: &DesktopNativeHostState,
    ) -> Result<(), DesktopNativeHostError> {
        let server = world.resource::<AssetServer>();
        if let Some(LoadState::Failed(error)) = server.get_load_state(state.asset.id()) {
            return Err(DesktopNativeHostError::AssetLoad(error.to_string()));
        }
        if !server.is_loaded_with_dependencies(state.asset.id()) {
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
        let snapshot = state
            .presentation
            .snapshot()
            .ok_or(DesktopNativeHostError::MissingState)?;
        let position = BoardPosition::from_fen(snapshot.board().fen(), snapshot.board().variant())?;
        renderer.apply_position(world, &position)?;
        world.insert_resource(renderer);
        Ok(())
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
        outcome
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
            state
                .services
                .events
                .send_replace(DesktopNativeHostEvent::PreparationRequested { generation });
            context.request_repaint();
        }
        Ok(())
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

    fn reset_egui(world: &mut World) {
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
            DesktopNativeHostError::AssetLoad(_) | DesktopNativeHostError::Assets(_) => {
                DesktopNativeRecoveryEnumStage::Assets
            }
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
            | DesktopNativeHostError::Presentation(_)
            | DesktopNativeHostError::System(_) => DesktopNativeRecoveryEnumStage::Renderer,
            DesktopNativeHostError::Dialogs(_) => DesktopNativeRecoveryEnumStage::Renderer,
            DesktopNativeHostError::Graphics { .. } => DesktopNativeRecoveryEnumStage::Renderer,
        };
        DesktopNativeRecoveryFailure::new(stage, error.to_string())
    }

    fn record_failure(world: &mut World, error: DesktopNativeHostError) {
        let Some(mut state) = world.remove_resource::<DesktopNativeHostState>() else {
            return;
        };
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
        if state.recovery.is_some() {
            return Ok(());
        }
        let Some(error) = state.failure.as_ref() else {
            return Ok(());
        };
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
        state.recovery = Some(DesktopNativeRecovery::new(Self::failure(error), generation));
        state
            .services
            .events
            .send_replace(DesktopNativeHostEvent::PreparationRequested { generation });
        if let Some(mut renderer) = world.remove_resource::<SceneRenderer>() {
            let outcome = renderer.fail(world);
            world.insert_resource(renderer);
            outcome?;
        }
        world.insert_resource(WinitSettings {
            focused_mode: UpdateMode::reactive_low_power(Duration::MAX),
            unfocused_mode: UpdateMode::reactive_low_power(Duration::MAX),
        });
        Ok(())
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
                if state.quit.is_none() {
                    let request = state
                        .services
                        .port
                        .request(DesktopCommand::new(DesktopEnumCommand::Quit, None))?;
                    let receipt = match state.services.port.try_submit(&request) {
                        Ok(receipt) => Some(receipt),
                        Err(DesktopApplicationError::Busy) => {
                            state.services.port.rearm_wake()?;
                            None
                        }
                        Err(error) => return Err(error.into()),
                    };
                    state.quit = Some((request, receipt));
                }
            }
            DesktopNativeRecoveryEnumAction::Retry => {
                if state.replacement.is_none() {
                    let mut query = world.query_filtered::<Entity, With<PrimaryWindow>>();
                    let window = query
                        .single(world)
                        .map_err(|_error| DesktopNativeHostError::PrimaryWindow)?;
                    let replacement = match Self::graphics(&state.services.runtime, Some(window)) {
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
                    if let Some(mut renderer) = world.remove_resource::<SceneRenderer>() {
                        renderer.begin_retry()?;
                        renderer.request_frame(world)?;
                        world.insert_resource(renderer);
                    }
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
            state.failure = None;
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

    use crate::capacity::budget::CapacityBudget;
    use crate::capacity::phase::CapacityPhase;
    use crate::clock::time_provider::ClockTimeProviderMonotonic;
    use crate::desktop::application::{DesktopApplication, DesktopApplicationPort};
    use crate::desktop::native::dialogs::DesktopNativeDialogsError;
    use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;
    use crate::desktop::native::host::{DesktopNativeHost, DesktopNativeHostError};

    struct DesktopNativeHostFixture {
        wake: Arc<DesktopNativeEventLoopWake>,
        port: DesktopApplicationPort<ClockTimeProviderMonotonic, DesktopNativeEventLoopWake>,
        capacity: Mutex<CapacityPhase>,
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
            Ok(Self {
                wake,
                port: application.port(),
                capacity: Mutex::new(CapacityPhase::new(CapacityBudget::new(workers)?)?),
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
}
