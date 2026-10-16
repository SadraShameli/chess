use bevy::app::App;
use bevy::asset::{Assets, Handle};
use bevy::camera::{Camera, RenderTarget, primitives::MeshAabb};
use bevy::ecs::system::SystemState;
use bevy::gltf::{GltfMaterial, GltfMesh, GltfNode, GltfPrimitive, gltf};
use bevy::math::{Quat, Vec3};
use bevy::picking::mesh_picking::ray_cast::MeshRayCast;
use bevy::platform::collections::HashMap;
use bevy::prelude::{
    ChildOf, Children, Entity, GlobalTransform, Image, InheritedVisibility, Mesh, Mesh3d, Name,
    StandardMaterial, Transform, ViewVisibility, With, World,
};
use bevy::shape::Cuboid;
use bevy_egui::egui::{Pos2, Rect};
use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::scene::assets::{SceneAssets, SceneAssetsPieceKey};
use chess::scene::camera::SceneCamera;
use chess::scene::enums::SceneEnumLifecycle;
use chess::scene::picking::{ScenePicking, ScenePickingViewport};
use chess::scene::renderer::SceneRenderer;

use crate::scene_task_pools::SceneTaskPoolFixture;

struct SceneRendererFixture {
    names: HashMap<Box<str>, Handle<GltfNode>>,
    nodes: Assets<GltfNode>,
    meshes: Assets<GltfMesh>,
    materials: Assets<GltfMaterial>,
    world: World,
}

impl SceneRendererFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let source = gltf::Gltf::from_slice(include_bytes!("../../resources/pieces/chess.glb"))?;
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

    fn spawn(&mut self) -> Result<SceneRenderer, Box<dyn std::error::Error>> {
        let registry = self.registry()?;
        Ok(SceneRenderer::spawn(
            &mut self.world,
            registry,
            Self::viewport()?,
            SceneCamera::initial(BoardEnumColor::White),
        )?)
    }

    fn global_transform(
        world: &World,
        entity: Entity,
    ) -> Result<GlobalTransform, Box<dyn std::error::Error>> {
        let local = *world
            .get::<Transform>(entity)
            .ok_or("missing local transform")?;
        match world.get::<ChildOf>(entity) {
            Some(parent) => {
                Ok(Self::global_transform(world, parent.parent())?.mul_transform(local))
            }
            None => Ok(GlobalTransform::from(local)),
        }
    }

    fn prepare_picking(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut app = App::new();
        app.add_plugins(SceneTaskPoolFixture::plugin());
        let entities = self
            .world
            .query_filtered::<Entity, With<Mesh3d>>()
            .iter(&self.world)
            .collect::<Vec<_>>();
        for entity in entities {
            let global = Self::global_transform(&self.world, entity)?;
            let mesh = self
                .world
                .get::<Mesh3d>(entity)
                .ok_or("missing mesh component")?;
            let aabb = self
                .world
                .resource::<Assets<Mesh>>()
                .get(&mesh.0)
                .and_then(MeshAabb::get_aabb)
                .ok_or("missing mesh bounds")?;
            self.world.entity_mut(entity).insert((
                global,
                aabb,
                InheritedVisibility::VISIBLE,
                ViewVisibility::default(),
            ));
        }
        Ok(())
    }

    fn scene_entity_count(&mut self) -> usize {
        self.world
            .query_filtered::<Entity, With<Transform>>()
            .iter(&self.world)
            .count()
    }
}

#[test]
fn named_registry_requires_all_twelve_templates_and_preserves_authored_transforms()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = SceneRendererFixture::new()?;
    let registry = fixture.registry()?;
    assert_eq!(registry.len(), 12);
    assert_eq!(SceneAssetsPieceKey::all().count(), registry.len());
    assert_eq!(
        fixture.world.resource::<Assets<StandardMaterial>>().len(),
        1
    );
    let mut renderer = SceneRenderer::spawn(
        &mut fixture.world,
        registry,
        SceneRendererFixture::viewport()?,
        SceneCamera::initial(BoardEnumColor::White),
    )?;
    renderer.apply_position(&mut fixture.world, &BoardPosition::standard())?;
    for (square, piece) in BoardPosition::standard().pieces() {
        let root = renderer.piece_root(square).ok_or("missing piece root")?;
        let parent = fixture
            .world
            .get::<Children>(root)
            .and_then(|children| children.first())
            .copied()
            .ok_or("missing authored parent")?;
        assert_eq!(
            fixture.world.get::<Name>(parent).map(Name::as_str),
            Some(SceneAssetsPieceKey::from(piece).name().as_str())
        );
        assert_eq!(
            fixture.world.get::<Transform>(parent),
            Some(&SceneRendererFixture::parent_transform())
        );
        let child = fixture
            .world
            .get::<Children>(parent)
            .and_then(|children| children.first())
            .copied()
            .ok_or("missing authored child")?;
        assert_eq!(
            fixture.world.get::<Transform>(child),
            Some(&SceneRendererFixture::child_transform())
        );
        for entity in renderer
            .piece_entities(square)
            .ok_or("missing descendants")?
        {
            assert_eq!(renderer.square_for_entity(*entity), Some(square));
        }
    }
    fixture.names.remove("white_king");
    assert!(fixture.registry().is_err());
    Ok(())
}

