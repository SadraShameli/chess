use std::collections::HashMap;
use std::fs::File;
use std::future::Future;
use std::path::PathBuf;

use same_file::Handle;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::DialogExt;
use thiserror::Error;
use tokio::sync::oneshot;

use crate::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use crate::desktop::identity::DesktopRequestId;
use crate::storage::atomic_file::{
    StorageAtomicFileError, StorageAtomicFileTarget, StorageFileIdentity,
};

#[derive(Debug, Error)]
pub enum DesktopFileHandleError {
    #[error("The native file handle is unavailable or expired; select the location again")]
    UnknownHandle,
    #[error("The native file handle belongs to a different window or operation")]
    WrongScope,
    #[error("Too many pending file selections")]
    Saturated,
    #[error("The native file dialog closed unexpectedly: {0}")]
    DialogChannel(#[from] oneshot::error::RecvError),
    #[error("The native file dialog failed: {source}")]
    NativeDialog {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("The native selection is not a local filesystem location")]
    NonlocalSelection,
    #[error("The selected location has the wrong file type")]
    WrongType,
    #[error(transparent)]
    File(#[from] StorageAtomicFileError),
    #[error("The selected location is unavailable: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Time(#[from] ClockDurationError),
    #[error(transparent)]
    Capacity(#[from] CapacityExecutorError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesktopFileEnumPurpose {
    ReadFile,
    WriteFile,
    Directory,
}

impl DesktopFileEnumPurpose {
    pub const ALL: [Self; 3] = [Self::ReadFile, Self::WriteFile, Self::Directory];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct DesktopFileHandleId(DesktopRequestId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopFileHandleOwner(DesktopRequestId);

impl DesktopFileHandleOwner {
    pub fn new() -> Self {
        Self(DesktopRequestId::new())
    }

    pub fn from_request_id(value: DesktopRequestId) -> Self {
        Self(value)
    }
}

impl Default for DesktopFileHandleOwner {
    fn default() -> Self {
        Self::new()
    }
}

pub trait DesktopFileDialogProvider: Send + Sync {
    fn select(
        &self,
        purpose: DesktopFileEnumPurpose,
    ) -> impl Future<Output = Result<Option<PathBuf>, DesktopFileHandleError>> + Send;
}

pub struct DesktopFileDialogNative<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> DesktopFileDialogNative<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> DesktopFileDialogProvider for DesktopFileDialogNative<R> {
    async fn select(
        &self,
        purpose: DesktopFileEnumPurpose,
    ) -> Result<Option<PathBuf>, DesktopFileHandleError> {
        let (sender, receiver) = oneshot::channel();
        let callback = move |selection: Option<tauri_plugin_dialog::FilePath>| {
            let result = selection
                .map(|path| {
                    path.into_path()
                        .map_err(|_error| DesktopFileHandleError::NonlocalSelection)
                })
                .transpose();
            if let Err(abandoned) = sender.send(result) {
                drop(abandoned);
            }
        };
        let dialog = self.app.dialog().file();
        match purpose {
            DesktopFileEnumPurpose::ReadFile => dialog.pick_file(callback),
            DesktopFileEnumPurpose::WriteFile => dialog.save_file(callback),
            DesktopFileEnumPurpose::Directory => dialog.pick_folder(callback),
        }
        receiver.await?
    }
}

pub enum DesktopFileEnumSelection {
    Source {
        path: PathBuf,
        identity: StorageFileIdentity,
    },
    Destination(StorageAtomicFileTarget),
    Directory {
        path: PathBuf,
        identity: Handle,
    },
}

impl DesktopFileEnumSelection {
    fn capture(
        path: PathBuf,
        purpose: DesktopFileEnumPurpose,
    ) -> Result<Self, DesktopFileHandleError> {
        match purpose {
            DesktopFileEnumPurpose::ReadFile => {
                let path = path.canonicalize()?;
                let identity = StorageFileIdentity::capture(&path)?;
                Ok(Self::Source { path, identity })
            }
            DesktopFileEnumPurpose::WriteFile => {
                Ok(Self::Destination(StorageAtomicFileTarget::capture(path)?))
            }
            DesktopFileEnumPurpose::Directory => {
                let path = path.canonicalize()?;
                if !path.is_dir() {
                    return Err(DesktopFileHandleError::WrongType);
                }
                let identity = Handle::from_path(&path)?;
                Ok(Self::Directory { path, identity })
            }
        }
    }

    pub fn into_source(self) -> Result<File, DesktopFileHandleError> {
        if let Self::Source { path, identity } = self {
            identity.validate(&path)?;
            let file = File::open(&path)?;
            identity.validate(&path)?;
            if Handle::from_file(file.try_clone()?)? != Handle::from_path(&path)? {
                return Err(StorageAtomicFileError::StaleSelection.into());
            }
            Ok(file)
        } else {
            Err(DesktopFileHandleError::WrongType)
        }
    }

    pub fn into_destination(self) -> Result<StorageAtomicFileTarget, DesktopFileHandleError> {
        if let Self::Destination(target) = self {
            Ok(target)
        } else {
            Err(DesktopFileHandleError::WrongType)
        }
    }

    pub fn into_directory(self) -> Result<PathBuf, DesktopFileHandleError> {
        if let Self::Directory { path, identity } = self {
            if identity != Handle::from_path(&path)? {
                return Err(StorageAtomicFileError::StaleSelection.into());
            }
            Ok(path)
        } else {
            Err(DesktopFileHandleError::WrongType)
        }
    }
}

struct DesktopFileHandleEntry {
    owner: DesktopFileHandleOwner,
    purpose: DesktopFileEnumPurpose,
    expires: ClockInstant,
    selection: DesktopFileEnumSelection,
}

pub struct DesktopFileHandleRequest {
    owner: DesktopFileHandleOwner,
    purpose: DesktopFileEnumPurpose,
    now: ClockInstant,
}

impl DesktopFileHandleRequest {
    pub fn new(
        owner: DesktopFileHandleOwner,
        purpose: DesktopFileEnumPurpose,
        now: ClockInstant,
    ) -> Self {
        Self {
            owner,
            purpose,
            now,
        }
    }
}

#[derive(Default)]
pub struct DesktopFileHandleRegistry {
    entries: HashMap<DesktopFileHandleId, DesktopFileHandleEntry>,
}

impl DesktopFileHandleRegistry {
    pub const CAPACITY: usize = 256;
    const LIFETIME: ClockDuration = ClockDuration::from_nanos(600_000_000_000);

    pub fn new() -> Self {
        Self::default()
    }

    pub async fn select<P: DesktopFileDialogProvider>(
        &mut self,
        provider: &P,
        request: DesktopFileHandleRequest,
        executor: &CapacityExecutor,
    ) -> Result<Option<DesktopFileHandleId>, DesktopFileHandleError> {
        self.check_capacity(request.now)?;
        let Some(path) = provider.select(request.purpose).await? else {
            return Ok(None);
        };
        self.register_selection(path, request, executor)
            .await
            .map(Some)
    }

    pub async fn register_selection(
        &mut self,
        path: PathBuf,
        request: DesktopFileHandleRequest,
        executor: &CapacityExecutor,
    ) -> Result<DesktopFileHandleId, DesktopFileHandleError> {
        let DesktopFileHandleRequest {
            owner,
            purpose,
            now,
        } = request;
        self.check_capacity(now)?;
        let selection = executor
            .submit(move |_cancellation| DesktopFileEnumSelection::capture(path, purpose))?
            .join()
            .await??;
        let expires = now.checked_add(Self::LIFETIME)?;
        let id = DesktopFileHandleId(DesktopRequestId::new());
        self.entries.insert(
            id,
            DesktopFileHandleEntry {
                owner,
                purpose,
                expires,
                selection,
            },
        );
        Ok(id)
    }

    fn check_capacity(&mut self, now: ClockInstant) -> Result<(), DesktopFileHandleError> {
        self.entries
            .retain(|_id, entry| !now.has_reached(entry.expires));
        if self.entries.len() >= Self::CAPACITY {
            return Err(DesktopFileHandleError::Saturated);
        }
        Ok(())
    }

    pub fn revoke(&mut self, id: DesktopFileHandleId) -> bool {
        self.entries.remove(&id).is_some()
    }

    pub fn consume(
        &mut self,
        id: DesktopFileHandleId,
        owner: DesktopFileHandleOwner,
        purpose: DesktopFileEnumPurpose,
        now: ClockInstant,
    ) -> Result<DesktopFileEnumSelection, DesktopFileHandleError> {
        let entry = self
            .entries
            .get(&id)
            .ok_or(DesktopFileHandleError::UnknownHandle)?;
        if now.has_reached(entry.expires) {
            self.entries.remove(&id);
            return Err(DesktopFileHandleError::UnknownHandle);
        }
        if entry.owner != owner || entry.purpose != purpose {
            return Err(DesktopFileHandleError::WrongScope);
        }
        self.entries
            .remove(&id)
            .map(|entry| entry.selection)
            .ok_or(DesktopFileHandleError::UnknownHandle)
    }

    pub fn revoke_owner(&mut self, owner: DesktopFileHandleOwner) {
        self.entries.retain(|_id, entry| entry.owner != owner);
    }
}
