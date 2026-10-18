use thiserror::Error;

#[derive(Debug, Error, Clone, Copy, PartialEq)]
pub enum PresentationWorkbenchWorkspaceError {
    #[error("The window is {width}x{height} logical points; the minimum is 1280x800")]
    BelowMinimum { width: f32, height: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationWorkbenchWorkspaceEnumPanel {
    Docked,
    Drawer,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentationWorkbenchWorkspaceLayout {
    navigation_width: f32,
    navigation_collapsed: bool,
    panel: PresentationWorkbenchWorkspaceEnumPanel,
    panel_width: f32,
    board_workspace_width: f32,
}

impl PresentationWorkbenchWorkspaceLayout {
    pub const fn navigation_width(self) -> f32 {
        self.navigation_width
    }

    pub const fn is_navigation_collapsed(self) -> bool {
        self.navigation_collapsed
    }

    pub const fn panel(self) -> PresentationWorkbenchWorkspaceEnumPanel {
        self.panel
    }

    pub const fn panel_width(self) -> f32 {
        self.panel_width
    }

    pub const fn board_workspace_width(self) -> f32 {
        self.board_workspace_width
    }
}

pub struct PresentationWorkbenchWorkspace;

impl PresentationWorkbenchWorkspace {
    pub const DEFAULT_WIDTH: f32 = 1440.0;
    pub const DEFAULT_HEIGHT: f32 = 900.0;
    pub const MIN_WIDTH: f32 = 1280.0;
    pub const MIN_HEIGHT: f32 = 800.0;
    pub const NAVIGATION_EXPANDED: f32 = 208.0;
    pub const NAVIGATION_COLLAPSED: f32 = 64.0;
    pub const PANEL_DEFAULT: f32 = 360.0;
    pub const PANEL_MIN: f32 = 300.0;
    pub const PANEL_MAX: f32 = 560.0;
    pub const BOARD_WORKSPACE_MIN: f32 = 560.0;
    pub const DRAWER_BELOW_WIDTH: f32 = 1360.0;

    pub fn layout(
        width: f32,
        height: f32,
        navigation_collapsed: bool,
        requested_panel_width: f32,
    ) -> Result<PresentationWorkbenchWorkspaceLayout, PresentationWorkbenchWorkspaceError> {
        if !(width >= Self::MIN_WIDTH && height >= Self::MIN_HEIGHT) {
            return Err(PresentationWorkbenchWorkspaceError::BelowMinimum { width, height });
        }
        let requested = if requested_panel_width.is_finite() {
            requested_panel_width.clamp(Self::PANEL_MIN, Self::PANEL_MAX)
        } else {
            Self::PANEL_DEFAULT
        };
        let navigation = |collapsed: bool| {
            if collapsed {
                Self::NAVIGATION_COLLAPSED
            } else {
                Self::NAVIGATION_EXPANDED
            }
        };
        if width < Self::DRAWER_BELOW_WIDTH {
            let collapsed = true;
            return Ok(PresentationWorkbenchWorkspaceLayout {
                navigation_width: navigation(collapsed),
                navigation_collapsed: collapsed,
                panel: PresentationWorkbenchWorkspaceEnumPanel::Drawer,
                panel_width: requested,
                board_workspace_width: width - navigation(collapsed),
            });
        }
        let mut collapsed = navigation_collapsed;
        let mut panel_width = requested;
        let board = |collapsed: bool, panel_width: f32| width - navigation(collapsed) - panel_width;
        if board(collapsed, panel_width) < Self::BOARD_WORKSPACE_MIN {
            collapsed = true;
        }
        if board(collapsed, panel_width) < Self::BOARD_WORKSPACE_MIN {
            panel_width = (width - navigation(collapsed) - Self::BOARD_WORKSPACE_MIN)
                .clamp(Self::PANEL_MIN, Self::PANEL_MAX);
        }
        if board(collapsed, panel_width) < Self::BOARD_WORKSPACE_MIN {
            return Ok(PresentationWorkbenchWorkspaceLayout {
                navigation_width: navigation(collapsed),
                navigation_collapsed: collapsed,
                panel: PresentationWorkbenchWorkspaceEnumPanel::Drawer,
                panel_width: requested,
                board_workspace_width: width - navigation(collapsed),
            });
        }
        Ok(PresentationWorkbenchWorkspaceLayout {
            navigation_width: navigation(collapsed),
            navigation_collapsed: collapsed,
            panel: PresentationWorkbenchWorkspaceEnumPanel::Docked,
            panel_width,
            board_workspace_width: board(collapsed, panel_width),
        })
    }
}
