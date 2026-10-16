use std::path::Path;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use bevy::app::{Last, MainScheduleOrder, TaskPoolPlugin};
use bevy::asset::{
    AssetPlugin, AssetServer, Assets, DependencyLoadState, Handle, LoadState,
    RecursiveDependencyLoadState, RenderAssetUsages,
};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::ecs::system::{NonSendMarker, SystemParam};
use bevy::gltf::Gltf;
use bevy::math::Vec3;
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::prelude::{
    App, AppExit, Camera3d, ClearColor, Color, Commands, DefaultPlugins, Image, Mesh, Mesh3d,
    MessageReader, MessageWriter, On, PluginGroup, Query, Res, ResMut, Resource, Startup,
    Transform, Update, With,
};
use bevy::render::RenderPlugin;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::shape::Cuboid;
use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool};
use bevy::window::{PrimaryWindow, Window, WindowOccluded, WindowPlugin};
use bevy::winit::WinitPlugin;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use serde::Deserialize;
use thiserror::Error;

use chess::package::digest::{PackageDigest, PackageDigestError};

use crate::scene_task_pools::SceneTaskPoolFixture;
use chess::presentation::theme::font::PresentationThemeFont;

#[path = "asset_comparison.rs"]
mod asset_comparison;

#[derive(Debug, Error)]
pub(super) enum NativeCompatibilityError {
    #[error("Native compatibility fixture I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Native compatibility asset provenance is invalid: {0}")]
    Provenance(#[from] serde_json::Error),
    #[error("Native compatibility asset checksum is invalid: {0}")]
    Digest(#[from] PackageDigestError),
    #[error("The native compatibility asset directory is not valid UTF-8")]
    AssetPath,
    #[error("The original piece GLB does not match its declared checksum or size")]
    AssetIntegrity,
    #[error("The original piece GLB must declare twelve named piece parents")]
    AssetRegistry,
    #[error("The original piece GLB failed to load: {0}")]
    AssetLoad(String),
    #[error("The original piece GLB is missing the declared node {0}")]
    MissingNode(String),
    #[error("The native compatibility host did not retain one thread per Bevy task pool")]
    TaskPools,
    #[error("The native compatibility update left the OS event-loop thread")]
    MainThread,
    #[error(
        "Native graphics initialization/readback exceeded ten seconds: {ui_frames} UI frames, {geometry_pixels} geometry pixels, {text_pixels} text pixels, adapter={adapter_ready}, capture_requested={capture_requested}, captures_received={captures_received}, model={model_loaded}, assets={asset_states:?}, {diagnostics}"
    )]
    Deadline {
        ui_frames: u32,
        geometry_pixels: usize,
        text_pixels: usize,
        adapter_ready: bool,
        capture_requested: bool,
        captures_received: u32,
        model_loaded: bool,
        asset_states: Option<(LoadState, DependencyLoadState, RecursiveDependencyLoadState)>,
        diagnostics: String,
    },
    #[error("The native screenshot did not contain a supported complete RGBA/BGRA image")]
    Screenshot,
    #[error(
        "The native screenshot dimensions {actual:?} did not match the primary window dimensions {expected:?}"
    )]
    ScreenshotDimensions {
        expected: (u32, u32),
        actual: (u32, u32),
    },
    #[error("The native compatibility host requires one primary window")]
    PrimaryWindow,
    #[error("The native pixel classifier failed its deterministic input self-check")]
    ClassifierSelfCheck,
    #[error("The native compatibility result channel is unavailable")]
    ResultChannel,
    #[error("The native compatibility event loop exited before completing GPU readback")]
    Incomplete,
    #[error("The native compatibility event loop failed: {0:?}")]
    EventLoop(AppExit),
}

#[derive(Deserialize)]
struct NativeCompatibilityAssetManifest {
    asset: NativeCompatibilityAssetIdentity,
    pieces: Vec<NativeCompatibilityPieceIdentity>,
}

#[derive(Deserialize)]
struct NativeCompatibilityAssetIdentity {
    sha256: String,
    bytes: usize,
}

