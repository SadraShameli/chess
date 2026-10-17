use bevy_egui::egui::{self, Ui};

use crate::presentation::board::enums::PresentationBoardEnumCameraAction;

pub struct PresentationBoardToolbar;

impl PresentationBoardToolbar {
    const ACTIONS: [(&'static str, PresentationBoardEnumCameraAction); 8] = [
        ("Rotate left", PresentationBoardEnumCameraAction::RotateLeft),
        (
            "Rotate right",
            PresentationBoardEnumCameraAction::RotateRight,
        ),
        ("Tilt up", PresentationBoardEnumCameraAction::TiltUp),
        ("Tilt down", PresentationBoardEnumCameraAction::TiltDown),
        ("Zoom in", PresentationBoardEnumCameraAction::ZoomIn),
        ("Zoom out", PresentationBoardEnumCameraAction::ZoomOut),
        ("Reset view", PresentationBoardEnumCameraAction::Reset),
        ("Flip board", PresentationBoardEnumCameraAction::Flip),
    ];

    pub fn show(ui: &mut Ui, enabled: bool) -> Option<PresentationBoardEnumCameraAction> {
        let mut selected = None;
        ui.horizontal_wrapped(|ui| {
            for (label, action) in Self::ACTIONS {
                if ui
                    .add_enabled(enabled, egui::Button::new(label).small())
                    .clicked()
                {
                    selected = Some(action);
                }
            }
        });
        selected
    }
}
