mod content;
mod destruction;
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
use crate::storage::library_inventory::StorageLibraryInventoryCursor;
use crate::storage::library_prepared::{
    StorageLibraryPreparedAdoption, StorageLibraryPreparedWrite,
};
use crate::storage::library_query::{StorageLibraryQuery, StorageLibraryQueryPage};
use crate::storage::library_repository::{
    StorageLibraryAdoptionCursor, StorageLibraryAdoptionPage, StorageLibraryCatalogRevision,
    StorageLibraryContentReceipt, StorageLibraryEnumAdoptionOutcome, StorageLibraryError,
    StorageLibraryStoredRecord,
};
use crate::storage::library_transaction::content::StorageLibraryTransactionContent;
use crate::storage::library_transaction::destruction::StorageLibraryTransactionDestruction;
use crate::storage::library_transaction::organization::StorageLibraryTransactionOrganization;
use crate::storage::library_transaction::rows::StorageLibraryTransactionRows;
use crate::storage::trash::{
    StorageTrashConfirmedRequest, StorageTrashPlan, StorageTrashPreviewRequest,
    StorageTrashReceipt, StorageTrashRestoreRequest,
};

pub(crate) enum StorageLibraryEnumTransactionRequest {
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
            Self::Save(prepared) => prepared.validate_memory_owner(client),
            Self::Adopt(prepared) => prepared.validate_memory_owner(client),
            Self::Load(_)
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
            Self::Save(_) | Self::Adopt(_) => return Ok(0),
            Self::Load(_)
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
        connection: &mut Connection,
        request: StorageLibraryEnumTransactionRequest,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageLibraryEnumTransactionResponse, StorageLibraryError> {
        let behavior = match &request {
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
        let response = match &request {
            StorageLibraryEnumTransactionRequest::Load(id) => {
                StorageLibraryEnumTransactionResponse::Loaded(StorageLibraryTransactionRows::load(
                    &transaction,
                    *id,
                    memory,
                )?)
            }
            StorageLibraryEnumTransactionRequest::Query(query) => {
                StorageLibraryEnumTransactionResponse::Page(StorageLibraryTransactionRows::query(
                    &transaction,
                    query,
                    memory,
                )?)
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
                    StorageLibraryTransactionRows::adoption_page(&transaction, *cursor, memory)?,
                )
            }
            StorageLibraryEnumTransactionRequest::Adopt(prepared) => {
                StorageLibraryEnumTransactionResponse::Adopted(
                    StorageLibraryTransactionContent::adopt(&transaction, prepared)?,
                )
            }
            StorageLibraryEnumTransactionRequest::PlayedInventory(cursor) => {
                StorageLibraryEnumTransactionResponse::AdoptionPage(
                    StorageLibraryTransactionRows::played_inventory(&transaction, *cursor, memory)?,
                )
            }
        };
        transaction.commit()?;
        drop(request);
        Ok(response)
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
        transaction.execute(
            "UPDATE library_state SET revision = ?1 WHERE singleton = 1",
            [revision.value().to_string()],
        )?;
        Ok(revision)
    }
}

#[cfg(test)]
mod tests;
