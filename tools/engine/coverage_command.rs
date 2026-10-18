use std::collections::{BTreeMap, btree_map::Entry};
use std::ffi::{OsStr, OsString};
use std::fmt::{Display, Formatter};
use std::fs::File;
#[cfg(not(unix))]
use std::fs::OpenOptions;
use std::io::{ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use chess::capacity::executor::{
    CapacityExecutor, CapacityExecutorCancellation, CapacityExecutorError,
};
use chess::engine::package::host::{EnginePackageHost, EnginePackageHostError};
use chess::engine::package::manifest::{EnginePackageManifest, EnginePackageManifestError};
use chess::engine::package::selector::{EnginePackageSelector, EnginePackageSelectorError};
#[cfg(unix)]
use rustix::fs::{AtFlags, Mode, OFlags};
use same_file::Handle;
use thiserror::Error;

use crate::engine::coverage::{
    ToolsEngineCoverageArtifact, ToolsEngineCoverageContext, ToolsEngineCoverageEntry,
    ToolsEngineCoverageEnumArtifact, ToolsEngineCoverageEnumPrerequisite,
    ToolsEngineCoverageEnumVerdict, ToolsEngineCoverageError, ToolsEngineCoverageId,
    ToolsEngineCoverageInventory, ToolsEngineCoverageObservation, ToolsEngineCoverageOutcome,
    ToolsEngineCoverageProvider, ToolsEngineCoverageReport, ToolsEngineCoverageRunner,
};

#[derive(Debug, Error)]
pub(crate) enum ToolsEngineCoverageCommandError {
    #[error(
        "Usage: chess-tools engine-coverage <package-directory> <new-output-report> [evidence-report...]"
    )]
    Configuration,
    #[error("Coverage command was cancelled before publication")]
    Cancelled,
    #[error("Coverage input {path:?} exceeds its {maximum}-byte limit")]
    Bounds { path: PathBuf, maximum: u64 },
    #[error(
        "Coverage input is not a stable regular file inside the selected evidence location: {0:?}"
    )]
    UnsafePath(PathBuf),
    #[error("Coverage artifact bytes do not match their recorded SHA-256: {0:?}")]
    ArtifactHash(PathBuf),
    #[error("Coverage reports contain conflicting observations for {0}")]
    Conflict(ToolsEngineCoverageId),
    #[error("Coverage destination already exists: {0:?}")]
    ExistingDestination(PathBuf),
    #[error("Coverage file operation failed at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Coverage report was published at {path:?}, but its directory sync failed: {source}")]
    PublishedSync {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Coverage command failed ({operation}); cleanup also failed ({cleanup})")]
    Cleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
    #[error(
        "Coverage operation failed ({operation}); checked path revalidation also failed ({validation})"
    )]
    PathRevalidation {
        #[source]
        operation: Box<Self>,
        validation: Box<Self>,
    },
    #[error("Coverage report was published at {path:?}, but staging cleanup failed: {cleanup}")]
    PublishedCleanup {
        path: PathBuf,
        #[source]
        cleanup: Box<Self>,
    },
    #[error(transparent)]
    Coverage(#[from] ToolsEngineCoverageError),
    #[error(transparent)]
    Host(#[from] EnginePackageHostError),
    #[error(transparent)]
    Manifest(#[from] EnginePackageManifestError),
    #[error(transparent)]
    Package(#[from] EnginePackageSelectorError),
    #[error(transparent)]
    Capacity(#[from] CapacityExecutorError),
}

impl ToolsEngineCoverageCommandError {
    fn validated<T>(operation: Result<T, Self>, validation: Result<(), Self>) -> Result<T, Self> {
        match (operation, validation) {
            (Err(operation), Err(validation)) => Err(Self::PathRevalidation {
                operation: Box::new(operation),
                validation: Box::new(validation),
            }),
            (Ok(_), Err(validation)) => Err(validation),
            (Err(operation), Ok(())) => Err(operation),
            (Ok(value), Ok(())) => Ok(value),
        }
    }

    fn finalized<T>(operation: Result<T, Self>, cleanup: Result<(), Self>) -> Result<T, Self> {
        match (operation, cleanup) {
            (Err(operation), Err(cleanup)) => Err(Self::Cleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            }),
            (Ok(_), Err(cleanup)) => Err(cleanup),
            (Err(operation), Ok(())) => Err(operation),
            (Ok(value), Ok(())) => Ok(value),
        }
    }
    fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_owned(),
            source,
        }
    }
}

pub(crate) struct ToolsEngineCoverageCommandConfig {
    package_directory: PathBuf,
    output_report: PathBuf,
    evidence_reports: Vec<PathBuf>,
}

impl ToolsEngineCoverageCommandConfig {
    const MAXIMUM_REPORTS: usize = 32;

    pub(crate) fn new(
        package_directory: PathBuf,
        output_report: PathBuf,
        evidence_reports: Vec<PathBuf>,
    ) -> Result<Self, ToolsEngineCoverageCommandError> {
        if package_directory.as_os_str().is_empty()
            || output_report.file_name().is_none()
            || evidence_reports.len() > Self::MAXIMUM_REPORTS
            || evidence_reports
                .iter()
                .any(|path| path.as_os_str().is_empty())
        {
            return Err(ToolsEngineCoverageCommandError::Configuration);
        }
        Ok(Self {
            package_directory,
            output_report,
            evidence_reports,
        })
    }
}

pub(crate) struct ToolsEngineCoverageCommandResources<'a> {
    executor: &'a CapacityExecutor,
    cancellation: Arc<AtomicBool>,
    runtime: tokio::runtime::Handle,
}

impl<'a> ToolsEngineCoverageCommandResources<'a> {
    pub(crate) fn new(
        executor: &'a CapacityExecutor,
        cancellation: Arc<AtomicBool>,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self {
            executor,
            cancellation,
            runtime,
        }
    }
}

struct ToolsEngineCoverageCommandIo {
    cancellation: Arc<AtomicBool>,
    remaining_input_bytes: u64,
}

struct ToolsEngineCoverageCommandIoDirectoryIdentity {
    path: PathBuf,
    handle: Handle,
    #[cfg(unix)]
    file: File,
}

impl ToolsEngineCoverageCommandIoDirectoryIdentity {
    #[cfg(unix)]
    fn is_dot_entry(name: &[u8]) -> bool {
        name == b"." || name == b".."
    }
    fn create_child(&self, name: &OsStr) -> Result<Self, ToolsEngineCoverageCommandError> {
        let path = self.path.join(name);
        #[cfg(unix)]
        {
            rustix::fs::mkdirat(&self.file, name, Mode::RWXU)
                .map_err(|error| ToolsEngineCoverageCommandError::io(&path, error.into()))?;
            let operation = Self::open_directory(&path, Some(self))
                .and_then(|file| Self::from_file(path.clone(), file));
            match operation {
                Ok(directory) => Ok(directory),
                Err(operation) => {
                    let cleanup = rustix::fs::unlinkat(&self.file, name, AtFlags::REMOVEDIR)
                        .map_err(|error| ToolsEngineCoverageCommandError::io(&path, error.into()));
                    ToolsEngineCoverageCommandError::finalized(Err(operation), cleanup)
                }
            }
        }
        #[cfg(not(unix))]
        {
            std::fs::create_dir(&path)
                .map_err(|error| ToolsEngineCoverageCommandError::io(&path, error))?;
            match Self::capture(path.clone(), Some(self)) {
                Ok(directory) => Ok(directory),
                Err(operation) => {
                    let cleanup = self.verify(None).and_then(|()| {
                        std::fs::remove_dir(&path)
                            .map_err(|error| ToolsEngineCoverageCommandError::io(&path, error))
                    });
                    ToolsEngineCoverageCommandError::finalized(Err(operation), cleanup)
                }
            }
        }
    }

    fn sync(&self) -> Result<(), ToolsEngineCoverageCommandError> {
        #[cfg(unix)]
        self.file
            .sync_all()
            .map_err(|error| ToolsEngineCoverageCommandError::io(&self.path, error))?;
        #[cfg(not(unix))]
        self.verify(None)?;
        Ok(())
    }

