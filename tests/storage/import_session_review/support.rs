use std::fs::File;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::Poll;

use crate::capacity::executor::CapacityExecutorCancellation;
use crate::storage::import_session::stage::tests::{
    StorageImportSessionTestFilesystem, StorageImportSessionTestFixture,
    StorageImportSessionTestIds, StorageImportSessionTestRepository,
};
use crate::storage::import_session::{
    StorageImportSession, StorageImportSessionBodyProof, StorageImportSessionCancellation,
    StorageImportSessionCandidate, StorageImportSessionCandidateBody,
    StorageImportSessionCandidatePage, StorageImportSessionCandidateQuery,
    StorageImportSessionCommitFootprint, StorageImportSessionCommitReceipt,
    StorageImportSessionEnumVolume, StorageImportSessionError,
    StorageImportSessionFilesystemProvider, StorageImportSessionPreparedCommit,
    StorageImportSessionRepository, StorageImportSessionRepositoryError,
    StorageImportSessionRepositoryFuture, StorageImportSessionResources,
    StorageImportSessionSnapshot, StorageImportSessionSpace,
};
use crate::storage::library_record::StorageLibraryRecord;

pub(super) type StorageImportSessionReviewSession = StorageImportSession<
    StorageImportSessionReviewRepository,
    StorageImportSessionReviewFilesystem,
    StorageImportSessionTestIds,
>;

pub(super) struct StorageImportSessionReviewCounter(AtomicU64);

impl StorageImportSessionReviewCounter {
    const LIMIT: u64 = 128 * 1024 * 1024;

    fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    fn add(&self, value: u64) -> std::io::Result<()> {
        self.0
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current
                    .checked_add(value)
                    .filter(|total| *total <= Self::LIMIT)
            })
            .map_err(|_value| {
                std::io::Error::other("review operation counter exceeded its bound")
            })?;
        Ok(())
    }

    pub(super) fn value(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

pub(super) struct StorageImportSessionReviewCounters {
    pub(super) reads: StorageImportSessionReviewCounter,
    pub(super) read_bytes: StorageImportSessionReviewCounter,
    pub(super) writes: StorageImportSessionReviewCounter,
    pub(super) write_bytes: StorageImportSessionReviewCounter,
    pub(super) projections: StorageImportSessionReviewCounter,
    pub(super) candidate_reads: StorageImportSessionReviewCounter,
    pub(super) candidate_pages: StorageImportSessionReviewCounter,
}

impl StorageImportSessionReviewCounters {
    fn new() -> Self {
        Self {
            reads: StorageImportSessionReviewCounter::new(),
            read_bytes: StorageImportSessionReviewCounter::new(),
            writes: StorageImportSessionReviewCounter::new(),
            write_bytes: StorageImportSessionReviewCounter::new(),
            projections: StorageImportSessionReviewCounter::new(),
            candidate_reads: StorageImportSessionReviewCounter::new(),
            candidate_pages: StorageImportSessionReviewCounter::new(),
        }
    }
}

pub(super) struct StorageImportSessionReviewCleanupGate {
    armed: AtomicBool,
    entered: tokio::sync::Notify,
    released: Mutex<bool>,
    resume: Condvar,
}

impl StorageImportSessionReviewCleanupGate {
    fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
            entered: tokio::sync::Notify::new(),
            released: Mutex::new(false),
            resume: Condvar::new(),
        }
    }

    pub(super) fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    fn wait(&self) -> Result<(), StorageImportSessionError> {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            let mut released = self
                .released
                .lock()
                .map_err(|_error| StorageImportSessionError::Poisoned)?;
            while !*released {
                released = self
                    .resume
                    .wait(released)
                    .map_err(|_error| StorageImportSessionError::Poisoned)?;
            }
        }
        Ok(())
    }

    pub(super) async fn entered(&self) {
        self.entered.notified().await;
    }

    pub(super) async fn release(&self) -> Result<(), StorageImportSessionError> {
        loop {
            let released = match self.released.try_lock() {
                Ok(mut released) => {
                    *released = true;
                    true
                }
                Err(std::sync::TryLockError::WouldBlock) => false,
                Err(std::sync::TryLockError::Poisoned(_error)) => {
                    return Err(StorageImportSessionError::Poisoned);
                }
            };
            if released {
                break;
            }
            tokio::task::yield_now().await;
        }
        self.resume.notify_all();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StorageImportSessionReviewEnumRead {
    Snapshot,
    Candidates,
    CandidateBody,
    Estimate,
}

pub(super) struct StorageImportSessionReviewReadGate {
    armed: Mutex<Option<StorageImportSessionReviewEnumRead>>,
    entered: tokio::sync::Notify,
    resume: tokio::sync::Notify,
    released: AtomicBool,
    active: AtomicBool,
    cancelled: AtomicBool,
}

struct StorageImportSessionReviewReadWait<'a>(&'a StorageImportSessionReviewReadGate);

