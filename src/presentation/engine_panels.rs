use std::sync::Arc;

use bevy_egui::egui::{self, Ui};
use thiserror::Error;
use tokio::sync::{mpsc, watch};

use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplicationError, DesktopApplicationPort, DesktopApplicationReceipt,
    DesktopApplicationWakeProvider,
};
use crate::desktop::dto::DesktopEnumValue;
use crate::desktop::engine_bridge::{DesktopEngineBridgeEnumEvent, DesktopEngineBridgeEnumResult};
use crate::desktop::engine_commands::{
    DesktopEngineCommandsContext, DesktopEngineCommandsEnumOperation,
    DesktopEngineCommandsOperation, DesktopEngineCommandsSnapshot,
};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::file_handle::DesktopFileEnumPurpose;
use crate::desktop::game_snapshot::{
    DesktopGameSnapshot, DesktopGameSnapshotEnumAnalysis,
    DesktopGameSnapshotEnumAnalysisUnavailable,
};
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};
use crate::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
    DesktopRequest,
};
use crate::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation;
use crate::desktop::workbench_export::DesktopWorkbenchExportPreview;
use crate::engine::search::enums::EngineSearchEnumState;
use crate::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchIdentityError, EngineSearchRevision,
};
use crate::engine::search::session::{EngineSearchSessionEnumEvent, EngineSearchSessionSnapshot};
use crate::game::enums::{GameEnumPhase, GameId};
use crate::game::session::GameSessionEnumInterruption;
use crate::presentation::analysis::controller::{
    PresentationAnalysisController, PresentationAnalysisControllerEnumAction,
    PresentationAnalysisControllerEnumWorkspace, PresentationAnalysisControllerError,
    PresentationAnalysisControllerIntent, PresentationAnalysisControllerSelection,
};
use crate::presentation::analysis::view::{
    PresentationAnalysisView, PresentationAnalysisViewError,
};
use crate::presentation::workbench::controller::{
    PresentationWorkbenchController, PresentationWorkbenchControllerContext,
    PresentationWorkbenchControllerEnumAction, PresentationWorkbenchControllerEnumAdmission,
    PresentationWorkbenchControllerEnumBusy, PresentationWorkbenchControllerError,
    PresentationWorkbenchControllerIntent,
};
use crate::presentation::workbench::snapshot::{
    PresentationWorkbenchJobSnapshot, PresentationWorkbenchSnapshot,
};
use crate::presentation::workbench::view::PresentationWorkbenchView;
use crate::workbench::job::{WorkbenchJobError, WorkbenchJobId};

