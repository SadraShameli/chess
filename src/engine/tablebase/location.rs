use std::collections::BTreeMap;
use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use same_file::Handle;
use thiserror::Error;
use uuid::Uuid;

use crate::engine::options::{EngineOptionsError, EngineOptionsSyzygyPath};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineTablebaseLocationEnumPlatform {
    Unix,
    Windows,
}

impl EngineTablebaseLocationEnumPlatform {
    pub const fn path_list_delimiter(self) -> char {
        match self {
            Self::Unix => ':',
            Self::Windows => ';',
        }
    }

    pub fn validate_alias_path(self, path: &Path) -> Result<(), EngineTablebaseLocationError> {
        let value = path
            .to_str()
            .ok_or_else(|| EngineTablebaseLocationError::AliasSyntax {
                path: path.to_path_buf(),
                delimiter: self.path_list_delimiter(),
            })?;
        if value.is_empty()
            || value.contains(|character: char| {
                character.is_whitespace()
                    || character.is_control()
                    || character == self.path_list_delimiter()
            })
        {
            return Err(EngineTablebaseLocationError::AliasSyntax {
                path: path.to_path_buf(),
                delimiter: self.path_list_delimiter(),
            });
        }
        Ok(())
    }

    const fn is_supported(self) -> bool {
        match self {
            Self::Unix => cfg!(unix),
            Self::Windows => cfg!(windows),
        }
    }
}

#[derive(Debug, Error)]
pub enum EngineTablebaseLocationError {
    #[error("Select at least one existing local tablebase directory")]
    EmptySelection,
    #[error("Select an existing readable directory instead of {path}")]
    NotDirectory { path: PathBuf },
    #[error("Grant read and directory traversal access to {path}, then select it again")]
    NotReadable { path: PathBuf },
    #[error("Select an owner-approved writable alias parent instead of {path}")]
    NotWritable { path: PathBuf },
    #[error(
        "Select an alias parent whose complete path has no whitespace, control characters or '{delimiter}' separator: {path}"
    )]
    AliasSyntax { path: PathBuf, delimiter: char },
    #[error("The {platform:?} tablebase location policy cannot run on this platform")]
    UnsupportedPlatform {
        platform: EngineTablebaseLocationEnumPlatform,
    },
    #[error("No readable local .rtbw or .rtbz table files were found in the selected directories")]
    NoTables,
    #[error(
        "The selected table must be a readable nonempty regular Syzygy file with a material filename: {path}"
    )]
    InvalidTable { path: PathBuf },
    #[error(
        "Different selected files share table name {name}; select directories with unambiguous material names"
    )]
    AliasCollision { name: String },
    #[error("Tablebase setup was cancelled")]
    Cancelled,
    #[error(
        "Cannot create a tablebase file link at {path}: {source}. On Windows, file links require Developer Mode or an existing link privilege; select a suitable owner-approved directory or continue ordinary chess without tablebases. Files are never copied and directory junctions are not substituted"
    )]
    LinkUnavailable {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "The managed alias identity changed at {path}; preserve its contents and prepare a new location"
    )]
    OwnershipChanged { path: PathBuf },
    #[error("Cannot access tablebase location {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Option(#[from] EngineOptionsError),
    #[error("Tablebase setup failed ({operation}); safe alias cleanup also failed ({cleanup})")]
    Cleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
}

impl EngineTablebaseLocationError {
    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

#[derive(Debug)]
pub struct EngineTablebaseLocationSelection {
    directories: Vec<PathBuf>,
}

impl EngineTablebaseLocationSelection {
    pub fn new(directories: Vec<PathBuf>) -> Result<Self, EngineTablebaseLocationError> {
        if directories.is_empty() {
            return Err(EngineTablebaseLocationError::EmptySelection);
        }
        Ok(Self { directories })
    }

    pub fn directories(&self) -> &[PathBuf] {
        &self.directories
    }
}

#[derive(Debug)]
pub struct EngineTablebaseLocationPolicy {
    parent: PathBuf,
    platform: EngineTablebaseLocationEnumPlatform,
}

impl EngineTablebaseLocationPolicy {
    pub const fn unix_temporary(parent: PathBuf) -> Self {
        Self {
            parent,
            platform: EngineTablebaseLocationEnumPlatform::Unix,
        }
    }

    pub const fn windows_owner_selected(parent: PathBuf) -> Self {
        Self {
            parent,
            platform: EngineTablebaseLocationEnumPlatform::Windows,
        }
    }

