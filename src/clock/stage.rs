use std::num::NonZeroU32;

use thiserror::Error;

use crate::clock::duration::{ClockDuration, ClockDurationError};

#[derive(Debug, Error)]
pub enum ClockStageError {
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockStageEnumQuota {
    Moves(NonZeroU32),
    Unlimited,
    Cyclic(NonZeroU32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockStageEnumBonus {
    None,
    Fischer(ClockDuration),
    SimpleDelay(ClockDuration),
    BronsteinDelay(ClockDuration),
}

impl ClockStageEnumBonus {
    pub const fn increment(self) -> ClockDuration {
        match self {
            Self::Fischer(duration) => duration,
            Self::None | Self::SimpleDelay(_) | Self::BronsteinDelay(_) => ClockDuration::ZERO,
        }
    }

    pub const fn delay(self) -> ClockDuration {
        match self {
            Self::SimpleDelay(duration) | Self::BronsteinDelay(duration) => duration,
            Self::None | Self::Fischer(_) => ClockDuration::ZERO,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockStage {
    quota: ClockStageEnumQuota,
    base_time: ClockDuration,
    bonus: ClockStageEnumBonus,
}

impl ClockStage {
    pub(crate) const INITIAL_DEFAULT: Self = Self {
        quota: ClockStageEnumQuota::Unlimited,
        base_time: ClockDuration::from_nanos(600_000_000_000),
        bonus: ClockStageEnumBonus::Fischer(ClockDuration::from_nanos(5_000_000_000)),
    };

    pub fn new(
        quota: ClockStageEnumQuota,
        base_time: ClockDuration,
        bonus: ClockStageEnumBonus,
    ) -> Result<Self, ClockStageError> {
        base_time.checked_add(bonus.increment())?;
        base_time.checked_add(bonus.delay())?;
        Ok(Self {
            quota,
            base_time,
            bonus,
        })
    }

    pub const fn quota(self) -> ClockStageEnumQuota {
        self.quota
    }

    pub const fn base_time(self) -> ClockDuration {
        self.base_time
    }

    pub const fn bonus(self) -> ClockStageEnumBonus {
        self.bonus
    }
}
