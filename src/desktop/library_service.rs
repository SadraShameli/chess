mod notation;
mod operation;
mod played;
mod port;
mod result;
mod work;

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use thiserror::Error;
use tokio::sync::{Semaphore, mpsc, watch};
use tokio::time::{Instant, timeout_at};

use crate::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumPending,
};
use crate::desktop::annotation_owner::{
    DesktopAnnotationOwnerError, DesktopAnnotationOwnerResources,
};
use crate::desktop::game_editor::DesktopGameEditorError;
use crate::desktop::game_registry::{DesktopGameRegistryError, DesktopGameRegistryPort};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::notation::{
    DesktopLibraryServiceNotation, DesktopLibraryServiceNotationProjection,
};
pub use crate::desktop::library_service::operation::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceOperation,
};
use crate::desktop::library_service::played::DesktopLibraryServicePlayed;
use crate::desktop::library_service::port::{
    DesktopLibraryServiceEnvelope, DesktopLibraryServiceReply, DesktopLibraryServiceShutdown,
};
pub use crate::desktop::library_service::port::{
    DesktopLibraryServiceFuture, DesktopLibraryServicePort, DesktopLibraryServiceProvider,
    DesktopLibraryServiceReceipt, DesktopLibraryServiceRequest,
    DesktopLibraryServiceShutdownReceipt,
};
pub use crate::desktop::library_service::result::{
    DesktopLibraryServiceEnumDisposition, DesktopLibraryServiceEnumResult,
    DesktopLibraryServiceEnumSaveReceipt,
};
use crate::desktop::library_service::work::DesktopLibraryServiceWork;
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::game::enums::GameId;
use crate::game::storage_repository::GameStorageWallTimeProvider;
use crate::storage::document_store::{StorageDocumentClient, StorageDocumentStore};
use crate::storage::library_repository::StorageLibraryError;