    fn validate_blocking(&self) -> Result<PathBuf, EngineTablebaseLocationError> {
        if !self.platform.is_supported() {
            return Err(EngineTablebaseLocationError::UnsupportedPlatform {
                platform: self.platform,
            });
        }
        self.platform.validate_alias_path(&self.parent)?;
        let parent = self
            .parent
            .canonicalize()
            .map_err(|source| EngineTablebaseLocationError::io(&self.parent, source))?;
        self.platform.validate_alias_path(&parent)?;
        EngineTablebaseLocation::validate_directory_blocking(&parent)?;
        let metadata = parent
            .metadata()
            .map_err(|source| EngineTablebaseLocationError::io(&parent, source))?;
        if !Self::has_write_permission(&metadata) {
            return Err(EngineTablebaseLocationError::NotWritable { path: parent });
        }
        Ok(parent)
    }

    fn has_write_permission(metadata: &Metadata) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            metadata.permissions().mode() & 0o222 != 0
        }
        #[cfg(not(unix))]
        {
            !metadata.permissions().readonly()
        }
    }
}

#[derive(Debug)]
struct EngineTablebaseLocationAlias {
    path: PathBuf,
    source: PathBuf,
    identity: Option<Metadata>,
}

impl EngineTablebaseLocationAlias {
    fn is_owned_blocking(&self) -> Result<bool, EngineTablebaseLocationError> {
        let metadata = self
            .path
            .symlink_metadata()
            .map_err(|source| EngineTablebaseLocationError::io(&self.path, source))?;
        let Some(identity) = &self.identity else {
            return Ok(false);
        };
        if !metadata.file_type().is_symlink() || !Self::same_identity(identity, &metadata) {
            return Ok(false);
        }
        Ok(fs::read_link(&self.path)
            .map_err(|source| EngineTablebaseLocationError::io(&self.path, source))?
            == self.source)
    }

    fn same_identity(first: &Metadata, second: &Metadata) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            first.dev() == second.dev() && first.ino() == second.ino()
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;

            first.creation_time() == second.creation_time()
                && first.last_write_time() == second.last_write_time()
                && first.file_attributes() == second.file_attributes()
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _identity = (first, second);
            false
        }
    }

    fn create_blocking(&self) -> Result<(), EngineTablebaseLocationError> {
        #[cfg(unix)]
        let result = std::os::unix::fs::symlink(&self.source, &self.path);
        #[cfg(windows)]
        let result = std::os::windows::fs::symlink_file(&self.source, &self.path);
        #[cfg(not(any(unix, windows)))]
        let result = Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Local tablebase links are supported on Unix and Windows desktop targets",
        ));
        result.map_err(|source| EngineTablebaseLocationError::LinkUnavailable {
            path: self.path.clone(),
            source,
        })
    }
}

#[derive(Debug)]
pub struct EngineTablebaseLocation {
    selection: EngineTablebaseLocationSelection,
    path: PathBuf,
    identity: Handle,
    aliases: Vec<EngineTablebaseLocationAlias>,
    engine_path: EngineOptionsSyzygyPath,
    is_removed: bool,
}

impl EngineTablebaseLocation {
    const ROOT_PREFIX: &str = "chess-syzygy-";
    #[cfg(unix)]
    const PRIVATE_MODE: u32 = 0o700;

