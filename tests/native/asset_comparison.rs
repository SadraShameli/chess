use std::path::Path;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use bevy::app::{Last, MainScheduleOrder};
use bevy::asset::{AssetServer, Assets, Handle, LoadState};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::ecs::system::{NonSendMarker, SystemParam};
use bevy::gltf::Gltf;
use bevy::math::Vec3;
use bevy::prelude::{
    AppExit, Camera3d, Children, ClearColor, Color, Commands, Component, DirectionalLight, Entity,
    Image, Mesh, Mesh3d, MeshMaterial3d, MessageWriter, Name, On, Query, Res, ResMut, Resource,
    StandardMaterial, Startup, Transform, Update, Visibility, With, Without, WorldAssetRoot,
};
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::shape::Cuboid;
use bevy::window::{PrimaryWindow, Window};
use bevy::world_serialization::WorldInstanceReady;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use chess::board::enums::BoardEnumPieceKind;
use chess::board::position::BoardPosition;
use chess::package::digest::PackageDigest;
use chess::presentation::theme::font::PresentationThemeFont;
use chess::scene::assets::SceneAssetsPieceKey;
use serde::Serialize;
use thiserror::Error;

use crate::native_compatibility::{NativeCompatibilityError, NativeCompatibilityHarness};

#[derive(Debug, Error)]
pub(super) enum NativeAssetComparisonError {
    #[error(transparent)]
    Compatibility(#[from] NativeCompatibilityError),
    #[error("Native asset comparison I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Native asset comparison metadata failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Native asset comparison image conversion or encoding failed: {source}")]
    Image {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("The {0} piece library has no named ChessPieceLibrary scene")]
    MissingScene(&'static str),
    #[error("The {source_name} library does not instantiate exactly one parent named {name}")]
    MissingParent {
        source_name: &'static str,
        name: String,
    },
    #[error("The {source_name} piece library failed to load: {message}")]
    AssetLoad {
        source_name: &'static str,
        message: String,
    },
    #[error(
        "Native comparison did not finish in thirty seconds: ready={ready:?}, captures={captures}"
    )]
    Deadline { ready: [bool; 2], captures: usize },
    #[error("Native comparison requires exactly one camera")]
    Camera,
    #[error("Native comparison piece indexing is outside its supported range: {0}")]
    Index(#[from] std::num::TryFromIntError),
    #[error("Native comparison will not overwrite {0}")]
    OutputExists(std::path::PathBuf),
}

#[derive(Component, Clone, Copy)]
enum NativeAssetComparisonEnumSource {
    Original,
    Candidate,
}

impl NativeAssetComparisonEnumSource {
    const ALL: [Self; 2] = [Self::Original, Self::Candidate];

    fn index(self) -> usize {
        match self {
            Self::Original => 0,
            Self::Candidate => 1,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Candidate => "candidate",
        }
    }

    fn path(self) -> &'static str {
        match self {
            Self::Original => "chess.glb",
            Self::Candidate => "candidate.glb",
        }
    }

    fn horizontal(self, distance: f32) -> f32 {
        match self {
            Self::Original => -distance,
            Self::Candidate => distance,
        }
    }
}

#[derive(Clone, Copy)]
enum NativeAssetComparisonEnumView {
    OverviewFront,
    OverviewRear,
    BishopsFront,
    BishopsSide,
}

impl NativeAssetComparisonEnumView {
    fn next(self) -> Option<Self> {
        match self {
            Self::OverviewFront => Some(Self::OverviewRear),
            Self::OverviewRear => Some(Self::BishopsFront),
            Self::BishopsFront => Some(Self::BishopsSide),
            Self::BishopsSide => None,
        }
    }

    fn filename(self) -> &'static str {
        match self {
            Self::OverviewFront => "lineup-front.png",
            Self::OverviewRear => "lineup-rear.png",
            Self::BishopsFront => "bishops-front.png",
            Self::BishopsSide => "bishops-side.png",
        }
    }

    fn is_detail(self) -> bool {
        match self {
            Self::OverviewFront | Self::OverviewRear => false,
            Self::BishopsFront | Self::BishopsSide => true,
        }
    }

    fn camera(self) -> Transform {
        let eye = match self {
            Self::OverviewFront => Vec3::new(0.0, 7.0, 12.0),
            Self::OverviewRear => Vec3::new(0.0, 7.0, -12.0),
            Self::BishopsFront => Vec3::new(4.5, 3.8, 6.2),
            Self::BishopsSide => Vec3::new(-4.5, 3.8, 6.2),
        };
        Transform::from_translation(eye).looking_at(Vec3::new(0.0, 0.45, 0.0), Vec3::Y)
    }
}

