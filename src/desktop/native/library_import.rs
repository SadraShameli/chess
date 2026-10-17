use std::path::Path;
use std::sync::Arc;

use crate::capacity::executor::CapacityExecutor;
use crate::desktop::library_service::DesktopLibraryServiceImports;
#[cfg(any(unix, windows))]
use crate::desktop::library_service::DesktopLibraryServiceImportStorage;
#[cfg(any(unix, windows))]
use crate::game::enums::{GameId, GameIdentityError, GameMoveId};
use crate::game::storage_repository::GameStorageWallTimeProvider;
use crate::storage::document_store::StorageDocumentClient;
#[cfg(any(unix, windows))]
use crate::storage::import_session::{
    StorageImportSessionDestination, StorageImportSessionFilesystem,
    StorageImportSessionIdentityProvider, StorageImportSessionResources,
};
#[cfg(unix)]
use crate::storage::import_session::StorageImportSessionUnixSpace;
#[cfg(windows)]
use crate::storage::import_session::StorageImportSessionWindowsSpace;
#[cfg(any(unix, windows))]
use crate::storage::library_import::StorageLibraryImportRepository;

#[cfg(any(unix, windows))]
pub struct DesktopNativeLibraryImportIdentities;

#[cfg(any(unix, windows))]
impl StorageImportSessionIdentityProvider for DesktopNativeLibraryImportIdentities {
    fn next_game(&self) -> Result<GameId, GameIdentityError> {
        uuid::Uuid::new_v4().to_string().parse()
    }

    fn next_move(&self) -> Result<GameMoveId, GameIdentityError> {
        uuid::Uuid::new_v4().to_string().parse()
    }
}

pub struct DesktopNativeLibraryImport;

impl DesktopNativeLibraryImport {
    pub const STAGING_DIRECTORY: &str = "import-staging";
    #[cfg(windows)]
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    #[cfg(unix)]
    fn filesystem(
        staging: &Path,
        data_directory: &Path,
    ) -> Result<Arc<StorageImportSessionFilesystem<StorageImportSessionUnixSpace>>, std::io::Error>
    {
        let stage = std::fs::File::open(staging)?;
        let library = std::fs::File::open(data_directory)?;
        Ok(Arc::new(StorageImportSessionFilesystem::new(
            stage,
            library,
            StorageImportSessionUnixSpace,
        )))
    }

    #[cfg(windows)]
    fn open_directory(path: &Path) -> Result<std::fs::File, std::io::Error> {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(Self::FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
    }

    #[cfg(windows)]
    fn filesystem(
        staging: &Path,
        data_directory: &Path,
    ) -> Result<
        Arc<StorageImportSessionFilesystem<StorageImportSessionWindowsSpace>>,
        std::io::Error,
    > {
        let stage = Self::open_directory(staging)?;
        let library = Self::open_directory(data_directory)?;
        let space = StorageImportSessionWindowsSpace::new(staging, data_directory)
            .map_err(std::io::Error::other)?;
        Ok(Arc::new(StorageImportSessionFilesystem::new(
            stage,
            library,
            space,
            staging.to_path_buf(),
        )))
    }

    #[cfg(any(unix, windows))]
    pub fn imports(
        client: StorageDocumentClient,
        executor: Arc<CapacityExecutor>,
        wall_time: Arc<dyn GameStorageWallTimeProvider>,
        data_directory: &Path,
    ) -> Result<Option<Arc<DesktopLibraryServiceImports>>, std::io::Error> {
        let staging = data_directory.join(Self::STAGING_DIRECTORY);
        std::fs::create_dir_all(&staging)?;
        let filesystem = Self::filesystem(&staging, data_directory)?;
        let repository = Arc::new(StorageLibraryImportRepository::new(
            client,
            StorageImportSessionDestination::unfiled(),
            wall_time,
        ));
        let backend = DesktopLibraryServiceImportStorage::new(StorageImportSessionResources {
            executor,
            repository,
            filesystem,
            identities: Arc::new(DesktopNativeLibraryImportIdentities),
        });
        Ok(Some(Arc::new(DesktopLibraryServiceImports::new(Arc::new(
            backend,
        )))))
    }

    #[cfg(not(any(unix, windows)))]
    pub fn imports(
        _client: StorageDocumentClient,
        _executor: Arc<CapacityExecutor>,
        _wall_time: Arc<dyn GameStorageWallTimeProvider>,
        _data_directory: &Path,
    ) -> Result<Option<Arc<DesktopLibraryServiceImports>>, std::io::Error> {
        Ok(None)
    }
}
