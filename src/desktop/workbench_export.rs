use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use thiserror::Error;
use tokio::time::{Instant, timeout_at};

use crate::capacity::executor::{
    CapacityExecutor, CapacityExecutorCancellation, CapacityExecutorError,
};
use crate::desktop::application::{DesktopApplicationCancellation, DesktopApplicationReceipt};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::file_handle::{DesktopFileEnumSelection, DesktopFileHandleError};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::workbench_commands::DesktopWorkbenchCommandsResult;
use crate::package::digest::PackageDigest;
use crate::storage::atomic_file::{StorageAtomicFileError, StorageAtomicFileStage};
use crate::workbench::export::{
    WorkbenchExport, WorkbenchExportEncoded, WorkbenchExportError, WorkbenchExportMetadata,
    WorkbenchExportPort, WorkbenchExportReceipt, WorkbenchExportWriteError,
};
use crate::workbench::job::WorkbenchJobId;

#[cfg(test)]
#[path = "../../tests/desktop/workbench_export.rs"]
mod tests;

#[derive(Debug, Error)]
pub enum DesktopWorkbenchExportError {
    #[error(transparent)]
    Export(#[from] WorkbenchExportError),
    #[error(transparent)]
    Selection(#[from] DesktopFileHandleError),
    #[error(transparent)]
    Staging(#[from] StorageAtomicFileError),
    #[error(transparent)]
    Write(#[from] WorkbenchExportWriteError<StorageAtomicFileError>),
    #[error(transparent)]
    Capacity(#[from] CapacityExecutorError),
    #[error("The native file job ended without a confirmed publication outcome: {0}")]
    UnresolvedWrite(#[source] CapacityExecutorError),
}

impl DesktopWorkbenchExportError {
    fn is_cancelled(&self) -> bool {
        matches!(
            self,
            Self::Staging(StorageAtomicFileError::Cancelled)
                | Self::Write(WorkbenchExportWriteError::Cancelled)
                | Self::Write(WorkbenchExportWriteError::Destination(
                    StorageAtomicFileError::Cancelled
                ))
        )
    }

    fn is_indeterminate(&self) -> bool {
        matches!(
            self,
            Self::UnresolvedWrite(_)
                | Self::Write(WorkbenchExportWriteError::Destination(
                    StorageAtomicFileError::PublicationIndeterminate(_)
                ))
        )
    }

    fn failure(&self) -> DesktopFailure {
        let code = if self.is_cancelled() {
            DesktopEnumFailureCode::Cancelled
        } else {
            match self {
                Self::Export(_) | Self::Write(WorkbenchExportWriteError::Export(_)) => {
                    DesktopEnumFailureCode::InvalidInput
                }
                Self::Selection(_)
                | Self::Staging(_)
                | Self::UnresolvedWrite(_)
                | Self::Write(WorkbenchExportWriteError::Destination(_)) => {
                    DesktopEnumFailureCode::FileUnavailable
                }
                Self::Capacity(_) => DesktopEnumFailureCode::ResourceRefusal,
                Self::Write(WorkbenchExportWriteError::Cancelled) => {
                    DesktopEnumFailureCode::Cancelled
                }
            }
        };
        DesktopFailure::new(
            code,
            self.to_string(),
            !self.is_indeterminate()
                && matches!(
                    code,
                    DesktopEnumFailureCode::FileUnavailable
                        | DesktopEnumFailureCode::ResourceRefusal
                ),
        )
    }
}

#[derive(Debug)]
pub struct DesktopWorkbenchExportPreview {
    id: DesktopRequestId,
    job: WorkbenchJobId,
    encoded: WorkbenchExportEncoded,
    diagnostic_lines: usize,
    evicted_lines: u64,
    evicted_bytes: u64,
}

impl DesktopWorkbenchExportPreview {
    pub const fn id(&self) -> DesktopRequestId {
        self.id
    }
    pub const fn job(&self) -> WorkbenchJobId {
        self.job
    }
    pub fn bytes(&self) -> &[u8] {
        self.encoded.bytes()
    }
    pub const fn length(&self) -> u64 {
        self.encoded.length()
    }
    pub const fn digest(&self) -> PackageDigest {
        self.encoded.sha256()
    }
    pub const fn diagnostic_lines(&self) -> usize {
        self.diagnostic_lines
    }
    pub const fn evicted_lines(&self) -> u64 {
        self.evicted_lines
    }
    pub const fn evicted_bytes(&self) -> u64 {
        self.evicted_bytes
    }
}

pub struct DesktopWorkbenchExportRequest {
    pub preview: DesktopRequestId,
    pub digest: PackageDigest,
    pub destination: DesktopFileEnumSelection,
    pub receipt: DesktopApplicationReceipt,
}

type DesktopWorkbenchExportOutcome =
    Result<WorkbenchExportReceipt, Arc<DesktopWorkbenchExportError>>;
type DesktopWorkbenchExportFuture =
    Pin<Box<dyn Future<Output = DesktopWorkbenchExportOutcome> + Send>>;

struct DesktopWorkbenchExportWrite {
    preview: DesktopRequestId,
    digest: PackageDigest,
    receipt: DesktopApplicationReceipt,
    future: DesktopWorkbenchExportFuture,
}

struct DesktopWorkbenchExportCompletion {
    preview: DesktopRequestId,
    digest: PackageDigest,
    receipt: DesktopApplicationReceipt,
    outcome: DesktopWorkbenchExportOutcome,
}

#[derive(Default)]
pub struct DesktopWorkbenchExport {
    preview: Option<Arc<DesktopWorkbenchExportPreview>>,
    writing: Option<DesktopWorkbenchExportWrite>,
    completed: Option<DesktopWorkbenchExportCompletion>,
}

impl DesktopWorkbenchExport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_pending(&self) -> bool {
        self.writing.is_some()
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.require_available().is_ok()
    }

    pub fn outcome(
        &self,
        request: DesktopRequestId,
    ) -> Option<Result<WorkbenchExportReceipt, DesktopFailure>> {
        self.completed
            .as_ref()
            .filter(|completed| completed.receipt.request_id() == request)
            .map(|completed| {
                completed
                    .outcome
                    .as_ref()
                    .copied()
                    .map_err(|error| error.failure())
            })
    }

    pub fn error(&self) -> Option<&DesktopWorkbenchExportError> {
        self.completed
            .as_ref()
            .and_then(|completed| completed.outcome.as_ref().err())
            .map(Arc::as_ref)
    }

    pub fn preview(
        &mut self,
        request: DesktopRequestId,
        result: &DesktopWorkbenchCommandsResult,
    ) -> Result<Arc<DesktopWorkbenchExportPreview>, DesktopFailure> {
        if let Some(preview) = &self.preview
            && preview.id == request
        {
            if preview.job != result.job().id() {
                return Err(Self::conflict());
            }
            return Ok(Arc::clone(preview));
        }
        self.require_available()?;
        let context = result.export_context();
        let export = WorkbenchExport::capture(result.job(), &context).map_err(Self::effect)?;
        let metadata = WorkbenchExportMetadata::with_optional_timestamp(
            request.as_uuid(),
            None,
            env!("CARGO_PKG_VERSION").to_owned(),
        )
        .map_err(Self::effect)?;
        let encoded = export.prepare(&metadata).map_err(Self::effect)?;
        let preview = Arc::new(DesktopWorkbenchExportPreview {
            id: request,
            job: result.job().id(),
            encoded,
            diagnostic_lines: export.diagnostic_lines(),
            evicted_lines: export.evicted_lines(),
            evicted_bytes: export.evicted_bytes(),
        });
        self.preview = Some(Arc::clone(&preview));
        Ok(preview)
    }

    pub fn begin_apply(
        &mut self,
        request: DesktopWorkbenchExportRequest,
        executor: &CapacityExecutor,
    ) -> Result<(), DesktopFailure> {
        let preview = self
            .preview
            .as_ref()
            .filter(|preview| preview.id == request.preview)
            .ok_or_else(|| {
                Self::refusal("The diagnostic export preview is stale; preview the result again")
            })?;
        if preview.digest() != request.digest {
            return Err(DesktopFailure::new(
                DesktopEnumFailureCode::InvalidInput,
                "The diagnostic export digest differs from the frozen preview".to_owned(),
                false,
            ));
        }
        if let Some(writing) = &self.writing {
            return Self::check_retry(&request, writing.preview, writing.digest, &writing.receipt);
        }
        if let Some(completed) = &self.completed
            && completed.receipt.request_id() == request.receipt.request_id()
        {
            return Self::check_retry(
                &request,
                completed.preview,
                completed.digest,
                &completed.receipt,
            );
        }
        self.require_available()?;
        if request.receipt.is_cancellation_requested() {
            return Err(Self::cancelled());
        }
        let frozen = Arc::clone(preview);
        let cancellation = request.receipt.cancellation();
        let destination = request.destination;
        let job = executor
            .submit(move |worker| {
                if worker.is_cancelled() || cancellation.is_requested() {
                    return Err(DesktopWorkbenchExportError::Write(
                        WorkbenchExportWriteError::Cancelled,
                    ));
                }
                let mut port = DesktopWorkbenchExportPort {
                    stage: Some(StorageAtomicFileStage::new(
                        destination.into_destination()?,
                    )?),
                    worker,
                    cancellation: &cancellation,
                };
                Ok(frozen.encoded.write(&mut port)?)
            })
            .map_err(Self::effect)?;
        self.completed = None;
        self.writing = Some(DesktopWorkbenchExportWrite {
            preview: request.preview,
            digest: request.digest,
            receipt: request.receipt,
            future: Box::pin(async move {
                job.join()
                    .await
                    .map_err(DesktopWorkbenchExportError::UnresolvedWrite)
                    .and_then(|result| result)
                    .map_err(Arc::new)
            }),
        });
        Ok(())
    }

    pub async fn apply(
        &mut self,
        request: DesktopWorkbenchExportRequest,
        executor: &CapacityExecutor,
    ) -> Result<WorkbenchExportReceipt, DesktopFailure> {
        self.begin_apply(request, executor)?;
        self.observe().await
    }

    pub async fn observe(&mut self) -> Result<WorkbenchExportReceipt, DesktopFailure> {
        if let Some(writing) = &mut self.writing {
            let outcome = writing.future.as_mut().await;
            let writing = self.writing.take().ok_or_else(|| {
                Self::refusal("The diagnostic export lost its native write owner")
            })?;
            self.completed = Some(DesktopWorkbenchExportCompletion {
                preview: writing.preview,
                digest: writing.digest,
                receipt: writing.receipt,
                outcome,
            });
        }
        self.completed
            .as_ref()
            .ok_or_else(|| Self::refusal("No diagnostic export write has been admitted"))?
            .outcome
            .as_ref()
            .copied()
            .map_err(|error| error.failure())
    }

    pub async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), DesktopFailure> {
        if let Some(writing) = &self.writing {
            writing.receipt.cancellation().request();
            let outcome = timeout_at(deadline, self.observe())
                .await
                .map_err(|error| Self::refusal(&error.to_string()))?;
            if let Err(failure) = outcome
                && !matches!(failure.code(), DesktopEnumFailureCode::Cancelled)
            {
                return Err(failure);
            }
        }
        self.require_available()
    }

    fn require_available(&self) -> Result<(), DesktopFailure> {
        if self.is_pending() {
            return Err(Self::refusal(
                "Observe the pending diagnostic export before preparing another write",
            ));
        }
        if let Some(error) = self.error()
            && error.is_indeterminate()
        {
            return Err(error.failure());
        }
        Ok(())
    }

    fn check_retry(
        request: &DesktopWorkbenchExportRequest,
        preview: DesktopRequestId,
        digest: PackageDigest,
        receipt: &DesktopApplicationReceipt,
    ) -> Result<(), DesktopFailure> {
        if receipt.shares_outcome(&request.receipt)
            && preview == request.preview
            && digest == request.digest
        {
            Ok(())
        } else if receipt.request_id() == request.receipt.request_id() {
            Err(Self::conflict())
        } else {
            Err(Self::refusal(
                "Observe the pending diagnostic export before starting another write",
            ))
        }
    }

    fn conflict() -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::RequestConflict,
            "The diagnostic export request identity belongs to another action".to_owned(),
            false,
        )
    }

    fn effect(error: impl Into<DesktopWorkbenchExportError>) -> DesktopFailure {
        error.into().failure()
    }

    fn refusal(message: &str) -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            message.to_owned(),
            true,
        )
    }

    fn cancelled() -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::Cancelled,
            "The diagnostic export was cancelled before publication".to_owned(),
            false,
        )
    }
}

struct DesktopWorkbenchExportPort<'a> {
    stage: Option<StorageAtomicFileStage>,
    worker: &'a CapacityExecutorCancellation,
    cancellation: &'a DesktopApplicationCancellation,
}

impl WorkbenchExportPort for DesktopWorkbenchExportPort<'_> {
    type Error = StorageAtomicFileError;

    fn is_cancelled(&self) -> bool {
        self.worker.is_cancelled() || self.cancellation.is_requested()
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.stage
            .as_mut()
            .ok_or(StorageAtomicFileError::InvalidLocation)?
            .write(bytes)
    }

    fn commit(&mut self) -> Result<(), Self::Error> {
        self.stage
            .take()
            .ok_or(StorageAtomicFileError::InvalidLocation)?
            .commit_if(|| self.is_cancelled())?;
        Ok(())
    }

    fn abort(&mut self) {
        drop(self.stage.take());
    }
}