#[derive(Component)]
struct NativeAssetComparisonCamera;

#[derive(Component)]
struct NativeAssetComparisonPiece {
    source: NativeAssetComparisonEnumSource,
    key: SceneAssetsPieceKey,
    ordinal: u8,
    authored: Transform,
}

impl NativeAssetComparisonPiece {
    const KINDS: u8 = 6;

    fn placement(&self, view: NativeAssetComparisonEnumView) -> Transform {
        let mut transform = self.authored;
        let row = if self.ordinal < Self::KINDS {
            1.1
        } else {
            -1.1
        };
        let x = if view.is_detail() {
            self.source.horizontal(1.35)
        } else {
            self.source.horizontal(4.2) + (f32::from(self.ordinal % Self::KINDS) - 2.5) * 1.2
        };
        transform.translation += Vec3::new(x, 0.0, row);
        transform
    }
}

struct NativeAssetComparisonCapture {
    view: NativeAssetComparisonEnumView,
    image: Image,
}

struct NativeAssetComparisonReport {
    adapter: String,
    backend: String,
    captures: Vec<NativeAssetComparisonCapture>,
}

#[derive(Serialize)]
struct NativeAssetComparisonCaptureReceipt {
    filename: &'static str,
    width: u32,
    height: u32,
}

#[derive(Serialize)]
struct NativeAssetComparisonReceipt {
    original_sha256: String,
    candidate_sha256: String,
    adapter: String,
    backend: String,
    named_parents_per_source: usize,
    captures: Vec<NativeAssetComparisonCaptureReceipt>,
}

#[derive(Resource)]
struct NativeAssetComparisonState {
    started: Instant,
    main_thread: ThreadId,
    result: SyncSender<Result<NativeAssetComparisonReport, NativeAssetComparisonError>>,
    assets: Vec<Handle<Gltf>>,
    roots_spawned: bool,
    ready: [bool; 2],
    bishop_keys: Vec<SceneAssetsPieceKey>,
    view: Option<NativeAssetComparisonEnumView>,
    arranged: bool,
    settling_frames: u32,
    capture_pending: bool,
    captures: Vec<NativeAssetComparisonCapture>,
    needs_fonts: bool,
    finished: bool,
}

impl NativeAssetComparisonState {
    fn complete(
        &mut self,
        outcome: Result<NativeAssetComparisonReport, NativeAssetComparisonError>,
    ) -> Result<(), NativeAssetComparisonError> {
        self.result
            .try_send(outcome)
            .map_err(|_error| NativeCompatibilityError::ResultChannel)?;
        self.finished = true;
        Ok(())
    }
}

#[derive(SystemParam)]
struct NativeAssetComparisonProbe<'w, 's> {
    commands: Commands<'w, 's>,
    assets: Res<'w, AssetServer>,
    models: Res<'w, Assets<Gltf>>,
    adapter: Option<Res<'w, RenderAdapterInfo>>,
    camera: Query<
        'w,
        's,
        &'static mut Transform,
        (
            With<NativeAssetComparisonCamera>,
            Without<NativeAssetComparisonPiece>,
        ),
    >,
    pieces: Query<
        'w,
        's,
        (
            &'static NativeAssetComparisonPiece,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        Without<NativeAssetComparisonCamera>,
    >,
    state: ResMut<'w, NativeAssetComparisonState>,
    _main_thread: NonSendMarker,
}

#[derive(SystemParam)]
struct NativeAssetComparisonReady<'w, 's> {
    commands: Commands<'w, 's>,
    children: Query<'w, 's, &'static Children>,
    sources: Query<'w, 's, &'static NativeAssetComparisonEnumSource>,
    parents: Query<'w, 's, (Entity, &'static Name, &'static Transform)>,
    state: ResMut<'w, NativeAssetComparisonState>,
}

#[derive(ScheduleLabel, Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct NativeAssetComparisonFinalExit;

pub(super) struct NativeAssetComparisonHarness;

impl NativeAssetComparisonHarness {
    const TITLE: &str = "Original and native-generated sculpted pieces";
    const DEADLINE: Duration = Duration::from_secs(30);
    const SETTLING_FRAMES: u32 = 16;
    const CAPTURES: usize = 4;
    const METADATA: &'static str = "comparison.json";
    const LIBRARY_SCENE: &'static str = "ChessPieceLibrary";

