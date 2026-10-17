mod context;
mod operation;
mod snapshot;
mod write;

use crate::desktop::application::DesktopApplicationReceipt;
use std::sync::Arc;

use thiserror::Error;

use crate::board::editor::{
    BoardEditorDraft, BoardEditorDraftId, BoardEditorDraftRevision, BoardEditorSession,
    BoardEditorSessionError, BoardEditorSessionRequest, BoardEditorValidationEnumStatus,
    BoardEditorValidationError,
};
use crate::board::position::BoardPositionError;
use crate::desktop::editor_commands::write::{
    DesktopEditorCommandsWrite, DesktopEditorCommandsWriteEnumContinuation,
};
use crate::storage::editor_repository::{
    StorageEditorRepository, StorageEditorRepositoryError, StorageEditorRepositoryReceipt,
    StorageEditorRepositoryRevision,
};

pub use context::{DesktopEditorCommandsContext, DesktopEditorCommandsOpening};
pub use operation::{
    DesktopEditorCommandsEnumChoice, DesktopEditorCommandsEnumOperation,
    DesktopEditorCommandsEnumPrompt,
};
pub use snapshot::{
    DesktopEditorCommandsEnumResult, DesktopEditorCommandsEnumWriteStatus,
    DesktopEditorCommandsPage, DesktopEditorCommandsSnapshot,
};

