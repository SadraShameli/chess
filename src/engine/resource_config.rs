use std::num::TryFromIntError;

use thiserror::Error;

use crate::clock::duration::{ClockDuration, ClockDurationError};
use crate::engine::enums::{EngineEnumOption, EngineOptionEnumDescriptor};

#[derive(Debug, Error)]
pub enum EngineResourceConfigError {
    #[error("Engine option {option:?} value {value} must be between {minimum} and {maximum}")]
    Range {
        option: EngineEnumOption,
        value: u64,
        minimum: u32,
        maximum: u32,
    },
    #[error("Engine option {0:?} is not a numeric resource option")]
    NonNumericOption(EngineEnumOption),
    #[error("Move overhead must be an exact whole number of milliseconds")]
    FractionalMoveOverhead,
    #[error("The engine resource value cannot be represented: {0}")]
    Conversion(#[from] TryFromIntError),
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineResourceConfig {
    threads: u32,
    hash_mib: u32,
    move_overhead: ClockDuration,
}

impl EngineResourceConfig {
    pub fn new(
        threads: usize,
        hash_mib: u32,
        move_overhead: ClockDuration,
    ) -> Result<Self, EngineResourceConfigError> {
        let threads = Self::number(EngineEnumOption::Threads, u64::try_from(threads)?)?;
        Self::number(EngineEnumOption::Hash, u64::from(hash_mib))?;
        let millis = move_overhead.as_millis();
        Self::number(EngineEnumOption::MoveOverhead, millis)?;
        if ClockDuration::from_millis(millis)? != move_overhead {
            return Err(EngineResourceConfigError::FractionalMoveOverhead);
        }
        Ok(Self {
            threads,
            hash_mib,
            move_overhead,
        })
    }

    pub const fn threads(self) -> u32 {
        self.threads
    }

    pub const fn hash_mib(self) -> u32 {
        self.hash_mib
    }

    pub const fn move_overhead(self) -> ClockDuration {
        self.move_overhead
    }

    pub(crate) fn settings(self) -> [(EngineEnumOption, u64); 3] {
        [
            (EngineEnumOption::Threads, u64::from(self.threads)),
            (EngineEnumOption::Hash, u64::from(self.hash_mib)),
            (
                EngineEnumOption::MoveOverhead,
                self.move_overhead.as_millis(),
            ),
        ]
    }

    fn number(option: EngineEnumOption, value: u64) -> Result<u32, EngineResourceConfigError> {
        let EngineOptionEnumDescriptor::Spin(range) = option.typed_descriptor() else {
            return Err(EngineResourceConfigError::NonNumericOption(option));
        };
        if !range.contains(value) {
            return Err(EngineResourceConfigError::Range {
                option,
                value,
                minimum: range.minimum(),
                maximum: range.maximum(),
            });
        }
        Ok(u32::try_from(value)?)
    }
}