#[derive(Debug, Error)]
pub enum PresentationEnginePanelsError {
    #[error(transparent)]
    Identity(#[from] EngineSearchIdentityError),
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
    #[error(transparent)]
    Analysis(#[from] PresentationAnalysisControllerError),
    #[error(transparent)]
    AnalysisView(#[from] PresentationAnalysisViewError),
    #[error(transparent)]
    Workbench(#[from] PresentationWorkbenchControllerError),
    #[error(transparent)]
    JobIdentity(#[from] WorkbenchJobError),
    #[error("The engine returned an unexpected response for its pending action")]
    Response,
    #[error("Wait for the pending engine action before submitting another")]
    Pending,
    #[error("The authoritative analysis selection is unavailable")]
    Selection,
    #[error("The native engine event owner is unavailable")]
    Disconnected(#[source] watch::error::RecvError),
    #[error("The native engine terminal event owner is unavailable")]
    TerminalDisconnected(#[source] mpsc::error::TryRecvError),
    #[error("Cannot capture the engine snapshot: {0}")]
    Snapshot(#[from] crate::engine::capabilities::EngineCapabilitiesError),
    #[error("Cannot capture diagnostic progress: {0}")]
    Progress(#[from] crate::clock::duration::ClockDurationError),
}

#[derive(Clone)]
pub struct PresentationEnginePanelsJobUpdate {
    pub context: DesktopEngineCommandsContext,
    pub request: DesktopRequestId,
    pub snapshot: PresentationWorkbenchJobSnapshot,
}

pub struct PresentationEnginePanelsConnection {
    pub owner: DesktopMessagesEntityId,
    pub initial: DesktopEngineCommandsSnapshot,
    pub selected: watch::Sender<Option<GameId>>,
    pub telemetry: watch::Receiver<Option<EngineSearchSessionSnapshot>>,
    pub terminal: mpsc::Receiver<DesktopEngineBridgeEnumEvent>,
    pub workbench: watch::Receiver<Option<PresentationEnginePanelsJobUpdate>>,
    pub admission: watch::Receiver<PresentationWorkbenchControllerEnumAdmission>,
    pub health: watch::Receiver<Option<DesktopFailure>>,
    pub tablebase_setup: Result<(), DesktopFailure>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PresentationEnginePanelsEnumTab {
    Analysis,
    Workbench,
}

#[derive(Clone, Copy)]
enum PresentationEnginePanelsEnumPending {
    Analysis(DesktopRequestId),
    Configure(DesktopRequestId, PresentationWorkbenchControllerContext),
    SelectTablebases(DesktopRequestId, PresentationWorkbenchControllerContext),
    PrepareTablebases(DesktopRequestId, PresentationWorkbenchControllerContext),
    WorkbenchStart(DesktopRequestId, PresentationWorkbenchControllerContext),
    WorkbenchCancel(DesktopRequestId, PresentationWorkbenchControllerContext),
    ExportPreview(
        DesktopRequestId,
        PresentationWorkbenchControllerContext,
        WorkbenchJobId,
    ),
    ExportDestination(
        DesktopRequestId,
        PresentationWorkbenchControllerContext,
        WorkbenchJobId,
    ),
    ExportApply(
        DesktopRequestId,
        PresentationWorkbenchControllerContext,
        WorkbenchJobId,
        DesktopRequestId,
    ),
    Refresh,
    Retry,
    Invalidate,
}

struct PresentationEnginePanelsPending {
    request: DesktopRequest,
    receipt: Option<DesktopApplicationReceipt>,
    action: PresentationEnginePanelsEnumPending,
}

pub struct PresentationEnginePanels {
    connection: PresentationEnginePanelsConnection,
    snapshot: DesktopEngineCommandsSnapshot,
    analysis: Option<PresentationAnalysisController>,
    analysis_view: PresentationAnalysisView,
    workbench: Option<PresentationWorkbenchController>,
    workbench_request: Option<DesktopRequestId>,
    workbench_view: PresentationWorkbenchView,
    pending: Option<PresentationEnginePanelsPending>,
    control: Option<PresentationEnginePanelsPending>,
    invalidation: Option<PresentationEnginePanelsPending>,
    job_update: Option<PresentationEnginePanelsJobUpdate>,
    pending_start: Option<(
        DesktopRequestId,
        DesktopRequestId,
        PresentationWorkbenchControllerContext,
        WorkbenchJobId,
    )>,
    export: Option<Arc<DesktopWorkbenchExportPreview>>,
    export_context: Option<(
        DesktopRequestId,
        PresentationWorkbenchControllerContext,
        WorkbenchJobId,
    )>,
    tab: PresentationEnginePanelsEnumTab,
    failure: Option<String>,
    refresh_frame: Option<u64>,
}

impl PresentationEnginePanels {
    pub fn new(connection: PresentationEnginePanelsConnection) -> Self {
        let mut workbench_view = PresentationWorkbenchView::new();
        workbench_view
            .set_tablebase_selection_failure(connection.tablebase_setup.as_ref().err().cloned());
        Self {
            snapshot: connection.initial.clone(),
            connection,
            analysis: None,
            analysis_view: PresentationAnalysisView::new(),
            workbench: None,
            workbench_request: None,
            workbench_view,
            pending: None,
            control: None,
            invalidation: None,
            job_update: None,
            pending_start: None,
            export: None,
            export_context: None,
            tab: PresentationEnginePanelsEnumTab::Analysis,
            failure: None,
            refresh_frame: None,
        }
    }

    pub fn selection_from(
        snapshot: &DesktopGameSnapshot,
    ) -> Result<Option<PresentationAnalysisControllerSelection>, EngineSearchIdentityError> {
        let workspace = match snapshot.phase() {
            GameEnumPhase::Analysis => PresentationAnalysisControllerEnumWorkspace::Analysis,
            GameEnumPhase::Completed => PresentationAnalysisControllerEnumWorkspace::Review,
            GameEnumPhase::Setup | GameEnumPhase::Active | GameEnumPhase::Interrupted => {
                return Ok(None);
            }
        };
        let DesktopGameSnapshotEnumAnalysis::Available(projection) = snapshot.analysis() else {
            return Ok(None);
        };
        if projection.node() != snapshot.view() {
            return Ok(None);
        }
        Ok(Some(PresentationAnalysisControllerSelection::new(
            EngineSearchContext::new(
                EngineSearchGameId::try_from(snapshot.id().as_uuid())?,
                EngineSearchRevision::new(snapshot.revision().value()),
            ),
            projection.position().clone(),
            workspace,
        )))
    }

    pub fn synchronize<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        selected: Option<&DesktopGameSnapshot>,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationEnginePanelsError> {
        self.poll(port)?;
        self.connection.selected.send_if_modified(|current| {
            let next = selected.map(DesktopGameSnapshot::id);
            if *current == next {
                false
            } else {
                *current = next;
                true
            }
        });
        self.workbench_view
            .set_recovered_paused(selected.is_some_and(|snapshot| {
                matches!(
                    snapshot.interruption(),
                    Some(GameSessionEnumInterruption::Recovered(_))
                )
            }));
        let selection = selected.map(Self::selection_from).transpose()?.flatten();
        if selected.is_some_and(|snapshot| {
            matches!(
                snapshot.analysis(),
                DesktopGameSnapshotEnumAnalysis::Unavailable(
                    DesktopGameSnapshotEnumAnalysisUnavailable::OversizedHistory
                )
            )
        }) {
            self.failure =
                Some("The selected position history exceeds the engine command limit".to_owned());
        }
        let changed = match (&self.analysis, &selection) {
            (Some(current), Some(next)) => {
                current.selection().context() != next.context()
                    || current.selection().workspace() != next.workspace()
                    || current.selection().position().initial() != next.position().initial()
                    || current.selection().position().history() != next.position().history()
            }
            (None, None) => false,
            (Some(_), None) | (None, Some(_)) => true,
        };
        if changed {
            if self.invalidation.is_some() {
                return Err(PresentationEnginePanelsError::Pending);
            }
            if let Some(current) = &self.analysis
                && let Some(metadata) = current.metadata().filter(|_| current.terminal().is_none())
            {
                self.invalidation = Some(Self::prepare(
                    port,
                    self.context(current.selection().context()),
                    DesktopEngineCommandsEnumOperation::Stop {
                        tag: metadata.tag(),
                    },
                    PresentationEnginePanelsEnumPending::Invalidate,
                )?);
            }
            if self.invalidation.is_none()
                && let Some(controller) = &self.workbench
                && let Some(job) = controller.job().filter(|job| !job.is_terminal())
            {
                self.invalidation = Some(Self::prepare(
                    port,
                    self.context(controller.context().selection()),
                    DesktopEngineCommandsEnumOperation::Workbench(
                        DesktopWorkbenchCommandsEnumOperation::Cancel { job: job.id() },
                    ),
                    PresentationEnginePanelsEnumPending::Invalidate,
                )?);
            }
            if let Some(pending) = &self.pending
                && pending.receipt.is_none()
            {
                self.pending = None;
            }
            self.analysis = selection.map(PresentationAnalysisController::new);
            self.workbench = self.analysis.as_ref().map(|analysis| {
                PresentationWorkbenchController::new(
                    self.workbench_context(analysis.selection().context()),
                    Some(analysis.selection().position().clone()),
                )
            });
            self.workbench_request = None;
            self.export = None;
            self.export_context = None;
            self.capture_engine()?;
        }
        if changed
            && self
                .invalidation
                .as_ref()
                .is_some_and(|pending| pending.receipt.is_none())
        {
            Self::admit(&mut self.invalidation, port)?;
        }
        self.poll(port)
    }

    fn context(&self, selection: EngineSearchContext) -> DesktopEngineCommandsContext {
        DesktopEngineCommandsContext::new(
            self.connection.owner,
            self.snapshot.generation(),
            self.snapshot.configuration(),
            selection,
        )
    }

    fn workbench_context(
        &self,
        selection: EngineSearchContext,
    ) -> PresentationWorkbenchControllerContext {
        PresentationWorkbenchControllerContext::new(
            self.snapshot.generation(),
            self.snapshot.configuration(),
            selection,
        )
    }

    fn retry_engine_information<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationEnginePanelsError> {
        let Some(analysis) = &self.analysis else {
            return Ok(());
        };
        let selection = analysis.selection().context();
        let position = analysis.selection().position().clone();
        let context = self.workbench_context(selection);
        self.workbench = Some(PresentationWorkbenchController::new(
            context,
            Some(position),
        ));
        self.workbench_request = None;
        self.capture_engine()?;
        if self.pending.is_none() {
            self.pending = Some(Self::prepare(
                port,
                self.context(selection),
                DesktopEngineCommandsEnumOperation::Retry,
                PresentationEnginePanelsEnumPending::Retry,
            )?);
            Self::admit(&mut self.pending, port)?;
        }
        Ok(())
    }

    fn capture_engine(&mut self) -> Result<(), PresentationEnginePanelsError> {
        let context = self
            .analysis
            .as_ref()
            .map(|analysis| self.workbench_context(analysis.selection().context()));
        if let Some(workbench) = &mut self.workbench {
            if let Some(context) = context {
                let position = self
                    .analysis
                    .as_ref()
                    .map(|analysis| analysis.selection().position().clone());
                if workbench.context() != context {
                    self.workbench_request = None;
                    workbench.replace_context(context, position);
                }
            }
            let context = workbench.context();
            workbench.receive_engine(
                context,
                PresentationWorkbenchSnapshot::capture(
                    self.snapshot.package(),
                    self.snapshot.capabilities(),
                    self.snapshot.applied(),
                    self.snapshot.tablebases(),
                )?,
            )?;
        }
        Ok(())
    }

    fn owns_workbench_request(
        &self,
        request: DesktopRequestId,
        context: PresentationWorkbenchControllerContext,
    ) -> bool {
        self.workbench_request == Some(request)
            && self.workbench.as_ref().is_some_and(|controller| {
                controller.context() == context && controller.is_pending()
            })
    }

    fn invalidate_job<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        context: PresentationWorkbenchControllerContext,
        job: WorkbenchJobId,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<bool, PresentationEnginePanelsError> {
        if let Some(pending) = &self.invalidation {
            return Ok(matches!(
                pending.request.command().operation(),
                DesktopEnumCommand::Engine(operation)
                    if matches!(operation.action(),
                        DesktopEngineCommandsEnumOperation::Workbench(
                            DesktopWorkbenchCommandsEnumOperation::Cancel { job: current }
                        ) if *current == job)
            ));
        }
        self.invalidation = Some(Self::prepare(
            port,
            self.context(context.selection()),
            DesktopEngineCommandsEnumOperation::Workbench(
                DesktopWorkbenchCommandsEnumOperation::Cancel { job },
            ),
            PresentationEnginePanelsEnumPending::Invalidate,
        )?);
        Self::admit(&mut self.invalidation, port)?;
        Ok(true)
    }

    fn prepare<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        port: &DesktopApplicationPort<C, W>,
        context: DesktopEngineCommandsContext,
        action: DesktopEngineCommandsEnumOperation,
        pending: PresentationEnginePanelsEnumPending,
    ) -> Result<PresentationEnginePanelsPending, PresentationEnginePanelsError> {
        let request = port.request(DesktopCommand::new(
            DesktopEnumCommand::Engine(DesktopEngineCommandsOperation::new(context, action)),
            Some(context.configuration()),
        ))?;
        Ok(PresentationEnginePanelsPending {
            request,
            receipt: None,
            action: pending,
        })
    }

    fn prepare_file<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &self,
        port: &DesktopApplicationPort<C, W>,
        purpose: DesktopFileEnumPurpose,
        action: PresentationEnginePanelsEnumPending,
    ) -> Result<PresentationEnginePanelsPending, PresentationEnginePanelsError> {
        let request = port.request(DesktopCommand::new(
            DesktopEnumCommand::FileSelect {
                owner: self.connection.owner,
                purpose,
            },
            Some(DesktopRevision::ZERO),
        ))?;
        Ok(PresentationEnginePanelsPending {
            request,
            receipt: None,
            action,
        })
    }

    fn admit<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        pending: &mut Option<PresentationEnginePanelsPending>,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), DesktopApplicationError> {
        if let Some(pending) = pending
            && pending.receipt.is_none()
        {
            pending.receipt = Some(port.try_submit(&pending.request)?);
        }
        Ok(())
    }

    pub fn has_pending_work(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.receipt.is_some())
            || self
                .invalidation
                .as_ref()
                .is_some_and(|pending| pending.receipt.is_some())
            || self
                .control
                .as_ref()
                .is_some_and(|pending| pending.receipt.is_some())
    }

    pub fn analysis(&self) -> Option<&PresentationAnalysisController> {
        self.analysis.as_ref()
    }
    pub fn workbench(&self) -> Option<&PresentationWorkbenchController> {
        self.workbench.as_ref()
    }
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub fn poll<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationEnginePanelsError> {
        if let Some(failure) = self.connection.health.borrow_and_update().clone() {
            self.failure = Some(failure.message().to_owned());
            if let Some(controller) = &mut self.workbench {
                controller.engine_unavailable(controller.context(), failure)?;
            }
        }
        let completed_control = self.control.as_ref().and_then(|pending| {
            pending
                .receipt
                .as_ref()
                .and_then(DesktopApplicationReceipt::response)
                .cloned()
        });
        if let Some(response) = completed_control {
            let pending = self
                .control
                .take()
                .ok_or(PresentationEnginePanelsError::Response)?;
            self.receive_response(pending.action, pending.request.request_id(), response, port)?;
        }
        let completed_invalidation = self.invalidation.as_ref().and_then(|pending| {
            pending
                .receipt
                .as_ref()
                .and_then(DesktopApplicationReceipt::response)
                .cloned()
        });
        if let Some(response) = completed_invalidation {
            let pending = self
                .invalidation
                .take()
                .ok_or(PresentationEnginePanelsError::Response)?;
            self.receive_response(pending.action, pending.request.request_id(), response, port)?;
        }
        let completed = self.pending.as_ref().and_then(|pending| {
            pending
                .receipt
                .as_ref()
                .and_then(DesktopApplicationReceipt::response)
                .cloned()
        });
        if let Some(response) = completed {
            let pending = self
                .pending
                .take()
                .ok_or(PresentationEnginePanelsError::Response)?;
            self.receive_response(pending.action, pending.request.request_id(), response, port)?;
        }
        if self
            .connection
            .workbench
            .has_changed()
            .map_err(PresentationEnginePanelsError::Disconnected)?
        {
            self.job_update = self.connection.workbench.borrow_and_update().clone();
        }
        if let Some((request, native, context, job)) = self.pending_start {
            if !self.owns_workbench_request(request, context) {
                if self.invalidate_job(context, job, port)? {
                    self.pending_start = None;
                }
            } else if let Some(update) = self
                .job_update
                .as_ref()
                .filter(|update| update.request == native && update.snapshot.id() == job)
                && let Some(controller) = &mut self.workbench
            {
                controller.accept_job(context, request, update.snapshot.clone())?;
                self.pending_start = None;
                self.job_update = None;
                self.failure = None;
            }
        }
        let awaiting_start = self.pending.as_ref().is_some_and(|pending| {
            matches!(
                pending.action,
                PresentationEnginePanelsEnumPending::Analysis(_)
                    | PresentationEnginePanelsEnumPending::WorkbenchStart(_, _)
            )
        }) || self.pending_start.is_some();
        if awaiting_start {
            return self.set_admission();
        }
        if self
            .connection
            .telemetry
            .has_changed()
            .map_err(PresentationEnginePanelsError::Disconnected)?
        {
            let snapshot = self.connection.telemetry.borrow_and_update().clone();
            if let Some(snapshot) = snapshot
                && let Some(analysis) = &mut self.analysis
            {
                analysis.observe_event(&EngineSearchSessionEnumEvent::Snapshot(snapshot))?;
            }
        }
        for _ in 0..crate::desktop::engine_bridge::DesktopEngineBridgePort::EVENT_CAPACITY {
            match self.connection.terminal.try_recv() {
                Ok(event) => self.receive_event(event, port)?,
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(error @ mpsc::error::TryRecvError::Disconnected) => {
                    return Err(PresentationEnginePanelsError::TerminalDisconnected(error));
                }
            }
        }
        if self
            .connection
            .workbench
            .has_changed()
            .map_err(PresentationEnginePanelsError::Disconnected)?
        {
            self.job_update = self.connection.workbench.borrow_and_update().clone();
        }
        self.receive_job(port)?;
        self.set_admission()?;
        Ok(())
    }

    fn set_admission(&mut self) -> Result<(), PresentationEnginePanelsError> {
        if let Some(controller) = &mut self.workbench {
            let admission = if self.connection.health.borrow().is_some() {
                PresentationWorkbenchControllerEnumAdmission::Unavailable
            } else if self.analysis.as_ref().is_some_and(|analysis| {
                analysis.metadata().is_some() && analysis.terminal().is_none()
            }) {
                PresentationWorkbenchControllerEnumAdmission::Busy(
                    PresentationWorkbenchControllerEnumBusy::Search,
                )
            } else {
                *self.connection.admission.borrow_and_update()
            };
            controller.set_admission(controller.context(), admission)?;
        }
        Ok(())
    }

    fn receive_job<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationEnginePanelsError> {
        let Some(update) = &self.job_update else {
            return Ok(());
        };
        let context = PresentationWorkbenchControllerContext::new(
            update.context.generation(),
            update.context.configuration(),
            update.context.selection(),
        );
        if let Some(controller) = &mut self.workbench
            && controller.context() == context
            && controller
                .job()
                .is_some_and(|job| job.id() == update.snapshot.id())
        {
            controller.receive_job(context, update.snapshot.clone())?;
            self.job_update = None;
        } else if update.snapshot.is_terminal()
            || self.invalidate_job(context, update.snapshot.id(), port)?
        {
            self.job_update = None;
        }
        Ok(())
    }

    fn receive_event<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        event: DesktopEngineBridgeEnumEvent,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationEnginePanelsError> {
        match event {
            DesktopEngineBridgeEnumEvent::Analysis(event) => {
                if let Some(analysis) = &mut self.analysis {
                    analysis.observe_event(&event)?;
                }
            }
            DesktopEngineBridgeEnumEvent::Workbench(result) => {
                let snapshot = PresentationWorkbenchJobSnapshot::capture(result.job())?;
                if let Some(update) = &mut self.job_update
                    && update.snapshot.id() == snapshot.id()
                {
                    update.snapshot = snapshot.clone();
                }
                if let Some(controller) = &mut self.workbench
                    && controller
                        .job()
                        .is_some_and(|job| job.id() == snapshot.id())
                {
                    controller.receive_job(controller.context(), snapshot)?;
                }
            }
            DesktopEngineBridgeEnumEvent::Failure(failure) => {
                self.failure = Some(failure.message().to_owned())
            }
        }
        if self.pending.is_none()
            && let Some(analysis) = &self.analysis
        {
            self.pending = Some(Self::prepare(
                port,
                self.context(analysis.selection().context()),
                DesktopEngineCommandsEnumOperation::Snapshot,
                PresentationEnginePanelsEnumPending::Refresh,
            )?);
            Self::admit(&mut self.pending, port)?;
        }
        Ok(())
    }

    fn receive_response<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        action: PresentationEnginePanelsEnumPending,
        native_request: DesktopRequestId,
        response: DesktopMessagesEnumResult,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationEnginePanelsError> {
        match response {
            DesktopMessagesEnumResult::Failure(failure) => self.reject(action, failure),
            DesktopMessagesEnumResult::Value {
                value: DesktopEnumValue::Engine(result),
                ..
            } => self.receive_engine_response(action, native_request, *result, port),
            DesktopMessagesEnumResult::Value {
                value: DesktopEnumValue::FileSelect(handle),
                ..
            } => {
                match action {
                    PresentationEnginePanelsEnumPending::SelectTablebases(request, context) => {
                        if !self.owns_workbench_request(request, context) {
                            return Ok(());
                        }
                        if let Some(directory) = handle {
                            self.pending = Some(Self::prepare(
                                port,
                                self.context(context.selection()),
                                DesktopEngineCommandsEnumOperation::PrepareTablebases { directory },
                                PresentationEnginePanelsEnumPending::PrepareTablebases(
                                    request, context,
                                ),
                            )?);
                            Self::admit(&mut self.pending, port)?;
                        } else if let Some(controller) = &mut self.workbench {
                            controller.selection_cancelled(context, request)?;
                        }
                    }
                    PresentationEnginePanelsEnumPending::ExportDestination(
                        request,
                        context,
                        job,
                    ) => {
                        if !self.owns_workbench_request(request, context) {
                            return Ok(());
                        }
                        if let Some(destination) = handle {
                            let preview = self
                                .export
                                .as_ref()
                                .ok_or(PresentationEnginePanelsError::Response)?;
                            self.pending = Some(Self::prepare(
                                port,
                                self.context(context.selection()),
                                DesktopEngineCommandsEnumOperation::Workbench(
                                    DesktopWorkbenchCommandsEnumOperation::ApplyExport {
                                        preview: preview.id(),
                                        digest: preview.digest(),
                                        destination,
                                    },
                                ),
                                PresentationEnginePanelsEnumPending::ExportApply(
                                    request,
                                    context,
                                    job,
                                    preview.id(),
                                ),
                            )?);
                            Self::admit(&mut self.pending, port)?;
                        } else {
                            self.reject(action, Self::cancelled())?;
                            self.export = None;
                            self.export_context = None;
                        }
                    }
                    PresentationEnginePanelsEnumPending::Analysis(_)
                    | PresentationEnginePanelsEnumPending::Configure(_, _)
                    | PresentationEnginePanelsEnumPending::PrepareTablebases(_, _)
                    | PresentationEnginePanelsEnumPending::WorkbenchStart(_, _)
                    | PresentationEnginePanelsEnumPending::WorkbenchCancel(_, _)
                    | PresentationEnginePanelsEnumPending::ExportPreview(_, _, _)
                    | PresentationEnginePanelsEnumPending::ExportApply(_, _, _, _)
                    | PresentationEnginePanelsEnumPending::Refresh
                    | PresentationEnginePanelsEnumPending::Retry
                    | PresentationEnginePanelsEnumPending::Invalidate => {
                        return Err(PresentationEnginePanelsError::Response);
                    }
                }
                Ok(())
            }
            DesktopMessagesEnumResult::Cancelled => self.reject(action, Self::cancelled()),
            DesktopMessagesEnumResult::Value { .. }
            | DesktopMessagesEnumResult::LibraryFailure(_)
            | DesktopMessagesEnumResult::EditorFailure(_)
            | DesktopMessagesEnumResult::Stopped
            | DesktopMessagesEnumResult::Paused
            | DesktopMessagesEnumResult::QuitReady
            | DesktopMessagesEnumResult::ReconciliationRequired => {
                Err(PresentationEnginePanelsError::Response)
            }
        }
    }

    fn receive_engine_response<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        action: PresentationEnginePanelsEnumPending,
        native_request: DesktopRequestId,
        result: DesktopEngineBridgeEnumResult,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationEnginePanelsError> {
        match (action, result) {
            (
                PresentationEnginePanelsEnumPending::Analysis(request),
                DesktopEngineBridgeEnumResult::Started(metadata),
            ) => {
                let accepted = if let Some(controller) = &mut self.analysis {
                    match controller.observe_started(request, &metadata) {
                        Ok(()) => true,
                        Err(PresentationAnalysisControllerError::StaleResult) => false,
                        Err(error) => return Err(error.into()),
                    }
                } else {
                    false
                };
                if !accepted {
                    self.invalidation = Some(Self::prepare(
                        port,
                        self.context(metadata.tag().context()),
                        DesktopEngineCommandsEnumOperation::Stop {
                            tag: metadata.tag(),
                        },
                        PresentationEnginePanelsEnumPending::Invalidate,
                    )?);
                    Self::admit(&mut self.invalidation, port)?;
                }
            }
            (
                PresentationEnginePanelsEnumPending::Analysis(_),
                DesktopEngineBridgeEnumResult::Stopped(terminal),
            ) => {
                if let Some(controller) = &mut self.analysis {
                    controller.observe_terminal(&terminal)?;
                }
            }
            (
                PresentationEnginePanelsEnumPending::Configure(request, context),
                DesktopEngineBridgeEnumResult::Snapshot(snapshot),
            ) => {
                if self.owns_workbench_request(request, context)
                    && let Some(controller) = &mut self.workbench
                {
                    controller.configuration_applied(
                        context,
                        request,
                        snapshot.configuration(),
                        PresentationWorkbenchSnapshot::capture(
                            snapshot.package(),
                            snapshot.capabilities(),
                            snapshot.applied(),
                            snapshot.tablebases(),
                        )?,
                    )?;
                }
                self.snapshot = snapshot;
                self.capture_engine()?;
            }
            (
                PresentationEnginePanelsEnumPending::PrepareTablebases(request, context),
                DesktopEngineBridgeEnumResult::Snapshot(snapshot),
            ) => {
                if self.owns_workbench_request(request, context)
                    && let Some(controller) = &mut self.workbench
                {
                    controller.tablebases_selected(
                        context,
                        request,
                        PresentationWorkbenchSnapshot::capture(
                            snapshot.package(),
                            snapshot.capabilities(),
                            snapshot.applied(),
                            snapshot.tablebases(),
                        )?,
                    )?;
                }
                self.snapshot = snapshot;
                self.capture_engine()?;
            }
            (
                PresentationEnginePanelsEnumPending::WorkbenchStart(request, context),
                DesktopEngineBridgeEnumResult::WorkbenchStarted(job),
            ) => {
                self.pending_start = Some((request, native_request, context, job));
            }
            (
                PresentationEnginePanelsEnumPending::WorkbenchCancel(_, _),
                DesktopEngineBridgeEnumResult::WorkbenchCancelled(_),
            ) => {}
            (
                PresentationEnginePanelsEnumPending::ExportPreview(request, context, job),
                DesktopEngineBridgeEnumResult::ExportPreview(preview),
            ) if preview.id() == native_request && preview.job() == job => {
                if self.owns_workbench_request(request, context) {
                    self.export = Some(preview);
                    self.export_context = Some((request, context, job));
                }
            }
            (
                PresentationEnginePanelsEnumPending::ExportApply(request, context, job, expected),
                DesktopEngineBridgeEnumResult::Exported { preview, .. },
            ) if expected == preview => {
                if self.owns_workbench_request(request, context)
                    && let Some(controller) = &mut self.workbench
                {
                    controller.exported(context, request, job)?;
                }
                self.export = None;
                self.export_context = None;
            }
            (
                PresentationEnginePanelsEnumPending::Refresh
                | PresentationEnginePanelsEnumPending::Retry,
                DesktopEngineBridgeEnumResult::Snapshot(snapshot),
            ) => {
                self.snapshot = snapshot;
                self.capture_engine()?;
            }
            (
                PresentationEnginePanelsEnumPending::Invalidate,
                DesktopEngineBridgeEnumResult::Stopped(_)
                | DesktopEngineBridgeEnumResult::WorkbenchCancelled(_),
            ) => {}
            _ => return Err(PresentationEnginePanelsError::Response),
        }
        Ok(())
    }

    fn reject(
        &mut self,
        action: PresentationEnginePanelsEnumPending,
        failure: DesktopFailure,
    ) -> Result<(), PresentationEnginePanelsError> {
        match action {
            PresentationEnginePanelsEnumPending::Analysis(request) => {
                self.failure = Some(failure.message().to_owned());
                if let Some(controller) = &mut self.analysis {
                    match controller.observe_failure(request, failure) {
                        Ok(()) | Err(PresentationAnalysisControllerError::StaleResult) => {}
                        Err(error) => return Err(error.into()),
                    }
                }
            }
            PresentationEnginePanelsEnumPending::Configure(request, context)
            | PresentationEnginePanelsEnumPending::SelectTablebases(request, context)
            | PresentationEnginePanelsEnumPending::PrepareTablebases(request, context)
            | PresentationEnginePanelsEnumPending::WorkbenchStart(request, context)
            | PresentationEnginePanelsEnumPending::WorkbenchCancel(request, context)
            | PresentationEnginePanelsEnumPending::ExportPreview(request, context, _)
            | PresentationEnginePanelsEnumPending::ExportDestination(request, context, _)
            | PresentationEnginePanelsEnumPending::ExportApply(request, context, _, _) => {
                if self.owns_workbench_request(request, context)
                    && let Some(controller) = &mut self.workbench
                {
                    self.failure = Some(failure.message().to_owned());
                    controller.reject(context, request, failure)?;
                }
            }
            PresentationEnginePanelsEnumPending::Refresh
            | PresentationEnginePanelsEnumPending::Retry
            | PresentationEnginePanelsEnumPending::Invalidate => {
                self.failure = Some(failure.message().to_owned());
            }
        }
        Ok(())
    }

    pub fn observe_value(
        &mut self,
        result: DesktopEngineBridgeEnumResult,
    ) -> Result<(), PresentationEnginePanelsError> {
        match result {
            DesktopEngineBridgeEnumResult::Snapshot(snapshot) => {
                self.snapshot = snapshot;
                self.capture_engine()
            }
            DesktopEngineBridgeEnumResult::WorkbenchResult(result) => {
                if let Some(controller) = &mut self.workbench
                    && controller
                        .job()
                        .is_some_and(|job| job.id() == result.job().id())
                {
                    controller.receive_job(
                        controller.context(),
                        PresentationWorkbenchJobSnapshot::capture(result.job())?,
                    )?;
                }
                Ok(())
            }
            DesktopEngineBridgeEnumResult::Started(_)
            | DesktopEngineBridgeEnumResult::Stopped(_)
            | DesktopEngineBridgeEnumResult::WorkbenchStarted(_)
            | DesktopEngineBridgeEnumResult::WorkbenchCancelled(_)
            | DesktopEngineBridgeEnumResult::ExportPreview(_)
            | DesktopEngineBridgeEnumResult::Exported { .. } => {
                Err(PresentationEnginePanelsError::Response)
            }
        }
    }

    fn cancelled() -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::Cancelled,
            "The native selection was cancelled".to_owned(),
            true,
        )
    }

    fn submit_analysis<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        intent: PresentationAnalysisControllerIntent,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationEnginePanelsError> {
        let action = match intent.action() {
            PresentationAnalysisControllerEnumAction::Start(limit) => {
                DesktopEngineCommandsEnumOperation::Start {
                    position: intent.selection().position().clone(),
                    limit: *limit,
                }
            }
            PresentationAnalysisControllerEnumAction::Stop(tag) => {
                DesktopEngineCommandsEnumOperation::Stop { tag: *tag }
            }
        };
        let control = action.is_control();
        let pending = Self::prepare(
            port,
            self.context(intent.selection().context()),
            action,
            PresentationEnginePanelsEnumPending::Analysis(intent.request_id()),
        )?;
        if control {
            self.control = Some(pending);
            Self::admit(&mut self.control, port)?;
        } else {
            self.pending = Some(pending);
            Self::admit(&mut self.pending, port)?;
        }
        Ok(())
    }

    fn submit_workbench<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        intent: PresentationWorkbenchControllerIntent,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationEnginePanelsError> {
        let request = intent.request();
        let context = intent.context();
        self.workbench_request = Some(request);
        let backend = self.context(context.selection());
        let (action, pending) = match intent.into_action() {
            PresentationWorkbenchControllerEnumAction::Configure(setting) => (
                DesktopEngineCommandsEnumOperation::Configure(setting),
                PresentationEnginePanelsEnumPending::Configure(request, context),
            ),
            PresentationWorkbenchControllerEnumAction::SelectTablebaseDirectories => {
                self.pending = Some(self.prepare_file(
                    port,
                    DesktopFileEnumPurpose::Directory,
                    PresentationEnginePanelsEnumPending::SelectTablebases(request, context),
                )?);
                Self::admit(&mut self.pending, port)?;
                return Ok(());
            }
            PresentationWorkbenchControllerEnumAction::ConfigureTablebases => (
                DesktopEngineCommandsEnumOperation::LoadTablebases,
                PresentationEnginePanelsEnumPending::Configure(request, context),
            ),
            PresentationWorkbenchControllerEnumAction::Start {
                parameters,
                deadline,
            } => (
                DesktopEngineCommandsEnumOperation::Workbench(
                    DesktopWorkbenchCommandsEnumOperation::Start {
                        parameters,
                        deadline,
                    },
                ),
                PresentationEnginePanelsEnumPending::WorkbenchStart(request, context),
            ),
            PresentationWorkbenchControllerEnumAction::Cancel { job } => (
                DesktopEngineCommandsEnumOperation::Workbench(
                    DesktopWorkbenchCommandsEnumOperation::Cancel { job },
                ),
                PresentationEnginePanelsEnumPending::WorkbenchCancel(request, context),
            ),
            PresentationWorkbenchControllerEnumAction::Export { job } => (
                DesktopEngineCommandsEnumOperation::Workbench(
                    DesktopWorkbenchCommandsEnumOperation::PreviewExport { job },
                ),
                PresentationEnginePanelsEnumPending::ExportPreview(request, context, job),
            ),
        };
        let control = action.is_control();
        let pending = Self::prepare(port, backend, action, pending)?;
        if control {
            self.control = Some(pending);
            Self::admit(&mut self.control, port)?;
        } else {
            self.pending = Some(pending);
            Self::admit(&mut self.pending, port)?;
        }
        Ok(())
    }

    pub fn show<C: ClockTimeProvider, W: DesktopApplicationWakeProvider>(
        &mut self,
        ui: &mut Ui,
        port: &DesktopApplicationPort<C, W>,
    ) -> Result<(), PresentationEnginePanelsError> {
        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.tab,
                PresentationEnginePanelsEnumTab::Analysis,
                "Analysis",
            );
            ui.selectable_value(
                &mut self.tab,
                PresentationEnginePanelsEnumTab::Workbench,
                "Workbench",
            );
        });
        if let Some(failure) = &self.failure {
            ui.colored_label(ui.visuals().error_fg_color, failure);
        }
        let frame = ui.ctx().cumulative_frame_nr();
        if (self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.receipt.is_none())
            || self
                .invalidation
                .as_ref()
                .is_some_and(|pending| pending.receipt.is_none())
            || self
                .control
                .as_ref()
                .is_some_and(|pending| pending.receipt.is_none()))
            && crate::presentation::theme::components::PresentationThemeEnumButton::Secondary
                .show(ui, "Retry Engine Action")
                .clicked()
            && self.refresh_frame != Some(frame)
        {
            self.refresh_frame = Some(frame);
            Self::admit(&mut self.invalidation, port)?;
            Self::admit(&mut self.control, port)?;
            Self::admit(&mut self.pending, port)?;
        }
        if self.control.is_none() && self.invalidation.is_none() {
            let setup = self.pending.as_ref().and_then(|pending| {
                if let PresentationEnginePanelsEnumPending::WorkbenchStart(_, context) =
                    pending.action
                {
                    Some((
                        pending.request.request_id(),
                        context,
                        pending.receipt.is_some(),
                    ))
                } else {
                    None
                }
            });
            if let Some((native, context, admitted)) = setup
                && crate::presentation::theme::components::PresentationThemeEnumButton::Ghost
                    .show(ui, "Cancel Diagnostic Setup")
                    .clicked()
            {
                if admitted {
                    let job = WorkbenchJobId::try_from(native.as_uuid())?;
                    self.control = Some(Self::prepare(
                        port,
                        self.context(context.selection()),
                        DesktopEngineCommandsEnumOperation::Workbench(
                            DesktopWorkbenchCommandsEnumOperation::Cancel { job },
                        ),
                        PresentationEnginePanelsEnumPending::Invalidate,
                    )?);
                    Self::admit(&mut self.control, port)?;
                } else if let Some(pending) = self.pending.take() {
                    self.reject(pending.action, Self::cancelled())?;
                }
            }
            if let Some(controller) = &mut self.analysis
                && controller.can_stop()
                && crate::presentation::theme::components::PresentationThemeEnumButton::Secondary
                    .show(ui, "Stop Active Analysis")
                    .clicked()
                && let Some(intent) = controller.stop(DesktopRequestId::new())?
            {
                self.submit_analysis(intent, port)?;
            }
            if self.control.is_none()
                && let Some(controller) = &mut self.workbench
                && controller.can_cancel()
                && let Some(job) = controller.job().map(|job| job.id())
                && crate::presentation::theme::components::PresentationThemeEnumButton::Ghost
                    .show(ui, "Cancel Active Diagnostic")
                    .clicked()
                && let Some(intent) = controller.cancel(job, DesktopRequestId::new())?
            {
                self.submit_workbench(intent, port)?;
            }
        }
        if self.control.is_some() {
            ui.label("Waiting for the native engine control and observed cleanup.");
        }
        if self.analysis.is_none() {
            ui.label("Select an Analysis or completed game. Engine assistance is unavailable during live play.");
            return Ok(());
        }
        if let Some((request, context, job)) = self.export_context {
            let preview = self
                .export
                .as_ref()
                .ok_or(PresentationEnginePanelsError::Response)?;
            ui.heading("Diagnostic export preview");
            ui.label(format!(
                "{} bytes, SHA-256 {}",
                preview.length(),
                preview.digest()
            ));
            ui.label(format!(
                "{} retained diagnostic lines; {} evicted lines and {} evicted bytes",
                preview.diagnostic_lines(),
                preview.evicted_lines(),
                preview.evicted_bytes()
            ));
            egui::ScrollArea::vertical()
                .max_height(160.0)
                .show(ui, |ui| {
                    ui.monospace(String::from_utf8_lossy(preview.bytes()));
                });
            if crate::presentation::theme::components::PresentationThemeEnumButton::Primary
                .show(ui, "Save Diagnostic Export")
                .clicked()
                && self.pending.is_none()
            {
                self.pending = Some(self.prepare_file(
                    port,
                    DesktopFileEnumPurpose::WriteFile,
                    PresentationEnginePanelsEnumPending::ExportDestination(request, context, job),
                )?);
                Self::admit(&mut self.pending, port)?;
            }
            if crate::presentation::theme::components::PresentationThemeEnumButton::Ghost
                .show(ui, "Cancel Diagnostic Export")
                .clicked()
                && self.pending.is_none()
            {
                self.reject(
                    PresentationEnginePanelsEnumPending::ExportPreview(request, context, job),
                    Self::cancelled(),
                )?;
                self.export = None;
                self.export_context = None;
            }
        }
        if self.snapshot.state() == EngineSearchEnumState::Failed
            && ui
                .add_enabled(self.pending.is_none(), egui::Button::new("Retry Engine"))
                .clicked()
            && self.refresh_frame != Some(frame)
        {
            self.refresh_frame = Some(frame);
            let selection = self
                .analysis
                .as_ref()
                .ok_or(PresentationEnginePanelsError::Selection)?
                .selection()
                .context();
            self.pending = Some(Self::prepare(
                port,
                self.context(selection),
                DesktopEngineCommandsEnumOperation::Retry,
                PresentationEnginePanelsEnumPending::Retry,
            )?);
            Self::admit(&mut self.pending, port)?;
        }
        match self.tab {
            PresentationEnginePanelsEnumTab::Analysis => {
                let controller = self
                    .analysis
                    .as_mut()
                    .ok_or(PresentationEnginePanelsError::Selection)?;
                let enabled = self.pending.is_none()
                    && self.control.is_none()
                    && self.invalidation.is_none()
                    && self.export.is_none()
                    && self.workbench.as_ref().is_none_or(|controller| {
                        !controller.is_pending()
                            && controller.job().is_none_or(|job| job.is_terminal())
                    });
                let intent = ui
                    .add_enabled_ui(enabled, |ui| self.analysis_view.show(ui, controller))
                    .inner?;
                if let Some(intent) = intent {
                    self.submit_analysis(intent, port)?;
                }
            }
            PresentationEnginePanelsEnumTab::Workbench => {
                if let Some(controller) = &mut self.workbench {
                    let enabled = self.pending.is_none()
                        && self.control.is_none()
                        && self.invalidation.is_none()
                        && self.export.is_none();
                    let intent = ui
                        .add_enabled_ui(enabled, |ui| self.workbench_view.show(ui, controller))
                        .inner;
                    if let Some(intent) = intent {
                        self.submit_workbench(intent, port)?;
                    }
                }
                if self.workbench_view.take_engine_retry() {
                    self.retry_engine_information(port)?;
                }
            }
        }
        Ok(())
    }
}
