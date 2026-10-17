use std::collections::HashMap;
use std::future::{Future, poll_fn};
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::task::Poll;

use rand::Rng;
use tokio::sync::{Notify, mpsc, watch};
use tokio::time::{Instant, timeout_at};

use crate::clock::duration::ClockInstant;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::game_commands::{DesktopGameCommandsEnumAction, DesktopGameCommandsRequest};
use crate::desktop::game_registry::saved::DesktopGameRegistrySavedRequest;
use crate::desktop::game_registry::{
    DesktopGameRegistry, DesktopGameRegistryActor, DesktopGameRegistryError,
    DesktopGameRegistryFactoryProvider, DesktopGameRegistryPort,
};
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::game_supervisor::{
    DesktopGameSupervisorEngineFactory, DesktopGameSupervisorError, DesktopGameSupervisorEvent,
    DesktopGameSupervisorReceipt,
};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_commands::DesktopLibraryCommandsLifecycleProvider;
use crate::game::adjudication::GameAdjudicationMateProofProvider;
use crate::game::engine_provider::GameEngineProvider;
use crate::game::enums::{GameId, GameRevision};
use crate::game::storage_repository::{GameStorageReceipt, GameStorageRevision};
use crate::storage::trash::{
    StorageTrashClosedGameEvidence, StorageTrashEnumAppliedAction, StorageTrashReceipt,
};

pub struct DesktopGameRegistryCloseTarget {
    game: GameId,
    storage_revision: GameStorageRevision,
    owner: Option<(GameRevision, watch::Receiver<DesktopGameSupervisorEvent>)>,
}

impl DesktopGameRegistryCloseTarget {
    fn matches(&self, other: &Self) -> bool {
        self.game == other.game
            && self.storage_revision == other.storage_revision
            && match (&self.owner, &other.owner) {
                (Some((revision, owner)), Some((other_revision, other_owner))) => {
                    revision == other_revision && owner.same_channel(other_owner)
                }
                (None, None) => true,
                (Some(_), None) | (None, Some(_)) => false,
            }
    }
}

#[derive(Debug)]
pub enum DesktopGameRegistryCloseEnumProof {
    Closed {
        acknowledgment: GameStorageReceipt,
        snapshot: Box<DesktopGameSnapshot>,
    },
    NotOpen {
        game: GameId,
        expected_storage_revision: GameStorageRevision,
    },
}

impl DesktopGameRegistryCloseEnumProof {
    const fn game(&self) -> GameId {
        match self {
            Self::Closed { acknowledgment, .. } => acknowledgment.game(),
            Self::NotOpen { game, .. } => *game,
        }
    }

    const fn storage_revision(&self) -> GameStorageRevision {
        match self {
            Self::Closed { acknowledgment, .. } => acknowledgment.storage_revision(),
            Self::NotOpen {
                expected_storage_revision,
                ..
            } => *expected_storage_revision,
        }
    }
}

#[derive(Clone)]
enum DesktopGameRegistryCloseEnumStatus {
    Pending,
    Failed(Arc<DesktopGameRegistryError>),
    Certified(Arc<DesktopGameRegistryCloseEnumProof>),
    Released,
}

struct DesktopGameRegistryCloseShared {
    id: DesktopRequestId,
    target: DesktopGameRegistryCloseTarget,
    response: watch::Sender<DesktopGameRegistryCloseEnumStatus>,
    certificate_held: AtomicBool,
    retry: AtomicBool,
    release: AtomicBool,
    rejected: AtomicBool,
}

impl DesktopGameRegistryCloseShared {
    fn receipt(self: &Arc<Self>) -> DesktopGameRegistryCloseReceipt {
        DesktopGameRegistryCloseReceipt {
            operation: Arc::clone(self),
            response: self.response.subscribe(),
        }
    }

    fn fail(&self, error: DesktopGameRegistryError) {
        self.response
            .send_replace(DesktopGameRegistryCloseEnumStatus::Failed(Arc::new(error)));
    }
}

#[derive(Clone)]
pub struct DesktopGameRegistryCloseReceipt {
    operation: Arc<DesktopGameRegistryCloseShared>,
    response: watch::Receiver<DesktopGameRegistryCloseEnumStatus>,
}

impl DesktopGameRegistryCloseReceipt {
    pub fn operation_id(&self) -> DesktopRequestId {
        self.operation.id
    }

