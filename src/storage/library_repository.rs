use std::num::TryFromIntError;
use std::str::FromStr;
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};
use uuid::Uuid;

use crate::capacity::executor::{
    CapacityExecutor, CapacityExecutorCancellation, CapacityExecutorError,
};
use crate::game::enums::{GameId, GameIdentityError, GameRevision, GameRevisionError};
use crate::game::library_organization::{
    GameLibraryOrganizationError, GameLibraryOrganizationReceipt, GameLibraryOrganizationRequest,
    GameLibraryOrganizationRevision, GameLibraryOrganizationState,
    GameLibraryOrganizationTimestamp,
};
use crate::game::session::GameMetadataError;
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::document_store::{StorageDocumentClient, StorageDocumentError};
use crate::storage::game_document::{StorageGameDocument, StorageGameDocumentError};
use crate::storage::library_prepared::{
    StorageLibraryAdoptionPreparation, StorageLibraryEnumContentChange,
    StorageLibraryPreparedAdoption, StorageLibraryPreparedMemory, StorageLibraryPreparedWrite,
    StorageLibraryPreparedWriteOptions,
};
use crate::storage::library_query::{StorageLibraryQuery, StorageLibraryQueryPage};
use crate::storage::library_record::{StorageLibraryRecord, StorageLibraryRecordError};
use crate::storage::library_transaction::{
    StorageLibraryEnumTransactionRequest, StorageLibraryEnumTransactionResponse,
};
use crate::storage::trash::{
    StorageTrashConfirmedRequest, StorageTrashEnumLocation, StorageTrashPlan,
    StorageTrashPreviewRequest, StorageTrashReceipt, StorageTrashRestoreRequest,
};

