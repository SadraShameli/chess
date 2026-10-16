use thiserror::Error;

use crate::desktop::error::{DesktopError, DesktopFailure};
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};
use crate::engine::options::{EngineOptionsEnumValue, EngineOptionsError, EngineOptionsSetting};
use crate::engine::package::selector::EnginePackageSelected;
use crate::engine::search::identity::{EngineSearchContext, EngineSearchGeneration};
use crate::engine::search::position::EngineSearchPosition;
use crate::presentation::workbench::snapshot::{
    PresentationWorkbenchJobSnapshot, PresentationWorkbenchSnapshot,
    PresentationWorkbenchTablebaseEnumState,
};
use crate::workbench::enums::{
    WorkbenchEnumJobState, WorkbenchEnumOperation, WorkbenchEnumParameters,
};
use crate::workbench::job::{WorkbenchJobDeadline, WorkbenchJobId};
use crate::workbench::parameters::{WorkbenchBenchParameters, WorkbenchParametersError};

#[derive(Debug, Error)]
pub enum PresentationWorkbenchControllerError {
    #[error(
        "The update belongs to another position, process generation, configuration revision, request or job"
    )]
    StaleContext,
    #[error("Wait for the engine identity and capability discovery to finish")]
    Loading,
    #[error("The engine is unavailable. Resolve its setup error and retry")]
    Unavailable,
    #[error("Wait for the pending workbench action to finish")]
    Pending,
    #[error("Engine maintenance and diagnostics require an idle engine: {0:?}")]
    Busy(PresentationWorkbenchControllerEnumBusy),
    #[error(
        "The diagnostic needs {required} CPU workers; only {available} are currently available"
    )]
    Capacity { required: usize, available: usize },
    #[error("The variant is derived from the current game. Choose a variant through New Game")]
    VariantOwned,
    #[error("Select tablebase directories through the native directory picker")]
    TablebaseSelectionRequired,
    #[error("Select and prepare tablebase directories before configuring the engine")]
    TablebasesUnprepared,
    #[error("There is no cancellable diagnostic job with this identity")]
    InactiveJob,
    #[error("A diagnostic must finish before its result can be exported")]
    IncompleteJob,
    #[error("The diagnostic parameters do not belong to the selected position")]
    PositionMismatch,
    #[error(transparent)]
    Options(#[from] EngineOptionsError),
    #[error(transparent)]
    Parameters(#[from] WorkbenchParametersError),
    #[error(transparent)]
    Revision(#[from] DesktopError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationWorkbenchControllerContext {
    generation: EngineSearchGeneration,
    configuration: DesktopRevision,
    selection: EngineSearchContext,
}

impl PresentationWorkbenchControllerContext {
    pub const fn new(
        generation: EngineSearchGeneration,
        configuration: DesktopRevision,
        selection: EngineSearchContext,
    ) -> Self {
        Self {
            generation,
            configuration,
            selection,
        }
    }
    pub const fn generation(self) -> EngineSearchGeneration {
        self.generation
    }
    pub const fn configuration(self) -> DesktopRevision {
        self.configuration
    }
    pub const fn selection(self) -> EngineSearchContext {
        self.selection
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationWorkbenchControllerEnumBusy {
    LiveGame,
    Search,
    Diagnostic,
    Configuration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationWorkbenchControllerEnumAdmission {
    Unavailable,
    Idle { available_workers: usize },
    Busy(PresentationWorkbenchControllerEnumBusy),
}

#[derive(Debug)]
pub enum PresentationWorkbenchControllerEnumAction {
    Configure(EngineOptionsSetting),
    SelectTablebaseDirectories,
    ConfigureTablebases,
    Start {
        parameters: WorkbenchEnumParameters,
        deadline: WorkbenchJobDeadline,
    },
    Cancel {
        job: WorkbenchJobId,
    },
    Export {
        job: WorkbenchJobId,
    },
}

#[derive(Debug)]
pub struct PresentationWorkbenchControllerIntent {
    context: PresentationWorkbenchControllerContext,
    request: DesktopRequestId,
    action: PresentationWorkbenchControllerEnumAction,
}

impl PresentationWorkbenchControllerIntent {
    pub const fn context(&self) -> PresentationWorkbenchControllerContext {
        self.context
    }
    pub const fn request(&self) -> DesktopRequestId {
        self.request
    }
    pub const fn action(&self) -> &PresentationWorkbenchControllerEnumAction {
        &self.action
    }
    pub fn into_action(self) -> PresentationWorkbenchControllerEnumAction {
        self.action
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationWorkbenchControllerEnumPending {
    Configure,
    SelectTablebases,
    ConfigureTablebases,
    Start(WorkbenchEnumOperation),
    Cancel(WorkbenchJobId),
    Export(WorkbenchJobId),
}

#[derive(Debug, Clone, Copy)]
struct PresentationWorkbenchControllerPending {
    request: DesktopRequestId,
    action: PresentationWorkbenchControllerEnumPending,
}

pub struct PresentationWorkbenchController {
    context: PresentationWorkbenchControllerContext,
    position: Option<EngineSearchPosition>,
    engine: Option<PresentationWorkbenchSnapshot>,
    admission: PresentationWorkbenchControllerEnumAdmission,
    job: Option<PresentationWorkbenchJobSnapshot>,
    pending: Option<PresentationWorkbenchControllerPending>,
    last_request: Option<DesktopRequestId>,
    failure: Option<DesktopFailure>,
    exported_job: Option<WorkbenchJobId>,
}

impl PresentationWorkbenchController {
    pub const fn new(
        context: PresentationWorkbenchControllerContext,
        position: Option<EngineSearchPosition>,
    ) -> Self {
        Self {
            context,
            position,
            engine: None,
            admission: PresentationWorkbenchControllerEnumAdmission::Unavailable,
            job: None,
            pending: None,
            last_request: None,
            failure: None,
            exported_job: None,
        }
    }

    pub const fn context(&self) -> PresentationWorkbenchControllerContext {
        self.context
    }
    pub const fn position(&self) -> Option<&EngineSearchPosition> {
        self.position.as_ref()
    }
    pub const fn engine(&self) -> Option<&PresentationWorkbenchSnapshot> {
        self.engine.as_ref()
    }
    pub const fn admission(&self) -> PresentationWorkbenchControllerEnumAdmission {
        self.admission
    }
    pub const fn job(&self) -> Option<&PresentationWorkbenchJobSnapshot> {
        self.job.as_ref()
    }
    pub const fn failure(&self) -> Option<&DesktopFailure> {
        self.failure.as_ref()
    }
    pub const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub const fn exported_job(&self) -> Option<WorkbenchJobId> {
        self.exported_job
    }

    pub fn replace_context(
        &mut self,
        context: PresentationWorkbenchControllerContext,
        position: Option<EngineSearchPosition>,
    ) {
        if context != self.context {
            *self = Self::new(context, position);
        }
    }

    pub fn receive_engine(
        &mut self,
        context: PresentationWorkbenchControllerContext,
        snapshot: PresentationWorkbenchSnapshot,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        self.check_context(context)?;
        self.check_package(snapshot.package())?;
        self.engine = Some(snapshot);
        self.failure = None;
        Ok(())
    }

    pub fn set_admission(
        &mut self,
        context: PresentationWorkbenchControllerContext,
        admission: PresentationWorkbenchControllerEnumAdmission,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        self.check_context(context)?;
        self.admission = admission;
        Ok(())
    }

    pub fn engine_unavailable(
        &mut self,
        context: PresentationWorkbenchControllerContext,
        failure: DesktopFailure,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        self.check_context(context)?;
        self.admission = PresentationWorkbenchControllerEnumAdmission::Unavailable;
        self.failure = Some(failure);
        Ok(())
    }

    pub fn can_configure(&self) -> bool {
        self.idle().is_ok()
    }

    pub fn can_cancel(&self) -> bool {
        self.pending.is_none()
            && self.job.as_ref().is_some_and(|job| {
                matches!(
                    job.state(),
                    WorkbenchEnumJobState::Queued
                        | WorkbenchEnumJobState::Admitted
                        | WorkbenchEnumJobState::Running
                )
            })
    }

    pub fn can_export(&self) -> bool {
        self.pending.is_none()
            && self
                .job
                .as_ref()
                .is_some_and(PresentationWorkbenchJobSnapshot::is_terminal)
    }

    pub fn check_parameters(
        &self,
        parameters: &WorkbenchEnumParameters,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        let available = self.idle()?;
        if parameters.workers() > available {
            return Err(PresentationWorkbenchControllerError::Capacity {
                required: parameters.workers(),
                available,
            });
        }
        match parameters {
            WorkbenchEnumParameters::Inspector(position)
            | WorkbenchEnumParameters::RawEvaluation(position) => self.check_position(position)?,
            WorkbenchEnumParameters::Perft(parameters) => {
                self.check_position(parameters.position())?
            }
            WorkbenchEnumParameters::Bench(parameters) => {
                WorkbenchBenchParameters::new(
                    self.ready()?.capabilities(),
                    Some(parameters.hash_mib()),
                    Some(parameters.workers()),
                    Some(parameters.depth()),
                )?;
            }
            WorkbenchEnumParameters::Compiler => {}
        }
        Ok(())
    }

    pub fn configure(
        &mut self,
        value: EngineOptionsEnumValue,
        request: DesktopRequestId,
    ) -> Result<Option<PresentationWorkbenchControllerIntent>, PresentationWorkbenchControllerError>
    {
        if self.last_request == Some(request) {
            return Ok(None);
        }
        let available = self.idle()?;
        match &value {
            EngineOptionsEnumValue::Chess960(_) => {
                return Err(PresentationWorkbenchControllerError::VariantOwned);
            }
            EngineOptionsEnumValue::SyzygyPath(_) => {
                return Err(PresentationWorkbenchControllerError::TablebaseSelectionRequired);
            }
            EngineOptionsEnumValue::Threads(threads) => {
                let required = usize::try_from(*threads).map_err(WorkbenchParametersError::from)?;
                if required > available {
                    return Err(PresentationWorkbenchControllerError::Capacity {
                        required,
                        available,
                    });
                }
            }
            EngineOptionsEnumValue::Hash(_)
            | EngineOptionsEnumValue::MoveOverhead(_)
            | EngineOptionsEnumValue::Minimal(_)
            | EngineOptionsEnumValue::ClearHash
            | EngineOptionsEnumValue::MultiPv(_) => {}
        }
        let setting = EngineOptionsSetting::new(value, self.ready()?.capabilities())?;
        Ok(Some(self.emit(
            request,
            PresentationWorkbenchControllerEnumAction::Configure(setting),
            PresentationWorkbenchControllerEnumPending::Configure,
        )))
    }

    pub fn select_tablebases(
        &mut self,
        request: DesktopRequestId,
    ) -> Result<Option<PresentationWorkbenchControllerIntent>, PresentationWorkbenchControllerError>
    {
        if self.last_request == Some(request) {
            return Ok(None);
        }
        self.idle()?;
        Ok(Some(self.emit(
            request,
            PresentationWorkbenchControllerEnumAction::SelectTablebaseDirectories,
            PresentationWorkbenchControllerEnumPending::SelectTablebases,
        )))
    }

    pub fn configure_tablebases(
        &mut self,
        request: DesktopRequestId,
    ) -> Result<Option<PresentationWorkbenchControllerIntent>, PresentationWorkbenchControllerError>
    {
        if self.last_request == Some(request) {
            return Ok(None);
        }
        self.idle()?;
        match self.ready()?.tablebases().state() {
            PresentationWorkbenchTablebaseEnumState::Unconfigured => {
                return Err(PresentationWorkbenchControllerError::TablebasesUnprepared);
            }
            PresentationWorkbenchTablebaseEnumState::Configured
            | PresentationWorkbenchTablebaseEnumState::Loaded(_)
            | PresentationWorkbenchTablebaseEnumState::Unavailable
            | PresentationWorkbenchTablebaseEnumState::Failed(_) => {}
        }
        Ok(Some(self.emit(
            request,
            PresentationWorkbenchControllerEnumAction::ConfigureTablebases,
            PresentationWorkbenchControllerEnumPending::ConfigureTablebases,
        )))
    }

    pub fn start(
        &mut self,
        parameters: WorkbenchEnumParameters,
        deadline: WorkbenchJobDeadline,
        request: DesktopRequestId,
    ) -> Result<Option<PresentationWorkbenchControllerIntent>, PresentationWorkbenchControllerError>
    {
        if self.last_request == Some(request) {
            return Ok(None);
        }
        self.check_parameters(&parameters)?;
        let operation = parameters.operation();
        Ok(Some(self.emit(
            request,
            PresentationWorkbenchControllerEnumAction::Start {
                parameters,
                deadline,
            },
            PresentationWorkbenchControllerEnumPending::Start(operation),
        )))
    }

    pub fn cancel(
        &mut self,
        job: WorkbenchJobId,
        request: DesktopRequestId,
    ) -> Result<Option<PresentationWorkbenchControllerIntent>, PresentationWorkbenchControllerError>
    {
        if self.last_request == Some(request) {
            return Ok(None);
        }
        if !self.can_cancel() || self.job.as_ref().is_none_or(|current| current.id() != job) {
            return Err(PresentationWorkbenchControllerError::InactiveJob);
        }
        Ok(Some(self.emit(
            request,
            PresentationWorkbenchControllerEnumAction::Cancel { job },
            PresentationWorkbenchControllerEnumPending::Cancel(job),
        )))
    }

    pub fn export(
        &mut self,
        job: WorkbenchJobId,
        request: DesktopRequestId,
    ) -> Result<Option<PresentationWorkbenchControllerIntent>, PresentationWorkbenchControllerError>
    {
        if self.last_request == Some(request) {
            return Ok(None);
        }
        if !self.can_export() || self.job.as_ref().is_none_or(|current| current.id() != job) {
            return Err(PresentationWorkbenchControllerError::IncompleteJob);
        }
        Ok(Some(self.emit(
            request,
            PresentationWorkbenchControllerEnumAction::Export { job },
            PresentationWorkbenchControllerEnumPending::Export(job),
        )))
    }

    pub fn configuration_applied(
        &mut self,
        context: PresentationWorkbenchControllerContext,
        request: DesktopRequestId,
        revision: DesktopRevision,
        snapshot: PresentationWorkbenchSnapshot,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        let pending = self.check_pending(context, request)?;
        if !matches!(
            pending,
            PresentationWorkbenchControllerEnumPending::Configure
                | PresentationWorkbenchControllerEnumPending::ConfigureTablebases
        ) || revision != self.context.configuration.next()?
        {
            return Err(PresentationWorkbenchControllerError::StaleContext);
        }
        self.check_package(snapshot.package())?;
        self.context.configuration = revision;
        self.engine = Some(snapshot);
        self.pending = None;
        self.failure = None;
        Ok(())
    }

    pub fn tablebases_selected(
        &mut self,
        context: PresentationWorkbenchControllerContext,
        request: DesktopRequestId,
        snapshot: PresentationWorkbenchSnapshot,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        if self.check_pending(context, request)?
            != PresentationWorkbenchControllerEnumPending::SelectTablebases
        {
            return Err(PresentationWorkbenchControllerError::StaleContext);
        }
        self.check_package(snapshot.package())?;
        self.engine = Some(snapshot);
        self.pending = None;
        Ok(())
    }

    pub fn selection_cancelled(
        &mut self,
        context: PresentationWorkbenchControllerContext,
        request: DesktopRequestId,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        if self.check_pending(context, request)?
            != PresentationWorkbenchControllerEnumPending::SelectTablebases
        {
            return Err(PresentationWorkbenchControllerError::StaleContext);
        }
        self.pending = None;
        Ok(())
    }

    pub fn accept_job(
        &mut self,
        context: PresentationWorkbenchControllerContext,
        request: DesktopRequestId,
        snapshot: PresentationWorkbenchJobSnapshot,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        if self.check_pending(context, request)?
            != PresentationWorkbenchControllerEnumPending::Start(snapshot.operation())
            || self
                .job
                .as_ref()
                .is_some_and(|current| current.id() == snapshot.id())
        {
            return Err(PresentationWorkbenchControllerError::StaleContext);
        }
        self.check_package(snapshot.package())?;
        self.job = Some(snapshot);
        self.pending = None;
        Ok(())
    }

    pub fn receive_job(
        &mut self,
        context: PresentationWorkbenchControllerContext,
        snapshot: PresentationWorkbenchJobSnapshot,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        self.check_context(context)?;
        let current = self
            .job
            .as_ref()
            .ok_or(PresentationWorkbenchControllerError::StaleContext)?;
        if current.id() != snapshot.id()
            || current.created_at() != snapshot.created_at()
            || current.operation() != snapshot.operation()
            || current.parameters() != snapshot.parameters()
            || current.timeout() != snapshot.timeout()
            || current.deadline() != snapshot.deadline()
            || current.progress().total() != snapshot.progress().total()
            || snapshot.observed_at() < current.observed_at()
            || snapshot.progress().completed() < current.progress().completed()
            || Self::state_order(snapshot.state()) < Self::state_order(current.state())
            || (current.is_terminal() && current.state() != snapshot.state())
        {
            return Err(PresentationWorkbenchControllerError::StaleContext);
        }
        self.check_package(snapshot.package())?;
        if self.pending.is_some_and(|pending| {
            pending.action == PresentationWorkbenchControllerEnumPending::Cancel(snapshot.id())
        }) && (snapshot.is_terminal() || snapshot.state() == WorkbenchEnumJobState::Cancelling)
        {
            self.pending = None;
        }
        self.job = Some(snapshot);
        Ok(())
    }

    pub fn exported(
        &mut self,
        context: PresentationWorkbenchControllerContext,
        request: DesktopRequestId,
        job: WorkbenchJobId,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        if self.check_pending(context, request)?
            != PresentationWorkbenchControllerEnumPending::Export(job)
        {
            return Err(PresentationWorkbenchControllerError::StaleContext);
        }
        self.pending = None;
        self.exported_job = Some(job);
        Ok(())
    }

    pub fn reject(
        &mut self,
        context: PresentationWorkbenchControllerContext,
        request: DesktopRequestId,
        failure: DesktopFailure,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        self.check_pending(context, request)?;
        self.pending = None;
        self.failure = Some(failure);
        Ok(())
    }

    fn ready(
        &self,
    ) -> Result<&PresentationWorkbenchSnapshot, PresentationWorkbenchControllerError> {
        self.engine.as_ref().ok_or_else(|| {
            if self.failure.is_some() {
                PresentationWorkbenchControllerError::Unavailable
            } else {
                PresentationWorkbenchControllerError::Loading
            }
        })
    }

    fn idle(&self) -> Result<usize, PresentationWorkbenchControllerError> {
        self.ready()?;
        if self.pending.is_some() {
            return Err(PresentationWorkbenchControllerError::Pending);
        }
        if self.job.as_ref().is_some_and(|job| !job.is_terminal()) {
            return Err(PresentationWorkbenchControllerError::Busy(
                PresentationWorkbenchControllerEnumBusy::Diagnostic,
            ));
        }
        match self.admission {
            PresentationWorkbenchControllerEnumAdmission::Idle { available_workers } => {
                Ok(available_workers)
            }
            PresentationWorkbenchControllerEnumAdmission::Busy(reason) => {
                Err(PresentationWorkbenchControllerError::Busy(reason))
            }
            PresentationWorkbenchControllerEnumAdmission::Unavailable => {
                Err(PresentationWorkbenchControllerError::Unavailable)
            }
        }
    }

    fn emit(
        &mut self,
        request: DesktopRequestId,
        action: PresentationWorkbenchControllerEnumAction,
        pending: PresentationWorkbenchControllerEnumPending,
    ) -> PresentationWorkbenchControllerIntent {
        self.pending = Some(PresentationWorkbenchControllerPending {
            request,
            action: pending,
        });
        self.last_request = Some(request);
        self.failure = None;
        if matches!(
            pending,
            PresentationWorkbenchControllerEnumPending::Start(_)
                | PresentationWorkbenchControllerEnumPending::Export(_)
        ) {
            self.exported_job = None;
        }
        PresentationWorkbenchControllerIntent {
            context: self.context,
            request,
            action,
        }
    }

    fn check_context(
        &self,
        context: PresentationWorkbenchControllerContext,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        if context == self.context {
            Ok(())
        } else {
            Err(PresentationWorkbenchControllerError::StaleContext)
        }
    }

    fn check_pending(
        &self,
        context: PresentationWorkbenchControllerContext,
        request: DesktopRequestId,
    ) -> Result<PresentationWorkbenchControllerEnumPending, PresentationWorkbenchControllerError>
    {
        self.check_context(context)?;
        self.pending
            .filter(|pending| pending.request == request)
            .map(|pending| pending.action)
            .ok_or(PresentationWorkbenchControllerError::StaleContext)
    }

    fn check_position(
        &self,
        position: &EngineSearchPosition,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        if self.position.as_ref().is_some_and(|selected| {
            selected.initial() == position.initial() && selected.history() == position.history()
        }) {
            Ok(())
        } else {
            Err(PresentationWorkbenchControllerError::PositionMismatch)
        }
    }

    fn check_package(
        &self,
        package: &EnginePackageSelected,
    ) -> Result<(), PresentationWorkbenchControllerError> {
        if self.engine.as_ref().is_none_or(|current| {
            current.package().digest() == package.digest()
                && current.package().target() == package.target()
                && current.package().profile() == package.profile()
        }) {
            Ok(())
        } else {
            Err(PresentationWorkbenchControllerError::StaleContext)
        }
    }

    const fn state_order(state: WorkbenchEnumJobState) -> u8 {
        match state {
            WorkbenchEnumJobState::Queued => 0,
            WorkbenchEnumJobState::Admitted => 1,
            WorkbenchEnumJobState::Running => 2,
            WorkbenchEnumJobState::Cancelling => 3,
            WorkbenchEnumJobState::Succeeded
            | WorkbenchEnumJobState::Failed
            | WorkbenchEnumJobState::Cancelled
            | WorkbenchEnumJobState::TimedOut => 4,
        }
    }
}