    pub fn prepare_blocking(
        selection: EngineTablebaseLocationSelection,
        policy: EngineTablebaseLocationPolicy,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<Self, EngineTablebaseLocationError> {
        Self::check_cancellation(&is_cancelled)?;
        let parent = policy.validate_blocking()?;
        let files = Self::collect_files_blocking(&selection, &is_cancelled)?;
        Self::check_cancellation(&is_cancelled)?;
        let path = parent.join(format!("{}{}", Self::ROOT_PREFIX, Uuid::new_v4().simple()));
        policy.platform.validate_alias_path(&path)?;
        let engine_path = EngineOptionsSyzygyPath::new(
            path.to_str()
                .ok_or_else(|| EngineTablebaseLocationError::AliasSyntax {
                    path: path.clone(),
                    delimiter: policy.platform.path_list_delimiter(),
                })?
                .to_owned(),
        )?;
        Self::create_directory_blocking(&path)?;
        let identity = match Handle::from_path(&path) {
            Ok(identity) => identity,
            Err(source) => {
                let operation = EngineTablebaseLocationError::io(&path, source);
                return Err(Self::cleanup_failure(
                    operation,
                    fs::remove_dir(&path),
                    &path,
                ));
            }
        };
        let mut location = Self {
            selection,
            path,
            identity,
            aliases: Vec::with_capacity(files.len()),
            engine_path,
            is_removed: false,
        };
        if let Err(operation) = location.link_files_blocking(files, &is_cancelled) {
            return match location.cleanup_blocking() {
                Ok(()) => Err(operation),
                Err(cleanup) => Err(EngineTablebaseLocationError::Cleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
            };
        }
        Ok(location)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn engine_path(&self) -> &EngineOptionsSyzygyPath {
        &self.engine_path
    }

    pub fn source_directories(&self) -> &[PathBuf] {
        self.selection.directories()
    }

    pub fn file_count(&self) -> usize {
        self.aliases.len()
    }

    pub fn cleanup_blocking(&mut self) -> Result<(), EngineTablebaseLocationError> {
        if self.is_removed {
            return Ok(());
        }
        self.validate_identity_blocking()?;
        let mut failure = None;
        let mut index = self.aliases.len();
        while index > 0 {
            index -= 1;
            let alias = &self.aliases[index];
            let result = match alias.path.symlink_metadata() {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(source) => Err(EngineTablebaseLocationError::io(&alias.path, source)),
                Ok(_metadata) => match alias.is_owned_blocking() {
                    Ok(true) => {
                        self.validate_identity_blocking()?;
                        fs::remove_file(&alias.path)
                            .map_err(|source| EngineTablebaseLocationError::io(&alias.path, source))
                    }
                    Ok(false) => Err(EngineTablebaseLocationError::OwnershipChanged {
                        path: alias.path.clone(),
                    }),
                    Err(error) => Err(error),
                },
            };
            match result {
                Ok(()) => {
                    self.aliases.swap_remove(index);
                }
                Err(error) => {
                    failure = Some(match failure {
                        None => error,
                        Some(operation) => EngineTablebaseLocationError::Cleanup {
                            operation: Box::new(operation),
                            cleanup: Box::new(error),
                        },
                    });
                }
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        self.validate_identity_blocking()?;
        fs::remove_dir(&self.path)
            .map_err(|source| EngineTablebaseLocationError::io(&self.path, source))?;
        self.is_removed = true;
        Ok(())
    }

    fn validate_identity_blocking(&self) -> Result<(), EngineTablebaseLocationError> {
        let metadata = self
            .path
            .symlink_metadata()
            .map_err(|source| EngineTablebaseLocationError::io(&self.path, source))?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || self.identity
                != Handle::from_path(&self.path)
                    .map_err(|source| EngineTablebaseLocationError::io(&self.path, source))?
        {
            return Err(EngineTablebaseLocationError::OwnershipChanged {
                path: self.path.clone(),
            });
        }
        Ok(())
    }

    fn collect_files_blocking(
        selection: &EngineTablebaseLocationSelection,
        is_cancelled: &impl Fn() -> bool,
    ) -> Result<BTreeMap<String, PathBuf>, EngineTablebaseLocationError> {
        let mut files = BTreeMap::new();
        for directory in selection.directories() {
            Self::check_cancellation(is_cancelled)?;
            Self::validate_directory_blocking(directory)?;
            let entries = fs::read_dir(directory)
                .map_err(|source| EngineTablebaseLocationError::io(directory, source))?;
            for entry in entries {
                Self::check_cancellation(is_cancelled)?;
                let path = entry
                    .map_err(|source| EngineTablebaseLocationError::io(directory, source))?
                    .path();
                if let Some(name) = Self::table_name(&path)? {
                    let source = path
                        .canonicalize()
                        .map_err(|source| EngineTablebaseLocationError::io(&path, source))?;
                    Self::validate_table_blocking(&source)?;
                    match files.entry(name) {
                        std::collections::btree_map::Entry::Occupied(entry) => {
                            if entry.get() != &source {
                                return Err(EngineTablebaseLocationError::AliasCollision {
                                    name: entry.key().clone(),
                                });
                            }
                        }
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(source);
                        }
                    }
                }
            }
        }
        if files.is_empty() {
            return Err(EngineTablebaseLocationError::NoTables);
        }
        Ok(files)
    }

    fn table_name(path: &Path) -> Result<Option<String>, EngineTablebaseLocationError> {
        const EXTENSIONS: [&str; 2] = ["rtbw", "rtbz"];
        const PIECES: &[u8] = b"KQRBNP";

        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            return Ok(None);
        };
        if !EXTENSIONS.contains(&extension) {
            return Ok(None);
        }
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .and_then(|value| value.split_once('v'));
        if name.is_none_or(|(white, black)| {
            [white, black].into_iter().any(|material| {
                !material.starts_with('K')
                    || !material.bytes().all(|piece| PIECES.contains(&piece))
                    || material.bytes().filter(|piece| *piece == b'K').count() != 1
            })
        }) {
            return Err(EngineTablebaseLocationError::InvalidTable {
                path: path.to_path_buf(),
            });
        }
        Ok(path
            .file_name()
            .and_then(|value| value.to_str())
            .map(str::to_owned))
    }

    fn validate_directory_blocking(path: &Path) -> Result<(), EngineTablebaseLocationError> {
        let metadata = path
            .metadata()
            .map_err(|source| EngineTablebaseLocationError::io(path, source))?;
        if !metadata.is_dir() {
            return Err(EngineTablebaseLocationError::NotDirectory {
                path: path.to_path_buf(),
            });
        }
        Self::validate_read_permission(path, &metadata)?;
        drop(fs::read_dir(path).map_err(|source| EngineTablebaseLocationError::io(path, source))?);
        Ok(())
    }

    fn validate_read_permission(
        path: &Path,
        metadata: &Metadata,
    ) -> Result<(), EngineTablebaseLocationError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = metadata.permissions().mode();
            if mode & 0o444 == 0 || (metadata.is_dir() && mode & 0o111 == 0) {
                return Err(EngineTablebaseLocationError::NotReadable {
                    path: path.to_path_buf(),
                });
            }
        }
        #[cfg(not(unix))]
        let _validated = (path, metadata);
        Ok(())
    }

    fn validate_table_blocking(path: &Path) -> Result<(), EngineTablebaseLocationError> {
        let metadata = path
            .metadata()
            .map_err(|source| EngineTablebaseLocationError::io(path, source))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(EngineTablebaseLocationError::InvalidTable {
                path: path.to_path_buf(),
            });
        }
        Self::validate_read_permission(path, &metadata)?;
        let mut file =
            File::open(path).map_err(|source| EngineTablebaseLocationError::io(path, source))?;
        let mut byte = [0];
        file.read_exact(&mut byte)
            .map_err(|source| EngineTablebaseLocationError::io(path, source))?;
        Ok(())
    }

