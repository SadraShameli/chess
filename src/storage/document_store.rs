use std::num::{ParseIntError, TryFromIntError};
use std::path::PathBuf;
use std::sync::Arc;

use diesel::dsl::exists;
use diesel::prelude::*;
use diesel::upsert::excluded;
use serde::de::IgnoredAny;
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};
use uuid::Uuid;

use crate::capacity::budget::{CapacityBudget, CapacityBudgetError};
use crate::puzzle::history::PuzzleHistoryAuthorization;
use crate::puzzle::persistence::{PuzzlePersistenceReceipt, PuzzlePersistenceSaveRequest};
use crate::puzzle::session::PuzzleSessionId;
use crate::storage::connection::StorageConnection;
use crate::storage::dataset_registry::{
    StorageDatasetRegistry, StorageDatasetRegistryEnumRequest, StorageDatasetRegistryEnumResponse,
    StorageDatasetRegistryError,
};
use crate::storage::document_store::rows::{StorageDocumentRowContent, StorageDocumentRowReceipt};
use crate::storage::editor_repository::{
    StorageEditorRepositoryEnumRequest, StorageEditorRepositoryEnumResponse,
    StorageEditorRepositoryError, StorageEditorRepositoryPending,
    StorageEditorRepositoryTransaction,
};
use crate::storage::library_import::StorageLibraryImportPageLimit;
use crate::storage::library_repository::{StorageLibraryError, StorageLibraryPending};
use crate::storage::library_transaction::{
    StorageLibraryEnumTransactionRequest, StorageLibraryEnumTransactionResponse,
    StorageLibraryTransaction,
};
use crate::storage::migration::StorageMigrationRunner;
use crate::storage::opening_repository::{
    StorageOpeningEnumRequest, StorageOpeningEnumResponse, StorageOpeningError,
    StorageOpeningTransaction,
};
use crate::storage::preferences_repository::{
    StoragePreferencesEnumRequest, StoragePreferencesEnumResponse, StoragePreferencesError,
    StoragePreferencesTransaction,
};
use crate::storage::puzzle_repository::{
    StoragePuzzleError, StoragePuzzleRecovered, StoragePuzzleRunPage, StoragePuzzleTransaction,
    StoragePuzzleWrite,
};
use crate::storage::report_repository::{
    StorageReportEnumRequest, StorageReportEnumResponse, StorageReportError,
    StorageReportTransaction,
};
use crate::storage::schema::{documents, library_entries};