    pub(super) fn run(
        candidate: &Path,
        output_directory: &Path,
    ) -> Result<(), NativeAssetComparisonError> {
        let temporary = tempfile::tempdir()?;
        let original = include_bytes!("../fixtures/assets/three-original.glb");
        NativeCompatibilityHarness::prepare_asset(
            temporary.path(),
            original,
            include_str!("../fixtures/assets/three-original-provenance.json"),
        )?;
        let candidate_digest = PackageDigest::read(std::fs::File::open(candidate)?)
            .map_err(NativeCompatibilityError::Digest)?;
        let original_digest =
            PackageDigest::read(original.as_slice()).map_err(NativeCompatibilityError::Digest)?;
        std::fs::copy(candidate, temporary.path().join("candidate.glb"))?;
        std::fs::create_dir_all(output_directory)?;
        let asset_path = temporary
            .path()
            .to_str()
            .ok_or(NativeCompatibilityError::AssetPath)?
            .to_owned();
        let (sender, receiver) = sync_channel(1);
        let mut app = NativeCompatibilityHarness::host(asset_path, Self::TITLE, (1600, 1000));
        let bishop_keys = BoardPosition::standard()
            .pieces()
            .filter(|(_square, piece)| piece.kind() == BoardEnumPieceKind::Bishop)
            .map(|(_square, piece)| SceneAssetsPieceKey::from(piece))
            .collect();
        app.insert_resource(NativeAssetComparisonState {
            started: Instant::now(),
            main_thread: thread::current().id(),
            result: sender,
            assets: Vec::new(),
            roots_spawned: false,
            ready: [false; 2],
            bishop_keys,
            view: Some(NativeAssetComparisonEnumView::OverviewFront),
            arranged: false,
            settling_frames: 0,
            capture_pending: false,
            captures: Vec::with_capacity(Self::CAPTURES),
            needs_fonts: true,
            finished: false,
        })
        .insert_resource(ClearColor(Color::srgb_u8(17, 21, 26)))
        .add_systems(Startup, Self::setup)
        .add_systems(Update, Self::probe)
        .add_systems(EguiPrimaryContextPass, Self::labels)
        .add_systems(NativeAssetComparisonFinalExit, Self::exit);
        app.world_mut()
            .resource_mut::<MainScheduleOrder>()
            .insert_after(Last, NativeAssetComparisonFinalExit);
        NativeCompatibilityHarness::validate_pools()?;
        let exit = app.run();
        if !exit.is_success() {
            return Err(NativeCompatibilityError::EventLoop(exit).into());
        }
        let report = receiver
            .try_recv()
            .map_err(|_error| NativeCompatibilityError::Incomplete)??;
        Self::save(report, output_directory, original_digest, candidate_digest)?;
        temporary.close()?;
        Ok(())
    }

