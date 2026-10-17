use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::watch;

use crate::capacity::executor::CapacityExecutorError;
use crate::clock::duration::ClockDurationError;
use crate::desktop::application::{DesktopApplicationReceipt, DesktopApplicationWakeError};
use crate::desktop::identity::DesktopRevision;
use crate::desktop::puzzle_bridge::{
    DesktopPuzzleBridgeEnumHealth, DesktopPuzzleBridgeEnumOperation, DesktopPuzzleBridgeEnumResult,
    DesktopPuzzleBridgeError,
};
use crate::desktop::puzzle_snapshot::DesktopPuzzleSnapshot;
use crate::puzzle::persistence::PuzzlePersistenceStorageRevision;
use crate::puzzle::session::{PuzzleSessionContext, PuzzleSessionId};
use crate::puzzle::summary::PuzzleSummary;
use crate::puzzle::timer::PuzzleTimerEnumPauseReason;
use crate::storage::puzzle_repository::{StoragePuzzleError, StoragePuzzleRunPage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopPuzzleServiceEnumOperation {
    Inventory {
        after: Option<PuzzleSessionId>,
    },
    OpenSaved {
        id: PuzzleSessionId,
        storage_revision: PuzzlePersistenceStorageRevision,
    },
    Session {
        expected: PuzzleSessionContext,
        operation: DesktopPuzzleBridgeEnumOperation,
    },
    Interrupt {
        session: PuzzleSessionId,
        reason: PuzzleTimerEnumPauseReason,
    },
}

impl DesktopPuzzleServiceEnumOperation {
    pub fn expected_revision(&self) -> DesktopRevision {
        match self {
            Self::Inventory { .. } | Self::Interrupt { .. } => DesktopRevision::ZERO,
            Self::OpenSaved {
                storage_revision, ..
            } => DesktopRevision::for_puzzle_storage(*storage_revision),
            Self::Session { expected, .. } => DesktopRevision::for_puzzle(expected.revision()),
        }
    }

    pub const fn is_urgent(&self) -> bool {
        match self {
            Self::Interrupt { .. } => true,
            Self::Session { operation, .. } => operation.is_urgent(),
            Self::Inventory { .. } | Self::OpenSaved { .. } => false,
        }
    }

    pub const fn is_available_while_closing(&self) -> bool {
        match self {
            Self::Interrupt { .. } => true,
            Self::Session { operation, .. } => operation.is_available_while_closing(),
            Self::Inventory { .. } | Self::OpenSaved { .. } => false,
        }
    }
}

pub struct DesktopPuzzleServiceRequest {
    pub receipt: DesktopApplicationReceipt,
    pub operation: DesktopPuzzleServiceEnumOperation,
}

#[derive(Clone)]
pub enum DesktopPuzzleServiceEnumResult {
    Inventory(Arc<StoragePuzzleRunPage>),
    Opened(Arc<DesktopPuzzleSnapshot>),
    Ended {
        id: PuzzleSessionId,
        storage_revision: PuzzlePersistenceStorageRevision,
        summary: PuzzleSummary,
    },
    Session(DesktopPuzzleBridgeEnumResult),
}

impl DesktopPuzzleServiceEnumResult {
    pub fn revision(&self) -> Option<DesktopRevision> {
        match self {
            Self::Inventory(_) => None,
            Self::Opened(snapshot) => {
                Some(DesktopRevision::for_puzzle(snapshot.context().revision()))
            }
            Self::Ended {
                storage_revision, ..
            } => Some(DesktopRevision::for_puzzle_storage(*storage_revision)),
            Self::Session(result) => Some(DesktopRevision::for_puzzle(
                result.snapshot().context().revision(),
            )),
        }
    }
}

impl std::fmt::Debug for DesktopPuzzleServiceEnumResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inventory(page) => formatter
                .debug_tuple("Inventory")
                .field(&page.entries())
                .finish(),
            Self::Opened(snapshot) => formatter.debug_tuple("Opened").field(snapshot).finish(),
            Self::Ended {
                id,
                storage_revision,
                summary,
            } => formatter
                .debug_struct("Ended")
                .field("id", id)
                .field("storage_revision", storage_revision)
                .field("summary", summary)
                .finish(),
            Self::Session(result) => formatter.debug_tuple("Session").field(result).finish(),
        }
    }
}

