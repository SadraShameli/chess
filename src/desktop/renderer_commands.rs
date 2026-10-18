use thiserror::Error;

use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};

#[derive(Debug, Error)]
pub enum DesktopRendererCommandsError {
    #[error("The renderer clock failed: {0}")]
    Clock(#[from] ClockDurationError),
    #[error("Retry is only available while graphics are interrupted")]
    RetryUnavailable,
    #[error("Recovery can only be reported while a retry is running")]
    NotRetrying,
    #[error("Games can only resume after graphics recovered")]
    ResumeUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
pub enum DesktopRendererCommandsEnumCause {
    #[strum(serialize = "Graphics could not be initialized")]
    Initialization,
    #[strum(serialize = "The graphics context was lost")]
    ContextLoss,
    #[strum(serialize = "A graphics shader failed")]
    Shader,
    #[strum(serialize = "The board assets could not be loaded")]
    Asset,
}

impl DesktopRendererCommandsEnumCause {
    pub fn label(self) -> &'static str {
        self.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopRendererCommandsEnumState {
    Ready,
    Interrupted(DesktopRendererCommandsEnumCause),
    Retrying {
        cause: DesktopRendererCommandsEnumCause,
        deadline: ClockInstant,
    },
    Unrecoverable(DesktopRendererCommandsEnumCause),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopRendererCommands {
    state: DesktopRendererCommandsEnumState,
    games_paused: bool,
}

impl Default for DesktopRendererCommands {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopRendererCommands {
    pub const RETRY_DEADLINE: ClockDuration = ClockDuration::from_nanos(10_000_000_000);

    pub const fn new() -> Self {
        Self {
            state: DesktopRendererCommandsEnumState::Ready,
            games_paused: false,
        }
    }

    pub const fn state(&self) -> DesktopRendererCommandsEnumState {
        self.state
    }

    pub const fn games_paused(&self) -> bool {
        self.games_paused
    }

    pub const fn is_board_available(&self) -> bool {
        matches!(self.state, DesktopRendererCommandsEnumState::Ready)
    }

    pub fn interrupt(&mut self, cause: DesktopRendererCommandsEnumCause) {
        self.state = DesktopRendererCommandsEnumState::Interrupted(cause);
        self.games_paused = true;
    }

    pub fn retry(
        &mut self,
        now: ClockInstant,
    ) -> Result<ClockInstant, DesktopRendererCommandsError> {
        let cause = match self.state {
            DesktopRendererCommandsEnumState::Interrupted(cause)
            | DesktopRendererCommandsEnumState::Unrecoverable(cause) => cause,
            DesktopRendererCommandsEnumState::Ready
            | DesktopRendererCommandsEnumState::Retrying { .. } => {
                return Err(DesktopRendererCommandsError::RetryUnavailable);
            }
        };
        let deadline = now.checked_add(Self::RETRY_DEADLINE)?;
        self.state = DesktopRendererCommandsEnumState::Retrying { cause, deadline };
        Ok(deadline)
    }

    pub fn observe(&mut self, now: ClockInstant) -> bool {
        match self.state {
            DesktopRendererCommandsEnumState::Retrying { cause, deadline }
                if now.has_reached(deadline) =>
            {
                self.state = DesktopRendererCommandsEnumState::Unrecoverable(cause);
                true
            }
            DesktopRendererCommandsEnumState::Ready
            | DesktopRendererCommandsEnumState::Interrupted(_)
            | DesktopRendererCommandsEnumState::Retrying { .. }
            | DesktopRendererCommandsEnumState::Unrecoverable(_) => false,
        }
    }

    pub fn recovered(&mut self, now: ClockInstant) -> Result<(), DesktopRendererCommandsError> {
        self.observe(now);
        match self.state {
            DesktopRendererCommandsEnumState::Retrying { .. } => {
                self.state = DesktopRendererCommandsEnumState::Ready;
                Ok(())
            }
            DesktopRendererCommandsEnumState::Ready
            | DesktopRendererCommandsEnumState::Interrupted(_)
            | DesktopRendererCommandsEnumState::Unrecoverable(_) => {
                Err(DesktopRendererCommandsError::NotRetrying)
            }
        }
    }

    pub fn resume_games(&mut self) -> Result<(), DesktopRendererCommandsError> {
        if !self.is_board_available() {
            return Err(DesktopRendererCommandsError::ResumeUnavailable);
        }
        self.games_paused = false;
        Ok(())
    }
}