    pub async fn wait(
        &mut self,
    ) -> Result<DesktopGameRegistryCloseCertificate, Arc<DesktopGameRegistryError>> {
        loop {
            if self.operation.release.load(Ordering::Acquire) {
                return Err(Arc::new(DesktopGameRegistryError::CloseConflict));
            }
            match self.response.borrow().clone() {
                DesktopGameRegistryCloseEnumStatus::Certified(proof) => {
                    self.operation
                        .certificate_held
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .map_err(|_held| Arc::new(DesktopGameRegistryError::Busy))?;
                    return Ok(DesktopGameRegistryCloseCertificate {
                        operation: Arc::clone(&self.operation),
                        proof,
                    });
                }
                DesktopGameRegistryCloseEnumStatus::Failed(error) => return Err(error),
                DesktopGameRegistryCloseEnumStatus::Released => {
                    return Err(Arc::new(DesktopGameRegistryError::CloseConflict));
                }
                DesktopGameRegistryCloseEnumStatus::Pending => {}
            }
            self.response
                .changed()
                .await
                .map_err(|_closed| Arc::new(DesktopGameRegistryError::Closed))?;
        }
    }

    pub async fn wait_released(&mut self) -> Result<(), Arc<DesktopGameRegistryError>> {
        loop {
            if matches!(
                *self.response.borrow(),
                DesktopGameRegistryCloseEnumStatus::Released
            ) {
                return Ok(());
            }
            self.response
                .changed()
                .await
                .map_err(|_closed| Arc::new(DesktopGameRegistryError::Closed))?;
        }
    }
}

pub struct DesktopGameRegistryCloseCertificate {
    operation: Arc<DesktopGameRegistryCloseShared>,
    proof: Arc<DesktopGameRegistryCloseEnumProof>,
}

impl DesktopGameRegistryCloseCertificate {
    pub fn proof(&self) -> &DesktopGameRegistryCloseEnumProof {
        &self.proof
    }

    pub fn game(&self) -> GameId {
        self.proof.game()
    }

    pub fn storage_revision(&self) -> GameStorageRevision {
        self.proof.storage_revision()
    }

    pub fn closed_game_evidence(
        &self,
    ) -> Result<StorageTrashClosedGameEvidence, DesktopGameRegistryError> {
        if self.operation.release.load(Ordering::Acquire) {
            return Err(DesktopGameRegistryError::CloseConflict);
        }
        Ok(StorageTrashClosedGameEvidence::new(
            self.game(),
            self.storage_revision(),
        ))
    }
}

impl Drop for DesktopGameRegistryCloseCertificate {
    fn drop(&mut self) {
        self.operation
            .certificate_held
            .store(false, Ordering::Release);
    }
}

impl DesktopLibraryCommandsLifecycleProvider for DesktopGameRegistryCloseCertificate {
    type Error = DesktopGameRegistryError;

    fn closed_revision(&self, game: GameId) -> Result<Option<GameStorageRevision>, Self::Error> {
        if game != self.game() || self.operation.release.load(Ordering::Acquire) {
            return Err(DesktopGameRegistryError::CloseConflict);
        }
        Ok(Some(self.storage_revision()))
    }
}

pub enum DesktopGameRegistryCloseEnumObservation<'a> {
    TrashCommitted(&'a StorageTrashReceipt),
    RollbackObserved,
    CancelConfirmed,
}

#[derive(Default)]
pub(super) struct DesktopGameRegistryCloseAdmissionState {
    operations: HashMap<GameId, Arc<DesktopGameRegistryCloseShared>>,
}

impl DesktopGameRegistryCloseAdmissionState {
    pub(super) fn check_game(&self, game: GameId) -> Result<(), DesktopGameRegistryError> {
        if self.operations.contains_key(&game) {
            Err(DesktopGameRegistryError::Busy)
        } else {
            Ok(())
        }
    }
}

pub(super) struct DesktopGameRegistryCloseAdmission {
    state: Mutex<DesktopGameRegistryCloseAdmissionState>,
    pub(super) changed: Notify,
}

impl DesktopGameRegistryCloseAdmission {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(DesktopGameRegistryCloseAdmissionState::default()),
            changed: Notify::new(),
        }
    }

    pub(super) fn lock(
        &self,
    ) -> Result<MutexGuard<'_, DesktopGameRegistryCloseAdmissionState>, DesktopGameRegistryError>
    {
        self.state
            .try_lock()
            .map_err(|unavailable| match unavailable {
                TryLockError::WouldBlock => DesktopGameRegistryError::Busy,
                TryLockError::Poisoned(_) => DesktopGameRegistryError::ClosePoisoned,
            })
    }
}

#[derive(Clone)]
pub(super) struct DesktopGameRegistrySavedAdmission {
    sender: mpsc::Sender<DesktopGameRegistrySavedRequest>,
    admission: Arc<DesktopGameRegistryCloseAdmission>,
}

impl DesktopGameRegistrySavedAdmission {
    pub(super) const fn new(
        sender: mpsc::Sender<DesktopGameRegistrySavedRequest>,
        admission: Arc<DesktopGameRegistryCloseAdmission>,
    ) -> Self {
        Self { sender, admission }
    }