#[test]
fn immutable_snapshots_reuse_unchanged_entities_and_offscreen_images()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = SceneRendererFixture::new()?;
    let mut renderer = fixture.spawn()?;
    let initial = BoardPosition::standard();
    assert!(renderer.apply_position(&mut fixture.world, &initial)?);
    let e2 = "e2".parse()?;
    let e4 = "e4".parse()?;
    let a8 = "a8".parse()?;
    let stable = renderer.piece_root(a8).ok_or("missing a8 rook")?;
    let image = renderer.image().id();
    let generation = renderer.generation();
    assert!(!renderer.apply_position(&mut fixture.world, &initial)?);
    assert_eq!(renderer.generation(), generation);
    assert_eq!(renderer.image().id(), image);
    let position = BoardPosition::from_fen(
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
        BoardEnumVariant::Standard,
    )?;
    assert!(renderer.apply_position(&mut fixture.world, &position)?);
    assert!(renderer.piece_root(e2).is_none());
    assert!(renderer.piece_root(e4).is_some());
    assert_eq!(renderer.piece_root(a8), Some(stable));
    assert_eq!(renderer.image().id(), image);
    assert_eq!(initial.fen(), BoardPosition::standard().fen());
    renderer.despawn(&mut fixture.world)?;
    assert_eq!(fixture.world.resource::<Assets<Image>>().len(), 0);
    assert_eq!(fixture.scene_entity_count(), 0);
    Ok(())
}

#[test]
fn viewport_dpi_clip_and_camera_changes_invalidate_geometry_and_recreate_sized_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = SceneRendererFixture::new()?;
    let mut renderer = fixture.spawn()?;
    let camera = SceneCamera::initial(BoardEnumColor::White);
    let initial_image = renderer.image().id();
    let initial_generation = renderer.generation();
    assert!(!renderer.update_view(
        &mut fixture.world,
        SceneRendererFixture::viewport()?,
        camera
    )?);
    assert_eq!(renderer.image().id(), initial_image);
    let rect = Rect::from_min_max(Pos2::new(30.0, 40.0), Pos2::new(830.0, 640.0));
    let clip = Rect::from_min_max(Pos2::new(60.0, 40.0), Pos2::new(830.0, 600.0));
    let viewport = ScenePickingViewport::new(rect, clip, 2.0, 1.0)?;
    assert!(renderer.update_view(&mut fixture.world, viewport, camera)?);
    assert_ne!(renderer.image().id(), initial_image);
    assert_ne!(renderer.generation(), initial_generation);
    let image = fixture
        .world
        .resource::<Assets<Image>>()
        .get(renderer.image())
        .ok_or("missing image")?;
    assert_eq!(image.width(), 1600);
    assert_eq!(image.height(), 1200);
    assert_eq!(fixture.world.resource::<Assets<Image>>().len(), 1);
    assert!(matches!(
        fixture.world.get::<RenderTarget>(renderer.camera_entity()),
        Some(RenderTarget::Image(target)) if target.handle.id() == renderer.image().id()
    ));
    let image = renderer.image().id();
    assert!(renderer.update_view(&mut fixture.world, viewport, camera.rotate_right())?);
    assert_eq!(renderer.image().id(), image);
    Ok(())
}

