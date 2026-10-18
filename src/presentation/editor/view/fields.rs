use bevy_egui::egui::{self, Ui};

use crate::board::editor::{BoardEditorSessionEnumEdit, BoardEditorSessionEnumOperation};
use crate::board::enums::{BoardEnumColor, BoardEnumVariant};
use crate::desktop::editor_commands::DesktopEditorCommandsSnapshot;
use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::presentation::editor::view::{PresentationEditorView, PresentationEditorViewEnumAction};
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
};

impl PresentationEditorView {
    pub(super) fn show_fields(
        &mut self,
        ui: &mut Ui,
        snapshot: &DesktopEditorCommandsSnapshot,
        action: &mut Option<PresentationEditorViewEnumAction>,
    ) {
        let context = snapshot.context();
        let fields = snapshot.draft().fields();
        PresentationThemeComponents::card(ui).show(ui, |ui| {
            PresentationThemeComponents::subhead(ui, "Position fields");
            let mut turn = fields.turn();
            ui.horizontal(|ui| {
                ui.label("Side to move");
                let white = ui
                    .selectable_value(&mut turn, BoardEnumColor::White, "White to move")
                    .changed();
                let black = ui
                    .selectable_value(&mut turn, BoardEnumColor::Black, "Black to move")
                    .changed();
                if white || black {
                    *action = Some(Self::edit(
                        context,
                        BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Turn(
                            turn,
                        )),
                    ));
                }
            });
            let mut variant = fields.variant();
            ui.horizontal(|ui| {
                ui.label("Declared variant");
                let standard = ui
                    .selectable_value(&mut variant, BoardEnumVariant::Standard, "Standard")
                    .changed();
                let chess960 = ui
                    .selectable_value(&mut variant, BoardEnumVariant::Chess960, "Chess960")
                    .changed();
                if standard || chess960 {
                    *action = Some(Self::edit(
                        context,
                        BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Variant(
                            variant,
                        )),
                    ));
                }
            });
            let mut ep = fields.en_passant();
            egui::ComboBox::from_id_salt("editor.en-passant")
                .selected_text(ep.map_or_else(
                    || "No en passant target".to_owned(),
                    |square| format!("En passant: {square}"),
                ))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut ep, None, "No en passant target");
                    for square in PresentationBoardSemanticGrid::squares() {
                        ui.selectable_value(&mut ep, Some(square), square.to_string());
                    }
                });
            if ep != fields.en_passant() {
                *action = Some(Self::edit(
                    context,
                    BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::EnPassant(
                        ep,
                    )),
                ));
            }
            let mut halfmoves = fields.halfmoves();
            let mut fullmoves = fields.fullmoves();
            let mut counters_changed = false;
            ui.horizontal(|ui| {
                ui.label("Halfmove counter (0..1000000)");
                counters_changed |= ui.add(egui::DragValue::new(&mut halfmoves)).changed();
                ui.label("Fullmove counter (1..1000000)");
                counters_changed |= ui.add(egui::DragValue::new(&mut fullmoves)).changed();
            });
            if counters_changed {
                *action = Some(Self::edit(
                    context,
                    BoardEditorSessionEnumOperation::Edit(BoardEditorSessionEnumEdit::Counters {
                        halfmoves,
                        fullmoves,
                    }),
                ));
            }
            PresentationThemeComponents::caption(
                ui,
                "Castling rights use the original rook squares",
            );
            ui.horizontal_wrapped(|ui| {
                for rook in fields.castling_rights() {
                    if ui
                        .add(PresentationThemeEnumButton::Secondary.widget(
                            ui,
                            format!("Revoke {rook} castling right"),
                            PresentationThemeEnumSize::Small,
                        ))
                        .clicked()
                    {
                        *action = Some(Self::edit(
                            context,
                            BoardEditorSessionEnumOperation::Edit(
                                BoardEditorSessionEnumEdit::RevokeCastling(rook),
                            ),
                        ));
                    }
                }
            });
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("editor.castling-rook")
                    .selected_text(self.castling_square.map_or_else(
                        || "Castling rook square".to_owned(),
                        |square| square.to_string(),
                    ))
                    .show_ui(ui, |ui| {
                        for square in PresentationBoardSemanticGrid::squares() {
                            ui.selectable_value(
                                &mut self.castling_square,
                                Some(square),
                                square.to_string(),
                            );
                        }
                    });
                if ui
                    .add_enabled(
                        self.castling_square.is_some(),
                        PresentationThemeEnumButton::Secondary.widget(
                            ui,
                            "Grant Castling Right",
                            PresentationThemeEnumSize::Small,
                        ),
                    )
                    .clicked()
                    && let Some(rook) = self.castling_square
                {
                    *action = Some(Self::edit(
                        context,
                        BoardEditorSessionEnumOperation::Edit(
                            BoardEditorSessionEnumEdit::GrantCastling(rook),
                        ),
                    ));
                }
            });
        });
    }
}
