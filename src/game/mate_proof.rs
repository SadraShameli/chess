use std::collections::TryReserveError;
use std::mem::size_of;
use std::time::Duration;

use hashbrown::HashSet;
use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::{BoardPosition, BoardPositionIdentity};
use crate::game::adjudication::{
    GameAdjudicationEnumInterruption, GameAdjudicationEnumMateProof,
    GameAdjudicationEnumMateQuestion, GameAdjudicationMateProofProvider,
};

pub trait GameMateProofControl {
    fn now(&self) -> Duration;
    fn is_cancelled(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameMateProofBudget {
    duration: Duration,
    states: u32,
    bytes: usize,
}

impl GameMateProofBudget {
    pub const DEFAULT_DURATION: Duration = Duration::from_secs(2);
    pub const DEFAULT_STATES: u32 = 1_000_000;
    pub const DEFAULT_BYTES: usize = 128 * 1024 * 1024;

    pub fn new(
        duration: Duration,
        states: u32,
        bytes: usize,
    ) -> Result<Self, GameMateProofBudgetError> {
        if duration.is_zero() || states == 0 || bytes == 0 {
            return Err(GameMateProofBudgetError::Empty);
        }
        if duration > Self::DEFAULT_DURATION
            || states > Self::DEFAULT_STATES
            || bytes > Self::DEFAULT_BYTES
        {
            return Err(GameMateProofBudgetError::ExceedsSelectedLimit);
        }
        Ok(Self {
            duration,
            states,
            bytes,
        })
    }

    pub const fn duration(self) -> Duration {
        self.duration
    }

    pub const fn states(self) -> u32 {
        self.states
    }

    pub const fn bytes(self) -> usize {
        self.bytes
    }
}

impl Default for GameMateProofBudget {
    fn default() -> Self {
        Self {
            duration: Self::DEFAULT_DURATION,
            states: Self::DEFAULT_STATES,
            bytes: Self::DEFAULT_BYTES,
        }
    }
}

#[derive(Debug, Error)]
pub enum GameMateProofBudgetError {
    #[error("Mate-proof time, state, and memory limits must all be positive")]
    Empty,
    #[error(
        "Mate-proof limits cannot exceed the selected two-second, one-million-state, 128 MiB budget"
    )]
    ExceedsSelectedLimit,
}

pub struct GameMateProof<C> {
    budget: GameMateProofBudget,
    control: C,
}

impl<C: GameMateProofControl> GameMateProof<C> {
    pub fn new(budget: GameMateProofBudget, control: C) -> Self {
        Self { budget, control }
    }

    pub fn report(
        &self,
        position: &BoardPosition,
        question: GameAdjudicationEnumMateQuestion,
    ) -> GameMateProofReport {
        let mut search = GameMateProofSearch::new(self.budget, &self.control);
        let (proof, witness) = match search.run(position, question) {
            Ok(Some(witness)) => (GameAdjudicationEnumMateProof::Possible, Some(witness)),
            Ok(None) => (GameAdjudicationEnumMateProof::Impossible, None),
            Err(cause) => (GameAdjudicationEnumMateProof::Unknown(cause), None),
        };
        GameMateProofReport {
            proof,
            witness,
            states: search.states,
            requested_bytes: search.peak_bytes,
            elapsed: search.elapsed,
            failure: search.failure,
        }
    }
}

impl<C: GameMateProofControl> GameAdjudicationMateProofProvider for GameMateProof<C> {
    fn prove(
        &self,
        position: &BoardPosition,
        question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        self.report(position, question).proof()
    }
}

#[derive(Debug)]
pub struct GameMateProofReport {
    proof: GameAdjudicationEnumMateProof,
    witness: Option<GameMateProofWitness>,
    states: u32,
    requested_bytes: usize,
    elapsed: Duration,
    failure: Option<GameMateProofError>,
}

impl GameMateProofReport {
    pub const fn proof(&self) -> GameAdjudicationEnumMateProof {
        self.proof
    }

    pub fn witness(&self) -> Option<&GameMateProofWitness> {
        self.witness.as_ref()
    }

    pub const fn states(&self) -> u32 {
        self.states
    }

    pub const fn requested_bytes(&self) -> usize {
        self.requested_bytes
    }

    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }

    pub fn failure(&self) -> Option<&GameMateProofError> {
        self.failure.as_ref()
    }
}

