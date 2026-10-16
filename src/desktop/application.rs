use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};

use thiserror::Error;
use tokio::sync::Notify;

use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::dto::{DesktopEnumEvent, DesktopEnumValue};
use crate::desktop::error::DesktopFailure;
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};
use crate::desktop::messages::{
    DesktopCommand, DesktopMessagesEntityId, DesktopMessagesEnumControlKey,
    DesktopMessagesEnumResult, DesktopRequest,
};
use crate::desktop::request_cache::DesktopRequestCache;
use crate::desktop::stream::{DesktopStream, DesktopStreamEvent};

#[derive(Debug, Error)]
pub enum DesktopApplicationWakeError {
    #[error("The native event loop is unavailable")]
    Unavailable,
    #[error("The native event-loop wake failed: {source}")]
    Native {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

pub trait DesktopApplicationWakeProvider: Send + Sync {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError>;
}

impl<W: DesktopApplicationWakeProvider> DesktopApplicationWakeProvider for Arc<W> {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        self.as_ref().wake()
    }
}

#[derive(Debug, Error)]
pub enum DesktopApplicationError {
    #[error("The application cannot admit another request right now")]
    Busy,
    #[error("The native event queue is full and requires resynchronization")]
    EventSaturated,
    #[error("An unapplied mutation or control already belongs to this entity")]
    EntityBusy,
    #[error("No admitted work belongs to this entity")]
    UnknownEntity,
    #[error("This action belongs to another application session")]
    WrongSession,
    #[error("This action must reconcile with an authoritative snapshot before retrying")]
    ResyncRequired,
    #[error("The application is closing and cannot admit new ordinary work")]
    Closing,
    #[error("The native action sequence is exhausted")]
    SequenceOverflow,
    #[error("Mutations require an expected entity revision")]
    MissingRevision,
    #[error("Monotonic application time moved backwards")]
    TimeOrder,
    #[error("Application ownership state is poisoned")]
    Poisoned,
    #[error("The work ticket is unavailable or is no longer active")]
    UnknownWork,
    #[error("This work has already completed")]
    AlreadyCompleted,
    #[error("Cancellation was requested before the commit boundary")]
    CancellationRequested,
    #[error("Owned work has not acknowledged stopping")]
    CancellationUnacknowledged,
    #[error("An irreversible commit must be reconciled before cancellation can complete")]
    CommitUnresolved,
    #[error("A mutating effect must acquire its commit boundary first")]
    CommitNotStarted,
    #[error("This operation does not support the requested work transition")]
    InvalidTransition,
    #[error(transparent)]
    Time(#[from] ClockDurationError),
    #[error(transparent)]
    Wake(#[from] DesktopApplicationWakeError),
}

struct DesktopApplicationReceiptState {
    request_id: DesktopRequestId,
    received_at: ClockInstant,
    deadline: Option<ClockInstant>,
    cancellation: AtomicBool,
    response: OnceLock<DesktopMessagesEnumResult>,
    pending_notice: Mutex<Option<DesktopFailure>>,
}

#[derive(Clone)]
pub struct DesktopApplicationReceipt(Arc<DesktopApplicationReceiptState>);

impl DesktopApplicationReceipt {
    fn new(
        request: &DesktopRequest,
        received_at: ClockInstant,
    ) -> Result<Self, DesktopApplicationError> {
        let deadline = request
            .command()
            .operation()
            .is_control()
            .then(|| received_at.checked_add(DesktopApplicationState::STOP_DEADLINE))
            .transpose()?;
        Ok(Self(Arc::new(DesktopApplicationReceiptState {
            request_id: request.request_id(),
            received_at,
            deadline,
            cancellation: AtomicBool::new(false),
            response: OnceLock::new(),
            pending_notice: Mutex::new(None),
        })))
    }

    pub fn request_id(&self) -> DesktopRequestId {
        self.0.request_id
    }

    pub fn received_at(&self) -> ClockInstant {
        self.0.received_at
    }

    pub fn deadline(&self) -> Option<ClockInstant> {
        self.0.deadline
    }

    pub fn response(&self) -> Option<&DesktopMessagesEnumResult> {
        self.0.response.get()
    }

    pub fn pending_notice(&self) -> Result<Option<DesktopFailure>, DesktopApplicationError> {
        Ok(self.lock_notice()?.clone())
    }

    fn lock_notice(
        &self,
    ) -> Result<MutexGuard<'_, Option<DesktopFailure>>, DesktopApplicationError> {
        self.0
            .pending_notice
            .try_lock()
            .map_err(|error| match error {
                TryLockError::WouldBlock => DesktopApplicationError::Busy,
                TryLockError::Poisoned(_error) => DesktopApplicationError::Poisoned,
            })
    }

    pub fn shares_outcome(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn is_reconciliation_required(&self, now: ClockInstant) -> bool {
        matches!(
            self.response(),
            Some(DesktopMessagesEnumResult::ReconciliationRequired)
        ) || self.response().is_none() && self.deadline().is_some_and(|deadline| now >= deadline)
    }

    pub fn is_cancellation_requested(&self) -> bool {
        self.0.cancellation.load(Ordering::Acquire)
    }

    fn request_cancellation(&self) {
        self.0.cancellation.store(true, Ordering::Release);
    }
}

pub struct DesktopApplicationWork {
    request: DesktopRequest,
    receipt: DesktopApplicationReceipt,
}

impl DesktopApplicationWork {
    pub fn request(&self) -> &DesktopRequest {
        &self.request
    }

    pub fn receipt(&self) -> &DesktopApplicationReceipt {
        &self.receipt
    }

    pub fn is_cancellation_requested(&self) -> bool {
        self.receipt.is_cancellation_requested()
    }
}

#[derive(Clone, Copy)]
enum DesktopApplicationEnumWorkPhase {
    Queued,
    Running,
    Committing,
    Completed(ClockInstant),
}

struct DesktopApplicationEntry {
    request: DesktopRequest,
    receipt: DesktopApplicationReceipt,
    phase: DesktopApplicationEnumWorkPhase,
    affected: Vec<DesktopApplicationReceipt>,
}

struct DesktopApplicationMutation {
    sequence: u64,
    receipt: DesktopApplicationReceipt,
}

enum DesktopApplicationEnumLifecycle {
    Accepting,
    Closing { quit_sequence: u64 },
}

struct DesktopApplicationState {
    events: VecDeque<DesktopStreamEvent<DesktopEnumEvent>>,
    entries: HashMap<u64, DesktopApplicationEntry>,
    ordinary: VecDeque<u64>,
    urgent: VecDeque<u64>,
    completed: VecDeque<u64>,
    mutations: HashMap<DesktopMessagesEntityId, DesktopApplicationMutation>,
    controls: HashMap<DesktopMessagesEnumControlKey, u64>,
    ordinary_in_flight: usize,
    retired_through: u64,
    last_time: Option<ClockInstant>,
    lifecycle: DesktopApplicationEnumLifecycle,
}

impl DesktopApplicationState {
    const ORDINARY_CAPACITY: usize = 256;
    const RESULT_CAPACITY: usize = DesktopRequestCache::<DesktopMessagesEnumResult>::CAPACITY;
    const RESULT_LIFETIME: ClockDuration = ClockDuration::from_nanos(300_000_000_000);
    const STOP_DEADLINE: ClockDuration = ClockDuration::from_nanos(2_000_000_000);

    fn new() -> Self {
        Self {
            events: VecDeque::with_capacity(DesktopStream::<DesktopEnumEvent>::MAX_UNACKNOWLEDGED),
            entries: HashMap::new(),
            ordinary: VecDeque::with_capacity(Self::ORDINARY_CAPACITY),
            urgent: VecDeque::new(),
            completed: VecDeque::with_capacity(Self::RESULT_CAPACITY),
            mutations: HashMap::new(),
            controls: HashMap::new(),
            ordinary_in_flight: 0,
            retired_through: 0,
            last_time: None,
            lifecycle: DesktopApplicationEnumLifecycle::Accepting,
        }
    }

    fn prune(&mut self, now: ClockInstant) -> Result<(), DesktopApplicationError> {
        if self.last_time.is_some_and(|previous| now < previous) {
            return Err(DesktopApplicationError::TimeOrder);
        }
        self.last_time = Some(now);
        while let Some(sequence) = self.completed.front().copied() {
            let entry = self
                .entries
                .get(&sequence)
                .ok_or(DesktopApplicationError::UnknownWork)?;
            let DesktopApplicationEnumWorkPhase::Completed(completed_at) = entry.phase else {
                return Err(DesktopApplicationError::InvalidTransition);
            };
            if self.completed.len() <= Self::RESULT_CAPACITY
                && now.elapsed_since(completed_at)? < Self::RESULT_LIFETIME
            {
                break;
            }
            self.completed.pop_front();
            self.entries.remove(&sequence);
            self.retired_through = self.retired_through.max(sequence);
        }
        Ok(())
    }

    fn retained(
        &self,
        request: &DesktopRequest,
    ) -> Result<Option<DesktopApplicationReceipt>, DesktopApplicationError> {
        if let Some(sequence) = request.admitted_sequence() {
            return self
                .entries
                .get(&sequence)
                .map(|entry| Some(entry.receipt.clone()))
                .ok_or(DesktopApplicationError::ResyncRequired);
        }
        if request.sequence() <= self.retired_through {
            return Err(DesktopApplicationError::ResyncRequired);
        }
        Ok(None)
    }

    fn complete(
        &mut self,
        sequence: u64,
        response: DesktopMessagesEnumResult,
        now: ClockInstant,
    ) -> Result<(), DesktopApplicationError> {
        let entry = self
            .entries
            .get_mut(&sequence)
            .ok_or(DesktopApplicationError::UnknownWork)?;
        entry
            .receipt
            .0
            .response
            .set(response)
            .map_err(|_response| DesktopApplicationError::AlreadyCompleted)?;
        entry.phase = DesktopApplicationEnumWorkPhase::Completed(now);
        if let Some(key) = entry.request.command().operation().control_key() {
            self.controls.remove(&key);
            match key {
                DesktopMessagesEnumControlKey::Stop(owner)
                | DesktopMessagesEnumControlKey::Pause(owner) => {
                    if !self
                        .controls
                        .keys()
                        .any(|pending| pending.belongs_to(owner))
                        && self
                            .mutations
                            .get(&owner)
                            .is_some_and(|mutation| mutation.receipt.response().is_some())
                    {
                        self.mutations.remove(&owner);
                    }
                }
                DesktopMessagesEnumControlKey::Quit => {}
            }
        } else {
            self.ordinary_in_flight -= 1;
            if let Some(owner) = entry.request.command().operation().mutation_entity()
                && !self.controls.keys().any(|key| key.belongs_to(owner))
            {
                self.mutations.remove(&owner);
            }
        }
        self.completed.push_back(sequence);
        self.prune(now)
    }

    fn targets(
        &self,
        key: DesktopMessagesEnumControlKey,
    ) -> Result<Vec<(u64, DesktopApplicationReceipt)>, DesktopApplicationError> {
        match key {
            DesktopMessagesEnumControlKey::Stop(owner)
            | DesktopMessagesEnumControlKey::Pause(owner) => self
                .mutations
                .get(&owner)
                .map(|mutation| vec![(mutation.sequence, mutation.receipt.clone())])
                .ok_or(DesktopApplicationError::UnknownEntity),
            DesktopMessagesEnumControlKey::Quit => Ok(self
                .entries
                .iter()
                .filter(|(_sequence, entry)| {
                    !entry.request.command().operation().is_control()
                        && entry.receipt.response().is_none()
                })
                .map(|(sequence, entry)| (*sequence, entry.receipt.clone()))
                .collect()),
        }
    }

    fn admit_control(
        &mut self,
        request: &DesktopRequest,
        received_at: ClockInstant,
        now: ClockInstant,
        key: DesktopMessagesEnumControlKey,
    ) -> Result<(DesktopApplicationReceipt, bool), DesktopApplicationError> {
        if let Some(sequence) = self.controls.get(&key).copied() {
            let receipt = self
                .entries
                .get(&sequence)
                .ok_or(DesktopApplicationError::UnknownWork)?
                .receipt
                .clone();
            request.retain_admission(sequence);
            return Ok((receipt, false));
        }
        if key != DesktopMessagesEnumControlKey::Quit
            && matches!(
                self.lifecycle,
                DesktopApplicationEnumLifecycle::Closing { .. }
            )
        {
            return Err(DesktopApplicationError::Closing);
        }
        if key == DesktopMessagesEnumControlKey::Quit
            && let DesktopApplicationEnumLifecycle::Closing { quit_sequence } = self.lifecycle
        {
            let receipt = self
                .entries
                .get(&quit_sequence)
                .ok_or(DesktopApplicationError::ResyncRequired)?
                .receipt
                .clone();
            request.retain_admission(quit_sequence);
            return Ok((receipt, false));
        }
        let targets = self.targets(key)?;
        let receipt = DesktopApplicationReceipt::new(request, received_at)?;
        self.controls.insert(key, request.sequence());
        let mut affected = Vec::with_capacity(targets.len());
        let mut completed_queued = false;
        for (sequence, target_receipt) in targets {
            target_receipt.request_cancellation();
            affected.push(target_receipt);
            if self
                .entries
                .get(&sequence)
                .is_some_and(|entry| matches!(entry.phase, DesktopApplicationEnumWorkPhase::Queued))
            {
                self.ordinary.retain(|queued| *queued != sequence);
                self.complete(sequence, DesktopMessagesEnumResult::Cancelled, now)?;
                completed_queued = true;
            }
        }
        let sequence = request.sequence();
        request.retain_admission(sequence);
        self.entries.insert(
            sequence,
            DesktopApplicationEntry {
                request: request.clone(),
                receipt: receipt.clone(),
                phase: DesktopApplicationEnumWorkPhase::Queued,
                affected,
            },
        );
        self.controls.insert(key, sequence);
        if key == DesktopMessagesEnumControlKey::Quit {
            self.lifecycle = DesktopApplicationEnumLifecycle::Closing {
                quit_sequence: sequence,
            };
        }
        self.urgent.push_back(sequence);
        Ok((receipt, completed_queued))
    }

    fn admit_ordinary(
        &mut self,
        request: &DesktopRequest,
        received_at: ClockInstant,
    ) -> Result<DesktopApplicationReceipt, DesktopApplicationError> {
        if matches!(
            self.lifecycle,
            DesktopApplicationEnumLifecycle::Closing { .. }
        ) {
            return Err(DesktopApplicationError::Closing);
        }
        if self.ordinary_in_flight == Self::ORDINARY_CAPACITY {
            return Err(DesktopApplicationError::Busy);
        }
        if let Some(owner) = request.command().operation().mutation_entity()
            && (self.mutations.contains_key(&owner)
                || self.controls.keys().any(|key| key.belongs_to(owner)))
        {
            return Err(DesktopApplicationError::EntityBusy);
        }
        if request.command().operation().mutation_entity().is_some()
            && self.mutations.len() == Self::ORDINARY_CAPACITY
        {
            return Err(DesktopApplicationError::Busy);
        }
        let receipt = DesktopApplicationReceipt::new(request, received_at)?;
        let sequence = request.sequence();
        request.retain_admission(sequence);
        self.entries.insert(
            sequence,
            DesktopApplicationEntry {
                request: request.clone(),
                receipt: receipt.clone(),
                phase: DesktopApplicationEnumWorkPhase::Queued,
                affected: Vec::new(),
            },
        );
        if let Some(owner) = request.command().operation().mutation_entity() {
            self.mutations.insert(
                owner,
                DesktopApplicationMutation {
                    sequence,
                    receipt: receipt.clone(),
                },
            );
        }
        self.ordinary.push_back(sequence);
        self.ordinary_in_flight += 1;
        Ok(receipt)
    }
}

struct DesktopApplicationShared<C: ClockTimeProvider, W: DesktopApplicationWakeProvider> {
    session: DesktopRequestId,
    next_sequence: AtomicU64,
    clock: Arc<C>,
    wake: Arc<W>,
    wake_pending: AtomicBool,
    quit_retry: AtomicBool,
    ready: Notify,
    state: Mutex<DesktopApplicationState>,
}

impl<C: ClockTimeProvider, W: DesktopApplicationWakeProvider> DesktopApplicationShared<C, W> {
    fn lock(&self) -> Result<MutexGuard<'_, DesktopApplicationState>, DesktopApplicationError> {
        self.state.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => DesktopApplicationError::Busy,
            TryLockError::Poisoned(_error) => DesktopApplicationError::Poisoned,
        })
    }

