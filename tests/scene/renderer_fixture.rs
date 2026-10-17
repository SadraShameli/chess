use bevy::asset::Assets;
use bevy::gltf::{GltfMaterial, GltfMesh, GltfNode, GltfPrimitive, gltf};
use bevy::platform::collections::HashMap;
use bevy::prelude::{Image, Mesh, StandardMaterial, Transform, World};
use bevy::shape::Cuboid;
use bevy_egui::egui;

use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::scene::assets::{SceneAssets, SceneAssetsPieceKey};
use crate::scene::camera::SceneCamera;
use crate::scene::picking::ScenePickingViewport;
use crate::scene::renderer::SceneRenderer;

pub(crate) struct SceneRendererFixture;

impl SceneRendererFixture {
    pub(crate) fn spawn(world: &mut World) -> Result<SceneRenderer, Box<dyn std::error::Error>> {
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StandardMaterial>>();
        let source = gltf::Gltf::from_slice(include_bytes!("../../resources/pieces/chess.glb"))?;
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
}
