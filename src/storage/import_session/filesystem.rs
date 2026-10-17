use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;
#[cfg(unix)]
use std::sync::Mutex;

use crate::storage::import_session::model::{
    StorageImportSessionEnumVolume, StorageImportSessionError,
    StorageImportSessionFilesystemProvider, StorageImportSessionSpace,
};

pub trait StorageImportSessionSpaceProvider:
    Send + Sync + std::panic::RefUnwindSafe + 'static
{
    fn observe(
        &self,
        anchor: &File,
    ) -> Result<StorageImportSessionSpace, StorageImportSessionError>;
}

pub struct StorageImportSessionFilesystem<S> {
    stage: Arc<File>,
    library: Arc<File>,
    space: S,
    #[cfg(unix)]
    orphan: Mutex<Vec<StorageImportSessionFilesystemOrphan>>,
    #[cfg(windows)]
    directory: std::path::PathBuf,
}

#[cfg(unix)]
struct StorageImportSessionFilesystemOrphan {
    file: File,
    name: String,
}

impl<S: StorageImportSessionSpaceProvider> StorageImportSessionFilesystem<S> {
    pub fn new(
        stage: File,
        library: File,
        space: S,
        #[cfg(windows)] directory: std::path::PathBuf,
    ) -> Self {
        Self {
            stage: Arc::new(stage),
            library: Arc::new(library),
            space,
            #[cfg(unix)]
            orphan: Mutex::new(Vec::new()),
            #[cfg(windows)]
            directory,
        }
    }

