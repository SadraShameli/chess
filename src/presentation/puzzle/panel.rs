use std::sync::Arc;

use bevy_egui::egui::Ui;
use thiserror::Error;
use tokio::sync::watch;

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplicationError, DesktopApplicationPort, DesktopApplicationReceipt,
    DesktopApplicationWakeProvider,
};
use crate::desktop::dto::{DesktopBoardSnapshot, DesktopEnumValue};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::messages::{
    DesktopCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult, DesktopRequest,
};
use crate::desktop::puzzle_bridge::{
    DesktopPuzzleBridgeEnumOperation, DesktopPuzzleBridgeEnumResult,
};
use crate::desktop::puzzle_service::{
    DesktopPuzzleServiceEnumActivity, DesktopPuzzleServiceEnumOperation,
    DesktopPuzzleServiceEnumResult, DesktopPuzzleServiceError, DesktopPuzzleServiceState,
};
use crate::presentation::board::view::PresentationBoardAction;
use crate::presentation::puzzle::controller::{
    PresentationPuzzleController, PresentationPuzzleControllerEnumAction,
    PresentationPuzzleControllerError, PresentationPuzzleControllerIntent,
};
use crate::presentation::puzzle::history::{
    PresentationPuzzleHistory, PresentationPuzzleHistoryEnumChoice,
};
use crate::presentation::puzzle::setup::PresentationPuzzleSetup;
use crate::presentation::puzzle::style::PresentationPuzzleStyle;
use crate::presentation::puzzle::summary::PresentationPuzzleSummary;
use crate::presentation::puzzle::view::PresentationPuzzleView;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton,
};
use crate::puzzle::history::{PuzzleHistoryEnumConfirmation, PuzzleHistoryEnumScope};
use crate::puzzle::persistence::PuzzlePersistenceStorageRevision;
use crate::puzzle::run::PuzzleRunEnumStatus;
use crate::puzzle::session::{PuzzleSessionContext, PuzzleSessionId};
use crate::puzzle::summary::PuzzleSummary;
use crate::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness};
use crate::storage::puzzle_repository::StoragePuzzleRunPage;

#[cfg(test)]
#[path = "../../../tests/presentation/puzzle_panel.rs"]
mod tests;

