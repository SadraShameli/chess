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
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
};

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
        PresentationThemeComponents::eyebrow(ui, "Edit position");
        ui.heading("Position Editor");
        if let Some(failure) = &self.failure {
            ui.colored_label(ui.visuals().error_fg_color, failure.message());
        }
        let Some(snapshot) = controller.snapshot() else {
            if ui
                .add_enabled(
                    !controller.is_pending(),
                    PresentationThemeEnumButton::Primary.widget(
                        ui,
                        "New Draft",
                        PresentationThemeEnumSize::Small,
                    ),
                )
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
            PresentationThemeComponents::card(ui).show(ui, |ui| {
                PresentationThemeComponents::subhead(ui, "Save this draft before continuing?");
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
                                PresentationThemeEnumButton::Secondary.widget(
                                    ui,
                                    label,
                                    PresentationThemeEnumSize::Small,
                                ),
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
                if PresentationThemeEnumButton::Secondary
                    .show(ui, "New Draft")
                    .clicked()
                {
                    action = Some(PresentationEditorViewEnumAction::Create);
                }
                if ui
                    .add_enabled(
                        matches!(snapshot.write(), DesktopEditorCommandsEnumWriteStatus::Idle),
                        PresentationThemeEnumButton::Primary.widget(
                            ui,
                            "Save Draft",
                            PresentationThemeEnumSize::Small,
                        ),
                    )
                    .clicked()
                {
                    action = Some(PresentationEditorViewEnumAction::Command(
                        DesktopEditorCommandsEnumOperation::Save { expected: context },
                    ));
                }
                if PresentationThemeEnumButton::Secondary
                    .show(ui, "Leave Editor")
                    .clicked()
                {
                    action = Some(PresentationEditorViewEnumAction::Command(
                        DesktopEditorCommandsEnumOperation::Leave { expected: context },
                    ));
                }
                if ui
                    .add_enabled(
                        snapshot.has_undo(),
                        PresentationThemeEnumButton::Secondary.widget(
                            ui,
                            "Undo Editor Edit",
                            PresentationThemeEnumSize::Small,
                        ),
                    )
                    .clicked()
                {
                    action = Some(Self::edit(context, BoardEditorSessionEnumOperation::Undo));
                }
                if ui
                    .add_enabled(
                        snapshot.has_redo(),
                        PresentationThemeEnumButton::Secondary.widget(
                            ui,
                            "Redo Editor Edit",
                            PresentationThemeEnumSize::Small,
                        ),
                    )
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
                    if ui
                        .add(PresentationThemeEnumButton::Secondary.widget(
                            ui,
                            label,
                            PresentationThemeEnumSize::Small,
                        ))
                        .clicked()
                    {
                        self.confirmation = Some((context, setup));
                    }
                }
            });
        });
        if let Some((expected, setup)) = self.confirmation {
            PresentationThemeComponents::card(ui).show(ui, |ui| {
                ui.label("Replace the draft position? This remains an editor Undo step.");
                let label = match setup {
                    BoardEditorSessionEnumSetup::ClearBoard => "Confirm Clear Board",
                    BoardEditorSessionEnumSetup::StandardSetup => "Confirm Standard Setup",
                };
                if ui
                    .add_enabled(
                        !controller.is_pending(),
                        PresentationThemeEnumButton::Secondary.widget(
                            ui,
                            label,
                            PresentationThemeEnumSize::Small,
                        ),
                    )
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
                if PresentationThemeEnumButton::Ghost
                    .show(ui, "Cancel Setup Replacement")
                    .clicked()
                {
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
                            PresentationThemeEnumButton::Secondary.widget(
                                ui,
                                "Place Selected Piece",
                                PresentationThemeEnumSize::Small,
                            ),
                        )
                        .clicked()
                    {
                        action = Some(Self::edit(
                            context,
                            BoardEditorSessionEnumOperation::PlaceSelected(square),
                        ));
                    }
                    if PresentationThemeEnumButton::Danger
                        .show(ui, "Remove Focused Piece")
                        .clicked()
                    {
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
                if PresentationThemeEnumButton::Secondary
                    .show(ui, "Validate Draft")
                    .clicked()
                {
                    action = Some(PresentationEditorViewEnumAction::Command(
                        DesktopEditorCommandsEnumOperation::Validate { expected: context },
                    ));
                }
                let valid = snapshot.status() == BoardEditorValidationEnumStatus::Valid;
                if ui
                    .add_enabled(
                        valid,
                        PresentationThemeEnumButton::Secondary.widget(
                            ui,
                            "Copy FEN",
                            PresentationThemeEnumSize::Small,
                        ),
                    )
                    .clicked()
                {
                    action = Some(PresentationEditorViewEnumAction::Command(
                        DesktopEditorCommandsEnumOperation::ExportFen { expected: context },
                    ));
                }
                if ui
                    .add_enabled(
                        valid,
                        PresentationThemeEnumButton::Primary.widget(
                            ui,
                            "Play from Position",
                            PresentationThemeEnumSize::Small,
                        ),
                    )
                    .clicked()
                {
                    action = Some(PresentationEditorViewEnumAction::Command(
                        DesktopEditorCommandsEnumOperation::Play { expected: context },
                    ));
                }
                if ui
                    .add_enabled(
                        valid,
                        PresentationThemeEnumButton::Primary.widget(
                            ui,
                            "Open as Study",
                            PresentationThemeEnumSize::Small,
                        ),
                    )
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
        PresentationThemeComponents::caption(
            ui,
            "Editor keys: K/Q/R/B/N/P choose a piece, C changes color, Enter places, Delete removes, Cmd/Ctrl+Z undoes, Shift+Cmd/Ctrl+Z redoes. Arrow keys focus squares.",
        );
        self.emit(frame, action)
    }

    fn show_fen(
        &mut self,
        ui: &mut Ui,
        enabled: bool,
        action: &mut Option<PresentationEditorViewEnumAction>,
    ) {
        PresentationThemeComponents::card(ui).show(ui, |ui| {
            PresentationThemeComponents::subhead(ui, "FEN to a separate draft");
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
                        PresentationThemeEnumButton::Primary.widget(
                            ui,
                            "Import FEN",
                            PresentationThemeEnumSize::Small,
                        ),
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
