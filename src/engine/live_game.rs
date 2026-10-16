use std::sync::Arc;
use std::time::Instant;

use thiserror::Error;
use tokio::time::{Instant as RuntimeInstant, timeout_at};

use crate::board::enums::BoardEnumColor;
use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::clock::stage::ClockStageEnumBonus;
use crate::clock::state::{
    ClockStateEnumStatus, ClockStateError, ClockStateSideView, ClockStateView,
};
use crate::clock::time_provider::ClockTimeProvider;
use crate::engine::process_provider::{EngineProcess, EngineProcessLimits};
use crate::engine::resource_config::EngineResourceConfig;
use crate::engine::search::enums::{EngineSearchEnumOutput, EngineSearchEnumState};
use crate::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchIdentityError, EngineSearchRevision,
    EngineSearchTag,
};
use crate::engine::search::limit::{EngineSearchClock, EngineSearchLimit, EngineSearchLimitError};
use crate::engine::search::position::{EngineSearchPosition, EngineSearchPositionError};
use crate::engine::search::stop::{EngineSearchStop, EngineSearchStopError};
use crate::engine::session::{EngineSession, EngineSessionError};
use crate::game::engine_provider::{
    GameEngineContext, GameEngineEnumDiscard, GameEngineEnumEvent, GameEngineMove,
    GameEngineProvider, GameEngineQuiescenceReceipt, GameEngineRequest, GameEngineSearchIdentity,
    GameEngineStopReceipt,
};
use crate::game::enums::GameId;

