mod fields;
mod keyboard;
mod palette;

use bevy_egui::egui::{self, Ui};

use crate::board::editor::{
    BoardEditorSession, BoardEditorSessionEnumConfirmation, BoardEditorSessionEnumEdit,
    BoardEditorSessionEnumOperation, BoardEditorSessionEnumSelection, BoardEditorSessionEnumSetup,
    BoardEditorValidationEnumStatus,
};
use crate::board::enums::BoardEnumVariant;
use crate::desktop::editor_commands::{
    DesktopEditorCommandsContext, DesktopEditorCommandsEnumChoice,
    DesktopEditorCommandsEnumOperation, DesktopEditorCommandsEnumWriteStatus,
};
use crate::desktop::error::DesktopFailure;
use crate::presentation::editor::controller::PresentationEditorController;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentationEditorViewEnumAction {
    Command(DesktopEditorCommandsEnumOperation),
    Create,
    ImportFen {
        fen: String,
        variant: BoardEnumVariant,
    },
}

pub struct PresentationEditorView {
    submitted_frame: Option<u64>,
    confirmation: Option<(DesktopEditorCommandsContext, BoardEditorSessionEnumSetup)>,
    fen: String,
    fen_variant: BoardEnumVariant,
    failure: Option<DesktopFailure>,
    castling_square: Option<crate::board::square::BoardSquare>,
}

impl Default for PresentationEditorView {
    fn default() -> Self {
        Self {
            submitted_frame: None,
            confirmation: None,
            fen: String::new(),
            fen_variant: BoardEnumVariant::Standard,
            failure: None,
            castling_square: None,
        }
    }
}

