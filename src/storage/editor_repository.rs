mod transaction;

use std::fmt::{Display, Formatter};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, oneshot};
use uuid::Uuid;

use crate::board::editor::{
    BoardEditorDraft, BoardEditorDraftId, BoardEditorDraftRevision, BoardEditorSession,
    BoardEditorValidationEnumStatus,
};
use crate::storage::document_store::{StorageDocumentClient, StorageDocumentError};

pub(super) use transaction::StorageEditorRepositoryTransaction;

#[derive(Debug, Error)]
pub enum StorageEditorRepositoryError {
    #[error(transparent)]
    Document(#[from] StorageDocumentError),
    #[error("Editor database operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Editor rollback failed after {primary}: {rollback}")]
    Rollback {
        primary: Box<Self>,
        rollback: rusqlite::Error,
    },
    #[error("The editor write does not have a confirmed rollback")]
    RetryUnconfirmed,
    #[error("The editor write belongs to another storage actor")]
    ForeignWrite,
    #[error("Invalid saved editor draft: {0}")]
    Json(#[from] serde_json::Error),
    #[error("The editor storage revision changed: expected {expected}, found {actual}")]
    Conflict {
        expected: StorageEditorRepositoryRevision,
        actual: StorageEditorRepositoryRevision,
    },
    #[error("Editor draft {id} has been deleted")]
    Deleted { id: BoardEditorDraftId },
    #[error("Editor draft {id} has not been saved")]
    NotFound { id: BoardEditorDraftId },
    #[error("The saved editor draft row is malformed or inconsistent")]
    Corrupt,
    #[error("Editor draft format {found} is newer than supported format {supported}")]
    FutureVersion { found: u32, supported: u32 },
    #[error("The editor storage revision is not a canonical unsigned decimal string")]
    NoncanonicalRevision,
    #[error("The editor storage revision is exhausted")]
    RevisionOverflow,
    #[error("The editor draft exceeds its bounded transport size")]
    DraftTooLarge,
    #[error("The editor write completion has already been consumed")]
    CompletionConsumed,
    #[error("The storage actor returned an unexpected editor reply")]
    UnexpectedReply,
    #[error("The editor database reply channel closed: {0}")]
    Channel(#[from] oneshot::error::RecvError),
    #[error("The editor database value exceeds its supported range: {0}")]
    Range(#[from] std::num::TryFromIntError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct StorageEditorRepositoryRevision(u64);

impl StorageEditorRepositoryRevision {
    pub const ABSENT: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    fn next(self) -> Result<Self, StorageEditorRepositoryError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(StorageEditorRepositoryError::RevisionOverflow)
    }
}

impl TryFrom<String> for StorageEditorRepositoryRevision {
    type Error = StorageEditorRepositoryError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let parsed: u64 = value
            .parse()
            .map_err(|_error| StorageEditorRepositoryError::NoncanonicalRevision)?;
        if parsed.to_string() != value {
            return Err(StorageEditorRepositoryError::NoncanonicalRevision);
        }
        Ok(Self(parsed))
    }
}

impl From<StorageEditorRepositoryRevision> for String {
    fn from(value: StorageEditorRepositoryRevision) -> Self {
        value.0.to_string()
    }
}

impl Display for StorageEditorRepositoryRevision {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageEditorRepositoryEnumMutation {
    Saved {
        draft_revision: BoardEditorDraftRevision,
        status: BoardEditorValidationEnumStatus,
    },
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageEditorRepositoryReceipt {
    id: BoardEditorDraftId,
    storage_revision: StorageEditorRepositoryRevision,
    mutation: StorageEditorRepositoryEnumMutation,
}

impl StorageEditorRepositoryReceipt {
    pub const fn id(self) -> BoardEditorDraftId {
        self.id
    }

    pub const fn storage_revision(self) -> StorageEditorRepositoryRevision {
        self.storage_revision
    }

    pub const fn mutation(self) -> StorageEditorRepositoryEnumMutation {
        self.mutation
    }
}

pub struct StorageEditorRepositoryWrite {
    id: BoardEditorDraftId,
    expected_storage_revision: StorageEditorRepositoryRevision,
    draft_revision: Option<BoardEditorDraftRevision>,
    intent: StorageEditorRepositoryEnumWriteIntent,
    origin: Uuid,
    state: StorageEditorRepositoryEnumWriteState,
    _memory_reservation: OwnedSemaphorePermit,
}

enum StorageEditorRepositoryEnumWriteIntent {
    Save(Arc<BoardEditorDraft>),
    Delete(BoardEditorDraftId),
}

impl StorageEditorRepositoryEnumWriteIntent {
    fn request(
        &self,
        expected_storage_revision: StorageEditorRepositoryRevision,
    ) -> StorageEditorRepositoryEnumRequest {
        match self {
            Self::Save(draft) => StorageEditorRepositoryEnumRequest::Save {
                draft: Arc::clone(draft),
                expected_storage_revision,
            },
            Self::Delete(id) => StorageEditorRepositoryEnumRequest::Delete {
                id: *id,
                expected_storage_revision,
            },
        }
    }
}

enum StorageEditorRepositoryEnumWriteState {
    Pending(StorageEditorRepositoryPending),
    RolledBack,
    Uncertain,
    Completed,
}

impl StorageEditorRepositoryWrite {
    pub const fn id(&self) -> BoardEditorDraftId {
        self.id
    }

    pub const fn expected_storage_revision(&self) -> StorageEditorRepositoryRevision {
        self.expected_storage_revision
    }

    pub const fn draft_revision(&self) -> Option<BoardEditorDraftRevision> {
        self.draft_revision
    }

    pub const fn is_retryable(&self) -> bool {
        matches!(
            self.state,
            StorageEditorRepositoryEnumWriteState::RolledBack
        )
    }

    pub async fn wait(
        &mut self,
    ) -> Result<StorageEditorRepositoryReceipt, StorageEditorRepositoryError> {
        let StorageEditorRepositoryEnumWriteState::Pending(pending) = &mut self.state else {
            return Err(StorageEditorRepositoryError::CompletionConsumed);
        };
        let outcome = pending.wait().await;
        self.state = StorageEditorRepositoryEnumWriteState::Uncertain;
        match outcome? {
            StorageEditorRepositoryEnumResponse::Write(receipt) => {
                self.state = StorageEditorRepositoryEnumWriteState::Completed;
                Ok(receipt)
            }
            StorageEditorRepositoryEnumResponse::RolledBack(error) => {
                self.state = StorageEditorRepositoryEnumWriteState::RolledBack;
                Err(error)
            }
            StorageEditorRepositoryEnumResponse::Read(_)
            | StorageEditorRepositoryEnumResponse::List(_) => {
                Err(StorageEditorRepositoryError::UnexpectedReply)
            }
        }
    }
}

pub struct StorageEditorRepositoryRecovered {
    draft: BoardEditorDraft,
    storage_revision: StorageEditorRepositoryRevision,
    _memory_reservation: OwnedSemaphorePermit,
}

impl StorageEditorRepositoryRecovered {
    pub const fn draft(&self) -> &BoardEditorDraft {
        &self.draft
    }

    pub const fn storage_revision(&self) -> StorageEditorRepositoryRevision {
        self.storage_revision
    }

    pub fn into_session(self) -> BoardEditorSession {
        BoardEditorSession::from_draft(self.draft)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageEditorRepositoryCursor(BoardEditorDraftId);

impl StorageEditorRepositoryCursor {
    pub const fn new(id: BoardEditorDraftId) -> Self {
        Self(id)
    }

    pub const fn id(self) -> BoardEditorDraftId {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageEditorRepositoryEntry {
    id: BoardEditorDraftId,
    storage_revision: StorageEditorRepositoryRevision,
    draft_revision: BoardEditorDraftRevision,
    status: BoardEditorValidationEnumStatus,
}

impl StorageEditorRepositoryEntry {
    pub const fn id(&self) -> BoardEditorDraftId {
        self.id
    }

    pub const fn storage_revision(&self) -> StorageEditorRepositoryRevision {
        self.storage_revision
    }

    pub const fn draft_revision(&self) -> BoardEditorDraftRevision {
        self.draft_revision
    }

    pub const fn status(&self) -> BoardEditorValidationEnumStatus {
        self.status
    }
}

pub struct StorageEditorRepositoryPage {
    entries: Vec<StorageEditorRepositoryEntry>,
    _memory_reservation: OwnedSemaphorePermit,
}

impl StorageEditorRepositoryPage {
    pub fn entries(&self) -> &[StorageEditorRepositoryEntry] {
        &self.entries
    }

    pub fn next_cursor(&self) -> Option<StorageEditorRepositoryCursor> {
        if self.entries.len() == StorageEditorRepository::PAGE_SIZE {
            self.entries
                .last()
                .map(|entry| StorageEditorRepositoryCursor::new(entry.id))
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub struct StorageEditorRepository {
    client: StorageDocumentClient,
}

impl StorageEditorRepository {
    pub const PAGE_SIZE: usize = 100;
    pub const FORMAT_VERSION: u32 = 1;
    pub const MAX_DRAFT_BYTES: usize = 16 * 1024;
    const DRAFT_MEMORY_BYTES: usize = 64 * 1024;
    const PAGE_MEMORY_BYTES: usize = Self::DRAFT_MEMORY_BYTES
        + Self::PAGE_SIZE * std::mem::size_of::<StorageEditorRepositoryEntry>();

    pub const fn new(client: StorageDocumentClient) -> Self {
        Self { client }
    }

    pub fn save(
        &self,
        draft: &BoardEditorDraft,
        expected_storage_revision: StorageEditorRepositoryRevision,
    ) -> Result<StorageEditorRepositoryWrite, StorageEditorRepositoryError> {
        let memory_reservation = self
            .client
            .reserve_editor_memory(Self::DRAFT_MEMORY_BYTES)?;
        let intent = StorageEditorRepositoryEnumWriteIntent::Save(Arc::new(draft.clone()));
        let pending = self.client.begin_editor(Self::DRAFT_MEMORY_BYTES, || {
            intent.request(expected_storage_revision)
        })?;
        Ok(StorageEditorRepositoryWrite {
            id: draft.id(),
            expected_storage_revision,
            draft_revision: Some(draft.revision()),
            intent,
            origin: self.client.library_preparation_origin(),
            state: StorageEditorRepositoryEnumWriteState::Pending(pending),
            _memory_reservation: memory_reservation,
        })
    }

    pub fn delete(
        &self,
        id: BoardEditorDraftId,
        expected_storage_revision: StorageEditorRepositoryRevision,
    ) -> Result<StorageEditorRepositoryWrite, StorageEditorRepositoryError> {
        let memory_reservation = self
            .client
            .reserve_editor_memory(Self::DRAFT_MEMORY_BYTES)?;
        let intent = StorageEditorRepositoryEnumWriteIntent::Delete(id);
        let pending = self.client.begin_editor(Self::DRAFT_MEMORY_BYTES, || {
            intent.request(expected_storage_revision)
        })?;
        Ok(StorageEditorRepositoryWrite {
            id,
            expected_storage_revision,
            draft_revision: None,
            intent,
            origin: self.client.library_preparation_origin(),
            state: StorageEditorRepositoryEnumWriteState::Pending(pending),
            _memory_reservation: memory_reservation,
        })
    }

    pub fn retry(
        &self,
        write: &mut StorageEditorRepositoryWrite,
    ) -> Result<(), StorageEditorRepositoryError> {
        if write.origin != self.client.library_preparation_origin() {
            return Err(StorageEditorRepositoryError::ForeignWrite);
        }
        if !write.is_retryable() {
            return Err(StorageEditorRepositoryError::RetryUnconfirmed);
        }
        let pending = self.client.begin_editor(Self::DRAFT_MEMORY_BYTES, || {
            write.intent.request(write.expected_storage_revision)
        })?;
        write.state = StorageEditorRepositoryEnumWriteState::Pending(pending);
        Ok(())
    }

    pub async fn read(
        &self,
        id: BoardEditorDraftId,
    ) -> Result<Option<StorageEditorRepositoryRecovered>, StorageEditorRepositoryError> {
        let mut pending = self.client.begin_editor(Self::DRAFT_MEMORY_BYTES, || {
            StorageEditorRepositoryEnumRequest::Read { id }
        })?;
        match pending.wait().await? {
            StorageEditorRepositoryEnumResponse::Read(recovered) => Ok(recovered),
            StorageEditorRepositoryEnumResponse::RolledBack(error) => Err(error),
            StorageEditorRepositoryEnumResponse::Write(_)
            | StorageEditorRepositoryEnumResponse::List(_) => {
                Err(StorageEditorRepositoryError::UnexpectedReply)
            }
        }
    }

    pub async fn list(
        &self,
        after: Option<StorageEditorRepositoryCursor>,
    ) -> Result<StorageEditorRepositoryPage, StorageEditorRepositoryError> {
        let mut pending = self.client.begin_editor(Self::PAGE_MEMORY_BYTES, || {
            StorageEditorRepositoryEnumRequest::List { after }
        })?;
        match pending.wait().await? {
            StorageEditorRepositoryEnumResponse::List(page) => Ok(page),
            StorageEditorRepositoryEnumResponse::RolledBack(error) => Err(error),
            StorageEditorRepositoryEnumResponse::Write(_)
            | StorageEditorRepositoryEnumResponse::Read(_) => {
                Err(StorageEditorRepositoryError::UnexpectedReply)
            }
        }
    }
}

pub(super) enum StorageEditorRepositoryEnumRequest {
    Save {
        draft: Arc<BoardEditorDraft>,
        expected_storage_revision: StorageEditorRepositoryRevision,
    },
    Delete {
        id: BoardEditorDraftId,
        expected_storage_revision: StorageEditorRepositoryRevision,
    },
    Read {
        id: BoardEditorDraftId,
    },
    List {
        after: Option<StorageEditorRepositoryCursor>,
    },
}

pub(super) enum StorageEditorRepositoryEnumResponse {
    Write(StorageEditorRepositoryReceipt),
    RolledBack(StorageEditorRepositoryError),
    Read(Option<StorageEditorRepositoryRecovered>),
    List(StorageEditorRepositoryPage),
}

pub(super) struct StorageEditorRepositoryPending {
    receiver: Option<
        oneshot::Receiver<
            Result<StorageEditorRepositoryEnumResponse, StorageEditorRepositoryError>,
        >,
    >,
}

impl StorageEditorRepositoryPending {
    pub(super) const fn new(
        receiver: oneshot::Receiver<
            Result<StorageEditorRepositoryEnumResponse, StorageEditorRepositoryError>,
        >,
    ) -> Self {
        Self {
            receiver: Some(receiver),
        }
    }

    async fn wait(
        &mut self,
    ) -> Result<StorageEditorRepositoryEnumResponse, StorageEditorRepositoryError> {
        let receiver = self
            .receiver
            .as_mut()
            .ok_or(StorageEditorRepositoryError::CompletionConsumed)?;
        let completion = receiver.await;
        self.receiver = None;
        completion?
    }
}
