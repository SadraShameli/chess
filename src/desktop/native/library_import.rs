use std::path::Path;
use std::sync::Arc;

use crate::capacity::executor::CapacityExecutor;
use crate::desktop::library_service::DesktopLibraryServiceImports;
#[cfg(unix)]
use crate::desktop::library_service::DesktopLibraryServiceImportStorage;
#[cfg(unix)]
use crate::game::enums::{GameId, GameIdentityError, GameMoveId};
use crate::game::storage_repository::GameStorageWallTimeProvider;
use crate::storage::document_store::StorageDocumentClient;
#[cfg(unix)]
use crate::storage::import_session::{
    StorageImportSessionDestination, StorageImportSessionFilesystem,
    StorageImportSessionIdentityProvider, StorageImportSessionResources,
    StorageImportSessionUnixSpace,
};
#[cfg(unix)]
use crate::storage::library_import::StorageLibraryImportRepository;

#[cfg(unix)]
pub struct DesktopNativeLibraryImportIdentities;

#[cfg(unix)]
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

    #[cfg(unix)]
    pub fn imports(
        client: StorageDocumentClient,
        executor: Arc<CapacityExecutor>,
        wall_time: Arc<dyn GameStorageWallTimeProvider>,
        data_directory: &Path,
    ) -> Result<Option<Arc<DesktopLibraryServiceImports>>, std::io::Error> {
        let staging = data_directory.join(Self::STAGING_DIRECTORY);
        std::fs::create_dir_all(&staging)?;
        let stage = std::fs::File::open(&staging)?;
        let library = std::fs::File::open(data_directory)?;
        let filesystem = Arc::new(StorageImportSessionFilesystem::new(
            stage,
            library,
            StorageImportSessionUnixSpace,
        ));
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

    #[cfg(not(unix))]
    pub fn imports(
        _client: StorageDocumentClient,
        _executor: Arc<CapacityExecutor>,
        _wall_time: Arc<dyn GameStorageWallTimeProvider>,
        _data_directory: &Path,
    ) -> Result<Option<Arc<DesktopLibraryServiceImports>>, std::io::Error> {
        Ok(None)
    }
}