    fn names(&self, maximum: usize) -> Result<Vec<OsString>, ToolsEngineCoverageCommandError> {
        let mut names = Vec::new();
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let mut directory = rustix::fs::Dir::read_from(&self.file)
                .map_err(|error| ToolsEngineCoverageCommandError::io(&self.path, error.into()))?;
            while let Some(entry) = directory.read() {
                let entry = entry.map_err(|error| {
                    ToolsEngineCoverageCommandError::io(&self.path, error.into())
                })?;
                let name = entry.file_name().to_bytes();
                if Self::is_dot_entry(name) {
                    continue;
                }
                if names.len() >= maximum {
                    return Err(ToolsEngineCoverageCommandError::UnsafePath(
                        self.path.clone(),
                    ));
                }
                names.push(OsStr::from_bytes(name).to_owned());
            }
        }
        #[cfg(not(unix))]
        for entry in std::fs::read_dir(&self.path)
            .map_err(|error| ToolsEngineCoverageCommandError::io(&self.path, error))?
        {
            let entry =
                entry.map_err(|error| ToolsEngineCoverageCommandError::io(&self.path, error))?;
            if names.len() >= maximum {
                return Err(ToolsEngineCoverageCommandError::UnsafePath(
                    self.path.clone(),
                ));
            }
            names.push(entry.file_name());
        }
        names.sort();
        Ok(names)
    }

    #[cfg(unix)]
    fn remove_private_contents(
        file: &File,
        display: &Path,
    ) -> Result<(), ToolsEngineCoverageCommandError> {
        use std::os::unix::ffi::OsStrExt;
        let mut directory = rustix::fs::Dir::read_from(file)
            .map_err(|error| ToolsEngineCoverageCommandError::io(display, error.into()))?;
        while let Some(entry) = directory.read() {
            let entry = entry
                .map_err(|error| ToolsEngineCoverageCommandError::io(display, error.into()))?;
            let name = entry.file_name();
            if Self::is_dot_entry(name.to_bytes()) {
                continue;
            }
            let path = display.join(OsStr::from_bytes(name.to_bytes()));
            let flags = match rustix::fs::openat(
                file,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            ) {
                Ok(child) => {
                    Self::remove_private_contents(&File::from(child), &path)?;
                    AtFlags::REMOVEDIR
                }
                Err(rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP) => AtFlags::empty(),
                Err(error) => return Err(ToolsEngineCoverageCommandError::io(&path, error.into())),
            };
            rustix::fs::unlinkat(file, name, flags)
                .map_err(|error| ToolsEngineCoverageCommandError::io(&path, error.into()))?;
        }
        Ok(())
    }

    fn capture(
        path: PathBuf,
        parent: Option<&Self>,
    ) -> Result<Self, ToolsEngineCoverageCommandError> {
        let metadata = path
            .symlink_metadata()
            .map_err(|error| ToolsEngineCoverageCommandError::io(&path, error))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(path));
        }
        #[cfg(unix)]
        {
            let file = Self::open_directory(&path, parent)?;
            Self::from_file(path, file)
        }
        #[cfg(not(unix))]
        {
            if let Some(parent) = parent {
                parent.verify(None)?;
            }
            let handle = Handle::from_path(&path)
                .map_err(|error| ToolsEngineCoverageCommandError::io(&path, error))?;
            Ok(Self { path, handle })
        }
    }

    #[cfg(unix)]
    fn from_file(path: PathBuf, file: File) -> Result<Self, ToolsEngineCoverageCommandError> {
        let handle = Handle::from_file(
            file.try_clone()
                .map_err(|error| ToolsEngineCoverageCommandError::io(&path, error))?,
        )
        .map_err(|error| ToolsEngineCoverageCommandError::io(&path, error))?;
        Ok(Self { path, handle, file })
    }

    fn duplicate(&self) -> Result<Self, ToolsEngineCoverageCommandError> {
        #[cfg(unix)]
        {
            Self::from_file(
                self.path.clone(),
                self.file
                    .try_clone()
                    .map_err(|error| ToolsEngineCoverageCommandError::io(&self.path, error))?,
            )
        }
        #[cfg(not(unix))]
        {
            let duplicate = Self::capture(self.path.clone(), None)?;
            if self.handle != duplicate.handle {
                return Err(ToolsEngineCoverageCommandError::UnsafePath(
                    self.path.clone(),
                ));
            }
            Ok(duplicate)
        }
    }

    #[cfg(unix)]
    fn open_directory(
        path: &Path,
        parent: Option<&Self>,
    ) -> Result<File, ToolsEngineCoverageCommandError> {
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        if let Some(parent) = parent {
            let name = path
                .file_name()
                .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
            return rustix::fs::openat(&parent.file, name, flags, Mode::empty())
                .map(File::from)
                .map_err(|error| ToolsEngineCoverageCommandError::io(path, error.into()));
        }
        if !path.is_absolute() {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(path.to_owned()));
        }
        let anchor = path
            .ancestors()
            .last()
            .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
        let mut directory = rustix::fs::open(anchor, flags, Mode::empty())
            .map(File::from)
            .map_err(|error| ToolsEngineCoverageCommandError::io(anchor, error.into()))?;
        for component in path.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(name) => {
                    directory = rustix::fs::openat(&directory, name, flags, Mode::empty())
                        .map(File::from)
                        .map_err(|error| ToolsEngineCoverageCommandError::io(path, error.into()))?;
                }
                Component::Prefix(_) | Component::CurDir | Component::ParentDir => {
                    return Err(ToolsEngineCoverageCommandError::UnsafePath(path.to_owned()));
                }
            }
        }
        Ok(directory)
    }

    fn verify(&self, parent: Option<&Self>) -> Result<(), ToolsEngineCoverageCommandError> {
        let current = Self::capture(self.path.clone(), parent)?;
        if self.handle != current.handle {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(
                self.path.clone(),
            ));
        }
        Ok(())
    }
}

struct ToolsEngineCoverageCommandIoPath {
    path: PathBuf,
    directories: Vec<ToolsEngineCoverageCommandIoDirectoryIdentity>,
}

impl ToolsEngineCoverageCommandIoPath {
    fn capture(root: &Path, relative: &Path) -> Result<Self, ToolsEngineCoverageCommandError> {
        Self::capture_relative(
            vec![ToolsEngineCoverageCommandIoDirectoryIdentity::capture(
                root.to_owned(),
                None,
            )?],
            relative,
        )
    }

    fn capture_relative(
        mut directories: Vec<ToolsEngineCoverageCommandIoDirectoryIdentity>,
        relative: &Path,
    ) -> Result<Self, ToolsEngineCoverageCommandError> {
        let components: Vec<_> = relative.components().collect();
        if components.is_empty() || relative.as_os_str().to_string_lossy().contains('\\') {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(
                relative.to_owned(),
            ));
        }
        let mut path = directories
            .last()
            .ok_or(ToolsEngineCoverageCommandError::Configuration)?
            .path
            .clone();
        for (index, component) in components.iter().enumerate() {
            let Component::Normal(name) = component else {
                return Err(ToolsEngineCoverageCommandError::UnsafePath(
                    relative.to_owned(),
                ));
            };
            path.push(name);
            if index + 1 < components.len() {
                directories.push(ToolsEngineCoverageCommandIoDirectoryIdentity::capture(
                    path.clone(),
                    directories.last(),
                )?);
            }
        }
        let checked = Self { path, directories };
        checked.verify()?;
        Ok(checked)
    }

    fn sibling(&self, relative: &Path) -> Result<Self, ToolsEngineCoverageCommandError> {
        self.verify()?;
        let directories = self
            .directories
            .iter()
            .map(ToolsEngineCoverageCommandIoDirectoryIdentity::duplicate)
            .collect::<Result<Vec<_>, _>>()?;
        Self::capture_relative(directories, relative)
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn verify(&self) -> Result<(), ToolsEngineCoverageCommandError> {
        let mut parent = None;
        for directory in &self.directories {
            directory.verify(parent)?;
            parent = Some(directory);
        }
        let metadata = self
            .path
            .symlink_metadata()
            .map_err(|error| ToolsEngineCoverageCommandError::io(&self.path, error))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(
                self.path.clone(),
            ));
        }
        Ok(())
    }

    fn open(&self) -> Result<File, ToolsEngineCoverageCommandError> {
        self.verify()?;
        #[cfg(unix)]
        let file = {
            let directory = self
                .directories
                .last()
                .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
            let name = self
                .path
                .file_name()
                .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
            rustix::fs::openat(
                &directory.file,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map(File::from)
            .map_err(|error| ToolsEngineCoverageCommandError::io(&self.path, error.into()))?
        };
        #[cfg(not(unix))]
        let file = File::open(&self.path)
            .map_err(|error| ToolsEngineCoverageCommandError::io(&self.path, error))?;
        if !file
            .metadata()
            .map_err(|error| ToolsEngineCoverageCommandError::io(&self.path, error))?
            .is_file()
        {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(
                self.path.clone(),
            ));
        }
        Ok(file)
    }

    fn finish<T>(
        &self,
        operation: Result<T, ToolsEngineCoverageCommandError>,
    ) -> Result<T, ToolsEngineCoverageCommandError> {
        ToolsEngineCoverageCommandError::validated(operation, self.verify())
    }
}

