use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::watch;
use tokio::time::{Instant, timeout_at};
use uuid::Uuid;

use crate::capacity::executor::CapacityExecutor;
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::library_prepared::StorageLibraryPreparedAdoption;
use crate::storage::library_repository::{
    StorageLibraryAdoptionCandidate, StorageLibraryAdoptionCursor, StorageLibraryAdoptionIssue,
    StorageLibraryAdoptionPage, StorageLibraryCatalogRevision, StorageLibraryEnumAdoptionOutcome,
    StorageLibraryEnumAdoptionPreparation, StorageLibraryEnumMutationReceipt, StorageLibraryError,
    StorageLibraryMutation, StorageLibraryRepository,
};

#[derive(Debug, Error)]
pub enum DesktopNativeLibraryStartupError {
    #[error(transparent)]
    Library(#[from] StorageLibraryError),
    #[error("Library startup exceeded its retained absolute deadline: {0}")]
    Deadline(#[source] tokio::time::error::Elapsed),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeLibraryStartupEnumStage {
    ReadingPage,
    Preparing,
    Adopting,
    Complete,
}

#[derive(Debug, Clone, Copy)]
pub struct DesktopNativeLibraryStartupProgress {
    sweep: u8,
    observed: u64,
    adopted: u64,
    unrelated: u64,
    last: Option<Uuid>,
    stage: DesktopNativeLibraryStartupEnumStage,
}

impl DesktopNativeLibraryStartupProgress {
    pub const fn sweep(self) -> u8 {
        self.sweep
    }
    pub const fn observed(self) -> u64 {
        self.observed
    }
    pub const fn adopted(self) -> u64 {
        self.adopted
    }
    pub const fn unrelated(self) -> u64 {
        self.unrelated
    }
    pub const fn last_observed(self) -> Option<Uuid> {
        self.last
    }
    pub const fn stage(self) -> DesktopNativeLibraryStartupEnumStage {
        self.stage
    }
}

#[derive(Debug, Clone)]
pub enum DesktopNativeLibraryStartupEnumIssueCause {
    Adoption(Arc<StorageLibraryAdoptionIssue>),
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
    catalog_changed: bool,
}

impl DesktopNativeLibraryStartupSummary {
    pub const MAX_ISSUES: usize = crate::storage::library_query::StorageLibraryQuery::PAGE_SIZE;

    pub fn issues(&self) -> &[DesktopNativeLibraryStartupIssue] {
        &self.issues
    }
    pub const fn issue_count(&self) -> u64 {
        self.issue_count
    }
    pub const fn is_catalog_unsettled(&self) -> bool {
        self.catalog_changed
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
    Adopting(DesktopNativeLibraryStartupProgress),
    Ready(StorageLibraryCatalogRevision),
    RecoveryRequired(Arc<DesktopNativeLibraryStartupSummary>),
}

type DesktopNativeLibraryStartupPageFuture =
    Pin<Box<dyn Future<Output = Result<StorageLibraryAdoptionPage, StorageLibraryError>> + Send>>;
type DesktopNativeLibraryStartupPrepareFuture = Pin<
    Box<
        dyn Future<Output = Result<StorageLibraryEnumAdoptionPreparation, StorageLibraryError>>
            + Send,
    >,
>;

enum DesktopNativeLibraryStartupEnumOperation {
    Page(DesktopNativeLibraryStartupPageFuture),
    Prepare(
        StorageLibraryAdoptionCandidate,
        DesktopNativeLibraryStartupPrepareFuture,
    ),
    Adopt(StorageLibraryAdoptionCandidate, StorageLibraryMutation),
}

enum DesktopNativeLibraryStartupEnumObservation {
    Page(Result<StorageLibraryAdoptionPage, StorageLibraryError>),
    Prepared(
        StorageLibraryAdoptionCandidate,
        Result<StorageLibraryEnumAdoptionPreparation, StorageLibraryError>,
    ),
    Adopted(
        StorageLibraryAdoptionCandidate,
        Result<StorageLibraryEnumMutationReceipt, StorageLibraryError>,
    ),
}

impl DesktopNativeLibraryStartupEnumOperation {
    async fn wait(&mut self) -> DesktopNativeLibraryStartupEnumObservation {
        match self {
            Self::Page(future) => DesktopNativeLibraryStartupEnumObservation::Page(future.await),
            Self::Prepare(candidate, future) => {
                DesktopNativeLibraryStartupEnumObservation::Prepared(*candidate, future.await)
            }
            Self::Adopt(candidate, mutation) => {
                DesktopNativeLibraryStartupEnumObservation::Adopted(
                    *candidate,
                    mutation.wait().await,
                )
            }
        }
    }
}

pub struct DesktopNativeLibraryStartup {
    client: StorageDocumentClient,
    executor: Arc<CapacityExecutor>,
    deadline: Instant,
    operation: Option<DesktopNativeLibraryStartupEnumOperation>,
    prepared: Option<(
        StorageLibraryAdoptionCandidate,
        StorageLibraryPreparedAdoption,
    )>,
    page: Option<StorageLibraryAdoptionPage>,
    index: usize,
    cursor: StorageLibraryAdoptionCursor,
    catalog: Option<StorageLibraryCatalogRevision>,
    sweep_changed: bool,
    progress: DesktopNativeLibraryStartupProgress,
    summary: DesktopNativeLibraryStartupSummary,
    readiness: watch::Sender<DesktopNativeLibraryStartupEnumReadiness>,
}

impl DesktopNativeLibraryStartup {
    pub const MAX_SWEEPS: u8 = 3;

    pub fn new(
        client: StorageDocumentClient,
        executor: Arc<CapacityExecutor>,
        deadline: Instant,
    ) -> Self {
        let progress = DesktopNativeLibraryStartupProgress {
            sweep: 1,
            observed: 0,
            adopted: 0,
            unrelated: 0,
            last: None,
            stage: DesktopNativeLibraryStartupEnumStage::ReadingPage,
        };
        let (readiness, _receiver) =
            watch::channel(DesktopNativeLibraryStartupEnumReadiness::Adopting(progress));
        Self {
            client,
            executor,
            deadline,
            operation: None,
            prepared: None,
            page: None,
            index: 0,
            cursor: StorageLibraryAdoptionCursor::start(),
            catalog: None,
            sweep_changed: false,
            progress,
            summary: DesktopNativeLibraryStartupSummary::default(),
            readiness,
        }
    }

    pub const fn deadline(&self) -> Instant {
        self.deadline
    }
    pub const fn progress(&self) -> DesktopNativeLibraryStartupProgress {
        self.progress
    }
    pub fn readiness(&self) -> DesktopNativeLibraryStartupEnumReadiness {
        self.readiness.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<DesktopNativeLibraryStartupEnumReadiness> {
        self.readiness.subscribe()
    }
    pub fn has_pending(&self) -> bool {
        self.operation.is_some()
    }

    pub async fn run(&mut self) -> Result<(), DesktopNativeLibraryStartupError> {
        while self.advance().await? {}
        Ok(())
    }

    pub async fn advance(&mut self) -> Result<bool, DesktopNativeLibraryStartupError> {
        if self.progress.stage == DesktopNativeLibraryStartupEnumStage::Complete {
            return Ok(false);
        }
        self.begin_operation();
        if self.operation.is_some() {
            self.observe_operation(self.deadline).await?;
        }
        Ok(self.progress.stage != DesktopNativeLibraryStartupEnumStage::Complete)
    }

    pub async fn drain_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopNativeLibraryStartupError> {
        while self.operation.is_some() {
            self.observe_operation(deadline).await?;
        }
        Ok(())
    }

    fn repository(&self) -> StorageLibraryRepository<'_> {
        StorageLibraryRepository::new(self.client.clone(), &self.executor)
    }

    fn publish_progress(&self) {
        self.readiness
            .send_replace(DesktopNativeLibraryStartupEnumReadiness::Adopting(
                self.progress,
            ));
    }

    fn begin_operation(&mut self) {
        if self.operation.is_some() {
            return;
        }
        if let Some((candidate, prepared)) = self.prepared.take() {
            let admission = self.repository().begin_adopt(prepared);
            match admission {
                Ok(mutation) => {
                    self.operation = Some(DesktopNativeLibraryStartupEnumOperation::Adopt(
                        candidate, mutation,
                    ));
                }
                Err(error) => {
                    self.record_candidate(
                        candidate,
                        DesktopNativeLibraryStartupEnumIssueCause::Storage(Arc::new(error)),
                    );
                    self.complete_candidate(candidate);
                }
            }
            return;
        }
        let candidate = self
            .page
            .as_ref()
            .and_then(|page| page.candidates().get(self.index))
            .copied();
        let client = self.client.clone();
        let executor = Arc::clone(&self.executor);
        self.operation = Some(if let Some(candidate) = candidate {
            self.progress.stage = DesktopNativeLibraryStartupEnumStage::Preparing;
            DesktopNativeLibraryStartupEnumOperation::Prepare(
                candidate,
                Box::pin(async move {
                    StorageLibraryRepository::new(client, &executor)
                        .prepare_adoption(candidate)
                        .await
                }),
            )
        } else {
            self.page = None;
            self.progress.stage = DesktopNativeLibraryStartupEnumStage::ReadingPage;
            let cursor = self.cursor;
            DesktopNativeLibraryStartupEnumOperation::Page(Box::pin(async move {
                StorageLibraryRepository::new(client, &executor)
                    .adoption_page(cursor)
                    .await
            }))
        });
        self.publish_progress();
    }

    async fn observe_operation(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopNativeLibraryStartupError> {
        let operation = self
            .operation
            .as_mut()
            .ok_or(StorageLibraryError::Protocol)?;
        let observation = timeout_at(deadline, operation.wait())
            .await
            .map_err(DesktopNativeLibraryStartupError::Deadline)?;
        self.operation = None;
        match observation {
            DesktopNativeLibraryStartupEnumObservation::Page(result) => self.observe_page(result?),
            DesktopNativeLibraryStartupEnumObservation::Prepared(candidate, result) => {
                self.observe_preparation(candidate, result)
            }
            DesktopNativeLibraryStartupEnumObservation::Adopted(candidate, result) => {
                match result {
                    Ok(StorageLibraryEnumMutationReceipt::Adopted(
                        StorageLibraryEnumAdoptionOutcome::Inserted(catalog),
                    )) => {
                        if self
                            .catalog
                            .and_then(|previous| previous.value().checked_add(1))
                            != Some(catalog.value())
                        {
                            self.sweep_changed = true;
                        }
                        self.catalog = Some(catalog);
                        self.progress.adopted = self.progress.adopted.saturating_add(1);
                    }
                    Ok(StorageLibraryEnumMutationReceipt::Adopted(
                        StorageLibraryEnumAdoptionOutcome::AlreadyCurrent,
                    )) => {}
                    Ok(StorageLibraryEnumMutationReceipt::Adopted(
                        StorageLibraryEnumAdoptionOutcome::Changed(actual),
                    )) => {
                        self.record_candidate(
                            candidate,
                            DesktopNativeLibraryStartupEnumIssueCause::Changed(actual),
                        );
                    }
                    Ok(
                        StorageLibraryEnumMutationReceipt::Saved(_)
                        | StorageLibraryEnumMutationReceipt::Organized(_)
                        | StorageLibraryEnumMutationReceipt::DestructionApplied(_)
                        | StorageLibraryEnumMutationReceipt::Restored(_),
                    ) => {
                        self.record_candidate(
                            candidate,
                            DesktopNativeLibraryStartupEnumIssueCause::Storage(Arc::new(
                                StorageLibraryError::Protocol,
                            )),
                        );
                    }
                    Err(error) => self.record_candidate(
                        candidate,
                        DesktopNativeLibraryStartupEnumIssueCause::Storage(Arc::new(error)),
                    ),
                }
                self.complete_candidate(candidate);
            }
        }
        Ok(())
    }

    fn observe_page(&mut self, page: StorageLibraryAdoptionPage) {
        let catalog = page.catalog_revision();
        if self.catalog.is_some_and(|expected| expected != catalog) {
            self.sweep_changed = true;
        }
        self.catalog = Some(catalog);
        if page.candidates().is_empty() {
            if self.sweep_changed && self.progress.sweep < Self::MAX_SWEEPS {
                self.progress.sweep += 1;
                self.progress.last = None;
                self.cursor = StorageLibraryAdoptionCursor::start();
                self.sweep_changed = false;
                self.publish_progress();
            } else {
                self.summary.catalog_changed = self.sweep_changed;
                self.progress.stage = DesktopNativeLibraryStartupEnumStage::Complete;
                self.publish_complete(catalog);
            }
        } else {
            self.index = 0;
            self.page = Some(page);
            self.publish_progress();
        }
    }

    fn observe_preparation(
        &mut self,
        candidate: StorageLibraryAdoptionCandidate,
        result: Result<StorageLibraryEnumAdoptionPreparation, StorageLibraryError>,
    ) {
        match result {
            Ok(StorageLibraryEnumAdoptionPreparation::Prepared(prepared)) => {
                self.prepared = Some((candidate, prepared));
                self.progress.stage = DesktopNativeLibraryStartupEnumStage::Adopting;
                self.publish_progress();
                return;
            }
            Ok(StorageLibraryEnumAdoptionPreparation::Unrelated) => {
                self.progress.unrelated = self.progress.unrelated.saturating_add(1);
            }
            Ok(StorageLibraryEnumAdoptionPreparation::Changed(actual)) => self.record_candidate(
                candidate,
                DesktopNativeLibraryStartupEnumIssueCause::Changed(actual),
            ),
            Ok(StorageLibraryEnumAdoptionPreparation::Issue(issue)) => self.record_candidate(
                candidate,
                DesktopNativeLibraryStartupEnumIssueCause::Adoption(Arc::new(issue)),
            ),
            Err(error) => self.record_candidate(
                candidate,
                DesktopNativeLibraryStartupEnumIssueCause::Storage(Arc::new(error)),
            ),
        }
        self.complete_candidate(candidate);
    }

    fn record_candidate(
        &mut self,
        candidate: StorageLibraryAdoptionCandidate,
        cause: DesktopNativeLibraryStartupEnumIssueCause,
    ) {
        self.summary.record(DesktopNativeLibraryStartupIssue {
            id: candidate.id(),
            revision: candidate.content_revision(),
            cause,
        });
    }

    fn complete_candidate(&mut self, candidate: StorageLibraryAdoptionCandidate) {
        self.cursor = StorageLibraryAdoptionCursor::after(candidate.id());
        self.index += 1;
        self.progress.observed = self.progress.observed.saturating_add(1);
        self.progress.last = Some(candidate.id());
        self.publish_progress();
    }

    fn publish_complete(&self, catalog: StorageLibraryCatalogRevision) {
        let readiness = if self.summary.issue_count == 0 && !self.summary.catalog_changed {
            DesktopNativeLibraryStartupEnumReadiness::Ready(catalog)
        } else {
            DesktopNativeLibraryStartupEnumReadiness::RecoveryRequired(Arc::new(
                self.summary.clone(),
            ))
        };
        self.readiness.send_replace(readiness);
    }

    pub(crate) fn observe_recovery_catalog(&mut self, catalog: StorageLibraryCatalogRevision) {
        if self.catalog != Some(catalog) {
            self.summary.catalog_changed = true;
            self.readiness.send_replace(
                DesktopNativeLibraryStartupEnumReadiness::RecoveryRequired(Arc::new(
                    self.summary.clone(),
                )),
            );
        }
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
