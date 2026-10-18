use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, watch};

use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::engine_commands::{
    DesktopEngineCommandsOperation, DesktopEngineCommandsSnapshot,
};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::file_handle::DesktopFileEnumSelection;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::workbench_commands::DesktopWorkbenchCommandsResult;
use crate::engine::search::session::{
    EngineSearchSessionEnumEvent, EngineSearchSessionMetadata, EngineSearchSessionSnapshot,
    EngineSearchSessionTerminal,
};
use crate::workbench::job::WorkbenchJobId;

pub struct DesktopEngineBridgeRequest {
    pub operation: DesktopEngineCommandsOperation,
    pub receipt: DesktopApplicationReceipt,
    pub selection: Option<DesktopFileEnumSelection>,
}

#[derive(Debug, Clone)]
pub enum DesktopEngineBridgeEnumResult {
    Snapshot(DesktopEngineCommandsSnapshot),
    Started(EngineSearchSessionMetadata),
    Stopped(EngineSearchSessionTerminal),
    WorkbenchStarted(WorkbenchJobId),
    WorkbenchCancelled(WorkbenchJobId),
    WorkbenchResult(DesktopWorkbenchCommandsResult),
    ExportPreview(Arc<crate::desktop::workbench_export::DesktopWorkbenchExportPreview>),
    Exported {
        preview: DesktopRequestId,
        receipt: crate::workbench::export::WorkbenchExportReceipt,
    },
}

#[derive(Debug, Clone)]
pub enum DesktopEngineBridgeEnumEvent {
    Analysis(Arc<EngineSearchSessionEnumEvent>),
    Workbench(DesktopWorkbenchCommandsResult),
    Failure(DesktopFailure),
}

pub struct DesktopEngineBridgeStreams {
    pub telemetry: watch::Receiver<Option<EngineSearchSessionSnapshot>>,
    pub terminal: mpsc::Receiver<DesktopEngineBridgeEnumEvent>,
}

pub type DesktopEngineBridgeFuture = Pin<
    Box<
        dyn Future<Output = Result<DesktopEngineBridgeEnumResult, DesktopFailure>> + Send + 'static,
    >,
>;

pub trait DesktopEngineBridgeProvider: Send + Sync {
    fn execute(&self, request: DesktopEngineBridgeRequest) -> DesktopEngineBridgeFuture;
    fn quiesce(
        &self,
        deadline: tokio::time::Instant,
    ) -> Pin<Box<dyn Future<Output = Result<(), DesktopFailure>> + Send>>;
}

pub(crate) struct DesktopEngineBridgeEnvelope {
    pub request: DesktopEngineBridgeRequest,
    pub response: oneshot::Sender<Result<DesktopEngineBridgeEnumResult, DesktopFailure>>,
}

#[derive(Clone, Copy)]
pub(crate) enum DesktopEngineBridgeEnumLifecycle {
    Shutdown,
    Quiesce,
}

pub(crate) struct DesktopEngineBridgeLifecycle {
    pub action: DesktopEngineBridgeEnumLifecycle,
    pub received_at: tokio::time::Instant,
    pub deadline: tokio::time::Instant,
    pub response: oneshot::Sender<Result<(), DesktopFailure>>,
}

#[derive(Clone)]
pub struct DesktopEngineBridgePort {
    pub(crate) ordinary: mpsc::Sender<DesktopEngineBridgeEnvelope>,
    pub(crate) urgent: mpsc::Sender<DesktopEngineBridgeEnvelope>,
    pub(crate) lifecycle: mpsc::Sender<DesktopEngineBridgeLifecycle>,
}

impl DesktopEngineBridgePort {
    pub const ORDINARY_CAPACITY: usize = 256;
    pub const URGENT_CAPACITY: usize = 2;
    pub const EVENT_CAPACITY: usize = 64;
    pub(crate) const LIFECYCLE_CAPACITY: usize = 1;

    pub fn shutdown(
        &self,
        deadline: tokio::time::Instant,
    ) -> Pin<Box<dyn Future<Output = Result<(), DesktopFailure>> + Send>> {
        self.lifecycle(DesktopEngineBridgeEnumLifecycle::Shutdown, deadline)
    }

    pub fn quiesce(
        &self,
        deadline: tokio::time::Instant,
    ) -> Pin<Box<dyn Future<Output = Result<(), DesktopFailure>> + Send>> {
        self.lifecycle(DesktopEngineBridgeEnumLifecycle::Quiesce, deadline)
    }

    fn lifecycle(
        &self,
        action: DesktopEngineBridgeEnumLifecycle,
        deadline: tokio::time::Instant,
    ) -> Pin<Box<dyn Future<Output = Result<(), DesktopFailure>> + Send>> {
        let (response, receiver) = oneshot::channel();
        let admitted = self
            .lifecycle
            .try_send(DesktopEngineBridgeLifecycle {
                action,
                received_at: tokio::time::Instant::now(),
                deadline,
                response,
            })
            .map_err(|error| {
                DesktopFailure::new(
                    DesktopEnumFailureCode::ResourceRefusal,
                    error.to_string(),
                    true,
                )
            });
        Box::pin(async move {
            admitted?;
            receiver.await.map_err(|error| {
                DesktopFailure::new(
                    DesktopEnumFailureCode::ResourceRefusal,
                    error.to_string(),
                    true,
                )
            })?
        })
    }
}

impl DesktopEngineBridgeProvider for DesktopEngineBridgePort {
    fn quiesce(
        &self,
        deadline: tokio::time::Instant,
    ) -> Pin<Box<dyn Future<Output = Result<(), DesktopFailure>> + Send>> {
        DesktopEngineBridgePort::quiesce(self, deadline)
    }

    fn execute(&self, request: DesktopEngineBridgeRequest) -> DesktopEngineBridgeFuture {
        let (response, receiver) = oneshot::channel();
        let queue = if request.operation.action().is_control() {
            &self.urgent
        } else {
            &self.ordinary
        };
        let admitted = queue
            .try_send(DesktopEngineBridgeEnvelope { request, response })
            .map_err(|error| {
                let message = match error {
                    mpsc::error::TrySendError::Full(_) => "The native engine command queue is full",
                    mpsc::error::TrySendError::Closed(_) => {
                        "The native engine command owner is unavailable"
                    }
                };
                DesktopFailure::new(
                    DesktopEnumFailureCode::ResourceRefusal,
                    message.to_owned(),
                    true,
                )
            });
        Box::pin(async move {
            admitted?;
            receiver.await.map_err(|_closed| {
                DesktopFailure::new(
                    DesktopEnumFailureCode::ResourceRefusal,
                    "The engine command outcome is unresolved; reconcile before retrying"
                        .to_owned(),
                    true,
                )
            })?
        })
    }
}