    fn create_directory_blocking(path: &Path) -> Result<(), EngineTablebaseLocationError> {
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;

            let mut builder = builder;
            builder.mode(Self::PRIVATE_MODE);
            builder
        };
        builder
            .create(path)
            .map_err(|source| EngineTablebaseLocationError::io(path, source))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Err(source) =
                fs::set_permissions(path, fs::Permissions::from_mode(Self::PRIVATE_MODE))
            {
                let operation = EngineTablebaseLocationError::io(path, source);
                return Err(Self::cleanup_failure(operation, fs::remove_dir(path), path));
            }
        }
        Ok(())
    }

    fn link_files_blocking(
        &mut self,
        files: BTreeMap<String, PathBuf>,
        is_cancelled: &impl Fn() -> bool,
    ) -> Result<(), EngineTablebaseLocationError> {
        for (name, source) in files {
            Self::check_cancellation(is_cancelled)?;
            self.validate_identity_blocking()?;
            let mut alias = EngineTablebaseLocationAlias {
                path: self.path.join(name),
                source,
                identity: None,
            };
            alias.create_blocking()?;
            match alias.path.symlink_metadata() {
                Ok(metadata) => {
                    alias.identity = Some(metadata);
                    self.aliases.push(alias);
                }
                Err(source) => {
                    let error = EngineTablebaseLocationError::io(&alias.path, source);
                    self.aliases.push(alias);
                    return Err(error);
                }
            }
        }
        Self::check_cancellation(is_cancelled)
    }

    fn check_cancellation(
        is_cancelled: &impl Fn() -> bool,
    ) -> Result<(), EngineTablebaseLocationError> {
        if is_cancelled() {
            return Err(EngineTablebaseLocationError::Cancelled);
        }
        Ok(())
    }

    fn cleanup_failure(
        operation: EngineTablebaseLocationError,
        cleanup: io::Result<()>,
        path: &Path,
    ) -> EngineTablebaseLocationError {
        match cleanup {
            Ok(()) => operation,
            Err(source) => EngineTablebaseLocationError::Cleanup {
                operation: Box::new(operation),
                cleanup: Box::new(EngineTablebaseLocationError::io(path, source)),
            },
        }
    }
}
