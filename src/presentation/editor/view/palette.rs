use bevy_egui::egui::{self, Ui};

use crate::board::editor::{BoardEditorSessionEnumOperation, BoardEditorSessionEnumSelection};
use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::desktop::editor_commands::DesktopEditorCommandsSnapshot;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::editor::controller::PresentationEditorControllerPalette;
use crate::presentation::editor::view::{PresentationEditorView, PresentationEditorViewEnumAction};

impl PresentationEditorView {
    pub(super) fn show_palette(
        ui: &mut Ui,
        snapshot: &DesktopEditorCommandsSnapshot,
        action: &mut Option<PresentationEditorViewEnumAction>,
    ) {
        ui.group(|ui| {
            ui.label("Piece palette");
            for color in [BoardEnumColor::White, BoardEnumColor::Black] {
                ui.horizontal_wrapped(|ui| {
                    for kind in [
                        BoardEnumPieceKind::King,
                        BoardEnumPieceKind::Queen,
                        BoardEnumPieceKind::Rook,
                        BoardEnumPieceKind::Bishop,
                        BoardEnumPieceKind::Knight,
                        BoardEnumPieceKind::Pawn,
                    ] {
                        let selection = BoardEditorSessionEnumSelection::Palette { color, kind };
                        let label = format!(
                            "{} {}",
                            PresentationBoardSemanticGrid::color_name(color),
                            PresentationBoardSemanticGrid::piece_name(kind)
                        );
                        let response = ui.add(
                            egui::Button::new(label)
                                .selected(snapshot.selection() == selection)
                                .sense(egui::Sense::click_and_drag()),
                        );
                        response.dnd_set_drag_payload(PresentationEditorControllerPalette::new(
                            snapshot.context(),
                            color,
                            kind,
                        ));
                        if response.clicked() {
                            *action = Some(Self::edit(
                                snapshot.context(),
                                BoardEditorSessionEnumOperation::Select(selection),
                            ));
                        }
                    }
                });
            }
        });
    }
}