    fn state_at(
        &self,
        now: ClockInstant,
    ) -> Result<MutexGuard<'_, DesktopApplicationState>, DesktopApplicationError> {
        let mut state = self.lock()?;
        state.prune(now)?;
        Ok(state)
    }

    fn notify_presentation(&self) -> Result<(), DesktopApplicationError> {
        if self
            .wake_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            && let Err(error) = self.wake.wake()
        {
            self.wake_pending.store(false, Ordering::Release);
            return Err(error.into());
        }
        Ok(())
    }
}

pub struct DesktopApplicationPort<C: ClockTimeProvider, W: DesktopApplicationWakeProvider> {
    shared: Arc<DesktopApplicationShared<C, W>>,
}

impl<C: ClockTimeProvider, W: DesktopApplicationWakeProvider> Clone
    for DesktopApplicationPort<C, W>
{
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<C: ClockTimeProvider, W: DesktopApplicationWakeProvider> DesktopApplicationPort<C, W> {
    pub fn request(
        &self,
        command: DesktopCommand,
    ) -> Result<DesktopRequest, DesktopApplicationError> {
        if command.operation().mutation_entity().is_some() && command.expected_revision().is_none()
        {
            return Err(DesktopApplicationError::MissingRevision);
        }
        let sequence = self
            .shared
            .next_sequence
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |sequence| {
                sequence.checked_add(1)
            })
            .map_err(|_sequence| DesktopApplicationError::SequenceOverflow)?;
        Ok(DesktopRequest::new(self.shared.session, sequence, command))
    }

    pub fn try_submit(
        &self,
        request: &DesktopRequest,
    ) -> Result<DesktopApplicationReceipt, DesktopApplicationError> {
        if request.session() != self.shared.session {
            return Err(DesktopApplicationError::WrongSession);
        }
        let now = self.shared.clock.now()?;
        let received_at = request.first_receipt(now);
        let mut state = self.shared.state_at(now)?;
        if let Some(receipt) = state.retained(request)? {
            return Ok(receipt);
        }
        let (receipt, presentation_changed) = match request.command().operation().control_key() {
            Some(key) => state.admit_control(request, received_at, now, key)?,
            None => (state.admit_ordinary(request, received_at)?, false),
        };
        drop(state);
        self.shared.ready.notify_one();
        if presentation_changed {
            self.shared.notify_presentation()?;
        }
        Ok(receipt)
    }

    pub fn acknowledge_wake(&self) {
        self.shared.wake_pending.store(false, Ordering::Release);
    }

    pub fn try_event(
        &self,
    ) -> Result<Option<DesktopStreamEvent<DesktopEnumEvent>>, DesktopApplicationError> {
        Ok(self.shared.lock()?.events.pop_front())
    }

    pub fn rearm_wake(&self) -> Result<(), DesktopApplicationError> {
        self.shared.notify_presentation()
    }

    pub fn retry_quit(&self) -> Result<(), DesktopApplicationError> {
        let state = self.shared.lock()?;
        if !matches!(
            state.lifecycle,
            DesktopApplicationEnumLifecycle::Closing { .. }
        ) {
            return Err(DesktopApplicationError::InvalidTransition);
        }
        self.shared.quit_retry.store(true, Ordering::Release);
        drop(state);
        self.shared.ready.notify_one();
        Ok(())
    }
}