#[derive(Debug, Error)]
pub enum StorageDocumentError {
    #[error(transparent)]
    Capacity(#[from] CapacityBudgetError),
    #[error("Local database connection failed: {0}")]
    Connection(#[from] diesel::ConnectionError),
    #[error("Local database query failed: {0}")]
    Diesel(#[from] diesel::result::Error),
    #[error("Local database migration failed: {0}")]
    Migration(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("Invalid JSON document: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Stored revision is invalid: {0}")]
    StoredRevision(#[from] ParseIntError),
    #[error("Stored document identity is invalid: {0}")]
    StoredIdentity(#[from] uuid::Error),
    #[error("Document revision changed: expected {expected}, found {actual}")]
    Conflict { expected: u64, actual: u64 },
    #[error("The storage actor stopped with an unresolved transaction")]
    UnresolvedTransaction,
    #[error("The storage actor stopped with an unresolved import page ceiling")]
    UnresolvedImportPageCeiling,
    #[error("Document revision is exhausted")]
    RevisionOverflow,
    #[error("Indexed library document {id} requires an atomic content and projection write")]
    IndexedLibraryRecord { id: Uuid },
    #[error("Library catalog operation failed: {0}")]
    Library(#[source] Box<StorageLibraryError>),
    #[error("The database predates Diesel migrations and is unsupported; delete it to start fresh")]
    UnsupportedLegacyDatabase,
    #[error("Applied migration {version} is not part of this build")]
    FutureMigration { version: String },
    #[error("The database queue or document memory budget is full")]
    Saturated,
    #[error("The document exceeds the 64 MiB transport budget")]
    DocumentTooLarge,
    #[error("The database worker is closed")]
    Closed,
    #[error("The database worker failed: {0}")]
    Worker(#[from] tokio::task::JoinError),
    #[error("The database result channel closed: {0}")]
    Channel(#[from] oneshot::error::RecvError),
    #[error("Database numeric value exceeds the supported range: {0}")]
    Range(#[from] TryFromIntError),
    #[error("Database shutdown exceeded its absolute deadline: {0}")]
    CloseDeadline(#[source] tokio::time::error::Elapsed),
    #[error("Database shutdown previously failed: {0}")]
    CloseFailed(#[source] Arc<Self>),
}

pub struct StorageDocument {
    revision: u64,
    body: String,
    _memory_reservation: OwnedSemaphorePermit,
}

impl StorageDocument {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn body(&self) -> &str {
        &self.body
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StorageDocumentReceipt {
    id: Uuid,
    revision: u64,
}

impl StorageDocumentReceipt {
    pub fn id(self) -> Uuid {
        self.id
    }
    pub fn revision(self) -> u64 {
        self.revision
    }
}

enum StorageDocumentEnumCommand {
    Editor {
        request: Box<StorageEditorRepositoryEnumRequest>,
        memory_reservation: OwnedSemaphorePermit,
        _admission: OwnedSemaphorePermit,
        result: oneshot::Sender<
            Result<StorageEditorRepositoryEnumResponse, StorageEditorRepositoryError>,
        >,
    },
    Library {
        request: Box<StorageLibraryEnumTransactionRequest>,
        _memory_reservation: OwnedSemaphorePermit,
        _admission: OwnedSemaphorePermit,
        result: oneshot::Sender<Result<StorageLibraryEnumTransactionResponse, StorageLibraryError>>,
    },
    Report {
        request: Box<StorageReportEnumRequest>,
        _admission: OwnedSemaphorePermit,
        result: oneshot::Sender<Result<StorageReportEnumResponse, StorageReportError>>,
    },
    Preferences {
        request: Box<StoragePreferencesEnumRequest>,
        _admission: OwnedSemaphorePermit,
        result: oneshot::Sender<Result<StoragePreferencesEnumResponse, StoragePreferencesError>>,
    },
    Opening {
        request: Box<StorageOpeningEnumRequest>,
        _memory_reservation: OwnedSemaphorePermit,
        _admission: OwnedSemaphorePermit,
        result: oneshot::Sender<Result<StorageOpeningEnumResponse, StorageOpeningError>>,
    },
    PuzzleInventory {
        after: Option<PuzzleSessionId>,
        memory_reservation: OwnedSemaphorePermit,
        _admission: OwnedSemaphorePermit,
        result: oneshot::Sender<Result<StoragePuzzleRunPage, StoragePuzzleError>>,
    },
    PuzzleSave {
        request: Box<PuzzlePersistenceSaveRequest>,
        _memory_reservation: OwnedSemaphorePermit,
        _admission: OwnedSemaphorePermit,
        result: oneshot::Sender<Result<PuzzlePersistenceReceipt, StoragePuzzleError>>,
    },
    PuzzleRead {
        id: PuzzleSessionId,
        _admission: OwnedSemaphorePermit,
        result: oneshot::Sender<Result<Option<StoragePuzzleRecovered>, StoragePuzzleError>>,
    },
    DatasetRegistry {
        request: StorageDatasetRegistryEnumRequest,
        _admission: OwnedSemaphorePermit,
        result: oneshot::Sender<
            Result<StorageDatasetRegistryEnumResponse, StorageDatasetRegistryError>,
        >,
    },
    PuzzleDelete {
        authorization: PuzzleHistoryAuthorization,
        _admission: OwnedSemaphorePermit,
        result: oneshot::Sender<Result<usize, StoragePuzzleError>>,
    },
    Save {
        id: Uuid,
        expected_revision: u64,
        body: String,
        _memory_reservation: OwnedSemaphorePermit,
        result: oneshot::Sender<Result<StorageDocumentReceipt, StorageDocumentError>>,
    },
    Read {
        id: Uuid,
        result: oneshot::Sender<Result<Option<StorageDocument>, StorageDocumentError>>,
    },
    Inventory {
        after: Option<Uuid>,
        result: oneshot::Sender<Result<Vec<StorageDocumentReceipt>, StorageDocumentError>>,
    },
}

pub struct StorageDocumentStore {
    commands: Option<mpsc::Sender<StorageDocumentEnumCommand>>,
    worker: Option<JoinHandle<Result<(), StorageDocumentError>>>,
    client: StorageDocumentClient,
    close_failure: Option<Arc<StorageDocumentError>>,
}

#[derive(Clone)]
pub struct StorageDocumentClient {
    commands: mpsc::WeakSender<StorageDocumentEnumCommand>,
    memory: Arc<Semaphore>,
    admission: Arc<Semaphore>,
    library_preparation_origin: Uuid,
}

impl StorageDocumentStore {
    pub const QUEUE_CAPACITY: usize = 256;
    pub const MEMORY_BUDGET: usize = 64 * 1024 * 1024;
    pub const INVENTORY_PAGE_SIZE: usize = Self::QUEUE_CAPACITY;

    pub async fn open(path: PathBuf, budget: CapacityBudget) -> Result<Self, StorageDocumentError> {
        let initialization_lease = budget.lease(1).await?;
        let (commands, receiver) = mpsc::channel(Self::QUEUE_CAPACITY);
        let (ready_sender, ready_receiver) = oneshot::channel();
        let memory = Arc::new(Semaphore::new(Self::MEMORY_BUDGET));
        let actor_memory = Arc::clone(&memory);
        let runtime = tokio::runtime::Handle::current();
        let worker = tokio::task::spawn_blocking(move || {
            let initialized = StorageDocumentActor::open(path, actor_memory, budget, runtime);
            drop(initialization_lease);
            match initialized {
                Ok(actor) => {
                    if ready_sender
                        .send(Ok(actor.library_preparation_origin))
                        .is_err()
                    {
                        return Ok(());
                    }
                    actor.run(receiver)
                }
                Err(error) => {
                    if let Err(abandoned_error) = ready_sender.send(Err(error)) {
                        drop(abandoned_error);
                    }
                    Ok(())
                }
            }
        });
        match ready_receiver.await {
            Ok(Ok(library_preparation_origin)) => {
                let client = StorageDocumentClient {
                    commands: commands.downgrade(),
                    memory,
                    admission: Arc::new(Semaphore::new(Self::QUEUE_CAPACITY)),
                    library_preparation_origin,
                };
                Ok(Self {
                    commands: Some(commands),
                    worker: Some(worker),
                    client,
                    close_failure: None,
                })
            }
            Ok(Err(error)) => {
                worker.await??;
                Err(error)
            }
            Err(error) => {
                worker.await??;
                Err(StorageDocumentError::Channel(error))
            }
        }
    }

    pub fn client(&self) -> StorageDocumentClient {
        self.client.clone()
    }

    pub async fn close(mut self) -> Result<(), StorageDocumentError> {
        self.close_admission();
        self.observe_close(None).await
    }

    pub async fn close_retained(&mut self, deadline: Instant) -> Result<(), StorageDocumentError> {
        self.close_admission();
        self.observe_close(Some(deadline)).await
    }

    pub fn is_closed(&self) -> bool {
        self.worker.is_none()
    }

    fn close_admission(&mut self) {
        self.client.admission.close();
        drop(self.commands.take());
    }

    async fn observe_close(
        &mut self,
        deadline: Option<Instant>,
    ) -> Result<(), StorageDocumentError> {
        if let Some(worker) = &mut self.worker {
            let joined = match deadline {
                Some(deadline) => timeout_at(deadline, worker)
                    .await
                    .map_err(StorageDocumentError::CloseDeadline)?,
                None => worker.await,
            };
            self.worker = None;
            let result = joined
                .map_err(StorageDocumentError::Worker)
                .and_then(|result| result);
            if let Err(error) = result {
                self.close_failure = Some(Arc::new(error));
            }
        }
        match &self.close_failure {
            Some(error) => Err(StorageDocumentError::CloseFailed(Arc::clone(error))),
            None => Ok(()),
        }
    }
}

impl StorageDocumentClient {
    pub(super) fn reserve_editor_memory(
        &self,
        memory_bytes: usize,
    ) -> Result<OwnedSemaphorePermit, StorageDocumentError> {
        Self::reserve_memory(&self.memory, memory_bytes)
    }

    pub(super) fn begin_editor(
        &self,
        memory_bytes: usize,
        request: impl FnOnce() -> StorageEditorRepositoryEnumRequest,
    ) -> Result<StorageEditorRepositoryPending, StorageEditorRepositoryError> {
        let admission = self.admit()?;
        let memory_reservation = Self::reserve_memory(&self.memory, memory_bytes)?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::Editor {
            request: Box::new(request()),
            memory_reservation,
            _admission: admission,
            result,
        })?;
        Ok(StorageEditorRepositoryPending::new(receiver))
    }

    pub(crate) const fn library_preparation_origin(&self) -> Uuid {
        self.library_preparation_origin
    }

    pub(crate) fn begin_library(
        &self,
        request: StorageLibraryEnumTransactionRequest,
    ) -> Result<StorageLibraryPending, StorageLibraryError> {
        request.validate_memory_owner(self)?;
        let admission = self.admit()?;
        let memory_reservation =
            Self::reserve_memory(&self.memory, request.unreserved_admission_bytes()?)?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::Library {
            request: Box::new(request),
            _memory_reservation: memory_reservation,
            _admission: admission,
            result,
        })?;
        Ok(StorageLibraryPending::new(receiver))
    }

    pub(crate) async fn inventory_puzzle_runs(
        &self,
        after: Option<PuzzleSessionId>,
    ) -> Result<StoragePuzzleRunPage, StoragePuzzleError> {
        let admission = self.admit()?;
        let memory_reservation =
            Self::reserve_memory(&self.memory, StoragePuzzleRunPage::memory_bytes())?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::PuzzleInventory {
            after,
            memory_reservation,
            _admission: admission,
            result,
        })?;
        receiver.await?
    }

    pub(crate) fn begin_puzzle_save(
        &self,
        request: PuzzlePersistenceSaveRequest,
    ) -> Result<StoragePuzzleWrite, StoragePuzzleError> {
        let admission = self.admit()?;
        let memory_reservation =
            Self::reserve_memory(&self.memory, request.snapshot().transport_bytes()?)?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::PuzzleSave {
            request: Box::new(request),
            _memory_reservation: memory_reservation,
            _admission: admission,
            result,
        })?;
        Ok(StoragePuzzleWrite::new(receiver))
    }

    pub(crate) async fn report(
        &self,
        request: StorageReportEnumRequest,
    ) -> Result<StorageReportEnumResponse, StorageReportError> {
        let admission = self.admit()?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::Report {
            request: Box::new(request),
            _admission: admission,
            result,
        })?;
        receiver.await?
    }

    pub(crate) async fn preferences(
        &self,
        request: StoragePreferencesEnumRequest,
    ) -> Result<StoragePreferencesEnumResponse, StoragePreferencesError> {
        let admission = self.admit()?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::Preferences {
            request: Box::new(request),
            _admission: admission,
            result,
        })?;
        receiver.await?
    }

    pub(crate) async fn opening(
        &self,
        request: StorageOpeningEnumRequest,
        memory_bytes: usize,
    ) -> Result<StorageOpeningEnumResponse, StorageOpeningError> {
        let admission = self.admit()?;
        let memory_reservation = Self::reserve_memory(&self.memory, memory_bytes)?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::Opening {
            request: Box::new(request),
            _memory_reservation: memory_reservation,
            _admission: admission,
            result,
        })?;
        receiver.await?
    }

    pub(crate) async fn read_puzzle(
        &self,
        id: PuzzleSessionId,
    ) -> Result<Option<StoragePuzzleRecovered>, StoragePuzzleError> {
        let admission = self.admit()?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::PuzzleRead {
            id,
            _admission: admission,
            result,
        })?;
        receiver.await?
    }

    pub(crate) async fn dataset_registry(
        &self,
        request: StorageDatasetRegistryEnumRequest,
    ) -> Result<StorageDatasetRegistryEnumResponse, StorageDatasetRegistryError> {
        let admission = self.admit()?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::DatasetRegistry {
            request,
            _admission: admission,
            result,
        })?;
        receiver.await?
    }

    pub(crate) async fn delete_puzzle_history(
        &self,
        authorization: PuzzleHistoryAuthorization,
    ) -> Result<usize, StoragePuzzleError> {
        let admission = self.admit()?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::PuzzleDelete {
            authorization,
            _admission: admission,
            result,
        })?;
        receiver.await?
    }

    pub async fn save(
        &self,
        id: Uuid,
        expected_revision: u64,
        body: String,
    ) -> Result<StorageDocumentReceipt, StorageDocumentError> {
        let _admission = self.admit()?;
        let memory_reservation = Self::reserve_memory(&self.memory, body.len())?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::Save {
            id,
            expected_revision,
            body,
            _memory_reservation: memory_reservation,
            result,
        })?;
        receiver.await?
    }

    pub async fn read(&self, id: Uuid) -> Result<Option<StorageDocument>, StorageDocumentError> {
        let _admission = self.admit()?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::Read { id, result })?;
        receiver.await?
    }

    pub async fn inventory(
        &self,
        after: Option<Uuid>,
    ) -> Result<Vec<StorageDocumentReceipt>, StorageDocumentError> {
        let _admission = self.admit()?;
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::Inventory { after, result })?;
        receiver.await?
    }