impl ToolsEngineCoverageCommandIo {
    const MAXIMUM_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
    const MAXIMUM_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
    const MAXIMUM_INPUT_BYTES: u64 = 512 * 1024 * 1024;
    const BUFFER_BYTES: usize = 64 * 1024;

    fn new(cancellation: Arc<AtomicBool>) -> Self {
        Self {
            cancellation,
            remaining_input_bytes: Self::MAXIMUM_INPUT_BYTES,
        }
    }

    fn check(
        &self,
        worker: &CapacityExecutorCancellation,
    ) -> Result<(), ToolsEngineCoverageCommandError> {
        if self.cancellation.load(Ordering::SeqCst) || worker.is_cancelled() {
            return Err(ToolsEngineCoverageCommandError::Cancelled);
        }
        Ok(())
    }

    fn consume(&mut self, path: &Path, bytes: u64) -> Result<(), ToolsEngineCoverageCommandError> {
        self.remaining_input_bytes =
            self.remaining_input_bytes
                .checked_sub(bytes)
                .ok_or_else(|| ToolsEngineCoverageCommandError::Bounds {
                    path: path.to_owned(),
                    maximum: Self::MAXIMUM_INPUT_BYTES,
                })?;
        Ok(())
    }

    fn read(
        &mut self,
        checked: &ToolsEngineCoverageCommandIoPath,
        maximum: u64,
        worker: &CapacityExecutorCancellation,
    ) -> Result<Vec<u8>, ToolsEngineCoverageCommandError> {
        let mut bytes = Vec::new();
        self.copy(checked, maximum, &mut bytes, worker)?;
        Ok(bytes)
    }

    fn copy(
        &mut self,
        checked: &ToolsEngineCoverageCommandIoPath,
        maximum: u64,
        output: &mut impl Write,
        worker: &CapacityExecutorCancellation,
    ) -> Result<u64, ToolsEngineCoverageCommandError> {
        checked.finish(self.copy_file(checked, maximum, output, worker))
    }

    fn copy_file(
        &mut self,
        checked: &ToolsEngineCoverageCommandIoPath,
        maximum: u64,
        output: &mut impl Write,
        worker: &CapacityExecutorCancellation,
    ) -> Result<u64, ToolsEngineCoverageCommandError> {
        self.check(worker)?;
        let path = checked.path();
        let mut file = checked.open()?;
        let identity = Handle::from_file(
            file.try_clone()
                .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?,
        )
        .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?;
        let before = file
            .metadata()
            .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?;
        let length = before.len();
        if length == 0 || length > maximum || length > self.remaining_input_bytes {
            return Err(ToolsEngineCoverageCommandError::Bounds {
                path: path.to_owned(),
                maximum,
            });
        }
        let mut total = 0_u64;
        let mut buffer = vec![0; Self::BUFFER_BYTES];
        loop {
            self.check(worker)?;
            let count = file
                .read(&mut buffer)
                .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?;
            if count == 0 {
                break;
            }
            let count_u64 =
                u64::try_from(count).map_err(|_error| ToolsEngineCoverageCommandError::Bounds {
                    path: path.to_owned(),
                    maximum,
                })?;
            self.consume(path, count_u64)?;
            total = total.checked_add(count_u64).ok_or_else(|| {
                ToolsEngineCoverageCommandError::Bounds {
                    path: path.to_owned(),
                    maximum,
                }
            })?;
            if total > length || total > maximum {
                return Err(ToolsEngineCoverageCommandError::Bounds {
                    path: path.to_owned(),
                    maximum,
                });
            }
            output
                .write_all(&buffer[..count])
                .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?;
        }
        let after = file
            .metadata()
            .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?;
        if total != length
            || after.len() != length
            || before
                .modified()
                .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?
                != after
                    .modified()
                    .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?
            || identity
                != Handle::from_file(checked.open()?)
                    .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?
        {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(path.to_owned()));
        }
        self.check(worker)?;
        Ok(total)
    }

    fn regular_below(
        root: &Path,
        relative: &Path,
    ) -> Result<ToolsEngineCoverageCommandIoPath, ToolsEngineCoverageCommandError> {
        ToolsEngineCoverageCommandIoPath::capture(root, relative)
    }
}

struct ToolsEngineCoverageCommandDestination {
    parent: PathBuf,
    parent_identity: ToolsEngineCoverageCommandIoDirectoryIdentity,
    report: PathBuf,
    bundle_name: String,
}

impl ToolsEngineCoverageCommandDestination {
    fn capture(path: &Path) -> Result<Self, ToolsEngineCoverageCommandError> {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.contains('\\') && !name.chars().any(char::is_control))
            .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = parent
            .canonicalize()
            .map_err(|error| ToolsEngineCoverageCommandError::io(parent, error))?;
        let report = parent.join(name);
        Self::require_absent(&report)?;
        let parent_identity =
            ToolsEngineCoverageCommandIoDirectoryIdentity::capture(parent.clone(), None)?;
        Ok(Self {
            parent,
            parent_identity,
            report,
            bundle_name: format!("{name}.artifacts"),
        })
    }

    fn require_absent(path: &Path) -> Result<(), ToolsEngineCoverageCommandError> {
        match path.symlink_metadata() {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(ToolsEngineCoverageCommandError::io(path, error)),
            Ok(_metadata) => Err(ToolsEngineCoverageCommandError::ExistingDestination(
                path.to_owned(),
            )),
        }
    }

    fn validate(&self) -> Result<(), ToolsEngineCoverageCommandError> {
        self.parent_identity.verify(None)?;
        Self::require_absent(&self.report)
    }

    fn bundle_path(&self) -> PathBuf {
        self.parent.join(&self.bundle_name)
    }

    fn artifact_path(&self, artifact: &ToolsEngineCoverageArtifact) -> String {
        let kind = match artifact.kind() {
            ToolsEngineCoverageEnumArtifact::Fixture => "fixture",
            ToolsEngineCoverageEnumArtifact::EngineTranscript => "engine-transcript",
            ToolsEngineCoverageEnumArtifact::WorkbenchExport => "workbench-export",
            ToolsEngineCoverageEnumArtifact::NativeMeasurement => "native-measurement",
            ToolsEngineCoverageEnumArtifact::Source => "source",
            ToolsEngineCoverageEnumArtifact::ControlledBuildLog => "controlled-build-log",
        };
        format!("{}/{kind}/{}", self.bundle_name, artifact.sha256())
    }
}

struct ToolsEngineCoverageCommandStage {
    parent: ToolsEngineCoverageCommandIoDirectoryIdentity,
    identity: ToolsEngineCoverageCommandIoDirectoryIdentity,
    closed: bool,
}

impl ToolsEngineCoverageCommandStage {
    fn new(
        parent: &ToolsEngineCoverageCommandIoDirectoryIdentity,
    ) -> Result<Self, ToolsEngineCoverageCommandError> {
        parent.verify(None)?;
        let parent = parent.duplicate()?;
        let identity = parent.create_child(OsStr::new(&format!(
            ".coverage-stage-{}",
            uuid::Uuid::new_v4()
        )))?;
        let mut stage = Self {
            parent,
            identity,
            closed: false,
        };
        match stage.verify() {
            Ok(()) => Ok(stage),
            Err(operation) => stage.finalize(Err(operation)),
        }
    }

    fn path(&self) -> &Path {
        &self.identity.path
    }