impl Drop for StorageImportSessionReviewReadWait<'_> {
    fn drop(&mut self) {
        self.0.active.store(false, Ordering::SeqCst);
        if !self.0.released.load(Ordering::SeqCst) {
            self.0.cancelled.store(true, Ordering::SeqCst);
        }
    }
}

impl StorageImportSessionReviewReadGate {
    fn new() -> Self {
        Self {
            armed: Mutex::new(None),
            entered: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
            released: AtomicBool::new(false),
            active: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
        }
    }

    pub(super) fn arm(
        &self,
        operation: StorageImportSessionReviewEnumRead,
    ) -> Result<(), StorageImportSessionError> {
        *self
            .armed
            .try_lock()
            .map_err(|_error| StorageImportSessionError::Poisoned)? = Some(operation);
        Ok(())
    }

    async fn pass(
        &self,
        operation: StorageImportSessionReviewEnumRead,
    ) -> Result<(), StorageImportSessionRepositoryError> {
        let pause = {
            let mut armed = self
                .armed
                .try_lock()
                .map_err(|_error| StorageImportSessionRepositoryError::InvalidResponse)?;
            if *armed == Some(operation) {
                *armed = None;
                true
            } else {
                false
            }
        };
        if pause {
            self.active.store(true, Ordering::SeqCst);
            let waiting = StorageImportSessionReviewReadWait(self);
            self.entered.notify_one();
            self.resume.notified().await;
            drop(waiting);
        }
        Ok(())
    }

    pub(super) async fn entered(&self) {
        self.entered.notified().await;
    }

    pub(super) fn was_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub(super) fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    pub(super) fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
        self.resume.notify_one();
    }
}

pub(super) struct StorageImportSessionReviewFilesystem {
    inner: Arc<StorageImportSessionTestFilesystem>,
    counters: Arc<StorageImportSessionReviewCounters>,
    pub(super) source_io_failure: AtomicBool,
    pub(super) cleanup_failures: AtomicUsize,
    pub(super) cleanup_gate: StorageImportSessionReviewCleanupGate,
}