    fn admit(&self) -> Result<OwnedSemaphorePermit, StorageDocumentError> {
        Arc::clone(&self.admission)
            .try_acquire_owned()
            .map_err(|error| match error {
                TryAcquireError::Closed => StorageDocumentError::Closed,
                TryAcquireError::NoPermits => StorageDocumentError::Saturated,
            })
    }

    pub(super) fn reserve_memory(
        memory: &Arc<Semaphore>,
        bytes: usize,
    ) -> Result<OwnedSemaphorePermit, StorageDocumentError> {
        if bytes > StorageDocumentStore::MEMORY_BUDGET {
            return Err(StorageDocumentError::DocumentTooLarge);
        }
        Arc::clone(memory)
            .try_acquire_many_owned(u32::try_from(bytes)?)
            .map_err(|_error| StorageDocumentError::Saturated)
    }

    pub(crate) fn reserve_library_memory(
        &self,
        bytes: usize,
    ) -> Result<OwnedSemaphorePermit, StorageDocumentError> {
        Self::reserve_memory(&self.memory, bytes)
    }

    pub(crate) fn is_library_memory_owner(&self, reservation: &OwnedSemaphorePermit) -> bool {
        Arc::ptr_eq(&self.memory, reservation.semaphore())
    }

    fn enqueue(&self, command: StorageDocumentEnumCommand) -> Result<(), StorageDocumentError> {
        if self.admission.is_closed() {
            return Err(StorageDocumentError::Closed);
        }
        self.commands
            .upgrade()
            .ok_or(StorageDocumentError::Closed)?
            .try_send(command)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_command) => StorageDocumentError::Saturated,
                mpsc::error::TrySendError::Closed(_command) => StorageDocumentError::Closed,
            })
    }
}