#[derive(Debug, Error)]
pub enum EngineLiveGameError {
    #[error("The live-game engine is not idle: {0:?}")]
    NotIdle(EngineSearchEnumState),
    #[error("The live-game engine has no active search")]
    NoSearch,
    #[error("The live-game engine faulted and requires an explicit restart")]
    Retired,
    #[error("The live-game request or stop belongs to a different search context")]
    StaleSearch,
    #[error("The live-game clock is paused or has already expired")]
    ClockInactive,
    #[error("The captured clock has no valid timed stage")]
    ClockStage,
    #[error("The available turn time cannot safely cover the configured move overhead")]
    UnsafeBudget,
    #[error("The engine turn deadline exceeds the runtime clock range")]
    RuntimeRange,
    #[error("Engine turn setup reached the native clock deadline: {0}")]
    SetupDeadline(#[source] tokio::time::error::Elapsed),
    #[error(
        "Reckless did not finish its untimed {budget:?} search within the stop-safety grace period"
    )]
    SearchDeadline { budget: ClockDuration },
    #[error(
        "Live-game engine operation failed ({operation}); retained cleanup also failed ({cleanup})"
    )]
    Cleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
    #[error(transparent)]
    Clock(#[from] ClockStateError),
    #[error(transparent)]
    Position(#[from] EngineSearchPositionError),
    #[error(transparent)]
    Limit(#[from] EngineSearchLimitError),
    #[error(transparent)]
    Identity(#[from] EngineSearchIdentityError),
    #[error(transparent)]
    Session(#[from] EngineSessionError),
    #[error(transparent)]
    Stop(#[from] EngineSearchStopError),
}

#[derive(Debug)]
pub struct EngineLiveGameSearchPlan {
    position: EngineSearchPosition,
    limit: EngineSearchLimit,
    deadline: Option<ClockInstant>,
}

impl EngineLiveGameSearchPlan {
    const MINIMUM_MOVE_TIME: ClockDuration = ClockDuration::from_nanos(1_000_000);
    const DELAY_MOVE_DIVISOR: u64 = 30;

    pub fn new(
        request: &GameEngineRequest,
        now: ClockInstant,
    ) -> Result<Self, EngineLiveGameError> {
        let view = request.clock().view(now)?;
        if view.status() != ClockStateEnumStatus::Running
            || view
                .deadline()
                .is_some_and(|deadline| now.has_reached(deadline))
        {
            return Err(EngineLiveGameError::ClockInactive);
        }
        let variant = request.initial().variant();
        let position = EngineSearchPosition::new(
            request.initial().clone(),
            request
                .history()
                .iter()
                .map(|movement| movement.uci(variant)),
        )?;
        let limit = if request.clock().control().is_timed() {
            Self::timed_limit(request, view)?
        } else {
            EngineSearchLimit::move_time(request.config().untimed_move_time())?
        };
        Ok(Self {
            position,
            limit,
            deadline: view.deadline(),
        })
    }

    pub const fn position(&self) -> &EngineSearchPosition {
        &self.position
    }

    pub const fn limit(&self) -> EngineSearchLimit {
        self.limit
    }

    pub const fn deadline(&self) -> Option<ClockInstant> {
        self.deadline
    }

    fn bonus(
        request: &GameEngineRequest,
        side: ClockStateSideView,
    ) -> Result<ClockStageEnumBonus, EngineLiveGameError> {
        side.stage_index()
            .and_then(|index| request.clock().control().stages().get(index))
            .map(|stage| stage.bonus())
            .ok_or(EngineLiveGameError::ClockStage)
    }

    fn remaining(side: ClockStateSideView) -> Result<ClockDuration, EngineLiveGameError> {
        side.remaining().ok_or(EngineLiveGameError::ClockStage)
    }

    fn timed_limit(
        request: &GameEngineRequest,
        view: ClockStateView,
    ) -> Result<EngineSearchLimit, EngineLiveGameError> {
        let active = view.side(view.active());
        match Self::bonus(request, active)? {
            ClockStageEnumBonus::SimpleDelay(_) | ClockStageEnumBonus::BronsteinDelay(_) => {
                Self::delay_limit(active, request.config().move_overhead())
            }
            ClockStageEnumBonus::None | ClockStageEnumBonus::Fischer(_) => {
                let white = view.side(BoardEnumColor::White);
                let black = view.side(BoardEnumColor::Black);
                Ok(EngineSearchLimit::clock(EngineSearchClock::new(
                    Self::remaining(white)?,
                    Self::remaining(black)?,
                    Self::bonus(request, white)?.increment(),
                    Self::bonus(request, black)?.increment(),
                    active.moves_to_go(),
                )?))
            }
        }
    }

    fn delay_limit(
        active: ClockStateSideView,
        overhead: ClockDuration,
    ) -> Result<EngineSearchLimit, EngineLiveGameError> {
        let remaining = Self::remaining(active)?;
        let unused_delay = active.delay_remaining();
        let available = remaining.checked_add(unused_delay)?;
        if available <= overhead {
            return Err(EngineLiveGameError::UnsafeBudget);
        }
        let safe = available.checked_sub(overhead)?;
        let share = ClockDuration::from_nanos(remaining.as_nanos() / Self::DELAY_MOVE_DIVISOR)
            .max(Self::MINIMUM_MOVE_TIME);
        let estimate = ClockDuration::from_nanos(
            share
                .checked_add(unused_delay)?
                .as_nanos()
                .saturating_sub(overhead.as_nanos()),
        );
        Ok(EngineSearchLimit::move_time(
            safe.min(estimate).max(Self::MINIMUM_MOVE_TIME),
        )?)
    }
}

#[derive(Clone, Copy)]
enum EngineLiveGameEnumDelivery {
    Eligible,
    Invalidated,
}

#[derive(Clone, Copy)]
enum EngineLiveGameEnumDeadline {
    Clock(ClockInstant),
    MoveTime {
        runtime: RuntimeInstant,
        budget: ClockDuration,
    },
}

impl EngineLiveGameEnumDeadline {
    fn new(
        deadline: Option<ClockInstant>,
        budget: ClockDuration,
    ) -> Result<Self, EngineLiveGameError> {
        match deadline {
            Some(deadline) => Ok(Self::Clock(deadline)),
            None => {
                let runtime = RuntimeInstant::now()
                    .checked_add(budget.as_duration())
                    .and_then(|deadline| deadline.checked_add(EngineSearchStop::SAFETY))
                    .ok_or(EngineLiveGameError::RuntimeRange)?;
                Ok(Self::MoveTime { runtime, budget })
            }
        }
    }

    fn clock(self) -> Option<ClockInstant> {
        match self {
            Self::Clock(deadline) => Some(deadline),
            Self::MoveTime { .. } => None,
        }
    }

    fn expired_watchdog(self) -> Option<ClockDuration> {
        match self {
            Self::Clock(_) => None,
            Self::MoveTime { runtime, budget } => {
                (RuntimeInstant::now() >= runtime).then_some(budget)
            }
        }
    }

    fn clock_runtime(
        deadline: ClockInstant,
        now: ClockInstant,
    ) -> Result<RuntimeInstant, EngineLiveGameError> {
        RuntimeInstant::now()
            .checked_add(deadline.elapsed_since(now)?.as_duration())
            .ok_or(EngineLiveGameError::RuntimeRange)
    }

    fn runtime(self, now: ClockInstant) -> Result<RuntimeInstant, EngineLiveGameError> {
        match self {
            Self::Clock(deadline) => Self::clock_runtime(deadline, now),
            Self::MoveTime { runtime, .. } => Ok(runtime),
        }
    }
}

#[derive(Clone, Copy)]
struct EngineLiveGameSearch {
    tag: EngineSearchTag,
    identity: GameEngineSearchIdentity,
    deadline: EngineLiveGameEnumDeadline,
    delivery: EngineLiveGameEnumDelivery,
}

impl EngineLiveGameSearch {
    fn new(
        tag: EngineSearchTag,
        context: GameEngineContext,
        deadline: EngineLiveGameEnumDeadline,
    ) -> Self {
        Self {
            tag,
            identity: GameEngineSearchIdentity::new(
                context,
                tag.id().value(),
                tag.generation().value(),
            ),
            deadline,
            delivery: EngineLiveGameEnumDelivery::Eligible,
        }
    }

    fn discard(
        self,
        expected: GameEngineContext,
        now: ClockInstant,
    ) -> Option<GameEngineEnumDiscard> {
        if matches!(self.delivery, EngineLiveGameEnumDelivery::Invalidated) {
            Some(GameEngineEnumDiscard::Cancelled)
        } else if self.identity.context() != expected {
            Some(GameEngineEnumDiscard::StaleContext)
        } else if self
            .deadline
            .clock()
            .is_some_and(|deadline| now.has_reached(deadline))
        {
            Some(GameEngineEnumDiscard::Deadline)
        } else {
            None
        }
    }
}

enum EngineLiveGameEnumAvailability {
    Usable,
    Faulted,
}

pub struct EngineLiveGame<P: EngineProcess, C: ClockTimeProvider> {
    session: EngineSession<P>,
    clock: Arc<C>,
    prepared_game: Option<GameId>,
    availability: EngineLiveGameEnumAvailability,
    active: Option<EngineLiveGameSearch>,
    last_barrier: Option<GameEngineStopReceipt>,
}

impl<P: EngineProcess, C: ClockTimeProvider> EngineLiveGame<P, C> {
    pub const fn new(session: EngineSession<P>, clock: Arc<C>) -> Self {
        Self {
            session,
            clock,
            prepared_game: None,
            availability: EngineLiveGameEnumAvailability::Usable,
            active: None,
            last_barrier: None,
        }
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.session.is_shutdown_complete()
    }

    pub fn resources(&self) -> Option<EngineResourceConfig> {
        self.session.resources()
    }

    pub async fn configure_resources(
        &mut self,
        config: EngineResourceConfig,
    ) -> Result<(), EngineLiveGameError> {
        self.require_idle()?;
        if let Err(error) = self.session.configure_resources(config).await {
            self.availability = EngineLiveGameEnumAvailability::Faulted;
            return Err(error.into());
        }
        Ok(())
    }

    pub async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), EngineLiveGameError> {
        self.invalidate();
        let result = self
            .session
            .shutdown_retained(RuntimeInstant::from_std(deadline))
            .await;
        if self.session.is_shutdown_complete() {
            self.active = None;
            self.last_barrier = None;
            self.prepared_game = None;
        }
        result.map_err(EngineLiveGameError::Session)
    }

    fn invalidate(&mut self) {
        if let Some(active) = &mut self.active {
            active.delivery = EngineLiveGameEnumDelivery::Invalidated;
        }
    }

    fn complete(&mut self, identity: GameEngineSearchIdentity) -> GameEngineStopReceipt {
        let receipt = GameEngineStopReceipt::new(identity);
        self.active = None;
        self.last_barrier = Some(receipt);
        receipt
    }

    fn require_idle(&self) -> Result<(), EngineLiveGameError> {
        if matches!(self.availability, EngineLiveGameEnumAvailability::Faulted) {
            return Err(EngineLiveGameError::Retired);
        }
        if self.is_idle() {
            Ok(())
        } else {
            Err(EngineLiveGameError::NotIdle(self.session.state()))
        }
    }

    async fn prepare(
        &mut self,
        request: &GameEngineRequest,
    ) -> Result<(EngineSearchTag, Option<ClockInstant>), EngineLiveGameError> {
        if self.prepared_game != Some(request.context().game()) {
            self.session.new_game(request.initial().variant()).await?;
            self.prepared_game = Some(request.context().game());
        }
        let plan = EngineLiveGameSearchPlan::new(request, self.clock.now()?)?;
        let context = EngineSearchContext::new(
            EngineSearchGameId::try_from(request.context().game().as_uuid())?,
            EngineSearchRevision::new(request.context().revision().value()),
        );
        let tag = self
            .session
            .start_search(context, plan.position, plan.limit)
            .await?;
        Ok((tag, plan.deadline))
    }

    async fn discard_at_deadline(
        &mut self,
        active: EngineLiveGameSearch,
    ) -> Result<GameEngineEnumEvent, EngineLiveGameError> {
        self.stop(active.identity, RuntimeInstant::now().into_std())
            .await?;
        Ok(GameEngineEnumEvent::Discarded {
            identity: active.identity,
            reason: GameEngineEnumDiscard::Deadline,
        })
    }

    async fn fail_untimed_watchdog(
        &mut self,
        active: EngineLiveGameSearch,
        budget: ClockDuration,
    ) -> EngineLiveGameError {
        self.availability = EngineLiveGameEnumAvailability::Faulted;
        let operation = EngineLiveGameError::SearchDeadline { budget };
        let operation = if self.active.is_some() {
            match self
                .stop(active.identity, RuntimeInstant::now().into_std())
                .await
            {
                Ok(_) => operation,
                Err(cleanup) => EngineLiveGameError::Cleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                },
            }
        } else {
            operation
        };
        self.cleanup_after_failure(operation).await
    }

    async fn cleanup_after_failure(
        &mut self,
        operation: EngineLiveGameError,
    ) -> EngineLiveGameError {
        self.availability = EngineLiveGameEnumAvailability::Faulted;
        match self
            .shutdown_retained((RuntimeInstant::now() + EngineProcessLimits::SHUTDOWN).into_std())
            .await
        {
            Ok(()) => operation,
            Err(cleanup) => EngineLiveGameError::Cleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            },
        }
    }
}

impl<P: EngineProcess, C: ClockTimeProvider> GameEngineProvider for EngineLiveGame<P, C> {
    type Error = EngineLiveGameError;

    fn is_idle(&self) -> bool {
        matches!(self.availability, EngineLiveGameEnumAvailability::Usable)
            && self.active.is_none()
            && self.session.state() == EngineSearchEnumState::Idle
            && !self.session.is_shutdown_complete()
    }

    async fn start(
        &mut self,
        request: GameEngineRequest,
    ) -> Result<GameEngineSearchIdentity, Self::Error> {
        self.require_idle()?;
        let now = self.clock.now()?;
        let plan = EngineLiveGameSearchPlan::new(&request, now)?;
        let runtime_deadline = plan
            .deadline()
            .map(|deadline| EngineLiveGameEnumDeadline::clock_runtime(deadline, now))
            .transpose()?;
        let prepared = match runtime_deadline {
            Some(deadline) => match timeout_at(deadline, self.prepare(&request)).await {
                Ok(result) => result,
                Err(elapsed) => {
                    return Err(self
                        .cleanup_after_failure(EngineLiveGameError::SetupDeadline(elapsed))
                        .await);
                }
            },
            None => self.prepare(&request).await,
        };
        let (tag, deadline) = prepared?;
        let deadline =
            match EngineLiveGameEnumDeadline::new(deadline, request.config().untimed_move_time()) {
                Ok(deadline) => deadline,
                Err(operation) => return Err(self.cleanup_after_failure(operation).await),
            };
        let active = EngineLiveGameSearch::new(tag, request.context(), deadline);
        self.active = Some(active);
        self.last_barrier = None;
        let received_at = match self.clock.now() {
            Ok(received_at) => received_at,
            Err(error) => {
                self.invalidate();
                return Err(error.into());
            }
        };
        if deadline
            .clock()
            .is_some_and(|deadline| received_at.has_reached(deadline))
        {
            self.stop(active.identity, RuntimeInstant::now().into_std())
                .await?;
            return Err(EngineLiveGameError::ClockInactive);
        }
        Ok(active.identity)
    }

    async fn next_event(
        &mut self,
        expected: GameEngineContext,
    ) -> Result<GameEngineEnumEvent, Self::Error> {
        let active = self.active.ok_or(EngineLiveGameError::NoSearch)?;
        let now = self.clock.now()?;
        if active
            .deadline
            .clock()
            .is_some_and(|deadline| now.has_reached(deadline))
        {
            return self.discard_at_deadline(active).await;
        }
        if let Some(budget) = active.deadline.expired_watchdog() {
            return Err(self.fail_untimed_watchdog(active, budget).await);
        }
        let event = match timeout_at(active.deadline.runtime(now)?, self.session.next_event()).await
        {
            Ok(event) => event?,
            Err(_elapsed) => match active.deadline {
                EngineLiveGameEnumDeadline::Clock(_) => {
                    return self.discard_at_deadline(active).await;
                }
                EngineLiveGameEnumDeadline::MoveTime { budget, .. } => {
                    return Err(self.fail_untimed_watchdog(active, budget).await);
                }
            },
        };
        if event.tag() != active.tag {
            return Err(EngineLiveGameError::StaleSearch);
        }
        if let Some(budget) = active.deadline.expired_watchdog() {
            if matches!(
                event.output(),
                EngineSearchEnumOutput::BestMove(_) | EngineSearchEnumOutput::Stopped
            ) {
                self.complete(active.identity);
            }
            return Err(self.fail_untimed_watchdog(active, budget).await);
        }
        match event.into_output() {
            EngineSearchEnumOutput::Info(analysis) => Ok(GameEngineEnumEvent::Info {
                identity: active.identity,
                text: analysis.into_text(),
            }),
            EngineSearchEnumOutput::Diagnostic { pipe: _, text } => {
                Ok(GameEngineEnumEvent::Diagnostic {
                    identity: active.identity,
                    text,
                })
            }
            EngineSearchEnumOutput::Readiness => Ok(GameEngineEnumEvent::Readiness {
                identity: active.identity,
            }),
            EngineSearchEnumOutput::BestMove(movement) => {
                self.complete(active.identity);
                let received_at = self.clock.now()?;
                match active.discard(expected, received_at) {
                    Some(reason) => Ok(GameEngineEnumEvent::Discarded {
                        identity: active.identity,
                        reason,
                    }),
                    None => Ok(GameEngineEnumEvent::Move(GameEngineMove::new(
                        active.identity,
                        movement,
                        received_at,
                    ))),
                }
            }
            EngineSearchEnumOutput::Stopped => {
                Ok(GameEngineEnumEvent::Stopped(self.complete(active.identity)))
            }
        }
    }

    async fn quiesce(
        &mut self,
        expected: GameEngineContext,
        received_at: Instant,
    ) -> Result<GameEngineQuiescenceReceipt, Self::Error> {
        let stopped_search = match self.active {
            Some(active) => {
                if active.identity.context() != expected {
                    return Err(EngineLiveGameError::StaleSearch);
                }
                Some(self.stop(active.identity, received_at).await?.identity())
            }
            None => {
                self.require_idle()?;
                None
            }
        };
        Ok(GameEngineQuiescenceReceipt::new(
            expected,
            self.session.generation().value(),
            stopped_search,
        ))
    }

    async fn stop(
        &mut self,
        identity: GameEngineSearchIdentity,
        received_at: Instant,
    ) -> Result<GameEngineStopReceipt, Self::Error> {
        let active = match self.active {
            Some(active) => active,
            None => {
                self.require_idle()?;
                return self
                    .last_barrier
                    .filter(|receipt| receipt.identity() == identity)
                    .ok_or(EngineLiveGameError::StaleSearch);
            }
        };
        if active.identity != identity {
            return Err(EngineLiveGameError::StaleSearch);
        }
        self.invalidate();
        let request =
            EngineSearchStop::received_at(active.tag, RuntimeInstant::from_std(received_at))?;
        if self.session.stop(request).await? != active.tag {
            return Err(EngineLiveGameError::StaleSearch);
        }
        Ok(self.complete(identity))
    }
}
