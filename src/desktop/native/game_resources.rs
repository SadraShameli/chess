use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::num::TryFromIntError;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use thiserror::Error;
use tokio::sync::{Mutex, Notify, watch};

use crate::capacity::budget::{CapacityBudget, CapacityBudgetError};
use crate::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use crate::capacity::lease::CapacityLease;
use crate::desktop::engine_commands::{
    DesktopEngineCommandsEnumWorkload, DesktopEngineCommandsResourceRequest,
};
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::desktop::native::game_engine::{
    DesktopNativeGameEngineAllocation, DesktopNativeGameEngineAllocationActivity,
    DesktopNativeGameEngineAllocationError, DesktopNativeGameEngineResources,
};
use crate::clock::duration::ClockDuration;
use crate::engine::configuration::{
    EngineConfiguration, EngineConfigurationEnumBoundary, EngineConfigurationOverrides,
};
use crate::engine::enums::{EngineEnumOption, EngineOptionEnumDescriptor};
use crate::engine::package::manifest::EnginePackageManifest;
use crate::engine::resource_config::{EngineResourceConfig, EngineResourceConfigError};
use crate::game::engine_provider::{GameEngineContext, GameEngineSearchConfig};
use crate::game::enums::GameId;

#[derive(Debug, Error)]
pub enum DesktopNativeGameResourcesPolicyError {
    #[error("Engine workloads need at least one shared CPU worker and one MiB of hash")]
    Empty,
    #[error("The configured workload of {requested} exceeds the shared CPU budget of {available}")]
    ThreadBudget { requested: usize, available: usize },
    #[error("Configured analysis hash exceeds the shared hash budget")]
    HashBudget,
    #[error("The pinned engine does not advertise a numeric option for {0:?}")]
    OptionDescriptor(EngineEnumOption),
    #[error("The resource accounting value exceeds its supported range: {0}")]
    Conversion(#[from] TryFromIntError),
    #[error(transparent)]
    Config(#[from] EngineResourceConfigError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopNativeGameResourcesPolicy {
    workers: usize,
    hash_mib: u32,
    startup_hash_mib: u32,
    move_overhead: ClockDuration,
}

impl DesktopNativeGameResourcesPolicy {
    pub const MIB: u64 = 1024 * 1024;
    pub const HASH_CAP_BYTES: u64 = 2 * 1024 * Self::MIB;
    pub const PROCESS_ALLOWANCE_BYTES: u64 = 128 * Self::MIB;
    pub const HEADROOM_BYTES: u64 = 512 * Self::MIB;
    const HASH_MEMORY_DIVISOR: u64 = 10;

    pub fn automatic(
        workers: usize,
        startup_available_bytes: u64,
    ) -> Result<Self, DesktopNativeGameResourcesPolicyError> {
        let hash_mib = u32::try_from(
            (startup_available_bytes / Self::HASH_MEMORY_DIVISOR).min(Self::HASH_CAP_BYTES)
                / Self::MIB,
        )?;
        Self::new(workers, hash_mib)
    }

    pub fn new(
        workers: usize,
        hash_mib: u32,
    ) -> Result<Self, DesktopNativeGameResourcesPolicyError> {
        if workers == 0 || hash_mib == 0 {
            return Err(DesktopNativeGameResourcesPolicyError::Empty);
        }
        let descriptor = |option: EngineEnumOption| match option.typed_descriptor() {
            EngineOptionEnumDescriptor::Spin(range) => Ok(range),
            EngineOptionEnumDescriptor::Button
            | EngineOptionEnumDescriptor::Check(_)
            | EngineOptionEnumDescriptor::String => Err(
                DesktopNativeGameResourcesPolicyError::OptionDescriptor(option),
            ),
        };
        let startup_hash_mib = descriptor(EngineEnumOption::Hash)?.default_value();
        EngineResourceConfig::new(
            workers,
            hash_mib,
            GameEngineSearchConfig::initial_default().move_overhead(),
        )?;
        Ok(Self {
            workers,
            hash_mib,
            startup_hash_mib,
            move_overhead: GameEngineSearchConfig::initial_default().move_overhead(),
        })
    }

    pub fn with_overrides(
        self,
        overrides: EngineConfigurationOverrides,
    ) -> Result<Self, DesktopNativeGameResourcesPolicyError> {
        let base = EngineResourceConfig::new(self.workers, self.hash_mib, self.move_overhead)?;
        let threads = match overrides.threads {
            Some(requested) => usize::try_from(requested)?.min(self.workers),
            None => self.workers,
        };
        let move_overhead = match overrides.move_overhead_ms {
            Some(milliseconds) => ClockDuration::from_millis(u64::from(milliseconds))
                .map_err(EngineResourceConfigError::from)?,
            None => self.move_overhead,
        };
        let requested = EngineResourceConfig::new(
            threads,
            overrides.hash_mib.unwrap_or(self.hash_mib),
            move_overhead,
        )?;
        let mut configuration = EngineConfiguration::new(base);
        configuration.request(requested, EngineConfigurationEnumBoundary::Idle);
        let applied = configuration.effective();
        Ok(Self {
            workers: usize::try_from(applied.threads())?,
            hash_mib: applied.hash_mib(),
            startup_hash_mib: self.startup_hash_mib,
            move_overhead: applied.move_overhead(),
        })
    }

    pub const fn workers(self) -> usize {
        self.workers
    }

    pub const fn hash_mib(self) -> u32 {
        self.hash_mib
    }

    pub const fn startup_hash_mib(self) -> u32 {
        self.startup_hash_mib
    }

    fn required_memory(
        self,
        processes: usize,
    ) -> Result<u64, DesktopNativeGameResourcesPolicyError> {
        let per_process = EnginePackageManifest::NETWORK_BYTES + Self::PROCESS_ALLOWANCE_BYTES;
        let processes = u64::try_from(processes)?;
        Ok(u64::from(self.hash_mib) * Self::MIB + per_process * processes + Self::HEADROOM_BYTES)
    }

    fn config(
        self,
        workers: usize,
        hash_mib: u32,
    ) -> Result<EngineResourceConfig, EngineResourceConfigError> {
        EngineResourceConfig::new(workers, hash_mib, self.move_overhead)
    }
}

pub trait DesktopNativeGameResourcesMemoryProvider: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn available_bytes(&self) -> impl Future<Output = Result<u64, Self::Error>> + Send;
}

pub struct DesktopNativeGameResourcesMemory {
    executor: Arc<CapacityExecutor>,
}

impl DesktopNativeGameResourcesMemory {
    pub fn new(executor: Arc<CapacityExecutor>) -> Self {
        Self { executor }
    }
}

impl DesktopNativeGameResourcesMemoryProvider for DesktopNativeGameResourcesMemory {
    type Error = CapacityExecutorError;

