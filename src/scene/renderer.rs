#[cfg(test)]
#[path = "../../tests/scene/renderer_fixture.rs"]
pub(crate) mod fixture;

use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::camera::{RenderTarget, Viewport, visibility::RenderLayers};
use bevy::math::{UVec2, Vec3};
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::platform::collections::HashMap;
use bevy::prelude::{
    Camera, Camera3d, ChildOf, ClearColorConfig, Color, DirectionalLight, Entity, Image, Mesh,
    Mesh3d, MeshMaterial3d, Name, Projection, Quat, Resource, StandardMaterial, Transform,
    Visibility, World,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::shape::Cuboid;
use bevy_egui::{EguiTextureHandle, EguiUserTextures, egui};
use thiserror::Error;
use uuid::Uuid;

use crate::board::enums::BoardEnumColor;
use crate::board::piece::BoardPiece;
use crate::board::position::BoardPosition;
use crate::board::square::{BoardSquare, BoardSquareError};
use crate::scene::assets::{SceneAssets, SceneAssetsNode};
use crate::scene::camera::{SceneCamera, SceneCameraError, SceneCameraFrame};
use crate::scene::enums::SceneEnumLifecycle;
use crate::scene::picking::{ScenePicking, ScenePickingViewport};

#[derive(Debug, Error)]
pub enum SceneRendererError {
    #[error("The scene generation exhausted its integer range")]
    GenerationOverflow,
    #[error("The scene requires initialized Bevy {0} assets")]
    MissingResource(&'static str),
    #[error("The scene entity {0:?} is unavailable")]
    MissingEntity(Entity),
    #[error("The piece library does not contain a required template")]
    MissingTemplate,
    #[error("The renderer cannot perform this operation while {0:?}")]
    Lifecycle(SceneEnumLifecycle),
    #[error("The completion belongs to an obsolete scene generation")]
    StaleGeneration,
    #[error(transparent)]
    Camera(#[from] SceneCameraError),
    #[error(transparent)]
    Square(#[from] BoardSquareError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneRendererGeneration {
    instance: Uuid,
    revision: u64,
}

impl SceneRendererGeneration {
    pub fn initial() -> Self {
        Self {
            instance: Uuid::new_v4(),
            revision: 1,
        }
    }

    pub fn next(self) -> Result<Self, SceneRendererError> {
        self.revision
            .checked_add(1)
            .map(|revision| Self { revision, ..self })
            .ok_or(SceneRendererError::GenerationOverflow)
    }
}

struct SceneRendererPiece {
    piece: BoardPiece,
    root: Entity,
    entities: Vec<Entity>,
}

#[derive(Resource)]
pub struct SceneRenderer {
    root: Entity,
    camera_entity: Entity,
    image: Handle<Image>,
    assets: SceneAssets,
    camera: SceneCamera,
    frame: SceneCameraFrame,
    viewport: ScenePickingViewport,
    generation: SceneRendererGeneration,
    lifecycle: SceneEnumLifecycle,
    snapshot: Option<BoardPosition>,
    pieces: HashMap<BoardSquare, SceneRendererPiece>,
    entity_squares: HashMap<Entity, BoardSquare>,
}

impl SceneRenderer {
    pub const RENDER_LAYER: usize = 1;
    const SQUARE_THICKNESS: f32 = 0.05;
    const LIGHT_SQUARE: Color = Color::srgb_u8(238, 229, 207);
    const DARK_SQUARE: Color = Color::srgb_u8(75, 112, 87);
    const BACKGROUND: Color = Color::srgb_u8(17, 21, 26);

    pub fn spawn(
        world: &mut World,
        assets: SceneAssets,
        viewport: ScenePickingViewport,
        camera: SceneCamera,
    ) -> Result<Self, SceneRendererError> {
        for (available, name) in [
            (world.contains_resource::<Assets<Mesh>>(), "mesh"),
            (
                world.contains_resource::<Assets<StandardMaterial>>(),
                "material",
            ),
            (world.contains_resource::<Assets<Image>>(), "image"),
        ] {
            if !available {
                return Err(SceneRendererError::MissingResource(name));
            }
        }
        let frame = camera.fit(viewport.physical_size())?;
        let image = Self::create_image(world, viewport.physical_size())?;
        let root = world
            .spawn((
                Name::new("chess_scene"),
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        let camera_entity = world
            .spawn((
                Camera3d::default(),
                Camera {
                    order: -1,
                    viewport: Some(Self::camera_viewport(viewport.physical_size())),
                    clear_color: ClearColorConfig::Custom(Self::BACKGROUND),
                    ..Camera::default()
                },
                RenderTarget::Image(image.clone().into()),
                Projection::Perspective(frame.projection()),
                frame.transform(),
                ChildOf(root),
                RenderLayers::layer(Self::RENDER_LAYER),
            ))
            .id();
        let mut renderer = Self {
            root,
            camera_entity,
            image,
            assets,
            camera,
            frame,
            viewport,
            generation: SceneRendererGeneration::initial(),
            lifecycle: SceneEnumLifecycle::Initializing,
            snapshot: None,
            pieces: HashMap::default(),
            entity_squares: HashMap::default(),
        };
        renderer.spawn_board(world)?;
        renderer.spawn_lights(world);
        Ok(renderer)
    }

    pub const fn generation(&self) -> SceneRendererGeneration {
        self.generation
    }

    pub const fn lifecycle(&self) -> SceneEnumLifecycle {
        self.lifecycle
    }

    pub fn image(&self) -> &Handle<Image> {
        &self.image
    }

    pub const fn camera_entity(&self) -> Entity {
        self.camera_entity
    }

    pub const fn viewport(&self) -> ScenePickingViewport {
        self.viewport
    }

    pub fn register_image(&self, textures: &mut EguiUserTextures) -> egui::TextureId {
        textures.add_image(EguiTextureHandle::Strong(self.image.clone()))
    }

    pub fn despawn(self, world: &mut World) -> Result<(), SceneRendererError> {
        Self::remove_image(world, &self.image)?;
        if !world.despawn(self.root) {
            return Err(SceneRendererError::MissingEntity(self.root));
        }
        Ok(())
    }

    pub fn piece_root(&self, square: BoardSquare) -> Option<Entity> {
        self.pieces.get(&square).map(|piece| piece.root)
    }

    pub fn piece_entities(&self, square: BoardSquare) -> Option<&[Entity]> {
        self.pieces
            .get(&square)
            .map(|piece| piece.entities.as_slice())
    }

    pub fn square_for_entity(&self, entity: Entity) -> Option<BoardSquare> {
        self.entity_squares.get(&entity).copied()
    }

    pub fn mark_ready(
        &mut self,
        generation: SceneRendererGeneration,
    ) -> Result<(), SceneRendererError> {
        if generation != self.generation {
            return Err(SceneRendererError::StaleGeneration);
        }
        match self.lifecycle {
            SceneEnumLifecycle::Initializing | SceneEnumLifecycle::Retrying => {
                self.lifecycle = SceneEnumLifecycle::Ready;
                Ok(())
            }
            SceneEnumLifecycle::Ready => Ok(()),
            SceneEnumLifecycle::Failed => Err(SceneRendererError::Lifecycle(self.lifecycle)),
        }
    }

    pub fn begin_retry(&mut self) -> Result<(), SceneRendererError> {
        if self.lifecycle != SceneEnumLifecycle::Failed {
            return Err(SceneRendererError::Lifecycle(self.lifecycle));
        }
        self.generation = self.generation.next()?;
        self.lifecycle = SceneEnumLifecycle::Retrying;
        Ok(())
    }

    pub fn fail(&mut self, world: &mut World) -> Result<(), SceneRendererError> {
        if self.lifecycle != SceneEnumLifecycle::Failed {
            self.generation = self.generation.next()?;
            self.lifecycle = SceneEnumLifecycle::Failed;
        }
        self.finish_frame(world)
    }

    pub fn request_frame(&self, world: &mut World) -> Result<(), SceneRendererError> {
        self.require_renderable()?;
        self.set_camera_active(world, true)
    }

    pub fn finish_frame(&self, world: &mut World) -> Result<(), SceneRendererError> {
        self.set_camera_active(world, false)
    }

    pub fn update_view(
        &mut self,
        world: &mut World,
        viewport: ScenePickingViewport,
        camera: SceneCamera,
    ) -> Result<bool, SceneRendererError> {
        self.require_renderable()?;
        if viewport == self.viewport && camera == self.camera {
            return Ok(false);
        }
        let frame = camera.fit(viewport.physical_size())?;
        let generation = self.generation.next()?;
        if viewport != self.viewport {
            let image = Self::create_image(world, viewport.physical_size())?;
            Self::remove_image(world, &self.image)?;
            self.image = image;
        }
        let mut entity = world
            .get_entity_mut(self.camera_entity)
            .map_err(|_error| SceneRendererError::MissingEntity(self.camera_entity))?;
        entity.insert((
            frame.transform(),
            Projection::Perspective(frame.projection()),
            RenderTarget::Image(self.image.clone().into()),
        ));
        let mut component = entity
            .get_mut::<Camera>()
            .ok_or(SceneRendererError::MissingEntity(self.camera_entity))?;
        component.viewport = Some(Self::camera_viewport(viewport.physical_size()));
        component.is_active = true;
        self.frame = frame;
        self.viewport = viewport;
        self.camera = camera;
        self.generation = generation;
        Ok(true)
    }

    pub fn apply_position(
        &mut self,
        world: &mut World,
        position: &BoardPosition,
    ) -> Result<bool, SceneRendererError> {
        self.require_renderable()?;
        if self
            .snapshot
            .as_ref()
            .is_some_and(|previous| previous.pieces().eq(position.pieces()))
        {
            return Ok(false);
        }
        let generation = self.generation.next()?;
        let removed = self
            .pieces
            .iter()
            .filter_map(|(square, current)| {
                (!position
                    .pieces()
                    .any(|entry| entry == (*square, current.piece)))
                .then_some(*square)
            })
            .collect::<Vec<_>>();
        for square in removed {
            self.remove_piece(world, square)?;
        }
        for (square, piece) in position.pieces() {
            if self.pieces.contains_key(&square) {
                continue;
            }
            let template = self
                .assets
                .template(piece.into())
                .ok_or(SceneRendererError::MissingTemplate)?;
            let rotation = match piece.color() {
                BoardEnumColor::White => Quat::IDENTITY,
                BoardEnumColor::Black => Quat::from_rotation_y(std::f32::consts::PI),
            };
            let root = world
                .spawn((
                    Transform::from_translation(ScenePicking::square_center(square))
                        .with_rotation(rotation),
                    Visibility::default(),
                    ChildOf(self.root),
                ))
                .id();
            let mut entities = vec![root];
            Self::spawn_node(world, &template.root, root, &mut entities);
            for entity in &entities {
                self.entity_squares.insert(*entity, square);
            }
            self.pieces.insert(
                square,
                SceneRendererPiece {
                    piece,
                    root,
                    entities,
                },
            );
        }
        self.snapshot = Some(position.clone());
        self.generation = generation;
        self.request_frame(world)?;
        Ok(true)
    }

    pub fn clear_position(&mut self, world: &mut World) -> Result<bool, SceneRendererError> {
        self.require_renderable()?;
        if self.snapshot.is_none() && self.pieces.is_empty() {
            return Ok(false);
        }
        let generation = self.generation.next()?;
        let removed = self.pieces.keys().copied().collect::<Vec<_>>();
        for square in removed {
            self.remove_piece(world, square)?;
        }
        self.snapshot = None;
        self.generation = generation;
        self.request_frame(world)?;
        Ok(true)
    }

    fn remove_piece(
        &mut self,
        world: &mut World,
        square: BoardSquare,
    ) -> Result<(), SceneRendererError> {
        if let Some(piece) = self.pieces.remove(&square) {
            for entity in piece.entities {
                self.entity_squares.remove(&entity);
            }
            if !world.despawn(piece.root) {
                return Err(SceneRendererError::MissingEntity(piece.root));
            }
        }
        Ok(())
    }

    pub fn pick(
        &self,
        point: egui::Pos2,
        generation: SceneRendererGeneration,
        ray_cast: &mut MeshRayCast<'_, '_>,
    ) -> Result<Option<BoardSquare>, SceneRendererError> {
        if generation != self.generation {
            return Err(SceneRendererError::StaleGeneration);
        }
        if self.lifecycle != SceneEnumLifecycle::Ready {
            return Err(SceneRendererError::Lifecycle(self.lifecycle));
        }
        let Some(pixel) = self.viewport.to_physical(point) else {
            return Ok(None);
        };
        let ray = self.frame.ray(pixel)?;
        let filter = |entity| self.entity_squares.contains_key(&entity);
        let settings = MeshRayCastSettings::default()
            .with_filter(&filter)
            .with_visibility(RayCastVisibility::Visible);
        Ok(ray_cast
            .cast_ray(ray, &settings)
            .first()
            .and_then(|(entity, _hit)| self.square_for_entity(*entity)))
    }

    fn create_image(world: &mut World, size: UVec2) -> Result<Handle<Image>, SceneRendererError> {
        let mut image = Image::new_uninit(
            Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Bgra8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
        Ok(world
            .get_resource_mut::<Assets<Image>>()
            .ok_or(SceneRendererError::MissingResource("image"))?
            .add(image))
    }

    fn camera_viewport(size: UVec2) -> Viewport {
        Viewport {
            physical_position: UVec2::ZERO,
            physical_size: size,
            ..Viewport::default()
        }
    }

    fn remove_image(world: &mut World, image: &Handle<Image>) -> Result<(), SceneRendererError> {
        if let Some(mut textures) = world.get_resource_mut::<EguiUserTextures>() {
            textures.remove_image(image.id());
        }
        world
            .get_resource_mut::<Assets<Image>>()
            .ok_or(SceneRendererError::MissingResource("image"))?
            .remove(image.id());
        Ok(())
    }

    fn set_camera_active(&self, world: &mut World, active: bool) -> Result<(), SceneRendererError> {
        let mut camera = world
            .get_mut::<Camera>(self.camera_entity)
            .ok_or(SceneRendererError::MissingEntity(self.camera_entity))?;
        camera.is_active = active;
        Ok(())
    }

    fn require_renderable(&self) -> Result<(), SceneRendererError> {
        if self.lifecycle == SceneEnumLifecycle::Failed {
            Err(SceneRendererError::Lifecycle(self.lifecycle))
        } else {
            Ok(())
        }
    }

    fn spawn_board(&mut self, world: &mut World) -> Result<(), SceneRendererError> {
        let mesh = world
            .get_resource_mut::<Assets<Mesh>>()
            .ok_or(SceneRendererError::MissingResource("mesh"))?
            .add(Cuboid::new(1.0, Self::SQUARE_THICKNESS, 1.0));
        let materials = {
            let mut materials = world
                .get_resource_mut::<Assets<StandardMaterial>>()
                .ok_or(SceneRendererError::MissingResource("material"))?;
            [Self::DARK_SQUARE, Self::LIGHT_SQUARE].map(|color| {
                materials.add(StandardMaterial {
                    base_color: color,
                    perceptual_roughness: 0.72,
                    ..StandardMaterial::default()
                })
            })
        };
        for index in 0..ScenePicking::SQUARES {
            let square = BoardSquare::try_from(index)?;
            let parity = (index / ScenePicking::FILES + index % ScenePicking::FILES) % 2;
            let mut center = ScenePicking::square_center(square);
            center.y = -Self::SQUARE_THICKNESS / 2.0;
            let entity = world
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(materials[usize::from(parity)].clone()),
                    Transform::from_translation(center),
                    ChildOf(self.root),
                    RenderLayers::layer(Self::RENDER_LAYER),
                ))
                .id();
            self.entity_squares.insert(entity, square);
        }
        Ok(())
    }

    fn spawn_lights(&self, world: &mut World) {
        for (illuminance, shadow_maps_enabled, position) in [
            (10_000.0, true, Vec3::new(-3.0, 8.0, 4.0)),
            (2_500.0, false, Vec3::new(4.0, 5.0, -3.0)),
        ] {
            world.spawn((
                DirectionalLight {
                    illuminance,
                    shadow_maps_enabled,
                    ..DirectionalLight::default()
                },
                Transform::from_translation(position).looking_at(Vec3::ZERO, Vec3::Y),
                ChildOf(self.root),
                RenderLayers::layer(Self::RENDER_LAYER),
            ));
        }
    }

    fn spawn_node(
        world: &mut World,
        node: &SceneAssetsNode,
        parent: Entity,
        entities: &mut Vec<Entity>,
    ) {
        let entity = world
            .spawn((
                Name::new(node.name.clone()),
                node.transform,
                Visibility::default(),
                ChildOf(parent),
            ))
            .id();
        entities.push(entity);
        for primitive in &node.primitives {
            entities.push(
                world
                    .spawn((
                        Mesh3d(primitive.mesh.clone()),
                        MeshMaterial3d(primitive.material.clone()),
                        Transform::default(),
                        ChildOf(entity),
                        RenderLayers::layer(Self::RENDER_LAYER),
                    ))
                    .id(),
            );
        }
        for child in &node.children {
            Self::spawn_node(world, child, entity, entities);
        }
    }
}
