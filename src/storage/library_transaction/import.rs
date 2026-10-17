use std::sync::Arc;

use diesel::connection::DefaultLoadingMode;
use diesel::dsl::{case_when, sql};
use diesel::sql_types::BigInt;
use diesel::sqlite::SqliteConnection;
use diesel::{
    ExpressionMethods, JoinOnDsl, NullableExpressionMethods, OptionalExtension, QueryDsl,
    Queryable, RunQueryDsl,
};
use sha2::{Digest, Sha256};
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
use crate::storage::schema::{documents, library_entries, library_folders};

#[derive(Queryable)]
struct StorageLibraryTransactionImportCandidateMetadata {
    actual: Option<String>,
    projected: Option<String>,
    bytes: i64,
    digest: Option<Vec<u8>>,
}

pub(super) struct StorageLibraryTransactionImport;

impl StorageLibraryTransactionImport {
    pub(super) fn snapshot(
        transaction: &mut SqliteConnection,
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
        transaction: &mut SqliteConnection,
        destination: StorageImportSessionDestination,
    ) -> Result<(), StorageLibraryError> {
        if let Some(folder) = destination.folder_id() {
            let revision: Option<String> = library_folders::table
                .find(folder.to_string())
                .select(library_folders::revision)
                .first(transaction)
                .optional()?;
            if revision.as_deref().map(str::parse::<crate::game::library_organization::GameLibraryOrganizationRevision>).transpose()?.map(|revision| revision.value()) != Some(destination.revision()) { return Err(StorageLibraryError::ImportDestination); }
        } else if destination.revision() != 0 {
            return Err(StorageLibraryError::ImportDestination);
        }
        Ok(())
    }

    fn validate(
        transaction: &mut SqliteConnection,
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
        transaction: &mut SqliteConnection,
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
        let key = query.structural_key();
        let after = query
            .cursor()
            .boundary()
            .map(|game| game.as_uuid().to_string())
            .unwrap_or_default();
        let rows = library_entries::table
            .inner_join(documents::table.on(documents::id.eq(library_entries::game_id)))
            .filter(library_entries::content_revision.eq(documents::revision))
            .filter(
                library_entries::rules_variant
                    .eq(StorageLibraryProjection::variant_sql(key.variant())),
            )
            .filter(library_entries::identity_initial_fen.eq(key.initial_fen()))
            .filter(library_entries::plies.eq(i64::try_from(key.mainline_uci().len())?))
            .filter(library_entries::identity_mainline_uci.eq(request.mainline()))
            .filter(library_entries::game_id.gt(after))
            .order(library_entries::game_id.asc())
            .limit(i64::try_from(limit + 1)?)
            .select((library_entries::game_id, documents::revision))
            .load_iter::<(String, String), DefaultLoadingMode>(transaction)?;
        let mut candidates = Vec::with_capacity(limit + 1);
        for row in rows {
            let (id, revision) = row?;
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
        transaction: &mut SqliteConnection,
        candidate: StorageImportSessionCandidate,
        memory: &Arc<Semaphore>,
    ) -> Result<StorageImportSessionCandidateBody, StorageLibraryError> {
        let key = candidate.game().as_uuid().to_string();
        let maximum_digits = i64::try_from(u64::MAX.to_string().len())?;
        let metadata = documents::table
            .inner_join(library_entries::table.on(library_entries::game_id.eq(documents::id)))
            .filter(documents::id.eq(&key))
            .select((
                case_when(
                    sql::<BigInt>("length(CAST(documents.revision AS BLOB))").le(maximum_digits),
                    documents::revision.nullable(),
                )
                .otherwise(None::<String>),
                case_when(
                    sql::<BigInt>("length(CAST(library_entries.content_revision AS BLOB))")
                        .le(maximum_digits),
                    library_entries::content_revision.nullable(),
                )
                .otherwise(None::<String>),
                sql::<BigInt>("length(CAST(documents.body AS BLOB))"),
                case_when(
                    sql::<BigInt>("length(library_entries.content_sha256)")
                        .eq(i64::try_from(Sha256::output_size())?),
                    library_entries::content_sha256.nullable(),
                )
                .otherwise(None::<Vec<u8>>),
            ))
            .first::<StorageLibraryTransactionImportCandidateMetadata>(transaction)
            .optional()?
            .ok_or(StorageLibraryError::ImportCandidate)?;
        let actual = metadata
            .actual
            .ok_or(StorageLibraryError::StoredDiscriminant)?;
        let projected = metadata
            .projected
            .ok_or(StorageLibraryError::StoredDiscriminant)?;
        if StorageLibraryError::content_revision(&actual)?.value() != candidate.revision()
            || projected != actual
        {
            return Err(StorageLibraryError::ImportCandidate);
        }
        let digest = metadata
            .digest
            .ok_or(StorageLibraryError::ProjectionStale)?;
        let bytes = usize::try_from(metadata.bytes)?
            .checked_add(StorageImportSessionCandidateBody::reserved_overhead())
            .ok_or(StorageLibraryError::TextTooLarge)?;
        let reservation = StorageDocumentClient::reserve_memory(memory, bytes)?;
        let body: String = documents::table
            .find(&key)
            .select(documents::body)
            .first(transaction)?;
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
        transaction: &mut SqliteConnection,
        snapshot: StorageImportSessionSnapshot,
        footprint: StorageImportSessionCommitFootprint,
        certified: bool,
    ) -> Result<u64, StorageLibraryError> {
        Self::validate(transaction, snapshot, certified)?;
        Ok(StorageLibraryImportSpace::plan(transaction, footprint)?.bytes())
    }

    pub(super) fn commit(
        transaction: &mut SqliteConnection,
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
