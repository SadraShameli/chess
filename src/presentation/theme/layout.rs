use crate::desktop::layout::DesktopLayout;
use crate::presentation::workbench::workspace::{
    PresentationWorkbenchWorkspace, PresentationWorkbenchWorkspaceError,
    PresentationWorkbenchWorkspaceLayout,
};

pub struct PresentationThemeLayout;

impl PresentationThemeLayout {
    pub fn resolve(
        layout: &DesktopLayout,
        window_width: f32,
        window_height: f32,
    ) -> Result<PresentationWorkbenchWorkspaceLayout, PresentationWorkbenchWorkspaceError> {
        PresentationWorkbenchWorkspace::layout(
            window_width,
            window_height,
            layout.navigation_collapsed(),
            layout.panel_width(),
        )
    }
}
