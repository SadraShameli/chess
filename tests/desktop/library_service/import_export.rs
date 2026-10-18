use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::clock::duration::ClockInstant;
use crate::desktop::file_handle::{
    DesktopFileEnumPurpose, DesktopFileHandleRegistry, DesktopFileHandleRequest,
};
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumImportAction,
    DesktopLibraryServiceEnumImportResult, DesktopLibraryServiceEnumResult,
    DesktopLibraryServiceError, DesktopLibraryServiceExport, DesktopLibraryServiceFile,
    DesktopLibraryServiceImportBackend, DesktopLibraryServiceImportFuture,
    DesktopLibraryServiceImportSession, DesktopLibraryServiceImports,
};
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::notation::pgn_export::NotationPgnExportEnumSelection;
use crate::storage::import_session::{
    StorageImportSessionConfirmation, StorageImportSessionDetailPage,
    StorageImportSessionDetailRequest, StorageImportSessionEnumCancelOutcome,
    StorageImportSessionEnumChoice, StorageImportSessionEnumCommitOutcome,
    StorageImportSessionError, StorageImportSessionPage, StorageImportSessionPageRequest,
    StorageImportSessionSummary,
};

struct DesktopLibraryServiceImportFakeSession {
    cancelled: Arc<AtomicBool>,
}

