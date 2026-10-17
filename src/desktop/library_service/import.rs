use std::fmt::{Debug, Formatter};
use std::fs::File;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::desktop::library_service::file::DesktopLibraryServiceFile;
use crate::desktop::library_service::DesktopLibraryServiceError;
use crate::storage::import_session::{
    StorageImportSession, StorageImportSessionConfirmation, StorageImportSessionDetailPage,
    StorageImportSessionDetailRequest, StorageImportSessionEnumCancelOutcome,
    StorageImportSessionEnumChoice, StorageImportSessionEnumCommitOutcome,
    StorageImportSessionError, StorageImportSessionFilesystemProvider,
    StorageImportSessionIdentityProvider, StorageImportSessionPage,
    StorageImportSessionPageRequest, StorageImportSessionRepository,
    StorageImportSessionResources, StorageImportSessionSummary,
};

#[derive(Clone, PartialEq, Eq)]
pub enum DesktopLibraryServiceEnumImportAction {
    Prepare(DesktopLibraryServiceFile),
    Page(StorageImportSessionPageRequest),
    Detail(StorageImportSessionDetailRequest),
    Choose {
        ordinal: u64,
        choice: StorageImportSessionEnumChoice,
    },
    Freeze,
    Commit(StorageImportSessionConfirmation),
    Cancel,
}

impl Debug for DesktopLibraryServiceEnumImportAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Prepare(file) => formatter.debug_tuple("Prepare").field(file).finish(),
            Self::Page(request) => formatter.debug_tuple("Page").field(request).finish(),
            Self::Detail(request) => formatter.debug_tuple("Detail").field(request).finish(),
            Self::Choose { ordinal, choice } => formatter
                .debug_struct("Choose")
                .field("ordinal", ordinal)
                .field("choice", choice)
                .finish(),
            Self::Freeze => formatter.write_str("Freeze"),
            Self::Commit(confirmation) => formatter
                .debug_tuple("Commit")
                .field(&confirmation.digest())
                .finish(),
            Self::Cancel => formatter.write_str("Cancel"),
        }
    }
}

#[derive(Clone)]
pub enum DesktopLibraryServiceEnumImportResult {
    Preview(Arc<StorageImportSessionSummary>),
    Page(Arc<StorageImportSessionPage>),
    Detail(Arc<StorageImportSessionDetailPage>),
    Confirmation(StorageImportSessionConfirmation),
    Committed(Arc<StorageImportSessionEnumCommitOutcome>),
    Cancelled(Arc<StorageImportSessionEnumCancelOutcome>),
}

impl Debug for DesktopLibraryServiceEnumImportResult {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Preview(summary) => formatter.debug_tuple("Preview").field(summary).finish(),
            Self::Page(page) => formatter
                .debug_struct("Page")
                .field("rows", &page.rows().len())
                .field("next", &page.next_cursor())
                .finish(),
            Self::Detail(_) => formatter.write_str("Detail"),
            Self::Confirmation(confirmation) => formatter
                .debug_tuple("Confirmation")
                .field(&confirmation.digest())
                .finish(),
            Self::Committed(outcome) => formatter.debug_tuple("Committed").field(outcome).finish(),
            Self::Cancelled(outcome) => formatter.debug_tuple("Cancelled").field(outcome).finish(),
        }
    }
}

pub type DesktopLibraryServiceImportFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, Arc<StorageImportSessionError>>> + Send + 'a>>;

