use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{Notify, watch};

use crate::desktop::coordinator::DesktopCoordinatorDialogProvider;
use crate::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileHandleError,
};
use crate::desktop::native::dialogs::DesktopNativeDialogsPort;
use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;

pub struct DesktopNativeDialogsProxy {
    ready: watch::Receiver<Option<DesktopNativeDialogsPort<Arc<DesktopNativeEventLoopWake>>>>,
    cancelled: Notify,
    cancellation: AtomicU64,
}

impl DesktopNativeDialogsProxy {
    pub fn new(
        ready: watch::Receiver<Option<DesktopNativeDialogsPort<Arc<DesktopNativeEventLoopWake>>>>,
    ) -> Self {
        Self {
            ready,
            cancelled: Notify::new(),
            cancellation: AtomicU64::new(0),
        }
    }
}

impl DesktopFileDialogProvider for DesktopNativeDialogsProxy {
    async fn select(
        &self,
        purpose: DesktopFileEnumPurpose,
    ) -> Result<Option<PathBuf>, DesktopFileHandleError> {
        let generation = self.cancellation.load(Ordering::Acquire);
        let mut ready = self.ready.clone();
        loop {
            if self.cancellation.load(Ordering::Acquire) != generation {
                return Ok(None);
            }
            let port = ready.borrow_and_update().clone();
            if let Some(port) = port {
                return port.select(purpose).await;
            }
            tokio::select! {
                biased;
                () = self.cancelled.notified() => {}
                changed = ready.changed() => changed.map_err(|source| DesktopFileHandleError::NativeDialog { source: Box::new(source) })?,
            }
        }
    }
}

impl DesktopCoordinatorDialogProvider for DesktopNativeDialogsProxy {
    fn request_cancel(&self) -> Result<(), DesktopFileHandleError> {
        self.cancellation
            .try_update(Ordering::AcqRel, Ordering::Acquire, |generation| {
                generation.checked_add(1)
            })
            .map_err(|_generation| DesktopFileHandleError::Saturated)?;
        self.cancelled.notify_one();
        let port = self.ready.borrow().clone();
        if let Some(port) = port {
            port.request_cancel()?;
        }
        Ok(())
    }
}