    pub(super) fn try_send(
        &self,
        request: DesktopGameRegistrySavedRequest,
    ) -> Result<(), mpsc::error::TrySendError<Box<DesktopGameRegistrySavedRequest>>> {
        let state = match self.admission.lock() {
            Ok(state) if !state.operations.contains_key(&request.game()) => state,
            Ok(_) | Err(DesktopGameRegistryError::Busy) => {
                return Err(mpsc::error::TrySendError::Full(Box::new(request)));
            }
            Err(error) => {
                request.complete(Err(Arc::new(error)));
                return Ok(());
            }
        };
        let result = self.sender.try_send(request).map_err(|error| match error {
            mpsc::error::TrySendError::Full(request) => {
                mpsc::error::TrySendError::Full(Box::new(request))
            }
            mpsc::error::TrySendError::Closed(request) => {
                mpsc::error::TrySendError::Closed(Box::new(request))
            }
        });
        drop(state);
        result
    }
}

impl DesktopGameRegistryPort {
    pub fn try_close_target(
        &self,
        game: GameId,
        expected_revision: GameRevision,
        expected_storage_revision: GameStorageRevision,
    ) -> Result<DesktopGameRegistryCloseTarget, DesktopGameRegistryError> {
        let admission = self.close_admission.lock()?;
        admission.check_game(game)?;
        let sessions = self
            .sessions
            .try_read()
            .map_err(|_busy| DesktopGameRegistryError::Busy)?;
        let owner = sessions
            .get(&game)
            .ok_or(DesktopGameRegistryError::UnknownGame)?;
        let event = owner.snapshot();
        if event.snapshot.revision() != expected_revision
            || event.storage_revision != expected_storage_revision
        {
            return Err(DesktopGameRegistryError::CloseConflict);
        }
        Ok(DesktopGameRegistryCloseTarget {
            game,
            storage_revision: expected_storage_revision,
            owner: Some((expected_revision, owner.events())),
        })
    }

    pub fn try_not_open_close_target(
        &self,
        game: GameId,
        expected_storage_revision: GameStorageRevision,
    ) -> Result<DesktopGameRegistryCloseTarget, DesktopGameRegistryError> {
        let admission = self.close_admission.lock()?;
        admission.check_game(game)?;
        if self
            .sessions
            .try_read()
            .map_err(|_busy| DesktopGameRegistryError::Busy)?
            .contains_key(&game)
        {
            return Err(DesktopGameRegistryError::CloseConflict);
        }
        Ok(DesktopGameRegistryCloseTarget {
            game,
            storage_revision: expected_storage_revision,
            owner: None,
        })
    }

    pub fn try_close_game(
        &self,
        target: DesktopGameRegistryCloseTarget,
    ) -> Result<DesktopGameRegistryCloseReceipt, DesktopGameRegistryError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DesktopGameRegistryError::Closed);
        }
        let mut admission = self.close_admission.lock()?;
        if self.closed.load(Ordering::Acquire) {
            return Err(DesktopGameRegistryError::Closed);
        }
        if let Some(existing) = admission.operations.get(&target.game) {
            return if existing.target.matches(&target) {
                Ok(existing.receipt())
            } else {
                Err(DesktopGameRegistryError::CloseConflict)
            };
        }
        if admission.operations.len() >= Self::CREATE_CAPACITY {
            return Err(DesktopGameRegistryError::Busy);
        }
        let (response, _receiver) = watch::channel(DesktopGameRegistryCloseEnumStatus::Pending);
        let shared = Arc::new(DesktopGameRegistryCloseShared {
            id: DesktopRequestId::new(),
            target,
            response,
            certificate_held: AtomicBool::new(false),
            retry: AtomicBool::new(false),
            release: AtomicBool::new(false),
            rejected: AtomicBool::new(false),
        });
        admission
            .operations
            .insert(shared.target.game, Arc::clone(&shared));
        self.close_admission.changed.notify_one();
        Ok(shared.receipt())
    }

    pub fn try_reconcile_close(
        &self,
        game: GameId,
        operation: DesktopRequestId,
    ) -> Result<DesktopGameRegistryCloseReceipt, DesktopGameRegistryError> {
        let admission = self.close_admission.lock()?;
        let shared = admission
            .operations
            .get(&game)
            .filter(|shared| shared.id == operation)
            .ok_or(DesktopGameRegistryError::CloseConflict)?;
        Ok(shared.receipt())
    }

    fn check_close_receipt(
        &self,
        receipt: &DesktopGameRegistryCloseReceipt,
    ) -> Result<(), DesktopGameRegistryError> {
        let admission = self.close_admission.lock()?;
        if admission
            .operations
            .get(&receipt.operation.target.game)
            .is_some_and(|operation| Arc::ptr_eq(operation, &receipt.operation))
        {
            Ok(())
        } else {
            Err(DesktopGameRegistryError::CloseConflict)
        }
    }

    pub fn try_retry_close(
        &self,
        receipt: &DesktopGameRegistryCloseReceipt,
    ) -> Result<(), DesktopGameRegistryError> {
        self.check_close_receipt(receipt)?;
        if receipt.operation.rejected.load(Ordering::Acquire)
            || !matches!(
                *receipt.operation.response.borrow(),
                DesktopGameRegistryCloseEnumStatus::Failed(_)
            )
        {
            return Err(DesktopGameRegistryError::CloseConflict);
        }
        receipt.operation.retry.store(true, Ordering::Release);
        receipt
            .operation
            .response
            .send_replace(DesktopGameRegistryCloseEnumStatus::Pending);
        self.close_admission.changed.notify_one();
        Ok(())
    }

    pub fn try_cancel_rejected_close(
        &self,
        receipt: &DesktopGameRegistryCloseReceipt,
    ) -> Result<(), DesktopGameRegistryError> {
        self.check_close_receipt(receipt)?;
        if !receipt.operation.rejected.load(Ordering::Acquire) {
            return Err(DesktopGameRegistryError::CloseIncomplete);
        }
        receipt.operation.release.store(true, Ordering::Release);
        self.close_admission.changed.notify_one();
        Ok(())
    }

    pub fn try_finish_close(
        &self,
        certificate: &DesktopGameRegistryCloseCertificate,
        observation: DesktopGameRegistryCloseEnumObservation<'_>,
    ) -> Result<(), DesktopGameRegistryError> {
        self.check_close_receipt(&certificate.operation.receipt())?;
        if let DesktopGameRegistryCloseEnumObservation::TrashCommitted(receipt) = observation {
            match receipt.action() {
                StorageTrashEnumAppliedAction::Trashed(game)
                | StorageTrashEnumAppliedAction::Deleted(game)
                    if game == certificate.game() => {}
                StorageTrashEnumAppliedAction::Trashed(_)
                | StorageTrashEnumAppliedAction::Deleted(_)
                | StorageTrashEnumAppliedAction::Restored(_)
                | StorageTrashEnumAppliedAction::RemovedFolder(_) => {
                    return Err(DesktopGameRegistryError::CloseConflict);
                }
            }
        }
        certificate.operation.release.store(true, Ordering::Release);
        self.close_admission.changed.notify_one();
        Ok(())
    }
}