pub struct DesktopApplication<C: ClockTimeProvider, W: DesktopApplicationWakeProvider> {
    shared: Arc<DesktopApplicationShared<C, W>>,
}

impl<C: ClockTimeProvider, W: DesktopApplicationWakeProvider> DesktopApplication<C, W> {
    pub const ORDINARY_CAPACITY: usize = DesktopApplicationState::ORDINARY_CAPACITY;
    pub const RESULT_CAPACITY: usize = DesktopApplicationState::RESULT_CAPACITY;

    pub fn new(clock: Arc<C>, wake: Arc<W>) -> Self {
        Self {
            shared: Arc::new(DesktopApplicationShared {
                session: DesktopRequestId::new(),
                next_sequence: AtomicU64::new(1),
                clock,
                wake,
                wake_pending: AtomicBool::new(false),
                quit_retry: AtomicBool::new(false),
                ready: Notify::new(),
                state: Mutex::new(DesktopApplicationState::new()),
            }),
        }
    }

    pub fn port(&self) -> DesktopApplicationPort<C, W> {
        DesktopApplicationPort {
            shared: Arc::clone(&self.shared),
        }
    }

    pub fn publish_event(
        &self,
        event: DesktopStreamEvent<DesktopEnumEvent>,
    ) -> Result<(), DesktopApplicationError> {
        let mut state = self.shared.lock()?;
        if state.events.len() == DesktopStream::<DesktopEnumEvent>::MAX_UNACKNOWLEDGED {
            return Err(DesktopApplicationError::EventSaturated);
        }
        state.events.push_back(event);
        drop(state);
        self.shared.notify_presentation()
    }

