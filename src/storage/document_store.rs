use std::num::{ParseIntError, TryFromIntError};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::de::IgnoredAny;
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::storage::migration::StorageMigration;

#[derive(Debug, Error)]
pub enum StorageDocumentError {
    #[error("Local database operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Invalid JSON document: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Stored revision is invalid: {0}")]
    StoredRevision(#[from] ParseIntError),
    #[error("Document revision changed: expected {expected}, found {actual}")]
    Conflict { expected: u64, actual: u64 },
    #[error("Document revision is exhausted")]
    RevisionOverflow,
    #[error("Database schema {found} is newer than supported schema {supported}")]
    FutureSchema { found: u32, supported: u32 },
    #[error("Migration versions must form a nonempty consecutive sequence starting at one")]
    InvalidMigrations,
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
}

pub struct StorageDocumentStore {
    commands: mpsc::Sender<StorageDocumentEnumCommand>,
    worker: JoinHandle<Result<(), StorageDocumentError>>,
    memory: Arc<Semaphore>,
}

impl StorageDocumentStore {
    pub const QUEUE_CAPACITY: usize = 256;
    pub const MEMORY_BUDGET: usize = 64 * 1024 * 1024;

    pub async fn open(path: PathBuf) -> Result<Self, StorageDocumentError> {
        Self::open_with_migrations(path, StorageMigration::core()).await
    }

    pub async fn open_with_migrations(
        path: PathBuf,
        migrations: Vec<StorageMigration>,
    ) -> Result<Self, StorageDocumentError> {
        let (commands, receiver) = mpsc::channel(Self::QUEUE_CAPACITY);
        let (ready_sender, ready_receiver) = oneshot::channel();
        let memory = Arc::new(Semaphore::new(Self::MEMORY_BUDGET));
        let actor_memory = Arc::clone(&memory);
        let worker = tokio::task::spawn_blocking(move || {
            let initialized = StorageDocumentActor::open(path, migrations, actor_memory);
            match initialized {
                Ok(actor) => {
                    if ready_sender.send(Ok(())).is_err() {
                        return Ok(());
                    }
                    actor.run(receiver);
                    Ok(())
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
            Ok(Ok(())) => Ok(Self {
                commands,
                worker,
                memory,
            }),
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

    pub async fn save(
        &self,
        id: Uuid,
        expected_revision: u64,
        body: String,
    ) -> Result<StorageDocumentReceipt, StorageDocumentError> {
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
        let (result, receiver) = oneshot::channel();
        self.enqueue(StorageDocumentEnumCommand::Read { id, result })?;
        receiver.await?
    }

    pub async fn close(self) -> Result<(), StorageDocumentError> {
        drop(self.commands);
        self.worker.await?
    }

    fn reserve_memory(
        memory: &Arc<Semaphore>,
        bytes: usize,
    ) -> Result<OwnedSemaphorePermit, StorageDocumentError> {
        if bytes > Self::MEMORY_BUDGET {
            return Err(StorageDocumentError::DocumentTooLarge);
        }
        Arc::clone(memory)
            .try_acquire_many_owned(u32::try_from(bytes)?)
            .map_err(|_error| StorageDocumentError::Saturated)
    }

    fn enqueue(&self, command: StorageDocumentEnumCommand) -> Result<(), StorageDocumentError> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_command) => StorageDocumentError::Saturated,
                mpsc::error::TrySendError::Closed(_command) => StorageDocumentError::Closed,
            })
    }
}

struct StorageDocumentActor {
    connection: Connection,
    memory: Arc<Semaphore>,
}

impl StorageDocumentActor {
    const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

    fn open(
        path: PathBuf,
        migrations: Vec<StorageMigration>,
        memory: Arc<Semaphore>,
    ) -> Result<Self, StorageDocumentError> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Self::BUSY_TIMEOUT)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        StorageMigration::apply(&mut connection, &migrations)?;
        Ok(Self { connection, memory })
    }

    fn run(mut self, mut receiver: mpsc::Receiver<StorageDocumentEnumCommand>) {
        while let Some(command) = receiver.blocking_recv() {
            match command {
                StorageDocumentEnumCommand::Save {
                    id,
                    expected_revision,
                    body,
                    _memory_reservation,
                    result,
                } => {
                    let outcome = self.save(id, expected_revision, &body);
                    drop((body, _memory_reservation));
                    if let Err(abandoned_result) = result.send(outcome) {
                        drop(abandoned_result);
                    }
                }
                StorageDocumentEnumCommand::Read { id, result } => {
                    if let Err(abandoned_result) = result.send(self.read(id)) {
                        drop(abandoned_result);
                    }
                }
            }
        }
    }

    fn save(
        &mut self,
        id: Uuid,
        expected_revision: u64,
        body: &str,
    ) -> Result<StorageDocumentReceipt, StorageDocumentError> {
        serde_json::from_str::<IgnoredAny>(body)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored: Option<String> = transaction
            .query_row(
                "SELECT revision FROM documents WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
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
        let revision = actual
            .checked_add(1)
            .ok_or(StorageDocumentError::RevisionOverflow)?;
        transaction.execute("INSERT INTO documents (id, revision, body) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET revision = excluded.revision, body = excluded.body", params![id.to_string(), revision.to_string(), body])?;
        transaction.commit()?;
        Ok(StorageDocumentReceipt { id, revision })
    }

    fn read(&self, id: Uuid) -> Result<Option<StorageDocument>, StorageDocumentError> {
        let stored: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT revision, body FROM documents WHERE id = ?1",
                [id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        stored
            .map(|(revision, body)| {
                let memory_reservation =
                    StorageDocumentStore::reserve_memory(&self.memory, body.len())?;
                Ok(StorageDocument {
                    revision: revision.parse()?,
                    body,
                    _memory_reservation: memory_reservation,
                })
            })
            .transpose()
    }
}
