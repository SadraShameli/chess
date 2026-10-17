use strum::IntoEnumIterator;
use std::mem::size_of;
use std::sync::Arc;

use thiserror::Error;
use uuid::Uuid;

use crate::board::r#move::BoardMove;
use crate::board::position::BoardPosition;
use crate::engine::enums::EngineEnumOption;
use crate::engine::options::EngineOptionsEnumTablebases;
use crate::engine::process_provider::{EngineProcess, EngineProcessLimits};
use crate::engine::search::enums::EngineSearchEnumState;
use crate::engine::search::identity::{EngineSearchContext, EngineSearchTag};
use crate::engine::search::limit::EngineSearchLimit;
use crate::engine::search::position::EngineSearchPosition;
use crate::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumCancellation, EngineSearchSessionEnumCompletion,
    EngineSearchSessionError, EngineSearchSessionMetadata, EngineSearchSessionTerminal,
};
use crate::game::enums::{GameEnumOutcomeReason, GameEnumResult};
use crate::game::outcome::GameOutcome;
use crate::report::configuration::ReportJobConfiguration;
use crate::report::finding::{ReportFindingPositionResult, ReportMoveFinding};
use crate::report::scoring::{ReportScoringEnumCoverage, ReportScoringError, ReportScoringSummary};
use crate::report::source::{ReportJobSource, ReportJobSourceError};