#[derive(Debug, Error)]
pub enum DesktopEditorCommandsError {
    #[error("The FEN import retains {bytes} bytes, exceeding its {maximum}-byte limit")]
    FenPayloadTooLarge { bytes: usize, maximum: usize },
    #[error("The editor request was canceled before accepting its next effect")]
    Cancelled,
    #[error("Open an editor draft first")]
    NoSession,
    #[error("The editor session or draft revision changed; reconcile before retrying")]
    StaleContext,
    #[error(
        "A new editor opening requires a separate session identity and new drafts require a separate draft identity"
    )]
    ReusedOpening,
    #[error("Resolve the existing Save, Discard or Stay prompt first")]
    PromptPending,
    #[error("There is no pending editor transition to resolve")]
    NoPrompt,
    #[error("The original editor write is still pending or unresolved")]
    WritePending,
    #[error("The original editor write failed and is retained for reconciliation: {0}")]
    WriteFailed(#[source] Arc<StorageEditorRepositoryError>),
    #[error("There is no editor write to observe")]
    NoWrite,
    #[error(
        "The editor write acknowledgement does not match the submitted draft and storage revision"
    )]
    MismatchedReceipt,
    #[error(
        "The saved draft must be clean before deletion; save it or leave it with an explicit choice"
    )]
    DirtyDelete,
    #[error("Editor draft {id} was not found")]
    NotFound { id: BoardEditorDraftId },
    #[error(transparent)]
    Session(#[from] BoardEditorSessionError),
    #[error(transparent)]
    Validation(#[from] BoardEditorValidationError),
    #[error(transparent)]
    Fen(#[from] BoardPositionError),
    #[error(transparent)]
    Storage(#[from] StorageEditorRepositoryError),
}

struct DesktopEditorCommandsSession {
    opening: DesktopEditorCommandsOpening,
    session: BoardEditorSession,
    storage_revision: StorageEditorRepositoryRevision,
    clean_revision: Option<BoardEditorDraftRevision>,
}

impl DesktopEditorCommandsSession {
    fn context(&self) -> DesktopEditorCommandsContext {
        DesktopEditorCommandsContext::new(self.opening, self.session.draft().revision())
    }

    fn is_dirty(&self) -> bool {
        self.clean_revision != Some(self.session.draft().revision())
    }
}

enum DesktopEditorCommandsEnumDestination {
    Draft {
        opening: DesktopEditorCommandsOpening,
        draft: BoardEditorDraft,
        prompt: DesktopEditorCommandsEnumPrompt,
    },
    Read(DesktopEditorCommandsOpening),
    Leave,
}

impl DesktopEditorCommandsEnumDestination {
    const fn prompt(&self) -> DesktopEditorCommandsEnumPrompt {
        match self {
            Self::Draft { prompt, .. } => *prompt,
            Self::Read(_) => DesktopEditorCommandsEnumPrompt::Read,
            Self::Leave => DesktopEditorCommandsEnumPrompt::Leave,
        }
    }
}

pub struct DesktopEditorCommands {
    repository: StorageEditorRepository,
    active: Option<DesktopEditorCommandsSession>,
    pending: Option<DesktopEditorCommandsWrite>,
    destination: Option<DesktopEditorCommandsEnumDestination>,
    last_receipt: Option<StorageEditorRepositoryReceipt>,
}

impl DesktopEditorCommands {
    pub const fn new(repository: StorageEditorRepository) -> Self {
        Self {
            repository,
            active: None,
            pending: None,
            destination: None,
            last_receipt: None,
        }
    }

    pub fn snapshot(&self) -> Option<DesktopEditorCommandsSnapshot> {
        let active = self.active.as_ref()?;
        let draft = active.session.draft();
        let (status, findings) = match draft.validate() {
            Ok(_) => (BoardEditorValidationEnumStatus::Valid, Vec::new()),
            Err(error) => (
                BoardEditorValidationEnumStatus::Invalid,
                error.findings().to_vec(),
            ),
        };
        Some(DesktopEditorCommandsSnapshot {
            context: active.context(),
            draft: draft.clone(),
            storage_revision: active.storage_revision,
            is_dirty: active.is_dirty(),
            selection: active.session.selection(),
            has_undo: active.session.has_undo(),
            has_redo: active.session.has_redo(),
            findings,
            status,
            prompt: self
                .destination
                .as_ref()
                .map(DesktopEditorCommandsEnumDestination::prompt),
            write: self.pending.as_ref().map_or(
                DesktopEditorCommandsEnumWriteStatus::Idle,
                DesktopEditorCommandsWrite::status,
            ),
        })
    }

    pub fn pending_draft(&self) -> Option<&BoardEditorDraft> {
        self.pending.as_ref()?.draft()
    }
    pub const fn has_pending_write(&self) -> bool {
        self.pending.is_some()
    }
    pub const fn last_receipt(&self) -> Option<StorageEditorRepositoryReceipt> {
        self.last_receipt
    }
    pub fn pending_error(&self) -> Option<&StorageEditorRepositoryError> {
        self.pending.as_ref()?.failure()
    }
    pub fn pending_application_receipt(&self) -> Option<&DesktopApplicationReceipt> {
        self.pending
            .as_ref()
            .map(DesktopEditorCommandsWrite::application_receipt)
    }

    pub async fn handle(
        &mut self,
        operation: DesktopEditorCommandsEnumOperation,
        receipt: &DesktopApplicationReceipt,
    ) -> Result<DesktopEditorCommandsEnumResult, DesktopEditorCommandsError> {
        Self::check_cancellation(receipt)?;
        operation.validate_payload()?;
        match operation {
            DesktopEditorCommandsEnumOperation::Create {
                opening,
                selected,
                expected,
            } => {
                self.check_opening(opening, true)?;
                self.transition(
                    expected,
                    DesktopEditorCommandsEnumDestination::Draft {
                        opening,
                        draft: BoardEditorDraft::new(opening.draft(), selected.as_ref()),
                        prompt: DesktopEditorCommandsEnumPrompt::Create,
                    },
                    receipt,
                )
                .await
            }
            DesktopEditorCommandsEnumOperation::ImportFen {
                opening,
                fen,
                variant,
                expected,
            } => {
                self.check_opening(opening, true)?;
                let draft = BoardEditorDraft::from_fen(opening.draft(), &fen, variant)?;
                self.transition(
                    expected,
                    DesktopEditorCommandsEnumDestination::Draft {
                        opening,
                        draft,
                        prompt: DesktopEditorCommandsEnumPrompt::ImportFen,
                    },
                    receipt,
                )
                .await
            }
            DesktopEditorCommandsEnumOperation::Read { opening, expected } => {
                self.check_opening(opening, false)?;
                self.transition(
                    expected,
                    DesktopEditorCommandsEnumDestination::Read(opening),
                    receipt,
                )
                .await
            }
            DesktopEditorCommandsEnumOperation::Leave { expected } => {
                self.transition(
                    Some(expected),
                    DesktopEditorCommandsEnumDestination::Leave,
                    receipt,
                )
                .await
            }
            DesktopEditorCommandsEnumOperation::Resolve { expected, choice } => {
                self.resolve(expected, choice, receipt).await
            }
            DesktopEditorCommandsEnumOperation::Apply {
                expected,
                operation,
            } => {
                self.check_context(Some(expected))?;
                if self
                    .pending
                    .as_ref()
                    .is_some_and(DesktopEditorCommandsWrite::is_delete)
                {
                    return Err(DesktopEditorCommandsError::WritePending);
                }
                Self::check_cancellation(receipt)?;
                self.active
                    .as_mut()
                    .ok_or(DesktopEditorCommandsError::NoSession)?
                    .session
                    .apply(BoardEditorSessionRequest {
                        draft: expected.draft(),
                        expected_revision: expected.revision(),
                        operation,
                    })?;
                Ok(self.state())
            }
            DesktopEditorCommandsEnumOperation::Validate { expected } => {
                self.current(expected)?.session.draft().validate()?;
                Ok(self.state())
            }
            DesktopEditorCommandsEnumOperation::Save { expected } => {
                self.check_context(Some(expected))?;
                self.begin_save(receipt)?;
                Ok(DesktopEditorCommandsEnumResult::WritePending(
                    self.snapshot(),
                ))
            }
            DesktopEditorCommandsEnumOperation::ExportFen { expected } => {
                Ok(DesktopEditorCommandsEnumResult::Fen {
                    context: expected,
                    fen: self.current(expected)?.session.draft().validated_fen()?,
                })
            }
            DesktopEditorCommandsEnumOperation::Play { expected } => {
                Ok(DesktopEditorCommandsEnumResult::Play {
                    context: expected,
                    start: self.current(expected)?.session.draft().play_prefill()?,
                })
            }
            DesktopEditorCommandsEnumOperation::List { after } => {
                let page = self.repository.list(after).await?;
                Self::check_cancellation(receipt)?;
                Ok(DesktopEditorCommandsEnumResult::List(
                    DesktopEditorCommandsPage::new(page),
                ))
            }
            DesktopEditorCommandsEnumOperation::Delete {
                id,
                storage_revision,
                expected,
            } => {
                self.check_context(expected)?;
                self.require_no_write()?;
                if self.destination.is_some() {
                    return Err(DesktopEditorCommandsError::PromptPending);
                }
                if let Some(active) = self
                    .active
                    .as_ref()
                    .filter(|active| active.opening.draft() == id)
                {
                    if active.is_dirty() {
                        return Err(DesktopEditorCommandsError::DirtyDelete);
                    }
                    if active.storage_revision != storage_revision {
                        return Err(DesktopEditorCommandsError::StaleContext);
                    }
                }
                Self::check_cancellation(receipt)?;
                let write = self.repository.delete(id, storage_revision)?;
                self.pending = Some(DesktopEditorCommandsWrite::deleting(
                    write,
                    self.active
                        .as_ref()
                        .map(DesktopEditorCommandsSession::context),
                    receipt.clone(),
                ));
                self.last_receipt = None;
                Ok(DesktopEditorCommandsEnumResult::WritePending(
                    self.snapshot(),
                ))
            }
            DesktopEditorCommandsEnumOperation::Snapshot => Ok(self.state()),
        }
    }

    pub async fn finish_write(
        &mut self,
    ) -> Result<DesktopEditorCommandsEnumResult, DesktopEditorCommandsError> {
        let pending = self
            .pending
            .as_mut()
            .ok_or(DesktopEditorCommandsError::NoWrite)?;
        let receipt = pending.wait().await?;
        pending.check_receipt(receipt)?;
        let transition_receipt = pending.transition_receipt().cloned();
        let pending = self
            .pending
            .take()
            .ok_or(DesktopEditorCommandsError::NoWrite)?;
        if let Some(active) = self
            .active
            .as_mut()
            .filter(|active| pending.belongs_to(active.context()))
        {
            if let Some(draft) = pending.draft() {
                active.storage_revision = receipt.storage_revision();
                active.clean_revision = Some(draft.revision());
            } else {
                self.active = None;
            }
        }
        self.last_receipt = Some(receipt);
        if let Some(application_receipt) = transition_receipt
            && self.destination.is_some()
            && self.active.as_ref().is_none_or(|active| !active.is_dirty())
        {
            self.commit_transition(&application_receipt).await?;
        }
        Ok(DesktopEditorCommandsEnumResult::Written {
            receipt,
            snapshot: self.snapshot(),
        })
    }

    pub(crate) fn retry_retained_write(&mut self) -> Result<(), DesktopEditorCommandsError> {
        self.pending
            .as_mut()
            .ok_or(DesktopEditorCommandsError::NoWrite)?
            .retry(
                &self.repository,
                DesktopEditorCommandsWriteEnumContinuation::Shutdown,
            )
    }

    fn state(&self) -> DesktopEditorCommandsEnumResult {
        DesktopEditorCommandsEnumResult::Snapshot(self.snapshot())
    }

    fn check_cancellation(
        receipt: &DesktopApplicationReceipt,
    ) -> Result<(), DesktopEditorCommandsError> {
        if receipt.is_cancellation_requested() {
            Err(DesktopEditorCommandsError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn require_no_write(&self) -> Result<(), DesktopEditorCommandsError> {
        if self.pending.is_some() {
            Err(DesktopEditorCommandsError::WritePending)
        } else {
            Ok(())
        }
    }

    fn check_context(
        &self,
        expected: Option<DesktopEditorCommandsContext>,
    ) -> Result<(), DesktopEditorCommandsError> {
        if self
            .active
            .as_ref()
            .map(DesktopEditorCommandsSession::context)
            == expected
        {
            Ok(())
        } else {
            Err(DesktopEditorCommandsError::StaleContext)
        }
    }

    fn current(
        &self,
        expected: DesktopEditorCommandsContext,
    ) -> Result<&DesktopEditorCommandsSession, DesktopEditorCommandsError> {
        self.check_context(Some(expected))?;
        self.active
            .as_ref()
            .ok_or(DesktopEditorCommandsError::NoSession)
    }

    fn check_opening(
        &self,
        opening: DesktopEditorCommandsOpening,
        is_new: bool,
    ) -> Result<(), DesktopEditorCommandsError> {
        if self.active.as_ref().is_some_and(|active| {
            active.opening.session() == opening.session()
                || is_new && active.opening.draft() == opening.draft()
        }) {
            Err(DesktopEditorCommandsError::ReusedOpening)
        } else {
            Ok(())
        }
    }

    fn begin_save(
        &mut self,
        receipt: &DesktopApplicationReceipt,
    ) -> Result<(), DesktopEditorCommandsError> {
        Self::check_cancellation(receipt)?;
        if let Some(pending) = &mut self.pending {
            return pending.retry(
                &self.repository,
                DesktopEditorCommandsWriteEnumContinuation::Application(receipt.clone()),
            );
        }
        let active = self
            .active
            .as_ref()
            .ok_or(DesktopEditorCommandsError::NoSession)?;
        let draft = active.session.draft();
        Self::check_cancellation(receipt)?;
        let write = self.repository.save(draft, active.storage_revision)?;
        self.pending = Some(DesktopEditorCommandsWrite::saving(
            write,
            active.context(),
            draft.clone(),
            receipt.clone(),
        ));
        self.last_receipt = None;
        Ok(())
    }

    async fn transition(
        &mut self,
        expected: Option<DesktopEditorCommandsContext>,
        destination: DesktopEditorCommandsEnumDestination,
        receipt: &DesktopApplicationReceipt,
    ) -> Result<DesktopEditorCommandsEnumResult, DesktopEditorCommandsError> {
        self.check_context(expected)?;
        self.require_no_write()?;
        if self.destination.is_some() {
            return Err(DesktopEditorCommandsError::PromptPending);
        }
        self.destination = Some(destination);
        if self.active.as_ref().is_none_or(|active| !active.is_dirty()) {
            self.commit_transition(receipt).await?;
        }
        Ok(self.state())
    }

    async fn resolve(
        &mut self,
        expected: DesktopEditorCommandsContext,
        choice: DesktopEditorCommandsEnumChoice,
        receipt: &DesktopApplicationReceipt,
    ) -> Result<DesktopEditorCommandsEnumResult, DesktopEditorCommandsError> {
        self.check_context(Some(expected))?;
        if self.destination.is_none() {
            return Err(DesktopEditorCommandsError::NoPrompt);
        }
        match choice {
            DesktopEditorCommandsEnumChoice::Stay => {
                self.destination = None;
            }
            DesktopEditorCommandsEnumChoice::Discard => {
                self.require_no_write()?;
                self.commit_transition(receipt).await?;
            }
            DesktopEditorCommandsEnumChoice::Save => {
                self.require_no_write()?;
                if self
                    .active
                    .as_ref()
                    .is_some_and(DesktopEditorCommandsSession::is_dirty)
                {
                    self.begin_save(receipt)?;
                    return Ok(DesktopEditorCommandsEnumResult::WritePending(
                        self.snapshot(),
                    ));
                }
                self.commit_transition(receipt).await?;
            }
        }
        Ok(self.state())
    }

    async fn commit_transition(
        &mut self,
        receipt: &DesktopApplicationReceipt,
    ) -> Result<(), DesktopEditorCommandsError> {
        let recovered =
            if let Some(DesktopEditorCommandsEnumDestination::Read(opening)) = &self.destination {
                Some(self.repository.read(opening.draft()).await?.ok_or(
                    DesktopEditorCommandsError::NotFound {
                        id: opening.draft(),
                    },
                )?)
            } else {
                None
            };
        if let Err(error) = Self::check_cancellation(receipt) {
            self.destination = None;
            return Err(error);
        }
        let destination = self
            .destination
            .take()
            .ok_or(DesktopEditorCommandsError::NoPrompt)?;
        self.active = match destination {
            DesktopEditorCommandsEnumDestination::Draft { opening, draft, .. } => {
                Some(DesktopEditorCommandsSession {
                    opening,
                    session: BoardEditorSession::from_draft(draft),
                    storage_revision: StorageEditorRepositoryRevision::ABSENT,
                    clean_revision: None,
                })
            }
            DesktopEditorCommandsEnumDestination::Read(opening) => {
                let recovered = recovered.ok_or(DesktopEditorCommandsError::NotFound {
                    id: opening.draft(),
                })?;
                let storage_revision = recovered.storage_revision();
                let revision = recovered.draft().revision();
                Some(DesktopEditorCommandsSession {
                    opening,
                    session: recovered.into_session(),
                    storage_revision,
                    clean_revision: Some(revision),
                })
            }
            DesktopEditorCommandsEnumDestination::Leave => None,
        };
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/desktop/editor_commands.rs"]
pub(crate) mod tests;