    fn setup(
        mut commands: Commands<'_, '_>,
        assets: Res<'_, AssetServer>,
        mut meshes: ResMut<'_, Assets<Mesh>>,
        mut materials: ResMut<'_, Assets<StandardMaterial>>,
        mut state: ResMut<'_, NativeAssetComparisonState>,
    ) {
        commands.spawn((
            Camera3d::default(),
            NativeAssetComparisonEnumView::OverviewFront.camera(),
            NativeAssetComparisonCamera,
        ));
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(18.0, 0.1, 7.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb_u8(83, 106, 93),
                perceptual_roughness: 0.8,
                ..StandardMaterial::default()
            })),
            Transform::from_xyz(0.0, -0.05, 0.0),
        ));
        for (illuminance, position) in [
            (12_000.0, Vec3::new(-3.0, 8.0, 4.0)),
            (4_000.0, Vec3::new(4.0, 5.0, -3.0)),
        ] {
            commands.spawn((
                DirectionalLight {
                    illuminance,
                    shadow_maps_enabled: true,
                    ..DirectionalLight::default()
                },
                Transform::from_translation(position).looking_at(Vec3::ZERO, Vec3::Y),
            ));
        }
        state.assets = NativeAssetComparisonEnumSource::ALL
            .into_iter()
            .map(|source| assets.load(source.path()))
            .collect();
    }

    fn probe(mut probe: NativeAssetComparisonProbe<'_, '_>) -> bevy::prelude::Result {
        if probe.state.finished {
            return Ok(());
        }
        if let Err(error) = Self::advance(&mut probe) {
            probe.state.complete(Err(error))?;
        }
        Ok(())
    }

    fn advance(
        probe: &mut NativeAssetComparisonProbe<'_, '_>,
    ) -> Result<(), NativeAssetComparisonError> {
        if thread::current().id() != probe.state.main_thread {
            return Err(NativeCompatibilityError::MainThread.into());
        }
        if probe.state.started.elapsed() >= Self::DEADLINE {
            return Err(NativeAssetComparisonError::Deadline {
                ready: probe.state.ready,
                captures: probe.state.captures.len(),
            });
        }
        for (source, handle) in NativeAssetComparisonEnumSource::ALL
            .into_iter()
            .zip(&probe.state.assets)
        {
            if let Some(LoadState::Failed(error)) = probe.assets.get_load_state(handle.id()) {
                return Err(NativeAssetComparisonError::AssetLoad {
                    source_name: source.name(),
                    message: error.to_string(),
                });
            }
        }
        if !probe.state.roots_spawned {
            if !probe
                .state
                .assets
                .iter()
                .all(|handle| probe.assets.is_loaded_with_dependencies(handle.id()))
            {
                return Ok(());
            }
            for (source, handle) in NativeAssetComparisonEnumSource::ALL
                .into_iter()
                .zip(&probe.state.assets)
            {
                let model = probe
                    .models
                    .get(handle)
                    .ok_or(NativeAssetComparisonError::MissingScene(source.name()))?;
                for key in SceneAssetsPieceKey::all() {
                    if !model.named_nodes.contains_key(key.name().as_str()) {
                        return Err(NativeAssetComparisonError::MissingParent {
                            source_name: source.name(),
                            name: key.name(),
                        });
                    }
                }
                let scene = model
                    .named_scenes
                    .get(Self::LIBRARY_SCENE)
                    .cloned()
                    .ok_or(NativeAssetComparisonError::MissingScene(source.name()))?;
                probe
                    .commands
                    .spawn((WorldAssetRoot(scene), source))
                    .observe(Self::ready);
            }
            probe.state.roots_spawned = true;
            return Ok(());
        }
        if !probe.state.ready.into_iter().all(|ready| ready) || probe.state.capture_pending {
            return Ok(());
        }
        let Some(view) = probe.state.view else {
            return Ok(());
        };
        if !probe.state.arranged {
            *probe
                .camera
                .single_mut()
                .map_err(|_error| NativeAssetComparisonError::Camera)? = view.camera();
            for (piece, mut transform, mut visibility) in &mut probe.pieces {
                *transform = piece.placement(view);
                *visibility = if !view.is_detail() || probe.state.bishop_keys.contains(&piece.key) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            probe.state.arranged = true;
            probe.state.settling_frames = 0;
            return Ok(());
        }
        if probe.state.settling_frames < Self::SETTLING_FRAMES {
            probe.state.settling_frames += 1;
        } else if probe.adapter.is_some() {
            probe.state.capture_pending = true;
            probe
                .commands
                .spawn(Screenshot::primary_window())
                .observe(Self::captured);
        }
        Ok(())
    }

    fn ready(
        ready: On<'_, '_, WorldInstanceReady>,
        mut scene: NativeAssetComparisonReady<'_, '_>,
    ) -> bevy::prelude::Result {
        let source = *scene.sources.get(ready.entity)?;
        for (ordinal, key) in SceneAssetsPieceKey::all().enumerate() {
            let name = key.name();
            let matches = scene
                .children
                .iter_descendants(ready.entity)
                .filter_map(|entity| scene.parents.get(entity).ok())
                .filter(|(_entity, actual, _transform)| actual.as_str() == name)
                .collect::<Vec<_>>();
            let [(entity, _name, transform)] = matches.as_slice() else {
                scene
                    .state
                    .complete(Err(NativeAssetComparisonError::MissingParent {
                        source_name: source.name(),
                        name,
                    }))?;
                return Ok(());
            };
            scene
                .commands
                .entity(*entity)
                .insert(NativeAssetComparisonPiece {
                    source,
                    key,
                    ordinal: u8::try_from(ordinal)?,
                    authored: **transform,
                });
        }
        scene.state.ready[source.index()] = true;
        Ok(())
    }

    fn labels(
        mut contexts: EguiContexts<'_, '_>,
        mut state: ResMut<'_, NativeAssetComparisonState>,
    ) -> bevy::prelude::Result {
        let context = contexts.ctx_mut()?;
        if state.needs_fonts {
            context.set_fonts(PresentationThemeFont::definitions());
            state.needs_fonts = false;
            return Ok(());
        }
        let mut root = egui::Ui::new(
            context.clone(),
            egui::Id::new(Self::TITLE),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                .max_rect(context.content_rect()),
        );
        egui::Panel::top(Self::TITLE).show(&mut root, |ui| {
            ui.visuals_mut().override_text_color = Some(egui::Color32::WHITE);
            ui.heading(Self::TITLE);
            ui.label("Original on the left, Rust candidate on the right in the front view.");
            ui.label("Ivory and ebony rows: pawn, knight, bishop, rook, queen, king.");
            if let Some(view) = state.view {
                ui.label(view.filename());
            }
        });
        Ok(())
    }

    fn captured(
        captured: On<'_, '_, ScreenshotCaptured>,
        windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
        adapter: Res<'_, RenderAdapterInfo>,
        mut state: ResMut<'_, NativeAssetComparisonState>,
    ) -> bevy::prelude::Result {
        if state.finished {
            return Ok(());
        }
        let outcome = windows
            .single()
            .map_err(|_error| {
                NativeAssetComparisonError::Compatibility(NativeCompatibilityError::PrimaryWindow)
            })
            .and_then(|window| {
                Self::record_capture(
                    &captured.image,
                    (window.physical_width(), window.physical_height()),
                    &adapter,
                    &mut state,
                )
            });
        if let Err(error) = outcome {
            state.complete(Err(error))?;
        }
        Ok(())
    }

    fn record_capture(
        image: &Image,
        expected: (u32, u32),
        adapter: &RenderAdapterInfo,
        state: &mut NativeAssetComparisonState,
    ) -> Result<(), NativeAssetComparisonError> {
        let actual = (image.width(), image.height());
        if actual != expected {
            return Err(NativeCompatibilityError::ScreenshotDimensions { expected, actual }.into());
        }
        NativeCompatibilityHarness::classify_pixels(image)?;
        let Some(view) = state.view else {
            return Err(NativeCompatibilityError::Incomplete.into());
        };
        state.captures.push(NativeAssetComparisonCapture {
            view,
            image: image.clone(),
        });
        state.view = view.next();
        state.arranged = false;
        state.capture_pending = false;
        if state.view.is_none() {
            let report = NativeAssetComparisonReport {
                adapter: adapter.name.clone(),
                backend: format!("{:?}", adapter.backend),
                captures: std::mem::take(&mut state.captures),
            };
            state.complete(Ok(report))?;
        }
        Ok(())
    }

    fn exit(state: Res<'_, NativeAssetComparisonState>, mut exits: MessageWriter<'_, AppExit>) {
        if state.finished {
            exits.write(AppExit::Success);
        }
    }

    fn save(
        report: NativeAssetComparisonReport,
        output_directory: &Path,
        original_digest: PackageDigest,
        candidate_digest: PackageDigest,
    ) -> Result<(), NativeAssetComparisonError> {
        let mut captures = Vec::with_capacity(Self::CAPTURES);
        for capture in report.captures {
            let filename = capture.view.filename();
            let destination = output_directory.join(filename);
            if destination.exists() {
                return Err(NativeAssetComparisonError::OutputExists(destination));
            }
            let receipt = NativeAssetComparisonCaptureReceipt {
                filename,
                width: capture.image.width(),
                height: capture.image.height(),
            };
            let dynamic = capture.image.try_into_dynamic().map_err(|source| {
                NativeAssetComparisonError::Image {
                    source: Box::new(source),
                }
            })?;
            dynamic
                .save(destination)
                .map_err(|source| NativeAssetComparisonError::Image {
                    source: Box::new(source),
                })?;
            captures.push(receipt);
        }
        let receipt = NativeAssetComparisonReceipt {
            original_sha256: original_digest.to_string(),
            candidate_sha256: candidate_digest.to_string(),
            adapter: report.adapter,
            backend: report.backend,
            named_parents_per_source: SceneAssetsPieceKey::all().count(),
            captures,
        };
        let metadata = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output_directory.join(Self::METADATA))?;
        serde_json::to_writer_pretty(metadata, &receipt)?;
        println!(
            "Native asset comparison saved {} GPU captures to {} via {} ({})",
            receipt.captures.len(),
            output_directory.display(),
            receipt.adapter,
            receipt.backend
        );
        Ok(())
    }
}
