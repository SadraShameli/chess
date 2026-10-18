use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::scene::camera::{SceneCamera, SceneCameraError, SceneCameraPose};

#[derive(Debug, Error)]
pub enum DesktopLayoutError {
    #[error("The display is {width}x{height} logical points; the minimum window is 1280x800")]
    DisplayTooSmall { width: f32, height: f32 },
    #[error("The remembered window geometry is not finite")]
    Geometry,
    #[error(transparent)]
    Camera(#[from] SceneCameraError),
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    Default,
    strum::EnumIter,
    strum::IntoStaticStr,
)]
pub enum DesktopLayoutEnumTab {
    #[default]
    #[strum(serialize = "Game")]
    Game,
    #[strum(serialize = "Analysis")]
    Analysis,
    #[strum(serialize = "Review")]
    Review,
    #[strum(serialize = "Report")]
    Report,
}

impl DesktopLayoutEnumTab {
    pub fn label(self) -> &'static str {
        self.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesktopLayoutDisplay {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DesktopLayout {
    window_x: f32,
    window_y: f32,
    window_width: f32,
    window_height: f32,
    #[serde(default)]
    window_positioned: bool,
    navigation_collapsed: bool,
    panel_width: f32,
    tab: DesktopLayoutEnumTab,
    camera_yaw: f32,
    camera_tilt: f32,
    camera_zoom: f32,
}

impl Default for DesktopLayout {
    fn default() -> Self {
        let camera = SceneCamera::initial(BoardEnumColor::White).pose();
        Self {
            window_x: 0.0,
            window_y: 0.0,
            window_width: Self::DEFAULT_WIDTH,
            window_height: Self::DEFAULT_HEIGHT,
            window_positioned: false,
            navigation_collapsed: false,
            panel_width: Self::DEFAULT_PANEL_WIDTH,
            tab: DesktopLayoutEnumTab::default(),
            camera_yaw: camera.yaw_degrees(),
            camera_tilt: camera.tilt_degrees(),
            camera_zoom: camera.zoom(),
        }
    }
}

impl DesktopLayout {
    pub const DEFAULT_WIDTH: f32 = 1440.0;
    pub const DEFAULT_HEIGHT: f32 = 900.0;
    pub const MIN_WIDTH: f32 = 1280.0;
    pub const MIN_HEIGHT: f32 = 800.0;
    pub const DEFAULT_PANEL_WIDTH: f32 = 360.0;
    pub const MIN_PANEL_WIDTH: f32 = 300.0;
    pub const MAX_PANEL_WIDTH: f32 = 560.0;

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub const fn window_size(&self) -> (f32, f32) {
        (self.window_width, self.window_height)
    }

    pub const fn window_position(&self) -> (f32, f32) {
        (self.window_x, self.window_y)
    }

    pub const fn is_window_positioned(&self) -> bool {
        self.window_positioned
    }

    pub fn set_window(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Result<(), DesktopLayoutError> {
        if ![x, y, width, height].iter().all(|value| value.is_finite()) {
            return Err(DesktopLayoutError::Geometry);
        }
        self.window_x = x;
        self.window_y = y;
        self.window_positioned = true;
        self.window_width = width.max(Self::MIN_WIDTH);
        self.window_height = height.max(Self::MIN_HEIGHT);
        Ok(())
    }

    pub const fn navigation_collapsed(&self) -> bool {
        self.navigation_collapsed
    }

    pub fn set_navigation_collapsed(&mut self, collapsed: bool) {
        self.navigation_collapsed = collapsed;
    }

    pub const fn panel_width(&self) -> f32 {
        self.panel_width
    }

    pub fn set_panel_width(&mut self, width: f32) {
        if width.is_finite() {
            self.panel_width = width.clamp(Self::MIN_PANEL_WIDTH, Self::MAX_PANEL_WIDTH);
        }
    }

    pub const fn tab(&self) -> DesktopLayoutEnumTab {
        self.tab
    }

    pub fn set_tab(&mut self, tab: DesktopLayoutEnumTab) {
        self.tab = tab;
    }

    pub fn remember_camera(&mut self, camera: SceneCamera) {
        let pose = camera.pose();
        self.camera_yaw = pose.yaw_degrees();
        self.camera_tilt = pose.tilt_degrees();
        self.camera_zoom = pose.zoom();
    }

    pub fn camera(&self, orientation: BoardEnumColor) -> Result<SceneCamera, DesktopLayoutError> {
        Ok(SceneCamera::new(
            self.camera_yaw,
            self.camera_tilt,
            self.camera_zoom,
            orientation,
        )?)
    }

    pub fn camera_pose(&self) -> Result<SceneCameraPose, DesktopLayoutError> {
        Ok(self.camera(BoardEnumColor::White)?.pose())
    }

    pub fn fit_to_display(
        &mut self,
        display: DesktopLayoutDisplay,
    ) -> Result<(), DesktopLayoutError> {
        if ![display.x, display.y, display.width, display.height]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(DesktopLayoutError::Geometry);
        }
        if display.width < Self::MIN_WIDTH || display.height < Self::MIN_HEIGHT {
            return Err(DesktopLayoutError::DisplayTooSmall {
                width: display.width,
                height: display.height,
            });
        }
        self.window_width = self.window_width.clamp(Self::MIN_WIDTH, display.width);
        self.window_height = self.window_height.clamp(Self::MIN_HEIGHT, display.height);
        let max_x = display.x + display.width - self.window_width;
        let max_y = display.y + display.height - self.window_height;
        self.window_x = self.window_x.clamp(display.x, max_x);
        self.window_y = self.window_y.clamp(display.y, max_y);
        Ok(())
    }
}
