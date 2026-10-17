use bevy_egui::egui::accesskit::{Action, ActionRequest};
use thiserror::Error;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeAccessibilityError {
    #[error("The assistive technology requested an unsupported action: {0:?}")]
    Unsupported(Action),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeAccessibilityEnumAction {
    Click,
    Focus,
}

pub struct DesktopNativeAccessibility;

impl DesktopNativeAccessibility {
    pub const SUPPORTED: [Action; 2] = [Action::Click, Action::Focus];

    pub fn classify(
        request: &ActionRequest,
    ) -> Result<DesktopNativeAccessibilityEnumAction, DesktopNativeAccessibilityError> {
        match request.action {
            Action::Click => Ok(DesktopNativeAccessibilityEnumAction::Click),
            Action::Focus => Ok(DesktopNativeAccessibilityEnumAction::Focus),
            other => Err(DesktopNativeAccessibilityError::Unsupported(other)),
        }
    }

    pub fn is_supported(action: Action) -> bool {
        Self::SUPPORTED.contains(&action)
    }
}
