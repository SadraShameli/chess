use std::sync::Arc;

use crate::capacity::executor::CapacityExecutor;
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::game_document::{
    StorageGameDocument, StorageGameDocumentEnumContentKind, StorageGameDocumentError,
};
use crate::storage::library_prepared::{
    StorageLibraryEnumContentChange, StorageLibraryPreparedWrite,
    StorageLibraryPreparedWriteOptions,
};
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};
use crate::storage::library_repository::{
    StorageLibraryError, StorageLibraryMutation, StorageLibraryRepository,
};

pub struct StorageLibraryNotationRepository<'a> {
    library: StorageLibraryRepository<'a>,
    executor: &'a CapacityExecutor,
}

impl<'a> StorageLibraryNotationRepository<'a> {
    pub const fn new(client: StorageDocumentClient, executor: &'a CapacityExecutor) -> Self {
        Self {
            library: StorageLibraryRepository::new(client, executor),
            executor,
        }
    }

    pub async fn prepare(
        &self,
        expected: GameStorageRevision,
        record: Arc<StorageLibraryRecord>,
        change: StorageLibraryEnumContentChange,
    ) -> Result<StorageLibraryPreparedWrite, StorageLibraryError> {
        let mut memory = self.library.prepared_memory(0)?;
        self.executor
            .submit_waiting(move |cancellation| {
                let game = match record.content() {
                    StorageLibraryEnumContent::Notation(game) => game,
                    StorageLibraryEnumContent::Played(_) => {
                        return Err(StorageGameDocumentError::ContentKind {
                            expected: StorageGameDocumentEnumContentKind::Notation,
                            found: StorageGameDocumentEnumContentKind::Played,
                        }
                        .into());
                    }
                };
                let id = record.id();
                let retained_bytes =
                    StorageGameDocument::notation_memory_bytes(game, cancellation)?;
                memory.grow(retained_bytes)?;
                let document =
                    StorageGameDocument::capture_notation_cancellable(id, game, cancellation)?;
                let body = document.json_with_memory(cancellation, retained_bytes, |required| {
                    memory.grow(required)
                })?;
                drop(document);
                memory.retain_body(body.capacity())?;
                StorageLibraryPreparedWrite::from_encoded(
                    StorageLibraryPreparedWriteOptions {
                        id,
                        expected,
                        body,
                        change,
                        memory,
                    },
                    cancellation,
                )
            })
            .await?
            .join()
            .await?
    }

    pub fn begin_save(
        &self,
        prepared: StorageLibraryPreparedWrite,
    ) -> Result<StorageLibraryMutation, StorageLibraryError> {
        self.library.begin_save(prepared)
    }
}

#[cfg(test)]
#[path = "../../tests/storage/library_notation.rs"]
mod tests;