struct StorageDocumentActor {
    connection: StorageConnection,
    memory: Arc<Semaphore>,
    budget: CapacityBudget,
    runtime: tokio::runtime::Handle,
    library_preparation_origin: Uuid,
}

impl StorageDocumentActor {
    fn open(
        path: PathBuf,
        memory: Arc<Semaphore>,
        budget: CapacityBudget,
        runtime: tokio::runtime::Handle,
    ) -> Result<Self, StorageDocumentError> {
        let mut diesel = StorageConnection::open(&path)?;
        StorageMigrationRunner::run_embedded(diesel.diesel())?;
        Ok(Self {
            connection: diesel,
            memory,
            budget,
            runtime,
            library_preparation_origin: Uuid::new_v4(),
        })
    }

    fn run(
        mut self,
        mut receiver: mpsc::Receiver<StorageDocumentEnumCommand>,
    ) -> Result<(), StorageDocumentError> {
        while let Some(command) = receiver.blocking_recv() {
            let lease = self.runtime.block_on(self.budget.lease(1))?;
            match command {
                StorageDocumentEnumCommand::Editor {
                    request,
                    memory_reservation,
                    _admission,
                    result,
                } => {
                    let execution = StorageEditorRepositoryTransaction::execute(
                        self.connection.diesel(),
                        *request,
                        memory_reservation,
                    );
                    let unresolved = execution.is_unresolved();
                    let outcome = execution.into_outcome();
                    drop((_admission, lease));
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                    if unresolved {
                        return Err(StorageDocumentError::UnresolvedTransaction);
                    }
                }
                StorageDocumentEnumCommand::Library {
                    request,
                    _memory_reservation,
                    _admission,
                    result,
                } => {
                    let outcome = StorageLibraryTransaction::execute(
                        self.connection.diesel(),
                        *request,
                        &self.memory,
                        self.library_preparation_origin,
                    );
                    let unresolved = !matches!(
                        StorageLibraryImportPageLimit::is_autocommit(self.connection.diesel()),
                        Ok(true)
                    );
                    let unresolved_import_limit = outcome
                        .as_ref()
                        .err()
                        .is_some_and(StorageLibraryError::requires_connection_retirement);
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                    drop((_memory_reservation, _admission, lease));
                    if unresolved {
                        return Err(StorageDocumentError::UnresolvedTransaction);
                    }
                    if unresolved_import_limit {
                        return Err(StorageDocumentError::UnresolvedImportPageCeiling);
                    }
                }
                StorageDocumentEnumCommand::Report {
                    request,
                    _admission,
                    result,
                } => {
                    let outcome =
                        StorageReportTransaction::execute(self.connection.diesel(), *request);
                    drop((_admission, lease));
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                }
                StorageDocumentEnumCommand::Preferences {
                    request,
                    _admission,
                    result,
                } => {
                    let outcome =
                        StoragePreferencesTransaction::execute(self.connection.diesel(), *request);
                    drop((_admission, lease));
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                }
                StorageDocumentEnumCommand::Opening {
                    request,
                    _memory_reservation,
                    _admission,
                    result,
                } => {
                    let outcome =
                        StorageOpeningTransaction::execute(self.connection.diesel(), *request);
                    drop((_memory_reservation, _admission, lease));
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                }
                StorageDocumentEnumCommand::PuzzleInventory {
                    after,
                    memory_reservation,
                    _admission,
                    result,
                } => {
                    let outcome = StoragePuzzleTransaction::inventory(
                        self.connection.diesel(),
                        after,
                        memory_reservation,
                    );
                    drop((_admission, lease));
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                }
                StorageDocumentEnumCommand::PuzzleSave {
                    request,
                    _memory_reservation,
                    _admission,
                    result,
                } => {
                    let outcome =
                        StoragePuzzleTransaction::save(self.connection.diesel(), *request);
                    drop((_memory_reservation, _admission, lease));
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                }
                StorageDocumentEnumCommand::PuzzleRead {
                    id,
                    _admission,
                    result,
                } => {
                    let outcome =
                        StoragePuzzleTransaction::load(self.connection.diesel(), id, &self.memory);
                    drop((_admission, lease));
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                }
                StorageDocumentEnumCommand::DatasetRegistry {
                    request,
                    _admission,
                    result,
                } => {
                    let outcome =
                        StorageDatasetRegistry::execute(self.connection.diesel(), request);
                    drop((_admission, lease));
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                }
                StorageDocumentEnumCommand::PuzzleDelete {
                    authorization,
                    _admission,
                    result,
                } => {
                    let outcome =
                        StoragePuzzleTransaction::delete(self.connection.diesel(), &authorization);
                    drop((_admission, lease));
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                }
                StorageDocumentEnumCommand::Save {
                    id,
                    expected_revision,
                    body,
                    _memory_reservation,
                    result,
                } => {
                    let outcome = self.save(id, expected_revision, &body);
                    drop((body, _memory_reservation, lease));
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                }
                StorageDocumentEnumCommand::Read { id, result } => {
                    let outcome = self.read(id);
                    drop(lease);
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                }
                StorageDocumentEnumCommand::Inventory { after, result } => {
                    let outcome = self.inventory(after);
                    drop(lease);
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                }
            }
        }
        Ok(())
    }

