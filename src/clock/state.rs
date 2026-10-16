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
    #[error("The saved clock stage counters do not match its completed moves")]
    SnapshotCounters,
    #[error("The saved clock balance exceeds the time earned by its completed stages and moves")]
    SnapshotBalance,
    #[error("The saved clock status does not match its active side and elapsed time")]
    SnapshotStatus,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockStateSideOptions {
    pub remaining: ClockDuration,
    pub completed_moves: u64,
    pub stage_index: usize,
    pub stage_moves: u64,
}

impl ClockStateSideOptions {
    fn validate(self, control: &ClockTimeControl) -> Result<ClockStateSide, ClockStateError> {
        if control.is_timed() {
            self.validate_timed(control)?;
        } else if self.remaining != ClockDuration::ZERO
            || self.stage_index != 0
            || self.stage_moves != 0
        {
            return Err(ClockStateError::SnapshotCounters);
        }
        Ok(ClockStateSide {
            remaining: self.remaining,
            completed_moves: self.completed_moves,
            stage_index: self.stage_index,
            stage_moves: self.stage_moves,
        })
    }

    fn validate_timed(self, control: &ClockTimeControl) -> Result<(), ClockStateError> {
        let current = control
            .stages()
            .get(self.stage_index)
            .ok_or(ClockStateError::Stage)?;
        self.remaining.checked_add(current.bonus().delay())?;
        let mut preceding_moves = 0_u64;
        let mut awarded = 0_u128;
        for stage in &control.stages()[..self.stage_index] {
            let ClockStageEnumQuota::Moves(quota) = stage.quota() else {
                return Err(ClockStateError::SnapshotCounters);
            };
            let moves = u64::from(quota.get());
            preceding_moves = preceding_moves
                .checked_add(moves)
                .ok_or(ClockStateError::MoveOverflow)?;
            awarded += u128::from(stage.base_time().as_nanos())
                + u128::from(stage.bonus().increment().as_nanos()) * u128::from(moves);
        }
        let current_moves = self
            .completed_moves
            .checked_sub(preceding_moves)
            .ok_or(ClockStateError::SnapshotCounters)?;
        let cycles = match current.quota() {
            ClockStageEnumQuota::Moves(quota) => {
                if self.stage_moves != current_moves || current_moves >= u64::from(quota.get()) {
                    return Err(ClockStateError::SnapshotCounters);
                }
                0
            }
            ClockStageEnumQuota::Unlimited => {
                if self.stage_moves != current_moves {
                    return Err(ClockStateError::SnapshotCounters);
                }
                0
            }
            ClockStageEnumQuota::Cyclic(quota) => {
                let quota = u64::from(quota.get());
                if self.stage_moves != current_moves % quota {
                    return Err(ClockStateError::SnapshotCounters);
                }
                current_moves / quota
            }
        };
        awarded += u128::from(current.base_time().as_nanos()) * (u128::from(cycles) + 1)
            + u128::from(current.bonus().increment().as_nanos()) * u128::from(current_moves);
        if u128::from(self.remaining.as_nanos()) > awarded {
            return Err(ClockStateError::SnapshotBalance);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockStateSnapshotOptions {
    pub control: ClockTimeControl,
    pub sides: [ClockStateSideOptions; 2],
    pub active: BoardEnumColor,
    pub elapsed: ClockDuration,
    pub status: ClockStateEnumStatus,
}

impl ClockStateSnapshot {
    pub fn from_persisted(options: ClockStateSnapshotOptions) -> Result<Self, ClockStateError> {
        let sides = [
            options.sides[0].validate(&options.control)?,
            options.sides[1].validate(&options.control)?,
        ];
        let active = sides[ClockStateSide::index(options.active)];
        if let Some(stage) = options.control.stages().get(active.stage_index) {
            let allowance = active.remaining.checked_add(stage.bonus().delay())?;
            let expired = options.elapsed >= allowance;
            match options.status {
                ClockStateEnumStatus::Paused | ClockStateEnumStatus::Running if expired => {
                    return Err(ClockStateError::SnapshotStatus);
                }
                ClockStateEnumStatus::Flagged(side) if side != options.active || !expired => {
                    return Err(ClockStateError::SnapshotStatus);
                }
                ClockStateEnumStatus::Paused
                | ClockStateEnumStatus::Running
                | ClockStateEnumStatus::Flagged(_) => {}
            }
        } else if matches!(options.status, ClockStateEnumStatus::Flagged(_)) {
            return Err(ClockStateError::SnapshotStatus);
        }
        Ok(Self {
            control: Arc::new(options.control),
            sides,
            active: options.active,
            elapsed: options.elapsed,
            status: options.status,
        })
    }

    pub fn persisted(&self) -> ClockStateSnapshotOptions {
        ClockStateSnapshotOptions {
            control: self.control.as_ref().clone(),
            sides: self.sides.map(|side| ClockStateSideOptions {
                remaining: side.remaining,
                completed_moves: side.completed_moves,
                stage_index: side.stage_index,
                stage_moves: side.stage_moves,
            }),
            active: self.active,
            elapsed: self.elapsed,
            status: self.status,
        }
    }

    pub fn control(&self) -> &ClockTimeControl {
        &self.control
    }

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
    pub const LOW_TIME: ClockDuration = ClockDuration::from_nanos(10_000_000_000);

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

    pub fn recover(
        snapshot: &ClockStateSnapshot,
        warnings_used: [bool; 2],
    ) -> Result<Self, ClockStateError> {
        let mut clock = Self::new(snapshot.control.as_ref().clone(), snapshot.active);
        clock.restore(snapshot)?;
        clock.warnings_used = warnings_used;
        Ok(clock)
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

    pub fn ordered_instant(&self, requested: ClockInstant) -> ClockInstant {
        self.last_observed
            .map_or(requested, |observed| requested.max(observed))
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