pub trait DesktopLibraryServiceImportSession: Send {
    fn prepare_preview(
        &mut self,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionSummary>>;
    fn page_rows(
        &mut self,
        request: StorageImportSessionPageRequest,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionPage>>;
    fn detail_rows(
        &mut self,
        request: StorageImportSessionDetailRequest,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionDetailPage>>;
    fn choose_row(
        &mut self,
        ordinal: u64,
        choice: StorageImportSessionEnumChoice,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionSummary>>;
    fn freeze_preview(
        &mut self,
    ) -> DesktopLibraryServiceImportFuture<'_, StorageImportSessionConfirmation>;
    fn commit_preview(
        &mut self,
        confirmation: StorageImportSessionConfirmation,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionEnumCommitOutcome>>;
    fn cancel_preview(
        &mut self,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionEnumCancelOutcome>>;
}

impl<R, F, I> DesktopLibraryServiceImportSession for StorageImportSession<R, F, I>
where
    R: StorageImportSessionRepository,
    F: StorageImportSessionFilesystemProvider,
    I: StorageImportSessionIdentityProvider,
{
    fn prepare_preview(
        &mut self,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionSummary>> {
        Box::pin(self.prepare())
    }
    fn page_rows(
        &mut self,
        request: StorageImportSessionPageRequest,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionPage>> {
        Box::pin(self.page(request))
    }
    fn detail_rows(
        &mut self,
        request: StorageImportSessionDetailRequest,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionDetailPage>> {
        Box::pin(self.detail(request))
    }
    fn choose_row(
        &mut self,
        ordinal: u64,
        choice: StorageImportSessionEnumChoice,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionSummary>> {
        Box::pin(self.choose(ordinal, choice))
    }
    fn freeze_preview(
        &mut self,
    ) -> DesktopLibraryServiceImportFuture<'_, StorageImportSessionConfirmation> {
        Box::pin(self.freeze())
    }
    fn commit_preview(
        &mut self,
        confirmation: StorageImportSessionConfirmation,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionEnumCommitOutcome>> {
        Box::pin(self.commit(confirmation))
    }
    fn cancel_preview(
        &mut self,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionEnumCancelOutcome>> {
        Box::pin(self.cancel_retained())
    }
}

pub trait DesktopLibraryServiceImportBackend: Send + Sync {
    fn begin(
        &self,
        source: File,
    ) -> Result<Box<dyn DesktopLibraryServiceImportSession>, StorageImportSessionError>;
}

pub struct DesktopLibraryServiceImportStorage<R, F, I> {
    resources: StorageImportSessionResources<R, F, I>,
}

impl<R, F, I> DesktopLibraryServiceImportStorage<R, F, I> {
    pub const fn new(resources: StorageImportSessionResources<R, F, I>) -> Self {
        Self { resources }
    }
}

impl<R, F, I> DesktopLibraryServiceImportBackend for DesktopLibraryServiceImportStorage<R, F, I>
where
    R: StorageImportSessionRepository,
    F: StorageImportSessionFilesystemProvider,
    I: StorageImportSessionIdentityProvider,
{
    fn begin(
        &self,
        source: File,
    ) -> Result<Box<dyn DesktopLibraryServiceImportSession>, StorageImportSessionError> {
        let session = StorageImportSession::new(
            source,
            StorageImportSessionResources {
                executor: Arc::clone(&self.resources.executor),
                repository: Arc::clone(&self.resources.repository),
                filesystem: Arc::clone(&self.resources.filesystem),
                identities: Arc::clone(&self.resources.identities),
            },
        )?;
        Ok(Box::new(session))
    }
}

pub struct DesktopLibraryServiceImports {
    backend: Arc<dyn DesktopLibraryServiceImportBackend>,
    session: Mutex<Option<Box<dyn DesktopLibraryServiceImportSession>>>,
}

impl DesktopLibraryServiceImports {
    pub fn new(backend: Arc<dyn DesktopLibraryServiceImportBackend>) -> Self {
        Self {
            backend,
            session: Mutex::new(None),
        }
    }

    pub(super) async fn run(
        &self,
        action: DesktopLibraryServiceEnumImportAction,
    ) -> Result<DesktopLibraryServiceEnumImportResult, DesktopLibraryServiceError> {
        let mut slot = self.session.lock().await;
        match action {
            DesktopLibraryServiceEnumImportAction::Prepare(file) => {
                if slot.is_some() {
                    return Err(DesktopLibraryServiceError::ImportActive);
                }
                let source = file.take()?.into_source().map_err(|error| {
                    DesktopLibraryServiceError::FileSelection(Arc::new(error))
                })?;
                let mut session = self
                    .backend
                    .begin(source)
                    .map_err(|error| DesktopLibraryServiceError::Import(Arc::new(error)))?;
                match session.prepare_preview().await {
                    Ok(summary) => {
                        *slot = Some(session);
                        Ok(DesktopLibraryServiceEnumImportResult::Preview(summary))
                    }
                    Err(error) => {
                        let cleanup = session.cancel_preview().await;
                        drop(cleanup);
                        Err(DesktopLibraryServiceError::Import(error))
                    }
                }
            }
            DesktopLibraryServiceEnumImportAction::Page(request) => {
                let session = Self::active(&mut slot)?;
                Ok(DesktopLibraryServiceEnumImportResult::Page(
                    session
                        .page_rows(request)
                        .await
                        .map_err(DesktopLibraryServiceError::Import)?,
                ))
            }
            DesktopLibraryServiceEnumImportAction::Detail(request) => {
                let session = Self::active(&mut slot)?;
                Ok(DesktopLibraryServiceEnumImportResult::Detail(
                    session
                        .detail_rows(request)
                        .await
                        .map_err(DesktopLibraryServiceError::Import)?,
                ))
            }
            DesktopLibraryServiceEnumImportAction::Choose { ordinal, choice } => {
                let session = Self::active(&mut slot)?;
                Ok(DesktopLibraryServiceEnumImportResult::Preview(
                    session
                        .choose_row(ordinal, choice)
                        .await
                        .map_err(DesktopLibraryServiceError::Import)?,
                ))
            }
            DesktopLibraryServiceEnumImportAction::Freeze => {
                let session = Self::active(&mut slot)?;
                Ok(DesktopLibraryServiceEnumImportResult::Confirmation(
                    session
                        .freeze_preview()
                        .await
                        .map_err(DesktopLibraryServiceError::Import)?,
                ))
            }
            DesktopLibraryServiceEnumImportAction::Commit(confirmation) => {
                let session = Self::active(&mut slot)?;
                let outcome = session
                    .commit_preview(confirmation)
                    .await
                    .map_err(DesktopLibraryServiceError::Import)?;
                *slot = None;
                Ok(DesktopLibraryServiceEnumImportResult::Committed(outcome))
            }
            DesktopLibraryServiceEnumImportAction::Cancel => {
                let mut session = slot
                    .take()
                    .ok_or(DesktopLibraryServiceError::ImportNotActive)?;
                let outcome = session
                    .cancel_preview()
                    .await
                    .map_err(DesktopLibraryServiceError::Import)?;
                Ok(DesktopLibraryServiceEnumImportResult::Cancelled(outcome))
            }
        }
    }

    fn active<'a>(
        slot: &'a mut Option<Box<dyn DesktopLibraryServiceImportSession>>,
    ) -> Result<&'a mut Box<dyn DesktopLibraryServiceImportSession>, DesktopLibraryServiceError>
    {
        slot.as_mut().ok_or(DesktopLibraryServiceError::ImportNotActive)
    }
}
