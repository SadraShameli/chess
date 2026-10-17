use std::io::{self, Write};
use std::sync::Arc;

use crate::desktop::annotation_owner::DesktopAnnotationOwnerResources;
use crate::desktop::library_service::file::DesktopLibraryServiceFile;
use crate::desktop::library_service::DesktopLibraryServiceError;
use crate::game::enums::GameId;
use crate::game::storage_repository::GameStorageRevision;
use crate::notation::pgn_export::{NotationPgnExport, NotationPgnExportEnumSelection};
use crate::storage::atomic_file::StorageAtomicFileStage;
use crate::storage::library_record::StorageLibraryEnumContent;
use crate::storage::library_repository::{StorageLibraryError, StorageLibraryRepository};
use crate::storage::trash::StorageTrashEnumLocation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopLibraryServiceExport {
    game: GameId,
    storage_revision: GameStorageRevision,
    selection: NotationPgnExportEnumSelection,
    destination: DesktopLibraryServiceFile,
}

impl DesktopLibraryServiceExport {
    pub const fn new(
        game: GameId,
        storage_revision: GameStorageRevision,
        selection: NotationPgnExportEnumSelection,
        destination: DesktopLibraryServiceFile,
    ) -> Self {
        Self {
            game,
            storage_revision,
            selection,
            destination,
        }
    }

    pub const fn game(&self) -> GameId {
        self.game
    }

    pub const fn storage_revision(&self) -> GameStorageRevision {
        self.storage_revision
    }

    pub const fn selection(&self) -> NotationPgnExportEnumSelection {
        self.selection
    }

    pub const fn destination(&self) -> &DesktopLibraryServiceFile {
        &self.destination
    }

    pub(super) async fn run(
        self,
        resources: DesktopAnnotationOwnerResources,
    ) -> Result<u64, DesktopLibraryServiceError> {
        let loaded =
            StorageLibraryRepository::new(resources.client.clone(), &resources.executor)
                .load(self.game)
                .await?
                .ok_or(StorageLibraryError::MissingGame(self.game))?;
        if loaded.location() != StorageTrashEnumLocation::Library {
            return Err(StorageLibraryError::InTrash(self.game).into());
        }
        if loaded.content_revision() != self.storage_revision {
            return Err(StorageLibraryError::ContentConflict {
                expected: self.storage_revision,
                actual: loaded.content_revision(),
            }
            .into());
        }
        let selection = self.destination.take()?;
        let scope = self.selection;
        let record = loaded.into_record();
        resources
            .executor
            .submit_waiting(move |worker| {
                let target = selection
                    .into_destination()
                    .map_err(|error| DesktopLibraryServiceError::FileSelection(Arc::new(error)))?;
                let stage = StorageAtomicFileStage::new(target)
                    .map_err(|error| DesktopLibraryServiceError::AtomicFile(Arc::new(error)))?;
                let mut sink = DesktopLibraryServiceExportSink { stage };
                let export = match record.content() {
                    StorageLibraryEnumContent::Notation(game) => NotationPgnExport::new(game),
                    StorageLibraryEnumContent::Played(session) => {
                        NotationPgnExport::from_played(session)
                    }
                };
                export
                    .with_selection(scope)
                    .write(&mut sink, || worker.is_cancelled())
                    .map_err(|error| DesktopLibraryServiceError::Export(Arc::new(error)))?;
                sink.stage
                    .commit_if(|| worker.is_cancelled())
                    .map_err(|error| DesktopLibraryServiceError::AtomicFile(Arc::new(error)))
            })
            .await?
            .join()
            .await?
    }
}

struct DesktopLibraryServiceExportSink {
    stage: StorageAtomicFileStage,
}

impl Write for DesktopLibraryServiceExportSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stage.write(bytes).map_err(io::Error::other)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