    async fn available_bytes(&self) -> Result<u64, Self::Error> {
        self.executor
            .submit_waiting(|_cancel| {
                let mut system = sysinfo::System::new();
                system.refresh_memory();
                system.available_memory()
            })
            .await?
            .join()
            .await
    }
}

#[derive(Debug, Error)]
pub enum DesktopNativeGameResourcesError<E: std::error::Error + Send + Sync + 'static> {
    #[error(
        "The {workload:?} startup hash hint {supplied:?} does not match the actual process requirement {required:?}"
    )]
    StartupHint {
        workload: DesktopEngineCommandsEnumWorkload,
        supplied: Option<u32>,
        required: Option<u32>,
    },
    #[error("A configured, acknowledged analysis-engine reservation is required")]
    AnalysisMissing,
    #[error(
        "This analysis engine already owns an unfinished allocation for the requested operation"
    )]
    AnalysisBusy,
    #[error("Analysis must use its acknowledged configured resources")]
    AnalysisResources,
    #[error("Live-game search demand has priority over analysis and diagnostics")]
    LivePriority,
    #[error(
        "The requested {requested} workers exceed the shared engine budget of {available} workers"
    )]
    AnalysisThreads { requested: usize, available: usize },
    #[error(
        "Analysis admission requires {required_mib} MiB of configured and startup hash, exceeding the shared {budget_mib} MiB budget"
    )]
    AnalysisHash { required_mib: u64, budget_mib: u32 },
    #[error("The analysis reservation is absent, unadmitted or belongs to another owner")]
    UnknownAnalysisReservation,
    #[error("Game {0:?} already owns a running-engine resource registration")]
    DuplicateGame(GameId),
    #[error("Game {0:?} has no admitted running-engine resource registration")]
    UnknownGame(GameId),
    #[error("Game {0:?} already owns a pending or running search allocation")]
    SearchPending(GameId),
    #[error("Starting {games} engine games would exceed the {workers} minimum-thread reservations")]
    ThreadAdmission { games: usize, workers: usize },
    #[error(
        "Starting an engine requires {required_mib} MiB of transient default hash, but the total budget is {budget_mib} MiB"
    )]
    StartupHashAdmission { required_mib: u64, budget_mib: u32 },
    #[error(
        "Engine admission needs {required} available bytes including hash, process allowances and app headroom; the OS reports {available}"
    )]
    MemoryAdmission { required: u64, available: u64 },
    #[error("Cannot query currently available memory: {0}")]
    Memory(#[source] E),
    #[error(
        "Game {game:?} acknowledged {received_mib} MiB of hash without a reservation; its maximum reserved value is {reserved_mib} MiB"
    )]
    HashAcknowledgment {
        game: GameId,
        received_mib: u32,
        reserved_mib: u32,
    },
    #[error(transparent)]
    Policy(#[from] DesktopNativeGameResourcesPolicyError),
    #[error(transparent)]
    Config(#[from] EngineResourceConfigError),
    #[error(transparent)]
    Capacity(#[from] CapacityBudgetError),
    #[error(transparent)]
    Allocation(#[from] DesktopNativeGameEngineAllocationError),
}

pub struct DesktopNativeGameResourcesOptions<M> {
    pub budget: CapacityBudget,
    pub policy: DesktopNativeGameResourcesPolicy,
    pub memory: Arc<M>,
}

pub struct DesktopNativeGameResources<M> {
    budget: CapacityBudget,
    policy: DesktopNativeGameResourcesPolicy,
    memory: Arc<M>,
    state: Mutex<DesktopNativeGameResourcesState>,
    changed: Arc<Notify>,
    live_demand: watch::Sender<Option<GameEngineContext>>,
}

pub struct DesktopNativeGameResourcesAnalysisReservation {
    claim: Arc<DesktopNativeGameResourcesAnalysisClaim>,
    changed: Arc<Notify>,
}

impl Drop for DesktopNativeGameResourcesAnalysisReservation {
    fn drop(&mut self) {
        self.claim.registered.store(false, Ordering::SeqCst);
        self.changed.notify_waiters();
    }
}

struct DesktopNativeGameResourcesAnalysisClaim {
    owner: DesktopMessagesEntityId,
    workload: DesktopEngineCommandsEnumWorkload,
    resources: EngineResourceConfig,
    registered: AtomicBool,
    admitted: AtomicBool,
    acknowledged: AtomicBool,
    reserved_hash_mib: AtomicU32,
}

impl DesktopNativeGameResourcesAnalysisClaim {
    fn is_active(&self) -> bool {
        !self.is_persistent() || !self.acknowledged.load(Ordering::SeqCst)
    }

    fn is_persistent(&self) -> bool {
        matches!(
            self.workload,
            DesktopEngineCommandsEnumWorkload::Configuration
                | DesktopEngineCommandsEnumWorkload::Retry
        )
    }
}

pub struct DesktopNativeGameResourcesRegistration {
    registered: Arc<AtomicBool>,
    changed: Arc<Notify>,
}

impl Drop for DesktopNativeGameResourcesRegistration {
    fn drop(&mut self) {
        self.registered.store(false, Ordering::SeqCst);
        self.changed.notify_waiters();
    }
}

struct DesktopNativeGameResourcesEntry {
    game: GameId,
    registered: Arc<AtomicBool>,
    searching: Arc<AtomicBool>,
    assigned_workers: Arc<AtomicUsize>,
    admitted: bool,
    applied_hash_mib: Option<u32>,
    reserved_hash_mib: u32,
    desired_hash_mib: u32,
}

impl DesktopNativeGameResourcesEntry {
    fn planned_hash_mib(&self) -> u32 {
        self.reserved_hash_mib.max(self.desired_hash_mib)
    }
}

#[derive(Default)]
struct DesktopNativeGameResourcesState {
    entries: Vec<DesktopNativeGameResourcesEntry>,
    analysis: Vec<Arc<DesktopNativeGameResourcesAnalysisClaim>>,
    rotation: usize,
}

impl DesktopNativeGameResourcesState {
    fn has_active_analysis(&self) -> bool {
        self.analysis.iter().any(|claim| claim.is_active())
    }

    fn retain_registered(&mut self) {
        self.entries
            .retain(|entry| entry.registered.load(Ordering::SeqCst));
        self.analysis
            .retain(|claim| claim.registered.load(Ordering::SeqCst));
    }

    fn analysis_hash(
        &self,
        pending: bool,
        candidate: Option<&Arc<DesktopNativeGameResourcesAnalysisClaim>>,
    ) -> u64 {
        let mut owners = HashMap::<DesktopMessagesEntityId, u32>::new();
        let mut diagnostics = 0_u64;
        for claim in self.analysis.iter().filter(|claim| {
            claim.registered.load(Ordering::SeqCst)
                && (pending
                    || claim.admitted.load(Ordering::SeqCst)
                    || candidate.is_some_and(|candidate| Arc::ptr_eq(candidate, claim)))
        }) {
            let hash = claim.reserved_hash_mib.load(Ordering::SeqCst);
            if claim.workload == DesktopEngineCommandsEnumWorkload::Diagnostic {
                diagnostics += u64::from(hash);
            } else {
                let reserved = owners.entry(claim.owner).or_default();
                *reserved = (*reserved).max(hash);
            }
        }
        diagnostics + owners.values().map(|hash| u64::from(*hash)).sum::<u64>()
    }

    fn process_count(&self) -> usize {
        let owners: HashSet<_> = self
            .analysis
            .iter()
            .filter(|claim| claim.workload != DesktopEngineCommandsEnumWorkload::Diagnostic)
            .map(|claim| claim.owner)
            .collect();
        self.entries.len()
            + owners.len()
            + self
                .analysis
                .iter()
                .filter(|claim| claim.workload == DesktopEngineCommandsEnumWorkload::Diagnostic)
                .count()
    }

    fn hash_for_games(
        &self,
        policy: DesktopNativeGameResourcesPolicy,
    ) -> Result<u32, DesktopNativeGameResourcesPolicyError> {
        policy
            .hash_mib
            .checked_sub(u32::try_from(self.analysis_hash(true, None))?)
            .ok_or(DesktopNativeGameResourcesPolicyError::HashBudget)
    }

    fn has_live_demand(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.searching.load(Ordering::SeqCst))
    }

    fn index(&self, game: GameId) -> Option<usize> {
        self.entries.iter().position(|entry| entry.game == game)
    }

    fn assigned(total: u32, count: u32, index: u32) -> u32 {
        total / count + u32::from(index < total % count)
    }

    fn rebalance(
        &mut self,
        policy: DesktopNativeGameResourcesPolicy,
    ) -> Result<(), DesktopNativeGameResourcesPolicyError> {
        self.retain_registered();
        let hash_mib = self.hash_for_games(policy)?;
        if self.entries.is_empty() {
            return Ok(());
        }
        let count = u32::try_from(self.entries.len())?;
        let mut startup_reserved = 0_u64;
        let mut existing = 0_u32;
        for (index, entry) in self.entries.iter_mut().enumerate() {
            if entry.applied_hash_mib.is_none() {
                entry.desired_hash_mib = Self::assigned(hash_mib, count, u32::try_from(index)?);
                startup_reserved += u64::from(entry.desired_hash_mib.max(policy.startup_hash_mib));
            } else {
                existing += 1;
            }
        }
        let total = u64::from(hash_mib);
        if startup_reserved + u64::from(existing) > total {
            return Ok(());
        }
        let remaining = u32::try_from(total - startup_reserved)?;
        let mut index = 0_u32;
        for entry in &mut self.entries {
            if entry.applied_hash_mib.is_some() {
                entry.desired_hash_mib = Self::assigned(remaining, existing, index);
                index += 1;
            }
        }
        Ok(())
    }

    fn startup_required(&self, policy: DesktopNativeGameResourcesPolicy) -> u64 {
        self.entries
            .iter()
            .map(|entry| {
                if entry.applied_hash_mib.is_none() {
                    u64::from(entry.desired_hash_mib.max(policy.startup_hash_mib))
                } else {
                    1
                }
            })
            .sum::<u64>()
            + self.analysis_hash(true, None)
    }

    fn reserved_hash(&self) -> u64 {
        self.entries
            .iter()
            .map(|entry| u64::from(entry.reserved_hash_mib))
            .sum::<u64>()
            + self.analysis_hash(false, None)
    }

    fn can_reserve_hash(&self, index: usize, policy: DesktopNativeGameResourcesPolicy) -> bool {
        let entry = &self.entries[index];
        let increase = entry.planned_hash_mib() - entry.reserved_hash_mib;
        self.reserved_hash() + u64::from(increase) <= u64::from(policy.hash_mib)
    }

    fn search_workers(&self, index: usize, policy: DesktopNativeGameResourcesPolicy) -> usize {
        let active: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.searching.load(Ordering::SeqCst))
            .map(|(index, _)| index)
            .collect();
        let count = active.len();
        if count == 0 {
            return 1;
        }
        let quotient = policy.workers / count;
        let granted_extras: usize = active
            .iter()
            .map(|entry_index| {
                self.entries[*entry_index]
                    .assigned_workers
                    .load(Ordering::SeqCst)
                    .saturating_sub(quotient)
            })
            .sum();
        let remaining_extras = (policy.workers % count).saturating_sub(granted_extras);
        let mut waiting: Vec<_> = active
            .iter()
            .enumerate()
            .filter(|(_, entry_index)| {
                self.entries[**entry_index]
                    .assigned_workers
                    .load(Ordering::SeqCst)
                    == 0
            })
            .map(|(rank, entry_index)| {
                ((rank + count - self.rotation % count) % count, *entry_index)
            })
            .collect();
        waiting.sort_unstable();
        let has_extra = waiting
            .iter()
            .position(|(_, waiting_index)| *waiting_index == index)
            .is_some_and(|rank| rank < remaining_extras);
        quotient + usize::from(has_extra)
    }
}

impl<M: DesktopNativeGameResourcesMemoryProvider> DesktopNativeGameResources<M> {
    pub fn new(
        options: DesktopNativeGameResourcesOptions<M>,
    ) -> Result<Self, DesktopNativeGameResourcesPolicyError> {
        if options.policy.workers() > options.budget.workers() {
            return Err(DesktopNativeGameResourcesPolicyError::ThreadBudget {
                requested: options.policy.workers(),
                available: options.budget.workers(),
            });
        }
        Ok(Self {
            budget: options.budget,
            policy: options.policy,
            memory: options.memory,
            state: Mutex::new(DesktopNativeGameResourcesState::default()),
            changed: Arc::new(Notify::new()),
            live_demand: watch::channel(None).0,
        })
    }

    pub const fn policy(&self) -> DesktopNativeGameResourcesPolicy {
        self.policy
    }

    pub fn live_demand(&self) -> watch::Receiver<Option<GameEngineContext>> {
        self.live_demand.subscribe()
    }

    fn analysis_priority(
        &self,
        state: &DesktopNativeGameResourcesState,
        resources: EngineResourceConfig,
    ) -> Result<usize, DesktopNativeGameResourcesError<M::Error>> {
        if state.has_live_demand() {
            return Err(DesktopNativeGameResourcesError::LivePriority);
        }
        let workers =
            usize::try_from(resources.threads()).map_err(EngineResourceConfigError::from)?;
        let available = self.policy.workers;
        if workers > available {
            return Err(DesktopNativeGameResourcesError::AnalysisThreads {
                requested: workers,
                available,
            });
        }
        Ok(workers)
    }

    fn validate_analysis(
        &self,
        state: &DesktopNativeGameResourcesState,
        owner: DesktopMessagesEntityId,
        request: DesktopEngineCommandsResourceRequest,
    ) -> Result<(), DesktopNativeGameResourcesError<M::Error>> {
        self.analysis_priority(state, request.resources())?;
        let claims: Vec<_> = state
            .analysis
            .iter()
            .filter(|claim| claim.owner == owner)
            .collect();
        let configured = claims
            .iter()
            .rev()
            .find(|claim| claim.is_persistent() && claim.acknowledged.load(Ordering::SeqCst));
        if claims.iter().any(|claim| claim.is_active()) {
            return Err(DesktopNativeGameResourcesError::AnalysisBusy);
        }
        match request.workload() {
            DesktopEngineCommandsEnumWorkload::Analysis => {
                let configured =
                    configured.ok_or(DesktopNativeGameResourcesError::AnalysisMissing)?;
                if configured.resources != request.resources() {
                    return Err(DesktopNativeGameResourcesError::AnalysisResources);
                }
            }
            DesktopEngineCommandsEnumWorkload::Configuration
            | DesktopEngineCommandsEnumWorkload::Retry => {
                if request.workload() == DesktopEngineCommandsEnumWorkload::Configuration
                    && configured.is_none()
                {
                    return Err(DesktopNativeGameResourcesError::AnalysisMissing);
                }
            }
            DesktopEngineCommandsEnumWorkload::Diagnostic => {}
        }
        Ok(())
    }

    fn memory_admission(
        &self,
        processes: usize,
        available: u64,
    ) -> Result<(), DesktopNativeGameResourcesError<M::Error>> {
        let required = self.policy.required_memory(processes)?;
        if required > available {
            return Err(DesktopNativeGameResourcesError::MemoryAdmission {
                required,
                available,
            });
        }
        Ok(())
    }

    pub(crate) async fn reserve_analysis(
        &self,
        owner: DesktopMessagesEntityId,
        request: DesktopEngineCommandsResourceRequest,
    ) -> Result<
        (CapacityLease, DesktopNativeGameResourcesAnalysisReservation),
        DesktopNativeGameResourcesError<M::Error>,
    > {
        let startup_hash_mib = match request.workload() {
            DesktopEngineCommandsEnumWorkload::Retry
            | DesktopEngineCommandsEnumWorkload::Diagnostic => Some(self.policy.startup_hash_mib),
            DesktopEngineCommandsEnumWorkload::Analysis
            | DesktopEngineCommandsEnumWorkload::Configuration => None,
        };
        if request.startup_hash_mib() != startup_hash_mib {
            return Err(DesktopNativeGameResourcesError::StartupHint {
                workload: request.workload(),
                supplied: request.startup_hash_mib(),
                required: startup_hash_mib,
            });
        }
        let available = self
            .memory
            .available_bytes()
            .await
            .map_err(DesktopNativeGameResourcesError::Memory)?;
        let reservation = {
            let mut state = self.state.lock().await;
            state.rebalance(self.policy)?;
            self.validate_analysis(&state, owner, request)?;
            let claim = Arc::new(DesktopNativeGameResourcesAnalysisClaim {
                owner,
                workload: request.workload(),
                resources: request.resources(),
                registered: AtomicBool::new(true),
                admitted: AtomicBool::new(false),
                acknowledged: AtomicBool::new(false),
                reserved_hash_mib: AtomicU32::new(
                    request
                        .resources()
                        .hash_mib()
                        .max(startup_hash_mib.unwrap_or(0)),
                ),
            });
            let reservation = DesktopNativeGameResourcesAnalysisReservation {
                claim: Arc::clone(&claim),
                changed: Arc::clone(&self.changed),
            };
            state.analysis.push(claim);
            let required_mib = state.analysis_hash(true, None)
                + u64::try_from(state.entries.len())
                    .map_err(DesktopNativeGameResourcesPolicyError::from)?;
            if required_mib > u64::from(self.policy.hash_mib) {
                return Err(DesktopNativeGameResourcesError::AnalysisHash {
                    required_mib,
                    budget_mib: self.policy.hash_mib,
                });
            }
            state.rebalance(self.policy)?;
            let required_mib = state.startup_required(self.policy);
            if required_mib > u64::from(self.policy.hash_mib) {
                return Err(DesktopNativeGameResourcesError::AnalysisHash {
                    required_mib,
                    budget_mib: self.policy.hash_mib,
                });
            }
            self.memory_admission(state.process_count(), available)?;
            reservation
        };
        self.changed.notify_waiters();
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let available = self
                .memory
                .available_bytes()
                .await
                .map_err(DesktopNativeGameResourcesError::Memory)?;
            {
                let mut state = self.state.lock().await;
                state.rebalance(self.policy)?;
                let workers = self.analysis_priority(&state, request.resources())?;
                self.memory_admission(state.process_count(), available)?;
                let game_hash: u64 = state
                    .entries
                    .iter()
                    .map(|entry| u64::from(entry.reserved_hash_mib))
                    .sum();
                if game_hash + state.analysis_hash(false, Some(&reservation.claim))
                    <= u64::from(self.policy.hash_mib)
                {
                    let lease = self.budget.try_lease(workers)?;
                    reservation.claim.admitted.store(true, Ordering::SeqCst);
                    drop(state);
                    self.changed.notify_waiters();
                    return Ok((lease, reservation));
                }
            }
            changed.await;
        }
    }

    pub(crate) async fn acknowledge_analysis(
        &self,
        owner: DesktopMessagesEntityId,
        reservation: &DesktopNativeGameResourcesAnalysisReservation,
    ) -> Result<(), DesktopNativeGameResourcesError<M::Error>> {
        let mut state = self.state.lock().await;
        state.rebalance(self.policy)?;
        let claim = &reservation.claim;
        if claim.owner != owner
            || !claim.admitted.load(Ordering::SeqCst)
            || !state
                .analysis
                .iter()
                .any(|current| Arc::ptr_eq(current, claim))
            || !claim.is_persistent()
        {
            return Err(DesktopNativeGameResourcesError::UnknownAnalysisReservation);
        }
        claim
            .reserved_hash_mib
            .store(claim.resources.hash_mib(), Ordering::SeqCst);
        claim.acknowledged.store(true, Ordering::SeqCst);
        state.rebalance(self.policy)?;
        drop(state);
        self.changed.notify_waiters();
        Ok(())
    }

    async fn allocation(
        &self,
        context: GameEngineContext,
        is_search: bool,
    ) -> Result<DesktopNativeGameEngineAllocation, DesktopNativeGameResourcesError<M::Error>> {
        let game = context.game();
        let activity = if is_search {
            let mut state = self.state.lock().await;
            state.rebalance(self.policy)?;
            let index = state
                .index(game)
                .filter(|index| state.entries[*index].admitted)
                .ok_or(DesktopNativeGameResourcesError::UnknownGame(game))?;
            let searching = Arc::clone(&state.entries[index].searching);
            searching
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .map_err(|_value| DesktopNativeGameResourcesError::SearchPending(game))?;
            let _previous = self.live_demand.send_replace(Some(context));
            Some(DesktopNativeGameEngineAllocationActivity::new(
                searching,
                Arc::clone(&self.changed),
                Arc::clone(&state.entries[index].assigned_workers),
            ))
        } else {
            None
        };
        self.changed.notify_waiters();
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let planned = {
                let mut state = self.state.lock().await;
                state.rebalance(self.policy)?;
                let index = state
                    .index(game)
                    .filter(|index| state.entries[*index].admitted)
                    .ok_or(DesktopNativeGameResourcesError::UnknownGame(game))?;
                if !is_search && state.entries[index].searching.load(Ordering::SeqCst) {
                    return Err(DesktopNativeGameResourcesError::SearchPending(game));
                }
                let needs_update = state.entries[index].applied_hash_mib
                    != Some(state.entries[index].desired_hash_mib);
                if (is_search || needs_update)
                    && (!is_search || !state.has_active_analysis())
                    && state.can_reserve_hash(index, self.policy)
                {
                    let workers = if is_search {
                        state.search_workers(index, self.policy)
                    } else {
                        1
                    };
                    Some((workers, state.entries[index].desired_hash_mib))
                } else {
                    None
                }
            };
            let Some((workers, hash_mib)) = planned else {
                changed.await;
                continue;
            };
            let lease = tokio::select! {
                biased;
                () = changed.as_mut() => continue,
                lease = self.budget.lease(workers) => lease?,
            };
            let mut state = self.state.lock().await;
            state.rebalance(self.policy)?;
            let index = state
                .index(game)
                .filter(|index| state.entries[*index].admitted)
                .ok_or(DesktopNativeGameResourcesError::UnknownGame(game))?;
            let current_workers = if is_search {
                state.search_workers(index, self.policy)
            } else {
                1
            };
            if current_workers != workers
                || state.entries[index].desired_hash_mib != hash_mib
                || (is_search && state.has_active_analysis())
                || !state.can_reserve_hash(index, self.policy)
            {
                continue;
            }
            state.entries[index].reserved_hash_mib = state.entries[index].planned_hash_mib();
            if is_search {
                let active = state
                    .entries
                    .iter()
                    .filter(|entry| entry.searching.load(Ordering::SeqCst))
                    .count();
                if workers > self.policy.workers / active {
                    state.rotation = state.rotation.wrapping_add(1);
                }
                state.entries[index]
                    .assigned_workers
                    .store(workers, Ordering::SeqCst);
            }
            let config = self.policy.config(workers, hash_mib)?;
            let allocation =
                DesktopNativeGameEngineAllocation::new(config, lease)?.with_activity(activity);
            drop(state);
            self.changed.notify_waiters();
            return Ok(allocation);
        }
    }
}

