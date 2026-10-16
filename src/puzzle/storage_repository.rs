use std::future::Future;

use thiserror::Error;

use crate::clock::duration::ClockInstant;
use crate::puzzle::checkpoint::{
    PuzzleCheckpoint, PuzzleCheckpointEnumAdvance, PuzzleCheckpointEnumCompletion,
    PuzzleCheckpointEnumFailure, PuzzleCheckpointEnumTrigger, PuzzleCheckpointError,
    PuzzleCheckpointObservation, PuzzleCheckpointSnapshot,
};
use crate::puzzle::persistence::{
    PuzzlePersistenceError, PuzzlePersistenceJournalProof, PuzzlePersistenceReceipt,
    PuzzlePersistenceSaveRequest, PuzzlePersistenceSnapshot, PuzzlePersistenceStorageRevision,
};

pub trait PuzzleStorageWrite: Send {
    type Error: std::error::Error + Send + Sync + 'static;

    fn wait(
        &mut self,
    ) -> impl Future<Output = Result<PuzzlePersistenceReceipt, Self::Error>> + Send;
}

pub trait PuzzleStorageRepository: Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    type Write: PuzzleStorageWrite<Error = Self::Error>;

    fn begin_save(&self, request: PuzzlePersistenceSaveRequest)
    -> Result<Self::Write, Self::Error>;
}