    #[cfg(unix)]
    fn anonymous(&self) -> Result<File, StorageImportSessionError> {
        self.cleanup_retained()?;
        let name = format!(".chess-import-{}", uuid::Uuid::new_v4());
        let fd = rustix::fs::openat(
            self.stage.as_ref(),
            name.as_str(),
            rustix::fs::OFlags::RDWR
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .map_err(std::io::Error::from)?;
        let file = File::from(fd);
        if let Err(error) = rustix::fs::unlinkat(
            self.stage.as_ref(),
            name.as_str(),
            rustix::fs::AtFlags::empty(),
        ) {
            let mut orphan = self
                .orphan
                .lock()
                .map_err(|_error| StorageImportSessionError::Poisoned)?;
            orphan.push(StorageImportSessionFilesystemOrphan { file, name });
            return Err(std::io::Error::from(error).into());
        }
        Ok(file)
    }

    #[cfg(windows)]
    fn anonymous(&self) -> Result<File, StorageImportSessionError> {
        Ok(tempfile::tempfile_in(&self.directory)?)
    }

    #[cfg(unix)]
    fn cleanup_orphan(
        &self,
        orphan: &StorageImportSessionFilesystemOrphan,
    ) -> Result<(), StorageImportSessionError> {
        let current = match rustix::fs::statat(
            self.stage.as_ref(),
            orphan.name.as_str(),
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Ok(current) => current,
            Err(rustix::io::Errno::NOENT) => return Ok(()),
            Err(error) => return Err(std::io::Error::from(error).into()),
        };
        let owned = rustix::fs::fstat(&orphan.file).map_err(std::io::Error::from)?;
        if current.st_dev != owned.st_dev || current.st_ino != owned.st_ino {
            return Err(StorageImportSessionError::ReplacedOrphan);
        }
        rustix::fs::unlinkat(
            self.stage.as_ref(),
            orphan.name.as_str(),
            rustix::fs::AtFlags::empty(),
        )
        .map_err(std::io::Error::from)?;
        Ok(())
    }
}

impl<S: StorageImportSessionSpaceProvider> StorageImportSessionFilesystemProvider
    for StorageImportSessionFilesystem<S>
{
    fn create_spool(&self) -> Result<File, StorageImportSessionError> {
        self.anonymous()
    }

    fn duplicate(&self, file: &File) -> std::io::Result<File> {
        file.try_clone()
    }

    fn observe_space(
        &self,
        volume: StorageImportSessionEnumVolume,
    ) -> Result<StorageImportSessionSpace, StorageImportSessionError> {
        let anchor = match volume {
            StorageImportSessionEnumVolume::Stage => &self.stage,
            StorageImportSessionEnumVolume::Library => &self.library,
        };
        self.space.observe(anchor)
    }

    fn read_at(&self, file: &mut File, offset: u64, bytes: &mut [u8]) -> std::io::Result<usize> {
        file.seek(SeekFrom::Start(offset))?;
        file.read(bytes)
    }

    fn write_at(&self, file: &mut File, offset: u64, bytes: &[u8]) -> std::io::Result<()> {
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(bytes)
    }

    fn length(&self, file: &File) -> std::io::Result<u64> {
        Ok(file.metadata()?.len())
    }
    fn flush(&self, file: &mut File) -> std::io::Result<()> {
        file.sync_all()
    }
    fn release(&self, file: &mut File) -> std::io::Result<()> {
        file.set_len(0)
    }

    fn cleanup_retained(&self) -> Result<(), StorageImportSessionError> {
        #[cfg(unix)]
        {
            let pending = {
                let mut orphan = self
                    .orphan
                    .lock()
                    .map_err(|_error| StorageImportSessionError::Poisoned)?;
                std::mem::take(&mut *orphan)
            };
            let mut retained = Vec::new();
            let mut failure = None;
            for orphan in pending {
                match self.cleanup_orphan(&orphan) {
                    Ok(()) => drop(orphan.file),
                    Err(error) => {
                        let cleanup = Arc::new(error);
                        failure = Some(match failure {
                            None => cleanup,
                            Some(primary) => {
                                Arc::new(StorageImportSessionError::Cleanup { primary, cleanup })
                            }
                        });
                        retained.push(orphan);
                    }
                }
            }
            if !retained.is_empty() {
                self.orphan
                    .lock()
                    .map_err(|_error| StorageImportSessionError::Poisoned)?
                    .extend(retained);
            }
            if let Some(error) = failure {
                return Err(StorageImportSessionError::Retained(error));
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
pub struct StorageImportSessionUnixSpace;

#[cfg(unix)]
impl StorageImportSessionUnixSpace {
    fn unsigned(value: impl TryInto<u64>) -> Result<u64, StorageImportSessionError> {
        value
            .try_into()
            .map_err(|_error| StorageImportSessionError::Arithmetic)
    }
}

#[cfg(unix)]
impl StorageImportSessionSpaceProvider for StorageImportSessionUnixSpace {
    fn observe(
        &self,
        anchor: &File,
    ) -> Result<StorageImportSessionSpace, StorageImportSessionError> {
        let volume = rustix::fs::fstatvfs(anchor).map_err(std::io::Error::from)?;
        let identity = rustix::fs::fstat(anchor).map_err(std::io::Error::from)?;
        let available = Self::unsigned(volume.f_bavail)?
            .checked_mul(Self::unsigned(volume.f_frsize)?)
            .ok_or(StorageImportSessionError::Arithmetic)?;
        Ok(StorageImportSessionSpace::new(
            Self::unsigned(identity.st_dev)?,
            available,
        ))
    }
}

#[cfg(windows)]
pub struct StorageImportSessionWindowsSpace {
    anchors: [StorageImportSessionWindowsAnchor; 2],
}

#[cfg(windows)]
struct StorageImportSessionWindowsAnchor {
    path: std::path::PathBuf,
    volume: u64,
    index: u64,
}

#[cfg(windows)]
impl StorageImportSessionWindowsSpace {
    pub fn new(
        stage: &std::path::Path,
        library: &std::path::Path,
    ) -> Result<Self, StorageImportSessionError> {
        Ok(Self {
            anchors: [Self::anchor(stage)?, Self::anchor(library)?],
        })
    }

    fn anchor(
        path: &std::path::Path,
    ) -> Result<StorageImportSessionWindowsAnchor, StorageImportSessionError> {
        let handle = winapi_util::Handle::from_path_any(path)?;
        let information = winapi_util::file::information(&handle)?;
        Ok(StorageImportSessionWindowsAnchor {
            path: path.to_path_buf(),
            volume: information.volume_serial_number(),
            index: information.file_index(),
        })
    }
}

#[cfg(windows)]
impl StorageImportSessionSpaceProvider for StorageImportSessionWindowsSpace {
    fn observe(
        &self,
        anchor: &File,
    ) -> Result<StorageImportSessionSpace, StorageImportSessionError> {
        let information = winapi_util::file::information(anchor)?;
        let known = self
            .anchors
            .iter()
            .find(|known| {
                known.volume == information.volume_serial_number()
                    && known.index == information.file_index()
            })
            .ok_or(StorageImportSessionError::State)?;
        let text = known
            .path
            .to_str()
            .ok_or(StorageImportSessionError::State)?;
        let root = winsafe::GetVolumePathName(text).map_err(std::io::Error::other)?;
        let mut available = 0_u64;
        winsafe::GetDiskFreeSpaceEx(Some(&root), Some(&mut available), None, None)
            .map_err(std::io::Error::other)?;
        Ok(StorageImportSessionSpace::new(known.volume, available))
    }
}

#[cfg(all(test, unix))]
#[path = "../../../tests/storage/import_session_review/filesystem.rs"]
mod review_tests;
