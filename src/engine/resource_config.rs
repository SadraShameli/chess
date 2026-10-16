use std::num::TryFromIntError;

use thiserror::Error;

use crate::clock::duration::{ClockDuration, ClockDurationError};
use crate::engine::capabilities::EngineCapabilities;
use crate::engine::enums::{EngineEnumOption, EngineOptionEnumDescriptor};

#[derive(Debug, Error)]
pub enum EngineResourceConfigError {
    #[error("The engine thread request must be positive")]
    EmptyThreads,
    #[error("Engine option {option:?} value {value} must be between {minimum} and {maximum}")]
    Range {
        option: EngineEnumOption,
        value: u64,
        minimum: u32,
        maximum: u32,
    },
    #[error("Engine option {0:?} is not a numeric option")]
    NonNumericOption(EngineEnumOption),
    #[error("The engine did not advertise resource option {0:?}")]
    MissingOption(EngineEnumOption),
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
        let threads = u32::try_from(threads)?;
        if threads == 0 {
            return Err(EngineResourceConfigError::EmptyThreads);
        }
        Self::number(
            EngineEnumOption::Hash,
            u64::from(hash_mib),
            EngineEnumOption::Hash.typed_descriptor(),
        )?;
        Self::validate_move_overhead(move_overhead)?;
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

    pub(crate) fn validate_move_overhead(
        move_overhead: ClockDuration,
    ) -> Result<(), EngineResourceConfigError> {
        let millis = move_overhead.as_millis();
        Self::number(
            EngineEnumOption::MoveOverhead,
            millis,
            EngineEnumOption::MoveOverhead.typed_descriptor(),
        )?;
        if ClockDuration::from_millis(millis)? != move_overhead {
            return Err(EngineResourceConfigError::FractionalMoveOverhead);
        }
        Ok(())
    }

    pub(crate) fn validate(
        self,
        capabilities: &EngineCapabilities,
    ) -> Result<(), EngineResourceConfigError> {
        for (option, value) in self.settings() {
            let descriptor = capabilities
                .descriptor(option)
                .ok_or(EngineResourceConfigError::MissingOption(option))?;
            Self::number(option, value, descriptor)?;
        }
        Ok(())
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

    pub(crate) fn number(
        option: EngineEnumOption,
        value: u64,
        descriptor: EngineOptionEnumDescriptor,
    ) -> Result<u32, EngineResourceConfigError> {
        let EngineOptionEnumDescriptor::Spin(range) = descriptor else {
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
