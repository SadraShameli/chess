use std::sync::Arc;

use rusqlite::{OptionalExtension, Transaction, params};
use tokio::sync::Semaphore;

use crate::storage::document_store::StorageDocumentClient;
use crate::storage::import_session::{
    StorageImportSessionCandidate, StorageImportSessionCandidateBody,
    StorageImportSessionCandidateCursor, StorageImportSessionCandidatePage,
    StorageImportSessionCatalogStamp, StorageImportSessionCommitFootprint,
    StorageImportSessionCommitReceipt, StorageImportSessionDestination, StorageImportSessionError,
    StorageImportSessionSnapshot,
};
use crate::storage::library_import::{
    StorageLibraryImportCandidateRequest, StorageLibraryImportCommitRequest,
    StorageLibraryImportPageLimit, StorageLibraryImportSpace,
};
use crate::storage::library_prepared::{
    StorageLibraryImportRecoveredProjection, StorageLibraryProjection,
};
use crate::storage::library_repository::StorageLibraryError;
use crate::storage::library_transaction::StorageLibraryTransaction;
use crate::storage::library_transaction::content::{
    StorageLibraryTransactionContent, StorageLibraryTransactionContentInsert,
};

pub(super) struct StorageLibraryTransactionImport;

impl StorageLibraryTransactionImport {
    pub(super) fn snapshot(
        transaction: &Transaction<'_>,
        destination: StorageImportSessionDestination,
        certified: bool,
    ) -> Result<StorageImportSessionSnapshot, StorageLibraryError> {
        Self::destination(transaction, destination)?;
        Ok(StorageImportSessionSnapshot::new(
            StorageImportSessionCatalogStamp::new(
                StorageLibraryTransaction::catalog(transaction)?.value(),
            ),
            certified,
            destination,
        ))
    }

    fn destination(
        transaction: &Transaction<'_>,
        destination: StorageImportSessionDestination,
    ) -> Result<(), StorageLibraryError> {
        if let Some(folder) = destination.folder_id() {
            let revision: Option<String> = transaction
                .query_row(
                    "SELECT revision FROM library_folders WHERE folder_id = ?1",
                    [folder.to_string()],
                    |row| row.get(0),
                )
                .optional()?;
            if revision.as_deref().map(str::parse::<crate::game::library_organization::GameLibraryOrganizationRevision>).transpose()?.map(|revision| revision.value()) != Some(destination.revision()) { return Err(StorageLibraryError::ImportDestination); }
        } else if destination.revision() != 0 {
            return Err(StorageLibraryError::ImportDestination);
        }
        Ok(())
    }

    fn validate(
        transaction: &Transaction<'_>,
        snapshot: StorageImportSessionSnapshot,
        certified: bool,
    ) -> Result<(), StorageLibraryError> {
        if !certified || !snapshot.is_adoption_complete() {
            return Err(StorageLibraryError::AdoptionRequired);
        }
        if StorageLibraryTransaction::catalog(transaction)?.value()
            != snapshot.catalog_stamp().value()
        {
            return Err(StorageLibraryError::ImportCatalog);
        }
        Self::destination(transaction, snapshot.destination())
    }

