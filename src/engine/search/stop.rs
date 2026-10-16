use std::time::Duration;

use thiserror::Error;
use tokio::time::Instant;

use crate::engine::process_provider::EngineProcessLimits;
use crate::engine::search::identity::EngineSearchTag;

#[derive(Debug, Error)]
pub enum EngineSearchStopError {
    #[error("The engine stop or termination deadline exceeds the runtime clock range")]
    DeadlineOverflow,
}

#[derive(Debug, Clone, Copy)]
pub struct EngineSearchStop {
    tag: EngineSearchTag,
    deadline: Instant,
    termination_deadline: Instant,
}

impl EngineSearchStop {
    pub const SAFETY: Duration = Duration::from_secs(2);

    pub fn received_at(
        tag: EngineSearchTag,
        receipt: Instant,
    ) -> Result<Self, EngineSearchStopError> {
        let deadline = receipt
            .checked_add(Self::SAFETY)
            .ok_or(EngineSearchStopError::DeadlineOverflow)?;
        let termination_deadline = deadline
            .checked_add(EngineProcessLimits::SHUTDOWN)
            .ok_or(EngineSearchStopError::DeadlineOverflow)?;
        Ok(Self {
            tag,
            deadline,
            termination_deadline,
        })
    }

    pub const fn tag(self) -> EngineSearchTag {
        self.tag
    }

    pub const fn deadline(self) -> Instant {
        self.deadline
    }

    pub fn has_expired(self, now: Instant) -> bool {
        now >= self.deadline
    }

    pub const fn termination_deadline(self) -> Instant {
        self.termination_deadline
    }
}