    fn verify(&self) -> Result<(), ToolsEngineCoverageCommandError> {
        if self.closed {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(
                self.identity.path.clone(),
            ));
        }
        self.parent.verify(None)?;
        self.identity.verify(Some(&self.parent))
    }

    fn open_new(&self, relative: &Path) -> Result<File, ToolsEngineCoverageCommandError> {
        let components: Vec<_> = relative.components().collect();
        let (last, parents) = components
            .split_last()
            .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
        let Component::Normal(name) = last else {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(
                relative.to_owned(),
            ));
        };
        let mut directory = self.identity.duplicate()?;
        for component in parents {
            let Component::Normal(child) = component else {
                return Err(ToolsEngineCoverageCommandError::UnsafePath(
                    relative.to_owned(),
                ));
            };
            let path = directory.path.join(child);
            #[cfg(unix)]
            {
                match rustix::fs::mkdirat(&directory.file, *child, Mode::RWXU) {
                    Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                    Err(error) => {
                        return Err(ToolsEngineCoverageCommandError::io(&path, error.into()));
                    }
                }
                let file = ToolsEngineCoverageCommandIoDirectoryIdentity::open_directory(
                    &path,
                    Some(&directory),
                )?;
                directory = ToolsEngineCoverageCommandIoDirectoryIdentity::from_file(path, file)?;
            }
            #[cfg(not(unix))]
            {
                match std::fs::create_dir(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(ToolsEngineCoverageCommandError::io(&path, error)),
                }
                directory =
                    ToolsEngineCoverageCommandIoDirectoryIdentity::capture(path, Some(&directory))?;
            }
        }
        let path = directory.path.join(name);
        #[cfg(unix)]
        {
            rustix::fs::openat(
                &directory.file,
                *name,
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map(File::from)
            .map_err(|error| ToolsEngineCoverageCommandError::io(&path, error.into()))
        }
        #[cfg(not(unix))]
        {
            OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&path)
                .map_err(|error| ToolsEngineCoverageCommandError::io(&path, error))
        }
    }

    fn with_file<T>(
        &self,
        relative: &Path,
        operation: impl FnOnce(&mut File) -> Result<T, ToolsEngineCoverageCommandError>,
    ) -> Result<T, ToolsEngineCoverageCommandError> {
        self.verify()?;
        let result = (|| {
            let mut file = self.open_new(relative)?;
            let value = operation(&mut file)?;
            file.sync_all().map_err(|error| {
                ToolsEngineCoverageCommandError::io(&self.identity.path.join(relative), error)
            })?;
            Ok(value)
        })();
        ToolsEngineCoverageCommandError::validated(result, self.verify())
    }

    fn cleanup(&mut self) -> Result<(), ToolsEngineCoverageCommandError> {
        if self.closed {
            return Ok(());
        }
        #[cfg(unix)]
        {
            ToolsEngineCoverageCommandIoDirectoryIdentity::remove_private_contents(
                &self.identity.file,
                &self.identity.path,
            )?;
            let current = ToolsEngineCoverageCommandIoDirectoryIdentity::open_directory(
                &self.identity.path,
                Some(&self.parent),
            )?;
            let current = Handle::from_file(current)
                .map_err(|error| ToolsEngineCoverageCommandError::io(&self.identity.path, error))?;
            if current != self.identity.handle {
                return Err(ToolsEngineCoverageCommandError::UnsafePath(
                    self.identity.path.clone(),
                ));
            }
            let name = self
                .identity
                .path
                .file_name()
                .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
            rustix::fs::unlinkat(&self.parent.file, name, AtFlags::REMOVEDIR).map_err(|error| {
                ToolsEngineCoverageCommandError::io(&self.identity.path, error.into())
            })?;
            self.closed = true;
            self.parent.sync()
        }
        #[cfg(not(unix))]
        {
            self.verify()?;
            std::fs::remove_dir_all(&self.identity.path)
                .map_err(|error| ToolsEngineCoverageCommandError::io(&self.identity.path, error))?;
            self.closed = true;
            self.parent.sync()
        }
    }

    fn finalize<T>(
        &mut self,
        operation: Result<T, ToolsEngineCoverageCommandError>,
    ) -> Result<T, ToolsEngineCoverageCommandError> {
        ToolsEngineCoverageCommandError::finalized(operation, self.cleanup())
    }
}

impl Drop for ToolsEngineCoverageCommandStage {
    fn drop(&mut self) {
        if !self.closed
            && let Err(error) = self.cleanup()
        {
            bevy::log::error!("{error}");
        }
    }
}

struct ToolsEngineCoverageCommandArtifacts {
    directory: ToolsEngineCoverageCommandStage,
    identity: ToolsEngineCoverageCommandIoDirectoryIdentity,
    staged: BTreeMap<String, PathBuf>,
}

struct ToolsEngineCoverageCommandArtifactsImport<'a> {
    artifact: &'a ToolsEngineCoverageArtifact,
    report: &'a ToolsEngineCoverageCommandIoPath,
    destination: &'a ToolsEngineCoverageCommandDestination,
}

impl ToolsEngineCoverageCommandArtifacts {
    fn new(
        parent: &ToolsEngineCoverageCommandIoDirectoryIdentity,
    ) -> Result<Self, ToolsEngineCoverageCommandError> {
        let mut directory = ToolsEngineCoverageCommandStage::new(parent)?;
        let identity = match directory.identity.duplicate() {
            Ok(identity) => identity,
            Err(operation) => return directory.finalize(Err(operation)),
        };
        Ok(Self {
            directory,
            identity,
            staged: BTreeMap::new(),
        })
    }

    fn import(
        &mut self,
        request: ToolsEngineCoverageCommandArtifactsImport<'_>,
        io: &mut ToolsEngineCoverageCommandIo,
        worker: &CapacityExecutorCancellation,
    ) -> Result<ToolsEngineCoverageArtifact, ToolsEngineCoverageCommandError> {
        self.directory.verify()?;
        let path = request.report.sibling(Path::new(request.artifact.path()))?;
        let bytes = io.read(
            &path,
            ToolsEngineCoverageCommandIo::MAXIMUM_ARTIFACT_BYTES,
            worker,
        )?;
        let relative = request.destination.artifact_path(request.artifact);
        let captured = ToolsEngineCoverageArtifact::capture(
            request.artifact.kind(),
            relative.clone(),
            &bytes,
        )?;
        if captured.sha256() != request.artifact.sha256() {
            return Err(ToolsEngineCoverageCommandError::ArtifactHash(
                path.path().to_owned(),
            ));
        }
        if let Entry::Vacant(slot) = self.staged.entry(relative.clone()) {
            let staged = self.directory.path().join(&relative);
            self.directory.with_file(Path::new(&relative), |file| {
                file.write_all(&bytes)
                    .map_err(|error| ToolsEngineCoverageCommandError::io(&staged, error))
            })?;
            slot.insert(staged);
        }
        Ok(captured)
    }
}

struct ToolsEngineCoverageCommandProvider {
    imported: BTreeMap<ToolsEngineCoverageId, ToolsEngineCoverageObservation>,
    cancellation: Arc<AtomicBool>,
}

impl ToolsEngineCoverageCommandProvider {
    fn insert(
        &mut self,
        observation: ToolsEngineCoverageObservation,
    ) -> Result<(), ToolsEngineCoverageCommandError> {
        let id = observation.id();
        match self.imported.entry(id) {
            Entry::Occupied(slot) => {
                let previous =
                    serde_json::to_vec(slot.get()).map_err(ToolsEngineCoverageError::from)?;
                let current =
                    serde_json::to_vec(&observation).map_err(ToolsEngineCoverageError::from)?;
                if previous != current {
                    return Err(ToolsEngineCoverageCommandError::Conflict(id));
                }
            }
            Entry::Vacant(slot) => {
                slot.insert(observation);
            }
        }
        Ok(())
    }

