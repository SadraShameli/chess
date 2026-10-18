use std::sync::Arc;

use crate::board::editor::BoardEditorDraft;
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::editor_commands::{
    DesktopEditorCommandsContext, DesktopEditorCommandsEnumWriteStatus, DesktopEditorCommandsError,
};
use crate::storage::editor_repository::{
    StorageEditorRepository, StorageEditorRepositoryEnumMutation, StorageEditorRepositoryError,
    StorageEditorRepositoryReceipt, StorageEditorRepositoryWrite,
};

pub(super) struct DesktopEditorCommandsWrite {
    write: StorageEditorRepositoryWrite,
    context: Option<DesktopEditorCommandsContext>,
    draft: Option<BoardEditorDraft>,
    receipt: Option<StorageEditorRepositoryReceipt>,
    failure: Option<Arc<StorageEditorRepositoryError>>,
    application_receipt: DesktopApplicationReceipt,
    continuation: DesktopEditorCommandsWriteEnumContinuation,
}

pub(super) enum DesktopEditorCommandsWriteEnumContinuation {
    Application(DesktopApplicationReceipt),
    Shutdown,
}

impl DesktopEditorCommandsWrite {
    pub(super) fn saving(
        write: StorageEditorRepositoryWrite,
        context: DesktopEditorCommandsContext,
        draft: BoardEditorDraft,
        application_receipt: DesktopApplicationReceipt,
    ) -> Self {
        Self {
            write,
            context: Some(context),
            draft: Some(draft),
            receipt: None,
            failure: None,
            continuation: DesktopEditorCommandsWriteEnumContinuation::Application(
                application_receipt.clone(),
            ),
            application_receipt,
        }
    }

    pub(super) fn deleting(
        write: StorageEditorRepositoryWrite,
        context: Option<DesktopEditorCommandsContext>,
        application_receipt: DesktopApplicationReceipt,
    ) -> Self {
        Self {
            write,
            context,
            draft: None,
            receipt: None,
            failure: None,
            continuation: DesktopEditorCommandsWriteEnumContinuation::Application(
                application_receipt.clone(),
            ),
            application_receipt,
        }
    }

    pub(super) const fn draft(&self) -> Option<&BoardEditorDraft> {
        self.draft.as_ref()
    }
    pub(super) fn failure(&self) -> Option<&StorageEditorRepositoryError> {
        self.failure.as_deref()
    }
    pub(super) const fn application_receipt(&self) -> &DesktopApplicationReceipt {
        &self.application_receipt
    }

    pub(super) fn transition_receipt(&self) -> Option<&DesktopApplicationReceipt> {
        if self.application_receipt.is_cancellation_requested() {
            return None;
        }
        match &self.continuation {
            DesktopEditorCommandsWriteEnumContinuation::Application(receipt)
                if !receipt.is_cancellation_requested() =>
            {
                Some(receipt)
            }
            DesktopEditorCommandsWriteEnumContinuation::Application(_)
            | DesktopEditorCommandsWriteEnumContinuation::Shutdown => None,
        }
    }
    pub(super) const fn is_delete(&self) -> bool {
        self.draft.is_none()
    }

    pub(super) fn retry(
        &mut self,
        repository: &StorageEditorRepository,
        continuation: DesktopEditorCommandsWriteEnumContinuation,
    ) -> Result<(), DesktopEditorCommandsError> {
        if !self.write.is_retryable() {
            return Err(DesktopEditorCommandsError::WritePending);
        }
        repository.retry(&mut self.write)?;
        self.continuation = continuation;
        self.failure = None;
        Ok(())
    }

    pub(super) fn belongs_to(&self, context: DesktopEditorCommandsContext) -> bool {
        self.write.id() == context.draft()
            && self
                .context
                .is_some_and(|submitted| submitted.session() == context.session())
    }

    pub(super) fn status(&self) -> DesktopEditorCommandsEnumWriteStatus {
        if let Some(error) = &self.failure {
            return DesktopEditorCommandsEnumWriteStatus::Unresolved {
                message: error.to_string(),
            };
        }
        if self.receipt.is_some() {
            return DesktopEditorCommandsEnumWriteStatus::Unresolved {
                message: DesktopEditorCommandsError::MismatchedReceipt.to_string(),
            };
        }
        match self.context.filter(|_| self.draft.is_some()) {
            Some(context) => DesktopEditorCommandsEnumWriteStatus::Saving { context },
            None => DesktopEditorCommandsEnumWriteStatus::Deleting,
        }
    }

    pub(super) async fn wait(
        &mut self,
    ) -> Result<StorageEditorRepositoryReceipt, DesktopEditorCommandsError> {
        if let Some(receipt) = self.receipt {
            return Ok(receipt);
        }
        if let Some(failure) = &self.failure {
            return Err(DesktopEditorCommandsError::WriteFailed(Arc::clone(failure)));
        }
        match self.write.wait().await {
            Ok(receipt) => {
                self.receipt = Some(receipt);
                Ok(receipt)
            }
            Err(error) => {
                let failure = Arc::new(error);
                self.failure = Some(Arc::clone(&failure));
                Err(DesktopEditorCommandsError::WriteFailed(failure))
            }
        }
    }

    pub(super) fn check_receipt(
        &self,
        receipt: StorageEditorRepositoryReceipt,
    ) -> Result<(), DesktopEditorCommandsError> {
        let matches = receipt.id() == self.write.id()
            && self
                .write
                .expected_storage_revision()
                .value()
                .checked_add(1)
                == Some(receipt.storage_revision().value())
            && match (self.draft.as_ref(), receipt.mutation()) {
                (
                    Some(draft),
                    StorageEditorRepositoryEnumMutation::Saved {
                        draft_revision,
                        status,
                    },
                ) => {
                    self.write.draft_revision() == Some(draft_revision)
                        && draft_revision == draft.revision()
                        && status == draft.status()
                }
                (None, StorageEditorRepositoryEnumMutation::Deleted) => {
                    self.write.draft_revision().is_none()
                }
                (Some(_), StorageEditorRepositoryEnumMutation::Deleted)
                | (None, StorageEditorRepositoryEnumMutation::Saved { .. }) => false,
            };
        if matches {
            Ok(())
        } else {
            Err(DesktopEditorCommandsError::MismatchedReceipt)
        }
    }
}
