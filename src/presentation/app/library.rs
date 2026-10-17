mod editor;
mod requests;
mod view;

use std::sync::Arc;

use thiserror::Error;

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplicationError, DesktopApplicationPort, DesktopApplicationReceipt,
    DesktopApplicationWakeProvider,
};
use crate::desktop::game_editor::{DesktopGameEditorContext, DesktopGameEditorPublication};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::{DesktopLibraryServiceError, DesktopLibraryServiceOperation};
use crate::desktop::messages::{DesktopMessagesEntityId, DesktopRequest};
use crate::game::enums::GameId;
use crate::presentation::analysis::annotations::{
    PresentationAnalysisAnnotations, PresentationAnalysisAnnotationsEnumDestination,
    PresentationAnalysisAnnotationsEnumEffect,
};
use crate::presentation::analysis::editor_intent::{
    PresentationAnalysisEditorError, PresentationAnalysisEditorIntent,
    PresentationAnalysisEditorModal,
};
use crate::presentation::analysis::variation_tree::PresentationAnalysisVariationTree;
use crate::presentation::app::PresentationAppEnumWorkspace;
use crate::presentation::library::error::PresentationLibraryError;
use crate::presentation::library::intent::PresentationLibraryIntent;
use crate::presentation::library::controller::PresentationLibraryController;
use crate::presentation::library::view::{
    PresentationLibraryEnumFileRequest, PresentationLibraryPanel,
};