    pub fn clear_events(&self) -> Result<(), DesktopApplicationError> {
        self.shared.lock()?.events.clear();
        Ok(())
    }

    pub async fn ready(&self) {
        self.shared.ready.notified().await;
    }

    pub fn take_quit_retry(&self) -> bool {
        self.shared.quit_retry.swap(false, Ordering::AcqRel)
    }

    pub fn try_next(&self) -> Result<Option<DesktopApplicationWork>, DesktopApplicationError> {
        let now = self.shared.clock.now()?;
        let mut state = self.shared.state_at(now)?;
        let Some(sequence) = state
            .urgent
            .pop_front()
            .or_else(|| state.ordinary.pop_front())
        else {
            return Ok(None);
        };
        let entry = state
            .entries
            .get_mut(&sequence)
            .ok_or(DesktopApplicationError::UnknownWork)?;
        if !matches!(entry.phase, DesktopApplicationEnumWorkPhase::Queued) {
            return Err(DesktopApplicationError::InvalidTransition);
        }
        entry.phase = DesktopApplicationEnumWorkPhase::Running;
        Ok(Some(DesktopApplicationWork {
            request: entry.request.clone(),
            receipt: entry.receipt.clone(),
        }))
    }

    pub fn begin_commit(
        &self,
        work: &DesktopApplicationWork,
    ) -> Result<(), DesktopApplicationError> {
        let mut state = self.shared.lock()?;
        let entry = self.entry(&mut state, work)?;
        if entry.request.command().operation().is_control() {
            return Err(DesktopApplicationError::InvalidTransition);
        }
        if entry.receipt.is_cancellation_requested() {
            return Err(DesktopApplicationError::CancellationRequested);
        }
        match entry.phase {
            DesktopApplicationEnumWorkPhase::Running => {
                entry.phase = DesktopApplicationEnumWorkPhase::Committing;
                Ok(())
            }
            DesktopApplicationEnumWorkPhase::Queued
            | DesktopApplicationEnumWorkPhase::Committing => {
                Err(DesktopApplicationError::InvalidTransition)
            }
            DesktopApplicationEnumWorkPhase::Completed(_) => {
                Err(DesktopApplicationError::AlreadyCompleted)
            }
        }
    }

