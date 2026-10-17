mod content;
mod destruction;
mod import;
mod metadata;
mod organization;
mod rows;

use std::sync::Arc;

use diesel::SqliteConnection;
#[cfg(test)]
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use tokio::sync::Semaphore;

use crate::game::enums::GameId;
use crate::game::library_organization::{
    GameLibraryOrganizationReceipt, GameLibraryOrganizationRequest,
};
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::import_session::{
    StorageImportSessionCancellation, StorageImportSessionCandidate,
    StorageImportSessionCandidateBody, StorageImportSessionCandidatePage,
    StorageImportSessionCommitFootprint, StorageImportSessionCommitReceipt,
    StorageImportSessionDestination, StorageImportSessionSnapshot,
};
use crate::storage::library_import::{
    StorageLibraryImportCandidateRequest, StorageLibraryImportCommitRequest,
    StorageLibraryImportPageLimit,
};
use crate::storage::library_inventory::StorageLibraryInventoryCursor;
use crate::storage::library_metadata::{
    StorageLibraryMetadataEnumRequest, StorageLibraryMetadataEnumResponse,
};
use crate::storage::library_prepared::{
    StorageLibraryPreparedAdoption, StorageLibraryPreparedWrite,
};
use crate::storage::library_query::{StorageLibraryQuery, StorageLibraryQueryPage};
use crate::storage::library_repository::StorageLibraryAdoptionCertification;
use crate::storage::library_repository::{
    StorageLibraryAdoptionCursor, StorageLibraryAdoptionPage, StorageLibraryCatalogRevision,
    StorageLibraryContentReceipt, StorageLibraryEnumAdoptionOutcome, StorageLibraryError,
    StorageLibraryStoredRecord,
};
use crate::storage::library_transaction::content::StorageLibraryTransactionContent;
use crate::storage::library_transaction::destruction::StorageLibraryTransactionDestruction;
use crate::storage::library_transaction::import::StorageLibraryTransactionImport;
use crate::storage::library_transaction::metadata::StorageLibraryTransactionMetadata;
use crate::storage::library_transaction::organization::StorageLibraryTransactionOrganization;
use crate::storage::library_transaction::rows::StorageLibraryTransactionRows;
use crate::storage::schema::library_state;
use crate::storage::trash::{
    StorageTrashConfirmedRequest, StorageTrashPlan, StorageTrashPreviewRequest,
    StorageTrashReceipt, StorageTrashRestoreRequest,
};

pub(crate) enum StorageLibraryEnumTransactionRequest {
    Metadata(StorageLibraryMetadataEnumRequest),
    CertifyAdoption(StorageLibraryCatalogRevision),
    RevokeAdoption,
    ImportSnapshot(StorageImportSessionDestination),
    ImportCandidates(StorageLibraryImportCandidateRequest),
    ImportCandidate(StorageImportSessionCandidate),
    ImportEstimate {
        snapshot: StorageImportSessionSnapshot,
        footprint: StorageImportSessionCommitFootprint,
        cancellation: StorageImportSessionCancellation,
    },
    CommitImport(Box<StorageLibraryImportCommitRequest>),
    #[cfg(test)]
    ImportTestSql(String),
    Load(GameId),
    Query(StorageLibraryQuery),
    Save(StorageLibraryPreparedWrite),
    Organize(GameLibraryOrganizationRequest),
    PreviewDestruction(StorageTrashPreviewRequest),
    ApplyDestruction(StorageTrashConfirmedRequest),
    Restore(StorageTrashRestoreRequest),
    AdoptionPage(StorageLibraryAdoptionCursor),
    PlayedInventory(StorageLibraryInventoryCursor),
    Adopt(StorageLibraryPreparedAdoption),
}

impl StorageLibraryEnumTransactionRequest {
    pub(crate) fn validate_memory_owner(
        &self,
        client: &StorageDocumentClient,
    ) -> Result<(), StorageLibraryError> {
        match self {
            #[cfg(test)]
            Self::ImportTestSql(_) => Ok(()),
            Self::ImportCandidates(request) => request.validate_owner(client),
            Self::CommitImport(request) => request.validate_owner(client),
            Self::Save(prepared) => prepared.validate_memory_owner(client),
            Self::Adopt(prepared) => prepared.validate_memory_owner(client),
            Self::Metadata(_)
            | Self::CertifyAdoption(_)
            | Self::RevokeAdoption
            | Self::ImportSnapshot(_)
            | Self::ImportCandidate(_)
            | Self::ImportEstimate { .. }
            | Self::Load(_)
            | Self::Query(_)
            | Self::Organize(_)
            | Self::PreviewDestruction(_)
            | Self::ApplyDestruction(_)
            | Self::Restore(_)
            | Self::AdoptionPage(_)
            | Self::PlayedInventory(_) => Ok(()),
        }
    }

