use std::time::Duration;

use thiserror::Error;

use crate::puzzle::attempt::{PuzzleAttempt, PuzzleAttemptId};
use crate::puzzle::session::PuzzleSessionId;
use crate::puzzle::summary::{PuzzleSummary, PuzzleSummaryAttempt, PuzzleSummaryError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleHistoryEnumScope {
    Attempt {
        session: PuzzleSessionId,
        attempt: PuzzleAttemptId,
    },
    Session(PuzzleSessionId),
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleHistoryEnumConfirmation {
    Unconfirmed,
    Confirmed,
}

#[derive(Debug, Error)]
pub enum PuzzleHistoryError {
    #[error("Deleting puzzle history requires explicit confirmation")]
    ConfirmationRequired,
    #[error("Puzzle history cannot be deleted while run {session:?} is active; end the run first")]
    ActiveRunRefused { session: PuzzleSessionId },
    #[error("No puzzle history matches the requested deletion scope")]
    NothingToDelete,
    #[error(transparent)]
    Summary(#[from] PuzzleSummaryError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleHistoryEntry {
    session: PuzzleSessionId,
    attempt: PuzzleAttempt,
    elapsed_active: Duration,
}

impl PuzzleHistoryEntry {
    pub const fn new(
        session: PuzzleSessionId,
        attempt: PuzzleAttempt,
        elapsed_active: Duration,
    ) -> Self {
        Self {
            session,
            attempt,
            elapsed_active,
        }
    }

    pub const fn session(&self) -> PuzzleSessionId {
        self.session
    }

    pub const fn attempt(&self) -> &PuzzleAttempt {
        &self.attempt
    }

    pub const fn elapsed_active(&self) -> Duration {
        self.elapsed_active
    }

    fn matches(&self, scope: PuzzleHistoryEnumScope) -> bool {
        match scope {
            PuzzleHistoryEnumScope::Attempt { session, attempt } => {
                self.session == session && self.attempt.id() == attempt
            }
            PuzzleHistoryEnumScope::Session(session) => self.session == session,
            PuzzleHistoryEnumScope::All => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleHistoryAuthorization {
    scope: PuzzleHistoryEnumScope,
}

impl PuzzleHistoryAuthorization {
    pub fn new(
        scope: PuzzleHistoryEnumScope,
        confirmation: PuzzleHistoryEnumConfirmation,
        active_run: Option<PuzzleSessionId>,
    ) -> Result<Self, PuzzleHistoryError> {
        if confirmation != PuzzleHistoryEnumConfirmation::Confirmed {
            return Err(PuzzleHistoryError::ConfirmationRequired);
        }
        if let Some(session) = active_run {
            let blocked = match scope {
                PuzzleHistoryEnumScope::Attempt {
                    session: target, ..
                }
                | PuzzleHistoryEnumScope::Session(target) => target == session,
                PuzzleHistoryEnumScope::All => true,
            };
            if blocked {
                return Err(PuzzleHistoryError::ActiveRunRefused { session });
            }
        }
        Ok(Self { scope })
    }

    pub const fn scope(&self) -> PuzzleHistoryEnumScope {
        self.scope
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleHistoryDeletion {
    authorization: PuzzleHistoryAuthorization,
    removed: usize,
    summary: PuzzleSummary,
}

impl PuzzleHistoryDeletion {
    pub const fn authorization(&self) -> PuzzleHistoryAuthorization {
        self.authorization
    }

    pub const fn scope(&self) -> PuzzleHistoryEnumScope {
        self.authorization.scope
    }

    pub const fn removed(&self) -> usize {
        self.removed
    }

    pub const fn summary(&self) -> &PuzzleSummary {
        &self.summary
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleHistory {
    entries: Vec<PuzzleHistoryEntry>,
}

impl PuzzleHistory {
    pub fn new(entries: Vec<PuzzleHistoryEntry>) -> Result<Self, PuzzleHistoryError> {
        let history = Self { entries };
        history.summary()?;
        Ok(history)
    }

    pub fn entries(&self) -> &[PuzzleHistoryEntry] {
        &self.entries
    }

    pub fn summary(&self) -> Result<PuzzleSummary, PuzzleHistoryError> {
        Ok(PuzzleSummary::from_attempts(self.entries.iter().map(
            |entry| PuzzleSummaryAttempt::new(entry.session, &entry.attempt, entry.elapsed_active),
        ))?)
    }

    pub fn delete(
        &mut self,
        scope: PuzzleHistoryEnumScope,
        confirmation: PuzzleHistoryEnumConfirmation,
        active_run: Option<PuzzleSessionId>,
    ) -> Result<PuzzleHistoryDeletion, PuzzleHistoryError> {
        let authorization = PuzzleHistoryAuthorization::new(scope, confirmation, active_run)?;
        let removed = self
            .entries
            .iter()
            .filter(|entry| entry.matches(scope))
            .count();
        if removed == 0 {
            return Err(PuzzleHistoryError::NothingToDelete);
        }
        self.entries.retain(|entry| !entry.matches(scope));
        Ok(PuzzleHistoryDeletion {
            authorization,
            removed,
            summary: self.summary()?,
        })
    }
}