type DesktopGameRegistryCloseCommandFuture = Pin<
    Box<dyn Future<Output = Result<DesktopGameSnapshot, Arc<DesktopGameSupervisorError>>> + Send>,
>;
type DesktopGameRegistryCloseCleanupFuture<F, C, P> = Pin<
    Box<
        dyn Future<
                Output = (
                    DesktopGameRegistryActor<F, C, P>,
                    Result<(), DesktopGameSupervisorError>,
                ),
            > + Send,
    >,
>;

enum DesktopGameRegistryCloseEnumPhase<F: DesktopGameRegistryFactoryProvider, C, P> {
    Queued,
    Command {
        future: DesktopGameRegistryCloseCommandFuture,
        retry_save: bool,
    },
    WaitingActor,
    SaveFailure(Arc<DesktopGameSupervisorError>),
    Cleanup(DesktopGameRegistryCloseCleanupFuture<F, C, P>),
    Failed,
    Closed,
}

pub(super) struct DesktopGameRegistryCloseOperation<F: DesktopGameRegistryFactoryProvider, C, P> {
    shared: Arc<DesktopGameRegistryCloseShared>,
    phase: DesktopGameRegistryCloseEnumPhase<F, C, P>,
    actor: Option<DesktopGameRegistryActor<F, C, P>>,
    actor_failure: Option<Arc<DesktopGameSupervisorError>>,
    retry_save: bool,
    received_at: Option<ClockInstant>,
    continuation: Option<GameRevision>,
}

pub(super) struct DesktopGameRegistryClosing<F: DesktopGameRegistryFactoryProvider, C, P> {
    pub(super) admission: Arc<DesktopGameRegistryCloseAdmission>,
    pub(super) operations: HashMap<GameId, DesktopGameRegistryCloseOperation<F, C, P>>,
}

impl<F: DesktopGameRegistryFactoryProvider, C, P> DesktopGameRegistryClosing<F, C, P> {
    pub(super) fn new(admission: Arc<DesktopGameRegistryCloseAdmission>) -> Self {
        Self {
            admission,
            operations: HashMap::new(),
        }
    }

    pub(super) fn is_closed(&self, game: GameId) -> bool {
        self.operations.get(&game).is_some_and(|operation| {
            matches!(operation.phase, DesktopGameRegistryCloseEnumPhase::Closed)
        })
    }

    pub(super) fn accepts_actor(
        &self,
        game: GameId,
        owner: &watch::Receiver<DesktopGameSupervisorEvent>,
    ) -> bool {
        self.operations.get(&game).is_some_and(|operation| {
            !operation.shared.rejected.load(Ordering::Acquire)
                && operation
                    .shared
                    .target
                    .owner
                    .as_ref()
                    .is_some_and(|(_, expected)| expected.same_channel(owner))
        })
    }