#[derive(Deserialize)]
struct NativeCompatibilityPieceIdentity {
    nodes: Vec<String>,
}

struct NativeCompatibilityReport {
    adapter: String,
    backend: String,
    named_pieces: usize,
    geometry_pixels: usize,
    text_pixels: usize,
    dimensions: (u32, u32),
}

#[derive(Clone, Copy)]
struct NativeCompatibilityReadback {
    format: TextureFormat,
    width: u32,
    height: u32,
    first: [u8; NativeCompatibilityHarness::BYTES_PER_PIXEL],
    last: [u8; NativeCompatibilityHarness::BYTES_PER_PIXEL],
    max_rgb: [u8; 3],
    nonblack_pixels: usize,
    geometry_pixels: usize,
    text_pixels: usize,
}

impl std::fmt::Display for NativeCompatibilityReadback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{:?} {}x{}, first={:?}, last={:?}, max_rgb={:?}, nonblack={}, geometry={}, text={}",
            self.format,
            self.width,
            self.height,
            self.first,
            self.last,
            self.max_rgb,
            self.nonblack_pixels,
            self.geometry_pixels,
            self.text_pixels
        )
    }
}

struct NativeCompatibilityWindow {
    focused: bool,
    visible: bool,
    occluded: Option<bool>,
    dimensions: (u32, u32),
}

impl std::fmt::Display for NativeCompatibilityWindow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "focused={}, visible={}, occluded={:?}, dimensions={:?}",
            self.focused, self.visible, self.occluded, self.dimensions
        )
    }
}

#[derive(Resource)]
struct NativeCompatibilityState {
    started: Instant,
    main_thread: ThreadId,
    result: SyncSender<Result<NativeCompatibilityReport, NativeCompatibilityError>>,
    required_nodes: Vec<String>,
    asset: Option<Handle<Gltf>>,
    capture_requested: bool,
    finished: bool,
    needs_fonts: bool,
    ui_frames: u32,
    geometry_pixels: usize,
    text_pixels: usize,
    captures_received: u32,
    asset_ready: bool,
    first_readback: Option<NativeCompatibilityReadback>,
    latest_readback: Option<NativeCompatibilityReadback>,
    window: Option<NativeCompatibilityWindow>,
    occluded: Option<bool>,
    capture_dimensions: Option<(u32, u32)>,
}

impl NativeCompatibilityState {
    fn complete(
        &mut self,
        outcome: Result<NativeCompatibilityReport, NativeCompatibilityError>,
    ) -> Result<(), NativeCompatibilityError> {
        self.result
            .try_send(outcome)
            .map_err(|_error| NativeCompatibilityError::ResultChannel)?;
        self.finished = true;
        Ok(())
    }
}

#[derive(SystemParam)]
struct NativeCompatibilityProbe<'w, 's> {
    commands: Commands<'w, 's>,
    assets: Res<'w, AssetServer>,
    models: Res<'w, Assets<Gltf>>,
    adapter: Option<Res<'w, RenderAdapterInfo>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    occlusions: MessageReader<'w, 's, WindowOccluded>,
    state: ResMut<'w, NativeCompatibilityState>,
    _main_thread: NonSendMarker,
}

pub(super) struct NativeCompatibilityHarness;

#[derive(ScheduleLabel, Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct NativeCompatibilityFinalExit;

impl NativeCompatibilityHarness {
    const TITLE: &str = "Chess native compatibility smoke";
    const SIDEBAR: &str = "native-compatibility-status";
    const WINDOW_WIDTH: u32 = 1280;
    const WINDOW_HEIGHT: u32 = 800;
    const SIDEBAR_WIDTH: f32 = 300.0;
    const DEADLINE: Duration = Duration::from_secs(10);
    const PIECE_PARENTS: usize = 12;
    const MIN_GEOMETRY_PIXELS: usize = 64;
    const MIN_TEXT_PIXELS: usize = 16;
    const COLOR_THRESHOLD: u8 = 180;
    const COLOR_MARGIN: u8 = 50;
    const BYTES_PER_PIXEL: usize = 4;
    const MIN_UI_FRAMES: u32 = 2;

