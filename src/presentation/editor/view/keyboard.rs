use bevy_egui::egui::{Key, Modifiers, Ui};
use strum::IntoEnumIterator;

use crate::board::enums::BoardEnumPieceKind;
use crate::desktop::editor_commands::DesktopEditorCommandsEnumOperation;
use crate::presentation::board::enums::PresentationBoardEnumDirection;
use crate::presentation::board::scene::PresentationBoardScene;
use crate::presentation::editor::controller::{
    PresentationEditorController, PresentationEditorControllerEnumKey,
    PresentationEditorControllerError,
};
use crate::presentation::editor::view::PresentationEditorView;

impl PresentationEditorView {
    pub fn keyboard(
        &mut self,
        ui: &Ui,
        controller: &mut PresentationEditorController,
        board_has_focus: bool,
        scene: &PresentationBoardScene,
    ) -> Result<Option<DesktopEditorCommandsEnumOperation>, PresentationEditorControllerError> {
        let frame = ui.ctx().cumulative_frame_nr();
        if self.submitted_frame == Some(frame)
            || !board_has_focus
            || !controller.can_edit()
            || self.confirmation.is_some()
            || ui.ctx().egui_wants_keyboard_input()
        {
            return Ok(None);
        }
        let key = ui.input_mut(|input| {
            if input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) {
                return Some(PresentationEditorControllerEnumKey::Redo);
            }
            if input.consume_key(Modifiers::COMMAND, Key::Z) {
                return Some(PresentationEditorControllerEnumKey::Undo);
            }
            for (key, kind) in [
                (Key::K, BoardEnumPieceKind::King),
                (Key::Q, BoardEnumPieceKind::Queen),
                (Key::R, BoardEnumPieceKind::Rook),
                (Key::B, BoardEnumPieceKind::Bishop),
                (Key::N, BoardEnumPieceKind::Knight),
                (Key::P, BoardEnumPieceKind::Pawn),
            ] {
                if input.consume_key(Modifiers::NONE, key) {
                    return Some(PresentationEditorControllerEnumKey::Piece(kind));
                }
            }
            for (key, action) in [
                (Key::C, PresentationEditorControllerEnumKey::ToggleColor),
                (Key::Delete, PresentationEditorControllerEnumKey::Remove),
                (Key::Backspace, PresentationEditorControllerEnumKey::Remove),
                (Key::Enter, PresentationEditorControllerEnumKey::Place),
            ] {
                if input.consume_key(Modifiers::NONE, key) {
                    return Some(action);
                }
            }
            None
        });
        if let Some(key) = key {
            self.submitted_frame = Some(frame);
            return Ok(Some(controller.key(key)?));
        }
        for direction in PresentationBoardEnumDirection::iter() {
            if ui.input_mut(|input| input.consume_key(Modifiers::NONE, direction.key())) {
                self.submitted_frame = Some(frame);
                controller.focus_direction(direction, scene)?;
                break;
            }
        }
        Ok(None)
    }
}