#[test]
fn lifecycle_failure_invalidates_old_generation_and_rendering_requires_explicit_readiness()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = SceneRendererFixture::new()?;
    let mut renderer = fixture.spawn()?;
    assert_eq!(renderer.lifecycle(), SceneEnumLifecycle::Initializing);
    let generation = renderer.generation();
    renderer.mark_ready(generation)?;
    renderer.finish_frame(&mut fixture.world)?;
    assert!(
        !fixture
            .world
            .get::<Camera>(renderer.camera_entity())
            .ok_or("missing camera")?
            .is_active
    );
    renderer.request_frame(&mut fixture.world)?;
    assert!(
        fixture
            .world
            .get::<Camera>(renderer.camera_entity())
            .ok_or("missing camera")?
            .is_active
    );
    renderer.fail(&mut fixture.world)?;
    assert_eq!(renderer.lifecycle(), SceneEnumLifecycle::Failed);
    assert_ne!(renderer.generation(), generation);
    assert!(renderer.mark_ready(generation).is_err());
    assert!(renderer.request_frame(&mut fixture.world).is_err());
    let failed_generation = renderer.generation();
    renderer.fail(&mut fixture.world)?;
    assert_eq!(renderer.generation(), failed_generation);
    assert!(
        renderer
            .apply_position(&mut fixture.world, &BoardPosition::standard())
            .is_err()
    );
    assert!(
        renderer
            .update_view(
                &mut fixture.world,
                SceneRendererFixture::viewport()?,
                SceneCamera::initial(BoardEnumColor::Black),
            )
            .is_err()
    );
    renderer.begin_retry()?;
    assert_eq!(renderer.lifecycle(), SceneEnumLifecycle::Retrying);
    renderer.mark_ready(renderer.generation())?;
    assert_eq!(renderer.lifecycle(), SceneEnumLifecycle::Ready);
    Ok(())
}

#[test]
fn malformed_asset_hierarchies_fail_before_a_scene_is_spawned()
-> Result<(), Box<dyn std::error::Error>> {
    let mut cyclic = SceneRendererFixture::new()?;
    let parent = cyclic
        .names
        .get("white_king")
        .ok_or("missing parent")?
        .clone();
    cyclic
        .nodes
        .get_mut(&parent)
        .ok_or("missing parent asset")?
        .children
        .push(parent.clone());
    assert!(cyclic.registry().is_err());
    let mut invalid = SceneRendererFixture::new()?;
    let parent = invalid
        .names
        .get("black_queen")
        .ok_or("missing parent")?
        .clone();
    invalid
        .nodes
        .get_mut(&parent)
        .ok_or("missing parent asset")?
        .transform
        .scale = Vec3::ZERO;
    assert!(invalid.registry().is_err());
    assert_eq!(cyclic.scene_entity_count(), 0);
    assert_eq!(invalid.scene_entity_count(), 0);
    Ok(())
}

#[test]
fn cached_scene_uses_nearest_mesh_hit_instead_of_the_square_beneath_an_occluding_piece()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = SceneRendererFixture::new()?;
    let mut renderer = fixture.spawn()?;
    let camera = SceneCamera::new(0.0, 45.0, 1.0, BoardEnumColor::White)?;
    let viewport = SceneRendererFixture::viewport()?;
    renderer.update_view(&mut fixture.world, viewport, camera)?;
    renderer.apply_position(&mut fixture.world, &BoardPosition::standard())?;
    renderer.mark_ready(renderer.generation())?;
    renderer.finish_frame(&mut fixture.world)?;
    fixture.prepare_picking()?;
    let frame = camera.fit(viewport.physical_size())?;
    let square = "e2".parse()?;
    let center = ScenePicking::square_center(square) + Vec3::new(0.0, 0.7, 0.03);
    let pixel = frame.project(center).ok_or("piece outside camera")?;
    assert_ne!(ScenePicking::board_square(frame.ray(pixel)?), Some(square));
    let logical = viewport.to_logical(pixel).ok_or("piece clipped out")?;
    let mut system = SystemState::<MeshRayCast<'_, '_>>::new(&mut fixture.world);
    let mut ray_cast = system.get_mut(&mut fixture.world)?;
    assert_eq!(
        renderer.pick(logical, renderer.generation(), &mut ray_cast)?,
        Some(square)
    );
    assert!(
        renderer
            .pick(Pos2::new(-1.0, -1.0), renderer.generation(), &mut ray_cast)?
            .is_none()
    );
    assert!(
        renderer
            .pick(logical, renderer.generation().next()?, &mut ray_cast)
            .is_err()
    );
    Ok(())
}
