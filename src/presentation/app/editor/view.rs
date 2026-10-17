use bevy_egui::egui::Ui;

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{DesktopApplicationPort, DesktopApplicationWakeProvider};
use crate::desktop::editor_commands::DesktopEditorCommandsEnumOperation;
use crate::presentation::app::editor::{
    PresentationAppEditorEnumIntent, PresentationAppEditorEnumRefresh, PresentationAppEditorError,
};
use crate::presentation::app::{PresentationApp, PresentationAppError};
use crate::presentation::editor::application_port::PresentationEditorApplicationPort;
use crate::presentation::editor::draft_list::PresentationEditorDraftListEnumAction;
use crate::presentation::editor::view::PresentationEditorViewEnumAction;

impl PresentationApp {
    pub(in crate::presentation::app) fn show_editor<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
    >(
        &mut self,
        ui: &mut Ui,
        application: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        let Some(editor) = &mut self.editor else {
            return Ok(());
        };
        if let Some(fen) = editor.copied_fen.take() {
            ui.ctx().copy_text(fen);
        }
        if let Some(error) = &editor.failure {
            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
        }
        if editor.controller.pending_request().is_some() && editor.controller.receipt().is_none() {
            ui.label("Editor action is waiting for capacity");
            if ui.button("Retry Editor Action").clicked() && !ui.is_sizing_pass() {
                let port =
                    PresentationEditorApplicationPort::new(editor.connection.owner, application);
                match editor.controller.retry(&port) {
                    Ok(()) => {}
                    Err(error) => editor.failure = Some(error.into()),
                }
            }
        }
        if ui.button("Refresh Editor Snapshot").clicked()
            && !editor.controller.is_pending()
            && !ui.is_sizing_pass()
        {
            editor.refresh = PresentationAppEditorEnumRefresh::Required;
        }
        let retry_write = editor.controller.snapshot().filter(|snapshot| matches!(snapshot.write(), crate::desktop::editor_commands::DesktopEditorCommandsEnumWriteStatus::Unresolved { .. })).map(|snapshot| snapshot.context());
        if let Some(expected) = retry_write
            && ui
                .add_enabled(
                    !editor.controller.is_pending()
                        && editor.refresh == PresentationAppEditorEnumRefresh::Ready
                        && !editor.view.is_confirming_setup(),
                    bevy_egui::egui::Button::new("Retry Retained Editor Write"),
                )
                .clicked()
            && !ui.is_sizing_pass()
        {
            let port = PresentationEditorApplicationPort::new(editor.connection.owner, application);
            editor.submit(DesktopEditorCommandsEnumOperation::Save { expected }, &port)?;
        }
        let action = ui
            .add_enabled_ui(
                editor.refresh == PresentationAppEditorEnumRefresh::Ready,
                |ui| editor.view.show(ui, &editor.controller),
            )
            .inner;
        if let Some(action) = action.filter(|_| !ui.is_sizing_pass()) {
            let intent = match action {
                PresentationEditorViewEnumAction::Create => PresentationAppEditorEnumIntent::Create,
                PresentationEditorViewEnumAction::ImportFen { fen, variant } => {
                    PresentationAppEditorEnumIntent::ImportFen { fen, variant }
                }
                PresentationEditorViewEnumAction::Command(operation) => {
                    PresentationAppEditorEnumIntent::Command(operation)
                }
            };
            self.editor_action(intent, application)?;
        }
        self.show_positions(ui, application)
    }

    pub(in crate::presentation::app) fn show_positions<
        C: ClockTimeProvider,
        W: DesktopApplicationWakeProvider,
    >(
        &mut self,
        ui: &mut Ui,
        application: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        let Some(editor) = &mut self.editor else {
            return Ok(());
        };
        if editor.page.is_none()
            && !editor.controller.is_pending()
            && editor.refresh == PresentationAppEditorEnumRefresh::Ready
        {
            editor.list_requested = true;
        }
        let Some(page) = &editor.page else {
            ui.label("Loading saved position drafts");
            return Ok(());
        };
        let expected = editor
            .controller
            .snapshot()
            .map(|snapshot| snapshot.context());
        let enabled = !editor.controller.is_pending()
            && editor.refresh == PresentationAppEditorEnumRefresh::Ready
            && !editor.view.is_confirming_setup()
            && editor
                .controller
                .snapshot()
                .is_none_or(|_| editor.can_input());
        let action = ui
            .add_enabled_ui(enabled, |ui| {
                editor.positions.show(ui, page.page(), expected)
            })
            .inner;
        if let Some(action) = action.filter(|_| !ui.is_sizing_pass()) {
            let intent = match action {
                PresentationEditorDraftListEnumAction::Open(id) => {
                    PresentationAppEditorEnumIntent::Read(id)
                }
                PresentationEditorDraftListEnumAction::Delete {
                    id,
                    storage_revision,
                    expected,
                } => PresentationAppEditorEnumIntent::Command(
                    DesktopEditorCommandsEnumOperation::Delete {
                        id,
                        storage_revision,
                        expected,
                    },
                ),
                PresentationEditorDraftListEnumAction::List { after } => {
                    PresentationAppEditorEnumIntent::Command(
                        DesktopEditorCommandsEnumOperation::List { after },
                    )
                }
            };
            self.editor_action(intent, application)?;
        }
        Ok(())
    }

    pub fn refresh_position_drafts<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        application: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        if self.editor.is_none() {
            return Err(PresentationAppEditorError::Unavailable.into());
        }
        self.editor_action(
            PresentationAppEditorEnumIntent::Command(DesktopEditorCommandsEnumOperation::List {
                after: None,
            }),
            application,
        )?;
        Ok(())
    }
}
