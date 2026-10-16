use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use tokio::time::Instant;

use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::desktop::engine_commands::{
    DesktopEngineCommandsAllocation, DesktopEngineCommandsContext,
};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::file_handle::DesktopFileHandleId;
use crate::desktop::identity::DesktopRequestId;
use crate::engine::capabilities::EngineCapabilities;
use crate::engine::diagnostics::EngineDiagnostics;
use crate::engine::enums::EngineEnumOption;
use crate::engine::package::selector::EnginePackageSelected;
use crate::engine::process_provider::{EngineProcess, EngineProcessProvider};
use crate::engine::resource_config::{EngineResourceConfig, EngineResourceConfigError};
use crate::package::digest::PackageDigest;
use crate::workbench::enums::WorkbenchEnumParameters;
use crate::workbench::executor::{WorkbenchExecutor, WorkbenchExecutorObservation};
use crate::workbench::job::{WorkbenchJob, WorkbenchJobDeadline, WorkbenchJobId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopWorkbenchCommandsEnumOperation {
    Start {
        parameters: WorkbenchEnumParameters,
        deadline: WorkbenchJobDeadline,
    },
    Cancel {
        job: WorkbenchJobId,
    },
    Result {
        job: WorkbenchJobId,
    },
    PreviewExport {
        job: WorkbenchJobId,
    },
    ApplyExport {
        preview: DesktopRequestId,
        digest: PackageDigest,
        destination: DesktopFileHandleId,
    },
}

pub trait DesktopWorkbenchCommandsJobProvider: Send + Sync {
    fn job(&self) -> &WorkbenchJob;
    fn observation(&self) -> Option<&WorkbenchExecutorObservation>;
}

impl<P: EngineProcess + Sync> DesktopWorkbenchCommandsJobProvider for WorkbenchExecutor<P> {
    fn job(&self) -> &WorkbenchJob {
        self.job()
    }
    fn observation(&self) -> Option<&WorkbenchExecutorObservation> {
        self.observation()
    }
}

#[derive(Clone)]
pub struct DesktopWorkbenchCommandsResult(Arc<dyn DesktopWorkbenchCommandsJobProvider>);

impl DesktopWorkbenchCommandsResult {
    pub fn job(&self) -> &WorkbenchJob {
        self.0.job()
    }
    pub fn observation(&self) -> Option<&WorkbenchExecutorObservation> {
        self.0.observation()
    }
    pub fn export_context(&self) -> crate::workbench::export::WorkbenchExportContext {
        match self.observation() {
            Some(observation) => crate::workbench::export::WorkbenchExportContext::observed(
                self.job(),
                observation.identity(),
                observation.applied_options(),
            ),
            None => crate::workbench::export::WorkbenchExportContext::unobserved(self.job()),
        }
    }
}

impl Debug for DesktopWorkbenchCommandsResult {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DesktopWorkbenchCommandsResult")
            .field("id", &self.job().id())
            .field("state", &self.job().state())
            .finish()
    }
}

pub struct DesktopWorkbenchCommandsRequest {
    pub package: EnginePackageSelected,
    pub parameters: WorkbenchEnumParameters,
    pub deadline: WorkbenchJobDeadline,
    pub id: WorkbenchJobId,
    pub now: ClockInstant,
}

pub struct DesktopWorkbenchCommandsObservation<'a> {
    pub context: DesktopEngineCommandsContext,
    pub request: DesktopRequestId,
    pub job: &'a WorkbenchJob,
    pub diagnostics: &'a EngineDiagnostics,
    pub execution: Option<&'a WorkbenchExecutorObservation>,
}

pub trait DesktopWorkbenchCommandsObservationProvider: Send {
    fn observe(
        &mut self,
        observation: DesktopWorkbenchCommandsObservation<'_>,
    ) -> Result<(), DesktopFailure>;
}

pub struct DesktopWorkbenchCommands<P: EngineProcess, R> {
    active: Option<WorkbenchExecutor<P>>,
    reservation: Option<R>,
    result: Option<DesktopWorkbenchCommandsResult>,
}

impl<P: EngineProcess, R> Default for DesktopWorkbenchCommands<P, R> {
    fn default() -> Self {
        Self {
            active: None,
            reservation: None,
            result: None,
        }
    }
}

