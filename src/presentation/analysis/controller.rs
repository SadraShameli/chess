use thiserror::Error;

use crate::board::r#move::BoardMoveError;
use crate::desktop::error::DesktopFailure;
use crate::desktop::identity::DesktopRequestId;
use crate::engine::search::identity::{EngineSearchContext, EngineSearchTag};
use crate::engine::search::limit::EngineSearchLimit;
use crate::engine::search::position::EngineSearchPosition;
use crate::engine::search::session::{
    EngineSearchSessionEnumCancellation, EngineSearchSessionEnumCompletion,
    EngineSearchSessionEnumEvent, EngineSearchSessionMetadata, EngineSearchSessionSnapshot,
    EngineSearchSessionTerminal,
};
use crate::presentation::analysis::line::PresentationAnalysisLine;

#[derive(Debug, Error)]
pub enum PresentationAnalysisControllerError {
    #[error("Switch to Analysis or Review to start analysis")]
    LivePlay,
    #[error("Wait for the current analysis request or stop its search first")]
    Busy,
    #[error("There is no active analysis search to stop")]
    Inactive,
    #[error(
        "The result belongs to another request, game, revision, position, search or process generation"
    )]
    StaleResult,
    #[error("This request identity was already used for another analysis action")]
    RequestConflict,
    #[error("The accepted analysis variation could not be displayed: {0}")]
    Variation(#[from] BoardMoveError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationAnalysisControllerEnumWorkspace {
    Analysis,
    Review,
    LivePlay,
}

