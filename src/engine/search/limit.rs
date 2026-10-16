use std::num::NonZeroU32;

use thiserror::Error;

use crate::clock::duration::ClockDuration;

#[derive(Debug, Error)]
pub enum EngineSearchLimitError {
    #[error("The search depth exceeds Reckless's supported positive depth range")]
    Depth,
    #[error("Reckless search numbers must fit its signed 32-bit parser")]
    NumericRange,
    #[error("A node or move-time search budget must be positive")]
    EmptyBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineSearchClock {
    white_time: u32,
    black_time: u32,
    white_increment: u32,
    black_increment: u32,
    moves_to_go: Option<NonZeroU32>,
}

impl EngineSearchClock {
    pub fn new(
        white_time: ClockDuration,
        black_time: ClockDuration,
        white_increment: ClockDuration,
        black_increment: ClockDuration,
        moves_to_go: Option<NonZeroU32>,
    ) -> Result<Self, EngineSearchLimitError> {
        if let Some(moves) = moves_to_go {
            EngineSearchLimit::validate_number(u64::from(moves.get()))?;
        }
        Ok(Self {
            white_time: EngineSearchLimit::validate_number(white_time.as_millis())?,
            black_time: EngineSearchLimit::validate_number(black_time.as_millis())?,
            white_increment: EngineSearchLimit::validate_number(white_increment.as_millis())?,
            black_increment: EngineSearchLimit::validate_number(black_increment.as_millis())?,
            moves_to_go,
        })
    }

    fn command(self) -> String {
        let mut command = format!(
            "go wtime {} btime {} winc {} binc {}",
            self.white_time, self.black_time, self.white_increment, self.black_increment
        );
        if let Some(moves) = self.moves_to_go {
            command.push_str(&format!(" movestogo {moves}"));
        }
        command
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineSearchEnumLimit {
    Infinite,
    Depth(u32),
    Nodes(u32),
    MoveTime(u32),
    Clock(EngineSearchClock),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineSearchLimit(EngineSearchEnumLimit);

impl EngineSearchLimit {
    pub const MAX_DEPTH: u32 = 127;
    pub const MAX_NUMBER: u64 = 2_147_483_647;

    pub const fn infinite() -> Self {
        Self(EngineSearchEnumLimit::Infinite)
    }

    pub fn depth(depth: u32) -> Result<Self, EngineSearchLimitError> {
        if !(1..=Self::MAX_DEPTH).contains(&depth) {
            return Err(EngineSearchLimitError::Depth);
        }
        Ok(Self(EngineSearchEnumLimit::Depth(depth)))
    }

    pub fn nodes(nodes: u64) -> Result<Self, EngineSearchLimitError> {
        Ok(Self(EngineSearchEnumLimit::Nodes(Self::positive_number(
            nodes,
        )?)))
    }

    pub fn move_time(time: ClockDuration) -> Result<Self, EngineSearchLimitError> {
        Ok(Self(EngineSearchEnumLimit::MoveTime(
            Self::positive_number(time.as_millis())?,
        )))
    }

    pub const fn clock(clock: EngineSearchClock) -> Self {
        Self(EngineSearchEnumLimit::Clock(clock))
    }

    pub fn command(self) -> String {
        match self.0 {
            EngineSearchEnumLimit::Infinite => "go infinite".to_owned(),
            EngineSearchEnumLimit::Depth(depth) => format!("go depth {depth}"),
            EngineSearchEnumLimit::Nodes(nodes) => format!("go nodes {nodes}"),
            EngineSearchEnumLimit::MoveTime(time) => format!("go movetime {time}"),
            EngineSearchEnumLimit::Clock(clock) => clock.command(),
        }
    }

    fn validate_number(value: u64) -> Result<u32, EngineSearchLimitError> {
        if value > Self::MAX_NUMBER {
            return Err(EngineSearchLimitError::NumericRange);
        }
        u32::try_from(value).map_err(|_error| EngineSearchLimitError::NumericRange)
    }

    fn positive_number(value: u64) -> Result<u32, EngineSearchLimitError> {
        if value == 0 {
            return Err(EngineSearchLimitError::EmptyBudget);
        }
        Self::validate_number(value)
    }
}