    pub(crate) fn unreserved_admission_bytes(&self) -> Result<usize, StorageLibraryError> {
        let payload = match self {
            #[cfg(test)]
            Self::ImportTestSql(sql) => sql.capacity(),
            Self::CommitImport(request) => return request.admission_bytes(),
            Self::ImportCandidates(_) | Self::Save(_) | Self::Adopt(_) => return Ok(0),
            Self::Metadata(_)
            | Self::CertifyAdoption(_)
            | Self::RevokeAdoption
            | Self::ImportSnapshot(_)
            | Self::ImportCandidate(_)
            | Self::ImportEstimate { .. }
            | Self::Load(_)
            | Self::PreviewDestruction(_)
            | Self::ApplyDestruction(_)
            | Self::Restore(_)
            | Self::AdoptionPage(_)
            | Self::PlayedInventory(_) => 0,
            Self::Query(query) => query.transport_bytes()?,
            Self::Organize(request) => request.transport_bytes(),
        };
        Ok(std::mem::size_of::<Self>() + payload)
    }
}

pub(crate) enum StorageLibraryEnumTransactionResponse {
    Metadata(StorageLibraryMetadataEnumResponse),
    AdoptionCertification(StorageLibraryAdoptionCertification),
    ImportSnapshot(StorageImportSessionSnapshot),
    ImportCandidates(StorageImportSessionCandidatePage),
    ImportCandidate(StorageImportSessionCandidateBody),
    ImportEstimate(u64),
    ImportCommitted(StorageImportSessionCommitReceipt),
    #[cfg(test)]
    ImportTestSql,
    Loaded(Option<StorageLibraryStoredRecord>),
    Page(StorageLibraryQueryPage),
    Saved(StorageLibraryContentReceipt),
    Organized(GameLibraryOrganizationReceipt),
    DestructionPreview(StorageTrashPlan),
    DestructionApplied(StorageTrashReceipt),
    Restored(StorageTrashReceipt),
    AdoptionPage(StorageLibraryAdoptionPage),
    Adopted(StorageLibraryEnumAdoptionOutcome),
}

pub(crate) struct StorageLibraryTransaction;