    pub(super) fn run() -> Result<(), NativeCompatibilityError> {
        Self::validate_pixel_classifier()?;
        let fixture = tempfile::tempdir()?;
        let required_nodes = Self::prepare_asset(
            fixture.path(),
            include_bytes!("../../resources/pieces/chess.glb"),
            include_str!("../../resources/pieces/provenance.json"),
        )?;
        let asset_path = fixture
            .path()
            .to_str()
            .ok_or(NativeCompatibilityError::AssetPath)?
            .to_owned();
        let (sender, receiver) = sync_channel(1);
        let mut app = Self::host(
            asset_path,
            Self::TITLE,
            (Self::WINDOW_WIDTH, Self::WINDOW_HEIGHT),
        );
        app.insert_resource(NativeCompatibilityState {
            started: Instant::now(),
            main_thread: thread::current().id(),
            result: sender,
            required_nodes,
            asset: None,
            capture_requested: false,
            finished: false,
            needs_fonts: true,
            ui_frames: 0,
            geometry_pixels: 0,
            text_pixels: 0,
            captures_received: 0,
            asset_ready: false,
            first_readback: None,
            latest_readback: None,
            window: None,
            occluded: None,
            capture_dimensions: None,
        })
        .insert_resource(ClearColor(Color::BLACK))
        .add_systems(Startup, Self::setup)
        .add_systems(Update, Self::probe)
        .add_systems(EguiPrimaryContextPass, Self::show_status)
        .add_systems(NativeCompatibilityFinalExit, Self::exit);
        app.world_mut()
            .resource_mut::<MainScheduleOrder>()
            .insert_after(Last, NativeCompatibilityFinalExit);
        Self::validate_pools()?;
        let exit = app.run();
        if !exit.is_success() {
            return Err(NativeCompatibilityError::EventLoop(exit));
        }
        Self::report(receiver)?;
        Ok(())
    }

    pub(super) fn compare_assets(
        candidate: &Path,
        output_directory: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        asset_comparison::NativeAssetComparisonHarness::run(candidate, output_directory)?;
        Ok(())
    }

