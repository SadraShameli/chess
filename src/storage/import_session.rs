mod classes;
mod difference;
mod disk;
mod filesystem;
mod index;
mod model;
mod prepared;
mod stage;

use std::fs::File;
use std::future::Future;
use std::panic::UnwindSafe;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::capacity::executor::CapacityExecutorCancellation;
use crate::game::enums::GameId;
use crate::storage::import_session::disk::StorageImportSessionDisk;
use crate::storage::import_session::stage::{
    StorageImportSessionStage, StorageImportSessionStageParseOptions,
};

pub use crate::storage::import_session::difference::{
    StorageImportSessionDetailPage, StorageImportSessionDetailRequest,
    StorageImportSessionDifferences, StorageImportSessionEnumMatchSource,
    StorageImportSessionRelationship,
};
#[cfg(unix)]
pub use crate::storage::import_session::filesystem::StorageImportSessionUnixSpace;
#[cfg(windows)]
pub use crate::storage::import_session::filesystem::StorageImportSessionWindowsSpace;
pub use crate::storage::import_session::filesystem::{
    StorageImportSessionFilesystem, StorageImportSessionSpaceProvider,
};
pub use crate::storage::import_session::model::{
    StorageImportSessionBodyProof, StorageImportSessionCancellation, StorageImportSessionCandidate,
    StorageImportSessionCandidateBody, StorageImportSessionCandidateCursor,
    StorageImportSessionCandidatePage, StorageImportSessionCandidateQuery,
    StorageImportSessionCatalogStamp, StorageImportSessionCommitFootprint,
    StorageImportSessionCommitReceipt, StorageImportSessionConfirmation,
    StorageImportSessionDestination, StorageImportSessionEnumCancelOutcome,
    StorageImportSessionEnumChoice, StorageImportSessionEnumCommitOutcome,
    StorageImportSessionEnumVolume, StorageImportSessionError, StorageImportSessionExclusion,
    StorageImportSessionFilesystemProvider, StorageImportSessionIdentityProvider,
    StorageImportSessionPage, StorageImportSessionPageRequest, StorageImportSessionRepository,
    StorageImportSessionRepositoryError, StorageImportSessionRepositoryFuture,
    StorageImportSessionResources, StorageImportSessionRow, StorageImportSessionSnapshot,
    StorageImportSessionSpace, StorageImportSessionSummary,
};
pub use crate::storage::import_session::prepared::{
    StorageImportSessionPreparedCommit, StorageImportSessionPreparedFrame,
};

type StorageImportSessionFuture = Pin<
    Box<
        dyn Future<Output = Result<StorageImportSessionResponse, StorageImportSessionError>> + Send,
    >,
>;
type StorageImportSessionResult<T> = Result<T, Arc<StorageImportSessionError>>;

#[derive(Clone, PartialEq, Eq)]
enum StorageImportSessionRequest {
    Prepare,
    Page(StorageImportSessionPageRequest),
    Detail(StorageImportSessionDetailRequest),
    Choose {
        ordinal: u64,
        choice: StorageImportSessionEnumChoice,
    },
    Freeze,
    Commit(StorageImportSessionConfirmation),
    Cancel,
}

#[derive(Clone)]
enum StorageImportSessionResponse {
    Summary(Arc<StorageImportSessionSummary>),
    Page(Arc<StorageImportSessionPage>),
    Detail(Arc<StorageImportSessionDetailPage>),
    Confirmation(StorageImportSessionConfirmation),
    Commit(Arc<StorageImportSessionEnumCommitOutcome>),
    Cancel(Arc<StorageImportSessionEnumCancelOutcome>),
}

#[derive(PartialEq, Eq)]
enum StorageImportSessionEnumPhase {
    Selected,
    Preparing,
    Ready,
    Committing,
    Committed,
    Failed,
    Cancelled,
}

struct StorageImportSessionWork {
    source: Option<File>,
    stage: StorageImportSessionStage,
    phase: StorageImportSessionEnumPhase,
    receipt: Option<StorageImportSessionCommitReceipt>,
    commit_confirmation: Option<StorageImportSessionConfirmation>,
    commit_outcome: Option<Arc<StorageImportSessionEnumCommitOutcome>>,
}

