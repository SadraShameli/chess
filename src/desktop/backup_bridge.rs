use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::ClockInstant;
use crate::desktop::backup_commands::{
    DesktopBackupCommands, DesktopBackupCommandsEntry, DesktopBackupCommandsError,
    DesktopBackupCommandsPrerequisites, DesktopBackupCommandsPreviewHandle,
};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::profile::DesktopProfileLease;
use crate::storage::backup::create::BackupSpaceNative;
use crate::storage::backup::preview::BackupPreviewEnumFinding;
use crate::storage::backup::scheduler::{BackupEnumDue, BackupScheduler, BackupState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopBackupBridgeEnumOperation {
    Status,
    BackupNow,
    RetryAutomatic,
    Preview {
        backup: Uuid,
    },
    Restore {
        preview: DesktopBackupCommandsPreviewHandle,
        prerequisites: DesktopBackupCommandsPrerequisites,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopBackupBridgeStatus {
    pub state: BackupState,
    pub entries: Vec<DesktopBackupCommandsEntry>,
    pub restore_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopBackupBridgeEnumResult {
    Status(DesktopBackupBridgeStatus),
    Preview {
        handle: DesktopBackupCommandsPreviewHandle,
        findings: Vec<BackupPreviewEnumFinding>,
    },
    RestoreStaged {
        safety_backup: Uuid,
    },
}

pub struct DesktopBackupBridgeRequest {
    pub received_at: ClockInstant,
    pub operation: DesktopBackupBridgeEnumOperation,
}

pub type DesktopBackupBridgeFuture = Pin<
    Box<
        dyn Future<Output = Result<DesktopBackupBridgeEnumResult, DesktopFailure>> + Send + 'static,
    >,
>;

pub trait DesktopBackupBridgeProvider: Send + Sync {
    fn execute(&self, request: DesktopBackupBridgeRequest) -> DesktopBackupBridgeFuture;
}

type DesktopBackupServiceCommands = Arc<Mutex<DesktopBackupCommands<BackupSpaceNative>>>;

pub struct DesktopBackupService {
    executor: Arc<CapacityExecutor>,
    commands: DesktopBackupServiceCommands,
    lease: Arc<DesktopProfileLease>,
    scheduler: Mutex<Option<JoinHandle<()>>>,
}

impl DesktopBackupService {
    pub const SCHEDULER_POLL: Duration = Duration::from_secs(15 * 60);

    pub fn new(
        executor: Arc<CapacityExecutor>,
        lease: Arc<DesktopProfileLease>,
        commands: DesktopBackupCommands<BackupSpaceNative>,
    ) -> Self {
        Self {
            executor,
            commands: Arc::new(Mutex::new(commands)),
            lease,
            scheduler: Mutex::new(None),
        }
    }

    fn failure(error: &DesktopBackupCommandsError) -> DesktopFailure {
        let code = match error {
            DesktopBackupCommandsError::Prerequisite(_) => DesktopEnumFailureCode::ResourceRefusal,
            DesktopBackupCommandsError::UnknownPreview(_) => DesktopEnumFailureCode::StaleRevision,
            DesktopBackupCommandsError::Backup(_) => DesktopEnumFailureCode::FileUnavailable,
        };
        DesktopFailure::new(code, error.to_string(), true)
    }

    fn internal(message: &str) -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::InternalFailure,
            message.to_owned(),
            true,
        )
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            })
    }

    async fn blocking<T: Send + 'static>(
        executor: Arc<CapacityExecutor>,
        work: impl FnOnce() -> Result<T, DesktopBackupCommandsError> + Send + 'static,
    ) -> Result<T, DesktopFailure> {
        let work = AssertUnwindSafe(work);
        let job = executor
            .submit(move |_cancellation| {
                let work = work;
                (work.0)()
            })
            .map_err(|error| Self::internal(&error.to_string()))?;
        match job.join().await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(Self::failure(&error)),
            Err(error) => Err(Self::internal(&error.to_string())),
        }
    }

    fn status(
        commands: &DesktopBackupCommands<BackupSpaceNative>,
    ) -> Result<DesktopBackupBridgeStatus, DesktopBackupCommandsError> {
        Ok(DesktopBackupBridgeStatus {
            state: commands.state_blocking()?,
            entries: commands.list_blocking()?,
            restore_pending: commands.restore_pending_blocking()?,
        })
    }

    pub fn start_scheduler(self: &Arc<Self>) {
        let service = Arc::clone(self);
        let handle = tokio::spawn(async move {
            let mut scheduler = BackupScheduler::new();
            loop {
                let now_ms = Self::now_ms();
                let commands = Arc::clone(&service.commands);
                let due = Self::blocking(Arc::clone(&service.executor), move || {
                    let guard = commands.lock().map_err(|_poisoned| poisoned_error())?;
                    guard.note_user_data_blocking(now_ms)?;
                    Ok(guard.state_blocking()?)
                })
                .await;
                if let Ok(state) = due
                    && scheduler.take_attempt(&state, now_ms)
                {
                    let commands = Arc::clone(&service.commands);
                    let outcome = Self::blocking(Arc::clone(&service.executor), move || {
                        let guard = commands.lock().map_err(|_poisoned| poisoned_error())?;
                        guard
                            .automatic_blocking(Uuid::new_v4(), now_ms)
                            .map(|_archive| ())
                    })
                    .await;
                    scheduler.finish_attempt(outcome.is_ok());
                }
                tokio::time::sleep(Self::SCHEDULER_POLL).await;
            }
        });
        if let Ok(mut slot) = self.scheduler.lock() {
            *slot = Some(handle);
        }
    }

    pub async fn shutdown(&self) {
        let handle = self.scheduler.lock().ok().and_then(|mut slot| slot.take());
        if let Some(handle) = handle {
            handle.abort();
            drop(handle.await);
        }
    }

    pub fn is_scheduler_stopped(&self) -> bool {
        self.scheduler.lock().map_or(true, |slot| slot.is_none())
    }
}

