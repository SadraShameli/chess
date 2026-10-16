use bevy::asset::{AssetId, Assets, Handle};
use bevy::gltf::{GltfMaterial, GltfMesh, GltfNode};
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::{Mesh, StandardMaterial, Transform};
use thiserror::Error;

use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::board::piece::BoardPiece;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneAssetsPieceKey {
    color: BoardEnumColor,
    kind: BoardEnumPieceKind,
}

impl SceneAssetsPieceKey {
    pub fn all() -> impl Iterator<Item = Self> {
        [BoardEnumColor::White, BoardEnumColor::Black]
            .into_iter()
            .flat_map(|color| {
                [
                    BoardEnumPieceKind::Pawn,
                    BoardEnumPieceKind::Knight,
                    BoardEnumPieceKind::Bishop,
                    BoardEnumPieceKind::Rook,
                    BoardEnumPieceKind::Queen,
                    BoardEnumPieceKind::King,
                ]
                .into_iter()
                .map(move |kind| Self { color, kind })
            })
    }

    pub fn name(self) -> String {
        let color = match self.color {
            BoardEnumColor::White => "white",
            BoardEnumColor::Black => "black",
        };
        let kind = match self.kind {
            BoardEnumPieceKind::Pawn => "pawn",
            BoardEnumPieceKind::Knight => "knight",
            BoardEnumPieceKind::Bishop => "bishop",
            BoardEnumPieceKind::Rook => "rook",
            BoardEnumPieceKind::Queen => "queen",
            BoardEnumPieceKind::King => "king",
        };
        format!("{color}_{kind}")
    }
}

impl From<BoardPiece> for SceneAssetsPieceKey {
    fn from(piece: BoardPiece) -> Self {
        Self {
            color: piece.color(),
            kind: piece.kind(),
        }
    }
}

#[derive(Debug, Error)]
pub enum SceneAssetsError {
    #[error("The piece library is missing named parent {0}")]
    MissingParent(String),
    #[error("The piece library has an unavailable node, mesh, or material")]
    MissingDependency,
    #[error("The piece library contains a cycle or exceeds the bounded node count")]
    Hierarchy,
    #[error("The piece library node {0} has an invalid transform or unsupported skin")]
    Node(String),
    #[error("The piece library parent {0} has no renderable descendants")]
    EmptyPiece(String),
}

pub struct SceneAssets {
    templates: Vec<SceneAssetsTemplate>,
}

impl SceneAssets {
    pub fn resolve(
        named_nodes: &HashMap<Box<str>, Handle<GltfNode>>,
        nodes: &Assets<GltfNode>,
        meshes: &Assets<GltfMesh>,
        gltf_materials: &Assets<GltfMaterial>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Result<Self, SceneAssetsError> {
        let mut resolver = SceneAssetsResolver {
            nodes,
            meshes,
            gltf_materials,
            materials,
            mapped_materials: HashMap::default(),
            ancestors: HashSet::default(),
            remaining_nodes: SceneAssetsResolver::MAX_NODES,
        };
        let mut templates = Vec::with_capacity(12);
        for key in SceneAssetsPieceKey::all() {
            let name = key.name();
            let handle = named_nodes
                .get(name.as_str())
                .ok_or_else(|| SceneAssetsError::MissingParent(name.clone()))?;
            let root = resolver.node(handle)?;
            if !root.has_primitives() {
                return Err(SceneAssetsError::EmptyPiece(name));
            }
            templates.push(SceneAssetsTemplate { key, root });
        }
        Ok(Self { templates })
    }

    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    pub(crate) fn template(&self, key: SceneAssetsPieceKey) -> Option<&SceneAssetsTemplate> {
        self.templates.iter().find(|template| template.key == key)
    }
}

pub(crate) struct SceneAssetsTemplate {
    pub(crate) key: SceneAssetsPieceKey,
    pub(crate) root: SceneAssetsNode,
}

pub(crate) struct SceneAssetsNode {
    pub(crate) name: String,
    pub(crate) transform: Transform,
    pub(crate) primitives: Vec<SceneAssetsPrimitive>,
    pub(crate) children: Vec<Self>,
}

impl SceneAssetsNode {
    fn has_primitives(&self) -> bool {
        !self.primitives.is_empty() || self.children.iter().any(Self::has_primitives)
    }
}

pub(crate) struct SceneAssetsPrimitive {
    pub(crate) mesh: Handle<Mesh>,
    pub(crate) material: Handle<StandardMaterial>,
}

struct SceneAssetsResolver<'a> {
    nodes: &'a Assets<GltfNode>,
    meshes: &'a Assets<GltfMesh>,
    gltf_materials: &'a Assets<GltfMaterial>,
    materials: &'a mut Assets<StandardMaterial>,
    mapped_materials: HashMap<AssetId<GltfMaterial>, Handle<StandardMaterial>>,
    ancestors: HashSet<AssetId<GltfNode>>,
    remaining_nodes: usize,
}

impl SceneAssetsResolver<'_> {
    const MAX_NODES: usize = 256;

    fn node(&mut self, handle: &Handle<GltfNode>) -> Result<SceneAssetsNode, SceneAssetsError> {
        if self.remaining_nodes == 0 || !self.ancestors.insert(handle.id()) {
            return Err(SceneAssetsError::Hierarchy);
        }
        self.remaining_nodes -= 1;
        let nodes = self.nodes;
        let node = nodes
            .get(handle)
            .ok_or(SceneAssetsError::MissingDependency)?;
        if node.skin.is_some()
            || !node.transform.translation.is_finite()
            || !node.transform.rotation.is_finite()
            || !node.transform.rotation.is_normalized()
            || !node.transform.scale.is_finite()
            || node.transform.scale.abs().min_element() <= f32::EPSILON
        {
            return Err(SceneAssetsError::Node(node.name.clone()));
        }
        let mut primitives = Vec::new();
        if let Some(handle) = &node.mesh {
            let mesh = self
                .meshes
                .get(handle)
                .ok_or(SceneAssetsError::MissingDependency)?;
            for primitive in &mesh.primitives {
                let material = primitive
                    .material
                    .as_ref()
                    .ok_or(SceneAssetsError::MissingDependency)?;
                let material = if let Some(mapped) = self.mapped_materials.get(&material.id()) {
                    mapped.clone()
                } else {
                    let source = self
                        .gltf_materials
                        .get(material)
                        .ok_or(SceneAssetsError::MissingDependency)?;
                    let mapped = self
                        .materials
                        .add(standard_material_from_gltf_material(source));
                    self.mapped_materials.insert(material.id(), mapped.clone());
                    mapped
                };
                primitives.push(SceneAssetsPrimitive {
                    mesh: primitive.mesh.clone(),
                    material,
                });
            }
        }
        let children = node
            .children
            .iter()
            .map(|child| self.node(child))
            .collect::<Result<Vec<_>, _>>()?;
        self.ancestors.remove(&handle.id());
        Ok(SceneAssetsNode {
            name: node.name.clone(),
            transform: node.transform,
            primitives,
            children,
        })
    }
}
