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
            other @ Action::Blur
            | other @ Action::Collapse
            | other @ Action::Expand
            | other @ Action::CustomAction
            | other @ Action::Decrement
            | other @ Action::Increment
            | other @ Action::HideTooltip
            | other @ Action::ShowTooltip
            | other @ Action::ReplaceSelectedText
            | other @ Action::ScrollDown
            | other @ Action::ScrollLeft
            | other @ Action::ScrollRight
            | other @ Action::ScrollUp
            | other @ Action::ScrollIntoView
            | other @ Action::ScrollToPoint
            | other @ Action::SetScrollOffset
            | other @ Action::SetTextSelection
            | other @ Action::SetSequentialFocusNavigationStartingPoint
            | other @ Action::SetValue
            | other @ Action::ShowContextMenu => {
                Err(DesktopNativeAccessibilityError::Unsupported(other))
            }
        }
    }

    pub fn is_supported(action: Action) -> bool {
        Self::SUPPORTED.contains(&action)
    }
}