fn poisoned_error() -> DesktopBackupCommandsError {
    DesktopBackupCommandsError::Backup(crate::storage::backup::error::BackupError::Io(
        std::io::Error::other("The backup service state is poisoned"),
    ))
}

impl DesktopBackupBridgeProvider for DesktopBackupService {
    fn execute(&self, request: DesktopBackupBridgeRequest) -> DesktopBackupBridgeFuture {
        let executor = Arc::clone(&self.executor);
        let commands = Arc::clone(&self.commands);
        let lease = Arc::clone(&self.lease);
        let now_ms = Self::now_ms();
        let operation = request.operation;
        Box::pin(async move {
            Self::blocking(executor, move || {
                let mut guard = commands.lock().map_err(|_poisoned| poisoned_error())?;
                match operation {
                    DesktopBackupBridgeEnumOperation::Status => {
                        Ok(DesktopBackupBridgeEnumResult::Status(Self::status(&guard)?))
                    }
                    DesktopBackupBridgeEnumOperation::BackupNow => {
                        guard.backup_now_blocking(Uuid::new_v4(), now_ms)?;
                        Ok(DesktopBackupBridgeEnumResult::Status(Self::status(&guard)?))
                    }
                    DesktopBackupBridgeEnumOperation::RetryAutomatic => {
                        let state = guard.state_blocking()?;
                        if !matches!(
                            BackupScheduler::due(&state, now_ms),
                            BackupEnumDue::Due { .. }
                        ) && state.warning().is_none()
                        {
                            return Ok(DesktopBackupBridgeEnumResult::Status(Self::status(
                                &guard,
                            )?));
                        }
                        guard.automatic_blocking(Uuid::new_v4(), now_ms)?;
                        Ok(DesktopBackupBridgeEnumResult::Status(Self::status(&guard)?))
                    }
                    DesktopBackupBridgeEnumOperation::Preview { backup } => {
                        let path = guard.find_blocking(backup)?;
                        let (handle, findings) = guard.preview_blocking(&path)?;
                        Ok(DesktopBackupBridgeEnumResult::Preview { handle, findings })
                    }
                    DesktopBackupBridgeEnumOperation::Restore {
                        preview,
                        prerequisites,
                    } => {
                        let outcome = guard.stage_restore_blocking(
                            &lease,
                            preview,
                            prerequisites,
                            Uuid::new_v4(),
                            Uuid::new_v4(),
                            now_ms,
                        )?;
                        Ok(DesktopBackupBridgeEnumResult::RestoreStaged {
                            safety_backup: outcome.safety_backup(),
                        })
                    }
                }
            })
            .await
        })
    }
}
