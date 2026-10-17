use crate::board::position::BoardPosition;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplicationError, DesktopApplicationPort, DesktopApplicationReceipt,
    DesktopApplicationWakeProvider,
};
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::editor_commands::{
    DesktopEditorCommandsEnumOperation, DesktopEditorCommandsEnumResult,
    DesktopEditorCommandsOpening, DesktopEditorCommandsSnapshot,
};
use crate::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEnumResult, DesktopRequest,
};
use crate::presentation::app::editor::{
    PresentationAppEditor, PresentationAppEditorConnection, PresentationAppEditorEnumIntent,
    PresentationAppEditorEnumRefresh, PresentationAppEditorError,
};
use crate::presentation::app::library::PresentationAppLibraryEnumNavigation;
use crate::presentation::app::{
    PresentationApp, PresentationAppEnumWorkspace, PresentationAppError,
};
use crate::presentation::editor::application_port::PresentationEditorApplicationPort;
use crate::presentation::editor::controller::{
    PresentationEditorCommandPort, PresentationEditorController,
    PresentationEditorControllerEnumEffect,
};
use crate::presentation::game::new_game::PresentationGameNewGame;
use crate::scene::editor::SceneEditorBinding;

struct PresentationAppEditorPreparedPort<
    'a,
    C: ClockTimeProvider,
    W: DesktopApplicationWakeProvider,
> {
    request: DesktopRequest,
    port: PresentationEditorApplicationPort<'a, C, W>,
}

impl<C: ClockTimeProvider, W: DesktopApplicationWakeProvider> PresentationEditorCommandPort
    for PresentationAppEditorPreparedPort<'_, C, W>
{
    fn prepare(
        &self,
        operation: DesktopEditorCommandsEnumOperation,
    ) -> Result<DesktopRequest, DesktopApplicationError> {
        if !matches!(self.request.command().operation(), DesktopEnumCommand::Editor { operation: actual, .. } if actual == &operation)
        {
            return Err(DesktopApplicationError::InvalidTransition);
        }
        operation
            .validate_payload()
            .map_err(|error| DesktopApplicationError::EditorPayload(std::sync::Arc::new(error)))?;
        Ok(self.request.clone())
    }

    fn try_submit(
        &self,
        request: &DesktopRequest,
    ) -> Result<DesktopApplicationReceipt, DesktopApplicationError> {
        self.port.try_submit(request)
    }
}

impl PresentationApp {
    pub fn mount_editor(&mut self, connection: PresentationAppEditorConnection) {
        self.editor = Some(PresentationAppEditor::new(connection));
    }

    pub fn editor_controller(&self) -> Option<&PresentationEditorController> {
        self.editor.as_ref().map(|editor| &editor.controller)
    }

    pub fn editor_snapshot(&self) -> Option<&DesktopEditorCommandsSnapshot> {
        self.editor_controller()?.snapshot()
    }

    pub fn editor_failure(&self) -> Option<&PresentationAppEditorError> {
        self.editor.as_ref()?.failure.as_ref()
    }

    pub fn editor_page(
        &self,
    ) -> Option<&crate::storage::editor_repository::StorageEditorRepositoryPage> {
        Some(self.editor.as_ref()?.page.as_ref()?.page())
    }

    pub fn new_game_configuration(
        &self,
    ) -> Option<
        Result<
            crate::game::configuration::GameConfiguration,
            crate::presentation::game::configuration::PresentationGameConfigurationError,
        >,
    > {
        self.new_game
            .as_ref()
            .map(PresentationGameNewGame::configuration)
    }

    pub fn bind_editor_scene(&mut self, binding: Option<SceneEditorBinding>) {
        if let Some(editor) = &mut self.editor {
            if editor.binding != binding {
                editor.gesture = None;
            }
            editor.binding = binding;
        }
    }

