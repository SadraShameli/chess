mod content;
mod destruction;
mod import;
mod metadata;
mod organization;
mod rows;

use std::sync::Arc;

use rusqlite::{Connection, Transaction, TransactionBehavior};
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
    const BUMP_CATALOG_SQL: &'static str =
        "UPDATE library_state SET revision = ?1 WHERE singleton = 1";

    pub(crate) fn execute(
        connection: &mut Connection,
        mut request: StorageLibraryEnumTransactionRequest,
        memory: &Arc<Semaphore>,
        certified: &mut bool,
        origin: uuid::Uuid,
    ) -> Result<StorageLibraryEnumTransactionResponse, StorageLibraryError> {
        #[cfg(test)]
        if let StorageLibraryEnumTransactionRequest::ImportTestSql(sql) = &request {
            connection.execute_batch(sql)?;
            return Ok(StorageLibraryEnumTransactionResponse::ImportTestSql);
        }
        let behavior = match &request {
            StorageLibraryEnumTransactionRequest::Metadata(_)
            | StorageLibraryEnumTransactionRequest::CertifyAdoption(_)
            | StorageLibraryEnumTransactionRequest::RevokeAdoption
            | StorageLibraryEnumTransactionRequest::ImportSnapshot(_)
            | StorageLibraryEnumTransactionRequest::ImportCandidates(_)
            | StorageLibraryEnumTransactionRequest::ImportCandidate(_)
            | StorageLibraryEnumTransactionRequest::ImportEstimate { .. } => {
                TransactionBehavior::Deferred
            }
            StorageLibraryEnumTransactionRequest::CommitImport(_) => TransactionBehavior::Immediate,
            #[cfg(test)]
            StorageLibraryEnumTransactionRequest::ImportTestSql(_) => {
                TransactionBehavior::Immediate
            }
            StorageLibraryEnumTransactionRequest::Load(_)
            | StorageLibraryEnumTransactionRequest::Query(_)
            | StorageLibraryEnumTransactionRequest::PreviewDestruction(_)
            | StorageLibraryEnumTransactionRequest::AdoptionPage(_)
            | StorageLibraryEnumTransactionRequest::PlayedInventory(_) => {
                TransactionBehavior::Deferred
            }
            StorageLibraryEnumTransactionRequest::Save(_)
            | StorageLibraryEnumTransactionRequest::Organize(_)
            | StorageLibraryEnumTransactionRequest::ApplyDestruction(_)
            | StorageLibraryEnumTransactionRequest::Restore(_)
            | StorageLibraryEnumTransactionRequest::Adopt(_) => TransactionBehavior::Immediate,
        };
        let transaction = connection.transaction_with_behavior(behavior)?;
        let mut import_scope = None;
        let result = (|| {
            Ok(match &mut request {
                StorageLibraryEnumTransactionRequest::Metadata(request) => {
                    StorageLibraryEnumTransactionResponse::Metadata(
                        StorageLibraryTransactionMetadata::execute(&transaction, request, memory)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::CertifyAdoption(expected) => {
                    StorageLibraryEnumTransactionResponse::AdoptionCertification(
                        StorageLibraryAdoptionCertification::new(
                            Self::catalog(&transaction)?,
                            Self::catalog(&transaction)? == *expected,
                        ),
                    )
                }
                StorageLibraryEnumTransactionRequest::RevokeAdoption => {
                    StorageLibraryEnumTransactionResponse::AdoptionCertification(
                        StorageLibraryAdoptionCertification::new(
                            Self::catalog(&transaction)?,
                            false,
                        ),
                    )
                }
                StorageLibraryEnumTransactionRequest::ImportSnapshot(destination) => {
                    StorageLibraryEnumTransactionResponse::ImportSnapshot(
                        StorageLibraryTransactionImport::snapshot(
                            &transaction,
                            *destination,
                            *certified,
                        )?,
                    )
                }
                StorageLibraryEnumTransactionRequest::ImportCandidates(request) => {
                    StorageLibraryEnumTransactionResponse::ImportCandidates(
                        StorageLibraryTransactionImport::candidates(
                            &transaction,
                            request,
                            *certified,
                            memory,
                        )?,
                    )
                }
                StorageLibraryEnumTransactionRequest::ImportCandidate(candidate) => {
                    StorageLibraryEnumTransactionResponse::ImportCandidate(
                        StorageLibraryTransactionImport::candidate(
                            &transaction,
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
                        &transaction,
                        *snapshot,
                        *footprint,
                        *certified,
                        cancellation,
                    )?,
                ),
                StorageLibraryEnumTransactionRequest::CommitImport(request) => {
                    StorageLibraryEnumTransactionResponse::ImportCommitted(
                        StorageLibraryTransactionImport::commit(
                            &transaction,
                            request,
                            *certified,
                            origin,
                            &mut import_scope,
                        )?,
                    )
                }
                #[cfg(test)]
                StorageLibraryEnumTransactionRequest::ImportTestSql(sql) => {
                    transaction.execute_batch(sql)?;
                    StorageLibraryEnumTransactionResponse::ImportTestSql
                }
                StorageLibraryEnumTransactionRequest::Load(id) => {
                    StorageLibraryEnumTransactionResponse::Loaded(
                        StorageLibraryTransactionRows::load(&transaction, *id, memory)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::Query(query) => {
                    StorageLibraryEnumTransactionResponse::Page(
                        StorageLibraryTransactionRows::query(&transaction, query, memory)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::Save(prepared) => {
                    StorageLibraryEnumTransactionResponse::Saved(
                        StorageLibraryTransactionContent::save(&transaction, prepared)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::Organize(request) => {
                    StorageLibraryEnumTransactionResponse::Organized(
                        StorageLibraryTransactionOrganization::execute(&transaction, request)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::PreviewDestruction(request) => {
                    StorageLibraryEnumTransactionResponse::DestructionPreview(
                        StorageLibraryTransactionDestruction::preview(&transaction, *request)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::ApplyDestruction(request) => {
                    StorageLibraryEnumTransactionResponse::DestructionApplied(
                        StorageLibraryTransactionDestruction::apply(&transaction, request)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::Restore(request) => {
                    StorageLibraryEnumTransactionResponse::Restored(
                        StorageLibraryTransactionDestruction::restore(&transaction, request)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::AdoptionPage(cursor) => {
                    StorageLibraryEnumTransactionResponse::AdoptionPage(
                        StorageLibraryTransactionRows::adoption_page(
                            &transaction,
                            *cursor,
                            memory,
                        )?,
                    )
                }
                StorageLibraryEnumTransactionRequest::Adopt(prepared) => {
                    StorageLibraryEnumTransactionResponse::Adopted(
                        StorageLibraryTransactionContent::adopt(&transaction, prepared)?,
                    )
                }
                StorageLibraryEnumTransactionRequest::PlayedInventory(cursor) => {
                    StorageLibraryEnumTransactionResponse::AdoptionPage(
                        StorageLibraryTransactionRows::played_inventory(
                            &transaction,
                            *cursor,
                            memory,
                        )?,
                    )
                }
            })
        })();
        let response = match result {
            Ok(response) => response,
            Err(primary) => {
                let primary = Self::rollback(transaction, primary);
                return StorageLibraryImportPageLimit::finish(
                    connection,
                    import_scope,
                    Err(primary),
                );
            }
        };
        if let Err(error) = transaction.execute_batch("COMMIT") {
            let primary = Self::rollback(transaction, error.into());
            return StorageLibraryImportPageLimit::finish(connection, import_scope, Err(primary));
        }
        drop(transaction);
        let response =
            StorageLibraryImportPageLimit::finish(connection, import_scope, Ok(response))?;
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
    fn rollback(transaction: Transaction<'_>, primary: StorageLibraryError) -> StorageLibraryError {
        if transaction.is_autocommit() {
            return primary;
        }
        match transaction.rollback() {
            Ok(()) => primary,
            Err(rollback) => StorageLibraryError::Rollback {
                primary: Box::new(primary),
                rollback,
            },
        }
    }
    pub(super) fn catalog(
        transaction: &Transaction<'_>,
    ) -> Result<StorageLibraryCatalogRevision, StorageLibraryError> {
        let revision: String = transaction.query_row(
            "SELECT revision FROM library_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        revision.parse()
    }
    pub(super) fn bump_catalog(
        transaction: &Transaction<'_>,
    ) -> Result<StorageLibraryCatalogRevision, StorageLibraryError> {
        let revision = Self::catalog(transaction)?.next()?;
        transaction.execute(Self::BUMP_CATALOG_SQL, [revision.value().to_string()])?;
        Ok(revision)
    }
}

#[cfg(test)]
mod tests;