#[derive(Debug, Error)]
pub enum DesktopPuzzleServiceError {
    #[error("The saved puzzle service is at capacity")]
    Busy,
    #[error("The saved puzzle service is unavailable")]
    Closed,
    #[error("The saved puzzle service has already started")]
    Started,
    #[error("The saved puzzle service is closing")]
    Closing,
    #[error("The saved puzzle request was cancelled")]
    Cancelled,
    #[error("The saved puzzle run {0:?} is missing")]
    Missing(PuzzleSessionId),
    #[error("The saved puzzle run {id:?} changed from revision {expected:?} to {actual:?}")]
    RevisionConflict {
        id: PuzzleSessionId,
        expected: PuzzlePersistenceStorageRevision,
        actual: PuzzlePersistenceStorageRevision,
    },
    #[error("The requested puzzle session {expected:?} is not the selected session {selected:?}")]
    SessionMismatch {
        expected: PuzzleSessionId,
        selected: Option<PuzzleSessionId>,
    },
    #[error("Replacement requires a paused, hidden, durable and fully observed puzzle owner")]
    ReplacementBlocked,
    #[error("The saved puzzle service shutdown deadline was reached")]
    ShutdownDeadline,
    #[error("The saved puzzle service has unobserved work")]
    Incomplete,
    #[error("The puzzle runtime refused the operation: {0}")]
    Runtime(#[source] Arc<DesktopPuzzleBridgeError>),
    #[error("The puzzle runtime worker failed before returning its owner: {0}")]
    Worker(#[source] Arc<tokio::task::JoinError>),
    #[error("The recovered puzzle could not be prepared: {0}")]
    Preparation(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    Storage(#[from] StoragePuzzleError),
    #[error(transparent)]
    Capacity(#[from] CapacityExecutorError),
    #[error(transparent)]
    Clock(#[from] ClockDurationError),
    #[error(transparent)]
    Wake(#[from] DesktopApplicationWakeError),
    #[error("The saved puzzle outcome channel closed: {0}")]
    Outcome(#[from] watch::error::RecvError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopPuzzleServiceEnumActivity {
    Idle,
    Loading { id: PuzzleSessionId },
    Replacing { id: PuzzleSessionId },
    Selected,
    Closing,
    ShutdownComplete,
}

#[derive(Debug, Clone)]
pub struct DesktopPuzzleServiceNativeProgress(Arc<()>);

impl DesktopPuzzleServiceNativeProgress {
    pub(crate) fn new() -> Self {
        Self(Arc::new(()))
    }

    pub fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Debug, Clone)]
pub struct DesktopPuzzleServiceState {
    selected: Option<PuzzleSessionId>,
    snapshot: Option<Arc<DesktopPuzzleSnapshot>>,
    activity: DesktopPuzzleServiceEnumActivity,
    health: Option<DesktopPuzzleBridgeEnumHealth>,
    failure: Option<Arc<DesktopPuzzleServiceError>>,
    native_admission_progress: Option<DesktopPuzzleServiceNativeProgress>,
}

impl DesktopPuzzleServiceState {
    pub(crate) const fn new(
        selected: Option<PuzzleSessionId>,
        snapshot: Option<Arc<DesktopPuzzleSnapshot>>,
        activity: DesktopPuzzleServiceEnumActivity,
        health: Option<DesktopPuzzleBridgeEnumHealth>,
        failure: Option<Arc<DesktopPuzzleServiceError>>,
    ) -> Self {
        Self {
            selected,
            snapshot,
            activity,
            health,
            failure,
            native_admission_progress: None,
        }
    }

    pub(crate) fn with_native_admission_progress(
        mut self,
        progress: DesktopPuzzleServiceNativeProgress,
    ) -> Self {
        self.native_admission_progress = Some(progress);
        self
    }

    pub const fn native_admission_progress(&self) -> Option<&DesktopPuzzleServiceNativeProgress> {
        self.native_admission_progress.as_ref()
    }

    pub const fn selected(&self) -> Option<PuzzleSessionId> {
        self.selected
    }
    pub const fn snapshot(&self) -> Option<&Arc<DesktopPuzzleSnapshot>> {
        self.snapshot.as_ref()
    }
    pub const fn activity(&self) -> DesktopPuzzleServiceEnumActivity {
        self.activity
    }
    pub const fn health(&self) -> Option<&DesktopPuzzleBridgeEnumHealth> {
        self.health.as_ref()
    }
    pub const fn failure(&self) -> Option<&Arc<DesktopPuzzleServiceError>> {
        self.failure.as_ref()
    }
}

pub type DesktopPuzzleServiceOutcome<T> = Result<T, Arc<DesktopPuzzleServiceError>>;
pub(crate) type DesktopPuzzleServiceOutcomeSender<T> =
    watch::Sender<Option<DesktopPuzzleServiceOutcome<T>>>;

#[derive(Clone)]
pub struct DesktopPuzzleServiceReceipt<T: Clone = DesktopPuzzleServiceEnumResult> {
    outcome: watch::Receiver<Option<DesktopPuzzleServiceOutcome<T>>>,
}

impl<T: Clone> DesktopPuzzleServiceReceipt<T> {
    pub(crate) fn channel() -> (DesktopPuzzleServiceOutcomeSender<T>, Self) {
        let (sender, outcome) = watch::channel(None);
        (sender, Self { outcome })
    }

    pub fn try_result(&mut self) -> Option<DesktopPuzzleServiceOutcome<T>> {
        let closed = self.outcome.has_changed().err();
        self.outcome.borrow_and_update().clone().or_else(|| {
            closed.map(|error| Err(Arc::new(DesktopPuzzleServiceError::Outcome(error))))
        })
    }

    pub async fn wait(&mut self) -> DesktopPuzzleServiceOutcome<T> {
        loop {
            if let Some(outcome) = self.try_result() {
                return outcome;
            }
            self.outcome
                .changed()
                .await
                .map_err(|error| Arc::new(DesktopPuzzleServiceError::Outcome(error)))?;
        }
    }
}

pub type DesktopPuzzleServiceFuture = Pin<
    Box<dyn Future<Output = DesktopPuzzleServiceOutcome<DesktopPuzzleServiceEnumResult>> + Send>,
>;

pub trait DesktopPuzzleServiceProvider: Send + Sync {
    fn submit(
        &self,
        request: DesktopPuzzleServiceRequest,
    ) -> DesktopPuzzleServiceOutcome<DesktopPuzzleServiceReceipt>;
    fn execute(&self, request: DesktopPuzzleServiceRequest) -> DesktopPuzzleServiceFuture {
        let admitted = self.submit(request);
        Box::pin(async move { admitted?.wait().await })
    }
    fn interrupt(
        &self,
        session: PuzzleSessionId,
        reason: PuzzleTimerEnumPauseReason,
    ) -> DesktopPuzzleServiceOutcome<DesktopPuzzleServiceReceipt>;
    fn state(&self) -> watch::Receiver<DesktopPuzzleServiceState>;
}
