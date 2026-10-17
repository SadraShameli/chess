use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch};
use tokio::time::Instant;

use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceError, DesktopLibraryServiceOperation,
};
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::storage::library_repository::StorageLibraryError;

pub type DesktopLibraryServiceFuture = Pin<
    Box<
        dyn Future<
                Output = Result<DesktopLibraryServiceEnumResult, Arc<DesktopLibraryServiceError>>,
            > + Send
            + 'static,
    >,
>;
pub(super) type DesktopLibraryServiceResponse =
    Result<DesktopLibraryServiceEnumResult, Arc<DesktopLibraryServiceError>>;

pub trait DesktopLibraryServiceProvider: Send + Sync {
    fn execute(&self, request: DesktopLibraryServiceRequest) -> DesktopLibraryServiceFuture;
}

pub struct DesktopLibraryServiceRequest {
    receipt: DesktopApplicationReceipt,
    operation: DesktopLibraryServiceOperation,
}

impl DesktopLibraryServiceRequest {
    pub const fn new(
        receipt: DesktopApplicationReceipt,
        operation: DesktopLibraryServiceOperation,
    ) -> Self {
        Self { receipt, operation }
    }
    pub const fn receipt(&self) -> &DesktopApplicationReceipt {
        &self.receipt
    }
    pub const fn operation(&self) -> &DesktopLibraryServiceOperation {
        &self.operation
    }
    pub fn into_parts(self) -> (DesktopApplicationReceipt, DesktopLibraryServiceOperation) {
        (self.receipt, self.operation)
    }
}

pub(super) struct DesktopLibraryServiceReply {
    pub(super) application: DesktopApplicationReceipt,
    pub(super) operation: Arc<DesktopLibraryServiceOperation>,
    pub(super) response: watch::Sender<Option<DesktopLibraryServiceResponse>>,
    pub(super) retryable: AtomicBool,
    pub(super) publication_context: OnceLock<crate::desktop::game_editor::DesktopGameEditorContext>,
    pub(super) _payload: OwnedSemaphorePermit,
}

impl DesktopLibraryServiceReply {
    pub(super) fn editor_context(
        &self,
    ) -> Option<crate::desktop::game_editor::DesktopGameEditorContext> {
        self.operation
            .editor_context()
            .or_else(|| self.publication_context.get().copied())
    }

    pub(super) fn complete(&self, result: DesktopLibraryServiceResponse, retryable: bool) {
        self.retryable.store(retryable, Ordering::Release);
        self.response.send_replace(Some(result));
    }

    fn is_reclaimable(&self) -> bool {
        !self.retryable.load(Ordering::Acquire) && self.response.borrow().as_ref().is_some_and(|result| {
            match result {
                Ok(_) => true,
                Err(error) => error.disposition() == crate::desktop::library_service::DesktopLibraryServiceEnumDisposition::Refused,
            }
        })
    }
}

pub(super) struct DesktopLibraryServiceEnvelope {
    pub(super) reply: Arc<DesktopLibraryServiceReply>,
    pub(super) _slot: OwnedSemaphorePermit,
}

pub(super) struct DesktopLibraryServiceShutdown {
    pub(super) deadline: Instant,
    pub(super) response: oneshot::Sender<Result<(), Arc<DesktopLibraryServiceError>>>,
}

pub struct DesktopLibraryServiceShutdownReceipt {
    response: oneshot::Receiver<Result<(), Arc<DesktopLibraryServiceError>>>,
}

impl DesktopLibraryServiceShutdownReceipt {
    pub async fn wait(&mut self) -> Result<(), Arc<DesktopLibraryServiceError>> {
        (&mut self.response)
            .await
            .map_err(|_closed| Arc::new(DesktopLibraryServiceError::Closed))?
    }
}

#[derive(Clone)]
pub struct DesktopLibraryServiceReceipt {
    reply: Arc<DesktopLibraryServiceReply>,
    response: watch::Receiver<Option<DesktopLibraryServiceResponse>>,
}

impl DesktopLibraryServiceReceipt {
    pub fn request_id(&self) -> DesktopRequestId {
        self.reply.application.request_id()
    }
    pub fn application_receipt(&self) -> &DesktopApplicationReceipt {
        &self.reply.application
    }
    pub fn try_result(&self) -> Option<DesktopLibraryServiceResponse> {
        self.response.borrow().clone()
    }
    pub async fn wait(&mut self) -> DesktopLibraryServiceResponse {
        loop {
            if let Some(result) = self.try_result() {
                return result;
            }
            self.response
                .changed()
                .await
                .map_err(|_closed| Arc::new(DesktopLibraryServiceError::Closed))?;
        }
    }
}

#[derive(Clone)]
pub struct DesktopLibraryServicePort {
    pub(super) workspace: DesktopMessagesEntityId,
    pub(super) requests: mpsc::Sender<DesktopLibraryServiceEnvelope>,
    pub(super) admission: Arc<Semaphore>,
    pub(super) payloads: Arc<Semaphore>,
    pub(super) replies: Arc<Mutex<VecDeque<Arc<DesktopLibraryServiceReply>>>>,
    pub(super) closing: Arc<AtomicBool>,
    pub(super) closed: Arc<AtomicBool>,
    pub(super) shutdown: mpsc::Sender<DesktopLibraryServiceShutdown>,
}