#[derive(Debug, Error)]
pub enum StorageLibraryError {
    #[error(transparent)]
    Store(#[from] StorageDocumentError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Codec(#[from] StorageGameDocumentError),
    #[error(transparent)]
    Record(#[from] StorageLibraryRecordError),
    #[error(transparent)]
    Executor(#[from] CapacityExecutorError),
    #[error(transparent)]
    Organization(#[from] GameLibraryOrganizationError),
    #[error(transparent)]
    IdentityValue(#[from] GameIdentityError),
    #[error(transparent)]
    DocumentIdentity(#[from] uuid::Error),
    #[error(transparent)]
    Metadata(#[from] GameMetadataError),
    #[error(transparent)]
    Revision(#[from] GameRevisionError),
    #[error(transparent)]
    Range(#[from] TryFromIntError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Channel(#[from] oneshot::error::RecvError),
    #[error(transparent)]
    Import(#[from] crate::storage::import_session::StorageImportSessionError),
    #[error("Invalid stored import value: {0}")]
    ImportValue(#[from] rusqlite::types::FromSqlError),
    #[error("The import catalog changed")]
    ImportCatalog,
    #[error("The import destination changed")]
    ImportDestination,
    #[error("An import identifier already exists")]
    ImportExisting,
    #[error("The duplicate candidate changed")]
    ImportCandidate,
    #[error("The private import projection proof is invalid")]
    ImportProof,
    #[error("The SQLite layout cannot satisfy the import space contract")]
    ImportLayout,
    #[error("The SQLite space allowance increased after confirmation")]
    ImportSpaceIncreased,
    #[error("SQLite page ceiling differs: requested {requested}, actual {actual}")]
    ImportPageCeiling { requested: u64, actual: u64 },
    #[error("The import page ceiling cannot be restored before transaction cleanup is observed")]
    ImportScopeTransaction,
    #[error("Import failed ({primary}); page ceiling restoration failed ({restore})")]
    ImportScopeCleanup {
        #[source]
        primary: Box<Self>,
        restore: Box<Self>,
    },
    #[error("Import committed; page ceiling restoration failed ({restore})")]
    ImportScopeCommittedCleanup {
        #[source]
        restore: Box<Self>,
    },
    #[error("Library operation failed ({primary}); rollback also failed ({rollback})")]
    Rollback {
        primary: Box<Self>,
        rollback: rusqlite::Error,
    },
    #[error("The library response does not match its request")]
    Protocol,
    #[error("The library operation result was already observed")]
    AlreadyObserved,
    #[error("The stored game identity differs from its document key")]
    Identity,
    #[error("Library preparation was cancelled")]
    Cancelled,
    #[error("Prepared library data belongs to a different storage byte budget")]
    ForeignPreparation,
    #[error("A library text field exceeds one MiB")]
    TextTooLarge,
    #[error("Prepared library storage needs {required} reserved bytes, found {reserved}")]
    PreparationMemory { required: usize, reserved: usize },
    #[error("An active text or tag filter cannot be empty or missing")]
    EmptyFilter,
    #[error("A date interval requires complete ordered valid calendar boundaries")]
    DateInterval,
    #[error("A ply interval must be ordered within 0..100000")]
    PlyInterval,
    #[error("The continuation belongs to a different query")]
    CursorQuery,
    #[error("The library changed; restart pagination")]
    StaleCursor,
    #[error("A date query must be prepared on the capacity executor")]
    UnpreparedQuery,
    #[error("A stored library discriminant or projection is invalid")]
    StoredDiscriminant,
    #[error("The library catalog revision is exhausted")]
    CatalogExhausted,
    #[error("The game content revision is exhausted")]
    ContentExhausted,
    #[error("Game content changed: expected {expected:?}, found {actual:?}")]
    ContentConflict {
        expected: GameStorageRevision,
        actual: GameStorageRevision,
    },
    #[error("The game is in Trash: {0:?}")]
    InTrash(GameId),
    #[error("The game is already in Library: {0:?}")]
    InLibrary(GameId),
    #[error("The game does not exist: {0:?}")]
    MissingGame(GameId),
    #[error("The folder does not exist: {0}")]
    MissingFolder(crate::game::library_organization::GameLibraryOrganizationFolderId),
    #[error("The tag does not exist: {0}")]
    MissingTag(crate::game::library_organization::GameLibraryOrganizationTagId),
    #[error("The folder identifier already exists")]
    FolderExists,
    #[error("The tag identifier already exists")]
    TagExists,
    #[error("An existing game cannot change content kind")]
    ContentKind,
    #[error("A new game requires a supplied content-change timestamp")]
    NewContentTimestamp,
    #[error("The document requires adoption before it can be overwritten")]
    AdoptionRequired,
    #[error("The projection does not match the current document revision")]
    ProjectionStale,
    #[error("The approved destructive plan is stale")]
    StalePlan,
    #[error("Destruction requires matching closed-game evidence from lifecycle orchestration")]
    LifecycleRequired,
}

impl StorageLibraryError {
    pub(crate) fn requires_connection_retirement(&self) -> bool {
        matches!(
            self,
            Self::ImportScopeCleanup { .. } | Self::ImportScopeCommittedCleanup { .. }
        )
    }

    pub(crate) fn check_cancellation(
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<(), Self> {
        if cancellation.is_cancelled() {
            Err(Self::Cancelled)
        } else {
            Ok(())
        }
    }
    pub(crate) fn content_revision(value: &str) -> Result<GameStorageRevision, Self> {
        Ok(GameStorageRevision::new(
            value.parse::<GameRevision>()?.value(),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageLibraryCatalogRevision(GameLibraryOrganizationRevision);

impl StorageLibraryCatalogRevision {
    pub const fn value(self) -> u64 {
        self.0.value()
    }
    pub(crate) fn next(self) -> Result<Self, StorageLibraryError> {
        self.0
            .next()
            .map(Self)
            .map_err(|_error| StorageLibraryError::CatalogExhausted)
    }
}

impl FromStr for StorageLibraryCatalogRevision {
    type Err = StorageLibraryError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}

pub(crate) struct StorageLibraryMemory {
    permits: Vec<OwnedSemaphorePermit>,
}

impl StorageLibraryMemory {
    pub(crate) fn new(memory: &Arc<Semaphore>, bytes: usize) -> Result<Self, StorageLibraryError> {
        Ok(Self {
            permits: vec![StorageDocumentClient::reserve_memory(memory, bytes)?],
        })
    }
    pub(crate) fn reserve(
        &mut self,
        memory: &Arc<Semaphore>,
        bytes: usize,
    ) -> Result<(), StorageLibraryError> {
        self.permits
            .push(StorageDocumentClient::reserve_memory(memory, bytes)?);
        Ok(())
    }
}

pub(crate) struct StorageLibraryStoredRecord {
    id: GameId,
    body: String,
    revision: GameStorageRevision,
    organization: GameLibraryOrganizationState,
    location: StorageTrashEnumLocation,
    modified: Option<GameLibraryOrganizationTimestamp>,
    _memory: StorageLibraryMemory,
}

pub(crate) struct StorageLibraryStoredRecordOptions {
    pub(crate) id: GameId,
    pub(crate) body: String,
    pub(crate) revision: GameStorageRevision,
    pub(crate) organization: GameLibraryOrganizationState,
    pub(crate) location: StorageTrashEnumLocation,
    pub(crate) modified: Option<GameLibraryOrganizationTimestamp>,
    pub(crate) memory: StorageLibraryMemory,
}

impl StorageLibraryStoredRecord {
    pub(crate) fn new(options: StorageLibraryStoredRecordOptions) -> Self {
        Self {
            id: options.id,
            body: options.body,
            revision: options.revision,
            organization: options.organization,
            location: options.location,
            modified: options.modified,
            _memory: options.memory,
        }
    }
    fn recover(
        self,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<StorageLibraryRecovered, StorageLibraryError> {
        let record = StorageGameDocument::recover_record_cancellable(&self.body, cancellation)?;
        if record.id() != self.id {
            return Err(StorageLibraryError::Identity);
        }
        Ok(StorageLibraryRecovered {
            record,
            content_revision: self.revision,
            organization: self.organization,
            location: self.location,
            modified: self.modified,
            _memory: self._memory,
        })
    }
}

pub struct StorageLibraryRecovered {
    record: StorageLibraryRecord,
    content_revision: GameStorageRevision,
    organization: GameLibraryOrganizationState,
    location: StorageTrashEnumLocation,
    modified: Option<GameLibraryOrganizationTimestamp>,
    _memory: StorageLibraryMemory,
}

pub struct StorageLibrarySourceMemory {
    _memory: StorageLibraryMemory,
}

impl StorageLibraryRecovered {
    pub fn record(&self) -> &StorageLibraryRecord {
        &self.record
    }
    pub fn into_record(self) -> StorageLibraryRecord {
        self.record
    }
    pub fn into_retained_record(self) -> (StorageLibraryRecord, StorageLibrarySourceMemory) {
        (
            self.record,
            StorageLibrarySourceMemory {
                _memory: self._memory,
            },
        )
    }
    pub const fn content_revision(&self) -> GameStorageRevision {
        self.content_revision
    }
    pub fn organization(&self) -> &GameLibraryOrganizationState {
        &self.organization
    }
    pub const fn location(&self) -> StorageTrashEnumLocation {
        self.location
    }
    pub const fn modified(&self) -> Option<GameLibraryOrganizationTimestamp> {
        self.modified
    }
}

#[derive(Debug)]
pub struct StorageLibraryContentReceipt {
    id: GameId,
    revision: GameStorageRevision,
    catalog: StorageLibraryCatalogRevision,
}

impl StorageLibraryContentReceipt {
    pub(crate) const fn new(
        id: GameId,
        revision: GameStorageRevision,
        catalog: StorageLibraryCatalogRevision,
    ) -> Self {
        Self {
            id,
            revision,
            catalog,
        }
    }
    pub const fn id(&self) -> GameId {
        self.id
    }
    pub const fn content_revision(&self) -> GameStorageRevision {
        self.revision
    }
    pub const fn catalog_revision(&self) -> StorageLibraryCatalogRevision {
        self.catalog
    }
}

pub(crate) struct StorageLibraryPending {
    receiver: Option<
        oneshot::Receiver<Result<StorageLibraryEnumTransactionResponse, StorageLibraryError>>,
    >,
}

impl StorageLibraryPending {
    pub(crate) fn new(
        receiver: oneshot::Receiver<
            Result<StorageLibraryEnumTransactionResponse, StorageLibraryError>,
        >,
    ) -> Self {
        Self {
            receiver: Some(receiver),
        }
    }
    pub(crate) async fn wait(
        &mut self,
    ) -> Result<StorageLibraryEnumTransactionResponse, StorageLibraryError> {
        let receiver = self
            .receiver
            .as_mut()
            .ok_or(StorageLibraryError::AlreadyObserved)?;
        let received = receiver.await;
        self.receiver = None;
        received?
    }
}

pub enum StorageLibraryEnumMutationReceipt {
    Saved(StorageLibraryContentReceipt),
    Organized(GameLibraryOrganizationReceipt),
    DestructionApplied(StorageTrashReceipt),
    Restored(StorageTrashReceipt),
    Adopted(StorageLibraryEnumAdoptionOutcome),
}

enum StorageLibraryEnumMutationKind {
    Save,
    Organize,
    Destruction,
    Restore,
    Adopt,
}

pub struct StorageLibraryMutation {
    pending: StorageLibraryPending,
    kind: StorageLibraryEnumMutationKind,
}

impl StorageLibraryMutation {
    pub async fn wait(&mut self) -> Result<StorageLibraryEnumMutationReceipt, StorageLibraryError> {
        let response = self.pending.wait().await?;
        if !matches!(
            (&self.kind, &response),
            (
                StorageLibraryEnumMutationKind::Save,
                StorageLibraryEnumTransactionResponse::Saved(_)
            ) | (
                StorageLibraryEnumMutationKind::Organize,
                StorageLibraryEnumTransactionResponse::Organized(_)
            ) | (
                StorageLibraryEnumMutationKind::Destruction,
                StorageLibraryEnumTransactionResponse::DestructionApplied(_)
            ) | (
                StorageLibraryEnumMutationKind::Restore,
                StorageLibraryEnumTransactionResponse::Restored(_)
            ) | (
                StorageLibraryEnumMutationKind::Adopt,
                StorageLibraryEnumTransactionResponse::Adopted(_)
            )
        ) {
            return Err(StorageLibraryError::Protocol);
        }
        match response {
            #[cfg(test)]
            StorageLibraryEnumTransactionResponse::ImportTestSql => {
                Err(StorageLibraryError::Protocol)
            }
            StorageLibraryEnumTransactionResponse::Saved(receipt) => {
                Ok(StorageLibraryEnumMutationReceipt::Saved(receipt))
            }
            StorageLibraryEnumTransactionResponse::Organized(receipt) => {
                Ok(StorageLibraryEnumMutationReceipt::Organized(receipt))
            }
            StorageLibraryEnumTransactionResponse::DestructionApplied(receipt) => Ok(
                StorageLibraryEnumMutationReceipt::DestructionApplied(receipt),
            ),
            StorageLibraryEnumTransactionResponse::Restored(receipt) => {
                Ok(StorageLibraryEnumMutationReceipt::Restored(receipt))
            }
            StorageLibraryEnumTransactionResponse::Adopted(outcome) => {
                Ok(StorageLibraryEnumMutationReceipt::Adopted(outcome))
            }
            StorageLibraryEnumTransactionResponse::Metadata(_)
            | StorageLibraryEnumTransactionResponse::Loaded(_)
            | StorageLibraryEnumTransactionResponse::Page(_)
            | StorageLibraryEnumTransactionResponse::DestructionPreview(_)
            | StorageLibraryEnumTransactionResponse::AdoptionPage(_)
            | StorageLibraryEnumTransactionResponse::AdoptionCertification(_)
            | StorageLibraryEnumTransactionResponse::ImportSnapshot(_)
            | StorageLibraryEnumTransactionResponse::ImportCandidates(_)
            | StorageLibraryEnumTransactionResponse::ImportCandidate(_)
            | StorageLibraryEnumTransactionResponse::ImportEstimate(_)
            | StorageLibraryEnumTransactionResponse::ImportCommitted(_) => {
                Err(StorageLibraryError::Protocol)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StorageLibraryAdoptionCursor {
    after: Option<Uuid>,
}

impl StorageLibraryAdoptionCursor {
    pub const fn start() -> Self {
        Self { after: None }
    }
    pub const fn after(id: Uuid) -> Self {
        Self { after: Some(id) }
    }
    pub(crate) const fn boundary(self) -> Option<Uuid> {
        self.after
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StorageLibraryAdoptionCandidate {
    id: Uuid,
    revision: GameStorageRevision,
    associated: bool,
}

impl StorageLibraryAdoptionCandidate {
    pub(crate) const fn new(id: Uuid, revision: GameStorageRevision, associated: bool) -> Self {
        Self {
            id,
            revision,
            associated,
        }
    }
    pub const fn id(self) -> Uuid {
        self.id
    }
    pub const fn content_revision(self) -> GameStorageRevision {
        self.revision
    }
}

pub struct StorageLibraryAdoptionPage {
    candidates: Vec<StorageLibraryAdoptionCandidate>,
    next: Option<StorageLibraryAdoptionCursor>,
    catalog: StorageLibraryCatalogRevision,
    _memory: StorageLibraryMemory,
}

impl StorageLibraryAdoptionPage {
    pub(crate) fn new(
        candidates: Vec<StorageLibraryAdoptionCandidate>,
        next: Option<StorageLibraryAdoptionCursor>,
        catalog: StorageLibraryCatalogRevision,
        memory: StorageLibraryMemory,
    ) -> Self {
        Self {
            candidates,
            next,
            catalog,
            _memory: memory,
        }
    }
    pub fn candidates(&self) -> &[StorageLibraryAdoptionCandidate] {
        &self.candidates
    }
    pub const fn next_cursor(&self) -> Option<StorageLibraryAdoptionCursor> {
        self.next
    }
    pub const fn catalog_revision(&self) -> StorageLibraryCatalogRevision {
        self.catalog
    }
}

#[derive(Debug)]
pub enum StorageLibraryEnumAdoptionOutcome {
    Inserted(StorageLibraryCatalogRevision),
    AlreadyCurrent,
    Changed(Option<GameStorageRevision>),
}

pub enum StorageLibraryEnumAdoptionPreparation {
    Prepared(StorageLibraryPreparedAdoption),
    Unrelated,
    Changed(Option<GameStorageRevision>),
    Issue(StorageLibraryAdoptionIssue),
}

#[derive(Debug)]
pub struct StorageLibraryAdoptionIssue {
    id: Uuid,
    revision: GameStorageRevision,
    source: Box<StorageLibraryError>,
}

impl StorageLibraryAdoptionIssue {
    pub const fn id(&self) -> Uuid {
        self.id
    }
    pub const fn content_revision(&self) -> GameStorageRevision {
        self.revision
    }
    pub fn source_error(&self) -> &StorageLibraryError {
        &self.source
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StorageLibraryAdoptionCertification {
    catalog: StorageLibraryCatalogRevision,
    certified: bool,
}

impl StorageLibraryAdoptionCertification {
    pub(crate) const fn new(catalog: StorageLibraryCatalogRevision, certified: bool) -> Self {
        Self { catalog, certified }
    }
    pub(crate) const fn is_certified(self) -> bool {
        self.certified
    }
    pub(crate) const fn catalog(self) -> StorageLibraryCatalogRevision {
        self.catalog
    }
}

pub(crate) struct StorageLibraryAdoptionCertificationPending(StorageLibraryPending);

impl StorageLibraryAdoptionCertificationPending {
    pub(crate) async fn wait(
        &mut self,
    ) -> Result<StorageLibraryAdoptionCertification, StorageLibraryError> {
        let StorageLibraryEnumTransactionResponse::AdoptionCertification(certification) =
            self.0.wait().await?
        else {
            return Err(StorageLibraryError::Protocol);
        };
        Ok(certification)
    }
}

pub struct StorageLibraryRepository<'a> {
    client: StorageDocumentClient,
    executor: &'a CapacityExecutor,
}

impl<'a> StorageLibraryRepository<'a> {
    pub const fn new(client: StorageDocumentClient, executor: &'a CapacityExecutor) -> Self {
        Self { client, executor }
    }
    pub(crate) fn begin_certify_adoption(
        &self,
        expected: StorageLibraryCatalogRevision,
    ) -> Result<StorageLibraryAdoptionCertificationPending, StorageLibraryError> {
        Ok(StorageLibraryAdoptionCertificationPending(
            self.client
                .begin_library(StorageLibraryEnumTransactionRequest::CertifyAdoption(
                    expected,
                ))?,
        ))
    }
    pub(crate) fn begin_revoke_adoption(
        &self,
    ) -> Result<StorageLibraryAdoptionCertificationPending, StorageLibraryError> {
        Ok(StorageLibraryAdoptionCertificationPending(
            self.client
                .begin_library(StorageLibraryEnumTransactionRequest::RevokeAdoption)?,
        ))
    }
    pub async fn load(
        &self,
        id: GameId,
    ) -> Result<Option<StorageLibraryRecovered>, StorageLibraryError> {
        let response = self
            .client
            .begin_library(StorageLibraryEnumTransactionRequest::Load(id))?
            .wait()
            .await?;
        let StorageLibraryEnumTransactionResponse::Loaded(stored) = response else {
            return Err(StorageLibraryError::Protocol);
        };
        let Some(stored) = stored else {
            return Ok(None);
        };
        self.executor
            .submit_waiting(move |cancellation| stored.recover(cancellation))
            .await?
            .join()
            .await?
            .map(Some)
    }
    pub async fn query(
        &self,
        query: StorageLibraryQuery,
    ) -> Result<StorageLibraryQueryPage, StorageLibraryError> {
        let query = self
            .executor
            .submit_waiting(move |cancellation| query.prepare(cancellation))
            .await?
            .join()
            .await??;
        let response = self
            .client
            .begin_library(StorageLibraryEnumTransactionRequest::Query(query))?
            .wait()
            .await?;
        let StorageLibraryEnumTransactionResponse::Page(page) = response else {
            return Err(StorageLibraryError::Protocol);
        };
        Ok(page)
    }
    pub async fn organize(
        &self,
        request: GameLibraryOrganizationRequest,
    ) -> Result<GameLibraryOrganizationReceipt, StorageLibraryError> {
        let receipt = self.begin_organize(request)?.wait().await?;
        let StorageLibraryEnumMutationReceipt::Organized(receipt) = receipt else {
            return Err(StorageLibraryError::Protocol);
        };
        Ok(receipt)
    }
    pub async fn prepare_write(
        &self,
        id: GameId,
        expected: GameStorageRevision,
        body: String,
        change: StorageLibraryEnumContentChange,
    ) -> Result<StorageLibraryPreparedWrite, StorageLibraryError> {
        let memory = self.prepared_memory(body.capacity())?;
        self.executor
            .submit_waiting(move |cancellation| {
                StorageLibraryPreparedWrite::from_encoded(
                    StorageLibraryPreparedWriteOptions {
                        id,
                        expected,
                        body,
                        change,
                        memory,
                    },
                    cancellation,
                )
            })
            .await?
            .join()
            .await?
    }
    pub(crate) fn prepared_memory(
        &self,
        body_bytes: usize,
    ) -> Result<StorageLibraryPreparedMemory, StorageLibraryError> {
        let reservation = self
            .client
            .reserve_library_memory(StorageLibraryPreparedMemory::reservation_bytes(body_bytes)?)?;
        StorageLibraryPreparedMemory::new(body_bytes, reservation)
    }
    pub fn begin_save(
        &self,
        prepared: StorageLibraryPreparedWrite,
    ) -> Result<StorageLibraryMutation, StorageLibraryError> {
        self.begin_mutation(
            StorageLibraryEnumTransactionRequest::Save(prepared),
            StorageLibraryEnumMutationKind::Save,
        )
    }
    pub fn begin_organize(
        &self,
        request: GameLibraryOrganizationRequest,
    ) -> Result<StorageLibraryMutation, StorageLibraryError> {
        self.begin_mutation(
            StorageLibraryEnumTransactionRequest::Organize(request),
            StorageLibraryEnumMutationKind::Organize,
        )
    }
    pub async fn preview_destruction(
        &self,
        request: StorageTrashPreviewRequest,
    ) -> Result<StorageTrashPlan, StorageLibraryError> {
        let response = self
            .client
            .begin_library(StorageLibraryEnumTransactionRequest::PreviewDestruction(
                request,
            ))?
            .wait()
            .await?;
        let StorageLibraryEnumTransactionResponse::DestructionPreview(plan) = response else {
            return Err(StorageLibraryError::Protocol);
        };
        Ok(plan)
    }
    pub fn begin_destruction(
        &self,
        request: StorageTrashConfirmedRequest,
    ) -> Result<StorageLibraryMutation, StorageLibraryError> {
        self.begin_mutation(
            StorageLibraryEnumTransactionRequest::ApplyDestruction(request),
            StorageLibraryEnumMutationKind::Destruction,
        )
    }
    pub fn begin_restore(
        &self,
        request: StorageTrashRestoreRequest,
    ) -> Result<StorageLibraryMutation, StorageLibraryError> {
        self.begin_mutation(
            StorageLibraryEnumTransactionRequest::Restore(request),
            StorageLibraryEnumMutationKind::Restore,
        )
    }
    fn begin_mutation(
        &self,
        request: StorageLibraryEnumTransactionRequest,
        kind: StorageLibraryEnumMutationKind,
    ) -> Result<StorageLibraryMutation, StorageLibraryError> {
        Ok(StorageLibraryMutation {
            pending: self.client.begin_library(request)?,
            kind,
        })
    }
    pub async fn adoption_page(
        &self,
        cursor: StorageLibraryAdoptionCursor,
    ) -> Result<StorageLibraryAdoptionPage, StorageLibraryError> {
        let response = self
            .client
            .begin_library(StorageLibraryEnumTransactionRequest::AdoptionPage(cursor))?
            .wait()
            .await?;
        let StorageLibraryEnumTransactionResponse::AdoptionPage(page) = response else {
            return Err(StorageLibraryError::Protocol);
        };
        Ok(page)
    }
    pub async fn prepare_adoption(
        &self,
        candidate: StorageLibraryAdoptionCandidate,
    ) -> Result<StorageLibraryEnumAdoptionPreparation, StorageLibraryError> {
        let Some(document) = self.client.read(candidate.id).await? else {
            return Ok(StorageLibraryEnumAdoptionPreparation::Changed(None));
        };
        let observed = GameStorageRevision::new(document.revision());
        if observed != candidate.revision {
            return Ok(StorageLibraryEnumAdoptionPreparation::Changed(Some(
                observed,
            )));
        }
        let memory = self.prepared_memory(document.body().len())?;
        self.executor
            .submit_waiting(move |cancellation| {
                StorageLibraryError::check_cancellation(cancellation)?;
                let preparation = (|| {
                    if !StorageLibraryAdoptionPreparation::recognizes(
                        document.body(),
                        candidate.associated,
                    )? {
                        return Ok(None);
                    }
                    StorageLibraryPreparedAdoption::from_encoded(
                        candidate.id.to_string().parse()?,
                        observed,
                        document.body(),
                        memory,
                        cancellation,
                    )
                    .map(Some)
                })();
                Ok(match preparation {
                    Ok(None) => StorageLibraryEnumAdoptionPreparation::Unrelated,
                    Ok(Some(prepared)) => StorageLibraryEnumAdoptionPreparation::Prepared(prepared),
                    Err(StorageLibraryError::Cancelled) => {
                        return Err(StorageLibraryError::Cancelled);
                    }
                    Err(source) => {
                        StorageLibraryEnumAdoptionPreparation::Issue(StorageLibraryAdoptionIssue {
                            id: candidate.id,
                            revision: observed,
                            source: Box::new(source),
                        })
                    }
                })
            })
            .await?
            .join()
            .await?
    }
    pub fn begin_adopt(
        &self,
        prepared: StorageLibraryPreparedAdoption,
    ) -> Result<StorageLibraryMutation, StorageLibraryError> {
        self.begin_mutation(
            StorageLibraryEnumTransactionRequest::Adopt(prepared),
            StorageLibraryEnumMutationKind::Adopt,
        )
    }
}