#[derive(Debug, Error)]
pub enum GameMateProofError {
    #[error("A validated legal proof transition failed: {0}")]
    Transition(#[source] BoardMoveError),
    #[error("The proof arena or witness allocation failed: {0}")]
    Allocation(#[source] TryReserveError),
    #[error("The proof transposition allocation failed: {0:?}")]
    MemoAllocation(hashbrown::TryReserveError),
    #[error("The injected proof clock moved backwards from {started:?} to {now:?}")]
    ClockWentBackwards { started: Duration, now: Duration },
}

#[derive(Debug, Clone)]
pub struct GameMateProofWitness {
    initial_identity: BoardPositionIdentity,
    winner: BoardEnumColor,
    movements: Vec<BoardMove>,
}

impl GameMateProofWitness {
    pub const fn winner(&self) -> BoardEnumColor {
        self.winner
    }

    pub fn movements(&self) -> &[BoardMove] {
        &self.movements
    }

    pub fn verify(&self, initial: &BoardPosition) -> Result<bool, BoardMoveError> {
        if initial.identity() != self.initial_identity {
            return Ok(false);
        }
        let mut position = initial.clone();
        for movement in &self.movements {
            position = position.play_for_mate_proof(*movement)?;
        }
        Ok(position.is_checkmate() && position.turn().opposite() == self.winner)
    }

    pub fn advance(
        &self,
        before: &BoardPosition,
        movement: BoardMove,
        after: &BoardPosition,
    ) -> Result<Option<Self>, BoardMoveError> {
        if before.identity() != self.initial_identity
            || self.movements.first() != Some(&movement)
            || before.play_for_mate_proof(movement)?.identity() != after.identity()
        {
            return Ok(None);
        }
        Ok(Some(Self {
            initial_identity: after.identity(),
            winner: self.winner,
            movements: self.movements.iter().skip(1).copied().collect(),
        }))
    }

    fn is_requested(question: GameAdjudicationEnumMateQuestion, winner: BoardEnumColor) -> bool {
        match question {
            GameAdjudicationEnumMateQuestion::AnySide => true,
            GameAdjudicationEnumMateQuestion::Side(side) => side == winner,
        }
    }
}

#[derive(Clone, Copy)]
enum GameMateProofEnumTemplate {
    BlackFools,
    WhiteFools,
    Scholars,
}

impl GameMateProofEnumTemplate {
    const ALL: [Self; 3] = [Self::BlackFools, Self::WhiteFools, Self::Scholars];

    fn winner(self) -> BoardEnumColor {
        match self {
            Self::BlackFools => BoardEnumColor::Black,
            Self::WhiteFools | Self::Scholars => BoardEnumColor::White,
        }
    }

    fn movements(self) -> &'static [&'static str] {
        match self {
            Self::BlackFools => &["f2f3", "e7e5", "g2g4", "d8h4"],
            Self::WhiteFools => &["e2e4", "f7f6", "a2a3", "g7g5", "d1h5"],
            Self::Scholars => &["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6", "h5f7"],
        }
    }
}

struct GameMateProofNode {
    position: BoardPosition,
    parent: Option<usize>,
    movement: Option<BoardMove>,
}

struct GameMateProofGraphEndpoint {
    cursor: usize,
    movement: BoardMove,
    winner: BoardEnumColor,
}

struct GameMateProofSearch<'a, C> {
    budget: GameMateProofBudget,
    control: &'a C,
    started: Duration,
    elapsed: Duration,
    states: u32,
    peak_bytes: usize,
    failure: Option<GameMateProofError>,
}

impl<'a, C: GameMateProofControl> GameMateProofSearch<'a, C> {
    const MAX_LEGAL_MOVES: usize = 256;
    const MEMO_ENTRY_ALLOCATION_FACTOR: usize = 4;
    const MEMO_ALLOCATION_OVERHEAD: usize = 128;

    fn new(budget: GameMateProofBudget, control: &'a C) -> Self {
        Self {
            budget,
            control,
            started: control.now(),
            elapsed: Duration::ZERO,
            states: 0,
            peak_bytes: 0,
            failure: None,
        }
    }

    fn checkpoint(&mut self) -> Result<(), GameAdjudicationEnumInterruption> {
        if self.control.is_cancelled() {
            return Err(GameAdjudicationEnumInterruption::Cancelled);
        }
        let now = self.control.now();
        self.elapsed = now.checked_sub(self.started).ok_or_else(|| {
            self.fail(
                GameMateProofError::ClockWentBackwards {
                    started: self.started,
                    now,
                },
                GameAdjudicationEnumInterruption::Unresolved,
            )
        })?;
        if self.elapsed >= self.budget.duration {
            return Err(GameAdjudicationEnumInterruption::TimeLimit);
        }
        Ok(())
    }

    fn fail(
        &mut self,
        error: GameMateProofError,
        cause: GameAdjudicationEnumInterruption,
    ) -> GameAdjudicationEnumInterruption {
        self.failure = Some(error);
        cause
    }