    fn outstanding(
        entry: &ToolsEngineCoverageEntry,
        context: &ToolsEngineCoverageContext,
    ) -> ToolsEngineCoverageOutcome {
        let prerequisite = entry.prerequisite(context.target());
        let workflow = match prerequisite {
            ToolsEngineCoverageEnumPrerequisite::ApplicationWorkflow => {
                "Execute the owning application, engine-session or isolated workbench workflow against the selected package"
            }
            ToolsEngineCoverageEnumPrerequisite::NativeTarget => {
                "Build and execute the required compatible package on authentic target hardware"
            }
            ToolsEngineCoverageEnumPrerequisite::CpuProfile => {
                "Execute the required CPU profile on compatible native hardware and retain its baseline comparison"
            }
            ToolsEngineCoverageEnumPrerequisite::ParallelScaling => {
                "Measure repeated optimized one-worker and detected-maximum-worker runs under the shared capacity budget"
            }
            ToolsEngineCoverageEnumPrerequisite::AuthenticTablebases => {
                "Supply authentic local WDL and DTZ tables and execute load, cardinality, probe and rule-50 checks"
            }
            ToolsEngineCoverageEnumPrerequisite::NumaHost => {
                "Execute the Linux NUMA build on a real NUMA host with matching libnuma and topology diagnostics"
            }
            ToolsEngineCoverageEnumPrerequisite::PgoExecution => {
                "Execute controlled instrument, train, merge and optimized rebuild steps with behavior comparison"
            }
            ToolsEngineCoverageEnumPrerequisite::SpsaCompileCheck => {
                "Run the controlled pinned SPSA compile check and retain source plus actual compiler output before classifying upstream unavailability"
            }
        };
        ToolsEngineCoverageOutcome::Outstanding {
            prerequisite,
            detail: format!(
                "No verified imported observation establishes {}: {}",
                entry.facility(),
                entry.requirement()
            ),
            next_action: format!(
                "{workflow}. Verify {} and retain exact fixture and output artifacts.",
                entry.requirement()
            ),
        }
    }
}

impl ToolsEngineCoverageProvider for ToolsEngineCoverageCommandProvider {
    async fn classify(
        &mut self,
        entry: &ToolsEngineCoverageEntry,
        context: &ToolsEngineCoverageContext,
    ) -> Result<ToolsEngineCoverageObservation, ToolsEngineCoverageError> {
        if self.cancellation.load(Ordering::SeqCst) {
            return Err(ToolsEngineCoverageError::Workflow(Box::new(
                ToolsEngineCoverageCommandError::Cancelled,
            )));
        }
        match self.imported.remove(&entry.id()) {
            Some(observation) => Ok(observation),
            None => ToolsEngineCoverageObservation::new(
                entry,
                context,
                Self::outstanding(entry, context),
            ),
        }
    }
}

struct ToolsEngineCoverageCommandPrepared {
    inventory: ToolsEngineCoverageInventory,
    context: ToolsEngineCoverageContext,
    provider: ToolsEngineCoverageCommandProvider,
    artifacts: ToolsEngineCoverageCommandArtifacts,
    destination: ToolsEngineCoverageCommandDestination,
}

impl ToolsEngineCoverageCommandPrepared {
    fn prepare(
        config: ToolsEngineCoverageCommandConfig,
        cancellation: Arc<AtomicBool>,
        host: EnginePackageHost,
        worker: &CapacityExecutorCancellation,
    ) -> Result<Self, ToolsEngineCoverageCommandError> {
        let mut io = ToolsEngineCoverageCommandIo::new(Arc::clone(&cancellation));
        io.check(worker)?;
        let destination = ToolsEngineCoverageCommandDestination::capture(&config.output_report)?;
        let inventory = ToolsEngineCoverageInventory::pinned()?;
        let context = Self::package_context(&config.package_directory, &mut io, host, worker)?;
        let artifacts = ToolsEngineCoverageCommandArtifacts::new(&destination.parent_identity)?;
        let provider = ToolsEngineCoverageCommandProvider {
            imported: BTreeMap::new(),
            cancellation,
        };
        let mut prepared = Self {
            inventory,
            context,
            provider,
            artifacts,
            destination,
        };
        let operation = (|| {
            for path in config.evidence_reports {
                prepared.import_report(&path, &mut io, worker)?;
            }
            io.check(worker)
        })();
        if let Err(operation) = operation {
            return prepared.fail(operation);
        }
        Ok(prepared)
    }

    fn fail<T>(
        &mut self,
        operation: ToolsEngineCoverageCommandError,
    ) -> Result<T, ToolsEngineCoverageCommandError> {
        self.artifacts.directory.finalize(Err(operation))
    }

    fn complete(
        mut self,
        runtime: &tokio::runtime::Handle,
        cancellation: Arc<AtomicBool>,
        worker: &CapacityExecutorCancellation,
    ) -> Result<ToolsEngineCoverageCommandSummary, ToolsEngineCoverageCommandError> {
        let operation = (|| {
            let report = runtime.block_on(ToolsEngineCoverageRunner::run(
                &self.inventory,
                &self.context,
                &mut self.provider,
            ))?;
            let bytes = report.encode()?;
            let summary = ToolsEngineCoverageCommandSummary::from_report(
                &report,
                self.destination.report.clone(),
            );
            Ok((bytes, summary))
        })();
        let (bytes, summary) = match operation {
            Ok(planned) => planned,
            Err(operation) => return self.fail(operation),
        };
        self.publish(&bytes, cancellation, worker)?;
        Ok(summary)
    }

    fn package_context(
        directory: &Path,
        io: &mut ToolsEngineCoverageCommandIo,
        host: EnginePackageHost,
        worker: &CapacityExecutorCancellation,
    ) -> Result<ToolsEngineCoverageContext, ToolsEngineCoverageCommandError> {
        let root = directory
            .canonicalize()
            .map_err(|error| ToolsEngineCoverageCommandError::io(directory, error))?;
        let manifest_path =
            ToolsEngineCoverageCommandIo::regular_below(&root, Path::new("manifest.json"))?;
        let manifest_bytes = io.read(
            &manifest_path,
            u64::try_from(ToolsEngineCoverageReport::MAXIMUM_DOCUMENT_BYTES)
                .map_err(|_error| ToolsEngineCoverageCommandError::Configuration)?,
            worker,
        )?;
        let manifest = EnginePackageManifest::parse(&manifest_bytes)?;
        let entry = manifest
            .entries()
            .iter()
            .find(|entry| entry.target() == host.target())
            .ok_or(EnginePackageSelectorError::MissingPackage)?;
        let source = manifest_path.sibling(entry.executable())?;
        let temporary = std::env::temp_dir()
            .canonicalize()
            .map_err(|error| ToolsEngineCoverageCommandError::io(&root, error))?;
        let temporary = ToolsEngineCoverageCommandIoDirectoryIdentity::capture(temporary, None)?;
        let mut snapshot = ToolsEngineCoverageCommandStage::new(&temporary)?;
        let operation = (|| {
            let bytes = snapshot.with_file(entry.executable(), |file| {
                let bytes = io.copy(
                    &source,
                    ToolsEngineCoverageCommandIo::MAXIMUM_PACKAGE_BYTES,
                    file,
                    worker,
                )?;
                let permissions = source
                    .open()?
                    .metadata()
                    .map_err(|error| ToolsEngineCoverageCommandError::io(source.path(), error))?
                    .permissions();
                file.set_permissions(permissions)
                    .map_err(|error| ToolsEngineCoverageCommandError::io(snapshot.path(), error))?;
                Ok(bytes)
            })?;
            io.consume(source.path(), bytes)?;
            io.check(worker)?;
            snapshot.verify()?;
            let selected = EnginePackageSelector::select(snapshot.path(), &manifest, host)?;
            io.check(worker)?;
            snapshot.verify()?;
            Ok(ToolsEngineCoverageContext::new(
                selected.target(),
                selected.profile(),
                selected.digest(),
            )?)
        })();
        let operation = ToolsEngineCoverageCommandError::validated(operation, snapshot.verify());
        snapshot.finalize(operation)
    }