impl<P: EngineProcess, R> DesktopWorkbenchCommands<P, R> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }
    pub fn active_job(&self) -> Option<&WorkbenchJob> {
        self.active.as_ref().map(WorkbenchExecutor::job)
    }
    pub fn latest_result(&self) -> Option<DesktopWorkbenchCommandsResult> {
        self.result.clone()
    }

    pub(crate) fn observe(
        &self,
        context: DesktopEngineCommandsContext,
        request: DesktopRequestId,
        observer: &mut impl DesktopWorkbenchCommandsObservationProvider,
    ) -> Result<(), DesktopFailure> {
        if let Some(active) = &self.active {
            observer.observe(DesktopWorkbenchCommandsObservation {
                context,
                request,
                job: active.job(),
                diagnostics: active.diagnostics(),
                execution: active.observation(),
            })?;
        } else if let Some(result) = &self.result {
            observer.observe(DesktopWorkbenchCommandsObservation {
                context,
                request,
                job: result.job(),
                diagnostics: result.job().diagnostics(),
                execution: result.observation(),
            })?;
        }
        Ok(())
    }
    pub fn result(
        &self,
        id: WorkbenchJobId,
    ) -> Result<DesktopWorkbenchCommandsResult, DesktopFailure> {
        self.result
            .as_ref()
            .filter(|result| result.job().id() == id)
            .cloned()
            .ok_or_else(|| {
                Self::failure("The diagnostic result is unavailable or belongs to another job")
            })
    }

    pub fn resources(
        &self,
        parameters: &WorkbenchEnumParameters,
        capabilities: &EngineCapabilities,
    ) -> Result<EngineResourceConfig, EngineResourceConfigError> {
        let hash = match parameters {
            WorkbenchEnumParameters::Bench(parameters) => parameters.hash_mib(),
            WorkbenchEnumParameters::Inspector(_)
            | WorkbenchEnumParameters::RawEvaluation(_)
            | WorkbenchEnumParameters::Compiler
            | WorkbenchEnumParameters::Perft(_) => {
                EngineResourceConfig::advertised_default(capabilities, EngineEnumOption::Hash)?
            }
        };
        let overhead = ClockDuration::from_millis(u64::from(
            EngineResourceConfig::advertised_default(capabilities, EngineEnumOption::MoveOverhead)?,
        ))?;
        EngineResourceConfig::new(parameters.workers(), hash, overhead)
    }

    pub async fn start(
        &mut self,
        provider: &impl EngineProcessProvider<Process = P>,
        request: DesktopWorkbenchCommandsRequest,
        allocation: DesktopEngineCommandsAllocation<R>,
    ) -> Result<(), DesktopFailure> {
        if self.active.is_some() {
            return Err(Self::failure(
                "A diagnostic still owns its child or cleanup",
            ));
        }
        let mut job = WorkbenchJob::new(
            request.id,
            request.parameters,
            request.package,
            request.deadline,
            request.now,
        )
        .map_err(Self::effect)?;
        let (lease, reservation) = allocation.into_parts();
        job.admit(request.id, lease, request.now)
            .map_err(Self::effect)?;
        let executor = WorkbenchExecutor::new(job, request.now).map_err(Self::effect)?;
        self.reservation = Some(reservation);
        self.active = Some(executor);
        self.result = None;
        self.active
            .as_mut()
            .ok_or_else(|| Self::failure("Missing admitted diagnostic"))?
            .start(provider)
            .await
            .map_err(Self::effect)
    }

    pub async fn run(&mut self) -> Result<(), DesktopFailure> {
        self.active
            .as_mut()
            .ok_or_else(|| Self::failure("No diagnostic is running"))?
            .run()
            .await
            .map_err(Self::effect)
    }

    pub async fn cancel(
        &mut self,
        id: WorkbenchJobId,
        received_at: Instant,
    ) -> Result<(), DesktopFailure> {
        if let Some(active) = &mut self.active {
            if active.job().id() != id {
                return Err(Self::failure("Cancellation belongs to another diagnostic"));
            }
            let deadline = received_at
                .checked_add(crate::engine::search::stop::EngineSearchStop::SAFETY)
                .and_then(|deadline| {
                    deadline
                        .checked_add(crate::engine::process_provider::EngineProcessLimits::SHUTDOWN)
                })
                .ok_or_else(|| {
                    Self::failure(
                        "The diagnostic cancellation receipt exceeds the runtime clock range",
                    )
                })?;
            tokio::time::timeout_at(deadline, async {
                if active.job().cancellation().is_some() {
                    active.shutdown_retained(deadline).await
                } else {
                    active.cancel(id).await
                }
            })
            .await
            .map_err(Self::effect)?
            .map_err(Self::effect)
        } else {
            self.result(id).map(|_result| ())
        }
    }

    pub async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), DesktopFailure> {
        if let Some(active) = &mut self.active {
            active
                .shutdown_retained(deadline)
                .await
                .map_err(Self::effect)?;
        }
        Ok(())
    }

    fn failure(message: &str) -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            message.to_owned(),
            true,
        )
    }
    fn effect(error: impl std::fmt::Display) -> DesktopFailure {
        Self::failure(&error.to_string())
    }
}

impl<P: EngineProcess + Sync + 'static, R> DesktopWorkbenchCommands<P, R> {
    pub fn observe_completion(
        &mut self,
    ) -> Result<Option<DesktopWorkbenchCommandsResult>, DesktopFailure> {
        if self
            .active
            .as_ref()
            .is_none_or(|active| !active.is_shutdown_complete() || active.job().outcome().is_none())
        {
            return Ok(None);
        }
        let active = self
            .active
            .take()
            .ok_or_else(|| Self::failure("The diagnostic owner disappeared"))?;
        let result = DesktopWorkbenchCommandsResult(Arc::new(active));
        self.reservation = None;
        self.result = Some(result.clone());
        Ok(Some(result))
    }
}