#[derive(Debug, Error)]
pub enum ReportJobError {
    #[error("A report job identity must not be nil")]
    NilIdentity,
    #[error("The report job cannot perform this transition from {0:?}")]
    State(ReportJobEnumState),
    #[error("The report source is outdated")]
    Outdated,
    #[error("The current source belongs to another game")]
    WrongGame,
    #[error("The report attempt does not identify its active immutable work")]
    StaleAttempt,
    #[error("The report search differs in engine, options, history, context, budget or generation")]
    SearchMismatch,
    #[error("The report position is outside the line or already completed")]
    PositionIndex,
    #[error("The report's checked counter is exhausted")]
    CounterExhausted,
    #[error("The report still owns an unobserved engine search")]
    SearchStillOwned,
    #[error("The outcome does not identify a terminal result for the selected source node")]
    OutcomeMismatch,
    #[error(transparent)]
    Source(#[from] ReportJobSourceError),
    #[error(transparent)]
    Scoring(#[from] ReportScoringError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReportJobId(Uuid);

impl TryFrom<Uuid> for ReportJobId {
    type Error = ReportJobError;
    fn try_from(value: Uuid) -> Result<Self, Self::Error> {
        if value.is_nil() {
            Err(ReportJobError::NilIdentity)
        } else {
            Ok(Self(value))
        }
    }
}

impl ReportJobId {
    pub const fn value(self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportJobEnumState {
    Queued,
    Running,
    Pausing,
    Paused,
    Cancelling,
    Cancelled,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportJobEnumFreshness {
    Current,
    Outdated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportJobEnumPositionState {
    Pending,
    Preparing,
    InFlight(EngineSearchTag),
    Completed,
}

#[derive(Debug, Clone)]
pub enum ReportJobEnumInterruption {
    PauseRequested,
    CancellationRequested,
    Outdated,
    EngineStopped,
    EngineCancelled,
    EngineFailed(Arc<EngineSearchSessionError>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportJobProgress {
    completed: u32,
    total: u32,
}

impl ReportJobProgress {
    pub const fn completed(self) -> u32 {
        self.completed
    }
    pub const fn total(self) -> u32 {
        self.total
    }
    pub const fn pending(self) -> u32 {
        self.total - self.completed
    }
}

#[derive(Debug)]
pub(crate) struct ReportJobBinding {
    id: ReportJobId,
    pub(crate) source: ReportJobSource,
    pub(crate) configuration: ReportJobConfiguration,
}

#[derive(Debug, Clone)]
pub struct ReportJobAttempt {
    binding: Arc<ReportJobBinding>,
    index: u32,
    sequence: u64,
    position: EngineSearchPosition,
}

impl ReportJobAttempt {
    pub fn job(&self) -> ReportJobId {
        self.binding.id
    }
    pub const fn index(&self) -> u32 {
        self.index
    }
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn context(&self) -> EngineSearchContext {
        self.binding.source.context()
    }
    pub const fn position(&self) -> &EngineSearchPosition {
        &self.position
    }
    pub fn limit(&self) -> EngineSearchLimit {
        self.binding.configuration.budget().limit()
    }
}

#[derive(Debug, Clone)]
pub struct ReportJobSearchBinding {
    attempt: ReportJobAttempt,
    tag: EngineSearchTag,
}

impl ReportJobSearchBinding {
    pub const fn attempt(&self) -> &ReportJobAttempt {
        &self.attempt
    }
    pub const fn tag(&self) -> EngineSearchTag {
        self.tag
    }
}

#[derive(Debug)]
struct ReportJobActivePosition {
    attempt: ReportJobAttempt,
    tag: Option<EngineSearchTag>,
    memory_charge: u64,
}

#[derive(Debug)]
pub struct ReportJob {
    binding: Arc<ReportJobBinding>,
    state: ReportJobEnumState,
    freshness: ReportJobEnumFreshness,
    progress: ReportJobProgress,
    results: Box<[Option<ReportFindingPositionResult>]>,
    active: Option<ReportJobActivePosition>,
    sequence: u64,
    memory_charge_bytes: u64,
    interruption: Option<ReportJobEnumInterruption>,
}

impl ReportJob {
    const CAPABILITY_ENTRY_MEMORY_BYTES: usize = 128;
    pub fn new(
        id: ReportJobId,
        source: ReportJobSource,
        configuration: ReportJobConfiguration,
    ) -> Result<Self, ReportJobError> {
        if source.positions()[0].position().variant() != configuration.options().variant() {
            return Err(ReportJobError::SearchMismatch);
        }
        let count = source.positions().len();
        let total = u32::try_from(count).map_err(|_error| ReportJobError::CounterExhausted)?;
        let result_bytes = count
            .checked_mul(size_of::<Option<ReportFindingPositionResult>>())
            .ok_or(ReportJobSourceError::MemoryLimit)?;
        let limits = source.limits();
        let mut memory_charge_bytes = limits.charge(source.memory_charge_bytes(), result_bytes)?;
        memory_charge_bytes = limits.charge(
            memory_charge_bytes,
            size_of::<Self>() + size_of::<ReportJobBinding>() + size_of::<ReportJobConfiguration>(),
        )?;
        memory_charge_bytes =
            limits.charge(memory_charge_bytes, configuration.identity().name().len())?;
        memory_charge_bytes =
            limits.charge(memory_charge_bytes, configuration.identity().author().len())?;
        let tablebase_bytes = match configuration.options().tablebases() {
            EngineOptionsEnumTablebases::Unconfigured => 0,
            EngineOptionsEnumTablebases::Loaded { path, .. }
            | EngineOptionsEnumTablebases::Unavailable { path } => path.as_str().len(),
        };
        memory_charge_bytes = limits.charge(memory_charge_bytes, tablebase_bytes)?;
        memory_charge_bytes = limits.charge(
            memory_charge_bytes,
            EngineEnumOption::iter().len()
                .checked_mul(Self::CAPABILITY_ENTRY_MEMORY_BYTES)
                .ok_or(ReportJobSourceError::MemoryLimit)?,
        )?;
        Ok(Self {
            binding: Arc::new(ReportJobBinding {
                id,
                source,
                configuration,
            }),
            state: ReportJobEnumState::Queued,
            freshness: ReportJobEnumFreshness::Current,
            progress: ReportJobProgress {
                completed: 0,
                total,
            },
            results: std::iter::repeat_with(|| None).take(count).collect(),
            active: None,
            sequence: 0,
            memory_charge_bytes,
            interruption: None,
        })
    }

    pub fn id(&self) -> ReportJobId {
        self.binding.id
    }
    pub(crate) const fn binding(&self) -> &Arc<ReportJobBinding> {
        &self.binding
    }
    pub(crate) const fn sequence(&self) -> u64 {
        self.sequence
    }
    pub const fn interruption(&self) -> Option<&ReportJobEnumInterruption> {
        self.interruption.as_ref()
    }
    pub fn source(&self) -> &ReportJobSource {
        &self.binding.source
    }
    pub fn configuration(&self) -> &ReportJobConfiguration {
        &self.binding.configuration
    }
    pub const fn state(&self) -> ReportJobEnumState {
        self.state
    }
    pub const fn freshness(&self) -> ReportJobEnumFreshness {
        self.freshness
    }
    pub const fn progress(&self) -> ReportJobProgress {
        self.progress
    }
    pub fn memory_charge_bytes(&self) -> u64 {
        self.active
            .as_ref()
            .map_or(self.memory_charge_bytes, |active| active.memory_charge)
    }
    pub fn active_search(&self) -> Option<EngineSearchTag> {
        self.active.as_ref().and_then(|active| active.tag)
    }
    pub fn results(&self) -> &[Option<ReportFindingPositionResult>] {
        &self.results
    }

    pub fn position_state(&self, index: u32) -> Result<ReportJobEnumPositionState, ReportJobError> {
        let slot = usize::try_from(index).map_err(|_error| ReportJobError::PositionIndex)?;
        let result = self
            .results
            .get(slot)
            .ok_or(ReportJobError::PositionIndex)?;
        if result.is_some() {
            return Ok(ReportJobEnumPositionState::Completed);
        }
        Ok(
            match self
                .active
                .as_ref()
                .filter(|active| active.attempt.index == index)
            {
                Some(active) => match active.tag {
                    Some(tag) => ReportJobEnumPositionState::InFlight(tag),
                    None => ReportJobEnumPositionState::Preparing,
                },
                None => ReportJobEnumPositionState::Pending,
            },
        )
    }

    pub fn resume(&mut self) -> Result<(), ReportJobError> {
        self.require_current()?;
        match self.state {
            ReportJobEnumState::Queued | ReportJobEnumState::Paused => {
                self.state = ReportJobEnumState::Running;
                Ok(())
            }
            ReportJobEnumState::Running
            | ReportJobEnumState::Pausing
            | ReportJobEnumState::Cancelling
            | ReportJobEnumState::Cancelled
            | ReportJobEnumState::Completed => Err(ReportJobError::State(self.state)),
        }
    }

    pub fn start_position(&mut self, index: u32) -> Result<ReportJobAttempt, ReportJobError> {
        self.require_current()?;
        if self.state != ReportJobEnumState::Running || self.active.is_some() {
            return Err(ReportJobError::State(self.state));
        }
        let slot = usize::try_from(index).map_err(|_error| ReportJobError::PositionIndex)?;
        if self.results.get(slot).is_none_or(Option::is_some) {
            return Err(ReportJobError::PositionIndex);
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(ReportJobError::CounterExhausted)?;
        let history_bytes = slot
            .checked_mul(size_of::<BoardMove>())
            .and_then(|bytes| {
                bytes.checked_add(
                    EngineProcessLimits::LINE_BYTES
                        + size_of::<ReportJobAttempt>()
                        + 2 * size_of::<BoardPosition>()
                        + size_of::<ReportJobActivePosition>(),
                )
            })
            .ok_or(ReportJobSourceError::MemoryLimit)?;
        let memory_charge = self
            .source()
            .limits()
            .charge(self.memory_charge_bytes, history_bytes)?;
        let position = self.source().search_position(index)?;
        let attempt = ReportJobAttempt {
            binding: Arc::clone(&self.binding),
            index,
            sequence,
            position,
        };
        self.active = Some(ReportJobActivePosition {
            attempt: attempt.clone(),
            tag: None,
            memory_charge,
        });
        self.sequence = sequence;
        Ok(attempt)
    }

    pub fn bind_engine<P: EngineProcess>(
        &mut self,
        attempt: &ReportJobAttempt,
        session: &EngineSearchSession<P>,
    ) -> Result<ReportJobSearchBinding, ReportJobError> {
        self.require_current()?;
        self.validate_attempt(attempt)?;
        if self.state != ReportJobEnumState::Running {
            return Err(ReportJobError::State(self.state));
        }
        let metadata = session.metadata().ok_or(ReportJobError::SearchMismatch)?;
        if !self.configuration().matches_engine(session.engine())
            || metadata.tag().generation() != session.engine().generation()
            || session.engine().state() != EngineSearchEnumState::Searching
            || session.terminal().is_some()
            || session.cancellation() != EngineSearchSessionEnumCancellation::NotRequested
        {
            return Err(ReportJobError::SearchMismatch);
        }
        Self::validate_metadata(attempt, metadata)?;
        let active = self.active.as_mut().ok_or(ReportJobError::StaleAttempt)?;
        if active.tag.is_some() {
            return Err(ReportJobError::StaleAttempt);
        }
        active.tag = Some(metadata.tag());
        Ok(ReportJobSearchBinding {
            attempt: attempt.clone(),
            tag: metadata.tag(),
        })
    }

    pub fn record_terminal(
        &mut self,
        binding: &ReportJobSearchBinding,
        terminal: &EngineSearchSessionTerminal,
    ) -> Result<(), ReportJobError> {
        self.validate_attempt(&binding.attempt)?;
        let active = self.active.as_ref().ok_or(ReportJobError::StaleAttempt)?;
        let metadata = terminal.snapshot().metadata();
        if active.tag != Some(binding.tag) || metadata.tag() != binding.tag {
            return Err(ReportJobError::SearchMismatch);
        }
        Self::validate_metadata(&binding.attempt, metadata)?;
        let interrupted = match terminal.completion() {
            EngineSearchSessionEnumCompletion::BestMove(_) => {
                terminal.snapshot().cancellation()
                    != EngineSearchSessionEnumCancellation::NotRequested
            }
            EngineSearchSessionEnumCompletion::Stopped
            | EngineSearchSessionEnumCompletion::Cancelled
            | EngineSearchSessionEnumCompletion::Failed(_) => true,
        };
        if interrupted
            || self.state != ReportJobEnumState::Running
            || self.freshness == ReportJobEnumFreshness::Outdated
        {
            let interruption = match terminal.completion() {
                EngineSearchSessionEnumCompletion::Failed(error) => {
                    ReportJobEnumInterruption::EngineFailed(Arc::clone(error))
                }
                EngineSearchSessionEnumCompletion::Stopped => {
                    ReportJobEnumInterruption::EngineStopped
                }
                EngineSearchSessionEnumCompletion::Cancelled => {
                    ReportJobEnumInterruption::EngineCancelled
                }
                EngineSearchSessionEnumCompletion::BestMove(_) => {
                    if self.freshness == ReportJobEnumFreshness::Outdated {
                        ReportJobEnumInterruption::Outdated
                    } else if self.state == ReportJobEnumState::Cancelling {
                        ReportJobEnumInterruption::CancellationRequested
                    } else {
                        ReportJobEnumInterruption::PauseRequested
                    }
                }
            };
            return self.finish_interruption(interruption);
        }
        let lines = terminal.snapshot().lines();
        let latest = terminal.snapshot().latest_update();
        let primary = terminal.snapshot().primary_evaluation();
        let bytes = ReportFindingPositionResult::search_charge(
            lines.len(),
            latest.is_some(),
            primary.is_some(),
        )?;
        let retained = self
            .source()
            .limits()
            .charge(self.memory_charge_bytes, bytes)?;
        self.source()
            .limits()
            .charge(self.memory_charge_bytes(), bytes)?;
        let result = ReportFindingPositionResult::search(
            binding.tag,
            lines.to_vec().into_boxed_slice(),
            latest.map(|line| Arc::new(line.clone())),
            primary.cloned(),
        );
        self.commit(&binding.attempt, result, retained)
    }

    pub fn record_unavailable(&mut self, attempt: &ReportJobAttempt) -> Result<(), ReportJobError> {
        self.require_unbound(attempt)?;
        self.commit(
            attempt,
            ReportFindingPositionResult::unavailable(),
            self.memory_charge_bytes,
        )
    }

    pub fn record_outcome(
        &mut self,
        attempt: &ReportJobAttempt,
        outcome: GameOutcome,
    ) -> Result<(), ReportJobError> {
        self.require_unbound(attempt)?;
        let index =
            usize::try_from(attempt.index).map_err(|_error| ReportJobError::PositionIndex)?;
        let node = &self.source().positions()[index];
        if node.node() != self.source().tip() || outcome.origin() != node.node() {
            return Err(ReportJobError::OutcomeMismatch);
        }
        match outcome.result() {
            GameEnumResult::Draw => {}
            GameEnumResult::Victory(winner) => {
                if outcome.reason() != GameEnumOutcomeReason::Checkmate
                    || !node.position().is_checkmate()
                    || winner != node.position().turn().opposite()
                {
                    return Err(ReportJobError::OutcomeMismatch);
                }
            }
        }
        self.commit(
            attempt,
            ReportFindingPositionResult::terminal(outcome.result()),
            self.memory_charge_bytes,
        )
    }

    pub fn pause(&mut self) -> Result<(), ReportJobError> {
        match self.state {
            ReportJobEnumState::Queued | ReportJobEnumState::Running => {
                if self.active_search().is_some() {
                    self.state = ReportJobEnumState::Pausing;
                } else {
                    self.active = None;
                    self.state = ReportJobEnumState::Paused;
                }
                Ok(())
            }
            ReportJobEnumState::Paused => Ok(()),
            ReportJobEnumState::Pausing
            | ReportJobEnumState::Cancelling
            | ReportJobEnumState::Cancelled
            | ReportJobEnumState::Completed => Err(ReportJobError::State(self.state)),
        }
    }

    pub fn cancel(&mut self) -> Result<(), ReportJobError> {
        match self.state {
            ReportJobEnumState::Queued
            | ReportJobEnumState::Running
            | ReportJobEnumState::Pausing
            | ReportJobEnumState::Paused => {
                if self.active_search().is_some() {
                    self.state = ReportJobEnumState::Cancelling;
                } else {
                    self.active = None;
                    self.state = ReportJobEnumState::Cancelled;
                }
                Ok(())
            }
            ReportJobEnumState::Cancelled => Ok(()),
            ReportJobEnumState::Cancelling | ReportJobEnumState::Completed => {
                Err(ReportJobError::State(self.state))
            }
        }
    }

    pub fn recover_after_relaunch(&mut self) -> Result<(), ReportJobError> {
        if self.active_search().is_some() {
            return Err(ReportJobError::SearchStillOwned);
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(ReportJobError::CounterExhausted)?;
        self.active = None;
        self.state = match self.state {
            ReportJobEnumState::Completed => ReportJobEnumState::Completed,
            ReportJobEnumState::Cancelling | ReportJobEnumState::Cancelled => {
                ReportJobEnumState::Cancelled
            }
            ReportJobEnumState::Queued
            | ReportJobEnumState::Running
            | ReportJobEnumState::Pausing
            | ReportJobEnumState::Paused => ReportJobEnumState::Paused,
        };
        Ok(())
    }

    pub fn observe_source(&mut self, source: &ReportJobSource) -> Result<(), ReportJobError> {
        if self.source().game() != source.game() {
            return Err(ReportJobError::WrongGame);
        }
        if !self.source().is_same_line(source) {
            self.freshness = ReportJobEnumFreshness::Outdated;
            if matches!(
                self.state,
                ReportJobEnumState::Queued | ReportJobEnumState::Running
            ) {
                self.pause()?;
            }
        }
        Ok(())
    }

    pub fn finding(&self, index: u32) -> Result<ReportMoveFinding<'_>, ReportJobError> {
        let index = usize::try_from(index).map_err(|_error| ReportJobError::PositionIndex)?;
        let next = index.checked_add(1).ok_or(ReportJobError::PositionIndex)?;
        ReportMoveFinding::new(
            &self.binding,
            index,
            self.results.get(index).and_then(Option::as_ref),
            self.results.get(next).and_then(Option::as_ref),
        )
    }

    pub fn summary(&self) -> Result<ReportScoringSummary, ReportJobError> {
        let coverage = if self.state == ReportJobEnumState::Completed {
            ReportScoringEnumCoverage::Complete
        } else {
            ReportScoringEnumCoverage::Partial
        };
        let mut summary =
            ReportScoringSummary::new(self.configuration().policy(), coverage, std::iter::empty())?;
        for index in 0..self.progress.total() - 1 {
            summary.record(*self.finding(index)?.sample())?;
        }
        Ok(summary)
    }

    fn require_current(&self) -> Result<(), ReportJobError> {
        if self.freshness == ReportJobEnumFreshness::Outdated {
            Err(ReportJobError::Outdated)
        } else {
            Ok(())
        }
    }

    fn validate_attempt(&self, attempt: &ReportJobAttempt) -> Result<(), ReportJobError> {
        if !Arc::ptr_eq(&self.binding, &attempt.binding)
            || self.active.as_ref().is_none_or(|active| {
                active.attempt.sequence != attempt.sequence || active.attempt.index != attempt.index
            })
        {
            return Err(ReportJobError::StaleAttempt);
        }
        Ok(())
    }

    fn require_unbound(&self, attempt: &ReportJobAttempt) -> Result<(), ReportJobError> {
        self.require_current()?;
        self.validate_attempt(attempt)?;
        if self.state != ReportJobEnumState::Running || self.active_search().is_some() {
            return Err(ReportJobError::State(self.state));
        }
        Ok(())
    }

    fn validate_metadata(
        attempt: &ReportJobAttempt,
        metadata: &EngineSearchSessionMetadata,
    ) -> Result<(), ReportJobError> {
        if metadata.tag().context() != attempt.context()
            || metadata.limit() != attempt.limit()
            || metadata.position() != attempt.position()
            || metadata.position().initial().fen() != attempt.position().initial().fen()
            || metadata.position().current().fen() != attempt.position().current().fen()
        {
            return Err(ReportJobError::SearchMismatch);
        }
        Ok(())
    }

    fn finish_interruption(
        &mut self,
        interruption: ReportJobEnumInterruption,
    ) -> Result<(), ReportJobError> {
        let state = match self.state {
            ReportJobEnumState::Running | ReportJobEnumState::Pausing => ReportJobEnumState::Paused,
            ReportJobEnumState::Cancelling => ReportJobEnumState::Cancelled,
            ReportJobEnumState::Queued
            | ReportJobEnumState::Paused
            | ReportJobEnumState::Cancelled
            | ReportJobEnumState::Completed => return Err(ReportJobError::State(self.state)),
        };
        self.active = None;
        self.state = state;
        self.interruption = Some(interruption);
        Ok(())
    }

    fn commit(
        &mut self,
        attempt: &ReportJobAttempt,
        result: ReportFindingPositionResult,
        memory_charge_bytes: u64,
    ) -> Result<(), ReportJobError> {
        self.validate_attempt(attempt)?;
        let completed = self
            .progress
            .completed
            .checked_add(1)
            .filter(|completed| *completed <= self.progress.total)
            .ok_or(ReportJobError::CounterExhausted)?;
        let index =
            usize::try_from(attempt.index).map_err(|_error| ReportJobError::PositionIndex)?;
        let slot = self
            .results
            .get_mut(index)
            .filter(|slot| slot.is_none())
            .ok_or(ReportJobError::PositionIndex)?;
        *slot = Some(result);
        self.progress.completed = completed;
        self.interruption = None;
        self.memory_charge_bytes = memory_charge_bytes;
        self.active = None;
        if completed == self.progress.total {
            self.state = ReportJobEnumState::Completed;
        }
        Ok(())
    }
}
