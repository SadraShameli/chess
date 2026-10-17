mod page_limit;
mod space;

pub(crate) use crate::storage::library_import::page_limit::StorageLibraryImportPageLimit;
pub(crate) use crate::storage::library_import::space::StorageLibraryImportSpace;

use std::sync::Arc;

use tokio::sync::OwnedSemaphorePermit;

use crate::capacity::executor::CapacityExecutorCancellation;
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::storage_repository::GameStorageWallTimeProvider;
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::import_session::{
    StorageImportSessionBodyProof, StorageImportSessionCancellation, StorageImportSessionCandidate,
    StorageImportSessionCandidateBody, StorageImportSessionCandidatePage,
    StorageImportSessionCandidateQuery, StorageImportSessionCommitFootprint,
    StorageImportSessionCommitReceipt, StorageImportSessionDestination,
    StorageImportSessionPreparedCommit, StorageImportSessionRepository,
    StorageImportSessionRepositoryError, StorageImportSessionRepositoryFuture,
    StorageImportSessionSnapshot,
};
use crate::storage::library_prepared::{
    StorageLibraryImportPreparedProjection, StorageLibraryPreparedIdentity,
    StorageLibraryPreparedMemory,
};
use crate::storage::library_record::StorageLibraryRecord;
use crate::storage::library_repository::StorageLibraryError;
use crate::storage::library_transaction::{
    StorageLibraryEnumTransactionRequest, StorageLibraryEnumTransactionResponse,
};

pub struct StorageLibraryImportRepository<W: ?Sized> {
    client: StorageDocumentClient,
    destination: StorageImportSessionDestination,
    wall_time: Arc<W>,
}

impl<W: GameStorageWallTimeProvider + ?Sized + 'static> StorageLibraryImportRepository<W> {
    pub const fn new(
        client: StorageDocumentClient,
        destination: StorageImportSessionDestination,
        wall_time: Arc<W>,
    ) -> Self {
        Self {
            client,
            destination,
            wall_time,
        }
    }

    fn effect(error: StorageLibraryError) -> StorageImportSessionRepositoryError {
        if matches!(&error, StorageLibraryError::ImportCatalog) {
            return StorageImportSessionRepositoryError::StaleCatalog;
        }
        if matches!(&error, StorageLibraryError::ImportDestination) {
            return StorageImportSessionRepositoryError::StaleDestination;
        }
        if matches!(&error, StorageLibraryError::ImportExisting) {
            return StorageImportSessionRepositoryError::ExistingGame;
        }
        if matches!(&error, StorageLibraryError::ImportCandidate) {
            return StorageImportSessionRepositoryError::StaleCandidate;
        }
        if matches!(&error, StorageLibraryError::AdoptionRequired) {
            return StorageImportSessionRepositoryError::IncompleteAdoption;
        }
        StorageImportSessionRepositoryError::Effect(Box::new(error))
    }

    async fn execute(
        &self,
        request: StorageLibraryEnumTransactionRequest,
    ) -> Result<StorageLibraryEnumTransactionResponse, StorageImportSessionRepositoryError> {
        self.client
            .begin_library(request)
            .map_err(Self::effect)?
            .wait()
            .await
            .map_err(Self::effect)
    }
}

pub(crate) struct StorageLibraryImportCandidateRequest {
    query: StorageImportSessionCandidateQuery,
    mainline: String,
    reservation: OwnedSemaphorePermit,
}

impl StorageLibraryImportCandidateRequest {
    fn new(
        client: &StorageDocumentClient,
        query: StorageImportSessionCandidateQuery,
    ) -> Result<Self, StorageLibraryError> {
        let key = query.structural_key();
        let bytes = key
            .mainline_uci()
            .iter()
            .try_fold(key.initial_fen().len(), |sum, token| {
                sum.checked_add(token.capacity())
                    .and_then(|sum| sum.checked_add(std::mem::size_of::<String>()))
                    .ok_or(StorageLibraryError::TextTooLarge)
            })?;
        let bytes = bytes
            .checked_add(
                query
                    .identity_bytes()
                    .map_err(StorageLibraryError::Import)?,
            )
            .ok_or(StorageLibraryError::TextTooLarge)?;
        let reservation = client
            .reserve_library_memory(StorageLibraryPreparedMemory::reservation_bytes(bytes)?)?;
        let mainline = StorageLibraryPreparedIdentity::encode_mainline(key)?;
        Ok(Self {
            query,
            mainline,
            reservation,
        })
    }
    pub(crate) fn validate_owner(
        &self,
        client: &StorageDocumentClient,
    ) -> Result<(), StorageLibraryError> {
        if !client.is_library_memory_owner(&self.reservation) {
            return Err(StorageLibraryError::ForeignPreparation);
        }
        Ok(())
    }
    pub(crate) const fn query(&self) -> &StorageImportSessionCandidateQuery {
        &self.query
    }
    pub(crate) fn mainline(&self) -> &str {
        &self.mainline
    }
}