#[derive(Debug, Error)]
pub enum PresentationPuzzlePanelError {
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
    #[error(transparent)]
    Controller(#[from] PresentationPuzzleControllerError),
    #[error(transparent)]
    Failure(#[from] DesktopFailure),
    #[error("The saved puzzle service failed: {0}")]
    Service(#[source] Arc<DesktopPuzzleServiceError>),
    #[error("The saved puzzle service state disconnected: {0}")]
    Disconnected(#[source] watch::error::RecvError),
    #[error("Wait for the pending saved puzzle request")]
    Pending,
    #[error("Pause the current puzzle run and wait for saving before opening another saved run")]
    PauseRequired,
    #[error("Open a saved puzzle run first")]
    NoRun,
    #[error("The puzzle response does not match its original request")]
    Response,
    #[error("The selected puzzle state changed. Select an action for the current state")]
    SelectionChanged,
    #[error("There is no failed puzzle request to retry")]
    NoRetry,
    #[error("There is no next saved-run page")]
    NoNextPage,
}

pub struct PresentationPuzzlePanelConnection {
    pub owner: DesktopMessagesEntityId,
    pub state: watch::Receiver<DesktopPuzzleServiceState>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PresentationPuzzlePanelEnumPendingState {
    Admitting,
    Waiting,
    Failed,
}

struct PresentationPuzzlePanelPending {
    operation: DesktopPuzzleServiceEnumOperation,
    intent: Option<PresentationPuzzleControllerIntent>,
    request: Option<DesktopRequest>,
    receipt: Option<DesktopApplicationReceipt>,
    state: PresentationPuzzlePanelEnumPendingState,
    intent_finished: bool,
}

impl PresentationPuzzlePanelPending {
    fn new(
        operation: DesktopPuzzleServiceEnumOperation,
        intent: Option<PresentationPuzzleControllerIntent>,
    ) -> Self {
        Self {
            operation,
            intent,
            request: None,
            receipt: None,
            state: PresentationPuzzlePanelEnumPendingState::Admitting,
            intent_finished: false,
        }
    }

    fn is_pending(&self) -> bool {
        self.state != PresentationPuzzlePanelEnumPendingState::Failed
    }

    fn can_replace(&self) -> bool {
        !self.is_pending() && (self.intent.is_none() || self.intent_finished)
    }

    fn response(&self) -> Option<DesktopMessagesEnumResult> {
        if self.state != PresentationPuzzlePanelEnumPendingState::Waiting {
            return None;
        }
        self.receipt.as_ref()?.response().cloned()
    }
}

pub struct PresentationPuzzlePanel {
    connection: PresentationPuzzlePanelConnection,
    controller: Option<PresentationPuzzleController>,
    view: PresentationPuzzleView,
    setup: PresentationPuzzleSetup,
    inventory: Option<Arc<StoragePuzzleRunPage>>,
    ended_summary: Option<PuzzleSummary>,
    ordinary: Option<PresentationPuzzlePanelPending>,
    urgent: Option<PresentationPuzzlePanelPending>,
    last_ordinary: Option<PresentationPuzzlePanelPending>,
    last_urgent: Option<PresentationPuzzlePanelPending>,
    failure: Option<PresentationPuzzlePanelError>,
    hidden: bool,
    readiness: PuzzleTimerEnumReadiness,
    opponent_requested: Option<PuzzleSessionContext>,
    deletion: Option<PuzzleHistoryEnumScope>,
}

impl PresentationPuzzlePanel {
    pub fn new(connection: PresentationPuzzlePanelConnection) -> Self {
        Self {
            connection,
            controller: None,
            view: PresentationPuzzleView::new(),
            setup: PresentationPuzzleSetup::new(),
            inventory: None,
            ended_summary: None,
            ordinary: None,
            urgent: None,
            last_ordinary: None,
            last_urgent: None,
            failure: None,
            hidden: false,
            readiness: PuzzleTimerEnumReadiness::BoardNotReady,
            opponent_requested: None,
            deletion: None,
        }
    }

    pub const fn pending_deletion(&self) -> Option<PuzzleHistoryEnumScope> {
        self.deletion
    }

    pub fn request_history_deletion<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        scope: PuzzleHistoryEnumScope,
        confirmation: PuzzleHistoryEnumConfirmation,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        self.submit(
            DesktopPuzzleServiceEnumOperation::DeleteHistory {
                scope,
                confirmation,
            },
            None,
            port,
        )?;
        self.deletion = None;
        Ok(())
    }

    pub fn begin_history_deletion(&mut self, scope: PuzzleHistoryEnumScope) {
        self.deletion = Some(scope);
    }

    pub fn cancel_history_deletion(&mut self) {
        self.deletion = None;
    }

    pub fn board(&self) -> Option<&DesktopBoardSnapshot> {
        if self.hidden || self.is_opening() || !self.is_selected() {
            return None;
        }
        self.controller.as_ref()?.board()
    }

    pub const fn controller(&self) -> Option<&PresentationPuzzleController> {
        self.controller.as_ref()
    }

    pub fn is_promotion_pending(&self) -> bool {
        self.board().is_some()
            && self
                .controller
                .as_ref()
                .is_some_and(|controller| !controller.promotion_choices().is_empty())
    }

    pub fn has_pending_work(&self) -> bool {
        self.ordinary
            .as_ref()
            .is_some_and(PresentationPuzzlePanelPending::is_pending)
            || self
                .urgent
                .as_ref()
                .is_some_and(PresentationPuzzlePanelPending::is_pending)
    }

    pub const fn inventory(&self) -> Option<&Arc<StoragePuzzleRunPage>> {
        self.inventory.as_ref()
    }

    pub fn summary(&self) -> Option<&PuzzleSummary> {
        self.ended_summary
            .as_ref()
            .or_else(|| self.controller.as_ref()?.summary())
    }

    pub const fn failure(&self) -> Option<&PresentationPuzzlePanelError> {
        self.failure.as_ref()
    }

    pub fn synchronize<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        self.observe_service()?;
        self.poll(true)?;
        self.poll(false)?;
        self.admit(true, port)?;
        self.admit(false, port)?;
        self.request_opponent(port)
    }

    pub fn first_page<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        self.submit(
            DesktopPuzzleServiceEnumOperation::Inventory { after: None },
            None,
            port,
        )
    }

    pub fn next_page<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        let after = self
            .inventory
            .as_ref()
            .and_then(|page| page.next_cursor())
            .ok_or(PresentationPuzzlePanelError::NoNextPage)?;
        self.submit(
            DesktopPuzzleServiceEnumOperation::Inventory { after: Some(after) },
            None,
            port,
        )
    }

    pub fn open_saved<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        id: PuzzleSessionId,
        storage_revision: PuzzlePersistenceStorageRevision,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        self.ensure_can_open()?;
        self.submit(
            DesktopPuzzleServiceEnumOperation::OpenSaved {
                id,
                storage_revision,
            },
            None,
            port,
        )
    }