impl PresentationAnalysisControllerEnumWorkspace {
    pub const fn has_evaluation_bar(self) -> bool {
        match self {
            Self::Analysis | Self::Review => true,
            Self::LivePlay => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PresentationAnalysisControllerSelection {
    context: EngineSearchContext,
    position: EngineSearchPosition,
    workspace: PresentationAnalysisControllerEnumWorkspace,
}

impl PresentationAnalysisControllerSelection {
    pub const fn new(
        context: EngineSearchContext,
        position: EngineSearchPosition,
        workspace: PresentationAnalysisControllerEnumWorkspace,
    ) -> Self {
        Self {
            context,
            position,
            workspace,
        }
    }

    pub const fn context(&self) -> EngineSearchContext {
        self.context
    }
    pub const fn position(&self) -> &EngineSearchPosition {
        &self.position
    }
    pub const fn workspace(&self) -> PresentationAnalysisControllerEnumWorkspace {
        self.workspace
    }

    fn matches_position(&self, position: &EngineSearchPosition) -> bool {
        self.position.initial() == position.initial()
            && self.position.history() == position.history()
    }

    fn matches(&self, other: &Self) -> bool {
        self.context == other.context
            && self.workspace == other.workspace
            && self.matches_position(&other.position)
    }

    fn accepts(&self, metadata: &EngineSearchSessionMetadata) -> bool {
        self.context == metadata.tag().context() && self.matches_position(metadata.position())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationAnalysisControllerEnumAction {
    Start(EngineSearchLimit),
    Stop(EngineSearchTag),
}

#[derive(Debug, Clone)]
pub struct PresentationAnalysisControllerIntent {
    request_id: DesktopRequestId,
    selection: PresentationAnalysisControllerSelection,
    action: PresentationAnalysisControllerEnumAction,
}

impl PresentationAnalysisControllerIntent {
    pub const fn request_id(&self) -> DesktopRequestId {
        self.request_id
    }
    pub const fn selection(&self) -> &PresentationAnalysisControllerSelection {
        &self.selection
    }
    pub const fn action(&self) -> &PresentationAnalysisControllerEnumAction {
        &self.action
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationAnalysisControllerEnumState {
    Idle,
    Starting,
    Searching,
    Stopping,
    Completed,
    Stopped,
    Cancelled,
    Failed,
}

pub struct PresentationAnalysisController {
    selection: PresentationAnalysisControllerSelection,
    state: PresentationAnalysisControllerEnumState,
    metadata: Option<EngineSearchSessionMetadata>,
    snapshot: Option<EngineSearchSessionSnapshot>,
    terminal: Option<EngineSearchSessionTerminal>,
    lines: Vec<PresentationAnalysisLine>,
    pending: Option<(DesktopRequestId, PresentationAnalysisControllerEnumAction)>,
    submitted: Option<(DesktopRequestId, PresentationAnalysisControllerEnumAction)>,
    failure: Option<DesktopFailure>,
}

impl PresentationAnalysisController {
    pub fn new(selection: PresentationAnalysisControllerSelection) -> Self {
        Self {
            selection,
            state: PresentationAnalysisControllerEnumState::Idle,
            metadata: None,
            snapshot: None,
            terminal: None,
            lines: Vec::new(),
            pending: None,
            submitted: None,
            failure: None,
        }
    }

    pub const fn selection(&self) -> &PresentationAnalysisControllerSelection {
        &self.selection
    }
    pub const fn state(&self) -> PresentationAnalysisControllerEnumState {
        self.state
    }
    pub const fn metadata(&self) -> Option<&EngineSearchSessionMetadata> {
        self.metadata.as_ref()
    }
    pub const fn snapshot(&self) -> Option<&EngineSearchSessionSnapshot> {
        self.snapshot.as_ref()
    }
    pub const fn terminal(&self) -> Option<&EngineSearchSessionTerminal> {
        self.terminal.as_ref()
    }
    pub const fn failure(&self) -> Option<&DesktopFailure> {
        self.failure.as_ref()
    }

    pub(super) fn lines(&self) -> &[PresentationAnalysisLine] {
        &self.lines
    }

    pub fn can_start(&self) -> bool {
        self.selection.workspace.has_evaluation_bar()
            && self.pending.is_none()
            && (self.metadata.is_none() || self.terminal.is_some())
    }

    pub fn can_stop(&self) -> bool {
        self.pending.is_none()
            && self.metadata.is_some()
            && self.terminal.is_none()
            && self.state != PresentationAnalysisControllerEnumState::Stopping
    }

    pub fn synchronize(
        &mut self,
        selection: PresentationAnalysisControllerSelection,
    ) -> Option<EngineSearchTag> {
        if self.selection.matches(&selection) {
            return None;
        }
        let stale = self
            .metadata
            .as_ref()
            .filter(|_metadata| self.terminal.is_none())
            .map(EngineSearchSessionMetadata::tag);
        self.selection = selection;
        self.state = PresentationAnalysisControllerEnumState::Idle;
        self.metadata = None;
        self.snapshot = None;
        self.terminal = None;
        self.lines.clear();
        self.pending = None;
        self.failure = None;
        stale
    }

    pub fn is_intent_current(&self, intent: &PresentationAnalysisControllerIntent) -> bool {
        self.selection.matches(intent.selection())
            && self.pending == Some((intent.request_id, intent.action))
    }

    pub fn start(
        &mut self,
        limit: EngineSearchLimit,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationAnalysisControllerIntent>, PresentationAnalysisControllerError>
    {
        let action = PresentationAnalysisControllerEnumAction::Start(limit);
        if self.is_duplicate(request_id, action)? {
            return Ok(None);
        }
        if !self.selection.workspace.has_evaluation_bar() {
            return Err(PresentationAnalysisControllerError::LivePlay);
        }
        if !self.can_start() {
            return Err(PresentationAnalysisControllerError::Busy);
        }
        self.metadata = None;
        self.snapshot = None;
        self.terminal = None;
        self.lines.clear();
        self.failure = None;
        self.state = PresentationAnalysisControllerEnumState::Starting;
        Ok(Some(self.submit(request_id, action)))
    }

    pub fn stop(
        &mut self,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationAnalysisControllerIntent>, PresentationAnalysisControllerError>
    {
        let tag = self
            .metadata
            .as_ref()
            .ok_or(PresentationAnalysisControllerError::Inactive)?
            .tag();
        let action = PresentationAnalysisControllerEnumAction::Stop(tag);
        if self.is_duplicate(request_id, action)? {
            return Ok(None);
        }
        if !self.can_stop() {
            return Err(PresentationAnalysisControllerError::Busy);
        }
        self.failure = None;
        self.state = PresentationAnalysisControllerEnumState::Stopping;
        Ok(Some(self.submit(request_id, action)))
    }

    pub fn observe_started(
        &mut self,
        request_id: DesktopRequestId,
        metadata: &EngineSearchSessionMetadata,
    ) -> Result<(), PresentationAnalysisControllerError> {
        if self.pending
            != Some((
                request_id,
                PresentationAnalysisControllerEnumAction::Start(metadata.limit()),
            ))
            || !self.selection.accepts(metadata)
        {
            return Err(PresentationAnalysisControllerError::StaleResult);
        }
        self.pending = None;
        self.metadata = Some(metadata.clone());
        self.state = PresentationAnalysisControllerEnumState::Searching;
        Ok(())
    }

    pub fn observe_failure(
        &mut self,
        request_id: DesktopRequestId,
        failure: DesktopFailure,
    ) -> Result<(), PresentationAnalysisControllerError> {
        if self
            .pending
            .is_none_or(|(pending, _action)| pending != request_id)
        {
            return Err(PresentationAnalysisControllerError::StaleResult);
        }
        self.pending = None;
        self.failure = Some(failure);
        self.state = PresentationAnalysisControllerEnumState::Failed;
        Ok(())
    }

    pub fn observe_event(
        &mut self,
        event: &EngineSearchSessionEnumEvent,
    ) -> Result<bool, PresentationAnalysisControllerError> {
        match event {
            EngineSearchSessionEnumEvent::Snapshot(snapshot) => {
                if self.terminal.is_some() || !self.accepts(snapshot.metadata()) {
                    return Ok(false);
                }
                let lines = PresentationAnalysisLine::from_snapshot(snapshot, &self.lines)?;
                self.lines = lines;
                self.snapshot = Some(snapshot.clone());
                if !matches!(
                    snapshot.cancellation(),
                    EngineSearchSessionEnumCancellation::NotRequested
                ) {
                    self.state = PresentationAnalysisControllerEnumState::Stopping;
                }
                Ok(true)
            }
            EngineSearchSessionEnumEvent::Finished(terminal) => self.observe_terminal(terminal),
            EngineSearchSessionEnumEvent::Readiness { tag }
            | EngineSearchSessionEnumEvent::Diagnostic { tag, .. } => Ok(self.terminal.is_none()
                && self
                    .metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata.tag() == *tag)),
        }
    }

    pub fn observe_terminal(
        &mut self,
        terminal: &EngineSearchSessionTerminal,
    ) -> Result<bool, PresentationAnalysisControllerError> {
        let snapshot = terminal.snapshot();
        if self.terminal.is_some() || !self.accepts(snapshot.metadata()) {
            return Ok(false);
        }
        let lines = PresentationAnalysisLine::from_snapshot(snapshot, &self.lines)?;
        self.state = match terminal.completion() {
            EngineSearchSessionEnumCompletion::BestMove(_) => {
                PresentationAnalysisControllerEnumState::Completed
            }
            EngineSearchSessionEnumCompletion::Stopped => {
                PresentationAnalysisControllerEnumState::Stopped
            }
            EngineSearchSessionEnumCompletion::Cancelled => {
                PresentationAnalysisControllerEnumState::Cancelled
            }
            EngineSearchSessionEnumCompletion::Failed(_) => {
                PresentationAnalysisControllerEnumState::Failed
            }
        };
        self.lines = lines;
        self.snapshot = Some(snapshot.clone());
        self.terminal = Some(terminal.clone());
        self.pending = None;
        self.failure = None;
        Ok(true)
    }

    fn accepts(&self, metadata: &EngineSearchSessionMetadata) -> bool {
        self.metadata.as_ref().is_some_and(|active| {
            active.tag() == metadata.tag()
                && active.limit() == metadata.limit()
                && active.started_at() == metadata.started_at()
        }) && self.selection.accepts(metadata)
    }

    fn is_duplicate(
        &self,
        request_id: DesktopRequestId,
        action: PresentationAnalysisControllerEnumAction,
    ) -> Result<bool, PresentationAnalysisControllerError> {
        match self.submitted {
            Some((previous, previous_action)) if previous == request_id => {
                if previous_action == action {
                    Ok(true)
                } else {
                    Err(PresentationAnalysisControllerError::RequestConflict)
                }
            }
            Some(_) | None => Ok(false),
        }
    }

    fn submit(
        &mut self,
        request_id: DesktopRequestId,
        action: PresentationAnalysisControllerEnumAction,
    ) -> PresentationAnalysisControllerIntent {
        self.pending = Some((request_id, action));
        self.submitted = self.pending;
        PresentationAnalysisControllerIntent {
            request_id,
            selection: self.selection.clone(),
            action,
        }
    }
}
