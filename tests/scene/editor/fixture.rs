use bevy::asset::{Assets, Handle};
use bevy::gltf::{GltfMaterial, GltfMesh, GltfNode, GltfPrimitive, gltf};
use bevy::math::{Quat, Vec3};
use bevy::platform::collections::HashMap;
use bevy::prelude::{Image, Mesh, StandardMaterial, Transform, World};
use bevy::shape::Cuboid;
use bevy_egui::egui::{Pos2, Rect};
use chess::board::enums::BoardEnumColor;
use chess::scene::assets::SceneAssets;
use chess::scene::camera::SceneCamera;
use chess::scene::picking::ScenePickingViewport;
use chess::scene::renderer::SceneRenderer;

pub(super) struct SceneEditorFixture {
    names: HashMap<Box<str>, Handle<GltfNode>>,
    nodes: Assets<GltfNode>,
    meshes: Assets<GltfMesh>,
    materials: Assets<GltfMaterial>,
    pub(super) world: World,
}

impl SceneEditorFixture {
    pub(super) fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let source = gltf::Gltf::from_slice(include_bytes!("../../../resources/pieces/chess.glb"))?;
        let mut world = World::new();
        let mut render_meshes = Assets::<Mesh>::default();
        let geometry = render_meshes.add(Cuboid::new(0.5, 0.8, 0.5));
        world.insert_resource(render_meshes);
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(Assets::<StandardMaterial>::default());
        let mut fixture = Self {
            names: HashMap::default(),
            nodes: Assets::default(),
            meshes: Assets::default(),
            materials: Assets::default(),
            world,
        };
        let material = fixture.materials.add(GltfMaterial::default());
        for parent in source
            .document
            .nodes()
            .filter(|node| node.children().next().is_some())
        {
            let child = parent.children().next().ok_or("missing mesh child")?;
            let mesh = fixture.meshes.add(GltfMesh {
                index: child.index(),
                name: "fixture_mesh".to_owned(),
                primitives: vec![GltfPrimitive {
                    index: 0,
                    parent_mesh_index: child.index(),
                    name: "fixture_primitive".to_owned(),
                    mesh: geometry.clone(),
                    material: Some(material.clone()),
                    extras: None,
                    material_extras: None,
                }],
                extras: None,
            });
            let child_handle = fixture.nodes.add(GltfNode::new(
                &child,
                Vec::new(),
                Some(mesh),
                Self::child_transform(),
                None,
                None,
            ));
            let parent_handle = fixture.nodes.add(GltfNode::new(
                &parent,
                vec![child_handle],
                None,
                Self::parent_transform(),
                None,
                None,
            ));
            fixture.names.insert(
                parent.name().ok_or("unnamed piece parent")?.into(),
                parent_handle,
            );
        }
        Ok(fixture)
    }

    fn parent_transform() -> Transform {
        Transform::from_xyz(0.01, 0.02, 0.03).with_rotation(Quat::from_rotation_y(0.1))
    }

    fn child_transform() -> Transform {
        Transform::from_xyz(0.0, 0.4, 0.0).with_scale(Vec3::splat(0.9))
    }

    fn registry(&mut self) -> Result<SceneAssets, Box<dyn std::error::Error>> {
        Ok(SceneAssets::resolve(
            &self.names,
            &self.nodes,
            &self.meshes,
            &self.materials,
            &mut self.world.resource_mut::<Assets<StandardMaterial>>(),
        )?)
    }

    fn viewport() -> Result<ScenePickingViewport, Box<dyn std::error::Error>> {
        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0));
        Ok(ScenePickingViewport::new(rect, rect, 1.5, 1.0)?)
    }

    pub(super) fn spawn(&mut self) -> Result<SceneRenderer, Box<dyn std::error::Error>> {
        let registry = self.registry()?;
        Ok(SceneRenderer::spawn(
            &mut self.world,
            registry,
            Self::viewport()?,
            SceneCamera::initial(BoardEnumColor::White),
        )?)
    }
}