    fn import_report(
        &mut self,
        path: &Path,
        io: &mut ToolsEngineCoverageCommandIo,
        worker: &CapacityExecutorCancellation,
    ) -> Result<(), ToolsEngineCoverageCommandError> {
        let report_path = path
            .canonicalize()
            .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?;
        if path
            .symlink_metadata()
            .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?
            .file_type()
            .is_symlink()
        {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(path.to_owned()));
        }
        let root = report_path
            .parent()
            .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
        let name = report_path
            .file_name()
            .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
        let checked = ToolsEngineCoverageCommandIo::regular_below(root, Path::new(name))?;
        let operation = self.import_checked_report(&checked, io, worker);
        checked.finish(operation)
    }

    fn import_checked_report(
        &mut self,
        checked: &ToolsEngineCoverageCommandIoPath,
        io: &mut ToolsEngineCoverageCommandIo,
        worker: &CapacityExecutorCancellation,
    ) -> Result<(), ToolsEngineCoverageCommandError> {
        let bytes = io.read(
            checked,
            u64::try_from(ToolsEngineCoverageReport::MAXIMUM_DOCUMENT_BYTES)
                .map_err(|_error| ToolsEngineCoverageCommandError::Configuration)?,
            worker,
        )?;
        let report = ToolsEngineCoverageReport::parse(&bytes, &self.inventory, &self.context)?;
        for observation in report.observations() {
            let mut outcome = observation.outcome().clone();
            match &mut outcome {
                ToolsEngineCoverageOutcome::Measured { artifacts, .. }
                | ToolsEngineCoverageOutcome::UpstreamUnavailable { artifacts, .. } => {
                    let mut unique = BTreeMap::new();
                    for artifact in artifacts.iter() {
                        let captured = self.artifacts.import(
                            ToolsEngineCoverageCommandArtifactsImport {
                                artifact,
                                report: checked,
                                destination: &self.destination,
                            },
                            io,
                            worker,
                        )?;
                        unique.entry(captured.path().to_owned()).or_insert(captured);
                    }
                    *artifacts = unique.into_values().collect();
                }
                ToolsEngineCoverageOutcome::Outstanding { .. } => continue,
            }
            let entry = self
                .inventory
                .entries()
                .find(|entry| entry.id() == observation.id())
                .ok_or(ToolsEngineCoverageError::Foreign(observation.id()))?;
            self.provider.insert(ToolsEngineCoverageObservation::new(
                entry,
                &self.context,
                outcome,
            )?)?;
        }
        Ok(())
    }

    fn publish(
        self,
        bytes: &[u8],
        cancellation: Arc<AtomicBool>,
        worker: &CapacityExecutorCancellation,
    ) -> Result<(), ToolsEngineCoverageCommandError> {
        let mut publication = ToolsEngineCoverageCommandPublication::new(self);
        let io = ToolsEngineCoverageCommandIo::new(cancellation);
        let mut result = publication.commit(bytes, &io, worker);
        if result.is_err()
            && !publication.published
            && let Err(cleanup) = publication.cleanup()
        {
            result = match result {
                Err(operation) => Err(ToolsEngineCoverageCommandError::Cleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
                Ok(()) => Err(cleanup),
            };
        }
        if let Err(cleanup) = publication.cleanup_staging() {
            result = match result {
                Err(operation) => Err(ToolsEngineCoverageCommandError::Cleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
                Ok(()) => Err(ToolsEngineCoverageCommandError::PublishedCleanup {
                    path: publication.prepared.destination.report.clone(),
                    cleanup: Box::new(cleanup),
                }),
            };
        }
        if result.is_ok() {
            let validation = publication
                .validate_bundle()
                .and_then(|()| publication.verify_report());
            if let Err(operation) = validation {
                publication.published = false;
                result = match publication.cleanup() {
                    Ok(()) => Err(operation),
                    Err(cleanup) => Err(ToolsEngineCoverageCommandError::Cleanup {
                        operation: Box::new(operation),
                        cleanup: Box::new(cleanup),
                    }),
                };
            }
        }
        result
    }
}

struct ToolsEngineCoverageCommandPublicationFile {
    identity: Handle,
    bytes: u64,
    modified: std::time::SystemTime,
}

impl ToolsEngineCoverageCommandPublicationFile {
    fn capture(file: &File, path: &Path) -> Result<Self, ToolsEngineCoverageCommandError> {
        let metadata = file
            .metadata()
            .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?;
        if !metadata.is_file()
            || metadata.len() == 0
            || metadata.len() > ToolsEngineCoverageCommandIo::MAXIMUM_ARTIFACT_BYTES
        {
            return Err(ToolsEngineCoverageCommandError::Bounds {
                path: path.to_owned(),
                maximum: ToolsEngineCoverageCommandIo::MAXIMUM_ARTIFACT_BYTES,
            });
        }
        Ok(Self {
            identity: Handle::from_file(
                file.try_clone()
                    .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?,
            )
            .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?,
            bytes: metadata.len(),
            modified: metadata
                .modified()
                .map_err(|error| ToolsEngineCoverageCommandError::io(path, error))?,
        })
    }

    fn verify(&self, file: &File, path: &Path) -> Result<(), ToolsEngineCoverageCommandError> {
        let current = Self::capture(file, path)?;
        if self.identity != current.identity
            || self.bytes != current.bytes
            || self.modified != current.modified
        {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(path.to_owned()));
        }
        Ok(())
    }
}

struct ToolsEngineCoverageCommandPublicationParts {
    kind: OsString,
    name: OsString,
    digest: chess::package::digest::PackageDigest,
}

struct ToolsEngineCoverageCommandPublication {
    prepared: ToolsEngineCoverageCommandPrepared,
    bundle_identity: Option<ToolsEngineCoverageCommandIoDirectoryIdentity>,
    kinds: BTreeMap<OsString, ToolsEngineCoverageCommandIoDirectoryIdentity>,
    linked: BTreeMap<String, ToolsEngineCoverageCommandPublicationFile>,
    report_identity: Option<Handle>,
    report_digest: Option<chess::package::digest::PackageDigest>,
    published: bool,
}

impl ToolsEngineCoverageCommandPublication {
    const REPORT_STAGE_NAME: &str = "coverage-report.json";

    fn new(prepared: ToolsEngineCoverageCommandPrepared) -> Self {
        Self {
            prepared,
            bundle_identity: None,
            kinds: BTreeMap::new(),
            linked: BTreeMap::new(),
            report_identity: None,
            report_digest: None,
            published: false,
        }
    }

