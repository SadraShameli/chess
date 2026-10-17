mod board;
mod requests;
mod view;

pub(super) use board::PresentationAppEditorBoardInput;

use std::sync::Arc;

use thiserror::Error;

use crate::board::editor::{BoardEditorDraftError, BoardEditorDraftId};
use crate::board::enums::BoardEnumVariant;
use crate::board::position::BoardPositionError;
use crate::desktop::application::DesktopApplicationError;
use crate::desktop::editor_commands::{
    DesktopEditorCommandsEnumOperation, DesktopEditorCommandsError, DesktopEditorCommandsPage,
};
use crate::desktop::error::DesktopFailure;
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::presentation::app::library::PresentationAppLibraryEnumNavigation;
use crate::presentation::board::view::PresentationBoardViewError;
use crate::presentation::editor::controller::{
    PresentationEditorController, PresentationEditorControllerError,
};
use crate::presentation::editor::view::PresentationEditorView;
use crate::presentation::library::positions::PresentationLibraryPositions;
use crate::scene::editor::SceneEditorBinding;

pub trait PresentationAppEditorDraftIdProvider: Send + Sync {
    fn next_id(&self) -> Result<BoardEditorDraftId, BoardEditorDraftError>;
}

pub struct PresentationAppEditorConnection {
    owner: DesktopMessagesEntityId,
    drafts: Arc<dyn PresentationAppEditorDraftIdProvider>,
}

impl PresentationAppEditorConnection {
    pub fn new(
        owner: DesktopMessagesEntityId,
        drafts: Arc<dyn PresentationAppEditorDraftIdProvider>,
    ) -> Self {
        Self { owner, drafts }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentationAppEditorEnumIntent {
    Create,
    Read(BoardEditorDraftId),
    ImportFen {
        fen: String,
        variant: BoardEnumVariant,
    },
    Command(DesktopEditorCommandsEnumOperation),
}

#[derive(Debug, Error)]
pub enum PresentationAppEditorError {
    #[error("The position editor is unavailable")]
    Unavailable,
    #[error("Finish or retry the current editor action before continuing")]
    Busy,
    #[error("The editor returned a response for another action")]
    UnexpectedResult,
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
    #[error(transparent)]
    Controller(#[from] PresentationEditorControllerError),
    #[error(transparent)]
    DraftId(#[from] BoardEditorDraftError),
    #[error(transparent)]
    Position(#[from] BoardPositionError),
    #[error(transparent)]
    Board(#[from] PresentationBoardViewError),
    #[error(transparent)]
    Command(#[from] Arc<DesktopEditorCommandsError>),
    #[error(transparent)]
    Failure(#[from] DesktopFailure),
    #[error("The editor action was canceled; refresh before continuing")]
    Canceled,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PresentationAppEditorEnumRefresh {
    Ready,
    Required,
    Blocked,
}

pub(super) struct PresentationAppEditor {
    connection: PresentationAppEditorConnection,
    pub(super) controller: PresentationEditorController,
    view: PresentationEditorView,
    positions: PresentationLibraryPositions,
    page: Option<DesktopEditorCommandsPage>,
    failure: Option<PresentationAppEditorError>,
    navigation: Option<PresentationAppLibraryEnumNavigation>,
    leave_requested: bool,
    refresh: PresentationAppEditorEnumRefresh,
    list_requested: bool,
    binding: Option<SceneEditorBinding>,
    gesture: Option<board::PresentationAppEditorBoardGesture>,
    input_frame: Option<u64>,
    pub(super) positions_visible: bool,
    copied_fen: Option<String>,
    pub(super) source_workspace: crate::presentation::app::PresentationAppEnumWorkspace,
    palette_binding: Option<SceneEditorBinding>,
    queued_opening: Option<PresentationAppEditorEnumIntent>,
}

impl PresentationAppEditor {
    pub(super) fn new(connection: PresentationAppEditorConnection) -> Self {
        Self {
            connection,
            controller: PresentationEditorController::new(None),
            view: PresentationEditorView::new(),
            positions: PresentationLibraryPositions::new(),
            page: None,
            failure: None,
            navigation: None,
            leave_requested: false,
            refresh: PresentationAppEditorEnumRefresh::Ready,
            list_requested: false,
            binding: None,
            gesture: None,
            input_frame: None,
            positions_visible: false,
            copied_fen: None,
            source_workspace: crate::presentation::app::PresentationAppEnumWorkspace::Games,
            palette_binding: None,
            queued_opening: None,
        }
    }

    fn can_input(&self) -> bool {
        self.controller.can_edit()
            && !self.view.is_confirming_setup()
            && self.refresh == PresentationAppEditorEnumRefresh::Ready
            && self.navigation.is_none()
    }

    pub(super) fn has_pending_work(&self) -> bool {
        self.controller.receipt().is_some()
            || self.leave_requested
            || self.refresh == PresentationAppEditorEnumRefresh::Required
            || self.list_requested && self.refresh == PresentationAppEditorEnumRefresh::Ready
    }
}