impl PresentationEditorView {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn report_failure(&mut self, failure: DesktopFailure) {
        self.failure = Some(failure);
    }
    pub const fn is_confirming_setup(&self) -> bool {
        self.confirmation.is_some()
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        controller: &PresentationEditorController,
    ) -> Option<PresentationEditorViewEnumAction> {
        let frame = ui.ctx().cumulative_frame_nr();
        let mut action = None;
        ui.heading("Position Editor");
        if let Some(failure) = &self.failure {
            ui.colored_label(ui.visuals().error_fg_color, failure.message());
        }
        let Some(snapshot) = controller.snapshot() else {
            if ui
                .add_enabled(!controller.is_pending(), egui::Button::new("New Draft"))
                .clicked()
            {
                action = Some(PresentationEditorViewEnumAction::Create);
            }
            self.show_fen(ui, !controller.is_pending(), &mut action);
            return self.emit(frame, action);
        };
        let context = snapshot.context();
        if self
            .confirmation
            .is_some_and(|(expected, _)| expected != context)
        {
            self.confirmation = None;
        }
        ui.label(match snapshot.status() {
            BoardEditorValidationEnumStatus::Valid => "Valid position draft",
            BoardEditorValidationEnumStatus::Invalid => "Invalid draft",
        });
        ui.label(if snapshot.is_dirty() {
            "Unsaved changes"
        } else {
            "Saved"
        });
        ui.label(format!(
            "Undo keeps the latest {} edits",
            BoardEditorSession::HISTORY_LIMIT
        ));
        match snapshot.write() {
            DesktopEditorCommandsEnumWriteStatus::Idle => {}
            DesktopEditorCommandsEnumWriteStatus::Saving { .. } => {
                ui.label("Saving submitted draft; newer edits remain unsaved");
            }
            DesktopEditorCommandsEnumWriteStatus::Deleting => {
                ui.label("Deleting saved draft");
            }
            DesktopEditorCommandsEnumWriteStatus::Unresolved { message } => {
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
        }
        if snapshot.prompt().is_some() {
            ui.group(|ui| {
                ui.label("Save this draft before continuing?");
                let can_resolve = !controller.is_pending()
                    && matches!(snapshot.write(), DesktopEditorCommandsEnumWriteStatus::Idle);
                ui.horizontal(|ui| {
                    for (label, choice) in [
                        ("Save", DesktopEditorCommandsEnumChoice::Save),
                        ("Discard", DesktopEditorCommandsEnumChoice::Discard),
                        ("Stay", DesktopEditorCommandsEnumChoice::Stay),
                    ] {
                        if ui
                            .add_enabled(
                                can_resolve
                                    || choice == DesktopEditorCommandsEnumChoice::Stay
                                        && !controller.is_pending(),
                                egui::Button::new(label),
                            )
                            .clicked()
                        {
                            action = Some(PresentationEditorViewEnumAction::Command(
                                DesktopEditorCommandsEnumOperation::Resolve {
                                    expected: context,
                                    choice,
                                },
                            ));
                        }
                    }
                });
            });
            return self.emit(frame, action);
        }
        let editable = controller.can_edit() && self.confirmation.is_none();
        ui.add_enabled_ui(editable, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("New Draft").clicked() {
                    action = Some(PresentationEditorViewEnumAction::Create);
                }
                if ui
                    .add_enabled(
                        matches!(snapshot.write(), DesktopEditorCommandsEnumWriteStatus::Idle),
                        egui::Button::new("Save Draft"),
                    )
                    .clicked()
                {
                    action = Some(PresentationEditorViewEnumAction::Command(
                        DesktopEditorCommandsEnumOperation::Save { expected: context },
                    ));
                }
                if ui.button("Leave Editor").clicked() {
                    action = Some(PresentationEditorViewEnumAction::Command(
                        DesktopEditorCommandsEnumOperation::Leave { expected: context },
                    ));
                }
                if ui
                    .add_enabled(snapshot.has_undo(), egui::Button::new("Undo Editor Edit"))
                    .clicked()
                {
                    action = Some(Self::edit(context, BoardEditorSessionEnumOperation::Undo));
                }
                if ui
                    .add_enabled(snapshot.has_redo(), egui::Button::new("Redo Editor Edit"))
                    .clicked()
                {
                    action = Some(Self::edit(context, BoardEditorSessionEnumOperation::Redo));
                }
            });
            ui.horizontal(|ui| {
                for (label, setup) in [
                    ("Clear Board", BoardEditorSessionEnumSetup::ClearBoard),
                    ("Standard Setup", BoardEditorSessionEnumSetup::StandardSetup),
                ] {
                    if ui.button(label).clicked() {
                        self.confirmation = Some((context, setup));
                    }
                }
            });
        });
        if let Some((expected, setup)) = self.confirmation {
            ui.group(|ui| {
                ui.label("Replace the draft position? This remains an editor Undo step.");
                let label = match setup {
                    BoardEditorSessionEnumSetup::ClearBoard => "Confirm Clear Board",
                    BoardEditorSessionEnumSetup::StandardSetup => "Confirm Standard Setup",
                };
                if ui
                    .add_enabled(!controller.is_pending(), egui::Button::new(label))
                    .clicked()
                {
                    action = Some(Self::edit(
                        expected,
                        BoardEditorSessionEnumOperation::Replace {
                            setup,
                            confirmation: BoardEditorSessionEnumConfirmation::Confirmed,
                        },
                    ));
                    self.confirmation = None;
                }
                if ui.button("Cancel Setup Replacement").clicked() {
                    self.confirmation = None;
                }
            });
        }
        ui.add_enabled_ui(editable, |ui| {
            Self::show_palette(ui, snapshot, &mut action);
            ui.horizontal(|ui| {
                if let Some(square) = controller.focused_square() {
                    ui.label(format!("Focused editor square: {square}"));
                    if ui
                        .add_enabled(
                            snapshot.selection() != BoardEditorSessionEnumSelection::None,
                            egui::Button::new("Place Selected Piece"),
                        )
                        .clicked()
                    {
                        action = Some(Self::edit(
                            context,
                            BoardEditorSessionEnumOperation::PlaceSelected(square),
                        ));
                    }
                    if ui.button("Remove Focused Piece").clicked() {
                        action = Some(Self::edit(
                            context,
                            BoardEditorSessionEnumOperation::Edit(
                                BoardEditorSessionEnumEdit::Remove { square },
                            ),
                        ));
                    }
                } else {
                    ui.label("Focus an editor board square to place or remove a piece");
                }
            });
            self.show_fields(ui, snapshot, &mut action);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Validate Draft").clicked() {
                    action = Some(PresentationEditorViewEnumAction::Command(
                        DesktopEditorCommandsEnumOperation::Validate { expected: context },
                    ));
                }
                let valid = snapshot.status() == BoardEditorValidationEnumStatus::Valid;
                if ui
                    .add_enabled(valid, egui::Button::new("Copy FEN"))
                    .clicked()
                {
                    action = Some(PresentationEditorViewEnumAction::Command(
                        DesktopEditorCommandsEnumOperation::ExportFen { expected: context },
                    ));
                }
                if ui
                    .add_enabled(valid, egui::Button::new("Play from Position"))
                    .clicked()
                {
                    action = Some(PresentationEditorViewEnumAction::Command(
                        DesktopEditorCommandsEnumOperation::Play { expected: context },
                    ));
                }
                if ui
                    .add_enabled(valid, egui::Button::new("Open as Study"))
                    .clicked()
                {
                    action = Some(PresentationEditorViewEnumAction::Command(
                        DesktopEditorCommandsEnumOperation::OpenStudy { expected: context },
                    ));
                }
            });
            self.show_fen(
                ui,
                matches!(snapshot.write(), DesktopEditorCommandsEnumWriteStatus::Idle),
                &mut action,
            );
        });
        for finding in snapshot.findings() {
            ui.colored_label(ui.visuals().error_fg_color, finding.to_string());
        }
        ui.label("Editor keys: K/Q/R/B/N/P choose a piece, C changes color, Enter places, Delete removes, Cmd/Ctrl+Z undoes, Shift+Cmd/Ctrl+Z redoes. Arrow keys focus squares.");
        self.emit(frame, action)
    }

    fn show_fen(
        &mut self,
        ui: &mut Ui,
        enabled: bool,
        action: &mut Option<PresentationEditorViewEnumAction>,
    ) {
        ui.group(|ui| {
            ui.label("FEN to a separate draft");
            ui.add(
                egui::TextEdit::multiline(&mut self.fen)
                    .hint_text("FEN")
                    .desired_rows(2),
            );
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.fen_variant,
                    BoardEnumVariant::Standard,
                    "FEN Standard",
                );
                ui.selectable_value(
                    &mut self.fen_variant,
                    BoardEnumVariant::Chess960,
                    "FEN Chess960",
                );
                if ui
                    .add_enabled(
                        enabled && !self.fen.trim().is_empty(),
                        egui::Button::new("Import FEN"),
                    )
                    .clicked()
                {
                    *action = Some(PresentationEditorViewEnumAction::ImportFen {
                        fen: self.fen.clone(),
                        variant: self.fen_variant,
                    });
                }
            });
        });
    }

    fn edit(
        expected: DesktopEditorCommandsContext,
        operation: BoardEditorSessionEnumOperation,
    ) -> PresentationEditorViewEnumAction {
        PresentationEditorViewEnumAction::Command(DesktopEditorCommandsEnumOperation::Apply {
            expected,
            operation,
        })
    }

    fn emit(
        &mut self,
        frame: u64,
        action: Option<PresentationEditorViewEnumAction>,
    ) -> Option<PresentationEditorViewEnumAction> {
        if action.is_some() && self.submitted_frame != Some(frame) {
            self.submitted_frame = Some(frame);
            self.failure = None;
            action
        } else {
            None
        }
    }
}