    fn parts(
        &self,
        relative: &str,
    ) -> Result<ToolsEngineCoverageCommandPublicationParts, ToolsEngineCoverageCommandError> {
        let mut components = Path::new(relative).components();
        let (
            Some(Component::Normal(bundle)),
            Some(Component::Normal(kind)),
            Some(Component::Normal(name)),
            None,
        ) = (
            components.next(),
            components.next(),
            components.next(),
            components.next(),
        )
        else {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(PathBuf::from(
                relative,
            )));
        };
        if bundle != OsStr::new(&self.prepared.destination.bundle_name) {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(PathBuf::from(
                relative,
            )));
        }
        let digest = name
            .to_str()
            .ok_or(ToolsEngineCoverageCommandError::Configuration)?
            .parse()
            .map_err(ToolsEngineCoverageError::from)?;
        Ok(ToolsEngineCoverageCommandPublicationParts {
            kind: kind.to_owned(),
            name: name.to_owned(),
            digest,
        })
    }

    fn bundle(
        &self,
    ) -> Result<&ToolsEngineCoverageCommandIoDirectoryIdentity, ToolsEngineCoverageCommandError>
    {
        self.bundle_identity
            .as_ref()
            .ok_or(ToolsEngineCoverageCommandError::Configuration)
    }

    fn create_bundle(&mut self) -> Result<(), ToolsEngineCoverageCommandError> {
        self.prepared.destination.validate()?;
        let bundle = self
            .prepared
            .destination
            .parent_identity
            .create_child(OsStr::new(&self.prepared.destination.bundle_name))?;
        self.bundle_identity = Some(bundle);
        self.validate_bundle()
    }

    fn ensure_kind(&mut self, name: &OsStr) -> Result<(), ToolsEngineCoverageCommandError> {
        if let Entry::Vacant(slot) = self.kinds.entry(name.to_owned()) {
            let bundle = self
                .bundle_identity
                .as_ref()
                .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
            slot.insert(bundle.create_child(name)?);
        }
        Ok(())
    }

    fn linked_path(
        &self,
        relative: &str,
    ) -> Result<ToolsEngineCoverageCommandIoPath, ToolsEngineCoverageCommandError> {
        let parts = self.parts(relative)?;
        let kind = self
            .kinds
            .get(parts.kind.as_os_str())
            .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
        ToolsEngineCoverageCommandIoPath::capture_relative(
            vec![
                self.prepared.destination.parent_identity.duplicate()?,
                self.bundle()?.duplicate()?,
                kind.duplicate()?,
            ],
            Path::new(&parts.name),
        )
    }

    fn link_artifact(&mut self, relative: &str) -> Result<(), ToolsEngineCoverageCommandError> {
        self.validate_bundle()?;
        let parts = self.parts(relative)?;
        let source = ToolsEngineCoverageCommandIoPath::capture_relative(
            vec![self.prepared.artifacts.identity.duplicate()?],
            Path::new(relative),
        )?;
        let source_file = source.open()?;
        let record =
            ToolsEngineCoverageCommandPublicationFile::capture(&source_file, source.path())?;
        self.ensure_kind(&parts.kind)?;
        let kind = self
            .kinds
            .get(parts.kind.as_os_str())
            .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
        Self::link_bound(&source, kind, &parts.name)?;
        self.linked.insert(relative.to_owned(), record);
        kind.sync()?;
        self.validate_bundle()
    }

    fn link_bound(
        source: &ToolsEngineCoverageCommandIoPath,
        destination: &ToolsEngineCoverageCommandIoDirectoryIdentity,
        name: &OsStr,
    ) -> Result<(), ToolsEngineCoverageCommandError> {
        let target = destination.path.join(name);
        #[cfg(unix)]
        {
            let source_parent = source
                .directories
                .last()
                .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
            let source_name = source
                .path()
                .file_name()
                .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
            rustix::fs::linkat(
                &source_parent.file,
                source_name,
                &destination.file,
                name,
                AtFlags::empty(),
            )
            .map_err(|error| ToolsEngineCoverageCommandError::io(&target, error.into()))?;
        }
        #[cfg(not(unix))]
        std::fs::hard_link(source.path(), &target)
            .map_err(|error| ToolsEngineCoverageCommandError::io(&target, error))?;
        Ok(())
    }

    fn validate_bundle(&self) -> Result<(), ToolsEngineCoverageCommandError> {
        self.prepared.destination.parent_identity.verify(None)?;
        let Some(bundle) = &self.bundle_identity else {
            if self.kinds.is_empty() && self.linked.is_empty() {
                return Ok(());
            }
            return Err(ToolsEngineCoverageCommandError::Configuration);
        };
        bundle.verify(Some(&self.prepared.destination.parent_identity))?;
        let names = bundle.names(self.kinds.len())?;
        if names.len() != self.kinds.len() {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(
                bundle.path.clone(),
            ));
        }
        let mut observed = 0;
        for name in names {
            let kind = self.kinds.get(name.as_os_str()).ok_or_else(|| {
                ToolsEngineCoverageCommandError::UnsafePath(bundle.path.join(&name))
            })?;
            kind.verify(Some(bundle))?;
            for file_name in kind.names(self.linked.len())? {
                let relative = Path::new(&self.prepared.destination.bundle_name)
                    .join(&name)
                    .join(&file_name);
                let relative = relative
                    .to_str()
                    .ok_or(ToolsEngineCoverageCommandError::Configuration)?
                    .replace('\\', "/");
                let record = self.linked.get(&relative).ok_or_else(|| {
                    ToolsEngineCoverageCommandError::UnsafePath(kind.path.join(&file_name))
                })?;
                let checked = self.linked_path(&relative)?;
                record.verify(&checked.open()?, checked.path())?;
                observed += 1;
            }
        }
        if observed != self.linked.len() {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(
                bundle.path.clone(),
            ));
        }
        Ok(())
    }

    fn validate_ready(
        &self,
        io: &ToolsEngineCoverageCommandIo,
        worker: &CapacityExecutorCancellation,
    ) -> Result<(), ToolsEngineCoverageCommandError> {
        self.validate_bundle()?;
        if self.linked.len() != self.prepared.artifacts.staged.len() {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(
                self.prepared.destination.bundle_path(),
            ));
        }
        for (relative, record) in &self.linked {
            io.check(worker)?;
            let checked = self.linked_path(relative)?;
            let file = checked.open()?;
            let maximum = record
                .bytes
                .checked_add(1)
                .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
            let digest = chess::package::digest::PackageDigest::read(file.take(maximum))
                .map_err(ToolsEngineCoverageError::from)?;
            if digest != self.parts(relative)?.digest {
                return Err(ToolsEngineCoverageCommandError::ArtifactHash(
                    checked.path().to_owned(),
                ));
            }
        }
        self.validate_bundle()
    }

    fn verify_report(&self) -> Result<(), ToolsEngineCoverageCommandError> {
        self.prepared.destination.parent_identity.verify(None)?;
        let file = self.report_file_bound()?;
        let maximum = u64::try_from(ToolsEngineCoverageReport::MAXIMUM_DOCUMENT_BYTES)
            .map_err(|_error| ToolsEngineCoverageCommandError::Configuration)?
            .checked_add(1)
            .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
        let digest = chess::package::digest::PackageDigest::read(file.take(maximum))
            .map_err(ToolsEngineCoverageError::from)?;
        if Some(digest) != self.report_digest {
            return Err(ToolsEngineCoverageCommandError::ArtifactHash(
                self.prepared.destination.report.clone(),
            ));
        }
        Ok(())
    }

    fn report_file_bound(&self) -> Result<File, ToolsEngineCoverageCommandError> {
        let Some(expected) = &self.report_identity else {
            return Err(ToolsEngineCoverageCommandError::Configuration);
        };
        let name = self
            .prepared
            .destination
            .report
            .file_name()
            .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
        #[cfg(unix)]
        let file = rustix::fs::openat(
            &self.prepared.destination.parent_identity.file,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(|error| {
            ToolsEngineCoverageCommandError::io(&self.prepared.destination.report, error.into())
        })?;
        #[cfg(not(unix))]
        let file = ToolsEngineCoverageCommandIoPath::capture_relative(
            vec![self.prepared.destination.parent_identity.duplicate()?],
            Path::new(name),
        )?
        .open()?;
        let current = Handle::from_file(file.try_clone().map_err(|error| {
            ToolsEngineCoverageCommandError::io(&self.prepared.destination.report, error)
        })?)
        .map_err(|error| {
            ToolsEngineCoverageCommandError::io(&self.prepared.destination.report, error)
        })?;
        if *expected != current {
            return Err(ToolsEngineCoverageCommandError::UnsafePath(
                self.prepared.destination.report.clone(),
            ));
        }
        Ok(file)
    }

    fn commit(
        &mut self,
        bytes: &[u8],
        io: &ToolsEngineCoverageCommandIo,
        worker: &CapacityExecutorCancellation,
    ) -> Result<(), ToolsEngineCoverageCommandError> {
        io.check(worker)?;
        self.prepared.destination.validate()?;
        if !self.prepared.artifacts.staged.is_empty() {
            self.create_bundle()?;
            let artifacts: Vec<_> = self.prepared.artifacts.staged.keys().cloned().collect();
            for relative in artifacts {
                io.check(worker)?;
                self.link_artifact(&relative)?;
            }
            self.bundle()?.sync()?;
        }
        self.publish_report(bytes, io, worker)
    }

    fn publish_report(
        &mut self,
        bytes: &[u8],
        io: &ToolsEngineCoverageCommandIo,
        worker: &CapacityExecutorCancellation,
    ) -> Result<(), ToolsEngineCoverageCommandError> {
        let identity = self.prepared.artifacts.directory.with_file(
            Path::new(Self::REPORT_STAGE_NAME),
            |file| {
                file.write_all(bytes).map_err(|error| {
                    ToolsEngineCoverageCommandError::io(
                        self.prepared.artifacts.directory.path(),
                        error,
                    )
                })?;
                Handle::from_file(file.try_clone().map_err(|error| {
                    ToolsEngineCoverageCommandError::io(&self.prepared.destination.report, error)
                })?)
                .map_err(|error| {
                    ToolsEngineCoverageCommandError::io(&self.prepared.destination.report, error)
                })
            },
        )?;
        let source = ToolsEngineCoverageCommandIoPath::capture_relative(
            vec![self.prepared.artifacts.identity.duplicate()?],
            Path::new(Self::REPORT_STAGE_NAME),
        )?;
        self.validate_ready(io, worker)?;
        io.check(worker)?;
        self.prepared.destination.validate()?;
        Self::link_bound(
            &source,
            &self.prepared.destination.parent_identity,
            self.prepared
                .destination
                .report
                .file_name()
                .ok_or(ToolsEngineCoverageCommandError::Configuration)?,
        )?;
        self.report_identity = Some(identity);
        self.report_digest = Some(
            chess::package::digest::PackageDigest::read(bytes)
                .map_err(ToolsEngineCoverageError::from)?,
        );
        self.validate_ready(io, worker)?;
        self.verify_report()?;
        self.published = true;
        self.prepared
            .destination
            .parent_identity
            .sync()
            .map_err(|error| ToolsEngineCoverageCommandError::PublishedSync {
                path: self.prepared.destination.report.clone(),
                source: std::io::Error::other(error),
            })
    }

    fn cleanup(&self) -> Result<(), ToolsEngineCoverageCommandError> {
        if self.report_identity.is_some() {
            drop(self.report_file_bound()?);
            #[cfg(unix)]
            rustix::fs::unlinkat(
                &self.prepared.destination.parent_identity.file,
                self.prepared
                    .destination
                    .report
                    .file_name()
                    .ok_or(ToolsEngineCoverageCommandError::Configuration)?,
                AtFlags::empty(),
            )
            .map_err(|error| {
                ToolsEngineCoverageCommandError::io(&self.prepared.destination.report, error.into())
            })?;
            #[cfg(not(unix))]
            std::fs::remove_file(&self.prepared.destination.report).map_err(|error| {
                ToolsEngineCoverageCommandError::io(&self.prepared.destination.report, error)
            })?;
        }
        for (relative, record) in &self.linked {
            let parts = self.parts(relative)?;
            let kind = self
                .kinds
                .get(parts.kind.as_os_str())
                .ok_or(ToolsEngineCoverageCommandError::Configuration)?;
            #[cfg(unix)]
            {
                let fd = rustix::fs::openat(
                    &kind.file,
                    parts.name.as_os_str(),
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
                    Mode::empty(),
                )
                .map(File::from)
                .map_err(|error| ToolsEngineCoverageCommandError::io(&kind.path, error.into()))?;
                record.verify(&fd, &kind.path.join(&parts.name))?;
                rustix::fs::unlinkat(&kind.file, parts.name.as_os_str(), AtFlags::empty())
                    .map_err(|error| {
                        ToolsEngineCoverageCommandError::io(&kind.path, error.into())
                    })?;
            }
            #[cfg(not(unix))]
            {
                let checked = self.linked_path(relative)?;
                record.verify(&checked.open()?, checked.path())?;
                std::fs::remove_file(checked.path())
                    .map_err(|error| ToolsEngineCoverageCommandError::io(checked.path(), error))?;
            }
        }
        if let Some(bundle) = &self.bundle_identity {
            for (name, kind) in &self.kinds {
                #[cfg(unix)]
                {
                    let current = ToolsEngineCoverageCommandIoDirectoryIdentity::open_directory(
                        &kind.path,
                        Some(bundle),
                    )?;
                    let identity = Handle::from_file(current)
                        .map_err(|error| ToolsEngineCoverageCommandError::io(&kind.path, error))?;
                    if identity != kind.handle {
                        return Err(ToolsEngineCoverageCommandError::UnsafePath(
                            kind.path.clone(),
                        ));
                    }
                    rustix::fs::unlinkat(&bundle.file, name.as_os_str(), AtFlags::REMOVEDIR)
                        .map_err(|error| {
                            ToolsEngineCoverageCommandError::io(&kind.path, error.into())
                        })?;
                }
                #[cfg(not(unix))]
                {
                    kind.verify(Some(bundle))?;
                    std::fs::remove_dir(&kind.path)
                        .map_err(|error| ToolsEngineCoverageCommandError::io(&kind.path, error))?;
                }
            }
            #[cfg(unix)]
            {
                let current = ToolsEngineCoverageCommandIoDirectoryIdentity::open_directory(
                    &bundle.path,
                    Some(&self.prepared.destination.parent_identity),
                )?;
                let identity = Handle::from_file(current)
                    .map_err(|error| ToolsEngineCoverageCommandError::io(&bundle.path, error))?;
                if identity != bundle.handle {
                    return Err(ToolsEngineCoverageCommandError::UnsafePath(
                        bundle.path.clone(),
                    ));
                }
                rustix::fs::unlinkat(
                    &self.prepared.destination.parent_identity.file,
                    OsStr::new(&self.prepared.destination.bundle_name),
                    AtFlags::REMOVEDIR,
                )
                .map_err(|error| ToolsEngineCoverageCommandError::io(&bundle.path, error.into()))?;
            }
            #[cfg(not(unix))]
            {
                bundle.verify(Some(&self.prepared.destination.parent_identity))?;
                std::fs::remove_dir(&bundle.path)
                    .map_err(|error| ToolsEngineCoverageCommandError::io(&bundle.path, error))?;
            }
        }
        Ok(())
    }

    fn cleanup_staging(&mut self) -> Result<(), ToolsEngineCoverageCommandError> {
        self.prepared.artifacts.directory.cleanup()
    }
}

