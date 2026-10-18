use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::Arc;

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::ClockInstant;
use chess::desktop::backup_bridge::{
    DesktopBackupBridgeEnumOperation, DesktopBackupBridgeEnumResult, DesktopBackupBridgeProvider,
    DesktopBackupBridgeRequest, DesktopBackupService,
};
use chess::desktop::backup_commands::{DesktopBackupCommands, DesktopBackupCommandsPrerequisites};
use chess::desktop::error::DesktopEnumFailureCode;
use chess::desktop::profile::{
    DesktopProfileAdmission, DesktopProfileEnumAdmission, DesktopProfileId, DesktopProfileLease,
    DesktopProfileOsLock, DesktopProfileSecurity,
};
use chess::storage::backup::create::BackupSpaceNative;
use chess::storage::backup::journal::BackupRestoreJournal;
use chess::storage::document_store::StorageDocumentStore;
use tempfile::TempDir;
use uuid::Uuid;

type DesktopBackupBridgeTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct DesktopBackupBridgeFixture {
    _directory: TempDir,
    lease: Arc<DesktopProfileLease>,
    _executor: Arc<CapacityExecutor>,
    service: DesktopBackupService,
}

impl DesktopBackupBridgeFixture {
    async fn new() -> DesktopBackupBridgeTestResult<Self> {
        let directory = TempDir::new()?;
        let security = DesktopProfileSecurity::current()?;
        let profile = DesktopProfileId::from_str("d92f840f-e575-4654-92bd-79b3ba8b8e55")?;
        let admission = DesktopProfileAdmission::new(security, DesktopProfileOsLock);
        let DesktopProfileEnumAdmission::Owned(lease) =
            admission.acquire(&directory.path().join("profile"), profile)?
        else {
            return Err("expected profile ownership".into());
        };
        let lease: Arc<DesktopProfileLease> = Arc::from(lease);
        let data = lease.identity().data_directory().to_path_buf();
        let database = data.join("library.sqlite3");
        let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("empty budget")?)?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store = StorageDocumentStore::open(database.clone(), budget).await?;
        store
            .client()
            .save(Uuid::from_u128(1), 0, r#"{"v":"original"}"#.to_owned())
            .await?;
        store.close().await?;
        let service = DesktopBackupService::new(
            Arc::clone(&executor),
            Arc::clone(&lease),
            DesktopBackupCommands::new(
                database,
                data.join("backups"),
                BackupSpaceNative,
                Vec::new(),
                Vec::new(),
            ),
        );
        Ok(Self {
            _directory: directory,
            lease,
            _executor: executor,
            service,
        })
    }

    async fn run(
        &self,
        operation: DesktopBackupBridgeEnumOperation,
    ) -> Result<DesktopBackupBridgeEnumResult, chess::desktop::error::DesktopFailure> {
        self.service
            .execute(DesktopBackupBridgeRequest {
                received_at: ClockInstant::from_nanos(0),
                operation,
                selection: None,
            })
            .await
    }
}

const READY: DesktopBackupCommandsPrerequisites = DesktopBackupCommandsPrerequisites {
    games_paused_and_saved: true,
    editors_have_no_unsaved_edits: true,
};

#[tokio::test]
async fn status_backup_now_preview_and_a_staged_restore_flow_through_the_bridge()
-> DesktopBackupBridgeTestResult {
    let fixture = DesktopBackupBridgeFixture::new().await?;
    let DesktopBackupBridgeEnumResult::Status(empty) = fixture
        .run(DesktopBackupBridgeEnumOperation::Status)
        .await?
    else {
        return Err("expected status".into());
    };
    assert!(empty.entries.is_empty());
    assert!(!empty.restore_pending);
    let DesktopBackupBridgeEnumResult::Status(after) = fixture
        .run(DesktopBackupBridgeEnumOperation::BackupNow)
        .await?
    else {
        return Err("expected status".into());
    };
    assert_eq!(after.entries.len(), 1);
    let backup = after.entries[0].manifest.id();
    let DesktopBackupBridgeEnumResult::Preview { handle, .. } = fixture
        .run(DesktopBackupBridgeEnumOperation::Preview { backup })
        .await?
    else {
        return Err("expected preview".into());
    };
    assert_eq!(handle.id(), backup);
    let blocked = fixture
        .run(DesktopBackupBridgeEnumOperation::Restore {
            preview: handle,
            prerequisites: DesktopBackupCommandsPrerequisites {
                games_paused_and_saved: false,
                editors_have_no_unsaved_edits: true,
            },
        })
        .await;
    assert!(
        blocked.is_err_and(|failure| failure.code() == DesktopEnumFailureCode::ResourceRefusal)
    );
    let staged = fixture
        .run(DesktopBackupBridgeEnumOperation::Restore {
            preview: handle,
            prerequisites: READY,
        })
        .await?;
    assert!(matches!(
        staged,
        DesktopBackupBridgeEnumResult::RestoreStaged { .. }
    ));
    let DesktopBackupBridgeEnumResult::Status(pending) = fixture
        .run(DesktopBackupBridgeEnumOperation::Status)
        .await?
    else {
        return Err("expected status".into());
    };
    assert!(pending.restore_pending);
    assert!(
        BackupRestoreJournal::read_blocking(fixture.lease.identity().data_directory())?.is_some()
    );
    let unknown = fixture
        .run(DesktopBackupBridgeEnumOperation::Preview {
            backup: Uuid::from_u128(999),
        })
        .await;
    assert!(unknown.is_err());
    Ok(())
}

#[tokio::test]
async fn shutdown_without_a_started_scheduler_is_a_no_op() -> DesktopBackupBridgeTestResult {
    let fixture = DesktopBackupBridgeFixture::new().await?;
    assert!(fixture.service.is_scheduler_stopped());
    fixture.service.shutdown().await;
    assert!(fixture.service.is_scheduler_stopped());
    Ok(())
}