    pub fn retry_editor_action<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        application: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        let editor = self
            .editor
            .as_mut()
            .ok_or(PresentationAppEditorError::Unavailable)?;
        let port = PresentationEditorApplicationPort::new(editor.connection.owner, application);
        editor
            .controller
            .retry(&port)
            .map_err(PresentationAppEditorError::from)?;
        Ok(())
    }

    fn selected_editor_position(
        &self,
    ) -> Result<Option<BoardPosition>, PresentationAppEditorError> {
        let workspace = if self.workspace == PresentationAppEnumWorkspace::Editor {
            self.editor
                .as_ref()
                .map_or(PresentationAppEnumWorkspace::Games, |editor| {
                    editor.source_workspace
                })
        } else {
            self.workspace
        };
        let selected = Self::board_for_workspace(
            workspace,
            Self::selected_game_snapshot(&self.games, self.selected_game),
            self.snapshot.as_ref(),
            self.puzzle_panel.as_ref(),
            self.library.as_ref(),
        );
        selected
            .map(|snapshot| BoardPosition::from_fen(snapshot.fen(), snapshot.variant()))
            .transpose()
            .map_err(Into::into)
    }

    pub fn editor_action<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        intent: PresentationAppEditorEnumIntent,
        application: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        match &intent {
            PresentationAppEditorEnumIntent::Command(operation) => operation
                .validate_payload()
                .map_err(|error| PresentationAppEditorError::Command(std::sync::Arc::new(error)))?,
            PresentationAppEditorEnumIntent::ImportFen { fen, .. } => {
                if fen.capacity() > BoardPosition::MAX_FEN_BYTES {
                    return Err(PresentationAppEditorError::Command(std::sync::Arc::new(crate::desktop::editor_commands::DesktopEditorCommandsError::FenPayloadTooLarge { bytes: fen.capacity(), maximum: BoardPosition::MAX_FEN_BYTES })).into());
                }
            }
            PresentationAppEditorEnumIntent::Create | PresentationAppEditorEnumIntent::Read(_) => {}
        }
        if !matches!(intent, PresentationAppEditorEnumIntent::Command(_))
            && self.workspace != PresentationAppEnumWorkspace::Editor
        {
            let editor = self
                .editor
                .as_ref()
                .ok_or(PresentationAppEditorError::Unavailable)?;
            if editor.controller.is_pending() || editor.queued_opening.is_some() {
                return Err(PresentationAppEditorError::Busy.into());
            }
            self.navigate_library(
                PresentationAppLibraryEnumNavigation::Workspace(
                    PresentationAppEnumWorkspace::Editor,
                ),
                application,
            )?;
            if self.workspace != PresentationAppEnumWorkspace::Editor {
                self.editor
                    .as_mut()
                    .ok_or(PresentationAppEditorError::Unavailable)?
                    .queued_opening = Some(intent);
                return Ok(());
            }
        }
        self.perform_editor_action(intent, application)?;
        Ok(())
    }

    fn perform_editor_action<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        intent: PresentationAppEditorEnumIntent,
        application: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppEditorError> {
        let selected = if matches!(intent, PresentationAppEditorEnumIntent::Create) {
            self.selected_editor_position()?
        } else {
            None
        };
        let editor = self
            .editor
            .as_mut()
            .ok_or(PresentationAppEditorError::Unavailable)?;
        if editor.controller.is_pending()
            || editor.refresh != PresentationAppEditorEnumRefresh::Ready
        {
            return Err(PresentationAppEditorError::Busy);
        }
        if editor.view.is_confirming_setup()
            && !matches!(
                intent,
                PresentationAppEditorEnumIntent::Command(
                    DesktopEditorCommandsEnumOperation::Snapshot
                )
            )
        {
            return Err(PresentationAppEditorError::Busy);
        }
        let expected = editor
            .controller
            .snapshot()
            .map(DesktopEditorCommandsSnapshot::context);
        let port = PresentationEditorApplicationPort::new(editor.connection.owner, application);
        match intent {
            PresentationAppEditorEnumIntent::Command(operation) => {
                if matches!(
                    operation,
                    DesktopEditorCommandsEnumOperation::Apply { .. }
                        | DesktopEditorCommandsEnumOperation::Validate { .. }
                        | DesktopEditorCommandsEnumOperation::Play { .. }
                        | DesktopEditorCommandsEnumOperation::ExportFen { .. }
                ) && !editor.can_input()
                {
                    return Err(PresentationAppEditorError::Busy);
                }
                editor.submit(operation, &port)?;
                Ok(())
            }
            opening @ (PresentationAppEditorEnumIntent::Create
            | PresentationAppEditorEnumIntent::Read(_)
            | PresentationAppEditorEnumIntent::ImportFen { .. }) => {
                let ticket = application.prepare_action()?;
                let draft = match opening {
                    PresentationAppEditorEnumIntent::Read(id) => id,
                    PresentationAppEditorEnumIntent::Create
                    | PresentationAppEditorEnumIntent::ImportFen { .. } => {
                        editor.connection.drafts.next_id()?
                    }
                    PresentationAppEditorEnumIntent::Command(_) => {
                        return Err(PresentationAppEditorError::UnexpectedResult);
                    }
                };
                let context = DesktopEditorCommandsOpening::new(draft, ticket.request_id());
                let operation = match opening {
                    PresentationAppEditorEnumIntent::Create => {
                        DesktopEditorCommandsEnumOperation::Create {
                            opening: context,
                            selected,
                            expected,
                        }
                    }
                    PresentationAppEditorEnumIntent::Read(_) => {
                        DesktopEditorCommandsEnumOperation::Read {
                            opening: context,
                            expected,
                        }
                    }
                    PresentationAppEditorEnumIntent::ImportFen { fen, variant } => {
                        DesktopEditorCommandsEnumOperation::ImportFen {
                            opening: context,
                            fen,
                            variant,
                            expected,
                        }
                    }
                    PresentationAppEditorEnumIntent::Command(_) => {
                        return Err(PresentationAppEditorError::UnexpectedResult);
                    }
                };
                operation.validate_payload().map_err(|error| {
                    PresentationAppEditorError::Command(std::sync::Arc::new(error))
                })?;
                let request = ticket.bind(DesktopCommand::editor(
                    editor.connection.owner,
                    operation.clone(),
                ))?;
                let prepared = PresentationAppEditorPreparedPort { request, port };
                editor.submit(operation, &prepared)?;
                self.workspace = PresentationAppEnumWorkspace::Editor;
                self.new_game = None;
                self.settings_open = false;
                Ok(())
            }
        }
    }

    pub fn synchronize_editor<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        application: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationAppError> {
        let opening = if self.workspace == PresentationAppEnumWorkspace::Editor {
            self.editor
                .as_mut()
                .and_then(|editor| editor.queued_opening.take())
        } else {
            None
        };
        if let Some(opening) = opening {
            self.perform_editor_action(opening, application)?;
        }
        if self.workspace != PresentationAppEnumWorkspace::Editor
            && self
                .library
                .as_ref()
                .is_none_or(|library| !library.has_guard())
            && let Some(editor) = &mut self.editor
        {
            editor.queued_opening = None;
        }
        let Some(editor) = &mut self.editor else {
            return Ok(());
        };
        let port = PresentationEditorApplicationPort::new(editor.connection.owner, application);
        if editor.leave_requested && !editor.controller.is_pending() {
            editor.leave_requested = false;
            if let Some(snapshot) = editor.controller.snapshot() {
                editor.submit(
                    DesktopEditorCommandsEnumOperation::Leave {
                        expected: snapshot.context(),
                    },
                    &port,
                )?;
            }
        }
        let mut effect = PresentationEditorControllerEnumEffect::None;
        if let Some(receipt) = editor.controller.receipt().cloned()
            && let Some(response) = receipt.response()
        {
            let operation = editor
                .controller
                .pending_request()
                .ok_or(PresentationAppEditorError::UnexpectedResult)?
                .command()
                .operation();
            let staying = matches!(
                operation,
                DesktopEnumCommand::Editor {
                    operation: DesktopEditorCommandsEnumOperation::Resolve {
                        choice:
                            crate::desktop::editor_commands::DesktopEditorCommandsEnumChoice::Stay,
                        ..
                    },
                    ..
                }
            );
            let refreshing = matches!(
                operation,
                DesktopEnumCommand::Editor {
                    operation: DesktopEditorCommandsEnumOperation::Snapshot,
                    ..
                }
            );
            match response {
                DesktopMessagesEnumResult::Value {
                    value: DesktopEnumValue::Editor(result),
                    ..
                } => {
                    let written = matches!(
                        result.as_ref(),
                        DesktopEditorCommandsEnumResult::Written { .. }
                    );
                    effect = editor
                        .controller
                        .accept(receipt.request_id())
                        .map_err(PresentationAppEditorError::from)?;
                    if refreshing {
                        editor.refresh = PresentationAppEditorEnumRefresh::Ready;
                    }
                    if written {
                        editor.list_requested = true;
                    }
                    if staying {
                        editor.navigation = None;
                    }
                }
                DesktopMessagesEnumResult::EditorFailure(error) => editor.observe_failure(
                    receipt.request_id(),
                    PresentationAppEditorError::Command(std::sync::Arc::clone(error)),
                    refreshing,
                )?,
                DesktopMessagesEnumResult::Failure(error) => editor.observe_failure(
                    receipt.request_id(),
                    PresentationAppEditorError::Failure(error.clone()),
                    refreshing,
                )?,
                DesktopMessagesEnumResult::Cancelled => editor.observe_failure(
                    receipt.request_id(),
                    PresentationAppEditorError::Canceled,
                    refreshing,
                )?,
                DesktopMessagesEnumResult::Value { .. }
                | DesktopMessagesEnumResult::LibraryFailure(_)
                | DesktopMessagesEnumResult::Stopped
                | DesktopMessagesEnumResult::Paused
                | DesktopMessagesEnumResult::QuitReady
                | DesktopMessagesEnumResult::ReconciliationRequired => {
                    return Err(PresentationAppEditorError::UnexpectedResult.into());
                }
            }
        }
        match effect {
            PresentationEditorControllerEnumEffect::None => {}
            PresentationEditorControllerEnumEffect::List(page) => editor.page = Some(page),
            PresentationEditorControllerEnumEffect::CopyFen { fen, .. } => {
                editor.copied_fen = Some(fen)
            }
            PresentationEditorControllerEnumEffect::NewGame { start, .. } => {
                self.new_game = Some(PresentationGameNewGame::from_position(
                    start.position().clone(),
                ));
                self.workspace = PresentationAppEnumWorkspace::Games;
                self.settings_open = false;
            }
        }
        if !editor.controller.is_pending() {
            match editor.refresh {
                PresentationAppEditorEnumRefresh::Required => {
                    editor.submit(DesktopEditorCommandsEnumOperation::Snapshot, &port)?;
                }
                PresentationAppEditorEnumRefresh::Blocked => {}
                PresentationAppEditorEnumRefresh::Ready => {
                    if editor.list_requested {
                        editor.list_requested = false;
                        editor.submit(
                            DesktopEditorCommandsEnumOperation::List { after: None },
                            &port,
                        )?;
                    }
                }
            }
        }
        let navigation =
            if editor.controller.snapshot().is_none() && !editor.controller.is_pending() {
                editor.navigation.take()
            } else {
                None
            };
        if let Some(target) = navigation {
            self.navigate_library(target, application)?;
        }
        Ok(())
    }

    pub(in crate::presentation::app) fn guard_editor_navigation(
        &mut self,
        target: &PresentationAppLibraryEnumNavigation,
    ) -> Result<bool, PresentationAppEditorError> {
        if matches!(
            target,
            PresentationAppLibraryEnumNavigation::Workspace(PresentationAppEnumWorkspace::Editor)
        ) {
            return Ok(false);
        }
        let Some(editor) = &mut self.editor else {
            return Ok(false);
        };
        if editor.controller.is_pending()
            || editor.navigation.is_some()
            || editor.refresh != PresentationAppEditorEnumRefresh::Ready
            || editor.queued_opening.is_some()
            || editor.view.is_confirming_setup()
        {
            return Err(PresentationAppEditorError::Busy);
        }
        if editor.controller.snapshot().is_none() {
            return Ok(false);
        }
        editor.navigation = Some(target.clone());
        editor.leave_requested = true;
        editor.gesture = None;
        self.workspace = PresentationAppEnumWorkspace::Editor;
        self.settings_open = false;
        Ok(true)
    }
}

impl PresentationAppEditor {
    fn observe_failure(
        &mut self,
        request: crate::desktop::identity::DesktopRequestId,
        failure: PresentationAppEditorError,
        refreshing: bool,
    ) -> Result<(), PresentationAppEditorError> {
        self.controller.acknowledge_failure(request)?;
        self.failure = Some(failure);
        self.refresh = if refreshing {
            PresentationAppEditorEnumRefresh::Blocked
        } else {
            PresentationAppEditorEnumRefresh::Required
        };
        self.navigation = None;
        Ok(())
    }

    pub(super) fn submit<P: PresentationEditorCommandPort>(
        &mut self,
        operation: DesktopEditorCommandsEnumOperation,
        port: &P,
    ) -> Result<(), PresentationAppEditorError> {
        match self.controller.submit(operation, port) {
            Ok(()) => Ok(()),
            Err(crate::presentation::editor::controller::PresentationEditorControllerError::Application(DesktopApplicationError::Busy | DesktopApplicationError::EntityBusy)) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