    fn consume_state(&mut self) -> Result<(), GameAdjudicationEnumInterruption> {
        self.checkpoint()?;
        if self.states >= self.budget.states {
            return Err(GameAdjudicationEnumInterruption::StateLimit);
        }
        self.states += 1;
        Ok(())
    }

    fn reserve_bytes(&mut self, bytes: usize) -> Result<(), GameAdjudicationEnumInterruption> {
        if bytes > self.budget.bytes {
            return Err(GameAdjudicationEnumInterruption::MemoryLimit);
        }
        self.peak_bytes = self.peak_bytes.max(bytes);
        Ok(())
    }

    fn run(
        &mut self,
        position: &BoardPosition,
        question: GameAdjudicationEnumMateQuestion,
    ) -> Result<Option<GameMateProofWitness>, GameAdjudicationEnumInterruption> {
        self.consume_state()?;
        if position.is_checkmate() {
            let winner = position.turn().opposite();
            self.checkpoint()?;
            return Ok(
                GameMateProofWitness::is_requested(question, winner).then(|| {
                    GameMateProofWitness {
                        initial_identity: position.identity(),
                        winner,
                        movements: Vec::new(),
                    }
                }),
            );
        }
        if position.is_stalemate() || Self::is_proven_insufficient(position, question) {
            self.checkpoint()?;
            return Ok(None);
        }
        for template in GameMateProofEnumTemplate::ALL {
            if !GameMateProofWitness::is_requested(question, template.winner()) {
                continue;
            }
            for offset in 0..template.movements().len() {
                if let Some(witness) = self.try_template(position, template, offset)? {
                    return Ok(Some(witness));
                }
            }
        }
        self.reachability(position, question)
    }

    fn is_proven_insufficient(
        position: &BoardPosition,
        question: GameAdjudicationEnumMateQuestion,
    ) -> bool {
        match question {
            GameAdjudicationEnumMateQuestion::AnySide => {
                position.has_insufficient_mating_material(BoardEnumColor::White)
                    && position.has_insufficient_mating_material(BoardEnumColor::Black)
            }
            GameAdjudicationEnumMateQuestion::Side(side) => {
                position.has_insufficient_mating_material(side)
            }
        }
    }

    fn try_template(
        &mut self,
        initial: &BoardPosition,
        template: GameMateProofEnumTemplate,
        offset: usize,
    ) -> Result<Option<GameMateProofWitness>, GameAdjudicationEnumInterruption> {
        self.checkpoint()?;
        let continuation = &template.movements()[offset..];
        let mut movements = Vec::new();
        self.reserve_bytes(continuation.len() * size_of::<BoardMove>())?;
        movements
            .try_reserve_exact(continuation.len())
            .map_err(|error| {
                self.fail(
                    GameMateProofError::Allocation(error),
                    GameAdjudicationEnumInterruption::MemoryLimit,
                )
            })?;
        self.reserve_bytes(movements.capacity() * size_of::<BoardMove>())?;
        let mut position = initial.clone();
        for uci in continuation {
            let movement = match position.parse_uci(uci) {
                Ok(movement) => movement,
                Err(_not_applicable) => return Ok(None),
            };
            self.consume_state()?;
            position = position.play_for_mate_proof(movement).map_err(|error| {
                self.fail(
                    GameMateProofError::Transition(error),
                    GameAdjudicationEnumInterruption::Unresolved,
                )
            })?;
            movements.push(movement);
        }
        self.checkpoint()?;
        if position.is_checkmate() && position.turn().opposite() == template.winner() {
            Ok(Some(GameMateProofWitness {
                initial_identity: initial.identity(),
                winner: template.winner(),
                movements,
            }))
        } else {
            Ok(None)
        }
    }

    fn arena_capacity(&self) -> Result<usize, GameAdjudicationEnumInterruption> {
        let per_entry = (size_of::<BoardPositionIdentity>() + 1)
            .checked_mul(Self::MEMO_ENTRY_ALLOCATION_FACTOR)
            .and_then(|memo| memo.checked_add(size_of::<GameMateProofNode>()))
            .and_then(|entry| entry.checked_add(size_of::<BoardMove>()))
            .ok_or(GameAdjudicationEnumInterruption::MemoryLimit)?;
        let remaining = self
            .budget
            .bytes
            .checked_sub(Self::scratch_bytes())
            .ok_or(GameAdjudicationEnumInterruption::MemoryLimit)?;
        let remaining_states = self
            .budget
            .states
            .checked_sub(self.states)
            .ok_or(GameAdjudicationEnumInterruption::StateLimit)?;
        if remaining_states == 0 {
            return Err(GameAdjudicationEnumInterruption::StateLimit);
        }
        let state_capacity = usize::try_from(remaining_states)
            .map_err(|_error| GameAdjudicationEnumInterruption::MemoryLimit)?;
        let capacity = (remaining / per_entry).min(state_capacity);
        if capacity == 0 {
            return Err(GameAdjudicationEnumInterruption::MemoryLimit);
        }
        Ok(capacity)
    }