impl StorageImportSessionWork {
    fn require_ready(&self) -> Result<(), StorageImportSessionError> {
        if self.phase != StorageImportSessionEnumPhase::Ready {
            return Err(StorageImportSessionError::State);
        }
        Ok(())
    }

    fn cleanup(&mut self) -> Result<(), StorageImportSessionError> {
        self.stage.disk.cleanup()?;
        drop(self.source.take());
        Ok(())
    }
}

struct StorageImportSessionWorkLease {
    slot: Arc<Mutex<Option<StorageImportSessionWork>>>,
    work: Option<StorageImportSessionWork>,
}

impl StorageImportSessionWorkLease {
    fn take(
        slot: Arc<Mutex<Option<StorageImportSessionWork>>>,
    ) -> Result<Self, StorageImportSessionError> {
        let work = slot
            .lock()
            .map_err(|_error| StorageImportSessionError::Poisoned)?
            .take()
            .ok_or(StorageImportSessionError::State)?;
        Ok(Self {
            slot,
            work: Some(work),
        })
    }

    fn work(&mut self) -> Result<&mut StorageImportSessionWork, StorageImportSessionError> {
        self.work.as_mut().ok_or(StorageImportSessionError::State)
    }
}

impl Drop for StorageImportSessionWorkLease {
    fn drop(&mut self) {
        if let Some(mut work) = self.work.take() {
            if std::thread::panicking() {
                work.phase = StorageImportSessionEnumPhase::Failed;
            }
            let mut slot = match self.slot.lock() {
                Ok(slot) => slot,
                Err(poisoned) => poisoned.into_inner(),
            };
            *slot = Some(work);
        }
    }
}

struct StorageImportSessionRuntime<R, F, I> {
    id: GameId,
    resources: StorageImportSessionResources<R, F, I>,
    work: Arc<Mutex<Option<StorageImportSessionWork>>>,
    cancellation: StorageImportSessionCancellation,
}

impl<
    R: StorageImportSessionRepository,
    F: StorageImportSessionFilesystemProvider,
    I: StorageImportSessionIdentityProvider,