    pub(super) fn candidates(
        transaction: &Transaction<'_>,
        request: &StorageLibraryImportCandidateRequest,
        certified: bool,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageImportSessionCandidatePage, StorageLibraryError> {
        if !certified {
            return Err(StorageLibraryError::AdoptionRequired);
        }
        let query = request.query();
        if StorageLibraryTransaction::catalog(transaction)?.value() != query.catalog_stamp().value()
        {
            return Err(StorageLibraryError::ImportCatalog);
        }
        let limit = query.limit();
        let reservation = StorageDocumentClient::reserve_memory(
            memory,
            (limit + 1)
                .checked_mul(std::mem::size_of::<StorageImportSessionCandidate>())
                .and_then(|bytes| {
                    bytes.checked_add(std::mem::size_of::<StorageImportSessionCandidatePage>())
                })
                .ok_or(StorageLibraryError::TextTooLarge)?,
        )?;
        let mut statement = transaction.prepare_cached("SELECT e.game_id, d.revision FROM library_entries e JOIN documents d ON d.id = e.game_id WHERE e.content_revision = d.revision AND e.rules_variant = ?1 AND e.identity_initial_fen = ?2 AND e.plies = ?3 AND e.identity_mainline_uci = ?4 AND e.game_id > ?5 ORDER BY e.game_id LIMIT ?6")?;
        let key = query.structural_key();
        let after = query
            .cursor()
            .boundary()
            .map(|game| game.as_uuid().to_string())
            .unwrap_or_default();
        let mut rows = statement.query(params![
            StorageLibraryProjection::variant_sql(key.variant()),
            key.initial_fen(),
            i64::try_from(key.mainline_uci().len())?,
            request.mainline(),
            after,
            i64::try_from(limit + 1)?
        ])?;
        let mut candidates = Vec::with_capacity(limit + 1);
        while let Some(row) = rows.next()? {
            let id = row.get_ref(0)?.as_str()?;
            let revision = row.get_ref(1)?.as_str()?;
            candidates.push(StorageImportSessionCandidate::new(
                id.parse()?,
                StorageLibraryError::content_revision(revision)?.value(),
            ));
        }
        let more = candidates.len() > limit;
        if more {
            candidates.pop();
        }
        let next = if more {
            candidates
                .last()
                .map(|candidate| StorageImportSessionCandidateCursor::after(candidate.game()))
        } else {
            None
        };
        Ok(StorageImportSessionCandidatePage::from_reserved(
            query.catalog_stamp(),
            candidates,
            next,
            reservation,
        )
        .map_err(StorageImportSessionError::from)?)
    }

    pub(super) fn candidate(
        transaction: &Transaction<'_>,
        candidate: StorageImportSessionCandidate,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageImportSessionCandidateBody, StorageLibraryError> {
        let mut statement = transaction.prepare_cached("SELECT d.revision, e.content_revision, d.body, e.content_sha256 FROM documents d JOIN library_entries e ON e.game_id = d.id WHERE d.id = ?1")?;
        let mut rows = statement.query([candidate.game().as_uuid().to_string()])?;
        let row = rows.next()?.ok_or(StorageLibraryError::ImportCandidate)?;
        let actual = row.get_ref(0)?.as_str()?;
        let projected = row.get_ref(1)?.as_str()?;
        if StorageLibraryError::content_revision(actual)?.value() != candidate.revision()
            || projected != actual
        {
            return Err(StorageLibraryError::ImportCandidate);
        }
        let body = row.get_ref(2)?.as_str()?;
        let digest = row.get_ref(3)?.as_blob()?;
        let bytes = body
            .len()
            .checked_add(StorageImportSessionCandidateBody::reserved_overhead())
            .ok_or(StorageLibraryError::TextTooLarge)?;
        let reservation = StorageDocumentClient::reserve_memory(memory, bytes)?;
        if crate::package::digest::PackageDigest::hash_slice(body.as_bytes())
            .bytes()
            .as_slice()
            != digest
        {
            return Err(StorageLibraryError::ProjectionStale);
        }
        Ok(StorageImportSessionCandidateBody::from_reserved(
            candidate,
            body.to_owned(),
            reservation,
        )
        .map_err(StorageImportSessionError::from)?)
    }

    pub(super) fn estimate(
        transaction: &Transaction<'_>,
        snapshot: StorageImportSessionSnapshot,
        footprint: StorageImportSessionCommitFootprint,
        certified: bool,
    ) -> Result<u64, StorageLibraryError> {
        Self::validate(transaction, snapshot, certified)?;
        Ok(StorageLibraryImportSpace::plan(transaction, footprint)?.bytes())
    }

    pub(super) fn commit(
        transaction: &Transaction<'_>,
        request: &mut StorageLibraryImportCommitRequest,
        certified: bool,
        origin: uuid::Uuid,
        scope: &mut Option<StorageLibraryImportPageLimit>,
    ) -> Result<StorageImportSessionCommitReceipt, StorageLibraryError> {
        let snapshot = StorageImportSessionSnapshot::new(
            request.prepared.catalog_stamp(),
            true,
            request.prepared.destination(),
        );
        Self::validate(transaction, snapshot, certified)?;
        request.prepared.recheck_space()?;
        let footprint = request.prepared.footprint();
        let space = StorageLibraryImportSpace::plan(transaction, footprint)?;
        if space.bytes() > request.prepared.confirmation().sqlite_bytes() {
            return Err(StorageLibraryError::ImportSpaceIncreased);
        }
        StorageLibraryImportPageLimit::install(transaction, space.page_ceiling(), scope)?;
        let mut count = 0_u64;
        let mut bodies = 0_u64;
        let mut projections = 0_u64;
        let destination = request.prepared.destination();
        let commit_id = request.prepared.commit_id();
        while let Some(frame) = request.prepared.next_frame(&request.cancellation)? {
            request.cancellation.check()?;
            let recovered = StorageLibraryImportRecoveredProjection::recover(&frame, origin)?;
            let body =
                std::str::from_utf8(frame.body()).map_err(StorageImportSessionError::from)?;
            StorageLibraryTransactionContent::insert_new(
                transaction,
                StorageLibraryTransactionContentInsert {
                    id: frame.game(),
                    body,
                    projection: recovered.projection(),
                    identity: recovered.identity(),
                    modified: Some(request.timestamp.value()),
                    folder: destination.folder_id(),
                },
            )?;
            count = count
                .checked_add(1)
                .ok_or(StorageLibraryError::ImportProof)?;
            bodies = bodies
                .checked_add(u64::try_from(frame.body().len())?)
                .ok_or(StorageLibraryError::ImportProof)?;
            projections = projections
                .checked_add(u64::try_from(frame.projection().len())?)
                .ok_or(StorageLibraryError::ImportProof)?;
        }
        request.prepared.finish(&request.cancellation)?;
        if count != footprint.count()
            || bodies != footprint.body_bytes()
            || projections != footprint.projection_bytes()
            || count != request.prepared.confirmation().summary().selected()
        {
            return Err(StorageLibraryError::ImportProof);
        }
        request.cancellation.check()?;
        StorageLibraryTransaction::bump_catalog(transaction)?;
        request.cancellation.check()?;
        Ok(StorageImportSessionCommitReceipt::new(commit_id, count))
    }
}
