use bevy_egui::{EguiUserTextures, egui};

use crate::scene::camera::{SceneCamera, SceneCameraError, SceneCameraFrame};
use crate::scene::enums::SceneEnumLifecycle;
use crate::scene::picking::ScenePickingViewport;
use crate::scene::renderer::{SceneRenderer, SceneRendererGeneration};

#[derive(Debug, Clone)]
pub struct PresentationBoardScene {
    texture: egui::TextureId,
    generation: SceneRendererGeneration,
    lifecycle: SceneEnumLifecycle,
    viewport: ScenePickingViewport,
    camera: SceneCameraFrame,
}

impl PresentationBoardScene {
    pub fn new(
        texture: egui::TextureId,
        generation: SceneRendererGeneration,
        lifecycle: SceneEnumLifecycle,
        viewport: ScenePickingViewport,
        camera: SceneCameraFrame,
    ) -> Self {
        Self {
            texture,
            generation,
            lifecycle,
            viewport,
            camera,
        }
    }

    pub fn from_renderer(
        renderer: &SceneRenderer,
        textures: &mut EguiUserTextures,
        camera: SceneCamera,
    ) -> Result<Self, SceneCameraError> {
        Ok(Self::new(
            renderer.register_image(textures),
            renderer.generation(),
            renderer.lifecycle(),
            renderer.viewport(),
            camera.fit(renderer.viewport().physical_size())?,
        ))
    }

    pub const fn texture(&self) -> egui::TextureId {
        self.texture
    }

    pub const fn generation(&self) -> SceneRendererGeneration {
        self.generation
    }

    pub const fn lifecycle(&self) -> SceneEnumLifecycle {
        self.lifecycle
    }

    pub fn fit(&self, rect: egui::Rect) -> egui::Rect {
        let physical = self.viewport.physical_size();
        let aspect = physical.x as f32 / physical.y.max(1) as f32;
        let size = if rect.width() / rect.height().max(f32::EPSILON) > aspect {
            egui::vec2(rect.height() * aspect, rect.height())
        } else {
            egui::vec2(rect.width(), rect.width() / aspect.max(f32::EPSILON))
        };
        egui::Rect::from_center_size(rect.center(), size)
    }

    pub const fn viewport(&self) -> ScenePickingViewport {
        self.viewport
    }

    pub const fn camera(&self) -> &SceneCameraFrame {
        &self.camera
    }
}
