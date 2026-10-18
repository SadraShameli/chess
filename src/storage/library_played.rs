use crate::capacity::executor::CapacityExecutor;
use crate::game::enums::{GameId, GameRevision};
use crate::game::storage_repository::{
    GameStorageEnumContentChange, GameStorageReceipt, GameStorageRecovered, GameStorageRepository,
    GameStorageSaveRequest,
};
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::game_document::{
    StorageGameDocument, StorageGameDocumentEnumContentKind, StorageGameDocumentError,
};
use crate::storage::library_prepared::{
    StorageLibraryEnumContentChange, StorageLibraryPreparedWrite,
    StorageLibraryPreparedWriteOptions,
};
use crate::storage::library_record::StorageLibraryEnumContent;
use crate::storage::library_repository::{
    StorageLibraryEnumMutationReceipt, StorageLibraryError, StorageLibraryMutation,
    StorageLibraryRepository,
};

struct StorageLibraryPlayedPrepared {
    game: GameId,
    game_revision: GameRevision,
    write: StorageLibraryPreparedWrite,
}

struct StorageLibraryPlayedPending {
    game: GameId,
    game_revision: GameRevision,
    mutation: StorageLibraryMutation,
}

impl StorageLibraryPlayedPending {
    async fn wait(&mut self) -> Result<GameStorageReceipt, StorageLibraryError> {
        let receipt = self.mutation.wait().await?;
        let StorageLibraryEnumMutationReceipt::Saved(receipt) = receipt else {
            return Err(StorageLibraryError::Protocol);
        };
        if receipt.id() != self.game {
            return Err(StorageLibraryError::Identity);
        }
        Ok(GameStorageReceipt::new(
            receipt.id(),
            self.game_revision,
            receipt.content_revision(),
        ))
    }
}

pub struct StorageLibraryPlayedRepository<'a> {
    library: StorageLibraryRepository<'a>,
    executor: &'a CapacityExecutor,
}

impl<'a> StorageLibraryPlayedRepository<'a> {
    pub const fn new(client: StorageDocumentClient, executor: &'a CapacityExecutor) -> Self {
        Self {
            library: StorageLibraryRepository::new(client, executor),
            executor,
        }
    }

    async fn prepare(
        &self,
        request: GameStorageSaveRequest,
    ) -> Result<StorageLibraryPlayedPrepared, StorageLibraryError> {
        let game = request.game();
        let game_revision = request.game_revision();
        let expected = request.expected_storage_revision();
        let change = match request.content_change() {
            GameStorageEnumContentChange::Checkpoint => StorageLibraryEnumContentChange::Checkpoint,
            GameStorageEnumContentChange::Changed(edit) => {
                StorageLibraryEnumContentChange::Changed(edit.timestamp())
            }
        };
        let mut memory = self.library.prepared_memory(0)?;
        let write = self
            .executor
            .submit_waiting(move |cancellation| {
                let retained_bytes =
                    StorageGameDocument::snapshot_memory_bytes(request.snapshot(), cancellation)?;
                memory.grow(retained_bytes)?;
                let document =
                    StorageGameDocument::from_snapshot(request.snapshot(), Some(cancellation))?;
                let body = document.json_with_memory(cancellation, retained_bytes, |required| {
                    memory.grow(required)
                })?;
                drop(document);
                memory.retain_body(body.capacity())?;
                StorageLibraryPreparedWrite::from_encoded(
                    StorageLibraryPreparedWriteOptions {
                        id: game,
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
            .await??;
        Ok(StorageLibraryPlayedPrepared {
            game,
            game_revision,
            write,
        })
    }

    fn begin_save(
        &self,
        prepared: StorageLibraryPlayedPrepared,
    ) -> Result<StorageLibraryPlayedPending, StorageLibraryError> {
        Ok(StorageLibraryPlayedPending {
            game: prepared.game,
            game_revision: prepared.game_revision,
            mutation: self.library.begin_save(prepared.write)?,
        })
    }
}

impl GameStorageRepository for StorageLibraryPlayedRepository<'_> {
    type Error = StorageLibraryError;

    async fn save(
        &self,
        request: GameStorageSaveRequest,
    ) -> Result<GameStorageReceipt, Self::Error> {
        let prepared = self.prepare(request).await?;
        self.begin_save(prepared)?.wait().await
    }

    async fn load(&self, game: GameId) -> Result<Option<GameStorageRecovered>, Self::Error> {
        let Some(recovered) = self.library.load(game).await? else {
            return Ok(None);
        };
        let revision = recovered.content_revision();
        let session = match recovered.into_record().into_content() {
            StorageLibraryEnumContent::Played(session) => *session,
            StorageLibraryEnumContent::Notation(_) => {
                return Err(StorageGameDocumentError::ContentKind {
                    expected: StorageGameDocumentEnumContentKind::Played,
                    found: StorageGameDocumentEnumContentKind::Notation,
                }
                .into());
            }
        };
        Ok(Some(GameStorageRecovered::new(session, revision)))
    }
}

#[cfg(test)]
#[path = "../../tests/storage/library_played.rs"]
mod tests;