impl<M: DesktopNativeGameResourcesMemoryProvider> DesktopNativeGameEngineResources
    for DesktopNativeGameResources<M>
{
    type Registration = DesktopNativeGameResourcesRegistration;
    type Error = DesktopNativeGameResourcesError<M::Error>;

    async fn register(
        &self,
        context: GameEngineContext,
    ) -> Result<Self::Registration, Self::Error> {
        let game = context.game();
        let available = self
            .memory
            .available_bytes()
            .await
            .map_err(DesktopNativeGameResourcesError::Memory)?;
        let registration = {
            let mut state = self.state.lock().await;
            state.rebalance(self.policy)?;
            if state.index(game).is_some() {
                return Err(Self::Error::DuplicateGame(game));
            }
            let count = state.entries.len() + 1;
            if count > self.policy.workers() {
                return Err(Self::Error::ThreadAdmission {
                    games: count,
                    workers: self.policy.workers(),
                });
            }
            self.memory_admission(state.process_count() + 1, available)?;
            let registered = Arc::new(AtomicBool::new(true));
            let registration = DesktopNativeGameResourcesRegistration {
                registered: Arc::clone(&registered),
                changed: Arc::clone(&self.changed),
            };
            state.entries.push(DesktopNativeGameResourcesEntry {
                game,
                registered,
                searching: Arc::new(AtomicBool::new(false)),
                assigned_workers: Arc::new(AtomicUsize::new(0)),
                admitted: false,
                applied_hash_mib: None,
                reserved_hash_mib: 0,
                desired_hash_mib: 0,
            });
            state.rebalance(self.policy)?;
            let required_mib = state.startup_required(self.policy);
            if required_mib > u64::from(self.policy.hash_mib) {
                return Err(Self::Error::StartupHashAdmission {
                    required_mib,
                    budget_mib: self.policy.hash_mib,
                });
            }
            registration
        };
        self.changed.notify_waiters();
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let can_admit = {
                let mut state = self.state.lock().await;
                state.rebalance(self.policy)?;
                let index = state.index(game).ok_or(Self::Error::UnknownGame(game))?;
                let startup = state.entries[index]
                    .desired_hash_mib
                    .max(self.policy.startup_hash_mib);
                state.reserved_hash() + u64::from(startup) <= u64::from(self.policy.hash_mib)
            };
            if can_admit {
                let available = self
                    .memory
                    .available_bytes()
                    .await
                    .map_err(Self::Error::Memory)?;
                let mut state = self.state.lock().await;
                state.rebalance(self.policy)?;
                let index = state.index(game).ok_or(Self::Error::UnknownGame(game))?;
                self.memory_admission(state.process_count(), available)?;
                let startup = state.entries[index]
                    .desired_hash_mib
                    .max(self.policy.startup_hash_mib);
                if state.reserved_hash() + u64::from(startup) <= u64::from(self.policy.hash_mib) {
                    state.entries[index].admitted = true;
                    state.entries[index].reserved_hash_mib = startup;
                    return Ok(registration);
                }
            }
            changed.await;
        }
    }

    async fn allocate(
        &self,
        context: GameEngineContext,
    ) -> Result<DesktopNativeGameEngineAllocation, Self::Error> {
        self.allocation(context, true).await
    }

    async fn next_update(
        &self,
        context: GameEngineContext,
    ) -> Result<DesktopNativeGameEngineAllocation, Self::Error> {
        self.allocation(context, false).await
    }

    async fn acknowledge(
        &self,
        context: GameEngineContext,
        config: EngineResourceConfig,
    ) -> Result<(), Self::Error> {
        let mut state = self.state.lock().await;
        state.rebalance(self.policy)?;
        let index = state
            .index(context.game())
            .filter(|index| state.entries[*index].admitted)
            .ok_or(Self::Error::UnknownGame(context.game()))?;
        let entry = &mut state.entries[index];
        let reserved_mib = entry.reserved_hash_mib;
        if config.hash_mib() > reserved_mib {
            return Err(Self::Error::HashAcknowledgment {
                game: context.game(),
                received_mib: config.hash_mib(),
                reserved_mib,
            });
        }
        entry.applied_hash_mib = Some(config.hash_mib());
        entry.reserved_hash_mib = config.hash_mib();
        state.rebalance(self.policy)?;
        drop(state);
        self.changed.notify_waiters();
        Ok(())
    }
}