    pub(super) fn is_shutdown_complete(&self) -> bool {
        let Ok(admission) = self.admission.lock() else {
            return false;
        };
        admission.operations.len() == self.operations.len()
            && self.operations.values().all(|operation| {
                operation.actor.is_none()
                    && matches!(
                        operation.phase,
                        DesktopGameRegistryCloseEnumPhase::Closed
                            | DesktopGameRegistryCloseEnumPhase::Failed
                    )
            })
    }
}

pub(super) enum DesktopGameRegistryCloseEnumReady<F: DesktopGameRegistryFactoryProvider, C, P> {
    Command {
        game: GameId,
        retry_save: bool,
        result: Box<Result<DesktopGameSnapshot, Arc<DesktopGameSupervisorError>>>,
    },
    Cleanup {
        game: GameId,
        actor: Box<DesktopGameRegistryActor<F, C, P>>,
        result: Result<(), DesktopGameSupervisorError>,
    },
}

impl<F, C, P, R> DesktopGameRegistry<F, C, P, R>
where
    F: DesktopGameRegistryFactoryProvider,
    F::Factory: 'static,
    <F::Factory as DesktopGameSupervisorEngineFactory>::Engine: 'static,
    <<F::Factory as DesktopGameSupervisorEngineFactory>::Engine as GameEngineProvider>::Error:
        'static,
    C: ClockTimeProvider + RefUnwindSafe + 'static,
    P: GameAdjudicationMateProofProvider + Send + Sync + RefUnwindSafe + 'static,
    R: Rng + Send + UnwindSafe + 'static,
{
    pub(super) fn progress_closes(&mut self) -> Result<(), DesktopGameRegistryError> {
        match self.try_progress_closes() {
            Err(DesktopGameRegistryError::Busy) => {
                self.closing.admission.changed.notify_one();
                Ok(())
            }
            result => result,
        }
    }

    fn try_progress_closes(&mut self) -> Result<(), DesktopGameRegistryError> {
        let admission = self.closing.admission.lock()?;
        for (&game, shared) in &admission.operations {
            self.closing.operations.entry(game).or_insert_with(|| {
                DesktopGameRegistryCloseOperation {
                    shared: Arc::clone(shared),
                    phase: DesktopGameRegistryCloseEnumPhase::Queued,
                    actor: None,
                    actor_failure: None,
                    retry_save: false,
                    received_at: None,
                    continuation: None,
                }
            });
        }
        drop(admission);
        let games: Vec<_> = self.closing.operations.keys().copied().collect();
        for game in games {
            if self
                .creating
                .as_ref()
                .is_some_and(|creating| creating.game == game)
                || self
                    .installing
                    .iter()
                    .any(|(_, port)| port.snapshot().snapshot.id() == game)
                || self
                    .activating
                    .as_ref()
                    .is_some_and(|activating| activating.game == game)
                || self
                    .saved
                    .as_ref()
                    .is_some_and(|saved| saved.game() == game)
            {
                continue;
            }
            self.progress_close(game)?;
        }
        Ok(())
    }

    fn progress_close(&mut self, game: GameId) -> Result<(), DesktopGameRegistryError> {
        let operation = self
            .closing
            .operations
            .get_mut(&game)
            .ok_or(DesktopGameRegistryError::CloseConflict)?;
        if operation.shared.release.load(Ordering::Acquire) {
            let mut admission = self.closing.admission.lock()?;
            let mut operation = self
                .closing
                .operations
                .remove(&game)
                .ok_or(DesktopGameRegistryError::CloseConflict)?;
            if let Some(actor) = operation.actor.take()
                && !actor.is_shutdown_complete()
            {
                let (_, owner) = operation
                    .shared
                    .target
                    .owner
                    .as_ref()
                    .ok_or(DesktopGameRegistryError::CloseConflict)?;
                self.retained
                    .push(crate::desktop::game_registry::DesktopGameRegistryRetained {
                        game,
                        owner: owner.clone(),
                        actor,
                    });
            }
            admission.operations.remove(&game);
            operation
                .shared
                .response
                .send_replace(DesktopGameRegistryCloseEnumStatus::Released);
            return Ok(());
        }
        if let DesktopGameRegistryCloseEnumPhase::SaveFailure(error) = &operation.phase {
            let (_, owner) = operation
                .shared
                .target
                .owner
                .as_ref()
                .ok_or(DesktopGameRegistryError::CloseConflict)?;
            if owner.borrow().snapshot.is_save_required() {
                operation
                    .shared
                    .fail(DesktopGameRegistryError::Supervisor(Arc::clone(error)));
                operation.phase = DesktopGameRegistryCloseEnumPhase::Failed;
            }
            return Ok(());
        }
        if matches!(operation.phase, DesktopGameRegistryCloseEnumPhase::Failed)
            && operation.shared.retry.swap(false, Ordering::AcqRel)
        {
            if let Some(mut actor) = operation.actor.take() {
                if actor.is_shutdown_complete() {
                    operation.actor = Some(actor);
                    operation.actor_failure = None;
                    operation.phase = DesktopGameRegistryCloseEnumPhase::WaitingActor;
                    return self.certify_close(game);
                }
                operation.phase =
                    DesktopGameRegistryCloseEnumPhase::Cleanup(Box::pin(async move {
                        let result = actor
                            .shutdown_retained(Instant::now() + Self::CONTROL_TIMEOUT)
                            .await;
                        (actor, result)
                    }));
            } else {
                let retry_save = operation.retry_save;
                self.submit_close(game, retry_save)?;
            }
            return Ok(());
        }
        if matches!(
            operation.phase,
            DesktopGameRegistryCloseEnumPhase::WaitingActor
        ) && operation.actor.is_some()
        {
            return self.certify_close(game);
        }
        if !matches!(operation.phase, DesktopGameRegistryCloseEnumPhase::Queued) {
            return Ok(());
        }
        let sessions = self
            .sessions
            .try_read()
            .map_err(|_busy| DesktopGameRegistryError::Busy)?;
        let valid = match (&operation.shared.target.owner, sessions.get(&game)) {
            (Some((revision, owner)), Some(current)) => {
                let event = current.snapshot();
                owner.same_channel(&current.events())
                    && *revision == event.snapshot.revision()
                    && operation.shared.target.storage_revision == event.storage_revision
            }
            (None, None) => !self.retained.iter().any(|retained| retained.game == game),
            (Some(_), None) | (None, Some(_)) => false,
        };
        drop(sessions);
        if !valid {
            operation.shared.rejected.store(true, Ordering::Release);
            operation
                .shared
                .fail(DesktopGameRegistryError::CloseConflict);
            operation.phase = DesktopGameRegistryCloseEnumPhase::Failed;
            return Ok(());
        }
        if operation.shared.target.owner.is_none() {
            operation
                .shared
                .response
                .send_replace(DesktopGameRegistryCloseEnumStatus::Certified(Arc::new(
                    DesktopGameRegistryCloseEnumProof::NotOpen {
                        game,
                        expected_storage_revision: operation.shared.target.storage_revision,
                    },
                )));
            operation.phase = DesktopGameRegistryCloseEnumPhase::Closed;
            return Ok(());
        }
        if let Some(index) = self.retained.iter().position(|retained| {
            retained.game == game
                && operation
                    .shared
                    .target
                    .owner
                    .as_ref()
                    .is_some_and(|(_, owner)| owner.same_channel(&retained.owner))
        }) {
            operation.actor = Some(self.retained.swap_remove(index).actor);
            if operation
                .actor
                .as_ref()
                .is_some_and(|actor| actor.is_shutdown_complete())
            {
                operation.phase = DesktopGameRegistryCloseEnumPhase::WaitingActor;
                return self.certify_close(game);
            }
            operation.phase = DesktopGameRegistryCloseEnumPhase::Failed;
            operation
                .shared
                .fail(DesktopGameRegistryError::CloseIncomplete);
            return Ok(());
        }
        self.submit_close(game, false)
    }

    fn submit_close(
        &mut self,
        game: GameId,
        retry_save: bool,
    ) -> Result<(), DesktopGameRegistryError> {
        let operation = self
            .closing
            .operations
            .get_mut(&game)
            .ok_or(DesktopGameRegistryError::CloseConflict)?;
        let result = (|| {
            let sessions = self
                .sessions
                .try_read()
                .map_err(|_busy| DesktopGameRegistryError::Busy)?;
            let port = sessions
                .get(&game)
                .ok_or(DesktopGameRegistryError::UnknownGame)?;
            let (_, owner) = operation
                .shared
                .target
                .owner
                .as_ref()
                .ok_or(DesktopGameRegistryError::CloseConflict)?;
            if !owner.same_channel(&port.events()) {
                return Err(DesktopGameRegistryError::CloseConflict);
            }
            let received_at = if retry_save {
                self.clock.now()?
            } else if operation.continuation.is_some() {
                owner.borrow().snapshot.captured_at()
            } else {
                match operation.received_at {
                    Some(received_at) => received_at,
                    None => {
                        let received_at = self.clock.now()?;
                        operation.received_at = Some(received_at);
                        received_at
                    }
                }
            };
            let expected_revision = if retry_save {
                port.snapshot().snapshot.revision()
            } else {
                operation.continuation.unwrap_or(
                    operation
                        .shared
                        .target
                        .owner
                        .as_ref()
                        .ok_or(DesktopGameRegistryError::CloseConflict)?
                        .0,
                )
            };
            port.try_submit(DesktopGameCommandsRequest {
                request_id: operation.shared.id,
                expected_revision,
                received_at,
                action: if retry_save {
                    DesktopGameCommandsEnumAction::RetrySave
                } else {
                    DesktopGameCommandsEnumAction::Quit
                },
            })
            .map_err(|error| DesktopGameRegistryError::Supervisor(Arc::new(error)))
        })();
        match result {
            Ok(receipt) => {
                operation.phase = DesktopGameRegistryCloseEnumPhase::Command {
                    future: Self::close_command(receipt),
                    retry_save,
                }
            }
            Err(DesktopGameRegistryError::Supervisor(error))
                if matches!(error.as_ref(), DesktopGameSupervisorError::Closed) =>
            {
                operation.phase = DesktopGameRegistryCloseEnumPhase::WaitingActor;
            }
            Err(error) => {
                operation.retry_save = retry_save;
                if matches!(error, DesktopGameRegistryError::CloseConflict) {
                    operation.shared.rejected.store(true, Ordering::Release);
                }
                operation.shared.fail(error);
                operation.phase = DesktopGameRegistryCloseEnumPhase::Failed;
            }
        }
        Ok(())
    }

    fn close_command(
        receipt: DesktopGameSupervisorReceipt,
    ) -> DesktopGameRegistryCloseCommandFuture {
        Box::pin(receipt.wait())
    }

    pub(super) async fn close_ready(
        closing: &mut DesktopGameRegistryClosing<F, C, P>,
    ) -> DesktopGameRegistryCloseEnumReady<F, C, P> {
        poll_fn(|context| {
            for (&game, operation) in &mut closing.operations {
                let ready = match &mut operation.phase {
                    DesktopGameRegistryCloseEnumPhase::Command { future, retry_save } => {
                        match future.as_mut().poll(context) {
                            Poll::Ready(result) => {
                                Some(DesktopGameRegistryCloseEnumReady::Command {
                                    game,
                                    retry_save: *retry_save,
                                    result: Box::new(result),
                                })
                            }
                            Poll::Pending => None,
                        }
                    }
                    DesktopGameRegistryCloseEnumPhase::Cleanup(future) => {
                        match future.as_mut().poll(context) {
                            Poll::Ready((actor, result)) => {
                                Some(DesktopGameRegistryCloseEnumReady::Cleanup {
                                    game,
                                    actor: Box::new(actor),
                                    result,
                                })
                            }
                            Poll::Pending => None,
                        }
                    }
                    DesktopGameRegistryCloseEnumPhase::Queued
                    | DesktopGameRegistryCloseEnumPhase::WaitingActor
                    | DesktopGameRegistryCloseEnumPhase::SaveFailure(_)
                    | DesktopGameRegistryCloseEnumPhase::Failed
                    | DesktopGameRegistryCloseEnumPhase::Closed => None,
                };
                if let Some(ready) = ready {
                    return Poll::Ready(ready);
                }
            }
            Poll::Pending
        })
        .await
    }

    pub(super) fn finish_close_step(
        &mut self,
        ready: DesktopGameRegistryCloseEnumReady<F, C, P>,
    ) -> Result<(), DesktopGameRegistryError> {
        match ready {
            DesktopGameRegistryCloseEnumReady::Command {
                game,
                retry_save,
                result,
            } => {
                let operation = self
                    .closing
                    .operations
                    .get_mut(&game)
                    .ok_or(DesktopGameRegistryError::CloseConflict)?;
                match *result {
                    Ok(snapshot) if retry_save => {
                        operation.continuation = Some(snapshot.revision());
                        self.submit_close(game, false)?;
                    }
                    Ok(_) => operation.phase = DesktopGameRegistryCloseEnumPhase::WaitingActor,
                    Err(error) => {
                        if matches!(error.as_ref(), DesktopGameSupervisorError::Storage(_)) {
                            operation.retry_save = true;
                            operation.phase = DesktopGameRegistryCloseEnumPhase::SaveFailure(error);
                            return Ok(());
                        }
                        if matches!(error.as_ref(), DesktopGameSupervisorError::Revision(_)) {
                            operation.shared.rejected.store(true, Ordering::Release);
                        }
                        operation.retry_save = operation
                            .shared
                            .target
                            .owner
                            .as_ref()
                            .is_some_and(|(_, owner)| owner.borrow().snapshot.is_save_required());
                        operation
                            .shared
                            .fail(DesktopGameRegistryError::Supervisor(error));
                        operation.phase = DesktopGameRegistryCloseEnumPhase::Failed;
                    }
                }
            }
            DesktopGameRegistryCloseEnumReady::Cleanup {
                game,
                actor,
                result,
            } => {
                self.observe_close_actor(game, *actor, result)?;
            }
        }
        Ok(())
    }

    pub(super) fn observe_close_actor(
        &mut self,
        game: GameId,
        actor: DesktopGameRegistryActor<F, C, P>,
        result: Result<(), DesktopGameSupervisorError>,
    ) -> Result<(), DesktopGameRegistryError> {
        let operation = self
            .closing
            .operations
            .get_mut(&game)
            .ok_or(DesktopGameRegistryError::CloseConflict)?;
        operation.actor = Some(actor);
        if let Err(error) = result {
            let error = Arc::new(error);
            operation.actor_failure = Some(Arc::clone(&error));
            if !matches!(
                operation.phase,
                DesktopGameRegistryCloseEnumPhase::Command { .. }
            ) {
                operation
                    .shared
                    .fail(DesktopGameRegistryError::Supervisor(error));
                operation.phase = DesktopGameRegistryCloseEnumPhase::Failed;
            }
        } else if matches!(
            operation.phase,
            DesktopGameRegistryCloseEnumPhase::Cleanup(_)
        ) {
            operation.actor_failure = None;
            operation.phase = DesktopGameRegistryCloseEnumPhase::WaitingActor;
        }
        Ok(())
    }

    fn certify_close(&mut self, game: GameId) -> Result<(), DesktopGameRegistryError> {
        let operation = self
            .closing
            .operations
            .get_mut(&game)
            .ok_or(DesktopGameRegistryError::CloseConflict)?;
        let actor = operation
            .actor
            .as_ref()
            .ok_or(DesktopGameRegistryError::CloseIncomplete)?;
        if let Some(error) = &operation.actor_failure {
            operation
                .shared
                .fail(DesktopGameRegistryError::Supervisor(Arc::clone(error)));
            operation.phase = DesktopGameRegistryCloseEnumPhase::Failed;
            return Ok(());
        }
        if !actor.is_shutdown_complete() {
            operation
                .shared
                .fail(DesktopGameRegistryError::CloseIncomplete);
            operation.phase = DesktopGameRegistryCloseEnumPhase::Failed;
            return Ok(());
        }
        let Some(acknowledgment) = actor.last_storage_acknowledgment() else {
            operation
                .shared
                .fail(DesktopGameRegistryError::CloseIncomplete);
            operation.phase = DesktopGameRegistryCloseEnumPhase::Failed;
            return Ok(());
        };
        let (_, owner) = operation
            .shared
            .target
            .owner
            .as_ref()
            .ok_or(DesktopGameRegistryError::CloseConflict)?;
        let event = owner.borrow().clone();
        if acknowledgment.game() != game
            || acknowledgment.game_revision() != event.snapshot.revision()
            || acknowledgment.storage_revision() != event.storage_revision
            || event.snapshot.is_save_required()
        {
            operation
                .shared
                .fail(DesktopGameRegistryError::CloseIncomplete);
            operation.phase = DesktopGameRegistryCloseEnumPhase::Failed;
            return Ok(());
        }
        let mut sessions = self
            .sessions
            .try_write()
            .map_err(|_busy| DesktopGameRegistryError::Busy)?;
        if !sessions
            .get(&game)
            .is_some_and(|current| owner.same_channel(&current.events()))
        {
            operation
                .shared
                .fail(DesktopGameRegistryError::CloseConflict);
            operation.phase = DesktopGameRegistryCloseEnumPhase::Failed;
            return Ok(());
        }
        sessions.remove(&game);
        drop(sessions);
        self.saved_sources.remove(&game);
        self.watches.retain(|(watched, _)| *watched != game);
        operation.actor = None;
        operation.phase = DesktopGameRegistryCloseEnumPhase::Closed;
        let mut snapshots = self.snapshots.borrow().clone();
        snapshots.retain(|snapshot| snapshot.id() != game);
        self.snapshots.send_replace(snapshots);
        operation
            .shared
            .response
            .send_replace(DesktopGameRegistryCloseEnumStatus::Certified(Arc::new(
                DesktopGameRegistryCloseEnumProof::Closed {
                    acknowledgment,
                    snapshot: Box::new(event.snapshot),
                },
            )));
        Ok(())
    }

    pub(super) async fn drain_closes(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopGameRegistryError> {
        self.progress_closes()?;
        while self.closing.operations.values().any(|operation| {
            matches!(
                operation.phase,
                DesktopGameRegistryCloseEnumPhase::Command { .. }
                    | DesktopGameRegistryCloseEnumPhase::Cleanup(_)
            )
        }) {
            let ready = timeout_at(deadline, Self::close_ready(&mut self.closing))
                .await
                .map_err(DesktopGameRegistryError::Deadline)?;
            self.finish_close_step(ready)?;
            self.progress_closes()?;
        }
        if self.closing.is_shutdown_complete() {
            Ok(())
        } else {
            Err(DesktopGameRegistryError::CloseIncomplete)
        }
    }
}
