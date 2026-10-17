use std::sync::Arc;

use tokio::sync::watch;
use uuid::Uuid;

use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_repository::StorageLibraryError;

#[derive(Debug, Clone)]
pub enum DesktopNativeLibraryStartupEnumIssueCause {
    Storage(Arc<StorageLibraryError>),
    Changed(Option<GameStorageRevision>),
    Missing,
    ContentKind,
}

#[derive(Debug, Clone)]
pub struct DesktopNativeLibraryStartupIssue {
    id: Uuid,
    revision: GameStorageRevision,
    cause: DesktopNativeLibraryStartupEnumIssueCause,
}

impl DesktopNativeLibraryStartupIssue {
    pub const fn id(&self) -> Uuid {
        self.id
    }
    pub const fn content_revision(&self) -> GameStorageRevision {
        self.revision
    }
    pub fn cause(&self) -> &DesktopNativeLibraryStartupEnumIssueCause {
        &self.cause
    }
}

#[derive(Debug, Clone, Default)]
pub struct DesktopNativeLibraryStartupSummary {
    issues: Vec<DesktopNativeLibraryStartupIssue>,
    issue_count: u64,
}

impl DesktopNativeLibraryStartupSummary {
    pub const MAX_ISSUES: usize = crate::storage::library_query::StorageLibraryQuery::PAGE_SIZE;

    pub fn issues(&self) -> &[DesktopNativeLibraryStartupIssue] {
        &self.issues
    }
    pub const fn issue_count(&self) -> u64 {
        self.issue_count
    }

    fn record(&mut self, issue: DesktopNativeLibraryStartupIssue) {
        self.issue_count = self.issue_count.saturating_add(1);
        if self.issues.len() < Self::MAX_ISSUES {
            self.issues.push(issue);
        }
    }
}

#[derive(Debug, Clone)]
pub enum DesktopNativeLibraryStartupEnumReadiness {
    Ready,
    RecoveryRequired(Arc<DesktopNativeLibraryStartupSummary>),
}

pub struct DesktopNativeLibraryStartup {
    summary: DesktopNativeLibraryStartupSummary,
    readiness: watch::Sender<DesktopNativeLibraryStartupEnumReadiness>,
}

impl DesktopNativeLibraryStartup {
    pub fn new() -> Self {
        let (readiness, _receiver) =
            watch::channel(DesktopNativeLibraryStartupEnumReadiness::Ready);
        Self {
            summary: DesktopNativeLibraryStartupSummary::default(),
            readiness,
        }
    }

    pub fn readiness(&self) -> DesktopNativeLibraryStartupEnumReadiness {
        self.readiness.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<DesktopNativeLibraryStartupEnumReadiness> {
        self.readiness.subscribe()
    }

    pub(crate) fn record_recovery_issue(
        &mut self,
        id: Uuid,
        revision: GameStorageRevision,
        cause: DesktopNativeLibraryStartupEnumIssueCause,
    ) {
        self.summary.record(DesktopNativeLibraryStartupIssue {
            id,
            revision,
            cause,
        });
        self.readiness
            .send_replace(DesktopNativeLibraryStartupEnumReadiness::RecoveryRequired(
                Arc::new(self.summary.clone()),
            ));
    }
}

impl Default for DesktopNativeLibraryStartup {
    fn default() -> Self {
        Self::new()
    }
}
