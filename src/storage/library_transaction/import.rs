use std::sync::Arc;

use diesel::SqliteConnection;
use diesel::prelude::*;
use tokio::sync::Semaphore;

use crate::storage::document_store::StorageDocumentClient;
use crate::storage::import_session::{
    StorageImportSessionCancellation, StorageImportSessionCandidate,
    StorageImportSessionCandidateBody, StorageImportSessionCandidateCursor,
    StorageImportSessionCandidatePage, StorageImportSessionCatalogStamp,
    StorageImportSessionCommitFootprint, StorageImportSessionCommitReceipt,
    StorageImportSessionDestination, StorageImportSessionError, StorageImportSessionSnapshot,
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
use crate::storage::schema::{documents, library_entries, library_folders};

pub(super) struct StorageLibraryTransactionImport;

impl StorageLibraryTransactionImport {
    pub(super) fn snapshot(
        connection: &mut SqliteConnection,
        destination: StorageImportSessionDestination,
    ) -> Result<StorageImportSessionSnapshot, StorageLibraryError> {
        Self::destination(connection, destination)?;
        Ok(StorageImportSessionSnapshot::new(
            StorageImportSessionCatalogStamp::new(
                StorageLibraryTransaction::diesel_catalog(connection)?.value(),
            ),
            destination,
        ))
    }

    fn destination(
        connection: &mut SqliteConnection,
        destination: StorageImportSessionDestination,
    ) -> Result<(), StorageLibraryError> {
        if let Some(folder) = destination.folder_id() {
            let revision: Option<String> = library_folders::table
                .filter(library_folders::folder_id.eq(folder.to_string()))
                .select(library_folders::revision)
                .first(connection)
                .optional()?;
            if revision.as_deref().map(str::parse::<crate::game::library_organization::GameLibraryOrganizationRevision>).transpose()?.map(|revision| revision.value()) != Some(destination.revision()) { return Err(StorageLibraryError::ImportDestination); }
        } else if destination.revision() != 0 {
            return Err(StorageLibraryError::ImportDestination);
        }
        Ok(())
    }

    fn validate(
        connection: &mut SqliteConnection,
        snapshot: StorageImportSessionSnapshot,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<(), StorageLibraryError> {
        cancellation.check()?;
        if StorageLibraryTransaction::diesel_catalog(connection)?.value()
            != snapshot.catalog_stamp().value()
        {
            return Err(StorageLibraryError::ImportCatalog);
        }
        cancellation.check()?;
        Self::destination(connection, snapshot.destination())
    }

    pub(super) fn candidates(
        connection: &mut SqliteConnection,
        request: &StorageLibraryImportCandidateRequest,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageImportSessionCandidatePage, StorageLibraryError> {
        let query = request.query();
        if StorageLibraryTransaction::diesel_catalog(connection)?.value()
            != query.catalog_stamp().value()
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
        let key = query.structural_key();
        let mut rows = library_entries::table
            .inner_join(documents::table)
            .filter(library_entries::content_revision.eq(documents::revision))
            .filter(
                library_entries::rules_variant
                    .eq(StorageLibraryProjection::variant_sql(key.variant())),
            )
            .filter(library_entries::identity_initial_fen.eq(key.initial_fen()))
            .filter(library_entries::plies.eq(i32::try_from(key.mainline_uci().len())?))
            .filter(library_entries::identity_mainline_uci.eq(request.mainline()))
            .select((library_entries::game_id, documents::revision))
            .order(library_entries::game_id.asc())
            .limit(i64::try_from(limit + 1)?)
            .into_boxed();
        if let Some(boundary) = query.cursor().boundary() {
            rows = rows.filter(library_entries::game_id.gt(boundary.as_uuid().to_string()));
        }
        let rows: Vec<(String, String)> = rows.load(connection)?;
        let mut candidates = Vec::with_capacity(limit + 1);
        for (id, revision) in rows {
            candidates.push(StorageImportSessionCandidate::new(
                id.parse()?,
                StorageLibraryError::content_revision(&revision)?.value(),
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
        connection: &mut SqliteConnection,
        candidate: StorageImportSessionCandidate,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageImportSessionCandidateBody, StorageLibraryError> {
        let (actual, projected, body, digest): (String, String, String, Vec<u8>) = documents::table
            .inner_join(library_entries::table)
            .filter(documents::id.eq(candidate.game().as_uuid().to_string()))
            .select((
                documents::revision,
                library_entries::content_revision,
                documents::body,
                library_entries::content_sha256,
            ))
            .first(connection)
            .optional()?
            .ok_or(StorageLibraryError::ImportCandidate)?;
        if StorageLibraryError::content_revision(&actual)?.value() != candidate.revision()
            || projected != actual
        {
            return Err(StorageLibraryError::ImportCandidate);
        }
        let bytes = body
            .len()
            .checked_add(StorageImportSessionCandidateBody::reserved_overhead())
            .ok_or(StorageLibraryError::TextTooLarge)?;
        let reservation = StorageDocumentClient::reserve_memory(memory, bytes)?;
        if crate::package::digest::PackageDigest::hash_slice(body.as_bytes())
            .bytes()
            .as_slice()
            != digest.as_slice()
        {
            return Err(StorageLibraryError::ProjectionStale);
        }
        Ok(
            StorageImportSessionCandidateBody::from_reserved(candidate, body, reservation)
                .map_err(StorageImportSessionError::from)?,
        )
    }

    pub(super) fn estimate(
        connection: &mut SqliteConnection,
        snapshot: StorageImportSessionSnapshot,
        footprint: StorageImportSessionCommitFootprint,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<u64, StorageLibraryError> {
        cancellation.check()?;
        Self::validate(connection, snapshot, cancellation)?;
        Ok(StorageLibraryImportSpace::plan(connection, footprint, cancellation)?.bytes())
    }

    pub(super) fn commit(
        connection: &mut SqliteConnection,
        request: &mut StorageLibraryImportCommitRequest,
        origin: uuid::Uuid,
        scope: &mut Option<StorageLibraryImportPageLimit>,
    ) -> Result<StorageImportSessionCommitReceipt, StorageLibraryError> {
        request.cancellation.check()?;
        let snapshot = StorageImportSessionSnapshot::new(
            request.prepared.catalog_stamp(),
            request.prepared.destination(),
        );
        Self::validate(connection, snapshot, &request.cancellation)?;
        request.prepared.recheck_space()?;
        let footprint = request.prepared.footprint();
        let space = StorageLibraryImportSpace::plan(connection, footprint, &request.cancellation)?;
        if space.bytes() > request.prepared.confirmation().sqlite_bytes() {
            return Err(StorageLibraryError::ImportSpaceIncreased);
        }
        request.cancellation.check()?;
        StorageLibraryImportPageLimit::install(connection, space.page_ceiling(), scope)?;
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
                connection,
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
        StorageLibraryTransaction::diesel_bump_catalog(connection)?;
        request.cancellation.check()?;
        Ok(StorageImportSessionCommitReceipt::new(commit_id, count))
    }
}