impl StorageLibraryTransaction {
    pub(crate) fn execute(
        connection: &mut SqliteConnection,
        mut request: StorageLibraryEnumTransactionRequest,
        memory: &Arc<Semaphore>,
        certified: &mut bool,
        origin: uuid::Uuid,
    ) -> Result<StorageLibraryEnumTransactionResponse, StorageLibraryError> {
        #[cfg(test)]
        if let StorageLibraryEnumTransactionRequest::ImportTestSql(sql) = &request {
            connection.batch_execute(sql)?;
            return Ok(StorageLibraryEnumTransactionResponse::ImportTestSql);
        }
        if let StorageLibraryEnumTransactionRequest::Query(query) = &request {
            return Ok(StorageLibraryEnumTransactionResponse::Page(
                StorageLibraryTransactionRows::query(connection, query, memory)?,
            ));
        }
        let is_write = matches!(
            request,
            StorageLibraryEnumTransactionRequest::CommitImport(_)
                | StorageLibraryEnumTransactionRequest::Save(_)
                | StorageLibraryEnumTransactionRequest::Organize(_)
                | StorageLibraryEnumTransactionRequest::ApplyDestruction(_)
                | StorageLibraryEnumTransactionRequest::Restore(_)
                | StorageLibraryEnumTransactionRequest::Adopt(_)
        );
        let mut import_scope = None;
        let mut run = |transaction: &mut SqliteConnection| -> Result<
            StorageLibraryEnumTransactionResponse,
            StorageLibraryError,
        > {
            Ok(match &mut request {
                StorageLibraryEnumTransactionRequest::Metadata(request) => {
                    StorageLibraryEnumTransactionResponse::Metadata(
                        StorageLibraryTransactionMetadata::execute(transaction, request, memory)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::CertifyAdoption(expected) => {
                    StorageLibraryEnumTransactionResponse::AdoptionCertification(
                        StorageLibraryAdoptionCertification::new(
                            Self::diesel_catalog(transaction)?,
                            Self::diesel_catalog(transaction)? == *expected,
                        ),
                    )
                }
                StorageLibraryEnumTransactionRequest::RevokeAdoption => {
                    StorageLibraryEnumTransactionResponse::AdoptionCertification(
                        StorageLibraryAdoptionCertification::new(
                            Self::diesel_catalog(transaction)?,
                            false,
                        ),
                    )
                }
                StorageLibraryEnumTransactionRequest::ImportSnapshot(destination) => {
                    StorageLibraryEnumTransactionResponse::ImportSnapshot(
                        StorageLibraryTransactionImport::snapshot(
                            transaction,
                            *destination,
                            *certified,
                        )?,
                    )
                }
                StorageLibraryEnumTransactionRequest::ImportCandidates(request) => {
                    StorageLibraryEnumTransactionResponse::ImportCandidates(
                        StorageLibraryTransactionImport::candidates(
                            transaction,
                            request,
                            *certified,
                            memory,
                        )?,
                    )
                }
                StorageLibraryEnumTransactionRequest::ImportCandidate(candidate) => {
                    StorageLibraryEnumTransactionResponse::ImportCandidate(
                        StorageLibraryTransactionImport::candidate(
                            transaction,
                            *candidate,
                            memory,
                        )?,
                    )
                }
                StorageLibraryEnumTransactionRequest::ImportEstimate {
                    snapshot,
                    footprint,
                    cancellation,
                } => StorageLibraryEnumTransactionResponse::ImportEstimate(
                    StorageLibraryTransactionImport::estimate(
                        transaction,
                        *snapshot,
                        *footprint,
                        *certified,
                        cancellation,
                    )?,
                ),
                StorageLibraryEnumTransactionRequest::CommitImport(request) => {
                    StorageLibraryEnumTransactionResponse::ImportCommitted(
                        StorageLibraryTransactionImport::commit(
                            transaction,
                            request,
                            *certified,
                            origin,
                            &mut import_scope,
                        )?,
                    )
                }
                #[cfg(test)]
                StorageLibraryEnumTransactionRequest::ImportTestSql(sql) => {
                    transaction.batch_execute(sql)?;
                    StorageLibraryEnumTransactionResponse::ImportTestSql
                }
                StorageLibraryEnumTransactionRequest::Load(id) => {
                    StorageLibraryEnumTransactionResponse::Loaded(
                        StorageLibraryTransactionRows::load(transaction, *id, memory)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::Query(_) => {
                    return Err(StorageLibraryError::Protocol);
                }
                StorageLibraryEnumTransactionRequest::Save(prepared) => {
                    StorageLibraryEnumTransactionResponse::Saved(
                        StorageLibraryTransactionContent::save(transaction, prepared)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::Organize(request) => {
                    StorageLibraryEnumTransactionResponse::Organized(
                        StorageLibraryTransactionOrganization::execute(transaction, request)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::PreviewDestruction(request) => {
                    StorageLibraryEnumTransactionResponse::DestructionPreview(
                        StorageLibraryTransactionDestruction::preview(transaction, *request)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::ApplyDestruction(request) => {
                    StorageLibraryEnumTransactionResponse::DestructionApplied(
                        StorageLibraryTransactionDestruction::apply(transaction, request)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::Restore(request) => {
                    StorageLibraryEnumTransactionResponse::Restored(
                        StorageLibraryTransactionDestruction::restore(transaction, request)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::AdoptionPage(cursor) => {
                    StorageLibraryEnumTransactionResponse::AdoptionPage(
                        StorageLibraryTransactionRows::adoption_page(
                            transaction,
                            *cursor,
                            memory,
                        )?,
                    )
                }
                StorageLibraryEnumTransactionRequest::Adopt(prepared) => {
                    StorageLibraryEnumTransactionResponse::Adopted(
                        StorageLibraryTransactionContent::adopt(transaction, prepared)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::PlayedInventory(cursor) => {
                    StorageLibraryEnumTransactionResponse::AdoptionPage(
                        StorageLibraryTransactionRows::played_inventory(
                            transaction,
                            *cursor,
                            memory,
                        )?,
                    )
                }
            })
        };
        let mut primary = None;
        let guarded = |transaction: &mut SqliteConnection| match run(transaction) {
            Ok(response) => Ok(response),
            Err(error) => {
                primary = Some(error);
                Err(StorageLibraryError::Protocol)
            }
        };
        let outcome = if is_write {
            connection.immediate_transaction(guarded)
        } else {
            connection.transaction(guarded)
        };
        let result = match (outcome, primary) {
            (Ok(response), _) => Ok(response),
            (Err(StorageLibraryError::Diesel(rollback)), Some(primary)) => {
                if rollback.to_string().contains("no transaction is active") {
                    Err(primary)
                } else {
                    Err(StorageLibraryError::Rollback {
                        primary: Box::new(primary),
                        rollback,
                    })
                }
            }
            (Err(_sentinel), Some(primary)) => Err(primary),
            (Err(error), None) => Err(error),
        };
        let response = StorageLibraryImportPageLimit::finish(connection, import_scope, result)?;
        match (&request, &response) {
            (
                StorageLibraryEnumTransactionRequest::CertifyAdoption(_),
                StorageLibraryEnumTransactionResponse::AdoptionCertification(result),
            ) => {
                if result.is_certified() {
                    *certified = true;
                }
            }
            (StorageLibraryEnumTransactionRequest::RevokeAdoption, _) => *certified = false,
            _ => {}
        }
        drop(request);
        Ok(response)
    }
    pub(super) fn diesel_catalog(
        connection: &mut SqliteConnection,
    ) -> Result<StorageLibraryCatalogRevision, StorageLibraryError> {
        let revision: String = library_state::table
            .filter(library_state::singleton.eq(Some(1)))
            .select(library_state::revision)
            .first(connection)?;
        revision.parse()
    }
    pub(super) fn diesel_bump_catalog(
        connection: &mut SqliteConnection,
    ) -> Result<StorageLibraryCatalogRevision, StorageLibraryError> {
        let revision = Self::diesel_catalog(connection)?.next()?;
        diesel::update(library_state::table.filter(library_state::singleton.eq(Some(1))))
            .set(library_state::revision.eq(revision.value().to_string()))
            .execute(connection)?;
        Ok(revision)
    }
}

#[cfg(test)]
mod tests;
