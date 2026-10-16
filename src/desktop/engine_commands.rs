use std::sync::Arc;

use thiserror::Error;
use tokio::time::Instant;

use crate::capacity::executor::CapacityExecutor;
use crate::capacity::lease::CapacityLease;
use crate::clock::duration::ClockInstant;
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::identity::DesktopRevision;
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::desktop::workbench_commands::DesktopWorkbenchCommands;
use crate::desktop::workbench_export::{
    DesktopWorkbenchExport, DesktopWorkbenchExportPreview, DesktopWorkbenchExportRequest,
};
use crate::engine::capabilities::{EngineCapabilities, EngineIdentity};
use crate::engine::options::{EngineOptionsApplied, EngineOptionsEnumValue, EngineOptionsSetting};
use crate::engine::package::selector::EnginePackageSelected;
use crate::engine::process_provider::{EngineProcessLimits, EngineProcessProvider};
use crate::engine::resource_config::EngineResourceConfig;
use crate::engine::search::enums::EngineSearchEnumState;
use crate::engine::search::identity::{
    EngineSearchContext, EngineSearchGeneration, EngineSearchTag,
};
use crate::engine::search::limit::EngineSearchLimit;
use crate::engine::search::position::EngineSearchPosition;
use crate::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumEvent, EngineSearchSessionMetadata,
    EngineSearchSessionRequest, EngineSearchSessionTerminal,
};
use crate::engine::search::stop::EngineSearchStop;
use crate::engine::session::{EngineSession, EngineSessionStartup};
use crate::engine::tablebase::status::EngineTablebaseStatus;
use crate::workbench::enums::WorkbenchEnumParameters;
use crate::workbench::export::WorkbenchExportReceipt;
use crate::workbench::job::{WorkbenchJobDeadline, WorkbenchJobId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopEngineCommandsContext {
    owner: DesktopMessagesEntityId,
    generation: EngineSearchGeneration,
    configuration: DesktopRevision,
    selection: EngineSearchContext,
}

impl DesktopEngineCommandsContext {
    pub const fn new(
        owner: DesktopMessagesEntityId,
        generation: EngineSearchGeneration,
        configuration: DesktopRevision,
        selection: EngineSearchContext,
    ) -> Self {
        Self {
            owner,
            generation,
            configuration,
            selection,
        }
    }
    pub const fn owner(self) -> DesktopMessagesEntityId {
        self.owner
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopEngineCommandsEnumOperation {
    Snapshot,
    Start {
        position: EngineSearchPosition,
        limit: EngineSearchLimit,
    },
    Stop {
        tag: EngineSearchTag,
    },
    Configure(EngineOptionsSetting),
    Retry,
    PrepareTablebases {
        directory: crate::desktop::file_handle::DesktopFileHandleId,
    },
    LoadTablebases,
    Workbench(crate::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation),
}

impl DesktopEngineCommandsEnumOperation {
    pub const fn is_control(&self) -> bool {
        matches!(self, Self::Stop { .. } | Self::Workbench(crate::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation::Cancel { .. }))
    }
    pub const fn is_mutation(&self) -> bool {
        !matches!(self, Self::Snapshot | Self::Stop { .. } | Self::Workbench(crate::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation::Cancel { .. } | crate::desktop::workbench_commands::DesktopWorkbenchCommandsEnumOperation::Result { .. }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEngineCommandsOperation {
    context: DesktopEngineCommandsContext,
    action: DesktopEngineCommandsEnumOperation,
}

pub(crate) struct DesktopEngineCommandsWorkbenchRequest {
    pub parameters: WorkbenchEnumParameters,
    pub deadline: WorkbenchJobDeadline,
    pub id: WorkbenchJobId,
    pub now: ClockInstant,
}

impl DesktopEngineCommandsOperation {
    pub const fn new(
        context: DesktopEngineCommandsContext,
        action: DesktopEngineCommandsEnumOperation,
    ) -> Self {
        Self { context, action }
    }
    pub const fn context(&self) -> DesktopEngineCommandsContext {
        self.context
    }
    pub const fn action(&self) -> &DesktopEngineCommandsEnumOperation {
        &self.action
    }
}

#[derive(Debug, Clone)]
pub struct DesktopEngineCommandsSnapshot {
    generation: EngineSearchGeneration,
    configuration: DesktopRevision,
    package: EnginePackageSelected,
    identity: EngineIdentity,
    applied: Option<EngineOptionsApplied>,
    tablebases: Option<Arc<EngineTablebaseStatus>>,
    state: EngineSearchEnumState,
    active_workbench: Option<WorkbenchJobId>,
    completed_workbench: Option<crate::desktop::workbench_commands::DesktopWorkbenchCommandsResult>,
}

impl DesktopEngineCommandsSnapshot {
    pub const fn generation(&self) -> EngineSearchGeneration {
        self.generation
    }
    pub const fn configuration(&self) -> DesktopRevision {
        self.configuration
    }
    pub const fn package(&self) -> &EnginePackageSelected {
        &self.package
    }
    pub fn capabilities(&self) -> &EngineCapabilities {
        self.identity.capabilities()
    }
    pub const fn identity(&self) -> &EngineIdentity {
        &self.identity
    }
    pub const fn applied(&self) -> Option<&EngineOptionsApplied> {
        self.applied.as_ref()
    }
    pub fn tablebases(&self) -> Option<&EngineTablebaseStatus> {
        self.tablebases.as_deref()
    }
    pub const fn state(&self) -> EngineSearchEnumState {
        self.state
    }
    pub const fn active_workbench(&self) -> Option<WorkbenchJobId> {
        self.active_workbench
    }
    pub const fn completed_workbench(
        &self,
    ) -> Option<&crate::desktop::workbench_commands::DesktopWorkbenchCommandsResult> {
        self.completed_workbench.as_ref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopEngineCommandsEnumWorkload {
    Analysis,
    Configuration,
    Retry,
    Diagnostic,
}

#[derive(Debug, Clone, Copy)]
pub struct DesktopEngineCommandsResourceRequest {
    workload: DesktopEngineCommandsEnumWorkload,
    resources: EngineResourceConfig,
    startup_hash_mib: Option<u32>,
}

impl DesktopEngineCommandsResourceRequest {
    pub const fn new(
        workload: DesktopEngineCommandsEnumWorkload,
        resources: EngineResourceConfig,
        startup_hash_mib: Option<u32>,
    ) -> Self {
        Self {
            workload,
            resources,
            startup_hash_mib,
        }
    }

    pub const fn workload(self) -> DesktopEngineCommandsEnumWorkload {
        self.workload
    }
    pub const fn resources(self) -> EngineResourceConfig {
        self.resources
    }
    pub const fn startup_hash_mib(self) -> Option<u32> {
        self.startup_hash_mib
    }
}

pub struct DesktopEngineCommandsAllocation<R> {
    lease: CapacityLease,
    reservation: R,
}

impl<R> DesktopEngineCommandsAllocation<R> {
    pub fn new(lease: CapacityLease, reservation: R) -> Self {
        Self { lease, reservation }
    }
    pub(crate) fn into_parts(self) -> (CapacityLease, R) {
        (self.lease, self.reservation)
    }
}

pub trait DesktopEngineCommandsAdmissionProvider: Send {
    type Reservation: Send + Sync + 'static;
    fn admit(
        &mut self,
        request: DesktopEngineCommandsResourceRequest,
    ) -> impl std::future::Future<
        Output = Result<DesktopEngineCommandsAllocation<Self::Reservation>, DesktopFailure>,
    > + Send;

    fn acknowledge(
        &mut self,
        reservation: &Self::Reservation,
    ) -> impl std::future::Future<Output = Result<(), DesktopFailure>> + Send;
}

pub trait DesktopEngineCommandsSelectionProvider: Send {
    fn authorize_context(&self, context: EngineSearchContext) -> Result<(), DesktopFailure>;
    fn authorize(
        &self,
        context: EngineSearchContext,
        position: &EngineSearchPosition,
    ) -> Result<(), DesktopFailure>;
}

pub struct DesktopEngineCommandsServices<P, A: DesktopEngineCommandsAdmissionProvider, S> {
    pub provider: P,
    pub admission: A,
    pub selection: S,
    pub executor: Arc<CapacityExecutor>,
    pub reservation: A::Reservation,
    pub owner: DesktopMessagesEntityId,
}

#[derive(Debug, Error)]
pub enum DesktopEngineCommandsError {
    #[error(
        "The engine request belongs to another owner, process generation or configuration revision"
    )]
    Stale,
    #[error("Engine maintenance requires an idle engine and no unfinished diagnostic")]
    Busy,
    #[error("Engine Retry requires an explicitly failed session")]
    RetryRequiresFailure,
    #[error(
        "Variant changes belong to New Game and tablebase paths belong to native directory selection"
    )]
    OwnedOption,
    #[error("The engine requires an admitted resource configuration")]
    Resources,
    #[error("The admission port returned {actual} CPU workers for a {expected}-worker request")]
    Allocation { expected: usize, actual: usize },
    #[error("The engine operation failed: {0}")]
    Effect(String),
}

impl From<DesktopEngineCommandsError> for DesktopFailure {
    fn from(error: DesktopEngineCommandsError) -> Self {
        let code = match error {
            DesktopEngineCommandsError::Stale => DesktopEnumFailureCode::StaleRevision,
            DesktopEngineCommandsError::OwnedOption => DesktopEnumFailureCode::InvalidInput,
            DesktopEngineCommandsError::Busy
            | DesktopEngineCommandsError::RetryRequiresFailure
            | DesktopEngineCommandsError::Resources
            | DesktopEngineCommandsError::Allocation { .. }
            | DesktopEngineCommandsError::Effect(_) => DesktopEnumFailureCode::ResourceRefusal,
        };
        Self::new(code, error.to_string(), true)
    }
}

pub struct DesktopEngineCommands<
    P: EngineProcessProvider,
    A: DesktopEngineCommandsAdmissionProvider,
    S,
> {
    pub(crate) session: EngineSearchSession<P::Process>,
    provider: P,
    admission: A,
    selection: S,
    pub(crate) executor: Arc<CapacityExecutor>,
    reservation: Option<A::Reservation>,
    transient: Option<DesktopEngineCommandsAllocation<A::Reservation>>,
    analysis_reservation: Option<A::Reservation>,
    startup: Option<EngineSessionStartup<P::Process>>,
    retry_session: Option<EngineSession<P::Process>>,
    last_applied: Option<EngineOptionsApplied>,
    owner: DesktopMessagesEntityId,
    configuration: DesktopRevision,
    pub(crate) tablebases: crate::desktop::engine_tablebases::DesktopEngineTablebases,
    pub(crate) workbench: DesktopWorkbenchCommands<P::Process, A::Reservation>,
    pub(crate) exports: DesktopWorkbenchExport,
}

impl<P, A, S> DesktopEngineCommands<P, A, S>
where
    P: EngineProcessProvider,
    P::Process: Sync + 'static,
    A: DesktopEngineCommandsAdmissionProvider,
    S: DesktopEngineCommandsSelectionProvider,
{
    pub fn new(
        session: EngineSearchSession<P::Process>,
        services: DesktopEngineCommandsServices<P, A, S>,
    ) -> Self {
        let last_applied = session.engine().applied_options().cloned();
        Self {
            session,
            provider: services.provider,
            admission: services.admission,
            selection: services.selection,
            executor: services.executor,
            reservation: Some(services.reservation),
            transient: None,
            analysis_reservation: None,
            startup: None,
            retry_session: None,
            last_applied,
            owner: services.owner,
            configuration: DesktopRevision::ZERO,
            tablebases: crate::desktop::engine_tablebases::DesktopEngineTablebases::new(),
            workbench: DesktopWorkbenchCommands::new(),
            exports: DesktopWorkbenchExport::new(),
        }
    }

    pub fn snapshot(&self) -> DesktopEngineCommandsSnapshot {
        let engine = self.session.engine();
        DesktopEngineCommandsSnapshot {
            generation: engine.generation(),
            configuration: self.configuration,
            package: engine.package().clone(),
            identity: engine.identity().clone(),
            applied: engine.applied_options().cloned(),
            tablebases: self.tablebases.status(),
            state: engine.state(),
            active_workbench: self.workbench.active_job().map(|job| job.id()),
            completed_workbench: self.workbench.latest_result(),
        }
    }

    pub(crate) fn validate(
        &self,
        context: DesktopEngineCommandsContext,
    ) -> Result<(), DesktopFailure> {
        if context.owner != self.owner
            || context.generation != self.session.engine().generation()
            || context.configuration != self.configuration
        {
            return Err(DesktopEngineCommandsError::Stale.into());
        }
        Ok(())
    }

    pub(crate) fn idle(&self) -> Result<(), DesktopFailure> {
        if self.session.engine().state() != EngineSearchEnumState::Idle
            || self.session.allocation().is_some()
            || self.workbench.is_active()
            || self.startup.is_some()
            || self.transient.is_some()
        {
            Err(DesktopEngineCommandsError::Busy.into())
        } else {
            Ok(())
        }
    }

    fn resources(&self) -> Result<EngineResourceConfig, DesktopFailure> {
        self.session
            .engine()
            .resources()
            .ok_or_else(|| DesktopEngineCommandsError::Resources.into())
    }

    async fn admit(
        &mut self,
        context: DesktopEngineCommandsContext,
        workload: DesktopEngineCommandsEnumWorkload,
        resources: EngineResourceConfig,
    ) -> Result<DesktopEngineCommandsAllocation<A::Reservation>, DesktopFailure> {
        let allocation = self
            .admission
            .admit(DesktopEngineCommandsResourceRequest {
                workload,
                resources,
                startup_hash_mib: match workload {
                    DesktopEngineCommandsEnumWorkload::Retry
                    | DesktopEngineCommandsEnumWorkload::Diagnostic => {
                        let crate::engine::enums::EngineOptionEnumDescriptor::Spin(range) =
                            crate::engine::enums::EngineEnumOption::Hash.typed_descriptor()
                        else {
                            return Err(DesktopEngineCommandsError::Resources.into());
                        };
                        Some(range.default_value())
                    }
                    DesktopEngineCommandsEnumWorkload::Analysis
                    | DesktopEngineCommandsEnumWorkload::Configuration => None,
                },
            })
            .await?;
        self.validate(context)?;
        self.selection.authorize_context(context.selection)?;
        let expected = usize::try_from(resources.threads()).map_err(Self::effect)?;
        if allocation.lease.workers() != expected {
            return Err(DesktopEngineCommandsError::Allocation {
                expected,
                actual: allocation.lease.workers(),
            }
            .into());
        }
        Ok(allocation)
    }

    async fn acknowledge_transient(&mut self) -> Result<(), DesktopFailure> {
        let allocation = self
            .transient
            .as_ref()
            .ok_or_else(|| DesktopFailure::from(DesktopEngineCommandsError::Resources))?;
        self.admission.acknowledge(&allocation.reservation).await
    }

    pub async fn start(
        &mut self,
        context: DesktopEngineCommandsContext,
        position: EngineSearchPosition,
        limit: EngineSearchLimit,
    ) -> Result<EngineSearchSessionMetadata, DesktopFailure> {
        self.validate(context)?;
        self.idle()?;
        self.selection.authorize(context.selection, &position)?;
        let allocation = self
            .admit(
                context,
                DesktopEngineCommandsEnumWorkload::Analysis,
                self.resources()?,
            )
            .await?;
        self.selection.authorize(context.selection, &position)?;
        self.analysis_reservation = Some(allocation.reservation);
        let request =
            EngineSearchSessionRequest::new(context.selection, position, limit, allocation.lease);
        let result = self.session.start(request).await;
        if self.session.allocation().is_none() {
            self.analysis_reservation = None;
        }
        result.map_err(Self::effect)?;
        self.session
            .metadata()
            .cloned()
            .ok_or_else(|| DesktopEngineCommandsError::Resources.into())
    }

    pub async fn next_event(
        &mut self,
        tag: EngineSearchTag,
    ) -> Result<EngineSearchSessionEnumEvent, DesktopFailure> {
        let result = self.session.next_event(tag).await;
        if self.session.allocation().is_none() {
            self.analysis_reservation = None;
        }
        result.map_err(Self::effect)
    }

    pub async fn stop(
        &mut self,
        context: DesktopEngineCommandsContext,
        tag: EngineSearchTag,
        received_at: Instant,
    ) -> Result<EngineSearchSessionTerminal, DesktopFailure> {
        self.validate(context)?;
        if tag.context() != context.selection {
            return Err(DesktopEngineCommandsError::Stale.into());
        }
        let stop = EngineSearchStop::received_at(tag, received_at).map_err(Self::effect)?;
        let result = self.session.stop(stop).await;
        if self.session.allocation().is_none() {
            self.analysis_reservation = None;
        }
        result.map_err(Self::effect)
    }

    pub async fn configure(
        &mut self,
        context: DesktopEngineCommandsContext,
        setting: EngineOptionsSetting,
    ) -> Result<DesktopEngineCommandsSnapshot, DesktopFailure> {
        self.validate(context)?;
        self.selection.authorize_context(context.selection)?;
        self.idle()?;
        if matches!(
            setting.value(),
            EngineOptionsEnumValue::Chess960(_) | EngineOptionsEnumValue::SyzygyPath(_)
        ) {
            return Err(DesktopEngineCommandsError::OwnedOption.into());
        }
        setting
            .validate(self.session.engine().identity().capabilities())
            .map_err(Self::effect)?;
        let resources = setting
            .updated_resources(Some(self.resources()?))
            .map_err(Self::effect)?
            .ok_or_else(|| DesktopFailure::from(DesktopEngineCommandsError::Resources))?;
        let revision = self.configuration.next().map_err(DesktopFailure::from)?;
        self.transient = Some(
            self.admit(
                context,
                DesktopEngineCommandsEnumWorkload::Configuration,
                resources,
            )
            .await?,
        );
        let outcome = self
            .session
            .idle_engine()
            .map_err(Self::effect)?
            .configure_option(setting)
            .await;
        match outcome {
            Ok(_) => {
                self.acknowledge_transient().await?;
                let allocation = self
                    .transient
                    .take()
                    .ok_or_else(|| DesktopFailure::from(DesktopEngineCommandsError::Resources))?;
                self.reservation = Some(allocation.reservation);
                self.configuration = revision;
                self.last_applied = self.session.engine().applied_options().cloned();
                Ok(self.snapshot())
            }
            Err(error) => Err(Self::effect(error)),
        }
    }

    pub async fn retry(
        &mut self,
        context: DesktopEngineCommandsContext,
    ) -> Result<DesktopEngineCommandsSnapshot, DesktopFailure> {
        self.validate(context)?;
        self.selection.authorize_context(context.selection)?;
        if self.session.engine().state() != EngineSearchEnumState::Failed
            || self.workbench.is_active()
        {
            return Err(DesktopEngineCommandsError::RetryRequiresFailure.into());
        }
        let applied = self
            .last_applied
            .clone()
            .ok_or_else(|| DesktopFailure::from(DesktopEngineCommandsError::Resources))?;
        let resources = applied
            .resources()
            .ok_or_else(|| DesktopFailure::from(DesktopEngineCommandsError::Resources))?;
        let revision = self.configuration.next().map_err(DesktopFailure::from)?;
        self.session
            .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
            .await
            .map_err(Self::effect)?;
        self.analysis_reservation = None;
        if let Some(engine) = &mut self.retry_session {
            engine
                .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
                .await
                .map_err(Self::effect)?;
            if engine.is_shutdown_complete() {
                self.retry_session = None;
            }
        }
        if let Some(startup) = &mut self.startup {
            startup
                .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
                .await
                .map_err(Self::effect)?;
            if startup.is_shutdown_complete() {
                self.startup = None;
            }
        }
        self.transient = None;
        if self.startup.is_none() {
            self.transient = Some(
                self.admit(context, DesktopEngineCommandsEnumWorkload::Retry, resources)
                    .await?,
            );
            self.startup = Some(
                EngineSessionStartup::new(&self.provider, self.session.engine().package().clone())
                    .map_err(Self::effect)?,
            );
        }
        let startup = self
            .startup
            .as_mut()
            .ok_or_else(|| DesktopFailure::from(DesktopEngineCommandsError::Resources))?;
        let engine = startup.initialize().await.map_err(Self::effect)?;
        self.startup = None;
        self.retry_session = Some(engine);
        let engine = self
            .retry_session
            .as_mut()
            .ok_or_else(|| DesktopFailure::from(DesktopEngineCommandsError::Resources))?;
        engine
            .configure_resources(resources)
            .await
            .map_err(Self::effect)?;
        engine
            .new_game(applied.variant())
            .await
            .map_err(Self::effect)?;
        let mut values = vec![
            EngineOptionsEnumValue::Minimal(applied.is_minimal()),
            EngineOptionsEnumValue::MultiPv(applied.multi_pv().get()),
        ];
        if let Some(status) = self.tablebases.status() {
            values.push(EngineOptionsEnumValue::SyzygyPath(
                status.engine_path().clone(),
            ));
        }
        for value in values {
            let setting = EngineOptionsSetting::new(value, engine.identity().capabilities())
                .map_err(Self::effect)?;
            let outcome = engine.configure_option(setting).await;
            if matches!(
                &outcome,
                Ok(crate::engine::options::EngineOptionsEnumOutcome::Tablebases(_))
            ) {
                self.tablebases.record_configuration(outcome)?;
            } else {
                outcome.map_err(Self::effect)?;
            }
        }
        self.acknowledge_transient().await?;
        let engine = self
            .retry_session
            .take()
            .ok_or_else(|| DesktopFailure::from(DesktopEngineCommandsError::Resources))?;
        self.session = EngineSearchSession::new(engine);
        self.configuration = revision;
        let allocation = self
            .transient
            .take()
            .ok_or_else(|| DesktopFailure::from(DesktopEngineCommandsError::Resources))?;
        self.reservation = Some(allocation.reservation);
        self.last_applied = self.session.engine().applied_options().cloned();
        Ok(self.snapshot())
    }

    pub(crate) async fn start_workbench(
        &mut self,
        context: DesktopEngineCommandsContext,
        request: DesktopEngineCommandsWorkbenchRequest,
    ) -> Result<(), DesktopFailure> {
        let DesktopEngineCommandsWorkbenchRequest {
            parameters,
            deadline,
            id,
            now,
        } = request;
        self.validate(context)?;
        self.selection.authorize_context(context.selection)?;
        self.idle()?;
        self.authorize_parameters(context, &parameters)?;
        let resources = self
            .workbench
            .resources(&parameters, self.session.engine().identity().capabilities())
            .map_err(Self::effect)?;
        let allocation = self
            .admit(
                context,
                DesktopEngineCommandsEnumWorkload::Diagnostic,
                resources,
            )
            .await?;
        self.authorize_parameters(context, &parameters)?;
        self.workbench
            .start(
                &self.provider,
                crate::desktop::workbench_commands::DesktopWorkbenchCommandsRequest {
                    package: self.session.engine().package().clone(),
                    parameters,
                    deadline,
                    id,
                    now,
                },
                allocation,
            )
            .await
    }

    fn authorize_parameters(
        &self,
        context: DesktopEngineCommandsContext,
        parameters: &WorkbenchEnumParameters,
    ) -> Result<(), DesktopFailure> {
        match parameters {
            WorkbenchEnumParameters::Inspector(position)
            | WorkbenchEnumParameters::RawEvaluation(position) => {
                self.selection.authorize(context.selection, position)
            }
            WorkbenchEnumParameters::Perft(parameters) => self
                .selection
                .authorize(context.selection, parameters.position()),
            WorkbenchEnumParameters::Compiler | WorkbenchEnumParameters::Bench(_) => Ok(()),
        }
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.session.engine().is_shutdown_complete()
            && self
                .startup
                .as_ref()
                .is_none_or(EngineSessionStartup::is_shutdown_complete)
            && self
                .retry_session
                .as_ref()
                .is_none_or(EngineSession::is_shutdown_complete)
            && !self.workbench.is_active()
            && self.exports.is_shutdown_complete()
            && self.tablebases.is_empty()
    }

    pub async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), DesktopFailure> {
        let mut failure = None;
        Self::record_cleanup(&mut failure, self.exports.shutdown_retained(deadline).await);
        Self::record_cleanup(
            &mut failure,
            self.workbench.shutdown_retained(deadline).await,
        );
        Self::record_cleanup(
            &mut failure,
            self.workbench.observe_completion().map(|_result| ()),
        );
        if let Some(startup) = &mut self.startup {
            let result = startup
                .shutdown_retained(deadline)
                .await
                .map_err(Self::effect);
            Self::record_cleanup(&mut failure, result);
            if startup.is_shutdown_complete() {
                self.startup = None;
            }
        }
        if let Some(engine) = &mut self.retry_session {
            let result = engine
                .shutdown_retained(deadline)
                .await
                .map_err(Self::effect);
            Self::record_cleanup(&mut failure, result);
            if engine.is_shutdown_complete() {
                self.retry_session = None;
            }
        }
        let result = self
            .session
            .shutdown_retained(deadline)
            .await
            .map_err(Self::effect);
        Self::record_cleanup(&mut failure, result);
        if self.session.engine().is_shutdown_complete() {
            self.reservation = None;
            self.analysis_reservation = None;
            if self.startup.is_none() && self.retry_session.is_none() {
                self.transient = None;
            }
        }
        if self.session.engine().is_shutdown_complete()
            && self.startup.is_none()
            && self.retry_session.is_none()
            && !self.workbench.is_active()
        {
            Self::record_cleanup(
                &mut failure,
                self.tablebases.cleanup(&self.executor, deadline).await,
            );
        }
        failure.map_or(Ok(()), Err)
    }

    pub(crate) fn record_cleanup(
        failure: &mut Option<DesktopFailure>,
        result: Result<(), DesktopFailure>,
    ) {
        if let Err(error) = result {
            *failure = Some(match failure.take() {
                Some(previous) => DesktopFailure::new(
                    DesktopEnumFailureCode::ResourceRefusal,
                    format!(
                        "{}; additional cleanup failed: {}",
                        previous.message(),
                        error.message()
                    ),
                    previous.is_retryable() && error.is_retryable(),
                ),
                None => error,
            });
        }
    }

    pub(crate) fn preview_export(
        &mut self,
        context: DesktopEngineCommandsContext,
        request: DesktopRequestId,
        job: WorkbenchJobId,
    ) -> Result<Arc<DesktopWorkbenchExportPreview>, DesktopFailure> {
        self.validate(context)?;
        self.exports.preview(request, &self.workbench.result(job)?)
    }

    pub(crate) async fn apply_export(
        &mut self,
        context: DesktopEngineCommandsContext,
        request: DesktopWorkbenchExportRequest,
    ) -> Result<WorkbenchExportReceipt, DesktopFailure> {
        self.validate(context)?;
        self.exports.begin_apply(request, &self.executor)?;
        self.exports.observe().await
    }

    pub(crate) fn export_outcome(
        &self,
        request: DesktopRequestId,
    ) -> Option<Result<WorkbenchExportReceipt, DesktopFailure>> {
        self.exports.outcome(request)
    }

    pub(crate) fn exports_quiescent(&self) -> bool {
        self.exports.is_shutdown_complete()
    }

    pub(crate) fn export_pending(&self) -> bool {
        self.exports.is_pending()
    }

    pub(crate) async fn quiesce_exports(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopFailure> {
        self.exports.shutdown_retained(deadline).await
    }

    pub(crate) fn effect(error: impl std::fmt::Display) -> DesktopFailure {
        DesktopEngineCommandsError::Effect(error.to_string()).into()
    }

    pub(crate) fn observe_search_capacity(&mut self) {
        if self.session.allocation().is_none() {
            self.analysis_reservation = None;
        }
    }

    pub async fn load_tablebases(
        &mut self,
        context: DesktopEngineCommandsContext,
    ) -> Result<DesktopEngineCommandsSnapshot, DesktopFailure> {
        self.validate(context)?;
        self.selection.authorize_context(context.selection)?;
        self.idle()?;
        let status = self
            .tablebases
            .status()
            .ok_or_else(|| DesktopFailure::from(DesktopEngineCommandsError::Resources))?;
        let setting = EngineOptionsSetting::new(
            EngineOptionsEnumValue::SyzygyPath(status.engine_path().clone()),
            self.session.engine().identity().capabilities(),
        )
        .map_err(Self::effect)?;
        let revision = self.configuration.next().map_err(DesktopFailure::from)?;
        self.transient = Some(
            self.admit(
                context,
                DesktopEngineCommandsEnumWorkload::Configuration,
                self.resources()?,
            )
            .await?,
        );
        let outcome = self
            .session
            .idle_engine()
            .map_err(Self::effect)?
            .configure_option(setting)
            .await;
        self.tablebases.record_configuration(outcome)?;
        if self.session.engine().state() == EngineSearchEnumState::Idle {
            self.acknowledge_transient().await?;
            self.transient = None;
            self.configuration = revision;
            self.last_applied = self.session.engine().applied_options().cloned();
            if self.tablebases.status().is_some_and(|status| matches!(status.state(), crate::engine::tablebase::status::EngineTablebaseStatusEnumState::Loaded { .. } | crate::engine::tablebase::status::EngineTablebaseStatusEnumState::Unavailable)) {
                self.tablebases.cleanup_retired(&self.executor).await?;
            }
        }
        Ok(self.snapshot())
    }

    pub async fn prepare_tablebases(
        &mut self,
        context: DesktopEngineCommandsContext,
        selection: crate::desktop::file_handle::DesktopFileEnumSelection,
        policy: crate::engine::tablebase::location::EngineTablebaseLocationPolicy,
        receipt: crate::desktop::application::DesktopApplicationReceipt,
    ) -> Result<DesktopEngineCommandsSnapshot, DesktopFailure> {
        self.validate(context)?;
        self.selection.authorize_context(context.selection)?;
        self.idle()?;
        self.tablebases
            .prepare(selection, policy, receipt, &self.executor)
            .await?;
        Ok(self.snapshot())
    }
}
