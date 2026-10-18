use bevy::math::{UVec2, Vec2, Vec3};
use bevy::prelude::Ray3d;
use bevy_egui::egui::{Pos2, Rect};
use thiserror::Error;

use crate::board::square::BoardSquare;
use crate::scene::renderer::SceneRendererGeneration;

#[derive(Debug, Error)]
pub enum ScenePickingViewportError {
    #[error("The board image rectangle and clip rectangle must contain finite coordinates")]
    Rectangle,
    #[error("The board display scale must be positive and render scale must be in 0.75..1")]
    Scale,
    #[error("The board target must have nonzero dimensions no larger than 4096 pixels")]
    Dimensions,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScenePickingViewport {
    image: Rect,
    clip: Rect,
    pixels_per_point: f32,
    render_scale: f32,
    physical_size: UVec2,
}

impl ScenePickingViewport {
    pub const MAX_TARGET_DIMENSION: f32 = 4096.0;

    pub fn new(
        image: Rect,
        clip: Rect,
        pixels_per_point: f32,
        render_scale: f32,
    ) -> Result<Self, ScenePickingViewportError> {
        if !image.min.is_finite()
            || !image.max.is_finite()
            || !clip.min.is_finite()
            || !clip.max.is_finite()
            || image.width() <= 0.0
            || image.height() <= 0.0
        {
            return Err(ScenePickingViewportError::Rectangle);
        }
        if !pixels_per_point.is_finite()
            || pixels_per_point <= 0.0
            || !(0.75..=1.0).contains(&render_scale)
        {
            return Err(ScenePickingViewportError::Scale);
        }
        let physical = Vec2::new(image.width(), image.height()) * pixels_per_point * render_scale;
        let physical = physical.ceil();
        if !physical.is_finite()
            || physical.cmplt(Vec2::ONE).any()
            || physical
                .cmpgt(Vec2::splat(Self::MAX_TARGET_DIMENSION))
                .any()
        {
            return Err(ScenePickingViewportError::Dimensions);
        }
        Ok(Self {
            image,
            clip,
            pixels_per_point,
            render_scale,
            physical_size: physical.as_uvec2(),
        })
    }

    pub const fn physical_size(self) -> UVec2 {
        self.physical_size
    }

    pub const fn image(self) -> Rect {
        self.image
    }

    pub fn to_physical(self, point: Pos2) -> Option<Vec2> {
        let visible = self.image.intersect(self.clip);
        if !point.is_finite()
            || point.x < visible.min.x
            || point.y < visible.min.y
            || point.x >= visible.max.x
            || point.y >= visible.max.y
        {
            return None;
        }
        let relative = point - self.image.min;
        Some(
            Vec2::new(
                relative.x / self.image.width(),
                relative.y / self.image.height(),
            ) * self.physical_size.as_vec2(),
        )
    }

    pub fn to_logical(self, pixel: Vec2) -> Option<Pos2> {
        let size = self.physical_size.as_vec2();
        if !pixel.is_finite() || pixel.cmplt(Vec2::ZERO).any() || pixel.cmpge(size).any() {
            return None;
        }
        let normalized = pixel / size;
        let point = Pos2::new(
            self.image.min.x + normalized.x * self.image.width(),
            self.image.min.y + normalized.y * self.image.height(),
        );
        self.to_physical(point).map(|_pixel| point)
    }
}

enum ScenePickingEnumGesture {
    Idle,
    Pressed(BoardSquare),
    ConsumeRelease,
}

pub struct ScenePicking {
    generation: SceneRendererGeneration,
    gesture: ScenePickingEnumGesture,
}

impl ScenePicking {
    pub const FILES: u8 = 8;
    pub const SQUARES: u8 = Self::FILES * Self::FILES;
    pub const HALF_BOARD: f32 = 4.0;

    pub const fn new(generation: SceneRendererGeneration) -> Self {
        Self {
            generation,
            gesture: ScenePickingEnumGesture::Idle,
        }
    }

    pub fn square_center(square: BoardSquare) -> Vec3 {
        let index = u8::from(square);
        Vec3::new(
            f32::from(index % Self::FILES) - Self::HALF_BOARD + 0.5,
            0.0,
            Self::HALF_BOARD - 0.5 - f32::from(index / Self::FILES),
        )
    }

    pub fn board_square(ray: Ray3d) -> Option<BoardSquare> {
        if ray.direction.y.abs() <= f32::EPSILON {
            return None;
        }
        let distance = -ray.origin.y / ray.direction.y;
        if distance < 0.0 || !distance.is_finite() {
            return None;
        }
        let point = ray.get_point(distance);
        let coordinates = Vec2::new(point.x + Self::HALF_BOARD, Self::HALF_BOARD - point.z);
        if !coordinates.is_finite()
            || coordinates.cmplt(Vec2::ZERO).any()
            || coordinates.cmpge(Vec2::splat(f32::from(Self::FILES))).any()
        {
            return None;
        }
        let coordinates = coordinates.floor().as_uvec2();
        let index = coordinates.y * u32::from(Self::FILES) + coordinates.x;
        BoardSquare::try_from(u8::try_from(index).ok()?).ok()
    }

    pub fn invalidate(&mut self, generation: SceneRendererGeneration) {
        if self.generation != generation {
            self.generation = generation;
            self.cancel();
        }
    }

    pub fn cancel(&mut self) {
        self.gesture = match self.gesture {
            ScenePickingEnumGesture::Idle => ScenePickingEnumGesture::Idle,
            ScenePickingEnumGesture::Pressed(_) | ScenePickingEnumGesture::ConsumeRelease => {
                ScenePickingEnumGesture::ConsumeRelease
            }
        };
    }

    pub fn press(&mut self, generation: SceneRendererGeneration, square: BoardSquare) {
        if self.generation == generation && matches!(self.gesture, ScenePickingEnumGesture::Idle) {
            self.gesture = ScenePickingEnumGesture::Pressed(square);
        }
    }

    pub fn release(
        &mut self,
        generation: SceneRendererGeneration,
        square: Option<BoardSquare>,
    ) -> Option<(BoardSquare, BoardSquare)> {
        let previous = std::mem::replace(&mut self.gesture, ScenePickingEnumGesture::Idle);
        match previous {
            ScenePickingEnumGesture::Pressed(from) if generation == self.generation => {
                square.map(|to| (from, to))
            }
            ScenePickingEnumGesture::Pressed(_)
            | ScenePickingEnumGesture::Idle
            | ScenePickingEnumGesture::ConsumeRelease => None,
        }
    }
}