#[derive(Debug, Error)]
pub enum PuzzleStorageDriverError<E: std::error::Error + 'static> {
    #[error(transparent)]
    Persistence(#[from] PuzzlePersistenceError),
    #[error(transparent)]
    Checkpoint(#[from] PuzzleCheckpointError),
    #[error("A puzzle database write is already in flight")]
    InFlight,
    #[error("No puzzle database write is in flight")]
    NoWrite,
    #[error("The puzzle database write has not completed")]
    CompletionPending,
    #[error("Puzzle persistence failed: {0}")]
    Repository(#[source] E),
}

pub struct PuzzleStorageDriver<R: PuzzleStorageRepository> {
    repository: R,
    checkpoint: PuzzleCheckpoint<PuzzlePersistenceSnapshot>,
    request: Option<PuzzlePersistenceSaveRequest>,
    in_flight: Option<R::Write>,
    committed: Option<PuzzlePersistenceReceipt>,
    failure: Option<R::Error>,
    storage_revision: PuzzlePersistenceStorageRevision,
    persisted_event_count: usize,
    persisted_journal_proof: PuzzlePersistenceJournalProof,
}

impl<R: PuzzleStorageRepository> PuzzleStorageDriver<R> {
    pub fn new(
        repository: R,
        initial: PuzzleCheckpointSnapshot<PuzzlePersistenceSnapshot>,
        storage_revision: PuzzlePersistenceStorageRevision,
        persisted_event_count: usize,
    ) -> Result<Self, PuzzleStorageDriverError<R::Error>> {
        Self::validate_snapshot(&initial)?;
        if initial.payload().base_event_count() > persisted_event_count
            || initial.payload().event_count() < persisted_event_count
            || (storage_revision == PuzzlePersistenceStorageRevision::INITIAL
                && persisted_event_count != 0)
        {
            return Err(PuzzlePersistenceError::InvalidPrefix.into());
        }
        let persisted_journal_proof = initial.payload().journal_proof_at(persisted_event_count)?;
        Ok(Self {
            repository,
            checkpoint: PuzzleCheckpoint::new(initial)?,
            request: None,
            in_flight: None,
            committed: None,
            failure: None,
            storage_revision,
            persisted_event_count,
            persisted_journal_proof,
        })
    }

    pub const fn checkpoint(&self) -> &PuzzleCheckpoint<PuzzlePersistenceSnapshot> {
        &self.checkpoint
    }
    pub const fn storage_revision(&self) -> PuzzlePersistenceStorageRevision {
        self.storage_revision
    }
    pub const fn persisted_event_count(&self) -> usize {
        self.persisted_event_count
    }

    pub fn before_advance(
        &mut self,
        now: ClockInstant,
    ) -> Result<PuzzleCheckpointEnumAdvance, PuzzleStorageDriverError<R::Error>> {
        Ok(self.checkpoint.before_advance(now)?)
    }

    pub fn needs_snapshot(
        &self,
        observation: &PuzzleCheckpointObservation,
        trigger: PuzzleCheckpointEnumTrigger,
    ) -> Result<bool, PuzzleStorageDriverError<R::Error>> {
        Ok(self.checkpoint.needs_snapshot(observation, trigger)?)
    }

    pub fn record(
        &mut self,
        snapshot: PuzzleCheckpointSnapshot<PuzzlePersistenceSnapshot>,
        trigger: PuzzleCheckpointEnumTrigger,
    ) -> Result<(), PuzzleStorageDriverError<R::Error>> {
        Self::validate_snapshot(&snapshot)?;
        if snapshot
            .payload()
            .journal_proof_at(self.persisted_event_count)?
            != self.persisted_journal_proof
        {
            return Err(PuzzlePersistenceError::InvalidPrefix.into());
        }
        self.checkpoint.record(snapshot, trigger)?;
        Ok(())
    }

    pub fn begin_write(
        &mut self,
        now: ClockInstant,
    ) -> Result<(), PuzzleStorageDriverError<R::Error>> {
        self.ensure_idle()?;
        let snapshot = self
            .checkpoint
            .pending()
            .ok_or(PuzzleCheckpointError::NoQueuedWrite)?
            .payload()
            .rebase(self.persisted_event_count, self.persisted_journal_proof)?;
        let token = self.checkpoint.begin_write(now)?;
        let request = PuzzlePersistenceSaveRequest::new(snapshot, self.storage_revision, token)?;
        self.start_request(request, now)
    }

    pub fn retry_save(
        &mut self,
        now: ClockInstant,
    ) -> Result<(), PuzzleStorageDriverError<R::Error>> {
        self.ensure_idle()?;
        let request = self
            .request
            .as_ref()
            .ok_or(PuzzleCheckpointError::RetryRequired)?
            .clone();
        let token = self.checkpoint.retry_save(now)?;
        let request = PuzzlePersistenceSaveRequest::new(
            request.snapshot().clone(),
            request.expected_storage_revision(),
            token,
        )?;
        self.start_request(request, now)
    }

    fn validate_snapshot(
        snapshot: &PuzzleCheckpointSnapshot<PuzzlePersistenceSnapshot>,
    ) -> Result<(), PuzzleStorageDriverError<R::Error>> {
        if snapshot.payload().context() != snapshot.observation().context()
            || snapshot.payload().elapsed_nanos()
                != snapshot.observation().elapsed_active().as_nanos()
        {
            return Err(PuzzlePersistenceError::ReplayMismatch.into());
        }
        Ok(())
    }

    pub fn pending_request(&self) -> Option<&PuzzlePersistenceSaveRequest> {
        self.request.as_ref()
    }

    pub fn failure(&self) -> Option<&R::Error> {
        self.failure.as_ref()
    }

    fn ensure_idle(&self) -> Result<(), PuzzleStorageDriverError<R::Error>> {
        if self.in_flight.is_some() || self.committed.is_some() || self.failure.is_some() {
            return Err(PuzzleStorageDriverError::InFlight);
        }
        Ok(())
    }

    fn start_request(
        &mut self,
        request: PuzzlePersistenceSaveRequest,
        now: ClockInstant,
    ) -> Result<(), PuzzleStorageDriverError<R::Error>> {
        self.request = Some(request.clone());
        match self.repository.begin_save(request) {
            Ok(write) => {
                self.in_flight = Some(write);
                Ok(())
            }
            Err(error) => {
                self.failure = Some(error);
                self.settle_failure(now).map(|_| ())
            }
        }
    }

    pub async fn wait(&mut self) -> Result<(), PuzzleStorageDriverError<R::Error>> {
        if self.failure.is_some() || self.committed.is_some() {
            return Ok(());
        }
        let outcome = self
            .in_flight
            .as_mut()
            .ok_or(PuzzleStorageDriverError::NoWrite)?
            .wait()
            .await;
        self.in_flight = None;
        match outcome {
            Ok(receipt) => self.committed = Some(receipt),
            Err(error) => self.failure = Some(error),
        }
        Ok(())
    }

    pub fn settle(
        &mut self,
        now: ClockInstant,
    ) -> Result<PuzzleCheckpointEnumAdvance, PuzzleStorageDriverError<R::Error>> {
        if self.in_flight.is_some() {
            return Err(PuzzleStorageDriverError::CompletionPending);
        }
        if self.failure.is_some() {
            return self.settle_failure(now);
        }
        let receipt = self
            .committed
            .as_ref()
            .ok_or(PuzzleStorageDriverError::NoWrite)?;
        let advance = receipt.acknowledge(&mut self.checkpoint, now)?;
        self.storage_revision = receipt.storage_revision();
        self.persisted_event_count = receipt.event_count();
        self.persisted_journal_proof = receipt.journal_proof();
        self.committed = None;
        self.request = None;
        Ok(advance)
    }

    fn settle_failure(
        &mut self,
        now: ClockInstant,
    ) -> Result<PuzzleCheckpointEnumAdvance, PuzzleStorageDriverError<R::Error>> {
        self.fail(now)?;
        let error = self
            .failure
            .take()
            .ok_or(PuzzleStorageDriverError::NoWrite)?;
        Err(PuzzleStorageDriverError::Repository(error))
    }

    fn fail(&mut self, now: ClockInstant) -> Result<(), PuzzleStorageDriverError<R::Error>> {
        let request = self
            .request
            .as_ref()
            .ok_or(PuzzleStorageDriverError::NoWrite)?;
        self.checkpoint.complete(
            now,
            request.token(),
            PuzzleCheckpointEnumCompletion::Failed(PuzzleCheckpointEnumFailure::WriteFailed),
        )?;
        Ok(())
    }

    pub fn resume(&mut self, now: ClockInstant) -> Result<(), PuzzleStorageDriverError<R::Error>> {
        self.checkpoint.resume(now)?;
        Ok(())
    }
}