    pub fn complete_value(
        &self,
        work: &DesktopApplicationWork,
        value: DesktopEnumValue,
        revision: Option<DesktopRevision>,
    ) -> Result<(), DesktopApplicationError> {
        self.complete_effect(work, DesktopMessagesEnumResult::Value { value, revision })
    }

    pub fn complete_failure(
        &self,
        work: &DesktopApplicationWork,
        failure: DesktopFailure,
    ) -> Result<(), DesktopApplicationError> {
        self.complete_effect(work, DesktopMessagesEnumResult::Failure(failure))
    }

    fn complete_effect(
        &self,
        work: &DesktopApplicationWork,
        response: DesktopMessagesEnumResult,
    ) -> Result<(), DesktopApplicationError> {
        let now = self.shared.clock.now()?;
        let mut state = self.shared.state_at(now)?;
        let entry = self.entry(&mut state, work)?;
        if entry.request.command().operation().is_control() {
            return Err(DesktopApplicationError::InvalidTransition);
        }
        match entry.phase {
            DesktopApplicationEnumWorkPhase::Running => {
                if entry.receipt.is_cancellation_requested() {
                    return Err(DesktopApplicationError::CancellationRequested);
                }
                if entry
                    .request
                    .command()
                    .operation()
                    .mutation_entity()
                    .is_some()
                    && matches!(response, DesktopMessagesEnumResult::Value { .. })
                {
                    return Err(DesktopApplicationError::CommitNotStarted);
                }
            }
            DesktopApplicationEnumWorkPhase::Committing => {}
            DesktopApplicationEnumWorkPhase::Queued => {
                return Err(DesktopApplicationError::InvalidTransition);
            }
            DesktopApplicationEnumWorkPhase::Completed(_) => {
                return Err(DesktopApplicationError::AlreadyCompleted);
            }
        }
        state.complete(work.request.sequence(), response, now)?;
        drop(state);
        self.shared.notify_presentation()
    }

