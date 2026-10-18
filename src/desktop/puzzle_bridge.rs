use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::watch;

use crate::clock::duration::ClockDurationError;
use crate::desktop::application::{DesktopApplicationReceipt, DesktopApplicationWakeError};
use crate::desktop::puzzle_snapshot::DesktopPuzzleSnapshot;
use crate::puzzle::checkpoint::PuzzleCheckpointEnumStatus;
use crate::puzzle::persistence::PuzzlePersistenceEnumCommand;
use crate::puzzle::run::PuzzleRunEnumConfirmation;
use crate::puzzle::session::{PuzzleSessionContext, PuzzleSessionId};
use crate::puzzle::summary::PuzzleSummary;
use crate::puzzle::timer::PuzzleTimerEnumReadiness;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopPuzzleBridgeEnumOperation {
    Command(PuzzlePersistenceEnumCommand),
    RetrySave,
    Snapshot,
    Summary,
}

impl DesktopPuzzleBridgeEnumOperation {
    pub const fn is_urgent(&self) -> bool {
        matches!(
            self,
            Self::Command(
                PuzzlePersistenceEnumCommand::Pause(_)
                    | PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Hidden)
                    | PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Confirmed)
            )
        )
    }

    pub const fn is_available_while_closing(&self) -> bool {
        matches!(
            self,
            Self::RetrySave
                | Self::Snapshot
                | Self::Summary
                | Self::Command(
                    PuzzlePersistenceEnumCommand::Pause(_)
                        | PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Hidden)
                )
        )
    }
}

pub struct DesktopPuzzleBridgeRequest {
    pub receipt: DesktopApplicationReceipt,
    pub expected: PuzzleSessionContext,
    pub operation: DesktopPuzzleBridgeEnumOperation,
}

#[derive(Debug, Clone)]
pub enum DesktopPuzzleBridgeEnumResult {
    Snapshot(Arc<DesktopPuzzleSnapshot>),
    Summary {
        snapshot: Arc<DesktopPuzzleSnapshot>,
        summary: PuzzleSummary,
    },
}

impl DesktopPuzzleBridgeEnumResult {
    pub fn snapshot(&self) -> &Arc<DesktopPuzzleSnapshot> {
        match self {
            Self::Snapshot(snapshot) | Self::Summary { snapshot, .. } => snapshot,
        }
    }
}

#[derive(Debug, Error)]
pub enum DesktopPuzzleBridgeError {
    #[error("The puzzle request queue is full")]
    Busy,
    #[error("The retained puzzle actor is unavailable")]
    Closed,
    #[error("The puzzle actor is draining for shutdown")]
    Closing,
    #[error("The puzzle request was cancelled before acceptance")]
    Cancelled,
    #[error("The puzzle shutdown deadline was reached before the newest state was durable")]
    ShutdownDeadline,
    #[error("Puzzle shutdown requires explicit save recovery: {0:?}")]
    SaveRequired(PuzzleCheckpointEnumStatus),
    #[error("The newest puzzle state has not been proved paused, hidden and durable")]
    Incomplete,
    #[error("The puzzle command owner refused the operation: {0}")]
    Commands(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    Clock(#[from] ClockDurationError),
    #[error(transparent)]
    Wake(#[from] DesktopApplicationWakeError),
    #[error("The puzzle outcome channel closed: {0}")]
    Outcome(#[from] watch::error::RecvError),
}

#[derive(Debug, Clone)]
pub enum DesktopPuzzleBridgeEnumHealth {
    Starting,
    Ready,
    SaveBlocked {
        status: PuzzleCheckpointEnumStatus,
        failure: Option<Arc<DesktopPuzzleBridgeError>>,
    },
    ShuttingDown,
    ShutdownComplete,
    Failed(Arc<DesktopPuzzleBridgeError>),
}

pub(crate) type DesktopPuzzleBridgeOutcome<T> = Result<T, Arc<DesktopPuzzleBridgeError>>;
pub(crate) type DesktopPuzzleBridgeOutcomeSender<T> =
    watch::Sender<Option<DesktopPuzzleBridgeOutcome<T>>>;

#[derive(Clone)]
pub struct DesktopPuzzleBridgeReceipt<T: Clone = DesktopPuzzleBridgeEnumResult> {
    outcome: watch::Receiver<Option<DesktopPuzzleBridgeOutcome<T>>>,
}

impl<T: Clone> DesktopPuzzleBridgeReceipt<T> {
    pub(crate) fn channel() -> (DesktopPuzzleBridgeOutcomeSender<T>, Self) {
        let (sender, outcome) = watch::channel(None);
        (sender, Self { outcome })
    }

    pub async fn wait(&mut self) -> DesktopPuzzleBridgeOutcome<T> {
        loop {
            if let Some(outcome) = self.outcome.borrow_and_update().clone() {
                return outcome;
            }
            self.outcome
                .changed()
                .await
                .map_err(|error| Arc::new(DesktopPuzzleBridgeError::Outcome(error)))?;
        }
    }
}

pub type DesktopPuzzleBridgeFuture =
    Pin<Box<dyn Future<Output = DesktopPuzzleBridgeOutcome<DesktopPuzzleBridgeEnumResult>> + Send>>;

pub trait DesktopPuzzleBridgeProvider: Send + Sync {
    fn session_id(&self) -> PuzzleSessionId;

    fn submit(
        &self,
        request: DesktopPuzzleBridgeRequest,
    ) -> DesktopPuzzleBridgeOutcome<DesktopPuzzleBridgeReceipt>;

    fn execute(&self, request: DesktopPuzzleBridgeRequest) -> DesktopPuzzleBridgeFuture {
        let admitted = self.submit(request);
        Box::pin(async move { admitted?.wait().await })
    }

    fn snapshots(&self) -> watch::Receiver<Option<Arc<DesktopPuzzleSnapshot>>>;
    fn health(&self) -> watch::Receiver<DesktopPuzzleBridgeEnumHealth>;
}