    fn host(asset_path: String, title: &str, dimensions: (u32, u32)) -> App {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set::<TaskPoolPlugin>(SceneTaskPoolFixture::plugin())
                .set(AssetPlugin {
                    file_path: asset_path,
                    watch_for_changes_override: Some(false),
                    ..AssetPlugin::default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: title.to_owned(),
                        resolution: dimensions.into(),
                        ..Window::default()
                    }),
                    ..WindowPlugin::default()
                })
                .set(WinitPlugin {
                    run_on_any_thread: false,
                })
                .set(RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..RenderPlugin::default()
                })
                .disable::<PipelinedRenderingPlugin>(),
        )
        .add_plugins(Self::egui_plugin());
        app
    }

    fn prepare_asset(
        destination: &Path,
        bytes: &[u8],
        provenance: &str,
    ) -> Result<Vec<String>, NativeCompatibilityError> {
        let manifest: NativeCompatibilityAssetManifest = serde_json::from_str(provenance)?;
        if bytes.len() != manifest.asset.bytes
            || PackageDigest::read(bytes)? != manifest.asset.sha256.parse::<PackageDigest>()?
        {
            return Err(NativeCompatibilityError::AssetIntegrity);
        }
        let mut names: Vec<String> = manifest
            .pieces
            .into_iter()
            .flat_map(|piece| piece.nodes)
            .collect();
        names.sort_unstable();
        names.dedup();
        if names.len() != Self::PIECE_PARENTS {
            return Err(NativeCompatibilityError::AssetRegistry);
        }
        std::fs::write(destination.join("chess.glb"), bytes)?;
        Ok(names)
    }

    fn egui_plugin() -> EguiPlugin {
        EguiPlugin {
            bindless_mode_array_size: if cfg!(target_os = "macos") {
                None
            } else {
                EguiPlugin::default().bindless_mode_array_size
            },
        }
    }

    fn validate_pools() -> Result<(), NativeCompatibilityError> {
        if [
            IoTaskPool::get().thread_num(),
            AsyncComputeTaskPool::get().thread_num(),
            ComputeTaskPool::get().thread_num(),
        ]
        .into_iter()
        .all(|threads| threads == SceneTaskPoolFixture::THREADS_PER_POOL)
        {
            Ok(())
        } else {
            Err(NativeCompatibilityError::TaskPools)
        }
    }

    fn setup(
        mut commands: Commands<'_, '_>,
        mut meshes: ResMut<'_, Assets<Mesh>>,
        mut materials: ResMut<'_, Assets<StandardMaterial>>,
        assets: Res<'_, AssetServer>,
        mut state: ResMut<'_, NativeCompatibilityState>,
    ) {
        commands.spawn((
            Camera3d::default(),
            Transform::from_xyz(0.0, 0.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::default())),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.05, 0.8, 0.1),
                unlit: true,
                ..StandardMaterial::default()
            })),
        ));
        state.asset = Some(assets.load("chess.glb"));
    }

    fn show_status(
        mut contexts: EguiContexts<'_, '_>,
        mut state: ResMut<'_, NativeCompatibilityState>,
    ) -> bevy::prelude::Result {
        let context = contexts.ctx_mut()?;
        if state.needs_fonts {
            context.set_fonts(PresentationThemeFont::definitions());
            state.needs_fonts = false;
            return Ok(());
        }
        let mut root_ui = egui::Ui::new(
            context.clone(),
            egui::Id::new(Self::SIDEBAR),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                .max_rect(context.content_rect()),
        );
        egui::Panel::left(Self::SIDEBAR)
            .exact_size(Self::SIDEBAR_WIDTH)
            .show(&mut root_ui, |ui| {
                ui.visuals_mut().override_text_color = Some(egui::Color32::WHITE);
                ui.heading(Self::TITLE);
                ui.label("Bevy, egui, original GLB, and GPU readback");
                ui.label("Compatibility initialization only");
                ui.label("Product-host acceptance remains a later task");
            });
        state.ui_frames += 1;
        Ok(())
    }

    fn probe(mut probe: NativeCompatibilityProbe<'_, '_>) -> bevy::prelude::Result {
        if probe.state.finished {
            return Ok(());
        }
        for event in probe.occlusions.read() {
            probe.state.occluded = Some(event.occluded);
        }
        probe.state.window = probe
            .windows
            .iter()
            .next()
            .map(|window| NativeCompatibilityWindow {
                focused: window.focused,
                visible: window.visible,
                occluded: probe.state.occluded,
                dimensions: (window.physical_width(), window.physical_height()),
            });
        if thread::current().id() != probe.state.main_thread {
            probe
                .state
                .complete(Err(NativeCompatibilityError::MainThread))?;
            return Ok(());
        }
        if probe.state.started.elapsed() >= Self::DEADLINE {
            let asset_states = probe
                .state
                .asset
                .as_ref()
                .and_then(|handle| probe.assets.get_load_states(handle.id()));
            let model_loaded = probe
                .state
                .asset
                .as_ref()
                .is_some_and(|handle| probe.models.contains(handle));
            let failure = NativeCompatibilityError::Deadline {
                ui_frames: probe.state.ui_frames,
                geometry_pixels: probe.state.geometry_pixels,
                text_pixels: probe.state.text_pixels,
                adapter_ready: probe.adapter.is_some(),
                capture_requested: probe.state.capture_requested,
                captures_received: probe.state.captures_received,
                model_loaded,
                asset_states,
                diagnostics: format!(
                    "first_readback={:?}, latest_readback={:?}, window={:?}",
                    probe.state.first_readback.as_ref().map(ToString::to_string),
                    probe
                        .state
                        .latest_readback
                        .as_ref()
                        .map(ToString::to_string),
                    probe.state.window.as_ref().map(ToString::to_string),
                ),
            };
            probe.state.complete(Err(failure))?;
            return Ok(());
        }
        if probe.adapter.is_some()
            && probe.state.ui_frames >= Self::MIN_UI_FRAMES
            && !probe.state.capture_requested
        {
            let window = probe
                .windows
                .single()
                .map_err(|_error| NativeCompatibilityError::PrimaryWindow)?;
            probe.state.capture_dimensions =
                Some((window.physical_width(), window.physical_height()));
            probe.state.capture_requested = true;
            probe
                .commands
                .spawn(Screenshot::primary_window())
                .observe(Self::captured);
        }
        let Some(handle) = &probe.state.asset else {
            return Ok(());
        };
        if let Some(LoadState::Failed(error)) = probe.assets.get_load_state(handle.id()) {
            probe
                .state
                .complete(Err(NativeCompatibilityError::AssetLoad(error.to_string())))?;
            return Ok(());
        }
        let Some(model) = probe.models.get(handle) else {
            return Ok(());
        };
        if let Some(missing) = probe
            .state
            .required_nodes
            .iter()
            .find(|name| !model.named_nodes.contains_key(name.as_str()))
        {
            let failure = NativeCompatibilityError::MissingNode(missing.clone());
            probe.state.complete(Err(failure))?;
            return Ok(());
        }
        probe.state.asset_ready = probe.assets.is_loaded_with_dependencies(handle.id());
        Ok(())
    }

    fn captured(
        captured: On<'_, '_, ScreenshotCaptured>,
        mut state: ResMut<'_, NativeCompatibilityState>,
        adapter: Res<'_, RenderAdapterInfo>,
    ) -> bevy::prelude::Result {
        if state.finished {
            return Ok(());
        }
        state.captures_received += 1;
        let actual_dimensions = (captured.image.width(), captured.image.height());
        let expected_dimensions = state
            .capture_dimensions
            .ok_or(NativeCompatibilityError::PrimaryWindow)?;
        if actual_dimensions != expected_dimensions {
            return Err(NativeCompatibilityError::ScreenshotDimensions {
                expected: expected_dimensions,
                actual: actual_dimensions,
            }
            .into());
        }
        let readback = Self::classify_pixels(&captured.image)?;
        state.first_readback.get_or_insert(readback);
        state.latest_readback = Some(readback);
        let geometry_pixels = readback.geometry_pixels;
        let text_pixels = readback.text_pixels;
        state.geometry_pixels = geometry_pixels;
        state.text_pixels = text_pixels;
        if geometry_pixels < Self::MIN_GEOMETRY_PIXELS
            || text_pixels < Self::MIN_TEXT_PIXELS
            || !state.asset_ready
        {
            state.capture_requested = false;
            return Ok(());
        }
        Self::validate_pools()?;
        let report = NativeCompatibilityReport {
            adapter: adapter.name.clone(),
            backend: format!("{:?}", adapter.backend),
            named_pieces: state.required_nodes.len(),
            geometry_pixels,
            text_pixels,
            dimensions: actual_dimensions,
        };
        state.complete(Ok(report))?;
        Ok(())
    }

    fn exit(state: Res<'_, NativeCompatibilityState>, mut exits: MessageWriter<'_, AppExit>) {
        if state.finished {
            exits.write(AppExit::Success);
        }
    }

    fn classify_pixels(
        image: &Image,
    ) -> Result<NativeCompatibilityReadback, NativeCompatibilityError> {
        if !matches!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8Unorm
                | TextureFormat::Rgba8UnormSrgb
                | TextureFormat::Bgra8Unorm
                | TextureFormat::Bgra8UnormSrgb
        ) {
            return Err(NativeCompatibilityError::Screenshot);
        }
        let expected = usize::try_from(image.width())
            .ok()
            .and_then(|width| {
                usize::try_from(image.height())
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(Self::BYTES_PER_PIXEL))
            .ok_or(NativeCompatibilityError::Screenshot)?;
        let bytes = image
            .data
            .as_ref()
            .ok_or(NativeCompatibilityError::Screenshot)?;
        if expected == 0 || bytes.len() != expected {
            return Err(NativeCompatibilityError::Screenshot);
        }
        let pixels = bytes.as_chunks::<{ Self::BYTES_PER_PIXEL }>().0;
        let mut readback = NativeCompatibilityReadback {
            format: image.texture_descriptor.format,
            width: image.width(),
            height: image.height(),
            first: *pixels.first().ok_or(NativeCompatibilityError::Screenshot)?,
            last: *pixels.last().ok_or(NativeCompatibilityError::Screenshot)?,
            max_rgb: [0; 3],
            nonblack_pixels: 0,
            geometry_pixels: 0,
            text_pixels: 0,
        };
        for pixel in pixels {
            for (maximum, channel) in readback.max_rgb.iter_mut().zip(&pixel[..3]) {
                *maximum = (*maximum).max(*channel);
            }
            if pixel[..3].iter().any(|channel| *channel != 0) {
                readback.nonblack_pixels += 1;
            }
            if pixel[1] > pixel[0].saturating_add(Self::COLOR_MARGIN)
                && pixel[1] > pixel[2].saturating_add(Self::COLOR_MARGIN)
            {
                readback.geometry_pixels += 1;
            }
            if pixel[..3]
                .iter()
                .all(|channel| *channel >= Self::COLOR_THRESHOLD)
            {
                readback.text_pixels += 1;
            }
        }
        Ok(readback)
    }

    fn validate_pixel_classifier() -> Result<(), NativeCompatibilityError> {
        const PIXELS: [[u8; NativeCompatibilityHarness::BYTES_PER_PIXEL]; 8] = [
            [0, 0, 0, 255],
            [12, 85, 30, 255],
            [255, 255, 255, 255],
            [179, 180, 180, 255],
            [200, 251, 200, 255],
            [205, 255, 205, 255],
            [0, 50, 0, 255],
            [0, 51, 0, 255],
        ];
        for format in [
            TextureFormat::Rgba8Unorm,
            TextureFormat::Rgba8UnormSrgb,
            TextureFormat::Bgra8Unorm,
            TextureFormat::Bgra8UnormSrgb,
        ] {
            let mut image = Image::new(
                Extent3d {
                    width: u32::try_from(PIXELS.len())
                        .map_err(|_error| NativeCompatibilityError::ClassifierSelfCheck)?,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                PIXELS.into_iter().flatten().collect(),
                format,
                RenderAssetUsages::MAIN_WORLD,
            );
            let readback = Self::classify_pixels(&image)?;
            if readback.geometry_pixels != 3
                || readback.text_pixels != 3
                || readback.nonblack_pixels != 7
                || readback.max_rgb != [255; 3]
                || readback.first != PIXELS[0]
                || readback.last != PIXELS[PIXELS.len() - 1]
            {
                return Err(NativeCompatibilityError::ClassifierSelfCheck);
            }
            let bytes = image
                .data
                .as_mut()
                .ok_or(NativeCompatibilityError::ClassifierSelfCheck)?;
            bytes.truncate(bytes.len().saturating_sub(1));
            Self::require_invalid_pixels(&image)?;
            image.data = None;
            Self::require_invalid_pixels(&image)?;
            image.texture_descriptor.format = TextureFormat::R8Unorm;
            Self::require_invalid_pixels(&image)?;
        }
        Ok(())
    }

    fn require_invalid_pixels(image: &Image) -> Result<(), NativeCompatibilityError> {
        if matches!(
            Self::classify_pixels(image),
            Err(NativeCompatibilityError::Screenshot)
        ) {
            Ok(())
        } else {
            Err(NativeCompatibilityError::ClassifierSelfCheck)
        }
    }

    fn report(
        receiver: Receiver<Result<NativeCompatibilityReport, NativeCompatibilityError>>,
    ) -> Result<(), NativeCompatibilityError> {
        let report = receiver
            .try_recv()
            .map_err(|_error| NativeCompatibilityError::Incomplete)??;
        println!(
            "Native compatibility passed: {} via {}, {} declared GLB pieces, {} geometry pixels, {} text pixels, {}x{} physical pixels",
            report.adapter,
            report.backend,
            report.named_pieces,
            report.geometry_pixels,
            report.text_pixels,
            report.dimensions.0,
            report.dimensions.1
        );
        Ok(())
    }
}
