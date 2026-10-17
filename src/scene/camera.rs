use bevy::camera::CameraProjection;
use bevy::math::{Dir3, UVec2, Vec2, Vec3};
use bevy::prelude::{PerspectiveProjection, Ray3d, Transform};
use thiserror::Error;

use crate::board::enums::BoardEnumColor;

#[derive(Debug, Error)]
pub enum SceneCameraError {
    #[error("Camera yaw must be finite, tilt must be in 0..45 degrees, and zoom in 1..1.6")]
    Pose,
    #[error("The camera requires a nonempty physical viewport")]
    Viewport,
    #[error("The camera could not produce a finite ray for this viewport point")]
    Ray,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneCamera {
    yaw_degrees: f32,
    tilt_degrees: f32,
    zoom: f32,
    orientation: BoardEnumColor,
}

impl SceneCamera {
    pub const FIELD_OF_VIEW_DEGREES: f32 = 35.0;
    pub const FRAMING_MARGIN: f32 = 1.08;
    pub const MAX_TILT_DEGREES: f32 = 45.0;
    pub const MIN_ZOOM: f32 = 1.0;
    pub const MAX_ZOOM: f32 = 1.6;
    const FULL_TURN_DEGREES: f32 = 360.0;
    const YAW_STEP_DEGREES: f32 = 15.0;
    const TILT_STEP_DEGREES: f32 = 5.0;
    const ZOOM_FACTOR: f32 = 1.1;
    const INITIAL_TILT_DEGREES: f32 = 20.0;
    const FRAME_HALF_WIDTH: f32 = 4.3;
    const FRAME_BOTTOM: f32 = -0.2;
    const PIECE_TOP: f32 = 1.3;

    pub fn new(
        yaw_degrees: f32,
        tilt_degrees: f32,
        zoom: f32,
        orientation: BoardEnumColor,
    ) -> Result<Self, SceneCameraError> {
        if !yaw_degrees.is_finite()
            || !(0.0..=Self::MAX_TILT_DEGREES).contains(&tilt_degrees)
            || !(Self::MIN_ZOOM..=Self::MAX_ZOOM).contains(&zoom)
        {
            return Err(SceneCameraError::Pose);
        }
        Ok(Self {
            yaw_degrees: yaw_degrees.rem_euclid(Self::FULL_TURN_DEGREES),
            tilt_degrees,
            zoom,
            orientation,
        })
    }

    pub const fn initial(orientation: BoardEnumColor) -> Self {
        Self {
            yaw_degrees: 0.0,
            tilt_degrees: Self::INITIAL_TILT_DEGREES,
            zoom: Self::ZOOM_FACTOR,
            orientation,
        }
    }

    pub const fn yaw_degrees(self) -> f32 {
        self.yaw_degrees
    }

    pub const fn tilt_degrees(self) -> f32 {
        self.tilt_degrees
    }

    pub const fn zoom(self) -> f32 {
        self.zoom
    }

    pub const fn orientation(self) -> BoardEnumColor {
        self.orientation
    }

    pub const fn with_orientation(self, orientation: BoardEnumColor) -> Self {
        Self {
            orientation,
            ..self
        }
    }

    pub const fn reset(self) -> Self {
        Self::initial(self.orientation)
    }

    pub fn rotate_right(self) -> Self {
        self.rotated(Self::YAW_STEP_DEGREES)
    }

    pub fn rotate_left(self) -> Self {
        self.rotated(-Self::YAW_STEP_DEGREES)
    }

    pub fn tilt_up(self) -> Self {
        self.tilted(Self::TILT_STEP_DEGREES)
    }

    pub fn tilt_down(self) -> Self {
        self.tilted(-Self::TILT_STEP_DEGREES)
    }

    pub fn zoom_in(self) -> Self {
        Self {
            zoom: (self.zoom / Self::ZOOM_FACTOR).max(Self::MIN_ZOOM),
            ..self
        }
    }

    pub fn zoom_out(self) -> Self {
        Self {
            zoom: (self.zoom * Self::ZOOM_FACTOR).min(Self::MAX_ZOOM),
            ..self
        }
    }

    pub fn framing_corners() -> impl Iterator<Item = Vec3> {
        [-Self::FRAME_HALF_WIDTH, Self::FRAME_HALF_WIDTH]
            .into_iter()
            .flat_map(|x| {
                [Self::FRAME_BOTTOM, Self::PIECE_TOP]
                    .into_iter()
                    .flat_map(move |y| {
                        [-Self::FRAME_HALF_WIDTH, Self::FRAME_HALF_WIDTH]
                            .into_iter()
                            .map(move |z| Vec3::new(x, y, z))
                    })
            })
    }