pub(crate) struct StorageLibraryImportCommitRequest {
    pub(crate) prepared: StorageImportSessionPreparedCommit,
    pub(crate) cancellation: StorageImportSessionCancellation,
    pub(crate) timestamp: GameLibraryOrganizationTimestamp,
    origin: OwnedSemaphorePermit,
}

impl StorageLibraryImportCommitRequest {
    pub(crate) fn validate_owner(
        &self,
        client: &StorageDocumentClient,
    ) -> Result<(), StorageLibraryError> {
        if !client.is_library_memory_owner(&self.origin) {
            return Err(StorageLibraryError::ForeignPreparation);
        }
        Ok(())
    }
    pub(crate) fn admission_bytes(&self) -> Result<usize, StorageLibraryError> {
        let frame = usize::try_from(self.prepared.footprint().largest_frame_bytes())?;
        StorageLibraryPreparedMemory::reservation_bytes(frame)?
            .checked_add(std::mem::size_of::<Self>())
            .ok_or(StorageLibraryError::TextTooLarge)
    }
}

impl<W: GameStorageWallTimeProvider + ?Sized + 'static> StorageImportSessionRepository
    for StorageLibraryImportRepository<W>
{
    type PreparedProjection = StorageLibraryImportPreparedProjection;

    fn snapshot(&self) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionSnapshot> {
        Box::pin(async move {
            let StorageLibraryEnumTransactionResponse::ImportSnapshot(snapshot) = self
                .execute(StorageLibraryEnumTransactionRequest::ImportSnapshot(
                    self.destination,
                ))
                .await?
            else {
                return Err(StorageImportSessionRepositoryError::InvalidResponse);
            };
            Ok(snapshot)
        })
    }
    fn candidates(
        &self,
        query: StorageImportSessionCandidateQuery,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCandidatePage> {
        Box::pin(async move {
            let request = StorageLibraryImportCandidateRequest::new(&self.client, query)
                .map_err(Self::effect)?;
            let StorageLibraryEnumTransactionResponse::ImportCandidates(page) = self
                .execute(StorageLibraryEnumTransactionRequest::ImportCandidates(
                    request,
                ))
                .await?
            else {
                return Err(StorageImportSessionRepositoryError::InvalidResponse);
            };
            Ok(page)
        })
    }
    fn read_candidate(
        &self,
        candidate: StorageImportSessionCandidate,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCandidateBody> {
        Box::pin(async move {
            let StorageLibraryEnumTransactionResponse::ImportCandidate(body) = self
                .execute(StorageLibraryEnumTransactionRequest::ImportCandidate(
                    candidate,
                ))
                .await?
            else {
                return Err(StorageImportSessionRepositoryError::InvalidResponse);
            };
            Ok(body)
        })
    }
    fn prepare_projection(
        &self,
        record: &StorageLibraryRecord,
        body: StorageImportSessionBodyProof<'_>,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Self::PreparedProjection, StorageImportSessionRepositoryError> {
        StorageLibraryImportPreparedProjection::prepare(&self.client, record, body, cancellation)
            .map_err(Self::effect)
    }
    fn estimate_commit_space(
        &self,
        snapshot: StorageImportSessionSnapshot,
        footprint: StorageImportSessionCommitFootprint,
        cancellation: StorageImportSessionCancellation,
    ) -> StorageImportSessionRepositoryFuture<'_, u64> {
        Box::pin(async move {
            let StorageLibraryEnumTransactionResponse::ImportEstimate(bytes) = self
                .execute(StorageLibraryEnumTransactionRequest::ImportEstimate {
                    snapshot,
                    footprint,
                    cancellation,
                })
                .await?
            else {
                return Err(StorageImportSessionRepositoryError::InvalidResponse);
            };
            Ok(bytes)
        })
    }
    fn commit_import(
        &self,
        prepared: StorageImportSessionPreparedCommit,
        cancellation: StorageImportSessionCancellation,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCommitReceipt> {
        Box::pin(async move {
            if prepared.destination() != self.destination {
                return Err(StorageImportSessionRepositoryError::StaleDestination);
            }
            cancellation
                .check()
                .map_err(|error| StorageImportSessionRepositoryError::Effect(Box::new(error)))?;
            let timestamp = self
                .wall_time
                .now()
                .map_err(|error| StorageImportSessionRepositoryError::Effect(Box::new(error)))?;
            let origin = self
                .client
                .reserve_library_memory(std::mem::size_of::<StorageLibraryImportCommitRequest>())
                .map_err(StorageLibraryError::from)
                .map_err(Self::effect)?;
            let request = StorageLibraryImportCommitRequest {
                prepared,
                cancellation,
                timestamp,
                origin,
            };
            let StorageLibraryEnumTransactionResponse::ImportCommitted(receipt) = self
                .execute(StorageLibraryEnumTransactionRequest::CommitImport(
                    Box::new(request),
                ))
                .await?
            else {
                return Err(StorageImportSessionRepositoryError::InvalidResponse);
            };
            Ok(receipt)
        })
    }
}

#[cfg(test)]
#[path = "../../tests/storage/library_import.rs"]
mod tests;