    pub fn acknowledge_stopped(
        &self,
        work: &DesktopApplicationWork,
    ) -> Result<(), DesktopApplicationError> {
        let now = self.shared.clock.now()?;
        let mut state = self.shared.state_at(now)?;
        let entry = self.entry(&mut state, work)?;
        if !entry.receipt.is_cancellation_requested()
            || entry.request.command().operation().is_control()
        {
            return Err(DesktopApplicationError::InvalidTransition);
        }
        match entry.phase {
            DesktopApplicationEnumWorkPhase::Running => {}
            DesktopApplicationEnumWorkPhase::Committing => {
                return Err(DesktopApplicationError::CommitUnresolved);
            }
            DesktopApplicationEnumWorkPhase::Queued => {
                return Err(DesktopApplicationError::InvalidTransition);
            }
            DesktopApplicationEnumWorkPhase::Completed(_) => {
                return Err(DesktopApplicationError::AlreadyCompleted);
            }
        }
        state.complete(
            work.request.sequence(),
            DesktopMessagesEnumResult::Cancelled,
            now,
        )?;
        drop(state);
        self.shared.notify_presentation()
    }

    pub fn complete_control(
        &self,
        work: &DesktopApplicationWork,
    ) -> Result<(), DesktopApplicationError> {
        let now = self.shared.clock.now()?;
        let mut state = self.shared.state_at(now)?;
        let entry = self.entry(&mut state, work)?;
        let key = entry
            .request
            .command()
            .operation()
            .control_key()
            .ok_or(DesktopApplicationError::InvalidTransition)?;
        match entry.phase {
            DesktopApplicationEnumWorkPhase::Running => {}
            DesktopApplicationEnumWorkPhase::Queued
            | DesktopApplicationEnumWorkPhase::Committing => {
                return Err(DesktopApplicationError::InvalidTransition);
            }
            DesktopApplicationEnumWorkPhase::Completed(_) => {
                return Err(DesktopApplicationError::AlreadyCompleted);
            }
        }
        if entry
            .affected
            .iter()
            .any(|receipt| receipt.response().is_none())
        {
            return Err(DesktopApplicationError::CancellationUnacknowledged);
        }
        let response = if entry.affected.iter().all(|receipt| {
            matches!(
                receipt.response(),
                Some(DesktopMessagesEnumResult::Cancelled)
            )
        }) {
            key.stopped_result()
        } else {
            DesktopMessagesEnumResult::ReconciliationRequired
        };
        state.complete(work.request.sequence(), response, now)?;
        drop(state);
        self.shared.notify_presentation()
    }

    pub fn report_pending_control(
        &self,
        work: &DesktopApplicationWork,
        notice: DesktopFailure,
    ) -> Result<(), DesktopApplicationError> {
        let mut state = self.shared.lock()?;
        let entry = self.entry(&mut state, work)?;
        if !entry.request.command().operation().is_control()
            || !matches!(entry.phase, DesktopApplicationEnumWorkPhase::Running)
        {
            return Err(DesktopApplicationError::InvalidTransition);
        }
        *entry.receipt.lock_notice()? = Some(notice);
        drop(state);
        self.shared.notify_presentation()
    }

    fn entry<'a>(
        &self,
        state: &'a mut DesktopApplicationState,
        work: &DesktopApplicationWork,
    ) -> Result<&'a mut DesktopApplicationEntry, DesktopApplicationError> {
        if work.request.session() != self.shared.session {
            return Err(DesktopApplicationError::WrongSession);
        }
        state
            .entries
            .get_mut(&work.request.sequence())
            .ok_or(DesktopApplicationError::UnknownWork)
    }
}