impl StorageImportSessionFilesystemProvider for StorageImportSessionReviewFilesystem {
    fn create_spool(&self) -> Result<File, StorageImportSessionError> {
        self.inner.create_spool()
    }
    fn duplicate(&self, file: &File) -> std::io::Result<File> {
        self.inner.duplicate(file)
    }
    fn observe_space(
        &self,
        volume: StorageImportSessionEnumVolume,
    ) -> Result<StorageImportSessionSpace, StorageImportSessionError> {
        self.inner.observe_space(volume)
    }
    fn read_at(&self, file: &mut File, offset: u64, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.counters.reads.add(1)?;
        if self.source_io_failure.swap(false, Ordering::SeqCst) {
            return Err(std::io::Error::from(std::io::ErrorKind::StorageFull));
        }
        let count = self.inner.read_at(file, offset, bytes)?;
        self.counters
            .read_bytes
            .add(u64::try_from(count).map_err(std::io::Error::other)?)?;
        Ok(count)
    }
    fn write_at(&self, file: &mut File, offset: u64, bytes: &[u8]) -> std::io::Result<()> {
        self.counters.writes.add(1)?;
        self.counters
            .write_bytes
            .add(u64::try_from(bytes.len()).map_err(std::io::Error::other)?)?;
        self.inner.write_at(file, offset, bytes)
    }
    fn length(&self, file: &File) -> std::io::Result<u64> {
        self.inner.length(file)
    }
    fn flush(&self, file: &mut File) -> std::io::Result<()> {
        self.inner.flush(file)
    }
    fn release(&self, file: &mut File) -> std::io::Result<()> {
        self.inner.release(file)
    }
    fn cleanup_retained(&self) -> Result<(), StorageImportSessionError> {
        self.cleanup_gate.wait()?;
        if self
            .cleanup_failures
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into());
        }
        self.inner.cleanup_retained()
    }
}

pub(super) struct StorageImportSessionReviewRepository {
    inner: Arc<StorageImportSessionTestRepository>,
    counters: Arc<StorageImportSessionReviewCounters>,
    pub(super) gate: StorageImportSessionReviewReadGate,
    pub(super) estimate_token: Mutex<Option<StorageImportSessionCancellation>>,
}