    pub fn fit(self, size: UVec2) -> Result<SceneCameraFrame, SceneCameraError> {
        if size.x == 0 || size.y == 0 {
            return Err(SceneCameraError::Viewport);
        }
        let orientation = match self.orientation {
            BoardEnumColor::White => 0.0,
            BoardEnumColor::Black => Self::FULL_TURN_DEGREES / 2.0,
        };
        let yaw = (self.yaw_degrees + orientation).to_radians();
        let tilt = self.tilt_degrees.to_radians();
        let offset = Vec3::new(yaw.sin() * tilt.sin(), tilt.cos(), yaw.cos() * tilt.sin());
        let up = Vec3::new(-yaw.sin() * tilt.cos(), tilt.sin(), -yaw.cos() * tilt.cos());
        let rotation = Transform::from_translation(offset)
            .looking_at(Vec3::ZERO, up)
            .rotation;
        let size_float = size.as_vec2();
        let mut projection = PerspectiveProjection {
            fov: Self::FIELD_OF_VIEW_DEGREES.to_radians(),
            aspect_ratio: size_float.x / size_float.y,
            ..PerspectiveProjection::default()
        };
        let vertical_tangent = (projection.fov / 2.0).tan();
        let horizontal_tangent = vertical_tangent * projection.aspect_ratio;
        let distance = Self::framing_corners().fold(projection.near, |distance, corner| {
            let view = rotation.inverse() * corner;
            distance
                .max(view.z + view.x.abs() * Self::FRAMING_MARGIN / horizontal_tangent)
                .max(view.z + view.y.abs() * Self::FRAMING_MARGIN / vertical_tangent)
        }) * self.zoom;
        projection.far = distance
            + Self::framing_corners()
                .map(Vec3::length)
                .fold(0.0, f32::max);
        Ok(SceneCameraFrame {
            size,
            orientation: self.orientation,
            transform: Transform::from_translation(offset * distance).with_rotation(rotation),
            projection,
        })
    }

    fn rotated(self, degrees: f32) -> Self {
        Self {
            yaw_degrees: (self.yaw_degrees + degrees).rem_euclid(Self::FULL_TURN_DEGREES),
            ..self
        }
    }

    fn tilted(self, degrees: f32) -> Self {
        Self {
            tilt_degrees: (self.tilt_degrees + degrees).clamp(0.0, Self::MAX_TILT_DEGREES),
            ..self
        }
    }
}

#[derive(Debug, Clone)]
pub struct SceneCameraFrame {
    size: UVec2,
    orientation: BoardEnumColor,
    transform: Transform,
    projection: PerspectiveProjection,
}

impl SceneCameraFrame {
    pub const fn orientation(&self) -> BoardEnumColor {
        self.orientation
    }

    pub const fn transform(&self) -> Transform {
        self.transform
    }

    pub fn projection(&self) -> PerspectiveProjection {
        self.projection.clone()
    }

    pub fn project(&self, world: Vec3) -> Option<Vec2> {
        if !world.is_finite() {
            return None;
        }
        let view = self.transform.to_matrix().inverse().transform_point3(world);
        if view.z >= -self.projection.near {
            return None;
        }
        let clip = self.projection.get_clip_from_view().project_point3(view);
        if !clip.is_finite() || clip.x.abs() > 1.0 || clip.y.abs() > 1.0 {
            return None;
        }
        Some(Vec2::new(clip.x + 1.0, 1.0 - clip.y) * self.size.as_vec2() / 2.0)
    }

    pub fn ray(&self, pixel: Vec2) -> Result<Ray3d, SceneCameraError> {
        let size = self.size.as_vec2();
        if !pixel.is_finite() || pixel.cmplt(Vec2::ZERO).any() || pixel.cmpge(size).any() {
            return Err(SceneCameraError::Ray);
        }
        let normalized = pixel / size;
        let ndc = Vec2::new(normalized.x * 2.0 - 1.0, 1.0 - normalized.y * 2.0);
        let inverse = self.projection.get_clip_from_view().inverse();
        let near = inverse.project_point3(ndc.extend(1.0));
        let far = inverse.project_point3(ndc.extend(f32::EPSILON));
        let direction = Dir3::new(self.transform.rotation * (far - near))
            .map_err(|_error| SceneCameraError::Ray)?;
        Ok(Ray3d::new(self.transform.transform_point(near), direction))
    }
}
