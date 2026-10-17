use bevy_egui::egui::{self, Ui};

use crate::presentation::board::enums::PresentationBoardEnumCameraAction;
use crate::presentation::board::markings::{PresentationBoardMarkColor, PresentationBoardMarkings};
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;

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

    pub fn show(
        ui: &mut Ui,
        enabled: bool,
        markings: &mut PresentationBoardMarkings,
    ) -> Option<PresentationBoardEnumCameraAction> {
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
        ui.horizontal_wrapped(|ui| {
            Self::show_markings(ui, markings);
        });
        selected
    }

    fn show_markings(ui: &mut Ui, markings: &mut PresentationBoardMarkings) {
        ui.label("Marking color");
        let mut color = markings.selected_color();
        for choice in PresentationBoardMarkColor::ALL {
            ui.selectable_value(
                &mut color,
                choice,
                egui::RichText::new(choice.label()).color(choice.color()),
            );
        }
        markings.select_color(color);
        ui.label("Mark from");
        ui.add(
            egui::TextEdit::singleline(markings.entry_origin_mut())
                .id_salt("chess.board.marking_origin")
                .hint_text("e2")
                .desired_width(40.0),
        )
        .widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Marking source square")
        });
        ui.label("to");
        ui.add(
            egui::TextEdit::singleline(markings.entry_destination_mut())
                .id_salt("chess.board.marking_destination")
                .hint_text("e4")
                .desired_width(40.0),
        )
        .widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::TextEdit,
                true,
                "Marking destination square for arrows",
            )
        });
        if ui.button("Mark Circle").clicked() {
            markings.toggle_entry(false);
        }
        if ui.button("Mark Arrow").clicked() {
            markings.toggle_entry(true);
        }
        if markings.is_entry_invalid() {
            ui.label("Enter squares such as e2 and e4, different for arrows");
        }
        if ui
            .add_enabled(
                markings.has_temporary(),
                egui::Button::new("Clear Board Markings").small(),
            )
            .clicked()
        {
            markings.clear();
        }
        let aids = markings.aids_mut();
        ui.checkbox(&mut aids.legal_dots, "Legal moves");
        ui.checkbox(&mut aids.last_move, "Last move");
        ui.checkbox(&mut aids.check, "Check");
        ui.checkbox(&mut aids.coordinates, "Coordinates");
        ui.checkbox(&mut aids.captured, "Captured");
        if markings.aids().captured {
            let captures = markings.captures();
            let text = if captures.is_empty() {
                "No captures".to_owned()
            } else {
                captures
                    .iter()
                    .map(|capture| {
                        format!(
                            "{} {} x{}",
                            PresentationBoardSemanticGrid::color_name(capture.color),
                            PresentationBoardSemanticGrid::piece_name(capture.kind),
                            capture.count
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            ui.label(format!("Captured: {text}"));
        }
    }
}
