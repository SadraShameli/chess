use std::future::Future;
use std::time::Instant;

use thiserror::Error;
use uuid::Uuid;

use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::BoardPosition;
use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::clock::state::ClockState;
use crate::game::enums::{GameId, GameRevision};

#[derive(Debug, Error)]
pub enum GameEngineRequestError {
    #[error("Engine play requires a running game clock")]
    NotRunning,
    #[error("The game clock and replayed position have different active sides")]
    ClockSide,
    #[error("Terminal positions must be adjudicated before requesting an engine turn")]
    TerminalPosition,
    #[error("An untimed engine turn needs a positive move-time budget")]
    EmptyBudget,
    #[error(transparent)]
    Move(#[from] BoardMoveError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameEngineContext {
    game: GameId,
    revision: GameRevision,
}

impl GameEngineContext {
    pub const fn new(game: GameId, revision: GameRevision) -> Self {
        Self { game, revision }
    }

    pub const fn game(self) -> GameId {
        self.game
    }

    pub const fn revision(self) -> GameRevision {
        self.revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameEngineSearchConfig {
    untimed_move_time: ClockDuration,
    move_overhead: ClockDuration,
}

impl GameEngineSearchConfig {
    pub const fn initial_default() -> Self {
        Self {
            untimed_move_time: ClockDuration::from_nanos(3_000_000_000),
            move_overhead: ClockDuration::from_nanos(100_000_000),
        }
    }

    pub fn new(
        untimed_move_time: ClockDuration,
        move_overhead: ClockDuration,
    ) -> Result<Self, GameEngineRequestError> {
        if untimed_move_time == ClockDuration::ZERO {
            return Err(GameEngineRequestError::EmptyBudget);
        }
        Ok(Self {
            untimed_move_time,
            move_overhead,
        })
    }

    pub const fn untimed_move_time(self) -> ClockDuration {
        self.untimed_move_time
    }

    pub const fn move_overhead(self) -> ClockDuration {
        self.move_overhead
    }
}

#[derive(Debug, Clone)]
pub struct GameEngineRequest {
    context: GameEngineContext,
    initial: BoardPosition,
    current: BoardPosition,
    history: Vec<BoardMove>,
    clock: ClockState,
    config: GameEngineSearchConfig,
}

impl GameEngineRequest {
    pub fn new(
        context: GameEngineContext,
        initial: BoardPosition,
        history: Vec<BoardMove>,
        clock: ClockState,
        config: GameEngineSearchConfig,
    ) -> Result<Self, GameEngineRequestError> {
        let mut current = initial.clone();
        for &movement in &history {
            current = current.play(movement)?;
        }
        if current.legal_moves().is_empty() {
            return Err(GameEngineRequestError::TerminalPosition);
        }
        if !clock.is_running() {
            return Err(GameEngineRequestError::NotRunning);
        }
        if current.turn() != clock.active() {
            return Err(GameEngineRequestError::ClockSide);
        }
        Ok(Self {
            context,
            initial,
            current,
            history,
            clock,
            config,
        })
    }

    pub const fn context(&self) -> GameEngineContext {
        self.context
    }

    pub const fn initial(&self) -> &BoardPosition {
        &self.initial
    }

    pub const fn current(&self) -> &BoardPosition {
        &self.current
    }

    pub fn history(&self) -> &[BoardMove] {
        &self.history
    }

    pub const fn clock(&self) -> &ClockState {
        &self.clock
    }

    pub const fn config(&self) -> GameEngineSearchConfig {
        self.config
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameEngineSearchIdentity {
    context: GameEngineContext,
    search: Uuid,
    generation: Uuid,
}

impl GameEngineSearchIdentity {
    pub(crate) const fn new(context: GameEngineContext, search: Uuid, generation: Uuid) -> Self {
        Self {
            context,
            search,
            generation,
        }
    }

    pub const fn context(self) -> GameEngineContext {
        self.context
    }

    pub const fn search(self) -> Uuid {
        self.search
    }

    pub const fn generation(self) -> Uuid {
        self.generation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameEngineMove {
    identity: GameEngineSearchIdentity,
    movement: BoardMove,
    received_at: ClockInstant,
}

impl GameEngineMove {
    pub(crate) const fn new(
        identity: GameEngineSearchIdentity,
        movement: BoardMove,
        received_at: ClockInstant,
    ) -> Self {
        Self {
            identity,
            movement,
            received_at,
        }
    }

    pub const fn identity(self) -> GameEngineSearchIdentity {
        self.identity
    }

    pub const fn movement(self) -> BoardMove {
        self.movement
    }

    pub const fn received_at(self) -> ClockInstant {
        self.received_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameEngineStopReceipt {
    identity: GameEngineSearchIdentity,
}

impl GameEngineStopReceipt {
    pub(crate) const fn new(identity: GameEngineSearchIdentity) -> Self {
        Self { identity }
    }

    pub const fn identity(self) -> GameEngineSearchIdentity {
        self.identity
    }
}

#[derive(Debug)]
pub struct GameEngineQuiescenceReceipt {
    context: GameEngineContext,
    generation: Uuid,
    stopped_search: Option<GameEngineSearchIdentity>,
}

impl GameEngineQuiescenceReceipt {
    pub(crate) const fn new(
        context: GameEngineContext,
        generation: Uuid,
        stopped_search: Option<GameEngineSearchIdentity>,
    ) -> Self {
        Self {
            context,
            generation,
            stopped_search,
        }
    }

    pub const fn context(&self) -> GameEngineContext {
        self.context
    }

    pub const fn generation(&self) -> Uuid {
        self.generation
    }

    pub const fn stopped_search(&self) -> Option<GameEngineSearchIdentity> {
        self.stopped_search
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameEngineEnumDiscard {
    StaleContext,
    Deadline,
    Cancelled,
}

#[derive(Debug)]
pub enum GameEngineEnumEvent {
    Info {
        identity: GameEngineSearchIdentity,
        text: String,
    },
    Diagnostic {
        identity: GameEngineSearchIdentity,
        text: String,
    },
    Readiness {
        identity: GameEngineSearchIdentity,
    },
    Move(GameEngineMove),
    Stopped(GameEngineStopReceipt),
    Discarded {
        identity: GameEngineSearchIdentity,
        reason: GameEngineEnumDiscard,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameEngineAllocationView {
    workers: u32,
    hash_mib: u32,
}

impl GameEngineAllocationView {
    pub const fn new(workers: u32, hash_mib: u32) -> Self {
        Self { workers, hash_mib }
    }

    pub const fn workers(self) -> u32 {
        self.workers
    }

    pub const fn hash_mib(self) -> u32 {
        self.hash_mib
    }
}

pub trait GameEngineProvider: Send {
    type Error: std::error::Error + Send + Sync;

    fn is_idle(&self) -> bool;

    fn allocation_view(&self) -> Option<GameEngineAllocationView> {
        None
    }
    fn start(
        &mut self,
        request: GameEngineRequest,
    ) -> impl Future<Output = Result<GameEngineSearchIdentity, Self::Error>> + Send;
    fn next_event(
        &mut self,
        expected: GameEngineContext,
    ) -> impl Future<Output = Result<GameEngineEnumEvent, Self::Error>> + Send;
    fn quiesce(
        &mut self,
        expected: GameEngineContext,
        received_at: Instant,
    ) -> impl Future<Output = Result<GameEngineQuiescenceReceipt, Self::Error>> + Send;
    fn stop(
        &mut self,
        identity: GameEngineSearchIdentity,
        received_at: Instant,
    ) -> impl Future<Output = Result<GameEngineStopReceipt, Self::Error>> + Send;
}
