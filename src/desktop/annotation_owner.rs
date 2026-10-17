mod saving;
mod source;

use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

use thiserror::Error;

use crate::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommands, DesktopAnnotationCommandsEnumAction,
    DesktopAnnotationCommandsEnumAdmission, DesktopAnnotationCommandsEnumPending,
    DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::annotation_owner::saving::DesktopAnnotationOwnerSave;
use crate::desktop::annotation_owner::source::DesktopAnnotationOwnerSource;
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::game_editor::{
    DesktopGameEditorContext, DesktopGameEditorError, DesktopGameEditorPublication,
    DesktopGameEditorPublicationError,
};
use crate::game::enums::{GameRevision, GameRevisionError};
use crate::game::storage_repository::{
    GameStorageContentEdit, GameStorageRevision, GameStorageWallTimeError,
    GameStorageWallTimeProvider,
};
use crate::notation::game::NotationGameError;
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::library_record::StorageLibraryRecord;
use crate::storage::library_repository::{StorageLibraryContentReceipt, StorageLibraryError};

#[derive(Debug, Error)]
pub enum DesktopAnnotationOwnerError {
    #[error(transparent)]
    Editor(#[from] DesktopGameEditorError),
    #[error(transparent)]
    Publication(#[from] DesktopGameEditorPublicationError),
    #[error(transparent)]
    Notation(#[from] NotationGameError),
    #[error(transparent)]
    Revision(#[from] GameRevisionError),
    #[error(transparent)]
    WallTime(#[from] GameStorageWallTimeError),
    #[error(transparent)]
    Executor(#[from] CapacityExecutorError),
    #[error("The annotation save failed: {0}")]
    Storage(#[source] Box<StorageLibraryError>),
    #[error("The annotation source is busy")]
    Busy,
    #[error("The annotation source lock was poisoned; ownership is retained")]
    Poisoned,
    #[error("The initial annotation snapshot has not been observed")]
    NotReady,
    #[error("The annotation computation is still owned")]
    ComputePending,
    #[error("The annotation save is still owned")]
    SavePending,
    #[error(
        "Annotation storage revision {storage_revision:?} is committed and its covered revision {covered} awaits projection"
    )]
    ProjectionPending {
        storage_revision: GameStorageRevision,
        covered: GameRevision,
    },
    #[error("The accepted annotation context {context:?} awaits publication")]
    PublicationPending { context: DesktopGameEditorContext },
    #[error("Save preparation still borrows the canonical annotation record")]
    SaveBorrowed,
    #[error("The application receipt does not own this pending annotation operation")]
    ForeignReceipt,
    #[error("No annotation computation is pending")]
    NoCompute,
    #[error("No annotation save is pending")]
    NoSave,
    #[error("The annotation variation token does not identify the retained proof")]
    VariationToken,
    #[error("The annotation confirmation scope offset is outside its retained proof")]
    ScopeOffset,
    #[error("The annotation action was cancelled before acceptance")]
    Cancelled,
    #[error("The annotation save failed and requires explicit Retry Save")]
    RetryRequired,
    #[error("The annotation operation returned an incompatible result")]
    Protocol,
    #[error("The retained annotation failure is: {0}")]
    Retained(#[source] Arc<Self>),
    #[error("Annotation completion failed ({operation}) and observation failed ({observation})")]
    CompletionObservation {
        #[source]
        operation: Box<Self>,
        observation: Box<Self>,
    },
}

impl From<StorageLibraryError> for DesktopAnnotationOwnerError {
    fn from(error: StorageLibraryError) -> Self {
        Self::Storage(Box::new(error))
    }
}

pub struct DesktopAnnotationOwnerResources {
    pub executor: Arc<CapacityExecutor>,
    pub client: StorageDocumentClient,
    pub wall_time: Arc<dyn GameStorageWallTimeProvider>,
}

#[derive(Error)]
#[error("{reason}")]
pub struct DesktopAnnotationOwnerOpenError {
    record: StorageLibraryRecord,
    #[source]
    reason: DesktopAnnotationOwnerError,
}

impl Debug for DesktopAnnotationOwnerOpenError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DesktopAnnotationOwnerOpenError")
            .field("game", &self.record.id())
            .field("reason", &self.reason)
            .finish()
    }
}

impl DesktopAnnotationOwnerOpenError {
    pub(super) const fn new(
        record: StorageLibraryRecord,
        reason: DesktopAnnotationOwnerError,
    ) -> Self {
        Self { record, reason }
    }

    pub fn into_parts(self) -> (StorageLibraryRecord, DesktopAnnotationOwnerError) {
        (self.record, self.reason)
    }
}

pub struct DesktopAnnotationOwnerRead<'a> {
    snapshot: &'a DesktopGameEditorPublication,
    pending: Option<DesktopAnnotationCommandsEnumPending>,
}

impl DesktopAnnotationOwnerRead<'_> {
    pub const fn snapshot(&self) -> &DesktopGameEditorPublication {
        self.snapshot
    }

    pub const fn pending(&self) -> Option<DesktopAnnotationCommandsEnumPending> {
        self.pending
    }
}

#[derive(Clone)]
pub struct DesktopAnnotationOwnerStatus {
    accepted: Option<GameStorageContentEdit>,
    durable: GameRevision,
    compute: bool,
    save: bool,
    failure: Option<Arc<DesktopAnnotationOwnerError>>,
}

impl DesktopAnnotationOwnerStatus {
    pub const fn accepted_edit(&self) -> Option<GameStorageContentEdit> {
        self.accepted
    }

    pub const fn durable_revision(&self) -> GameRevision {
        self.durable
    }

    pub fn is_dirty(&self) -> bool {
        self.accepted
            .is_some_and(|edit| edit.revision() > self.durable)
    }

    pub const fn has_compute(&self) -> bool {
        self.compute
    }

    pub const fn has_save(&self) -> bool {
        self.save
    }

    pub fn failure(&self) -> Option<&Arc<DesktopAnnotationOwnerError>> {
        self.failure.as_ref()
    }
}

#[derive(Debug, Clone)]
pub enum DesktopAnnotationOwnerEnumSaveProgress {
    Committing,
    Saved {
        receipt: Arc<StorageLibraryContentReceipt>,
        covered: GameRevision,
    },
}

enum DesktopAnnotationOwnerComputeEnumPhase {
    Running(
        Pin<
            Box<
                dyn Future<
                        Output = Result<
                            DesktopAnnotationCommandsEnumResult,
                            DesktopAnnotationOwnerError,
                        >,
                    > + Send
                    + 'static,
            >,
        >,
    ),
    Publishing,
}

struct DesktopAnnotationOwnerCompute {
    receipt: DesktopApplicationReceipt,
    phase: DesktopAnnotationOwnerComputeEnumPhase,
}

enum DesktopAnnotationOwnerEnumOperation {
    Open,
    Command {
        expected: DesktopGameEditorContext,
        action: Box<DesktopAnnotationCommandsEnumAction>,
    },
    Publish,
}

pub struct DesktopAnnotationOwner {
    source: Arc<Mutex<DesktopAnnotationOwnerSource>>,
    resources: DesktopAnnotationOwnerResources,
    snapshot: Option<DesktopGameEditorPublication>,
    publishing: Option<DesktopGameEditorContext>,
    accepted: Option<GameStorageContentEdit>,
    durable: GameRevision,
    compute: Option<DesktopAnnotationOwnerCompute>,
    save: Option<DesktopAnnotationOwnerSave>,
    failure: Option<Arc<DesktopAnnotationOwnerError>>,
    save_failed: bool,
}

impl DesktopAnnotationOwner {
    pub fn open(
        record: StorageLibraryRecord,
        storage_revision: GameStorageRevision,
        receipt: DesktopApplicationReceipt,
        resources: DesktopAnnotationOwnerResources,
    ) -> Result<Self, DesktopAnnotationOwnerOpenError> {
        let source = DesktopAnnotationOwnerSource::new(
            record,
            storage_revision,
            &receipt,
            resources.client.clone(),
        )?;
        let mut owner = Self {
            source: Arc::new(Mutex::new(source)),
            resources,
            snapshot: None,
            publishing: None,
            accepted: None,
            durable: GameRevision::ZERO,
            compute: None,
            save: None,
            failure: None,
            save_failed: false,
        };
        owner.start_compute(receipt, DesktopAnnotationOwnerEnumOperation::Open);
        Ok(owner)
    }

    pub fn snapshot(&self) -> Result<&DesktopGameEditorPublication, DesktopAnnotationOwnerError> {
        if let Some(context) = self.publishing {
            return Err(DesktopAnnotationOwnerError::PublicationPending { context });
        }
        if let Some(DesktopAnnotationCommandsEnumPending::Projecting {
            storage_revision,
            covered,
        }) = self.pending()
        {
            return Err(DesktopAnnotationOwnerError::ProjectionPending {
                storage_revision,
                covered,
            });
        }
        if matches!(
            self.pending(),
            Some(DesktopAnnotationCommandsEnumPending::Unresolved)
        ) {
            return Err(DesktopAnnotationOwnerError::SavePending);
        }
        self.snapshot
            .as_ref()
            .ok_or(DesktopAnnotationOwnerError::NotReady)
    }

    pub fn read_snapshot(
        &self,
    ) -> Result<DesktopAnnotationOwnerRead<'_>, DesktopAnnotationOwnerError> {
        Ok(DesktopAnnotationOwnerRead {
            snapshot: self
                .snapshot
                .as_ref()
                .ok_or(DesktopAnnotationOwnerError::NotReady)?,
            pending: self.pending(),
        })
    }

    fn pending(&self) -> Option<DesktopAnnotationCommandsEnumPending> {
        if let Some(context) = self.publishing {
            return Some(DesktopAnnotationCommandsEnumPending::Publishing { context });
        }
        match &self.save {
            Some(save) => Some(save.pending(self.compute.is_some())),
            None => self
                .compute
                .as_ref()
                .map(|_compute| DesktopAnnotationCommandsEnumPending::Computing),
        }
    }

    pub fn status(&self) -> DesktopAnnotationOwnerStatus {
        DesktopAnnotationOwnerStatus {
            accepted: self.accepted,
            durable: self.durable,
            compute: self.compute.is_some(),
            save: self.save.is_some(),
            failure: self.failure.as_ref().map(Arc::clone),
        }
    }

    pub fn submit(
        &mut self,
        command: DesktopAnnotationCommands,
    ) -> Result<DesktopAnnotationCommandsEnumAdmission, DesktopAnnotationOwnerError> {
        let (receipt, expected, action) = command.into_parts();
        self.snapshot()?.context().check_expected(expected)?;
        if receipt.is_cancellation_requested() {
            return Err(DesktopAnnotationOwnerError::Cancelled);
        }
        if matches!(action, DesktopAnnotationCommandsEnumAction::Snapshot) {
            if let Some(pending) = self.pending() {
                return Ok(DesktopAnnotationCommandsEnumAdmission::PendingSnapshot {
                    snapshot: self.snapshot()?.clone(),
                    pending,
                });
            }
            return Ok(DesktopAnnotationCommandsEnumAdmission::Ready(Box::new(
                DesktopAnnotationCommandsEnumResult::Snapshot(self.snapshot()?.clone()),
            )));
        }
        if self.compute.is_some() {
            return Err(DesktopAnnotationOwnerError::ComputePending);
        }
        if self
            .save
            .as_ref()
            .is_some_and(DesktopAnnotationOwnerSave::is_projecting)
        {
            return Err(DesktopAnnotationOwnerError::SavePending);
        }
        {
            let source = Self::lock(&self.source)?;
            source.context()?.check_expected(expected)?;
            source.check_action(&action)?;
        }
        match action {
            DesktopAnnotationCommandsEnumAction::Checkpoint
            | DesktopAnnotationCommandsEnumAction::RetrySave => {
                let retry = matches!(action, DesktopAnnotationCommandsEnumAction::RetrySave);
                self.start_save(receipt, retry)?;
                Ok(DesktopAnnotationCommandsEnumAdmission::SavePending)
            }
            action @ (DesktopAnnotationCommandsEnumAction::Snapshot
            | DesktopAnnotationCommandsEnumAction::TreePage { .. }
            | DesktopAnnotationCommandsEnumAction::ChildrenPage { .. }
            | DesktopAnnotationCommandsEnumAction::Navigate { .. }
            | DesktopAnnotationCommandsEnumAction::SaveNotesAndNags { .. }
            | DesktopAnnotationCommandsEnumAction::SaveMarkings { .. }
            | DesktopAnnotationCommandsEnumAction::AppendStudyBranch { .. }
            | DesktopAnnotationCommandsEnumAction::PreviewVariation(_)
            | DesktopAnnotationCommandsEnumAction::VariationScope { .. }
            | DesktopAnnotationCommandsEnumAction::ConfirmVariation { .. }) => {
                self.start_compute(
                    receipt,
                    DesktopAnnotationOwnerEnumOperation::Command {
                        expected,
                        action: Box::new(action),
                    },
                );
                Ok(DesktopAnnotationCommandsEnumAdmission::ComputePending)
            }
        }
    }

    fn start_compute(
        &mut self,
        receipt: DesktopApplicationReceipt,
        operation: DesktopAnnotationOwnerEnumOperation,
    ) {
        let phase = self.compute_phase(&receipt, operation);
        self.compute = Some(DesktopAnnotationOwnerCompute { receipt, phase });
    }

    fn compute_phase(
        &self,
        receipt: &DesktopApplicationReceipt,
        operation: DesktopAnnotationOwnerEnumOperation,
    ) -> DesktopAnnotationOwnerComputeEnumPhase {
        let source = Arc::clone(&self.source);
        let wall_time = Arc::clone(&self.resources.wall_time);
        let cancellation = receipt.cancellation();
        let token = receipt.request_id();
        let executor = Arc::clone(&self.resources.executor);
        let future = Box::pin(async move {
            executor
                .submit_waiting(move |cpu_cancellation| {
                    if cpu_cancellation.is_cancelled()
                        || (cancellation.is_requested()
                            && !matches!(operation, DesktopAnnotationOwnerEnumOperation::Publish))
                    {
                        return Err(DesktopAnnotationOwnerError::Cancelled);
                    }
                    let mut source = source
                        .lock()
                        .map_err(|_error| DesktopAnnotationOwnerError::Poisoned)?;
                    match operation {
                        DesktopAnnotationOwnerEnumOperation::Command { expected, action } => {
                            source.execute(expected, *action, token, wall_time.as_ref())
                        }
                        DesktopAnnotationOwnerEnumOperation::Open => source.publish_snapshot(),
                        DesktopAnnotationOwnerEnumOperation::Publish => source.retry_publication(),
                    }
                })
                .await?
                .join()
                .await?
        });
        DesktopAnnotationOwnerComputeEnumPhase::Running(future)
    }

    pub async fn observe_compute(
        &mut self,
        receipt: &DesktopApplicationReceipt,
    ) -> Result<DesktopAnnotationCommandsEnumResult, DesktopAnnotationOwnerError> {
        let pending = self
            .compute
            .as_ref()
            .ok_or(DesktopAnnotationOwnerError::NoCompute)?;
        if !pending.receipt.shares_outcome(receipt) {
            return Err(DesktopAnnotationOwnerError::ForeignReceipt);
        }
        if matches!(
            pending.phase,
            DesktopAnnotationOwnerComputeEnumPhase::Publishing
        ) {
            let phase = self.compute_phase(receipt, DesktopAnnotationOwnerEnumOperation::Publish);
            self.compute
                .as_mut()
                .ok_or(DesktopAnnotationOwnerError::NoCompute)?
                .phase = phase;
        }
        let pending = self
            .compute
            .as_mut()
            .ok_or(DesktopAnnotationOwnerError::NoCompute)?;
        let DesktopAnnotationOwnerComputeEnumPhase::Running(future) = &mut pending.phase else {
            return Err(DesktopAnnotationOwnerError::Protocol);
        };
        let result = future.as_mut().await;
        let retained_receipt = pending.receipt.clone();
        self.compute = None;
        let observation = self.refresh_observation();
        if self.publishing.is_some() {
            self.compute = Some(DesktopAnnotationOwnerCompute {
                receipt: retained_receipt,
                phase: DesktopAnnotationOwnerComputeEnumPhase::Publishing,
            });
        } else if result.is_ok()
            && self.failure.as_ref().is_some_and(|error| {
                matches!(error.as_ref(), DesktopAnnotationOwnerError::Publication(_))
            })
        {
            self.failure = None;
        }
        match (result, observation) {
            (Ok(result), Ok(())) => match self.pending() {
                Some(DesktopAnnotationCommandsEnumPending::Projecting {
                    storage_revision,
                    covered,
                }) => Ok(DesktopAnnotationCommandsEnumResult::PendingProjection {
                    result: Box::new(result),
                    storage_revision,
                    covered,
                }),
                Some(DesktopAnnotationCommandsEnumPending::Unresolved) => {
                    Err(DesktopAnnotationOwnerError::SavePending)
                }
                Some(
                    DesktopAnnotationCommandsEnumPending::Computing
                    | DesktopAnnotationCommandsEnumPending::Saving { .. }
                    | DesktopAnnotationCommandsEnumPending::Publishing { .. }
                    | DesktopAnnotationCommandsEnumPending::Preparing { .. }
                    | DesktopAnnotationCommandsEnumPending::Committing { .. },
                )
                | None => Ok(result),
            },
            (Err(operation), Err(observation)) => Err(self.retain_failure(
                DesktopAnnotationOwnerError::CompletionObservation {
                    operation: Box::new(operation),
                    observation: Box::new(observation),
                },
            )),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(self.retain_failure(error)),
        }
    }

    fn refresh_observation(&mut self) -> Result<(), DesktopAnnotationOwnerError> {
        let source = Self::lock(&self.source)?;
        self.snapshot = source.cached_snapshot().cloned();
        self.accepted = source.accepted();
        self.publishing = source.publication_context()?;
        Ok(())
    }

    fn retain_failure(
        &mut self,
        error: DesktopAnnotationOwnerError,
    ) -> DesktopAnnotationOwnerError {
        let failure = Arc::new(error);
        if self.failure.is_none() {
            self.failure = Some(Arc::clone(&failure));
        }
        DesktopAnnotationOwnerError::Retained(failure)
    }

    fn lock(
        source: &Mutex<DesktopAnnotationOwnerSource>,
    ) -> Result<MutexGuard<'_, DesktopAnnotationOwnerSource>, DesktopAnnotationOwnerError> {
        source.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => DesktopAnnotationOwnerError::Busy,
            TryLockError::Poisoned(_error) => DesktopAnnotationOwnerError::Poisoned,
        })
    }
}

#[cfg(test)]
#[path = "../../tests/desktop/annotation_owner.rs"]
mod tests;