    fn save(
        &mut self,
        id: Uuid,
        expected_revision: u64,
        body: &str,
    ) -> Result<StorageDocumentReceipt, StorageDocumentError> {
        serde_json::from_str::<IgnoredAny>(body)?;
        let key = id.to_string();
        let revision = self
            .connection
            .diesel()
            .immediate_transaction(|transaction| {
                let stored: Option<String> = documents::table
                    .filter(documents::id.eq(&key))
                    .select(documents::revision)
                    .first(transaction)
                    .optional()?;
                let actual = stored
                    .as_deref()
                    .map(str::parse::<u64>)
                    .transpose()?
                    .unwrap_or(0);
                if actual != expected_revision {
                    return Err(StorageDocumentError::Conflict {
                        expected: expected_revision,
                        actual,
                    });
                }
                let indexed: bool = diesel::select(exists(
                    library_entries::table.filter(library_entries::game_id.eq(&key)),
                ))
                .get_result(transaction)?;
                if indexed {
                    return Err(StorageDocumentError::IndexedLibraryRecord { id });
                }
                let revision = actual
                    .checked_add(1)
                    .ok_or(StorageDocumentError::RevisionOverflow)?;
                diesel::insert_into(documents::table)
                    .values((
                        documents::id.eq(&key),
                        documents::revision.eq(revision.to_string()),
                        documents::body.eq(body),
                    ))
                    .on_conflict(documents::id)
                    .do_update()
                    .set((
                        documents::revision.eq(excluded(documents::revision)),
                        documents::body.eq(excluded(documents::body)),
                    ))
                    .execute(transaction)?;
                StorageLibraryTransaction::diesel_bump_catalog(transaction)
                    .map_err(|error| StorageDocumentError::Library(Box::new(error)))?;
                Ok(revision)
            })?;
        Ok(StorageDocumentReceipt { id, revision })
    }