> StorageImportSessionRuntime<R, F, I>
{
    async fn read<T: Send>(
        &self,
        operation: StorageImportSessionRepositoryFuture<'_, T>,
    ) -> Result<T, StorageImportSessionError> {
        tokio::select! {
            biased;
            () = self.cancellation.wait() => Err(StorageImportSessionError::Cancelled),
            result = operation => Ok(result?),
        }
    }

    async fn worker<T: Send + 'static>(
        &self,
        operation: impl FnOnce(
            &mut StorageImportSessionWork,
            &CapacityExecutorCancellation,
            &StorageImportSessionCancellation,
        ) -> Result<T, StorageImportSessionError>
        + Send
        + UnwindSafe
        + 'static,
    ) -> Result<T, StorageImportSessionError> {
        let work = Arc::clone(&self.work);
        let cancellation = self.cancellation.clone();
        let (sender, mut result) = tokio::sync::oneshot::channel();
        let sender = Mutex::new(Some(sender));
        let job = self
            .resources
            .executor
            .submit_waiting(move |token| {
                let outcome = match StorageImportSessionWorkLease::take(work) {
                    Ok(mut lease) => operation(lease.work()?, token, &cancellation),
                    Err(error) => Err(error),
                };
                let sender = sender
                    .lock()
                    .map_err(|_error| StorageImportSessionError::Poisoned)?
                    .take()
                    .ok_or(StorageImportSessionError::State)?;
                if let Err(unobserved) = sender.send(outcome) {
                    drop(unobserved);
                }
                Ok::<(), StorageImportSessionError>(())
            })
            .await?;
        let outcome = tokio::select! {
            outcome = &mut result => outcome,
            () = self.cancellation.wait() => {
                job.cancel();
                result.await
            }
        };
        let completion = job
            .join()
            .await
            .map_err(StorageImportSessionError::from)
            .and_then(|result| result);
        match (outcome, completion) {
            (Ok(Ok(value)), Ok(())) => Ok(value),
            (Ok(Err(primary)), Err(cleanup)) => Err(StorageImportSessionError::Cleanup {
                primary: Arc::new(primary),
                cleanup: Arc::new(cleanup),
            }),
            (Ok(Err(error)), Ok(())) => Err(error),
            (Ok(Ok(_)) | Err(_), Err(error)) => Err(error),
            (Err(error), Ok(())) => Err(error.into()),
        }
    }

    async fn cleanup(&self) -> Result<(), StorageImportSessionError> {
        let work = Arc::clone(&self.work);
        self.resources
            .executor
            .submit_waiting(move |_token| {
                let mut lease = StorageImportSessionWorkLease::take(work)?;
                lease.work()?.cleanup()
            })
            .await?
            .join()
            .await?
    }

    async fn prepare(&self) -> Result<StorageImportSessionResponse, StorageImportSessionError> {
        let ready = self
            .worker(|work, _token, _cancellation| match work.phase {
                StorageImportSessionEnumPhase::Selected => {
                    work.phase = StorageImportSessionEnumPhase::Preparing;
                    Ok(None)
                }
                StorageImportSessionEnumPhase::Ready => Ok(Some(work.stage.summary)),
                StorageImportSessionEnumPhase::Preparing
                | StorageImportSessionEnumPhase::Committing
                | StorageImportSessionEnumPhase::Committed
                | StorageImportSessionEnumPhase::Failed
                | StorageImportSessionEnumPhase::Cancelled => Err(StorageImportSessionError::State),
            })
            .await?;
        if let Some(summary) = ready {
            return Ok(StorageImportSessionResponse::Summary(Arc::new(summary)));
        }
        let snapshot = self.read(self.resources.repository.snapshot()).await?;
        self.cancellation.check()?;
        let repository = Arc::clone(&self.resources.repository);
        let ids = Arc::clone(&self.resources.identities);
        let records = self
            .worker(move |work, token, cancellation| {
                work.stage.snapshot = Some(snapshot);
                let source = work
                    .source
                    .as_mut()
                    .ok_or(StorageImportSessionError::State)?;
                work.stage.parse(
                    StorageImportSessionStageParseOptions {
                        source,
                        repository: repository.as_ref(),
                        identities: ids.as_ref(),
                    },
                    token,
                    cancellation,
                )?;
                Ok(work.stage.records)
            })
            .await?;
        for ordinal in 1..=records {
            let Some(mut query) = self
                .worker(move |work, token, cancellation| {
                    StorageImportSessionStage::check(token, cancellation)?;
                    work.stage
                        .query(ordinal, StorageImportSessionCandidateCursor::start(), token)
                })
                .await?
            else {
                continue;
            };
            loop {
                self.cancellation.check()?;
                let page = self
                    .read(self.resources.repository.candidates(query.clone()))
                    .await?;
                page.validate(&query)?;
                for candidate in page.candidates() {
                    self.cancellation.check()?;
                    let body = self
                        .read(self.resources.repository.read_candidate(*candidate))
                        .await?;
                    if body.candidate() != *candidate {
                        return Err(StorageImportSessionRepositoryError::InvalidResponse.into());
                    }
                    self.worker(move |work, token, cancellation| {
                        StorageImportSessionStage::check(token, cancellation)?;
                        work.stage.candidate(ordinal, body, token)
                    })
                    .await?;
                }
                if let Some(next) = page.next_cursor() {
                    query = self
                        .worker(move |work, token, cancellation| {
                            StorageImportSessionStage::check(token, cancellation)?;
                            work.stage
                                .query(ordinal, next, token)?
                                .ok_or(StorageImportSessionError::State)
                        })
                        .await?;
                } else {
                    break;
                }
            }
            self.worker(move |work, _token, _cancellation| work.stage.finish_candidates(ordinal))
                .await?;
        }
        if self.read(self.resources.repository.snapshot()).await? != snapshot {
            return Err(StorageImportSessionRepositoryError::StaleCatalog.into());
        }
        let summary = self
            .worker(|work, token, cancellation| {
                let summary = work.stage.finish_preview(token, cancellation)?;
                work.phase = StorageImportSessionEnumPhase::Ready;
                Ok(summary)
            })
            .await?;
        Ok(StorageImportSessionResponse::Summary(Arc::new(summary)))
    }

    async fn freeze(&self) -> Result<StorageImportSessionResponse, StorageImportSessionError> {
        let (snapshot, footprint) = self
            .worker(|work, token, cancellation| {
                work.require_ready()?;
                StorageImportSessionStage::check(token, cancellation)?;
                Ok((
                    work.stage
                        .snapshot
                        .ok_or(StorageImportSessionError::State)?,
                    work.stage.footprint(token, cancellation)?,
                ))
            })
            .await?;
        if self.read(self.resources.repository.snapshot()).await? != snapshot {
            return Err(StorageImportSessionRepositoryError::StaleCatalog.into());
        }
        let sqlite = self
            .read(self.resources.repository.estimate_commit_space(
                snapshot,
                footprint,
                self.cancellation.clone(),
            ))
            .await?;
        let session = self.id;
        let confirmation = self
            .worker(move |work, token, cancellation| {
                work.stage.freeze(session, sqlite, token, cancellation)
            })
            .await?;
        Ok(StorageImportSessionResponse::Confirmation(confirmation))
    }

    async fn commit(
        &self,
        confirmation: StorageImportSessionConfirmation,
    ) -> Result<StorageImportSessionResponse, StorageImportSessionError> {
        let requested = confirmation.clone();
        let retained = self
            .worker(move |work, _token, _cancellation| {
                if work.receipt.is_some() {
                    if work.commit_confirmation.as_ref() != Some(&requested) {
                        return Err(StorageImportSessionError::Proof);
                    }
                    return Ok(work.commit_outcome.as_ref().map(Arc::clone));
                }
                work.require_ready()?;
                Ok(None)
            })
            .await?;
        if let Some(outcome) = retained {
            return Ok(StorageImportSessionResponse::Commit(outcome));
        }
        let requested = confirmation.clone();
        let prepared = self
            .worker(move |work, token, cancellation| {
                StorageImportSessionStage::check(token, cancellation)?;
                let prepared = work.stage.take_prepared(&requested, cancellation.clone())?;
                work.phase = StorageImportSessionEnumPhase::Committing;
                work.commit_confirmation = Some(requested);
                Ok(prepared)
            })
            .await?;
        let expected_count = prepared.footprint().count();
        let result = self
            .resources
            .repository
            .commit_import(prepared, self.cancellation.clone())
            .await;
        let receipt = match result {
            Ok(receipt) => receipt,
            Err(error) => {
                let mut work = self
                    .work
                    .try_lock()
                    .map_err(|_error| StorageImportSessionError::Poisoned)?;
                work.as_mut().ok_or(StorageImportSessionError::State)?.phase =
                    StorageImportSessionEnumPhase::Failed;
                return Err(error.into());
            }
        };
        {
            let mut work = self
                .work
                .try_lock()
                .map_err(|_error| StorageImportSessionError::Poisoned)?;
            let work = work.as_mut().ok_or(StorageImportSessionError::State)?;
            work.receipt = Some(receipt);
            work.phase = StorageImportSessionEnumPhase::Committed;
        }
        let cleanup = if receipt.commit_id() != self.id || receipt.count() != expected_count {
            Err(StorageImportSessionError::Repository(
                StorageImportSessionRepositoryError::InvalidResponse,
            ))
        } else {
            self.cleanup().await
        };
        let outcome = Arc::new(match cleanup {
            Ok(()) => StorageImportSessionEnumCommitOutcome::Committed(receipt),
            Err(cleanup) => StorageImportSessionEnumCommitOutcome::CommittedCleanupFailed {
                receipt,
                cleanup: Arc::new(cleanup),
            },
        });
        self.work
            .try_lock()
            .map_err(|_error| StorageImportSessionError::Poisoned)?
            .as_mut()
            .ok_or(StorageImportSessionError::State)?
            .commit_outcome = Some(Arc::clone(&outcome));
        Ok(StorageImportSessionResponse::Commit(outcome))
    }

    async fn detail(
        &self,
        request: StorageImportSessionDetailRequest,
    ) -> Result<StorageImportSessionResponse, StorageImportSessionError> {
        self.detail_page(request)
            .await
            .map(|page| StorageImportSessionResponse::Detail(Arc::new(page)))
    }

    async fn dispatch(
        &self,
        request: StorageImportSessionRequest,
    ) -> Result<StorageImportSessionResponse, StorageImportSessionError> {
        match request {
            StorageImportSessionRequest::Prepare => self.prepare().await,
            StorageImportSessionRequest::Page(request) => {
                self.worker(move |work, token, cancellation| {
                    work.require_ready()?;
                    StorageImportSessionStage::check(token, cancellation)?;
                    work.stage
                        .page(request)
                        .map(|page| StorageImportSessionResponse::Page(Arc::new(page)))
                })
                .await
            }
            StorageImportSessionRequest::Detail(request) => self.detail(request).await,
            StorageImportSessionRequest::Choose { ordinal, choice } => {
                self.worker(move |work, token, cancellation| {
                    work.require_ready()?;
                    work.stage
                        .choose(ordinal, choice, token, cancellation)
                        .map(|summary| StorageImportSessionResponse::Summary(Arc::new(summary)))
                })
                .await
            }
            StorageImportSessionRequest::Freeze => self.freeze().await,
            StorageImportSessionRequest::Commit(confirmation) => self.commit(confirmation).await,
            StorageImportSessionRequest::Cancel => {
                self.cleanup().await?;
                self.worker(|work, _token, _cancellation| {
                    if let Some(receipt) = work.receipt {
                        Ok(StorageImportSessionResponse::Cancel(Arc::new(
                            StorageImportSessionEnumCancelOutcome::Committed(receipt),
                        )))
                    } else {
                        work.phase = StorageImportSessionEnumPhase::Cancelled;
                        Ok(StorageImportSessionResponse::Cancel(Arc::new(
                            StorageImportSessionEnumCancelOutcome::Cancelled,
                        )))
                    }
                })
                .await
            }
        }
    }
}