#[derive(Debug, Error)]
pub enum PresentationAppLibraryError {
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
    #[error(transparent)]
    Panel(#[from] PresentationLibraryError),
    #[error(transparent)]
    Editor(#[from] PresentationAnalysisEditorError),
    #[error("The Library returned a result for another operation")]
    UnexpectedResult,
    #[error("Finish the current Library action before leaving")]
    Busy,
    #[error("The live game changed. Your draft and earlier board are retained")]
    SourceChanged,
    #[error(transparent)]
    Identity(#[from] crate::game::enums::GameIdentityError),
}

#[derive(Debug, Clone)]
pub(super) enum PresentationAppLibraryEnumNavigation {
    OpenSaved(Box<PresentationLibraryIntent>),
    Workspace(PresentationAppEnumWorkspace),
    NewGame,
    Game(GameId),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PresentationAppLibraryEnumEditor {
    Notes,
    Tree,
    Navigate,
}

enum PresentationAppLibraryEnumOwner {
    Panel(PresentationLibraryIntent),
    Editor {
        owner: PresentationAppLibraryEnumEditor,
        intent: PresentationAnalysisEditorIntent,
    },
}

pub(super) struct PresentationAppLibraryPending {
    owner: PresentationAppLibraryEnumOwner,
    operation: DesktopLibraryServiceOperation,
    request: DesktopRequest,
    receipt: Option<DesktopApplicationReceipt>,
    observed: bool,
    failure: Option<Arc<DesktopLibraryServiceError>>,
}

pub(super) struct PresentationAppLibraryFileSelection {
    request: DesktopRequest,
    receipt: Option<DesktopApplicationReceipt>,
    file: PresentationLibraryEnumFileRequest,
}

pub(super) struct PresentationAppLibraryNavigation {
    token: DesktopRequestId,
    expected: DesktopGameEditorContext,
    target: PresentationAppLibraryEnumNavigation,
    released: bool,
}

pub(super) struct PresentationAppLibrary {
    workspace: DesktopMessagesEntityId,
    pub(super) panel: PresentationLibraryPanel,
    pub(super) annotations: PresentationAnalysisAnnotations,
    pub(super) tree: PresentationAnalysisVariationTree,
    pub(super) publication: Option<DesktopGameEditorPublication>,
    pub(super) pending: Vec<PresentationAppLibraryPending>,
    pub(super) navigation: Option<PresentationAppLibraryNavigation>,
    source_changed: bool,
    notes: String,
    selected_move: Option<crate::board::r#move::BoardMove>,
    frame: Option<u64>,
    input_frame: Option<u64>,
    failure: Option<String>,
    service_failure: Option<Arc<DesktopLibraryServiceError>>,
    modal: PresentationAnalysisEditorModal,
    guard_visible: bool,
    file_selection: Option<PresentationAppLibraryFileSelection>,
}

impl PresentationAppLibrary {
    const MAX_PANEL_LANES: usize = PresentationLibraryController::LANES;
    const MAX_EDITOR_ACTIONS: usize = 1;

    pub(super) fn new(workspace: DesktopMessagesEntityId) -> Self {
        Self {
            workspace,
            panel: PresentationLibraryPanel::new(workspace),
            annotations: PresentationAnalysisAnnotations::new(),
            tree: PresentationAnalysisVariationTree::new(),
            publication: None,
            pending: Vec::with_capacity(Self::MAX_PANEL_LANES + Self::MAX_EDITOR_ACTIONS),
            navigation: None,
            source_changed: false,
            notes: String::new(),
            selected_move: None,
            frame: None,
            input_frame: None,
            failure: None,
            service_failure: None,
            modal: PresentationAnalysisEditorModal::default(),
            guard_visible: false,
            file_selection: None,
        }
    }

    pub(super) fn board(&self) -> Option<&crate::desktop::dto::DesktopBoardSnapshot> {
        self.publication
            .as_ref()
            .map(DesktopGameEditorPublication::board)
    }

    pub(super) fn has_pending_work(&self) -> bool {
        self.file_selection.is_some()
            || self.pending.iter().any(|pending| !pending.observed)
            || self
                .navigation
                .as_ref()
                .is_some_and(|navigation| navigation.released)
    }

    fn editor_busy(&self) -> bool {
        self.pending.iter().any(|pending| {
            matches!(
                pending.owner,
                PresentationAppLibraryEnumOwner::Editor { .. }
            )
        })
    }

    pub(super) const fn has_guard(&self) -> bool {
        self.annotations.has_guard()
    }

    fn opening_busy(&self) -> bool {
        self.pending.iter().any(|pending| matches!(&pending.owner, PresentationAppLibraryEnumOwner::Panel(intent) if intent.is_open_action()))
    }

    pub(super) fn observe_unsaved_games(&mut self, games: Vec<GameId>) {
        self.panel.controller_mut().observe_unsaved_games(games);
    }

    pub(super) fn observe_live_source(
        &mut self,
        snapshot: &crate::desktop::game_snapshot::DesktopGameSnapshot,
    ) {
        if let Some(publication) = &self.publication
            && publication.context().game() == snapshot.id()
            && let crate::desktop::game_editor::DesktopGameEditorEnumSource::Played { revision } =
                publication.context().source()
            && snapshot.revision() > revision
        {
            self.source_changed = true;
        }
    }

    pub(super) fn request_navigation(
        &mut self,
        target: PresentationAppLibraryEnumNavigation,
    ) -> Result<Option<PresentationAppLibraryEnumNavigation>, PresentationAppLibraryError> {
        if self.navigation.is_some() || self.editor_busy() || self.opening_busy() {
            if let PresentationAppLibraryEnumNavigation::OpenSaved(intent) = &target {
                self.panel.controller_mut().abandon_intent(intent)?;
            }
            return Err(PresentationAppLibraryError::Busy);
        }
        let Some(publication) = &self.publication else {
            return Ok(Some(target));
        };
        let expected = publication.context();
        let token = DesktopRequestId::new();
        let effect = match self.annotations.request_switch(
            token,
            PresentationAnalysisAnnotationsEnumDestination::Leave(token),
        ) {
            Ok(effect) => effect,
            Err(error) => {
                if let PresentationAppLibraryEnumNavigation::OpenSaved(intent) = &target {
                    self.panel.controller_mut().abandon_intent(intent)?;
                }
                return Err(error.into());
            }
        };
        self.navigation = Some(PresentationAppLibraryNavigation {
            token,
            expected,
            target,
            released: false,
        });
        if let Some(effect) = effect {
            self.release_navigation(effect)?;
        }
        Ok(None)
    }

    fn release_navigation(
        &mut self,
        effect: PresentationAnalysisAnnotationsEnumEffect,
    ) -> Result<(), PresentationAppLibraryError> {
        let PresentationAnalysisAnnotationsEnumEffect::Leave {
            request_id,
            expected,
            destination,
        } = effect
        else {
            return Err(PresentationAppLibraryError::UnexpectedResult);
        };
        let navigation = self
            .navigation
            .as_mut()
            .ok_or(PresentationAppLibraryError::UnexpectedResult)?;
        if request_id != navigation.token || destination != navigation.token || self.annotations.context() != Some(expected) || !crate::presentation::analysis::editor_intent::PresentationAnalysisEditorServiceCompletion::same_owner(navigation.expected, expected) {
            return Err(PresentationAppLibraryError::UnexpectedResult);
        }
        navigation.released = true;
        Ok(())
    }

    fn abandon_navigation(&mut self) -> Result<(), PresentationAppLibraryError> {
        if let Some(navigation) = self.navigation.take()
            && let PresentationAppLibraryEnumNavigation::OpenSaved(intent) = navigation.target
        {
            self.panel.controller_mut().abandon_intent(&intent)?;
        }
        Ok(())
    }

    fn take_navigation(&mut self) -> Option<PresentationAppLibraryEnumNavigation> {
        if self
            .navigation
            .as_ref()
            .is_some_and(|navigation| navigation.released)
        {
            self.navigation.take().map(|navigation| navigation.target)
        } else {
            None
        }
    }

    pub(super) fn synchronize<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<Option<PresentationAppLibraryEnumNavigation>, PresentationAppLibraryError> {
        self.admit(port)?;
        self.poll(port)?;
        if self
            .navigation
            .as_ref()
            .is_some_and(|navigation| !navigation.released)
            && !self.annotations.has_guard()
        {
            self.abandon_navigation()?;
        }
        if let Some(target) = self.take_navigation() {
            if let PresentationAppLibraryEnumNavigation::OpenSaved(intent) = target {
                self.dispatch_panel(*intent, port)?;
            } else {
                return Ok(Some(target));
            }
        }
        Ok(None)
    }
}