    fn read(&mut self, id: Uuid) -> Result<Option<StorageDocument>, StorageDocumentError> {
        let stored = documents::table
            .filter(documents::id.eq(id.to_string()))
            .select(StorageDocumentRowContent::as_select())
            .first(self.connection.diesel())
            .optional()?;
        stored
            .map(|row| {
                let memory_reservation =
                    StorageDocumentClient::reserve_memory(&self.memory, row.body.len())?;
                Ok(StorageDocument {
                    revision: row.revision.parse()?,
                    body: row.body,
                    _memory_reservation: memory_reservation,
                })
            })
            .transpose()
    }

    fn inventory(
        &mut self,
        after: Option<Uuid>,
    ) -> Result<Vec<StorageDocumentReceipt>, StorageDocumentError> {
        let cursor = after.map(|id| id.to_string()).unwrap_or_default();
        let rows = documents::table
            .filter(documents::id.gt(cursor))
            .order(documents::id.asc())
            .limit(i64::try_from(StorageDocumentStore::INVENTORY_PAGE_SIZE)?)
            .select(StorageDocumentRowReceipt::as_select())
            .load(self.connection.diesel())?;
        rows.into_iter()
            .map(|row| {
                Ok(StorageDocumentReceipt {
                    id: row.id.parse()?,
                    revision: row.revision.parse()?,
                })
            })
            .collect()
    }
}

mod rows;