pub struct StorageImportSession<R, F, I> {
    runtime: Arc<StorageImportSessionRuntime<R, F, I>>,
    pending: Option<(StorageImportSessionRequest, StorageImportSessionFuture)>,
    primary_failure: Option<Arc<StorageImportSessionError>>,
    completed: Option<(
        StorageImportSessionRequest,
        StorageImportSessionResult<StorageImportSessionResponse>,
    )>,
}

impl<
    R: StorageImportSessionRepository,
    F: StorageImportSessionFilesystemProvider,
    I: StorageImportSessionIdentityProvider,
> StorageImportSession<R, F, I>
{
    pub fn new(
        source: File,
        resources: StorageImportSessionResources<R, F, I>,
    ) -> Result<Self, StorageImportSessionError> {
        let id = resources.identities.next_game()?;
        let concrete = Arc::clone(&resources.filesystem);
        let filesystem: Arc<dyn StorageImportSessionFilesystemProvider> = concrete;
        let work = Arc::new(Mutex::new(Some(StorageImportSessionWork {
            source: Some(source),
            stage: StorageImportSessionStage::new(StorageImportSessionDisk::new(filesystem)),
            phase: StorageImportSessionEnumPhase::Selected,
            receipt: None,
            commit_confirmation: None,
            commit_outcome: None,
        })));
        Ok(Self {
            runtime: Arc::new(StorageImportSessionRuntime {
                id,
                resources,
                work,
                cancellation: StorageImportSessionCancellation::new(),
            }),
            pending: None,
            primary_failure: None,
            completed: None,
        })
    }

    async fn observe_pending(
        &mut self,
    ) -> StorageImportSessionResult<StorageImportSessionResponse> {
        let (request, pending) = self
            .pending
            .as_mut()
            .ok_or_else(|| Arc::new(StorageImportSessionError::State))?;
        let result = pending.as_mut().await.map_err(Arc::new);
        if *request != StorageImportSessionRequest::Cancel && self.primary_failure.is_none() {
            self.primary_failure = result.as_ref().err().map(Arc::clone);
        }
        self.completed = Some((request.clone(), result.clone()));
        self.pending = None;
        result
    }

    async fn perform(
        &mut self,
        request: StorageImportSessionRequest,
    ) -> StorageImportSessionResult<StorageImportSessionResponse> {
        if let Some((pending, _future)) = self.pending.as_ref() {
            if *pending != request {
                return Err(Arc::new(StorageImportSessionError::Pending));
            }
        } else {
            if let Some((completed, result)) = &self.completed
                && *completed == request
                && (request != StorageImportSessionRequest::Cancel || result.is_ok())
            {
                return result.clone();
            }
            let runtime = Arc::clone(&self.runtime);
            let operation = request.clone();
            self.pending = Some((
                request,
                Box::pin(async move { runtime.dispatch(operation).await }),
            ));
        }
        self.observe_pending().await
    }

    pub async fn prepare(
        &mut self,
    ) -> StorageImportSessionResult<Arc<StorageImportSessionSummary>> {
        let StorageImportSessionResponse::Summary(summary) =
            self.perform(StorageImportSessionRequest::Prepare).await?
        else {
            return Err(Arc::new(StorageImportSessionError::State));
        };
        Ok(summary)
    }

    pub async fn page(
        &mut self,
        request: StorageImportSessionPageRequest,
    ) -> StorageImportSessionResult<Arc<StorageImportSessionPage>> {
        let StorageImportSessionResponse::Page(page) = self
            .perform(StorageImportSessionRequest::Page(request))
            .await?
        else {
            return Err(Arc::new(StorageImportSessionError::State));
        };
        Ok(page)
    }

    pub async fn detail(
        &mut self,
        request: StorageImportSessionDetailRequest,
    ) -> StorageImportSessionResult<Arc<StorageImportSessionDetailPage>> {
        let StorageImportSessionResponse::Detail(page) = self
            .perform(StorageImportSessionRequest::Detail(request))
            .await?
        else {
            return Err(Arc::new(StorageImportSessionError::State));
        };
        Ok(page)
    }

    pub async fn choose(
        &mut self,
        ordinal: u64,
        choice: StorageImportSessionEnumChoice,
    ) -> StorageImportSessionResult<Arc<StorageImportSessionSummary>> {
        let StorageImportSessionResponse::Summary(summary) = self
            .perform(StorageImportSessionRequest::Choose { ordinal, choice })
            .await?
        else {
            return Err(Arc::new(StorageImportSessionError::State));
        };
        Ok(summary)
    }

    pub async fn freeze(&mut self) -> StorageImportSessionResult<StorageImportSessionConfirmation> {
        let StorageImportSessionResponse::Confirmation(confirmation) =
            self.perform(StorageImportSessionRequest::Freeze).await?
        else {
            return Err(Arc::new(StorageImportSessionError::State));
        };
        Ok(confirmation)
    }

    pub async fn commit(
        &mut self,
        confirmation: StorageImportSessionConfirmation,
    ) -> StorageImportSessionResult<Arc<StorageImportSessionEnumCommitOutcome>> {
        let StorageImportSessionResponse::Commit(outcome) = self
            .perform(StorageImportSessionRequest::Commit(confirmation))
            .await?
        else {
            return Err(Arc::new(StorageImportSessionError::State));
        };
        Ok(outcome)
    }

    pub async fn cancel_retained(
        &mut self,
    ) -> StorageImportSessionResult<Arc<StorageImportSessionEnumCancelOutcome>> {
        self.runtime.cancellation.cancel();
        if self.pending.is_some() {
            let observed = self.observe_pending().await;
            drop(observed);
        }
        let result = self.perform(StorageImportSessionRequest::Cancel).await;
        match result {
            Ok(StorageImportSessionResponse::Cancel(outcome)) => Ok(outcome),
            Ok(_) => Err(Arc::new(StorageImportSessionError::State)),
            Err(cleanup) => Err(match self.primary_failure.as_ref().map(Arc::clone) {
                None => cleanup,
                Some(primary) => Arc::new(StorageImportSessionError::Cleanup { primary, cleanup }),
            }),
        }
    }
}
