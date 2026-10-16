use std::num::NonZeroU32;
use std::sync::Arc;

use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::clock::stage::{ClockStage, ClockStageEnumBonus, ClockStageEnumQuota};
use crate::clock::time_control::ClockTimeControl;

#[derive(Debug, Error)]
pub enum ClockStateError {
    #[error("This operation requires a running clock")]
    NotRunning,
    #[error("This operation requires a paused clock")]
    NotPaused,
    #[error("The clock snapshot belongs to a different time control")]
    DifferentControl,
    #[error("The clock stage or move quota is inconsistent")]
    Stage,
    #[error("The clock move counter exhausted its integer range")]
    MoveOverflow,
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockStateEnumStatus {
    Paused,
    Running,
    Flagged(BoardEnumColor),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClockStateEnumPhase {
    Paused,
    Running(ClockInstant),
    Flagged(BoardEnumColor),
}

impl ClockStateEnumPhase {
    const fn status(self) -> ClockStateEnumStatus {
        match self {
            Self::Paused => ClockStateEnumStatus::Paused,
            Self::Running(_) => ClockStateEnumStatus::Running,
            Self::Flagged(side) => ClockStateEnumStatus::Flagged(side),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClockStateSide {
    remaining: ClockDuration,
    completed_moves: u64,
    stage_index: usize,
    stage_moves: u64,
}

impl ClockStateSide {
    const fn index(side: BoardEnumColor) -> usize {
        match side {
            BoardEnumColor::White => 0,
            BoardEnumColor::Black => 1,
        }
    }

    fn moves_to_go(self, stage: ClockStage) -> Result<Option<NonZeroU32>, ClockStateError> {
        match stage.quota() {
            ClockStageEnumQuota::Unlimited => Ok(None),
            ClockStageEnumQuota::Moves(quota) | ClockStageEnumQuota::Cyclic(quota) => {
                let remaining = u64::from(quota.get())
                    .checked_sub(self.stage_moves)
                    .ok_or(ClockStateError::Stage)?;
                let remaining =
                    u32::try_from(remaining).map_err(|_error| ClockStateError::Stage)?;
                NonZeroU32::new(remaining)
                    .map(Some)
                    .ok_or(ClockStateError::Stage)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockStateSideView {
    remaining: Option<ClockDuration>,
    displayed: Option<ClockDuration>,
    delay_remaining: ClockDuration,
    pending_refund: ClockDuration,
    completed_moves: u64,
    stage_index: Option<usize>,
    moves_to_go: Option<NonZeroU32>,
}

impl ClockStateSideView {
    pub const fn remaining(self) -> Option<ClockDuration> {
        self.remaining
    }

    pub const fn displayed(self) -> Option<ClockDuration> {
        self.displayed
    }

    pub const fn delay_remaining(self) -> ClockDuration {
        self.delay_remaining
    }

    pub const fn pending_refund(self) -> ClockDuration {
        self.pending_refund
    }

    pub const fn completed_moves(self) -> u64 {
        self.completed_moves
    }

    pub const fn stage_index(self) -> Option<usize> {
        self.stage_index
    }

    pub const fn moves_to_go(self) -> Option<NonZeroU32> {
        self.moves_to_go
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockStateView {
    sides: [ClockStateSideView; 2],
    active: BoardEnumColor,
    status: ClockStateEnumStatus,
    elapsed: ClockDuration,
    deadline: Option<ClockInstant>,
}

impl ClockStateView {
    pub fn side(self, side: BoardEnumColor) -> ClockStateSideView {
        self.sides[ClockStateSide::index(side)]
    }

    pub const fn active(self) -> BoardEnumColor {
        self.active
    }

    pub const fn status(self) -> ClockStateEnumStatus {
        self.status
    }

    pub const fn elapsed(self) -> ClockDuration {
        self.elapsed
    }

    pub const fn deadline(self) -> Option<ClockInstant> {
        self.deadline
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockStateUpdate {
    view: ClockStateView,
    new_warning: Option<BoardEnumColor>,
}

impl ClockStateUpdate {
    pub const fn view(&self) -> &ClockStateView {
        &self.view
    }

    pub const fn new_warning(&self) -> Option<BoardEnumColor> {
        self.new_warning
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockStateSnapshot {
    control: Arc<ClockTimeControl>,
    sides: [ClockStateSide; 2],
    active: BoardEnumColor,
    elapsed: ClockDuration,
    status: ClockStateEnumStatus,
}

impl ClockStateSnapshot {
    pub const fn active(&self) -> BoardEnumColor {
        self.active
    }

    pub const fn status(&self) -> ClockStateEnumStatus {
        self.status
    }

    pub const fn elapsed(&self) -> ClockDuration {
        self.elapsed
    }

    pub fn completed_moves(&self, side: BoardEnumColor) -> u64 {
        self.sides[ClockStateSide::index(side)].completed_moves
    }

    pub fn stage_index(&self, side: BoardEnumColor) -> Option<usize> {
        self.control
            .is_timed()
            .then_some(self.sides[ClockStateSide::index(side)].stage_index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockStateMove {
    before: ClockStateSnapshot,
    after: ClockStateSnapshot,
    update: ClockStateUpdate,
}

impl ClockStateMove {
    pub const fn before(&self) -> &ClockStateSnapshot {
        &self.before
    }

    pub const fn after(&self) -> &ClockStateSnapshot {
        &self.after
    }

    pub const fn update(&self) -> &ClockStateUpdate {
        &self.update
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockStateEnumMoveOutcome {
    Accepted(ClockStateMove),
    Expired(ClockStateUpdate),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockState {
    control: Arc<ClockTimeControl>,
    sides: [ClockStateSide; 2],
    active: BoardEnumColor,
    elapsed: ClockDuration,
    phase: ClockStateEnumPhase,
    last_observed: Option<ClockInstant>,
    warnings_used: [bool; 2],
}

impl ClockState {
    const LOW_TIME: ClockDuration = ClockDuration::from_nanos(10_000_000_000);

    pub fn new(control: ClockTimeControl, active: BoardEnumColor) -> Self {
        let remaining = match control.stages().first() {
            Some(stage) => stage.base_time(),
            None => ClockDuration::ZERO,
        };
        Self {
            control: Arc::new(control),
            sides: [ClockStateSide {
                remaining,
                completed_moves: 0,
                stage_index: 0,
                stage_moves: 0,
            }; 2],
            active,
            elapsed: ClockDuration::ZERO,
            phase: ClockStateEnumPhase::Paused,
            last_observed: None,
            warnings_used: [false; 2],
        }
    }

    pub fn control(&self) -> &ClockTimeControl {
        &self.control
    }

    pub const fn active(&self) -> BoardEnumColor {
        self.active
    }

    pub const fn status(&self) -> ClockStateEnumStatus {
        self.phase.status()
    }

    pub const fn is_running(&self) -> bool {
        matches!(self.phase, ClockStateEnumPhase::Running(_))
    }

    pub fn warning_used(&self, side: BoardEnumColor) -> bool {
        self.warnings_used[ClockStateSide::index(side)]
    }

    pub fn merge_warning_used(&mut self, side: BoardEnumColor) {
        self.warnings_used[ClockStateSide::index(side)] = true;
    }

    pub fn view(&self, now: ClockInstant) -> Result<ClockStateView, ClockStateError> {
        self.validate_time(now)?;
        let elapsed = match self.phase {
            ClockStateEnumPhase::Running(since) => {
                self.elapsed.checked_add(now.elapsed_since(since)?)?
            }
            ClockStateEnumPhase::Paused | ClockStateEnumPhase::Flagged(_) => self.elapsed,
        };
        let mut status = self.status();
        let mut deadline = None;
        if let Some(stage) = self.stage(self.active)? {
            let allowance = self.sides[ClockStateSide::index(self.active)]
                .remaining
                .checked_add(stage.bonus().delay())?;
            if elapsed >= allowance {
                status = ClockStateEnumStatus::Flagged(self.active);
            }
            if let ClockStateEnumPhase::Running(since) = self.phase {
                deadline = Some(since.checked_add(allowance.checked_sub(self.elapsed)?)?);
            }
        }
        Ok(ClockStateView {
            sides: [
                self.side_view(BoardEnumColor::White, elapsed)?,
                self.side_view(BoardEnumColor::Black, elapsed)?,
            ],
            active: self.active,
            status,
            elapsed,
            deadline,
        })
    }

    pub fn snapshot(&self, now: ClockInstant) -> Result<ClockStateSnapshot, ClockStateError> {
        let view = self.view(now)?;
        Ok(ClockStateSnapshot {
            control: Arc::clone(&self.control),
            sides: self.sides,
            active: self.active,
            elapsed: view.elapsed,
            status: view.status,
        })
    }

    pub fn restore(&mut self, snapshot: &ClockStateSnapshot) -> Result<(), ClockStateError> {
        if self.control != snapshot.control {
            return Err(ClockStateError::DifferentControl);
        }
        self.sides = snapshot.sides;
        self.active = snapshot.active;
        self.elapsed = snapshot.elapsed;
        self.phase = match snapshot.status {
            ClockStateEnumStatus::Paused | ClockStateEnumStatus::Running => {
                ClockStateEnumPhase::Paused
            }
            ClockStateEnumStatus::Flagged(side) => ClockStateEnumPhase::Flagged(side),
        };
        self.last_observed = None;
        Ok(())
    }

    pub fn resume(&mut self, now: ClockInstant) -> Result<ClockStateUpdate, ClockStateError> {
        if self.phase != ClockStateEnumPhase::Paused {
            return Err(ClockStateError::NotPaused);
        }
        self.validate_time(now)?;
        let mut candidate = self.clone();
        candidate.phase = ClockStateEnumPhase::Running(now);
        let update = candidate.poll(now)?;
        *self = candidate;
        Ok(update)
    }

    pub fn pause(&mut self, now: ClockInstant) -> Result<ClockStateUpdate, ClockStateError> {
        if !self.is_running() {
            return Err(ClockStateError::NotRunning);
        }
        let mut candidate = self.clone();
        let mut update = candidate.poll(now)?;
        if candidate.is_running() {
            candidate.elapsed = update.view.elapsed;
            candidate.phase = ClockStateEnumPhase::Paused;
            update.view.status = ClockStateEnumStatus::Paused;
            update.view.deadline = None;
        }
        *self = candidate;
        Ok(update)
    }

    pub fn poll(&mut self, now: ClockInstant) -> Result<ClockStateUpdate, ClockStateError> {
        let mut view = self.view(now)?;
        let new_warning = if self.is_running()
            && !self.warning_used(self.active)
            && view
                .side(self.active)
                .displayed()
                .is_some_and(|time| time < Self::LOW_TIME)
        {
            Some(self.active)
        } else {
            None
        };
        if let Some(side) = new_warning {
            self.merge_warning_used(side);
        }
        if let ClockStateEnumStatus::Flagged(side) = view.status {
            self.elapsed = view.elapsed;
            self.phase = ClockStateEnumPhase::Flagged(side);
            view.deadline = None;
        }
        self.last_observed = Some(now);
        Ok(ClockStateUpdate { view, new_warning })
    }

    pub fn complete_move(
        &mut self,
        now: ClockInstant,
    ) -> Result<ClockStateEnumMoveOutcome, ClockStateError> {
        if !self.is_running() {
            return Err(ClockStateError::NotRunning);
        }
        let mut candidate = self.clone();
        let mut update = candidate.poll(now)?;
        if matches!(update.view.status, ClockStateEnumStatus::Flagged(_)) {
            *self = candidate;
            return Ok(ClockStateEnumMoveOutcome::Expired(update));
        }
        let before = candidate.snapshot(now)?;
        candidate.apply_move(now, update.view)?;
        update.view = candidate.view(now)?;
        let after = candidate.snapshot(now)?;
        *self = candidate;
        Ok(ClockStateEnumMoveOutcome::Accepted(ClockStateMove {
            before,
            after,
            update,
        }))
    }

    fn apply_move(
        &mut self,
        now: ClockInstant,
        view: ClockStateView,
    ) -> Result<(), ClockStateError> {
        let index = ClockStateSide::index(self.active);
        let mut side = self.sides[index];
        side.completed_moves = side
            .completed_moves
            .checked_add(1)
            .ok_or(ClockStateError::MoveOverflow)?;
        if let Some(stage) = self.stage(self.active)? {
            side.remaining = view
                .side(self.active)
                .remaining
                .ok_or(ClockStateError::Stage)?
                .checked_add(stage.bonus().increment())?;
            side.stage_moves = side
                .stage_moves
                .checked_add(1)
                .ok_or(ClockStateError::MoveOverflow)?;
            match stage.quota() {
                ClockStageEnumQuota::Moves(quota) if side.stage_moves == u64::from(quota.get()) => {
                    side.stage_index = side
                        .stage_index
                        .checked_add(1)
                        .ok_or(ClockStateError::Stage)?;
                    let next = self
                        .control
                        .stages()
                        .get(side.stage_index)
                        .ok_or(ClockStateError::Stage)?;
                    side.remaining = side.remaining.checked_add(next.base_time())?;
                    side.stage_moves = 0;
                }
                ClockStageEnumQuota::Cyclic(quota)
                    if side.stage_moves == u64::from(quota.get()) =>
                {
                    side.remaining = side.remaining.checked_add(stage.base_time())?;
                    side.stage_moves = 0;
                }
                ClockStageEnumQuota::Moves(_)
                | ClockStageEnumQuota::Cyclic(_)
                | ClockStageEnumQuota::Unlimited => {}
            }
            let next = self
                .control
                .stages()
                .get(side.stage_index)
                .ok_or(ClockStateError::Stage)?;
            side.remaining.checked_add(next.bonus().delay())?;
        }
        self.sides[index] = side;
        self.active = self.active.opposite();
        self.elapsed = ClockDuration::ZERO;
        self.phase = ClockStateEnumPhase::Running(now);
        self.last_observed = Some(now);
        Ok(())
    }

    fn validate_time(&self, now: ClockInstant) -> Result<(), ClockStateError> {
        if let Some(previous) = self.last_observed {
            now.elapsed_since(previous)?;
        }
        Ok(())
    }

    fn stage(&self, side: BoardEnumColor) -> Result<Option<ClockStage>, ClockStateError> {
        if !self.control.is_timed() {
            return Ok(None);
        }
        self.control
            .stages()
            .get(self.sides[ClockStateSide::index(side)].stage_index)
            .copied()
            .map(Some)
            .ok_or(ClockStateError::Stage)
    }

    fn side_view(
        &self,
        color: BoardEnumColor,
        elapsed: ClockDuration,
    ) -> Result<ClockStateSideView, ClockStateError> {
        let side = self.sides[ClockStateSide::index(color)];
        let mut view = ClockStateSideView {
            remaining: None,
            displayed: None,
            delay_remaining: ClockDuration::ZERO,
            pending_refund: ClockDuration::ZERO,
            completed_moves: side.completed_moves,
            stage_index: None,
            moves_to_go: None,
        };
        if let Some(stage) = self.stage(color)? {
            let elapsed = if color == self.active {
                elapsed
            } else {
                ClockDuration::ZERO
            };
            let delay = stage.bonus().delay();
            let charge = Self::subtract_saturating(elapsed, delay);
            let remaining = Self::subtract_saturating(side.remaining, charge);
            view.remaining = Some(remaining);
            view.displayed = Some(match stage.bonus() {
                ClockStageEnumBonus::BronsteinDelay(_) => {
                    Self::subtract_saturating(side.remaining, elapsed)
                }
                ClockStageEnumBonus::None
                | ClockStageEnumBonus::Fischer(_)
                | ClockStageEnumBonus::SimpleDelay(_) => remaining,
            });
            if color == self.active {
                view.delay_remaining = Self::subtract_saturating(delay, elapsed);
                if matches!(stage.bonus(), ClockStageEnumBonus::BronsteinDelay(_)) {
                    view.pending_refund = elapsed.min(delay);
                }
            }
            view.stage_index = Some(side.stage_index);
            view.moves_to_go = side.moves_to_go(stage)?;
        }
        Ok(view)
    }

    fn subtract_saturating(left: ClockDuration, right: ClockDuration) -> ClockDuration {
        ClockDuration::from_nanos(left.as_nanos().saturating_sub(right.as_nanos()))
    }
}