impl DesktopLibraryServicePort {
    pub const CAPACITY: usize = 256;
    pub fn workspace(&self) -> DesktopMessagesEntityId {
        self.workspace
    }
    pub fn begin_closing(&self) {
        self.closing.store(true, Ordering::Release);
    }

    pub fn try_shutdown(
        &self,
        deadline: Instant,
    ) -> Result<DesktopLibraryServiceShutdownReceipt, DesktopLibraryServiceError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DesktopLibraryServiceError::Closed);
        }
        let permit = self.shutdown.try_reserve().map_err(Self::admission_error)?;
        let (response, receiver) = oneshot::channel();
        self.begin_closing();
        permit.send(DesktopLibraryServiceShutdown { deadline, response });
        Ok(DesktopLibraryServiceShutdownReceipt { response: receiver })
    }

    pub fn try_submit(
        &self,
        request: DesktopLibraryServiceRequest,
    ) -> Result<DesktopLibraryServiceReceipt, DesktopLibraryServiceError> {
        let (application, operation) = request.into_parts();
        if operation.workspace() != self.workspace {
            return Err(DesktopLibraryServiceError::ForeignWorkspace);
        }
        if self.closed.load(Ordering::Acquire) {
            return Err(DesktopLibraryServiceError::Closed);
        }
        let bytes = operation
            .transport_bytes()
            .map_err(StorageLibraryError::from)?;
        let mut replies = self.replies.try_lock().map_err(|error| match error {
            std::sync::TryLockError::WouldBlock => DesktopLibraryServiceError::Busy,
            std::sync::TryLockError::Poisoned(_) => DesktopLibraryServiceError::Poisoned,
        })?;
        if let Some(reply) = replies
            .iter()
            .find(|reply| reply.application.request_id() == application.request_id())
        {
            if !reply.application.shares_outcome(&application)
                || reply.operation.as_ref() != &operation
            {
                return Err(DesktopLibraryServiceError::ForeignReceipt);
            }
            if reply.retryable.load(Ordering::Acquire) && reply.response.borrow().is_some() {
                let slot = Arc::clone(&self.admission)
                    .try_acquire_owned()
                    .map_err(|_full| DesktopLibraryServiceError::Busy.continuation(reply))?;
                let permit = self
                    .requests
                    .try_reserve()
                    .map_err(|error| Self::admission_error(error).continuation(reply))?;
                reply.response.send_replace(None);
                permit.send(DesktopLibraryServiceEnvelope {
                    reply: Arc::clone(reply),
                    _slot: slot,
                });
            }
            return Ok(DesktopLibraryServiceReceipt {
                reply: Arc::clone(reply),
                response: reply.response.subscribe(),
            });
        }
        if self.closing.load(Ordering::Acquire) && !operation.is_available_while_closing() {
            return Err(DesktopLibraryServiceError::Closing);
        }
        let evict = if replies.len() == Self::CAPACITY {
            Some(
                replies
                    .iter()
                    .position(|reply| reply.is_reclaimable())
                    .ok_or(DesktopLibraryServiceError::Busy)?,
            )
        } else {
            None
        };
        let slot = Arc::clone(&self.admission)
            .try_acquire_owned()
            .map_err(|_full| DesktopLibraryServiceError::Busy)?;
        let permit = self.requests.try_reserve().map_err(Self::admission_error)?;
        if let Some(index) = evict {
            replies.remove(index);
        }
        let payload = loop {
            match Arc::clone(&self.payloads).try_acquire_many_owned(bytes) {
                Ok(payload) => break payload,
                Err(_full) => {
                    let index = replies
                        .iter()
                        .position(|reply| reply.is_reclaimable())
                        .ok_or(DesktopLibraryServiceError::Busy)?;
                    replies.remove(index);
                }
            }
        };
        let (response, receiver) = watch::channel(None);
        let reply = Arc::new(DesktopLibraryServiceReply {
            application,
            operation: Arc::new(operation),
            response,
            retryable: AtomicBool::new(false),
            publication_context: OnceLock::new(),
            _payload: payload,
        });
        permit.send(DesktopLibraryServiceEnvelope {
            reply: Arc::clone(&reply),
            _slot: slot,
        });
        replies.push_back(Arc::clone(&reply));
        Ok(DesktopLibraryServiceReceipt {
            reply,
            response: receiver,
        })
    }

    fn admission_error<T>(error: mpsc::error::TrySendError<T>) -> DesktopLibraryServiceError {
        match error {
            mpsc::error::TrySendError::Full(_) => DesktopLibraryServiceError::Busy,
            mpsc::error::TrySendError::Closed(_) => DesktopLibraryServiceError::Closed,
        }
    }
}

impl DesktopLibraryServiceProvider for DesktopLibraryServicePort {
    fn execute(&self, request: DesktopLibraryServiceRequest) -> DesktopLibraryServiceFuture {
        let admitted = self.try_submit(request);
        Box::pin(async move { admitted.map_err(Arc::new)?.wait().await })
    }
}