pub(crate) struct ToolsEngineCoverageCommandSummary {
    classified: usize,
    passed: usize,
    failed: usize,
    outstanding: usize,
    upstream_unavailable: usize,
    satisfied: bool,
    output: PathBuf,
}

impl ToolsEngineCoverageCommandSummary {
    fn from_report(report: &ToolsEngineCoverageReport, output: PathBuf) -> Self {
        let mut summary = Self {
            classified: report.observations().len(),
            passed: 0,
            failed: 0,
            outstanding: 0,
            upstream_unavailable: 0,
            satisfied: report.is_satisfied(),
            output,
        };
        for observation in report.observations() {
            match observation.outcome() {
                ToolsEngineCoverageOutcome::Measured { verdict, .. } => match verdict {
                    ToolsEngineCoverageEnumVerdict::Passed => summary.passed += 1,
                    ToolsEngineCoverageEnumVerdict::Failed => summary.failed += 1,
                },
                ToolsEngineCoverageOutcome::Outstanding { .. } => summary.outstanding += 1,
                ToolsEngineCoverageOutcome::UpstreamUnavailable { .. } => {
                    summary.upstream_unavailable += 1
                }
            }
        }
        summary
    }
}

impl Display for ToolsEngineCoverageCommandSummary {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Coverage report {}: {} classified, {} measured passed, {} measured failed, {} upstream unavailable, {} outstanding. Evidence satisfied: {}. T090 real execution acceptance remains open.",
            self.output.display(),
            self.classified,
            self.passed,
            self.failed,
            self.upstream_unavailable,
            self.outstanding,
            self.satisfied
        )
    }
}

pub(crate) struct ToolsEngineCoverageCommand;

impl ToolsEngineCoverageCommand {
    pub(crate) async fn run(
        config: ToolsEngineCoverageCommandConfig,
        resources: ToolsEngineCoverageCommandResources<'_>,
    ) -> Result<ToolsEngineCoverageCommandSummary, ToolsEngineCoverageCommandError> {
        Self::run_with_host(config, resources, EnginePackageHost::detect).await
    }

    async fn run_with_host<F>(
        config: ToolsEngineCoverageCommandConfig,
        resources: ToolsEngineCoverageCommandResources<'_>,
        host: F,
    ) -> Result<ToolsEngineCoverageCommandSummary, ToolsEngineCoverageCommandError>
    where
        F: FnOnce() -> Result<EnginePackageHost, EnginePackageHostError>
            + Send
            + std::panic::UnwindSafe
            + 'static,
    {
        let cancellation = resources.cancellation;
        let runtime = resources.runtime;
        resources
            .executor
            .submit(move |worker| {
                ToolsEngineCoverageCommandIo::new(Arc::clone(&cancellation)).check(worker)?;
                let prepared = ToolsEngineCoverageCommandPrepared::prepare(
                    config,
                    Arc::clone(&cancellation),
                    host()?,
                    worker,
                )?;
                prepared.complete(&runtime, cancellation, worker)
            })?
            .join()
            .await?
    }
}

#[cfg(test)]
#[path = "../../tests/engine/coverage_command.rs"]
mod tests;
