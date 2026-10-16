use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::num::TryFromIntError;
use std::path::Path;

use bevy::color::{Color, ColorToComponents};
use bevy::math::{Mat3, Vec2, Vec3};
use boolmesh::prelude::{Manifold, OpType, compute_boolean};
use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use chess::capacity::budget::{CapacityBudget, CapacityBudgetError};
use chess::capacity::executor::{CapacityExecutor, CapacityExecutorError, CapacityExecutorJob};
use chess::package::digest::{PackageDigest, PackageDigestError};
use gltf_json::validation::{Checked, USize64, Validate};
use gltf_json::{Index, Root};
use serde::Serialize;
use thiserror::Error;

#[cfg(test)]
#[path = "../../tests/assets/canonical.rs"]
mod canonical;

#[derive(Debug, Error)]
pub(crate) enum ToolsAssetsPiecesError {
    #[error("Piece generation I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Piece generation JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Piece geometry exceeds the glTF index range: {0}")]
    Range(#[from] TryFromIntError),
    #[error("Piece asset digest failed: {0}")]
    Digest(#[from] PackageDigestError),
    #[error("Piece polygon triangulation failed: {0}")]
    Triangulation(#[from] earcutr::Error),
    #[error("Piece Boolean operation failed: {0}")]
    Boolean(String),
    #[error("Piece generation CPU execution failed: {0}")]
    Executor(#[from] CapacityExecutorError),
    #[error("Piece generation capacity admission failed: {0}")]
    Capacity(#[from] CapacityBudgetError),
    #[error("Piece generation was cancelled")]
    Cancelled,
    #[error("Several piece generation jobs failed: {0:?}")]
    Jobs(Vec<Self>),
    #[error("Piece geometry contains an invalid position, normal, triangle, or contour")]
    Geometry,
    #[error("Generated glTF validation failed: {0:?}")]
    Validation(Vec<String>),
    #[error("Generated asset differs from {0}; the existing file was preserved")]
    Drift(std::path::PathBuf),
}

pub(crate) struct ToolsAssetsPiecesOutput {
    binary: Vec<u8>,
    provenance: Vec<u8>,
}

impl ToolsAssetsPiecesOutput {
    pub(crate) fn binary(&self) -> &[u8] {
        &self.binary
    }

    pub(crate) fn provenance(&self) -> &[u8] {
        &self.provenance
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolsAssetsPiecesProvenance {
    schema_version: u8,
    title: &'static str,
    authorship: &'static str,
    rights: &'static str,
    generator: ToolsAssetsPiecesGeneratorIdentity,
    asset: ToolsAssetsPiecesAssetIdentity,
    coordinates: ToolsAssetsPiecesCoordinates,
    construction: ToolsAssetsPiecesConstruction,
    materials: Vec<ToolsAssetsPiecesFinish>,
    pieces: Vec<ToolsAssetsPiecesSculptureIdentity>,
    dependencies: Vec<ToolsAssetsPiecesDependency>,
    visual_acceptance: &'static str,
}

#[derive(Serialize)]
struct ToolsAssetsPiecesGeneratorIdentity {
    path: &'static str,
    sha256: String,
    command: &'static str,
}

#[derive(Serialize)]
struct ToolsAssetsPiecesAssetIdentity {
    path: &'static str,
    sha256: String,
    bytes: usize,
    format: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolsAssetsPiecesCoordinates {
    up: &'static str,
    knight_forward: &'static str,
    square_size: u8,
    origin: &'static str,
    placement: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolsAssetsPiecesConstruction {
    radial_segments: u16,
    curve_segments: u16,
    bevel_segments: u16,
    geometry_sharing: &'static str,
    external_resources: u8,
    required_extensions: [String; 0],
    metallic_factor: u8,
}

#[derive(Clone, Copy, Serialize)]
struct ToolsAssetsPiecesFinish {
    color: BoardEnumColor,
    name: &'static str,
    srgb: &'static str,
    roughness: f32,
    #[serde(skip)]
    channels: [u8; 3],
}

impl ToolsAssetsPiecesFinish {
    const REGISTRY: [Self; 2] = [
        Self {
            color: BoardEnumColor::White,
            name: "IvorySatin",
            srgb: "#f2e6cb",
            roughness: 0.32,
            channels: [242, 230, 203],
        },
        Self {
            color: BoardEnumColor::Black,
            name: "EbonySatin",
            srgb: "#202328",
            roughness: 0.36,
            channels: [32, 35, 40],
        },
    ];

    fn node_name(self, sculpture: ToolsAssetsPiecesSculpture) -> String {
        let color = match self.color {
            BoardEnumColor::White => "white",
            BoardEnumColor::Black => "black",
        };
        format!("{color}_{}", sculpture.name())
    }

    fn material(self) -> gltf_json::Material {
        let [red, green, blue] = self.channels;
        gltf_json::Material {
            name: Some(self.name.to_owned()),
            pbr_metallic_roughness: gltf_json::material::PbrMetallicRoughness {
                base_color_factor: gltf_json::material::PbrBaseColorFactor(
                    Color::srgb_u8(red, green, blue).to_linear().to_f32_array(),
                ),
                metallic_factor: gltf_json::material::StrengthFactor(0.0),
                roughness_factor: gltf_json::material::StrengthFactor(self.roughness),
                ..gltf_json::material::PbrMetallicRoughness::default()
            },
            ..gltf_json::Material::default()
        }
    }
}

#[derive(Serialize)]
struct ToolsAssetsPiecesSculptureIdentity {
    kind: BoardEnumPieceKind,
    silhouette: &'static str,
    nodes: Vec<String>,
    vertices: usize,
    triangles: usize,
    bounds: ToolsAssetsPiecesBounds,
}

#[derive(Clone, Copy, Serialize)]
struct ToolsAssetsPiecesBounds {
    min: [f32; 3],
    max: [f32; 3],
}

#[derive(Serialize)]
struct ToolsAssetsPiecesDependency {
    name: &'static str,
    version: &'static str,
    license: &'static str,
    role: &'static str,
}

pub(crate) struct ToolsAssetsPieces;

#[derive(Default)]
struct ToolsAssetsPiecesJobs {
    jobs: VecDeque<
        CapacityExecutorJob<
            Result<(ToolsAssetsPiecesSculpture, ToolsAssetsPiecesGeometry), ToolsAssetsPiecesError>,
        >,
    >,
}

impl ToolsAssetsPiecesJobs {
    async fn next(
        &mut self,
    ) -> Result<(ToolsAssetsPiecesSculpture, ToolsAssetsPiecesGeometry), ToolsAssetsPiecesError>
    {
        self.jobs
            .pop_front()
            .ok_or(ToolsAssetsPiecesError::Geometry)?
            .join()
            .await?
    }

    async fn cancel_and_drain(mut self, first: ToolsAssetsPiecesError) -> ToolsAssetsPiecesError {
        for job in &self.jobs {
            job.cancel();
        }
        let mut failures = vec![first];
        while !self.jobs.is_empty() {
            if let Err(error) = self.next().await {
                failures.push(error);
            }
        }
        if failures.len() == 1 {
            failures.remove(0)
        } else {
            ToolsAssetsPiecesError::Jobs(failures)
        }
    }
}

impl ToolsAssetsPieces {
    const RADIAL_SEGMENTS: u16 = 64;
    const CURVE_SEGMENTS: u16 = 12;
    const BEVEL_SEGMENTS: u16 = 3;
    const BINARY_NAME: &str = "chess.glb";
    const PROVENANCE_NAME: &str = "provenance.json";

    pub(crate) async fn run(
        destination: &Path,
        check: bool,
        executor: &CapacityExecutor,
        budget: &CapacityBudget,
    ) -> Result<(), ToolsAssetsPiecesError> {
        let generated = Self::generate(executor).await?;
        let binary = destination.join(Self::BINARY_NAME);
        let provenance = destination.join(Self::PROVENANCE_NAME);
        if check {
            let (existing_binary, existing_provenance) = Self::file_pair(
                budget,
                tokio::fs::read(&binary),
                tokio::fs::read(&provenance),
            )
            .await?;
            if existing_binary != generated.binary() {
                return Err(ToolsAssetsPiecesError::Drift(binary));
            }
            if existing_provenance != generated.provenance() {
                return Err(ToolsAssetsPiecesError::Drift(provenance));
            }
        } else {
            Self::file_operation(budget, tokio::fs::create_dir_all(destination)).await?;
            Self::file_pair(
                budget,
                tokio::fs::write(binary, generated.binary()),
                tokio::fs::write(provenance, generated.provenance()),
            )
            .await?;
        }
        Ok(())
    }

    async fn file_operation<T>(
        budget: &CapacityBudget,
        operation: impl Future<Output = Result<T, std::io::Error>>,
    ) -> Result<T, ToolsAssetsPiecesError> {
        let lease = budget.lease(1).await?;
        let result = operation.await;
        drop(lease);
        Ok(result?)
    }

    async fn file_pair<T, U>(
        budget: &CapacityBudget,
        first: impl Future<Output = Result<T, std::io::Error>>,
        second: impl Future<Output = Result<U, std::io::Error>>,
    ) -> Result<(T, U), ToolsAssetsPiecesError> {
        match tokio::join!(
            Self::file_operation(budget, first),
            Self::file_operation(budget, second),
        ) {
            (Ok(first), Ok(second)) => Ok((first, second)),
            (Err(first), Err(second)) => Err(ToolsAssetsPiecesError::Jobs(vec![first, second])),
            (Err(error), Ok(_)) | (Ok(_), Err(error)) => Err(error),
        }
    }

    pub(crate) async fn generate(
        executor: &CapacityExecutor,
    ) -> Result<ToolsAssetsPiecesOutput, ToolsAssetsPiecesError> {
        let mut pending = ToolsAssetsPiecesJobs::default();
        let mut sculptures = ToolsAssetsPiecesSculpture::REGISTRY.into_iter().peekable();
        let mut geometries = Vec::with_capacity(ToolsAssetsPiecesSculpture::REGISTRY.len());
        while let Some(sculpture) = sculptures.peek().copied() {
            match executor.submit(move |cancellation| {
                if cancellation.is_cancelled() {
                    return Err(ToolsAssetsPiecesError::Cancelled);
                }
                let geometry = sculpture.geometry()?.indexed()?;
                geometry.validate()?;
                if cancellation.is_cancelled() {
                    return Err(ToolsAssetsPiecesError::Cancelled);
                }
                Ok((sculpture, geometry))
            }) {
                Ok(job) => {
                    pending.jobs.push_back(job);
                    if sculptures.next().is_none() {
                        return Err(pending
                            .cancel_and_drain(ToolsAssetsPiecesError::Geometry)
                            .await);
                    }
                }
                Err(CapacityExecutorError::Budget(CapacityBudgetError::Unavailable))
                    if !pending.jobs.is_empty() =>
                {
                    match pending.next().await {
                        Ok(geometry) => geometries.push(geometry),
                        Err(error) => return Err(pending.cancel_and_drain(error).await),
                    }
                }
                Err(error) => return Err(pending.cancel_and_drain(error.into()).await),
            }
        }
        while !pending.jobs.is_empty() {
            match pending.next().await {
                Ok(geometry) => geometries.push(geometry),
                Err(error) => return Err(pending.cancel_and_drain(error).await),
            }
        }
        executor
            .submit(move |cancellation| {
                if cancellation.is_cancelled() {
                    return Err(ToolsAssetsPiecesError::Cancelled);
                }
                Self::assemble(geometries)
            })?
            .join()
            .await?
    }

    fn assemble(
        geometries: Vec<(ToolsAssetsPiecesSculpture, ToolsAssetsPiecesGeometry)>,
    ) -> Result<ToolsAssetsPiecesOutput, ToolsAssetsPiecesError> {
        let mut document = ToolsAssetsPiecesDocument::new();
        let mut pieces = Vec::with_capacity(ToolsAssetsPiecesSculpture::REGISTRY.len());
        for (sculpture, geometry) in geometries {
            let accessors = document.geometry(sculpture, &geometry)?;
            let mut nodes = Vec::with_capacity(ToolsAssetsPiecesFinish::REGISTRY.len());
            for (material, finish) in ToolsAssetsPiecesFinish::REGISTRY.into_iter().enumerate() {
                let name = finish.node_name(sculpture);
                document.piece(&name, &accessors, material)?;
                nodes.push(name);
            }
            pieces.push(ToolsAssetsPiecesSculptureIdentity {
                kind: sculpture.kind,
                silhouette: sculpture.silhouette(),
                nodes,
                vertices: geometry.vertices.len(),
                triangles: geometry.indices.len() / ToolsAssetsPiecesGeometry::TRIANGLE_VERTICES,
                bounds: geometry.bounds()?,
            });
        }
        let binary = document.finish()?;
        let provenance = ToolsAssetsPiecesProvenance {
            schema_version: 1,
            title: "Reckless sculpted chess pieces",
            authorship: "Original parametric artwork authored for this project. No downloaded models, textures or paid assets.",
            rights: "Project owner retains rights. Private personal use; no separate application or artwork license is granted.",
            generator: ToolsAssetsPiecesGeneratorIdentity {
                path: "tools/assets/pieces.rs",
                sha256: PackageDigest::read(include_bytes!("pieces.rs").as_slice())?.to_string(),
                command: "cargo run --locked --features asset-tools --bin chess-tools -- assets-generate resources/pieces",
            },
            asset: ToolsAssetsPiecesAssetIdentity {
                path: "resources/pieces/chess.glb",
                sha256: PackageDigest::read(binary.as_slice())?.to_string(),
                bytes: binary.len(),
                format: "glTF 2.0 binary",
            },
            coordinates: ToolsAssetsPiecesCoordinates {
                up: "+Y",
                knight_forward: "-Z",
                square_size: 1,
                origin: "Base center at x=0,z=0; bottom at y=0",
                placement: "Clone a named group; all library nodes overlap at the origin by design",
            },
            construction: ToolsAssetsPiecesConstruction {
                radial_segments: Self::RADIAL_SEGMENTS,
                curve_segments: Self::CURVE_SEGMENTS,
                bevel_segments: Self::BEVEL_SEGMENTS,
                geometry_sharing: "Six indexed geometries shared by twelve color variants",
                external_resources: 0,
                required_extensions: [],
                metallic_factor: 0,
            },
            materials: ToolsAssetsPiecesFinish::REGISTRY.to_vec(),
            pieces,
            dependencies: vec![
                ToolsAssetsPiecesDependency {
                    name: "bevy",
                    version: "0.20.0",
                    license: "MIT OR Apache-2.0",
                    role: "Parametric vector geometry and linear color conversion",
                },
                ToolsAssetsPiecesDependency {
                    name: "earcutr",
                    version: "0.5.0",
                    license: "ISC",
                    role: "Beveled silhouette cap triangulation",
                },
                ToolsAssetsPiecesDependency {
                    name: "boolmesh",
                    version: "0.2.1",
                    license: "MPL-2.0",
                    role: "Bishop mitre slit subtraction in offline asset tooling",
                },
                ToolsAssetsPiecesDependency {
                    name: "gltf-json",
                    version: "1.4.1",
                    license: "MIT OR Apache-2.0",
                    role: "Typed validated glTF serialization",
                },
            ],
            visual_acceptance: "Native procedural generation retains the original construction. Replacement validation and platform limits are recorded in specs/001-reckless-desktop-chess/evidence/native-assets.md.",
        };
        let mut provenance = serde_json::to_vec_pretty(&provenance)?;
        provenance.push(b'\n');
        Ok(ToolsAssetsPiecesOutput { binary, provenance })
    }
}

struct ToolsAssetsPiecesDocument {
    root: Root,
    binary: Vec<u8>,
}

struct ToolsAssetsPiecesAccessors {
    position: Index<gltf_json::Accessor>,
    normal: Index<gltf_json::Accessor>,
    indices: Index<gltf_json::Accessor>,
}

struct ToolsAssetsPiecesAccessorDefinition<'a> {
    sculpture: ToolsAssetsPiecesSculpture,
    suffix: &'static str,
    bytes: &'a [u8],
    count: usize,
    kind: gltf_json::accessor::Type,
    bounds: Option<ToolsAssetsPiecesBounds>,
}

impl ToolsAssetsPiecesDocument {
    const GLB_MAGIC: u32 = 0x4654_6c67;
    const GLB_VERSION: u32 = 2;
    const JSON_CHUNK: u32 = 0x4e4f_534a;
    const BINARY_CHUNK: u32 = 0x004e_4942;
    const ALIGNMENT: usize = 4;
    const GLB_OVERHEAD: usize = 28;

    fn new() -> Self {
        let mut root = Root::default();
        root.asset.generator =
            Some("Reckless original piece sculptures; Rust gltf-json 1.4.1".to_owned());
        root.asset.copyright =
            Some("Original project-owned parametric artwork. Private personal use.".to_owned());
        root.materials = ToolsAssetsPiecesFinish::REGISTRY
            .into_iter()
            .map(ToolsAssetsPiecesFinish::material)
            .collect();
        root.scenes.push(gltf_json::Scene {
            name: Some("ChessPieceLibrary".to_owned()),
            nodes: Vec::new(),
            extensions: None,
            extras: Default::default(),
        });
        root.scene = Some(Index::new(0));
        Self {
            root,
            binary: Vec::new(),
        }
    }

    fn index<T>(length: usize) -> Result<Index<T>, ToolsAssetsPiecesError> {
        Ok(Index::new(u32::try_from(length)?))
    }

    fn geometry(
        &mut self,
        sculpture: ToolsAssetsPiecesSculpture,
        geometry: &ToolsAssetsPiecesGeometry,
    ) -> Result<ToolsAssetsPiecesAccessors, ToolsAssetsPiecesError> {
        let bounds = geometry.bounds()?;
        let position_bytes = geometry
            .vertices
            .iter()
            .flat_map(|vertex| vertex.position.to_array())
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let normal_bytes = geometry
            .vertices
            .iter()
            .flat_map(|vertex| vertex.normal.to_array())
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let index_bytes = geometry
            .indices
            .iter()
            .copied()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let position = self.accessor(ToolsAssetsPiecesAccessorDefinition {
            sculpture,
            suffix: "position",
            bytes: &position_bytes,
            count: geometry.vertices.len(),
            kind: gltf_json::accessor::Type::Vec3,
            bounds: Some(bounds),
        })?;
        let normal = self.accessor(ToolsAssetsPiecesAccessorDefinition {
            sculpture,
            suffix: "normal",
            bytes: &normal_bytes,
            count: geometry.vertices.len(),
            kind: gltf_json::accessor::Type::Vec3,
            bounds: None,
        })?;
        let indices = self.accessor(ToolsAssetsPiecesAccessorDefinition {
            sculpture,
            suffix: "indices",
            bytes: &index_bytes,
            count: geometry.indices.len(),
            kind: gltf_json::accessor::Type::Scalar,
            bounds: None,
        })?;
        Ok(ToolsAssetsPiecesAccessors {
            position,
            normal,
            indices,
        })
    }

    fn accessor(
        &mut self,
        definition: ToolsAssetsPiecesAccessorDefinition<'_>,
    ) -> Result<Index<gltf_json::Accessor>, ToolsAssetsPiecesError> {
        let ToolsAssetsPiecesAccessorDefinition {
            sculpture,
            suffix,
            bytes,
            count,
            kind,
            bounds,
        } = definition;
        let name = format!("{}_{suffix}", sculpture.name());
        let is_indices = kind == gltf_json::accessor::Type::Scalar;
        let view = Self::index(self.root.buffer_views.len())?;
        self.root.buffer_views.push(gltf_json::buffer::View {
            buffer: Index::new(0),
            byte_length: USize64::from(bytes.len()),
            byte_offset: Some(USize64::from(self.binary.len())),
            byte_stride: None,
            name: Some(name.clone()),
            target: Some(Checked::Valid(if is_indices {
                gltf_json::buffer::Target::ElementArrayBuffer
            } else {
                gltf_json::buffer::Target::ArrayBuffer
            })),
            extensions: None,
            extras: Default::default(),
        });
        self.binary.extend_from_slice(bytes);
        let accessor = Self::index(self.root.accessors.len())?;
        self.root.accessors.push(gltf_json::Accessor {
            buffer_view: Some(view),
            byte_offset: None,
            count: USize64::from(count),
            component_type: Checked::Valid(gltf_json::accessor::GenericComponentType(
                if is_indices {
                    gltf_json::accessor::ComponentType::U32
                } else {
                    gltf_json::accessor::ComponentType::F32
                },
            )),
            extensions: None,
            extras: Default::default(),
            type_: Checked::Valid(kind),
            min: bounds
                .map(|bounds| serde_json::to_value(bounds.min))
                .transpose()?,
            max: bounds
                .map(|bounds| serde_json::to_value(bounds.max))
                .transpose()?,
            name: Some(name),
            normalized: false,
            sparse: None,
        });
        Ok(accessor)
    }

    fn piece(
        &mut self,
        name: &str,
        accessors: &ToolsAssetsPiecesAccessors,
        material: usize,
    ) -> Result<(), ToolsAssetsPiecesError> {
        let mesh = Self::index(self.root.meshes.len())?;
        self.root.meshes.push(gltf_json::Mesh {
            name: Some(format!("{name}_mesh")),
            primitives: vec![gltf_json::mesh::Primitive {
                attributes: BTreeMap::from([
                    (
                        Checked::Valid(gltf_json::mesh::Semantic::Positions),
                        accessors.position,
                    ),
                    (
                        Checked::Valid(gltf_json::mesh::Semantic::Normals),
                        accessors.normal,
                    ),
                ]),
                indices: Some(accessors.indices),
                material: Some(Self::index(material)?),
                mode: Checked::Valid(gltf_json::mesh::Mode::Triangles),
                targets: None,
                extensions: None,
                extras: Default::default(),
            }],
            weights: None,
            extensions: None,
            extras: Default::default(),
        });
        let child = Self::index(self.root.nodes.len())?;
        self.root.nodes.push(gltf_json::Node {
            name: Some(format!("{name}_sculpture")),
            mesh: Some(mesh),
            ..gltf_json::Node::default()
        });
        let parent = Self::index(self.root.nodes.len())?;
        self.root.nodes.push(gltf_json::Node {
            name: Some(name.to_owned()),
            children: Some(vec![child]),
            ..gltf_json::Node::default()
        });
        self.root.scenes[0].nodes.push(parent);
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<u8>, ToolsAssetsPiecesError> {
        self.root.buffers.push(gltf_json::Buffer {
            byte_length: USize64::from(self.binary.len()),
            name: Some("SharedPieceGeometry".to_owned()),
            uri: None,
            extensions: None,
            extras: Default::default(),
        });
        let mut errors = Vec::new();
        self.root
            .validate(&self.root, gltf_json::Path::new, &mut |path, error| {
                errors.push(format!("{}: {error:?}", path()))
            });
        if !errors.is_empty() {
            return Err(ToolsAssetsPiecesError::Validation(errors));
        }
        let mut json = serde_json::to_vec(&self.root)?;
        Self::pad(&mut json, b' ');
        Self::pad(&mut self.binary, 0);
        let total = Self::GLB_OVERHEAD
            .checked_add(json.len())
            .and_then(|length| length.checked_add(self.binary.len()))
            .ok_or(ToolsAssetsPiecesError::Geometry)?;
        let mut output = Vec::with_capacity(total);
        for value in [
            Self::GLB_MAGIC,
            Self::GLB_VERSION,
            u32::try_from(total)?,
            u32::try_from(json.len())?,
            Self::JSON_CHUNK,
        ] {
            output.extend_from_slice(&value.to_le_bytes());
        }
        output.extend_from_slice(&json);
        output.extend_from_slice(&u32::try_from(self.binary.len())?.to_le_bytes());
        output.extend_from_slice(&Self::BINARY_CHUNK.to_le_bytes());
        output.extend_from_slice(&self.binary);
        Ok(output)
    }

    fn pad(bytes: &mut Vec<u8>, fill: u8) {
        let padding = (Self::ALIGNMENT - bytes.len() % Self::ALIGNMENT) % Self::ALIGNMENT;
        bytes.resize(bytes.len() + padding, fill);
    }
}

#[derive(Clone, Copy)]
struct ToolsAssetsPiecesVertex {
    position: Vec3,
    normal: Vec3,
}

impl ToolsAssetsPiecesVertex {
    fn key(self) -> [u32; 6] {
        [
            self.position.x.to_bits(),
            self.position.y.to_bits(),
            self.position.z.to_bits(),
            self.normal.x.to_bits(),
            self.normal.y.to_bits(),
            self.normal.z.to_bits(),
        ]
    }
}

#[derive(Default)]
struct ToolsAssetsPiecesGeometry {
    vertices: Vec<ToolsAssetsPiecesVertex>,
    indices: Vec<u32>,
}

impl ToolsAssetsPiecesGeometry {
    const TRIANGLE_VERTICES: usize = 3;
    const NORMAL_TOLERANCE: f32 = 0.000_01;
    const DEGENERATE_AREA: f32 = 1.0e-20;

    fn vertex(&mut self, position: Vec3, normal: Vec3) -> Result<u32, ToolsAssetsPiecesError> {
        let index = u32::try_from(self.vertices.len())?;
        self.vertices
            .push(ToolsAssetsPiecesVertex { position, normal });
        Ok(index)
    }

    fn triangle(
        &mut self,
        points: [Vec3; Self::TRIANGLE_VERTICES],
    ) -> Result<(), ToolsAssetsPiecesError> {
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize_or_zero();
        if normal == Vec3::ZERO {
            return Ok(());
        }
        for point in points {
            let index = self.vertex(point, normal)?;
            self.indices.push(index);
        }
        Ok(())
    }

    fn combine(parts: Vec<Self>) -> Result<Self, ToolsAssetsPiecesError> {
        let mut combined = Self::default();
        for part in parts {
            let offset = u32::try_from(combined.vertices.len())?;
            combined.vertices.extend(part.vertices);
            for index in part.indices {
                combined.indices.push(
                    index
                        .checked_add(offset)
                        .ok_or(ToolsAssetsPiecesError::Geometry)?,
                );
            }
        }
        Ok(combined)
    }

    fn indexed(self) -> Result<Self, ToolsAssetsPiecesError> {
        let mut result = Self::default();
        let mut vertices = BTreeMap::new();
        for index in &self.indices {
            let vertex = self.vertex_at(*index)?;
            if !vertex.position.is_finite() || !vertex.normal.is_finite() {
                return Err(ToolsAssetsPiecesError::Geometry);
            }
            vertices.entry(vertex.key()).or_insert(vertex);
        }
        let mut registry = BTreeMap::new();
        for (key, vertex) in vertices {
            let index = result.vertex(vertex.position, vertex.normal)?;
            if registry.insert(key, index).is_some() {
                return Err(ToolsAssetsPiecesError::Geometry);
            }
        }
        if !self.indices.len().is_multiple_of(Self::TRIANGLE_VERTICES) {
            return Err(ToolsAssetsPiecesError::Geometry);
        }
        let mut triangles = Vec::with_capacity(self.indices.len() / Self::TRIANGLE_VERTICES);
        for triangle in self.indices.as_chunks::<3>().0 {
            let [a, b, c] = [
                self.canonical_index(triangle[0], &registry)?,
                self.canonical_index(triangle[1], &registry)?,
                self.canonical_index(triangle[2], &registry)?,
            ];
            let triangle = if b < a && b <= c {
                [b, c, a]
            } else if c < a && c < b {
                [c, a, b]
            } else {
                [a, b, c]
            };
            triangles.push(triangle);
        }
        triangles.sort_unstable();
        result.indices = triangles.into_iter().flatten().collect();
        Ok(result)
    }

    fn canonical_index(
        &self,
        index: u32,
        registry: &BTreeMap<[u32; 6], u32>,
    ) -> Result<u32, ToolsAssetsPiecesError> {
        registry
            .get(&self.vertex_at(index)?.key())
            .copied()
            .ok_or(ToolsAssetsPiecesError::Geometry)
    }

    fn transform(mut self, rotation: Mat3, translation: Vec3) -> Self {
        for vertex in &mut self.vertices {
            vertex.position = rotation * vertex.position + translation;
            vertex.normal = (rotation * vertex.normal).normalize_or_zero();
        }
        self
    }

    fn bounds(&self) -> Result<ToolsAssetsPiecesBounds, ToolsAssetsPiecesError> {
        let first = self
            .vertices
            .first()
            .ok_or(ToolsAssetsPiecesError::Geometry)?
            .position;
        let (min, max) = self
            .vertices
            .iter()
            .fold((first, first), |(min, max), vertex| {
                (min.min(vertex.position), max.max(vertex.position))
            });
        Ok(ToolsAssetsPiecesBounds {
            min: min.to_array(),
            max: max.to_array(),
        })
    }

    fn validate(&self) -> Result<(), ToolsAssetsPiecesError> {
        if self.vertices.is_empty()
            || self.indices.is_empty()
            || !self.indices.len().is_multiple_of(Self::TRIANGLE_VERTICES)
        {
            return Err(ToolsAssetsPiecesError::Geometry);
        }
        for vertex in &self.vertices {
            if !vertex.position.is_finite()
                || !vertex.normal.is_finite()
                || (vertex.normal.length_squared() - 1.0).abs() > Self::NORMAL_TOLERANCE
            {
                return Err(ToolsAssetsPiecesError::Geometry);
            }
        }
        for index in &self.indices {
            if usize::try_from(*index)? >= self.vertices.len() {
                return Err(ToolsAssetsPiecesError::Geometry);
            }
        }
        Ok(())
    }

    fn lathe(profile: &[[f32; 2]]) -> Result<Self, ToolsAssetsPiecesError> {
        if profile.len() < 2 {
            return Err(ToolsAssetsPiecesError::Geometry);
        }
        let mut geometry = Self::default();
        let mut profile_normals = Vec::with_capacity(profile.len());
        for (index, pair) in profile.windows(2).enumerate() {
            let delta = Vec2::from_array(pair[1]) - Vec2::from_array(pair[0]);
            let normal = Vec2::new(delta.y, -delta.x);
            if index == 0 {
                profile_normals.push(normal.normalize_or_zero());
            } else {
                let previous_delta =
                    Vec2::from_array(profile[index]) - Vec2::from_array(profile[index - 1]);
                let previous = Vec2::new(previous_delta.y, -previous_delta.x);
                profile_normals.push((previous + normal).normalize_or_zero());
            }
        }
        let last_delta = Vec2::from_array(profile[profile.len() - 1])
            - Vec2::from_array(profile[profile.len() - 2]);
        profile_normals.push(Vec2::new(last_delta.y, -last_delta.x).normalize_or_zero());
        for segment in 0..=ToolsAssetsPieces::RADIAL_SEGMENTS {
            let angle = if segment == ToolsAssetsPieces::RADIAL_SEGMENTS {
                0.0
            } else {
                f32::from(segment) / f32::from(ToolsAssetsPieces::RADIAL_SEGMENTS)
                    * std::f32::consts::TAU
            };
            let (sine, cosine) = angle.sin_cos();
            for ([radius, height], normal) in profile.iter().copied().zip(&profile_normals) {
                geometry.vertex(
                    Vec3::new(radius * sine, height, radius * cosine),
                    Vec3::new(normal.x * sine, normal.y, normal.x * cosine).normalize_or_zero(),
                )?;
            }
        }
        let stride = u32::try_from(profile.len())?;
        for segment in 0..u32::from(ToolsAssetsPieces::RADIAL_SEGMENTS) {
            for point in 0..stride - 1 {
                let a = segment * stride + point;
                let b = (segment + 1) * stride + point;
                let c = b + 1;
                let d = a + 1;
                geometry.indices.extend([a, b, d, c, d, b]);
            }
        }
        Ok(geometry)
    }

    fn sphere(size: Vec3, position: Vec3) -> Result<Self, ToolsAssetsPiecesError> {
        const WIDTH: u16 = 32;
        const HEIGHT: u16 = 20;
        let mut geometry = Self::default();
        for row in 0..=HEIGHT {
            let theta = f32::from(row) / f32::from(HEIGHT) * std::f32::consts::PI;
            for column in 0..=WIDTH {
                let phi = f32::from(column) / f32::from(WIDTH) * std::f32::consts::TAU;
                let unit = Vec3::new(
                    -phi.cos() * theta.sin(),
                    theta.cos(),
                    phi.sin() * theta.sin(),
                );
                geometry.vertex(unit * size + position, (unit / size).normalize_or_zero())?;
            }
        }
        let stride = u32::from(WIDTH) + 1;
        for row in 0..u32::from(HEIGHT) {
            for column in 0..u32::from(WIDTH) {
                let b = row * stride + column;
                let a = b + 1;
                let c = b + stride;
                let d = c + 1;
                if row != 0 {
                    geometry.indices.extend([a, b, d]);
                }
                if row != u32::from(HEIGHT) - 1 {
                    geometry.indices.extend([b, c, d]);
                }
            }
        }
        Ok(geometry)
    }

    fn extrude(
        mut contour: Vec<Vec2>,
        depth: f32,
        bevel: Vec2,
    ) -> Result<Self, ToolsAssetsPiecesError> {
        if contour.first() == contour.last() && contour.pop().is_none() {
            return Err(ToolsAssetsPiecesError::Geometry);
        }
        if contour.len() < 3 {
            return Err(ToolsAssetsPiecesError::Geometry);
        }
        let area: f32 = contour
            .iter()
            .zip(contour.iter().cycle().skip(1))
            .take(contour.len())
            .map(|(current, next)| current.perp_dot(*next))
            .sum();
        if area > 0.0 {
            contour.reverse();
        }
        let movements = (0..contour.len())
            .map(|index| {
                Self::bevel_movement(
                    contour[index],
                    contour[(index + contour.len() - 1) % contour.len()],
                    contour[(index + 1) % contour.len()],
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let coordinates = contour
            .iter()
            .flat_map(|point| [f64::from(point.x), f64::from(point.y)])
            .collect::<Vec<_>>();
        let faces = earcutr::earcut(&coordinates, &[], 2)?;
        let mut layers = Vec::new();
        for segment in 0..ToolsAssetsPieces::BEVEL_SEGMENTS {
            let angle = f32::from(segment) / f32::from(ToolsAssetsPieces::BEVEL_SEGMENTS)
                * std::f32::consts::FRAC_PI_2;
            layers.push((bevel.x * angle.sin(), -bevel.y * angle.cos() - depth / 2.0));
        }
        layers.push((bevel.x, -depth / 2.0));
        layers.push((bevel.x, depth / 2.0));
        for segment in (0..ToolsAssetsPieces::BEVEL_SEGMENTS).rev() {
            let angle = f32::from(segment) / f32::from(ToolsAssetsPieces::BEVEL_SEGMENTS)
                * std::f32::consts::FRAC_PI_2;
            layers.push((bevel.x * angle.sin(), depth / 2.0 + bevel.y * angle.cos()));
        }
        let positions = layers
            .iter()
            .map(|(size, height)| {
                contour
                    .iter()
                    .zip(&movements)
                    .map(|(point, movement)| (*point + *movement * *size).extend(*height))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut geometry = Self::default();
        let first = positions.first().ok_or(ToolsAssetsPiecesError::Geometry)?;
        let last = positions.last().ok_or(ToolsAssetsPiecesError::Geometry)?;
        for face in faces.as_chunks::<3>().0 {
            geometry.triangle([first[face[2]], first[face[1]], first[face[0]]])?;
            geometry.triangle([last[face[0]], last[face[1]], last[face[2]]])?;
        }
        for index in (0..contour.len()).rev() {
            let previous = (index + contour.len() - 1) % contour.len();
            for layer in positions.windows(2) {
                let [a, b, c, d] = [
                    layer[0][index],
                    layer[0][previous],
                    layer[1][previous],
                    layer[1][index],
                ];
                geometry.triangle([a, b, d])?;
                geometry.triangle([b, c, d])?;
            }
        }
        Ok(geometry)
    }

    fn bevel_movement(
        point: Vec2,
        previous: Vec2,
        next: Vec2,
    ) -> Result<Vec2, ToolsAssetsPiecesError> {
        let before = point - previous;
        let after = next - point;
        let cross = before.perp_dot(after);
        if before.length_squared() == 0.0 || after.length_squared() == 0.0 {
            return Err(ToolsAssetsPiecesError::Geometry);
        }
        let translation = if cross.abs() > f32::EPSILON {
            let shifted_before = previous + Vec2::new(-before.y, before.x) / before.length();
            let shifted_after = next + Vec2::new(-after.y, after.x) / after.length();
            let scale = (shifted_after - shifted_before).perp_dot(after) / cross;
            shifted_before + before * scale - point
        } else if before.dot(after) > 0.0 {
            Vec2::new(-before.y, before.x).normalize()
        } else {
            before.normalize() * std::f32::consts::SQRT_2
        };
        if translation.length_squared() > 2.0 {
            Ok(translation.normalize() * std::f32::consts::SQRT_2)
        } else {
            Ok(translation)
        }
    }

    fn cuboid(size: Vec3) -> Result<Self, ToolsAssetsPiecesError> {
        let [x, y, z] = (size / 2.0).to_array();
        let positions = [
            Vec3::new(-x, -y, -z),
            Vec3::new(x, -y, -z),
            Vec3::new(x, y, -z),
            Vec3::new(-x, y, -z),
            Vec3::new(-x, -y, z),
            Vec3::new(x, -y, z),
            Vec3::new(x, y, z),
            Vec3::new(-x, y, z),
        ];
        let mut geometry = Self::default();
        for [a, b, c, d] in [
            [0, 3, 2, 1],
            [4, 5, 6, 7],
            [0, 4, 7, 3],
            [1, 2, 6, 5],
            [0, 1, 5, 4],
            [3, 7, 6, 2],
        ] {
            geometry.triangle([positions[a], positions[b], positions[d]])?;
            geometry.triangle([positions[b], positions[c], positions[d]])?;
        }
        Ok(geometry)
    }

    fn manifold(&self) -> Result<Manifold<boolmesh::Vec3>, ToolsAssetsPiecesError> {
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        let mut values = Vec::new();
        for triangle in self.indices.as_chunks::<3>().0 {
            let [a, b, c] = [
                self.vertex_at(triangle[0])?,
                self.vertex_at(triangle[1])?,
                self.vertex_at(triangle[2])?,
            ];
            if (b.position - a.position)
                .cross(c.position - a.position)
                .length_squared()
                <= Self::DEGENERATE_AREA
            {
                continue;
            }
            for vertex in [a, b, c] {
                indices.push(positions.len() / 3);
                positions.extend(
                    vertex
                        .position
                        .to_array()
                        .map(|value| if value == 0.0 { 0.0 } else { f64::from(value) }),
                );
                values.push(boolmesh::Vec3::from_array(
                    vertex.normal.to_array().map(f64::from),
                ));
            }
        }
        Manifold::new_with_values(&positions, &indices, &values)
            .map_err(ToolsAssetsPiecesError::Boolean)
    }

    fn subtract(self, cutter: Self) -> Result<Self, ToolsAssetsPiecesError> {
        let result = compute_boolean(&self.manifold()?, &cutter.manifold()?, OpType::Subtract)
            .map_err(ToolsAssetsPiecesError::Boolean)?;
        let mut geometry = Self::default();
        if !result.hs.len().is_multiple_of(Self::TRIANGLE_VERTICES)
            || result.hs.len() != result.vals.len()
            || !result.is_manifold()
        {
            return Err(ToolsAssetsPiecesError::Geometry);
        }
        for (edges, normals) in result
            .hs
            .as_chunks::<3>()
            .0
            .iter()
            .zip(result.vals.as_chunks::<3>().0)
        {
            let points = [
                result.ps.get(edges[0].tail),
                result.ps.get(edges[1].tail),
                result.ps.get(edges[2].tail),
            ];
            let [Some(a), Some(b), Some(c)] = points else {
                return Err(ToolsAssetsPiecesError::Geometry);
            };
            let points = [a, b, c].map(|point| Vec3::from_array(point.as_vec3().to_array()));
            let winding = (points[1] - points[0])
                .cross(points[2] - points[0])
                .normalize_or_zero();
            if winding == Vec3::ZERO {
                continue;
            }
            for (point, normal) in points.into_iter().zip(normals) {
                let mut normal = Vec3::from_array(normal.as_vec3().to_array()).normalize_or_zero();
                if normal == Vec3::ZERO {
                    return Err(ToolsAssetsPiecesError::Geometry);
                }
                if normal.dot(winding) < 0.0 {
                    normal = -normal;
                }
                let index = geometry.vertex(point, normal)?;
                geometry.indices.push(index);
            }
        }
        Ok(geometry)
    }

    fn vertex_at(&self, index: u32) -> Result<ToolsAssetsPiecesVertex, ToolsAssetsPiecesError> {
        self.vertices
            .get(usize::try_from(index)?)
            .copied()
            .ok_or(ToolsAssetsPiecesError::Geometry)
    }
}

struct ToolsAssetsPiecesPath {
    points: Vec<Vec2>,
}

impl ToolsAssetsPiecesPath {
    fn new(x: f32, y: f32) -> Self {
        Self {
            points: vec![Vec2::new(x, y)],
        }
    }

    fn line(&mut self, x: f32, y: f32) {
        self.points.push(Vec2::new(x, y));
    }

    fn cubic(&mut self, controls: [Vec2; 3]) -> Result<(), ToolsAssetsPiecesError> {
        let start = *self.points.last().ok_or(ToolsAssetsPiecesError::Geometry)?;
        for step in 1..=ToolsAssetsPieces::CURVE_SEGMENTS {
            let t = f32::from(step) / f32::from(ToolsAssetsPieces::CURVE_SEGMENTS);
            let inverse = 1.0 - t;
            self.points.push(
                start * inverse.powi(3)
                    + controls[0] * (3.0 * inverse.powi(2) * t)
                    + controls[1] * (3.0 * inverse * t.powi(2))
                    + controls[2] * t.powi(3),
            );
        }
        Ok(())
    }

    fn quadratic(&mut self, control: Vec2, end: Vec2) -> Result<(), ToolsAssetsPiecesError> {
        let start = *self.points.last().ok_or(ToolsAssetsPiecesError::Geometry)?;
        for step in 1..=ToolsAssetsPieces::CURVE_SEGMENTS {
            let t = f32::from(step) / f32::from(ToolsAssetsPieces::CURVE_SEGMENTS);
            let inverse = 1.0 - t;
            self.points
                .push(start * inverse.powi(2) + control * (2.0 * inverse * t) + end * t.powi(2));
        }
        Ok(())
    }

    fn arc(&mut self, radius: f32, start: f32, end: f32) {
        let steps = ToolsAssetsPieces::CURVE_SEGMENTS * 2;
        for step in 1..=steps {
            let angle = start + (end - start) * f32::from(step) / f32::from(steps);
            self.points
                .push(Vec2::new(radius * angle.cos(), radius * angle.sin()));
        }
    }
}

#[derive(Clone, Copy)]
struct ToolsAssetsPiecesSculpture {
    kind: BoardEnumPieceKind,
}

impl ToolsAssetsPiecesSculpture {
    const QUEEN_BASE_RADIUS: f32 = 318.0 / 1_000.0;

    const REGISTRY: [Self; 6] = [
        Self {
            kind: BoardEnumPieceKind::Pawn,
        },
        Self {
            kind: BoardEnumPieceKind::Knight,
        },
        Self {
            kind: BoardEnumPieceKind::Bishop,
        },
        Self {
            kind: BoardEnumPieceKind::Rook,
        },
        Self {
            kind: BoardEnumPieceKind::Queen,
        },
        Self {
            kind: BoardEnumPieceKind::King,
        },
    ];

    fn name(self) -> &'static str {
        match self.kind {
            BoardEnumPieceKind::Pawn => "pawn",
            BoardEnumPieceKind::Knight => "knight",
            BoardEnumPieceKind::Bishop => "bishop",
            BoardEnumPieceKind::Rook => "rook",
            BoardEnumPieceKind::Queen => "queen",
            BoardEnumPieceKind::King => "king",
        }
    }

    fn silhouette(self) -> &'static str {
        match self.kind {
            BoardEnumPieceKind::Pawn => "Round finial and slim neck",
            BoardEnumPieceKind::Knight => "Swept horse profile, ears, muzzle and raised mane",
            BoardEnumPieceKind::Bishop => "Pointed mitre with a carved diagonal slit",
            BoardEnumPieceKind::Rook => "Broad tower with six open crenellations",
            BoardEnumPieceKind::Queen => "Eight rising crown points with pearl tips",
            BoardEnumPieceKind::King => "Tall stem and beveled cross finial",
        }
    }

    fn geometry(self) -> Result<ToolsAssetsPiecesGeometry, ToolsAssetsPiecesError> {
        match self.kind {
            BoardEnumPieceKind::Pawn => Self::pawn(),
            BoardEnumPieceKind::Knight => Self::knight(),
            BoardEnumPieceKind::Bishop => Self::bishop(),
            BoardEnumPieceKind::Rook => Self::rook(),
            BoardEnumPieceKind::Queen => Self::queen(),
            BoardEnumPieceKind::King => Self::king(),
        }
    }

    fn base(radius: f32) -> Result<ToolsAssetsPiecesGeometry, ToolsAssetsPiecesError> {
        ToolsAssetsPiecesGeometry::lathe(&[
            [0.0, 0.0],
            [radius * 0.83, 0.0],
            [radius * 0.94, 0.012],
            [radius, 0.034],
            [radius, 0.055],
            [radius * 0.98, 0.075],
            [radius * 0.91, 0.093],
            [radius * 0.84, 0.105],
            [radius * 0.83, 0.118],
            [radius * 0.87, 0.129],
            [radius * 0.87, 0.145],
            [radius * 0.81, 0.16],
            [radius * 0.66, 0.172],
            [0.0, 0.172],
        ])
    }

    fn pawn() -> Result<ToolsAssetsPiecesGeometry, ToolsAssetsPiecesError> {
        ToolsAssetsPiecesGeometry::combine(vec![
            Self::base(0.255)?,
            ToolsAssetsPiecesGeometry::lathe(&[
                [0.0, 0.153],
                [0.17, 0.153],
                [0.16, 0.19],
                [0.126, 0.235],
                [0.095, 0.3],
                [0.085, 0.365],
                [0.104, 0.394],
                [0.145, 0.406],
                [0.15, 0.425],
                [0.132, 0.445],
                [0.0, 0.452],
            ])?,
            ToolsAssetsPiecesGeometry::sphere(Vec3::splat(0.124), Vec3::new(0.0, 0.549, 0.0))?,
        ])
    }

    fn knight() -> Result<ToolsAssetsPiecesGeometry, ToolsAssetsPiecesError> {
        let mut horse = ToolsAssetsPiecesPath::new(-0.18, 0.25);
        horse.cubic([
            Vec2::new(-0.265, 0.48),
            Vec2::new(-0.21, 0.75),
            Vec2::new(-0.13, 0.855),
        ])?;
        horse.quadratic(Vec2::new(-0.035, 0.925), Vec2::new(0.065, 0.855))?;
        horse.cubic([
            Vec2::new(0.15, 0.82),
            Vec2::new(0.19, 0.74),
            Vec2::new(0.25, 0.705),
        ])?;
        horse.line(0.272, 0.625);
        horse.quadratic(Vec2::new(0.25, 0.582), Vec2::new(0.206, 0.577))?;
        horse.line(0.085, 0.615);
        horse.cubic([
            Vec2::new(0.025, 0.535),
            Vec2::new(-0.04, 0.41),
            Vec2::new(0.09, 0.26),
        ])?;
        let mut mane = ToolsAssetsPiecesPath::new(-0.18, 0.31);
        mane.cubic([
            Vec2::new(-0.32, 0.52),
            Vec2::new(-0.25, 0.755),
            Vec2::new(-0.145, 0.878),
        ])?;
        mane.line(-0.12, 0.81);
        mane.cubic([
            Vec2::new(-0.205, 0.66),
            Vec2::new(-0.2, 0.48),
            Vec2::new(-0.13, 0.35),
        ])?;
        let rotation = Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2);
        ToolsAssetsPiecesGeometry::combine(vec![
            Self::base(0.29)?,
            ToolsAssetsPiecesGeometry::lathe(&[
                [0.0, 0.15],
                [0.2, 0.15],
                [0.2, 0.19],
                [0.178, 0.218],
                [0.16, 0.24],
                [0.0, 0.24],
            ])?,
            ToolsAssetsPiecesGeometry::extrude(horse.points, 0.025, Vec2::new(0.03, 0.12))?
                .transform(rotation, Vec3::ZERO),
            ToolsAssetsPiecesGeometry::extrude(mane.points, 0.055, Vec2::splat(0.014))?
                .transform(rotation, Vec3::ZERO),
            ToolsAssetsPiecesGeometry::sphere(
                Vec3::new(0.137, 0.128, 0.136),
                Vec3::new(0.0, 0.777, -0.07),
            )?,
            ToolsAssetsPiecesGeometry::sphere(
                Vec3::new(0.129, 0.07, 0.114),
                Vec3::new(0.0, 0.645, -0.176),
            )?,
            Self::ear(-0.077)?,
            Self::ear(0.077)?,
            ToolsAssetsPiecesGeometry::sphere(
                Vec3::splat(0.017),
                Vec3::new(-0.137, 0.785, -0.109),
            )?,
            ToolsAssetsPiecesGeometry::sphere(Vec3::splat(0.017), Vec3::new(0.137, 0.785, -0.109))?,
        ])
    }

    fn ear(x: f32) -> Result<ToolsAssetsPiecesGeometry, ToolsAssetsPiecesError> {
        ToolsAssetsPiecesGeometry::extrude(
            vec![
                Vec2::new(-0.034, 0.0),
                Vec2::new(-0.014, 0.117),
                Vec2::new(0.03, 0.011),
            ],
            0.045,
            Vec2::splat(0.009),
        )
        .map(|geometry| geometry.transform(Mat3::IDENTITY, Vec3::new(x, 0.861, -0.023)))
    }

    fn bishop() -> Result<ToolsAssetsPiecesGeometry, ToolsAssetsPiecesError> {
        let head = ToolsAssetsPiecesGeometry::lathe(&[
            [0.0, 0.653],
            [0.065, 0.66],
            [0.116, 0.7],
            [0.147, 0.75],
            [0.15, 0.79],
            [0.132, 0.84],
            [0.094, 0.895],
            [0.045, 0.95],
            [0.0, 0.99],
        ])?;
        let cutter = ToolsAssetsPiecesGeometry::cuboid(Vec3::new(0.045, 0.34, 0.42))?.transform(
            Mat3::from_rotation_z(-std::f32::consts::PI / 5.0),
            Vec3::new(0.09, 0.91, 0.0),
        );
        ToolsAssetsPiecesGeometry::combine(vec![
            Self::base(0.285)?,
            ToolsAssetsPiecesGeometry::lathe(&[
                [0.0, 0.15],
                [0.195, 0.15],
                [0.175, 0.205],
                [0.135, 0.26],
                [0.101, 0.37],
                [0.085, 0.48],
                [0.097, 0.555],
                [0.145, 0.583],
                [0.155, 0.605],
                [0.145, 0.63],
                [0.095, 0.649],
                [0.0, 0.66],
            ])?,
            head.subtract(cutter)?,
        ])
    }

    fn rook() -> Result<ToolsAssetsPiecesGeometry, ToolsAssetsPiecesError> {
        const TEETH: u16 = 6;
        let mut parts = vec![
            Self::base(0.3)?,
            ToolsAssetsPiecesGeometry::lathe(&[
                [0.0, 0.15],
                [0.205, 0.15],
                [0.2, 0.205],
                [0.17, 0.24],
                [0.15, 0.34],
                [0.155, 0.52],
                [0.18, 0.59],
                [0.237, 0.615],
                [0.249, 0.638],
                [0.249, 0.67],
                [0.23, 0.69],
                [0.0, 0.69],
            ])?,
        ];
        for tooth in 0..TEETH {
            let start = f32::from(tooth) * std::f32::consts::TAU / f32::from(TEETH);
            let end = start + std::f32::consts::PI / f32::from(TEETH) * 0.98;
            let mut crown = ToolsAssetsPiecesPath::new(0.234 * start.cos(), 0.234 * start.sin());
            crown.arc(0.234, start, end);
            crown.line(end.cos() * 0.156, end.sin() * 0.156);
            crown.arc(0.156, end, start);
            parts.push(
                ToolsAssetsPiecesGeometry::extrude(crown.points, 0.119, Vec2::splat(0.009))?
                    .transform(
                        Mat3::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                        Vec3::new(0.0, 0.735, 0.0),
                    ),
            );
        }
        ToolsAssetsPiecesGeometry::combine(parts)
    }

    fn queen() -> Result<ToolsAssetsPiecesGeometry, ToolsAssetsPiecesError> {
        const POINTS: u16 = 8;
        let mut parts = vec![
            Self::base(Self::QUEEN_BASE_RADIUS)?,
            ToolsAssetsPiecesGeometry::lathe(&[
                [0.0, 0.15],
                [0.215, 0.15],
                [0.195, 0.21],
                [0.147, 0.28],
                [0.106, 0.4],
                [0.088, 0.56],
                [0.107, 0.67],
                [0.165, 0.71],
                [0.176, 0.734],
                [0.17, 0.76],
                [0.13, 0.776],
                [0.14, 0.82],
                [0.2, 0.897],
                [0.21, 0.92],
                [0.195, 0.94],
                [0.0, 0.94],
            ])?,
            ToolsAssetsPiecesGeometry::sphere(Vec3::splat(0.055), Vec3::new(0.0, 0.941, 0.0))?,
        ];
        for point in 0..POINTS {
            let angle = f32::from(point) * std::f32::consts::TAU / f32::from(POINTS);
            let x = angle.sin() * 0.185;
            let z = angle.cos() * 0.185;
            parts.push(
                ToolsAssetsPiecesGeometry::lathe(&[
                    [0.0, 0.883],
                    [0.044, 0.885],
                    [0.039, 0.935],
                    [0.025, 0.995],
                    [0.0, 1.027],
                ])?
                .transform(Mat3::IDENTITY, Vec3::new(x, 0.0, z)),
            );
            parts.push(ToolsAssetsPiecesGeometry::sphere(
                Vec3::splat(0.031),
                Vec3::new(x, 1.022, z),
            )?);
        }
        ToolsAssetsPiecesGeometry::combine(parts)
    }

    fn king() -> Result<ToolsAssetsPiecesGeometry, ToolsAssetsPiecesError> {
        let cross = vec![
            Vec2::new(-0.032, 0.944),
            Vec2::new(0.032, 0.944),
            Vec2::new(0.032, 1.056),
            Vec2::new(0.109, 1.056),
            Vec2::new(0.109, 1.116),
            Vec2::new(0.032, 1.116),
            Vec2::new(0.032, 1.19),
            Vec2::new(-0.032, 1.19),
            Vec2::new(-0.032, 1.116),
            Vec2::new(-0.109, 1.116),
            Vec2::new(-0.109, 1.056),
            Vec2::new(-0.032, 1.056),
        ];
        ToolsAssetsPiecesGeometry::combine(vec![
            Self::base(0.33)?,
            ToolsAssetsPiecesGeometry::lathe(&[
                [0.0, 0.15],
                [0.222, 0.15],
                [0.205, 0.21],
                [0.16, 0.28],
                [0.113, 0.43],
                [0.096, 0.61],
                [0.115, 0.724],
                [0.18, 0.755],
                [0.19, 0.778],
                [0.18, 0.802],
                [0.127, 0.82],
                [0.12, 0.863],
                [0.16, 0.897],
                [0.164, 0.919],
                [0.14, 0.943],
                [0.0, 0.955],
            ])?,
            ToolsAssetsPiecesGeometry::extrude(cross, 0.064, Vec2::splat(0.009))?,
        ])
    }
}