#[derive(Debug, Error)]
pub enum DesktopLibraryServiceError {
    #[error("The library service cannot admit another operation")]
    Busy,
    #[error("The library service is closing")]
    Closing,
    #[error("The library service has stopped")]
    Closed,
    #[error("The operation belongs to another library workspace")]
    ForeignWorkspace,
    #[error("The application receipt does not identify this retained library operation")]
    ForeignReceipt,
    #[error("The library operation was cancelled before acceptance")]
    Cancelled,
    #[error("The library editor source has not been opened")]
    NotOpen,
    #[error("The library source has accepted unsaved content and requires Retry Save")]
    DirtySource,
    #[error("The library operation returned an incompatible result")]
    Protocol,
    #[error("The library admission state is poisoned")]
    Poisoned,
    #[error("The original library operation remains owned: {source}")]
    Continuation {
        request: DesktopRequestId,
        expected: Option<crate::desktop::game_editor::DesktopGameEditorContext>,
        #[source]
        source: Arc<Self>,
    },
    #[error("The retained library cleanup failed: {0}")]
    Retained(#[source] Arc<Self>),
    #[error("The library owner remains active beyond its cleanup deadline: {0}")]
    Deadline(#[source] tokio::time::error::Elapsed),
    #[error("The library repository operation failed: {0}")]
    Library(#[source] Box<StorageLibraryError>),
    #[error("The running game operation failed: {0}")]
    Registry(#[source] Arc<DesktopGameRegistryError>),
    #[error("The annotation source operation failed: {0}")]
    Annotation(#[source] Arc<DesktopAnnotationOwnerError>),
    #[error(transparent)]
    Context(#[from] DesktopGameEditorError),
    #[error(transparent)]
    Executor(#[from] CapacityExecutorError),
}

impl DesktopLibraryServiceError {
    pub fn disposition(&self) -> DesktopLibraryServiceEnumDisposition {
        match self {
            Self::Continuation {
                request, expected, ..
            } => DesktopLibraryServiceEnumDisposition::Retained {
                request: *request,
                expected: *expected,
            },
            Self::Retained(source) => source.disposition(),
            Self::Busy
            | Self::Closing
            | Self::Closed
            | Self::ForeignWorkspace
            | Self::ForeignReceipt
            | Self::Cancelled
            | Self::NotOpen
            | Self::DirtySource
            | Self::Protocol
            | Self::Poisoned
            | Self::Deadline(_)
            | Self::Library(_)
            | Self::Registry(_)
            | Self::Annotation(_)
            | Self::Context(_)
            | Self::Executor(_) => DesktopLibraryServiceEnumDisposition::Refused,
        }
    }

    fn continuation(self, reply: &DesktopLibraryServiceReply) -> Self {
        if matches!(self, Self::Continuation { .. }) {
            return self;
        }
        Self::Continuation {
            request: reply.application.request_id(),
            expected: reply.editor_context(),
            source: Arc::new(self),
        }
    }
}

impl From<StorageLibraryError> for DesktopLibraryServiceError {
    fn from(error: StorageLibraryError) -> Self {
        Self::Library(Box::new(error))
    }
}

impl From<DesktopGameRegistryError> for DesktopLibraryServiceError {
    fn from(error: DesktopGameRegistryError) -> Self {
        Self::Registry(Arc::new(error))
    }
}

impl From<DesktopAnnotationOwnerError> for DesktopLibraryServiceError {
    fn from(error: DesktopAnnotationOwnerError) -> Self {
        Self::Annotation(Arc::new(error))
    }
}

pub struct DesktopLibraryServiceOptions {
    pub workspace: DesktopMessagesEntityId,
    pub storage: StorageDocumentClient,
    pub executor: Arc<CapacityExecutor>,
    pub games: DesktopGameRegistryPort,
    pub wall_time: Arc<dyn GameStorageWallTimeProvider>,
}

struct DesktopLibraryServiceSource {
    notation: Option<DesktopLibraryServiceNotation>,
    projection: watch::Receiver<DesktopLibraryServiceNotationProjection>,
}

type DesktopLibraryServiceWorkFuture =
    Pin<Box<dyn Future<Output = DesktopLibraryServiceWork> + Send + 'static>>;

struct DesktopLibraryServicePendingWork {
    reply: Arc<DesktopLibraryServiceReply>,
    future: DesktopLibraryServiceWorkFuture,
}

pub struct DesktopLibraryService {
    options: DesktopLibraryServiceOptions,
    requests: mpsc::Receiver<DesktopLibraryServiceEnvelope>,
    shutdown: mpsc::Receiver<DesktopLibraryServiceShutdown>,
    sources: HashMap<GameId, DesktopLibraryServiceSource>,
    active: HashSet<GameId>,
    work: Vec<DesktopLibraryServicePendingWork>,
    played: HashMap<DesktopRequestId, DesktopLibraryServicePlayed>,
    closing: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    finalized: bool,
}

impl DesktopLibraryService {
    const CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);

    pub fn new(options: DesktopLibraryServiceOptions) -> (Self, DesktopLibraryServicePort) {
        let (requests, receiver) = mpsc::channel(DesktopLibraryServicePort::CAPACITY);
        let (shutdown, shutdown_receiver) = mpsc::channel(1);
        let closing = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        let port = DesktopLibraryServicePort {
            workspace: options.workspace,
            requests,
            admission: Arc::new(Semaphore::new(DesktopLibraryServicePort::CAPACITY)),
            payloads: Arc::new(Semaphore::new(StorageDocumentStore::MEMORY_BUDGET)),
            replies: Arc::new(Mutex::new(VecDeque::new())),
            closing: Arc::clone(&closing),
            closed: Arc::clone(&closed),
            shutdown,
        };
        (
            Self {
                options,
                requests: receiver,
                shutdown: shutdown_receiver,
                sources: HashMap::new(),
                active: HashSet::new(),
                work: Vec::new(),
                played: HashMap::new(),
                closing,
                closed,
                finalized: false,
            },
            port,
        )
    }

    pub async fn run(&mut self) -> Result<(), DesktopLibraryServiceError> {
        while !self.closed.load(Ordering::Acquire) {
            tokio::select! {
                biased;
                Some(shutdown) = self.shutdown.recv() => {
                    let result = self.shutdown_retained(shutdown.deadline).await.map_err(Arc::new);
                    let retained = result.as_ref().err().cloned();
                    if let Err(abandoned) = shutdown.response.send(result) { drop(abandoned); }
                    return retained.map_or(Ok(()), |error| Err(DesktopLibraryServiceError::Retained(error)));
                }
                completed = poll_fn(|context| Self::poll_work(&mut self.work, context)), if !self.work.is_empty() => self.finish(completed),
                envelope = self.requests.recv() => match envelope {
                    Some(envelope) => self.begin(envelope),
                    None => return self.shutdown_retained(Instant::now() + Self::CLEANUP_TIMEOUT).await,
                },
            }
        }
        Ok(())
    }

    fn begin(&mut self, envelope: DesktopLibraryServiceEnvelope) {
        let operation = &envelope.reply.operation;
        let game = operation.game();
        if game.is_some_and(|game| self.active.contains(&game)) {
            let result = self.pending_snapshot(operation).map_err(Arc::new);
            envelope.reply.complete(result, false);
            return;
        }
        let original = envelope.reply.application.request_id();
        let retained = if self.played.contains_key(&original) {
            Some(original)
        } else if let DesktopLibraryServiceEnumAction::Editor {
            expected,
            action: DesktopAnnotationCommandsEnumAction::RetrySave,
        } = operation.action()
        {
            let retained = self
                .played
                .iter()
                .find(|(_, retained)| retained.game() == Some(expected.game()));
            if let Some((request, retained)) = retained {
                let refusal = if envelope.reply.application.is_cancellation_requested()
                    && !retained.has_application(&envelope.reply.application)
                {
                    Some(DesktopLibraryServiceError::Cancelled)
                } else {
                    retained.check_retry(*expected).err()
                };
                if let Some(error) = refusal {
                    envelope.reply.complete(Err(Arc::new(error)), false);
                    return;
                }
                Some(*request)
            } else {
                None
            }
        } else {
            None
        };
        let played = match retained.and_then(|request| self.played.remove(&request)) {
            Some(mut played) => {
                if let Err(error) = played.retain_retry(Arc::clone(&envelope.reply)) {
                    self.played.insert(played.request_id(), played);
                    envelope.reply.complete(Err(Arc::new(error)), false);
                    return;
                }
                Some(played)
            }
            None => None,
        };
        if let Some(game) = game {
            self.active.insert(game);
        }
        let notation = game
            .and_then(|game| self.sources.get_mut(&game))
            .and_then(|source| source.notation.take());
        let progress = if game.is_some() && notation.is_none() {
            let (progress, projection) =
                watch::channel(DesktopLibraryServiceNotationProjection::default());
            if let Some(game) = game {
                self.sources.insert(
                    game,
                    DesktopLibraryServiceSource {
                        notation: None,
                        projection,
                    },
                );
            }
            Some(progress)
        } else {
            None
        };
        let resources = DesktopAnnotationOwnerResources {
            executor: Arc::clone(&self.options.executor),
            client: self.options.storage.clone(),
            wall_time: Arc::clone(&self.options.wall_time),
        };
        let games = self.options.games.clone();
        let reply = played.as_ref().map_or_else(
            || Arc::clone(&envelope.reply),
            |played| Arc::clone(&played.completion),
        );
        self.work.push(DesktopLibraryServicePendingWork {
            reply,
            future: Box::pin(
                DesktopLibraryServiceWork::new(envelope, notation, played)
                    .execute(progress, resources, games),
            ),
        });
    }

    fn pending_snapshot(
        &self,
        operation: &DesktopLibraryServiceOperation,
    ) -> Result<DesktopLibraryServiceEnumResult, DesktopLibraryServiceError> {
        let DesktopLibraryServiceEnumAction::Editor {
            expected,
            action: DesktopAnnotationCommandsEnumAction::Snapshot,
        } = operation.action()
        else {
            return Err(DesktopLibraryServiceError::Busy);
        };
        let source = self
            .sources
            .get(&expected.game())
            .ok_or(DesktopLibraryServiceError::Busy)?;
        let projection = source.projection.borrow();
        let snapshot = projection
            .snapshot
            .as_ref()
            .ok_or(DesktopLibraryServiceError::Busy)?;
        snapshot.context().check_expected(*expected)?;
        Ok(DesktopLibraryServiceEnumResult::PendingSnapshot {
            snapshot: snapshot.clone(),
            pending: projection
                .pending
                .unwrap_or(DesktopAnnotationCommandsEnumPending::Computing),
        })
    }

    fn finish(&mut self, completed: DesktopLibraryServiceWork) {
        let game = completed.envelope.reply.operation.game();
        if let Some(game) = game {
            self.active.remove(&game);
            match completed.notation {
                Some(notation) => {
                    if let Some(source) = self.sources.get_mut(&game) {
                        source.notation = Some(notation);
                    }
                }
                None => {
                    self.sources.remove(&game);
                }
            }
        }
        let retryable = completed.result.is_err()
            && (game
                .and_then(|game| self.sources.get(&game))
                .and_then(|source| source.notation.as_ref())
                .is_some_and(DesktopLibraryServiceNotation::has_pending)
                || completed
                    .played
                    .as_ref()
                    .is_some_and(DesktopLibraryServicePlayed::has_pending));
        if let Some(retained) = completed.played
            && retained.has_pending()
        {
            self.played.insert(retained.request_id(), retained);
        }
        completed
            .envelope
            .reply
            .complete(completed.result, retryable);
    }

    fn poll_work(
        work: &mut Vec<DesktopLibraryServicePendingWork>,
        context: &mut Context<'_>,
    ) -> Poll<DesktopLibraryServiceWork> {
        for index in 0..work.len() {
            if let Poll::Ready(completed) = work[index].future.as_mut().poll(context) {
                drop(work.swap_remove(index));
                return Poll::Ready(completed);
            }
        }
        Poll::Pending
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.closed.load(Ordering::Acquire)
            && self.finalized
            && self.work.is_empty()
            && self.sources.is_empty()
            && self.played.is_empty()
            && self.requests.is_empty()
    }

    pub async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopLibraryServiceError> {
        self.closing.store(true, Ordering::Release);
        loop {
            while let Ok(envelope) = self.requests.try_recv() {
                self.begin(envelope);
            }
            if self.work.is_empty() {
                break;
            }
            let completed = timeout_at(
                deadline,
                poll_fn(|context| Self::poll_work(&mut self.work, context)),
            )
            .await
            .map_err(|elapsed| {
                let error = DesktopLibraryServiceError::Deadline(elapsed);
                match self.work.first() {
                    Some(work) => error.continuation(&work.reply),
                    None => error,
                }
            })?;
            self.finish(completed);
        }
        for source in self.sources.values_mut() {
            if let Some(notation) = &mut source.notation {
                timeout_at(deadline, notation.cleanup())
                    .await
                    .map_err(DesktopLibraryServiceError::Deadline)??;
            }
        }
        if let Some(retained) = self.played.values().find(|played| played.has_pending()) {
            return Err(DesktopLibraryServiceError::DirtySource.continuation(&retained.completion));
        }
        self.closed.store(true, Ordering::Release);
        self.requests.close();
        self.shutdown.close();
        while let Some(envelope) = timeout_at(deadline, self.requests.recv())
            .await
            .map_err(DesktopLibraryServiceError::Deadline)?
        {
            envelope
                .reply
                .complete(Err(Arc::new(DesktopLibraryServiceError::Closed)), false);
        }
        while let Some(shutdown) = timeout_at(deadline, self.shutdown.recv())
            .await
            .map_err(DesktopLibraryServiceError::Deadline)?
        {
            if let Err(abandoned) = shutdown.response.send(Ok(())) {
                drop(abandoned);
            }
        }
        self.played.clear();
        self.sources.clear();
        self.finalized = true;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/desktop/library_service.rs"]
pub(crate) mod tests;
