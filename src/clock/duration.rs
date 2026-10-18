use std::num::TryFromIntError;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClockDurationError {
    #[error("The duration exceeds the native nanosecond range")]
    Overflow,
    #[error("Monotonic time cannot move backwards")]
    TimeOrder,
    #[error("The native time value is out of range: {0}")]
    Conversion(#[from] TryFromIntError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClockDuration(u64);

impl ClockDuration {
    pub const ZERO: Self = Self(0);
    const NANOS_PER_MILLI: u64 = 1_000_000;

    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    pub fn from_millis(millis: u64) -> Result<Self, ClockDurationError> {
        millis
            .checked_mul(Self::NANOS_PER_MILLI)
            .map(Self)
            .ok_or(ClockDurationError::Overflow)
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }
    pub const fn as_millis(self) -> u64 {
        self.0 / Self::NANOS_PER_MILLI
    }
    pub fn as_duration(self) -> Duration {
        Duration::from_nanos(self.0)
    }

    pub fn checked_add(self, other: Self) -> Result<Self, ClockDurationError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(ClockDurationError::Overflow)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, ClockDurationError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(ClockDurationError::TimeOrder)
    }
}

impl TryFrom<Duration> for ClockDuration {
    type Error = ClockDurationError;
    fn try_from(duration: Duration) -> Result<Self, Self::Error> {
        Ok(Self(u64::try_from(duration.as_nanos())?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClockInstant(ClockDuration);

impl ClockInstant {
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(ClockDuration::from_nanos(nanos))
    }

    pub fn from_millis(millis: u64) -> Result<Self, ClockDurationError> {
        Ok(Self(ClockDuration::from_millis(millis)?))
    }

    pub fn elapsed_since(self, earlier: Self) -> Result<ClockDuration, ClockDurationError> {
        self.0.checked_sub(earlier.0)
    }

    pub fn checked_add(self, duration: ClockDuration) -> Result<Self, ClockDurationError> {
        Ok(Self(self.0.checked_add(duration)?))
    }

    pub const fn has_reached(self, deadline: Self) -> bool {
        self.0.0 >= deadline.0.0
    }
}