impl StorageImportSessionRepository for StorageImportSessionReviewRepository {
    type PreparedProjection = Vec<u8>;
    fn snapshot(&self) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionSnapshot> {
        Box::pin(async move {
            self.gate
                .pass(StorageImportSessionReviewEnumRead::Snapshot)
                .await?;
            self.inner.snapshot().await
        })
    }
    fn candidates(
        &self,
        query: StorageImportSessionCandidateQuery,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCandidatePage> {
        Box::pin(async move {
            self.counters
                .candidate_pages
                .add(1)
                .map_err(|error| StorageImportSessionRepositoryError::Effect(Box::new(error)))?;
            self.gate
                .pass(StorageImportSessionReviewEnumRead::Candidates)
                .await?;
            self.inner.candidates(query).await
        })
    }
    fn read_candidate(
        &self,
        candidate: StorageImportSessionCandidate,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCandidateBody> {
        Box::pin(async move {
            self.counters
                .candidate_reads
                .add(1)
                .map_err(|error| StorageImportSessionRepositoryError::Effect(Box::new(error)))?;
            self.gate
                .pass(StorageImportSessionReviewEnumRead::CandidateBody)
                .await?;
            self.inner.read_candidate(candidate).await
        })
    }
    fn prepare_projection(
        &self,
        record: &StorageLibraryRecord,
        body: StorageImportSessionBodyProof<'_>,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Vec<u8>, StorageImportSessionRepositoryError> {
        self.counters
            .projections
            .add(1)
            .map_err(|error| StorageImportSessionRepositoryError::Effect(Box::new(error)))?;
        self.inner.prepare_projection(record, body, cancellation)
    }
    fn estimate_commit_space(
        &self,
        snapshot: StorageImportSessionSnapshot,
        footprint: StorageImportSessionCommitFootprint,
        cancellation: StorageImportSessionCancellation,
    ) -> StorageImportSessionRepositoryFuture<'_, u64> {
        Box::pin(async move {
            *self
                .estimate_token
                .lock()
                .map_err(|_poisoned| StorageImportSessionRepositoryError::InvalidResponse)? =
                Some(cancellation.clone());
            self.gate
                .pass(StorageImportSessionReviewEnumRead::Estimate)
                .await?;
            self.inner
                .estimate_commit_space(snapshot, footprint, cancellation)
                .await
        })
    }
    fn commit_import(
        &self,
        prepared: StorageImportSessionPreparedCommit,
        cancellation: StorageImportSessionCancellation,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCommitReceipt> {
        self.inner.commit_import(prepared, cancellation)
    }
}

pub(super) struct StorageImportSessionReviewFixture {
    base: StorageImportSessionTestFixture,
    pub(super) filesystem: Arc<StorageImportSessionReviewFilesystem>,
    pub(super) repository: Arc<StorageImportSessionReviewRepository>,
    pub(super) counters: Arc<StorageImportSessionReviewCounters>,
}

impl StorageImportSessionReviewFixture {
    pub(super) async fn new(has_candidate: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let mut base = StorageImportSessionTestFixture::new()?;
        if has_candidate {
            base.catalog_game("1. e4 *", 100).await?;
        }
        let counters = Arc::new(StorageImportSessionReviewCounters::new());
        Ok(Self {
            filesystem: Arc::new(StorageImportSessionReviewFilesystem {
                inner: Arc::clone(&base.filesystem),
                counters: Arc::clone(&counters),
                source_io_failure: AtomicBool::new(false),
                cleanup_failures: AtomicUsize::new(0),
                cleanup_gate: StorageImportSessionReviewCleanupGate::new(),
            }),
            repository: Arc::new(StorageImportSessionReviewRepository {
                inner: Arc::clone(&base.repository),
                counters: Arc::clone(&counters),
                gate: StorageImportSessionReviewReadGate::new(),
                estimate_token: Mutex::new(None),
            }),
            counters,
            base,
        })
    }

    pub(super) async fn session(
        &self,
        text: &str,
    ) -> Result<StorageImportSessionReviewSession, Box<dyn std::error::Error>> {
        use std::io::Write;
        let text = text.to_owned();
        let source = self
            .base
            .executor
            .submit_waiting(move |_token| -> std::io::Result<_> {
                let mut source = tempfile::tempfile()?;
                source.write_all(text.as_bytes())?;
                Ok(source)
            })
            .await?
            .join()
            .await??;
        Ok(StorageImportSession::new(
            source,
            StorageImportSessionResources {
                executor: Arc::clone(&self.base.executor),
                repository: Arc::clone(&self.repository),
                filesystem: Arc::clone(&self.filesystem),
                identities: Arc::new(StorageImportSessionTestIds(AtomicU64::new(1))),
            },
        )?)
    }

    pub(super) async fn finish(self) -> Result<(), Box<dyn std::error::Error>> {
        let Self {
            base,
            filesystem,
            repository,
            counters,
        } = self;
        let StorageImportSessionTestFixture {
            executor,
            repository: base_repository,
            filesystem: base_filesystem,
        } = base;
        executor
            .submit_waiting(move |_token| {
                drop((
                    repository,
                    filesystem,
                    counters,
                    base_repository,
                    base_filesystem,
                ));
            })
            .await?
            .join()
            .await?;
        let executor = Arc::try_unwrap(executor)
            .map_err(|_executor| "review executor still has an owner after drain")?;
        executor.shutdown().await?;
        Ok(())
    }
}

pub(super) struct StorageImportSessionReviewPollBudget;

impl StorageImportSessionReviewPollBudget {
    const POLLS: usize = 64;

    pub(super) async fn observe<F: Future>(mut future: Pin<&mut F>) -> Option<F::Output> {
        for _poll in 0..Self::POLLS {
            let result =
                std::future::poll_fn(|context| Poll::Ready(future.as_mut().poll(context))).await;
            if let Poll::Ready(result) = result {
                return Some(result);
            }
            tokio::task::yield_now().await;
        }
        None
    }

    pub(super) async fn reach<T>(
        future: impl Future<Output = Result<T, Arc<StorageImportSessionError>>>,
        entered: impl Future<Output = ()>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        tokio::pin!(future);
        tokio::select! {
            result = &mut future => match result { Ok(_value) => Err("operation completed before reaching its review barrier".into()), Err(error) => Err(error.into()) },
            () = entered => Ok(()),
        }
    }
}
