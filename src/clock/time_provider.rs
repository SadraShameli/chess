use std::time::Instant;

use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};

pub trait ClockTimeProvider: Send + Sync {
    fn now(&self) -> Result<ClockInstant, ClockDurationError>;
}

pub struct ClockTimeProviderMonotonic {
    origin: Instant,
}

impl ClockTimeProviderMonotonic {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for ClockTimeProviderMonotonic {
    fn default() -> Self {
        Self::new()
    }
}

impl ClockTimeProvider for ClockTimeProviderMonotonic {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        let elapsed = ClockDuration::try_from(self.origin.elapsed())?;
        Ok(ClockInstant::from_nanos(elapsed.as_nanos()))
    }
}