    pub fn request_summary<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        self.request_projection(DesktopPuzzleBridgeEnumOperation::Summary, port)
    }

    pub fn refresh_snapshot<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        self.request_projection(DesktopPuzzleBridgeEnumOperation::Snapshot, port)
    }

    pub fn request_action<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        action: PresentationPuzzleControllerEnumAction,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        let urgent = matches!(
            action,
            PresentationPuzzleControllerEnumAction::Pause(_)
                | PresentationPuzzleControllerEnumAction::Hidden
        );
        if !urgent
            && action != PresentationPuzzleControllerEnumAction::RetrySave
            && (self.hidden || self.is_opening() || !self.is_selected())
        {
            return Err(PresentationPuzzlePanelError::NoRun);
        }
        self.ensure_slot(urgent)?;
        let controller = self
            .controller
            .as_mut()
            .ok_or(PresentationPuzzlePanelError::NoRun)?;
        if let Some(intent) = controller.request(action, DesktopRequestId::new())? {
            self.submit_intent(intent, port)?;
        }
        Ok(())
    }

    pub fn board_action<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        action: PresentationBoardAction,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        if self.board().is_none() {
            return Err(PresentationPuzzlePanelError::NoRun);
        }
        self.ensure_slot(false)?;
        let controller = self
            .controller
            .as_mut()
            .ok_or(PresentationPuzzlePanelError::NoRun)?;
        if let Some(intent) = controller.board_action(action)? {
            self.submit_intent(intent, port)?;
        }
        Ok(())
    }

    pub fn report_readiness<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        readiness: PuzzleTimerEnumReadiness,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        self.readiness = readiness;
        self.hidden = readiness == PuzzleTimerEnumReadiness::Hidden;
        let can_submit = self.ensure_slot(self.hidden).is_ok()
            && (self.hidden || (!self.is_opening() && self.is_selected()));
        if let Some(controller) = &mut self.controller {
            let readiness = if !can_submit && readiness == PuzzleTimerEnumReadiness::Ready {
                PuzzleTimerEnumReadiness::BoardNotReady
            } else {
                readiness
            };
            if let Some(intent) = controller.report_readiness(readiness, DesktopRequestId::new())? {
                self.submit_intent(intent, port)?;
            }
        }
        self.request_opponent(port)
    }

    pub fn retry<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        let urgent = self
            .urgent
            .as_ref()
            .is_some_and(|pending| !pending.is_pending());
        let previous = self
            .slot(urgent)
            .as_ref()
            .filter(|pending| !pending.is_pending())
            .ok_or(PresentationPuzzlePanelError::NoRetry)?;
        let operation = previous.operation.clone();
        let original = previous.intent.clone();
        let finished = previous.intent_finished;
        if matches!(
            operation,
            DesktopPuzzleServiceEnumOperation::OpenSaved { .. }
        ) {
            self.ensure_can_open()?;
        }
        let intent = if finished {
            if let Some(original) = original {
                let controller = self
                    .controller
                    .as_mut()
                    .ok_or(PresentationPuzzlePanelError::NoRun)?;
                let same_session_interrupt = matches!(operation, DesktopPuzzleServiceEnumOperation::Interrupt { session, .. }
                    if session == controller.snapshot().context().session_id());
                if controller.snapshot().context() != original.context() && !same_session_interrupt
                {
                    return Err(PresentationPuzzlePanelError::SelectionChanged);
                }
                controller.request(original.action(), DesktopRequestId::new())?
            } else {
                None
            }
        } else {
            original
        };
        let previous = self.slot_mut(urgent).take();
        *self.last_slot_mut(urgent) = previous;
        *self.slot_mut(urgent) = Some(PresentationPuzzlePanelPending::new(operation, intent));
        self.failure = None;
        self.admit(urgent, port)
    }

    pub fn show<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        ui: &mut Ui,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        self.synchronize(port)?;
        if let Some(failure) = &self.failure {
            PresentationPuzzleStyle::error(ui, failure.to_string());
        }
        if (self
            .ordinary
            .as_ref()
            .is_some_and(|pending| !pending.is_pending())
            || self
                .urgent
                .as_ref()
                .is_some_and(|pending| !pending.is_pending()))
            && PresentationThemeEnumButton::Secondary
                .show(ui, "Retry Puzzle Request")
                .clicked()
        {
            self.retry(port)?;
        }
        self.show_inventory(ui, port)?;
        self.show_archived_attempts(ui);
        match self.connection.state.borrow().activity() {
            DesktopPuzzleServiceEnumActivity::Idle => {}
            DesktopPuzzleServiceEnumActivity::Loading { .. } => {
                PresentationPuzzleStyle::note(ui, "Loading saved puzzle run");
            }
            DesktopPuzzleServiceEnumActivity::Replacing { .. } => {
                PresentationPuzzleStyle::note(ui, "Replacing the selected puzzle run");
            }
            DesktopPuzzleServiceEnumActivity::Selected => {}
            DesktopPuzzleServiceEnumActivity::Closing => {
                PresentationPuzzleStyle::note(
                    ui,
                    "Closing puzzle runs. Save recovery remains available",
                );
            }
            DesktopPuzzleServiceEnumActivity::ShutdownComplete => {
                PresentationPuzzleStyle::note(ui, "Puzzle service stopped");
            }
        }
        if self.controller.is_some() && !self.is_opening() {
            let projection_pending = self
                .ordinary
                .as_ref()
                .is_some_and(|pending| pending.is_pending() && pending.intent.is_none());
            if projection_pending {
                PresentationPuzzleStyle::note(ui, "Waiting for the saved puzzle request");
                let can_pause = self.controller.as_ref().is_some_and(|controller| {
                    controller.can_request(PresentationPuzzleControllerEnumAction::Pause(
                        PuzzleTimerEnumPauseReason::Manual,
                    ))
                });
                if PresentationPuzzleStyle::small(
                    ui,
                    PresentationThemeEnumButton::Secondary,
                    "Pause",
                    can_pause,
                ) {
                    self.request_action(
                        PresentationPuzzleControllerEnumAction::Pause(
                            PuzzleTimerEnumPauseReason::Manual,
                        ),
                        port,
                    )?;
                }
            } else if !self.hidden && self.is_selected() {
                let intent = self
                    .controller
                    .as_mut()
                    .and_then(|controller| self.view.show(ui, controller));
                if let Some(intent) = intent {
                    self.submit_intent(intent, port)?;
                }
            } else {
                PresentationPuzzleStyle::note(
                    ui,
                    "Puzzle content is hidden until its workspace and selected run are available",
                );
                let can_retry_save = self.controller.as_ref().is_some_and(|controller| {
                    controller.can_request(PresentationPuzzleControllerEnumAction::RetrySave)
                }) && self.ensure_slot(false).is_ok();
                if PresentationPuzzleStyle::small(
                    ui,
                    PresentationThemeEnumButton::Secondary,
                    "Retry Save",
                    can_retry_save,
                ) {
                    self.request_action(PresentationPuzzleControllerEnumAction::RetrySave, port)?;
                }
            }
            ui.horizontal(|ui| -> Result<(), PresentationPuzzlePanelError> {
                let enabled = self.ensure_slot(false).is_ok();
                if PresentationPuzzleStyle::small(
                    ui,
                    PresentationThemeEnumButton::Secondary,
                    "Refresh Puzzle State",
                    enabled,
                ) {
                    self.refresh_snapshot(port)?;
                }
                if PresentationPuzzleStyle::small(
                    ui,
                    PresentationThemeEnumButton::Secondary,
                    "Show Run Summary",
                    enabled,
                ) {
                    self.request_summary(port)?;
                }
                Ok(())
            })
            .inner?;
        } else if let Some(summary) = &self.ended_summary {
            PresentationPuzzleSummary::show(ui, summary);
        }
        PresentationThemeComponents::divider(ui);
        if self.setup.show(ui, &[], false).is_some() {
            return Err(PresentationPuzzlePanelError::Response);
        }
        Ok(())
    }

    fn observe_service(&mut self) -> Result<(), PresentationPuzzlePanelError> {
        if !self
            .connection
            .state
            .has_changed()
            .map_err(PresentationPuzzlePanelError::Disconnected)?
        {
            return Ok(());
        }
        let state = self.connection.state.borrow_and_update().clone();
        if let Some(failure) = state.failure() {
            self.failure = Some(PresentationPuzzlePanelError::Service(Arc::clone(failure)));
        }
        self.observe_authoritative_snapshot(&state);
        Ok(())
    }

    fn observe_authoritative_snapshot(&mut self, state: &DesktopPuzzleServiceState) {
        if let Some(controller) = &mut self.controller {
            if let Some(snapshot) = state.snapshot()
                && state.selected() == Some(controller.snapshot().context().session_id())
            {
                controller.observe_snapshot((**snapshot).clone());
            }
            if self.hidden
                || !matches!(state.activity(), DesktopPuzzleServiceEnumActivity::Selected)
            {
                controller.cancel_selection();
            }
        }
    }

    fn ensure_can_open(&mut self) -> Result<(), PresentationPuzzlePanelError> {
        self.observe_service()?;
        let Some(controller) = &self.controller else {
            return Ok(());
        };
        match controller.snapshot().status() {
            PuzzleRunEnumStatus::AwaitingReadiness
            | PuzzleRunEnumStatus::Active
            | PuzzleRunEnumStatus::Review
            | PuzzleRunEnumStatus::Exhausted => Err(PresentationPuzzlePanelError::PauseRequired),
            PuzzleRunEnumStatus::Paused { .. }
            | PuzzleRunEnumStatus::Expired(_)
            | PuzzleRunEnumStatus::Ended(_) => Ok(()),
        }
    }

    fn is_selected(&self) -> bool {
        let state = self.connection.state.borrow();
        self.connection.state.has_changed().is_ok()
            && matches!(state.activity(), DesktopPuzzleServiceEnumActivity::Selected)
            && self.controller.as_ref().is_some_and(|controller| {
                state.selected() == Some(controller.snapshot().context().session_id())
            })
    }

    fn is_opening(&self) -> bool {
        self.ordinary.as_ref().is_some_and(|pending| {
            pending.is_pending()
                && matches!(
                    pending.operation,
                    DesktopPuzzleServiceEnumOperation::OpenSaved { .. }
                )
        })
    }

    fn ensure_slot(&self, urgent: bool) -> Result<(), PresentationPuzzlePanelError> {
        if self
            .slot(urgent)
            .as_ref()
            .is_some_and(|pending| !pending.can_replace())
        {
            Err(PresentationPuzzlePanelError::Pending)
        } else {
            Ok(())
        }
    }

    fn slot(&self, urgent: bool) -> &Option<PresentationPuzzlePanelPending> {
        if urgent { &self.urgent } else { &self.ordinary }
    }

    fn slot_mut(&mut self, urgent: bool) -> &mut Option<PresentationPuzzlePanelPending> {
        if urgent {
            &mut self.urgent
        } else {
            &mut self.ordinary
        }
    }

    fn last_slot_mut(&mut self, urgent: bool) -> &mut Option<PresentationPuzzlePanelPending> {
        if urgent {
            &mut self.last_urgent
        } else {
            &mut self.last_ordinary
        }
    }

    fn submit<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        operation: DesktopPuzzleServiceEnumOperation,
        intent: Option<PresentationPuzzleControllerIntent>,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        let urgent = matches!(
            operation,
            DesktopPuzzleServiceEnumOperation::Interrupt { .. }
        );
        self.ensure_slot(urgent)?;
        let previous = self.slot_mut(urgent).take();
        if previous.is_some() {
            *self.last_slot_mut(urgent) = previous;
        }
        *self.slot_mut(urgent) = Some(PresentationPuzzlePanelPending::new(operation, intent));
        self.failure = None;
        self.admit(urgent, port)
    }

    fn submit_intent<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        intent: PresentationPuzzleControllerIntent,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        let operation = match intent.action() {
            PresentationPuzzleControllerEnumAction::Pause(reason) => {
                self.hidden = true;
                DesktopPuzzleServiceEnumOperation::Interrupt {
                    session: intent.context().session_id(),
                    reason,
                }
            }
            PresentationPuzzleControllerEnumAction::Hidden => {
                self.hidden = true;
                DesktopPuzzleServiceEnumOperation::Interrupt {
                    session: intent.context().session_id(),
                    reason: PuzzleTimerEnumPauseReason::FocusLost,
                }
            }
            action @ (PresentationPuzzleControllerEnumAction::Move(_)
            | PresentationPuzzleControllerEnumAction::OpponentReply
            | PresentationPuzzleControllerEnumAction::Hint
            | PresentationPuzzleControllerEnumAction::RetryPractice
            | PresentationPuzzleControllerEnumAction::Skip
            | PresentationPuzzleControllerEnumAction::Next
            | PresentationPuzzleControllerEnumAction::Resume
            | PresentationPuzzleControllerEnumAction::End(_)
            | PresentationPuzzleControllerEnumAction::Reshuffle
            | PresentationPuzzleControllerEnumAction::Ready
            | PresentationPuzzleControllerEnumAction::RetrySave) => {
                DesktopPuzzleServiceEnumOperation::Session {
                    expected: intent.context().clone(),
                    operation: action.command().map_or(
                        DesktopPuzzleBridgeEnumOperation::RetrySave,
                        DesktopPuzzleBridgeEnumOperation::Command,
                    ),
                }
            }
        };
        self.submit(operation, Some(intent), port)
    }

    fn request_projection<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        operation: DesktopPuzzleBridgeEnumOperation,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        let expected = self
            .controller
            .as_ref()
            .ok_or(PresentationPuzzlePanelError::NoRun)?
            .snapshot()
            .context()
            .clone();
        self.submit(
            DesktopPuzzleServiceEnumOperation::Session {
                expected,
                operation,
            },
            None,
            port,
        )
    }

    fn admit<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        urgent: bool,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        let owner = self.connection.owner;
        let Some(pending) = self.slot_mut(urgent).as_mut() else {
            return Ok(());
        };
        if pending.state != PresentationPuzzlePanelEnumPendingState::Admitting {
            return Ok(());
        }
        let result = (|| {
            if pending.request.is_none() {
                pending.request =
                    Some(port.request(DesktopCommand::puzzle(owner, pending.operation.clone()))?);
            }
            let request = pending
                .request
                .as_ref()
                .ok_or(DesktopApplicationError::InvalidTransition)?;
            port.try_submit(request)
        })();
        match result {
            Ok(receipt) => {
                pending.receipt = Some(receipt);
                pending.state = PresentationPuzzlePanelEnumPendingState::Waiting;
            }
            Err(DesktopApplicationError::Busy | DesktopApplicationError::EntityBusy) => {}
            Err(error) => {
                let failure = DesktopFailure::new(
                    DesktopEnumFailureCode::ResourceRefusal,
                    error.to_string(),
                    true,
                );
                if let Some(pending) = self.slot_mut(urgent) {
                    pending.state = PresentationPuzzlePanelEnumPendingState::Failed;
                }
                self.failure = Some(PresentationPuzzlePanelError::Application(error));
                self.fail_intent(urgent, failure)?;
            }
        }
        Ok(())
    }

    fn poll(&mut self, urgent: bool) -> Result<(), PresentationPuzzlePanelError> {
        let Some(response) = self
            .slot(urgent)
            .as_ref()
            .and_then(PresentationPuzzlePanelPending::response)
        else {
            return Ok(());
        };
        let result = match response {
            DesktopMessagesEnumResult::Value {
                value: DesktopEnumValue::Puzzle(result),
                ..
            } => self.observe_result(urgent, *result),
            DesktopMessagesEnumResult::Failure(failure) => self
                .fail_intent(urgent, failure.clone())
                .and(Err(PresentationPuzzlePanelError::Failure(failure))),
            DesktopMessagesEnumResult::Cancelled
            | DesktopMessagesEnumResult::Stopped
            | DesktopMessagesEnumResult::Paused
            | DesktopMessagesEnumResult::QuitReady
            | DesktopMessagesEnumResult::ReconciliationRequired => {
                let failure = DesktopFailure::new(DesktopEnumFailureCode::ResyncRequired, "The puzzle action stopped before an authoritative result. Retry or refresh its original session".into(), true);
                self.fail_intent(urgent, failure.clone())
                    .and(Err(PresentationPuzzlePanelError::Failure(failure)))
            }
            DesktopMessagesEnumResult::Value { .. }
            | DesktopMessagesEnumResult::LibraryFailure(_)
            | DesktopMessagesEnumResult::EditorFailure(_) => {
                Err(PresentationPuzzlePanelError::Response)
            }
        };
        match result {
            Ok(()) => {
                let completed = self.slot_mut(urgent).take();
                *self.last_slot_mut(urgent) = completed;
            }
            Err(error) => {
                if let Some(pending) = self.slot_mut(urgent) {
                    pending.state = PresentationPuzzlePanelEnumPendingState::Failed;
                }
                self.failure = Some(error);
            }
        }
        Ok(())
    }

    fn fail_intent(
        &mut self,
        urgent: bool,
        failure: DesktopFailure,
    ) -> Result<(), PresentationPuzzlePanelError> {
        let intent = self
            .slot(urgent)
            .as_ref()
            .and_then(|pending| pending.intent.as_ref())
            .cloned();
        if let Some(intent) = intent
            && let Some(controller) = &mut self.controller
            && controller.snapshot().context().session_id() == intent.context().session_id()
        {
            controller.observe_failure(&intent, failure)?;
        }
        if let Some(pending) = self.slot_mut(urgent) {
            pending.intent_finished = true;
        }
        Ok(())
    }

    fn observe_result(
        &mut self,
        urgent: bool,
        result: DesktopPuzzleServiceEnumResult,
    ) -> Result<(), PresentationPuzzlePanelError> {
        let pending = self
            .slot(urgent)
            .as_ref()
            .ok_or(PresentationPuzzlePanelError::Response)?;
        match (&pending.operation, result) {
            (
                DesktopPuzzleServiceEnumOperation::Inventory { .. },
                DesktopPuzzleServiceEnumResult::Inventory(page),
            ) => {
                self.inventory = Some(page);
            }
            (
                DesktopPuzzleServiceEnumOperation::OpenSaved { id, .. },
                DesktopPuzzleServiceEnumResult::Opened(snapshot),
            ) => {
                let state = self.connection.state.borrow().clone();
                if snapshot.context().session_id() != *id || state.selected() != Some(*id) {
                    return Err(PresentationPuzzlePanelError::Response);
                }
                self.controller = Some(PresentationPuzzleController::new((*snapshot).clone()));
                self.observe_authoritative_snapshot(&state);
                self.ended_summary = None;
                self.opponent_requested = None;
                self.view = PresentationPuzzleView::new();
            }
            (
                DesktopPuzzleServiceEnumOperation::OpenSaved {
                    id,
                    storage_revision,
                },
                DesktopPuzzleServiceEnumResult::Ended {
                    id: ended,
                    storage_revision: revision,
                    summary,
                },
            ) => {
                if *id != ended || *storage_revision != revision {
                    return Err(PresentationPuzzlePanelError::Response);
                }
                self.controller = None;
                self.ended_summary = Some(summary);
                self.opponent_requested = None;
            }
            (
                DesktopPuzzleServiceEnumOperation::Session {
                    expected,
                    operation,
                },
                DesktopPuzzleServiceEnumResult::Session(result),
            ) => {
                if result.snapshot().context().session_id() != expected.session_id()
                    || matches!(operation, DesktopPuzzleBridgeEnumOperation::Summary)
                        != matches!(result, DesktopPuzzleBridgeEnumResult::Summary { .. })
                {
                    return Err(PresentationPuzzlePanelError::Response);
                }
                self.observe_session(urgent, result)?;
            }
            (
                DesktopPuzzleServiceEnumOperation::Interrupt { session, .. },
                DesktopPuzzleServiceEnumResult::Session(result),
            ) => {
                if result.snapshot().context().session_id() != *session
                    || !matches!(result, DesktopPuzzleBridgeEnumResult::Snapshot(_))
                {
                    return Err(PresentationPuzzlePanelError::Response);
                }
                self.observe_session(urgent, result)?;
            }
            (
                DesktopPuzzleServiceEnumOperation::DeleteHistory { .. },
                DesktopPuzzleServiceEnumResult::HistoryDeleted { .. },
            ) => {
                self.inventory = None;
            }
            _ => return Err(PresentationPuzzlePanelError::Response),
        }
        Ok(())
    }

    fn observe_session(
        &mut self,
        urgent: bool,
        result: DesktopPuzzleBridgeEnumResult,
    ) -> Result<(), PresentationPuzzlePanelError> {
        let intent = self
            .slot(urgent)
            .as_ref()
            .and_then(|pending| pending.intent.as_ref())
            .cloned();
        let (snapshot, summary) = match result {
            DesktopPuzzleBridgeEnumResult::Snapshot(snapshot) => (snapshot, None),
            DesktopPuzzleBridgeEnumResult::Summary { snapshot, summary } => {
                (snapshot, Some(summary))
            }
        };
        if let Some(controller) = &mut self.controller
            && controller.snapshot().context().session_id() == snapshot.context().session_id()
        {
            if let Some(intent) = intent {
                controller.observe_success(&intent, (*snapshot).clone(), summary)?;
            } else {
                controller.observe_state((*snapshot).clone(), summary);
            }
        }
        Ok(())
    }

    fn request_opponent<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        if self.hidden
            || self.readiness != PuzzleTimerEnumReadiness::Ready
            || self.is_opening()
            || !self.is_selected()
            || self.ensure_slot(false).is_err()
        {
            return Ok(());
        }
        let Some(controller) = &self.controller else {
            return Ok(());
        };
        let context = controller.snapshot().context();
        if self.opponent_requested.as_ref() != Some(context)
            && controller.can_request(PresentationPuzzleControllerEnumAction::OpponentReply)
        {
            self.opponent_requested = Some(context.clone());
            self.request_action(PresentationPuzzleControllerEnumAction::OpponentReply, port)?;
        }
        Ok(())
    }

    fn show_archived_attempts(&mut self, ui: &mut Ui) {
        let Some(controller) = &self.controller else {
            return;
        };
        let snapshot = controller.snapshot();
        let session = snapshot.context().session_id();
        let attempts = snapshot.archived_attempt_ids();
        if attempts.is_empty() {
            return;
        }
        let finished = matches!(
            snapshot.status(),
            PuzzleRunEnumStatus::Ended(_)
                | PuzzleRunEnumStatus::Expired(_)
                | PuzzleRunEnumStatus::Exhausted
        );
        let enabled = finished && self.deletion.is_none();
        let mut requested = None;
        PresentationThemeComponents::subhead(ui, "Archived attempts");
        for attempt in attempts {
            PresentationPuzzleStyle::row(ui, &format!("Attempt {}", attempt.value()), |ui| {
                if PresentationPuzzleStyle::small(
                    ui,
                    PresentationThemeEnumButton::Danger,
                    "Delete Attempt",
                    enabled,
                ) {
                    requested = Some(PuzzleHistoryEnumScope::Attempt {
                        session,
                        attempt: *attempt,
                    });
                }
            });
        }
        if !finished {
            PresentationThemeComponents::caption(
                ui,
                "Attempts can be deleted after the run has ended",
            );
        }
        if requested.is_some() {
            self.deletion = requested;
        }
    }

    fn show_inventory<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        ui: &mut Ui,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        PresentationThemeComponents::subhead(ui, "Saved puzzle runs");
        PresentationThemeComponents::caption(
            ui,
            "Run identifiers are ordered numerically. Open validates the saved run; this list contains metadata only",
        );
        let enabled = self.ensure_slot(false).is_ok();
        ui.horizontal(|ui| -> Result<(), PresentationPuzzlePanelError> {
            if PresentationPuzzleStyle::small(
                ui,
                PresentationThemeEnumButton::Secondary,
                "First Page",
                enabled,
            ) {
                self.first_page(port)?;
            }
            let has_next = self
                .inventory
                .as_ref()
                .is_some_and(|page| page.next_cursor().is_some());
            if PresentationPuzzleStyle::small(
                ui,
                PresentationThemeEnumButton::Secondary,
                "Next Page",
                enabled && has_next,
            ) {
                self.next_page(port)?;
            }
            Ok(())
        })
        .inner?;
        if let Some(page) = &self.inventory {
            if page.entries().is_empty() {
                PresentationPuzzleStyle::note(ui, "No saved puzzle runs on this page");
            }
            let mut selected = None;
            let mut requested = None;
            for entry in page.entries() {
                let label = format!(
                    "Run {}: saved revision {}, {} events",
                    entry.id().value(),
                    entry.storage_revision().value(),
                    entry.persisted_event_count()
                );
                PresentationPuzzleStyle::row(ui, &label, |ui| {
                    if PresentationPuzzleStyle::small(
                        ui,
                        PresentationThemeEnumButton::Danger,
                        "Delete Run",
                        enabled,
                    ) {
                        requested = Some(PuzzleHistoryEnumScope::Session(entry.id()));
                    }
                    if PresentationPuzzleStyle::small(
                        ui,
                        PresentationThemeEnumButton::Secondary,
                        "Open",
                        enabled,
                    ) {
                        selected = Some((entry.id(), entry.storage_revision()));
                    }
                });
            }
            if let Some((id, revision)) = selected {
                self.open_saved(id, revision, port)?;
            }
            if requested.is_some() {
                self.deletion = requested;
            }
        } else {
            PresentationPuzzleStyle::note(ui, "Select First Page to read saved runs");
        }
        if PresentationPuzzleStyle::small(
            ui,
            PresentationThemeEnumButton::Danger,
            "Delete All History",
            enabled,
        ) {
            self.deletion = Some(PuzzleHistoryEnumScope::All);
        }
        self.show_deletion_confirmation(ui, port)?;
        if self.ordinary.as_ref().is_some_and(|pending| {
            pending.is_pending()
                && matches!(
                    pending.operation,
                    DesktopPuzzleServiceEnumOperation::Inventory { .. }
                )
        }) {
            PresentationPuzzleStyle::note(ui, "Loading saved-run page");
        }
        Ok(())
    }

    fn show_deletion_confirmation<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        ui: &mut Ui,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationPuzzlePanelError> {
        let Some(scope) = self.deletion else {
            return Ok(());
        };
        match PresentationPuzzleHistory::confirm_deletion(ui, scope) {
            PresentationPuzzleHistoryEnumChoice::Cancel => self.cancel_history_deletion(),
            PresentationPuzzleHistoryEnumChoice::Confirm => self.request_history_deletion(
                scope,
                PuzzleHistoryEnumConfirmation::Confirmed,
                port,
            )?,
            PresentationPuzzleHistoryEnumChoice::Pending => {}
        }
        Ok(())
    }
}