impl DesktopLibraryServiceImportSession for DesktopLibraryServiceImportFakeSession {
    fn prepare_preview(
        &mut self,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionSummary>> {
        Box::pin(async {
            Ok(Arc::new(StorageImportSessionSummary {
                valid: 2,
                excluded: 1,
                duplicates: 1,
                selected: 1,
            }))
        })
    }

    fn page_rows(
        &mut self,
        _request: StorageImportSessionPageRequest,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionPage>> {
        Box::pin(async {
            Ok(Arc::new(StorageImportSessionPage {
                rows: Vec::new(),
                next: None,
            }))
        })
    }

    fn detail_rows(
        &mut self,
        _request: StorageImportSessionDetailRequest,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionDetailPage>> {
        Box::pin(async { Err(Arc::new(StorageImportSessionError::State)) })
    }

    fn choose_row(
        &mut self,
        _ordinal: u64,
        choice: StorageImportSessionEnumChoice,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionSummary>> {
        Box::pin(async move {
            let kept = u64::from(choice == StorageImportSessionEnumChoice::KeepCopy);
            Ok(Arc::new(StorageImportSessionSummary {
                valid: 2,
                excluded: 1,
                duplicates: 1,
                selected: 1 + kept,
            }))
        })
    }

    fn freeze_preview(
        &mut self,
    ) -> DesktopLibraryServiceImportFuture<'_, StorageImportSessionConfirmation> {
        Box::pin(async { Err(Arc::new(StorageImportSessionError::State)) })
    }

    fn commit_preview(
        &mut self,
        _confirmation: StorageImportSessionConfirmation,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionEnumCommitOutcome>> {
        Box::pin(async { Err(Arc::new(StorageImportSessionError::State)) })
    }

    fn cancel_preview(
        &mut self,
    ) -> DesktopLibraryServiceImportFuture<'_, Arc<StorageImportSessionEnumCancelOutcome>> {
        let cancelled = Arc::clone(&self.cancelled);
        Box::pin(async move {
            cancelled.store(true, Ordering::SeqCst);
            Ok(Arc::new(StorageImportSessionEnumCancelOutcome::Cancelled))
        })
    }
}

struct DesktopLibraryServiceImportFakeBackend {
    cancelled: Arc<AtomicBool>,
}

impl DesktopLibraryServiceImportBackend for DesktopLibraryServiceImportFakeBackend {
    fn begin(
        &self,
        _source: File,
    ) -> Result<Box<dyn DesktopLibraryServiceImportSession>, StorageImportSessionError> {
        Ok(Box::new(DesktopLibraryServiceImportFakeSession {
            cancelled: Arc::clone(&self.cancelled),
        }))
    }
}

async fn selected(
    fixture: &DesktopLibraryServiceFixture,
    path: PathBuf,
    purpose: DesktopFileEnumPurpose,
) -> DesktopLibraryServiceTestResult<DesktopLibraryServiceFile> {
    let mut registry = DesktopFileHandleRegistry::new();
    let owner = DesktopMessagesEntityId::new().file_handle_owner();
    let now = ClockInstant::from_nanos(1);
    let handle = registry
        .register_selection(
            path,
            DesktopFileHandleRequest::new(owner, purpose, now),
            &fixture.executor,
        )
        .await?;
    let selection = registry.consume(handle, owner, purpose, now)?;
    let file = DesktopLibraryServiceFile::new(handle);
    file.fill(selection)?;
    Ok(file)
}

#[tokio::test]
async fn import_session_is_single_active_and_cancel_releases_it() -> DesktopLibraryServiceTestResult
{
    let fixture = DesktopLibraryServiceFixture::new().await?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let imports =
        DesktopLibraryServiceImports::new(Arc::new(DesktopLibraryServiceImportFakeBackend {
            cancelled: Arc::clone(&cancelled),
        }));
    let source = fixture.directory.path().join("games.pgn");
    std::fs::write(&source, "1. e4 *\n")?;
    let missing = imports
        .run(DesktopLibraryServiceEnumImportAction::Freeze)
        .await;
    assert!(matches!(
        missing,
        Err(DesktopLibraryServiceError::ImportNotActive)
    ));
    let file = selected(&fixture, source.clone(), DesktopFileEnumPurpose::ReadFile).await?;
    let prepared = imports
        .run(DesktopLibraryServiceEnumImportAction::Prepare(file))
        .await?;
    let DesktopLibraryServiceEnumImportResult::Preview(summary) = prepared else {
        return Err("missing preview".into());
    };
    assert_eq!(summary.selected(), 1);
    let again = selected(&fixture, source, DesktopFileEnumPurpose::ReadFile).await?;
    assert!(matches!(
        imports
            .run(DesktopLibraryServiceEnumImportAction::Prepare(again))
            .await,
        Err(DesktopLibraryServiceError::ImportActive)
    ));
    let DesktopLibraryServiceEnumImportResult::Preview(kept) = imports
        .run(DesktopLibraryServiceEnumImportAction::Choose {
            ordinal: 1,
            choice: StorageImportSessionEnumChoice::KeepCopy,
        })
        .await?
    else {
        return Err("missing choice preview".into());
    };
    assert_eq!(kept.selected(), 2);
    assert!(matches!(
        imports
            .run(DesktopLibraryServiceEnumImportAction::Freeze)
            .await,
        Err(DesktopLibraryServiceError::Import(_))
    ));
    imports
        .run(DesktopLibraryServiceEnumImportAction::Cancel)
        .await?;
    assert!(cancelled.load(Ordering::SeqCst));
    assert!(matches!(
        imports
            .run(DesktopLibraryServiceEnumImportAction::Cancel)
            .await,
        Err(DesktopLibraryServiceError::ImportNotActive)
    ));
    fixture.close().await
}

#[tokio::test]
async fn import_without_a_session_backend_is_refused_by_the_service()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let source = fixture.directory.path().join("games.pgn");
    std::fs::write(&source, "1. e4 *\n")?;
    let file = selected(&fixture, source, DesktopFileEnumPurpose::ReadFile).await?;
    let refused = fixture
        .execute(DesktopLibraryServiceEnumAction::Import(
            DesktopLibraryServiceEnumImportAction::Prepare(file),
        ))
        .await;
    let message = refused.err().map(|error| error.to_string());
    assert!(message.is_some_and(|message| message.contains("unavailable")));
    fixture.close().await
}

#[tokio::test]
async fn export_writes_full_and_mainline_pgn_to_the_selected_destination_once()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(960, true).await?;
    let game = DesktopLibraryServiceFixture::id(960)?;
    let full = fixture.directory.path().join("full.pgn");
    let mainline = fixture.directory.path().join("mainline.pgn");
    for (path, selection) in [
        (full.clone(), NotationPgnExportEnumSelection::Full),
        (mainline.clone(), NotationPgnExportEnumSelection::Mainline),
    ] {
        let destination = selected(&fixture, path, DesktopFileEnumPurpose::WriteFile).await?;
        let request = DesktopLibraryServiceExport::new(game, revision, selection, destination);
        let result = fixture
            .execute(DesktopLibraryServiceEnumAction::Export(request.clone()))
            .await?;
        let DesktopLibraryServiceEnumResult::Exported(bytes) = result else {
            return Err("missing export result".into());
        };
        assert!(bytes > 0);
        let replay = fixture
            .execute(DesktopLibraryServiceEnumAction::Export(request))
            .await;
        assert!(replay.is_err());
    }
    let full_text = std::fs::read_to_string(&full)?;
    let mainline_text = std::fs::read_to_string(&mainline)?;
    assert!(full_text.contains("d4"));
    assert!(!mainline_text.contains("d4"));
    assert!(mainline_text.contains("e4"));
    fixture.close().await
}

#[tokio::test]
async fn export_refuses_a_stale_revision_before_writing() -> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    fixture.seed(961, true).await?;
    let game = DesktopLibraryServiceFixture::id(961)?;
    let path = fixture.directory.path().join("stale.pgn");
    let destination = selected(&fixture, path.clone(), DesktopFileEnumPurpose::WriteFile).await?;
    let stale = crate::game::storage_repository::GameStorageRevision::new(999);
    let result = fixture
        .execute(DesktopLibraryServiceEnumAction::Export(
            DesktopLibraryServiceExport::new(
                game,
                stale,
                NotationPgnExportEnumSelection::Full,
                destination,
            ),
        ))
        .await;
    assert!(result.is_err());
    assert!(!path.exists());
    fixture.close().await
}