    fn scratch_bytes() -> usize {
        Self::MEMO_ALLOCATION_OVERHEAD + (Self::MAX_LEGAL_MOVES + 1) * size_of::<BoardMove>()
    }

    fn reachability(
        &mut self,
        initial: &BoardPosition,
        question: GameAdjudicationEnumMateQuestion,
    ) -> Result<Option<GameMateProofWitness>, GameAdjudicationEnumInterruption> {
        self.checkpoint()?;
        let capacity = self.arena_capacity()?;
        let mut memo = HashSet::new();
        memo.try_reserve(capacity).map_err(|error| {
            self.fail(
                GameMateProofError::MemoAllocation(error),
                GameAdjudicationEnumInterruption::MemoryLimit,
            )
        })?;
        let mut arena = Vec::new();
        arena.try_reserve_exact(capacity).map_err(|error| {
            self.fail(
                GameMateProofError::Allocation(error),
                GameAdjudicationEnumInterruption::MemoryLimit,
            )
        })?;
        let arena_bytes = arena.capacity() * size_of::<GameMateProofNode>();
        let owned_bytes = memo.allocation_size() + arena_bytes;
        self.reserve_bytes(
            owned_bytes + Self::scratch_bytes() + capacity * size_of::<BoardMove>(),
        )?;
        memo.insert(initial.identity());
        arena.push(GameMateProofNode {
            position: initial.clone(),
            parent: None,
            movement: None,
        });
        let mut cursor = 0;
        while cursor < arena.len() {
            self.checkpoint()?;
            let legal_moves = arena[cursor].position.legal_moves();
            for movement in legal_moves {
                self.checkpoint()?;
                let next = arena[cursor]
                    .position
                    .play_for_mate_proof(movement)
                    .map_err(|error| {
                        self.fail(
                            GameMateProofError::Transition(error),
                            GameAdjudicationEnumInterruption::Unresolved,
                        )
                    })?;
                let identity = next.identity();
                if memo.contains(&identity) {
                    continue;
                }
                self.consume_state()?;
                if next.is_checkmate() {
                    let winner = next.turn().opposite();
                    if GameMateProofWitness::is_requested(question, winner) {
                        self.checkpoint()?;
                        return self
                            .graph_witness(
                                initial,
                                &arena,
                                GameMateProofGraphEndpoint {
                                    cursor,
                                    movement,
                                    winner,
                                },
                                owned_bytes,
                            )
                            .map(Some);
                    }
                    continue;
                }
                if next.is_stalemate() || Self::is_proven_insufficient(&next, question) {
                    continue;
                }
                if arena.len() >= capacity || memo.len() >= capacity {
                    return Err(GameAdjudicationEnumInterruption::MemoryLimit);
                }
                memo.insert(identity);
                arena.push(GameMateProofNode {
                    position: next,
                    parent: Some(cursor),
                    movement: Some(movement),
                });
            }
            cursor += 1;
        }
        self.checkpoint()?;
        Ok(None)
    }

    fn graph_witness(
        &mut self,
        initial: &BoardPosition,
        arena: &[GameMateProofNode],
        endpoint: GameMateProofGraphEndpoint,
        owned_bytes: usize,
    ) -> Result<GameMateProofWitness, GameAdjudicationEnumInterruption> {
        let mut length: usize = 1;
        let mut cursor = endpoint.cursor;
        let mut parent = arena[cursor].parent;
        while let Some(index) = parent {
            length += 1;
            parent = arena[index].parent;
        }
        self.reserve_bytes(owned_bytes + Self::scratch_bytes() + length * size_of::<BoardMove>())?;
        let mut movements = Vec::new();
        movements.try_reserve_exact(length).map_err(|error| {
            self.fail(
                GameMateProofError::Allocation(error),
                GameAdjudicationEnumInterruption::MemoryLimit,
            )
        })?;
        self.reserve_bytes(
            owned_bytes + Self::scratch_bytes() + movements.capacity() * size_of::<BoardMove>(),
        )?;
        movements.push(endpoint.movement);
        while let Some(parent) = arena[cursor].parent {
            movements.push(
                arena[cursor]
                    .movement
                    .ok_or(GameAdjudicationEnumInterruption::Unresolved)?,
            );
            cursor = parent;
        }
        movements.reverse();
        self.checkpoint()?;
        Ok(GameMateProofWitness {
            initial_identity: initial.identity(),
            winner: endpoint.winner,
            movements,
        })
    }
}
