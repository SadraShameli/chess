use std::error::Error;
use std::fs::File;
use std::future::Future;
use std::panic::RefUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::capacity::executor::{
    CapacityExecutor, CapacityExecutorCancellation, CapacityExecutorError,
};
use crate::game::enums::{GameId, GameIdentityError, GameMoveId};
use crate::game::tree::GameTreeError;
use crate::notation::game_identity::{NotationGameIdentity, NotationGameIdentityStructuralKey};
use crate::notation::pgn_import::NotationPgnImportError;
use crate::storage::game_document::StorageGameDocumentError;
use crate::storage::import_session::prepared::StorageImportSessionPreparedCommit;
use crate::storage::library_record::{StorageLibraryRecord, StorageLibraryRecordError};

pub type StorageImportSessionRepositoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, StorageImportSessionRepositoryError>> + Send + 'a>>;

pub struct StorageImportSessionBodyProof<'a> {
    record: &'a StorageLibraryRecord,
    body: &'a str,
    digest: [u8; 32],
    length: u64,
}

impl<'a> StorageImportSessionBodyProof<'a> {
    pub(super) fn new(
        record: &'a StorageLibraryRecord,
        body: &'a str,
        mut check: impl FnMut() -> Result<(), StorageImportSessionError>,
    ) -> Result<Self, StorageImportSessionError> {
        let mut digest = Sha256::new();
        for chunk in body
            .as_bytes()
            .chunks(crate::storage::import_session::disk::StorageImportSessionDisk::COPY_BYTES)
        {
            check()?;
            digest.update(chunk);
        }
        Ok(Self {
            record,
            body,
            digest: digest.finalize().into(),
            length: u64::try_from(body.len())
                .map_err(|_error| StorageImportSessionError::Arithmetic)?,
        })
    }
    pub(crate) fn proves_record(&self, record: &StorageLibraryRecord) -> bool {
        std::ptr::eq(self.record, record)
    }
    pub const fn body(&self) -> &str {
        self.body
    }
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }
    pub const fn byte_length(&self) -> u64 {
        self.length
    }
}

#[derive(Debug, Error)]
pub enum StorageImportSessionRepositoryError {
    #[error("The library catalog changed after preview")]
    StaleCatalog,
    #[error("The import destination changed after preview")]
    StaleDestination,
    #[error("The duplicate candidate changed after it was listed")]
    StaleCandidate,
    #[error("Library adoption is not certified")]
    IncompleteAdoption,
    #[error("The import game identifier is already in use")]
    ExistingGame,
    #[error("The repository returned an invalid or unbounded response")]
    InvalidResponse,
    #[error("The repository operation failed: {0}")]
    Effect(#[source] Box<dyn Error + Send + Sync>),
}

#[derive(Debug, Error)]
pub enum StorageImportSessionError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Executor(#[from] CapacityExecutorError),
    #[error("The import worker result channel closed: {0}")]
    WorkerResult(#[from] tokio::sync::oneshot::error::RecvError),
    #[error(transparent)]
    Retained(Arc<Self>),
    #[error(transparent)]
    Parser(#[from] NotationPgnImportError),
    #[error(transparent)]
    Codec(#[from] StorageGameDocumentError),
    #[error(transparent)]
    Record(#[from] StorageLibraryRecordError),
    #[error(transparent)]
    Identity(#[from] GameIdentityError),
    #[error(transparent)]
    Tree(#[from] GameTreeError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Utf8(#[from] std::str::Utf8Error),
    #[error(transparent)]
    Repository(#[from] StorageImportSessionRepositoryError),
    #[error("Import accounting exceeds its checked range")]
    Arithmetic,
    #[error("The import source exceeds 20 GiB")]
    SourceLimit,
    #[error("The import directory exceeds one million games")]
    DirectoryLimit,
    #[error("Import processing was cancelled")]
    Cancelled,
    #[error("A retained import staging entry was replaced")]
    ReplacedOrphan,
    #[error("Another retained import operation must be observed first")]
    Pending,
    #[error("This import operation is unavailable in the current state")]
    State,
    #[error("Library adoption must finish before importing")]
    IncompleteAdoption,
    #[error("The import has no selected valid games")]
    EmptySelection,
    #[error("A duplicate requires an explicit Keep Copy choice")]
    KeepCopyRequired,
    #[error("The import page exceeds its item or byte budget")]
    PageBudget,
    #[error("The import stage or its private proof changed")]
    Proof,
    #[error("The identity provider repeated a game identifier")]
    RepeatedGame,
    #[error("Import state was poisoned by an interrupted worker")]
    Poisoned,
    #[error("Import requires {required} additional bytes but only {available} are available")]
    Space { required: u64, available: u64 },
    #[error("Import failed ({primary}); cleanup also failed ({cleanup})")]
    Cleanup {
        #[source]
        primary: Arc<Self>,
        cleanup: Arc<Self>,
    },
}

#[derive(Clone)]
pub struct StorageImportSessionCancellation(Arc<StorageImportSessionCancellationState>);

struct StorageImportSessionCancellationState {
    cancelled: AtomicBool,
    changed: tokio::sync::Notify,
}

impl StorageImportSessionCancellation {
    pub(crate) fn new() -> Self {
        Self(Arc::new(StorageImportSessionCancellationState {
            cancelled: AtomicBool::new(false),
            changed: tokio::sync::Notify::new(),
        }))
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    pub(crate) fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        self.0.changed.notify_waiters();
    }

    pub(crate) async fn wait(&self) {
        let changed = self.0.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if !self.is_cancelled() {
            changed.await;
        }
    }

    pub(crate) fn check(&self) -> Result<(), StorageImportSessionError> {
        if self.is_cancelled() {
            Err(StorageImportSessionError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageImportSessionCatalogStamp(u64);

impl StorageImportSessionCatalogStamp {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageImportSessionEnumVolume {
    Stage,
    Library,
}

#[derive(Debug, Clone, Copy)]
pub struct StorageImportSessionSpace {
    volume: u64,
    available: u64,
}

impl StorageImportSessionSpace {
    pub const fn new(volume: u64, available: u64) -> Self {
        Self { volume, available }
    }
    pub const fn volume(self) -> u64 {
        self.volume
    }
    pub const fn available(self) -> u64 {
        self.available
    }
    pub(crate) fn require(self, required: u64) -> Result<(), StorageImportSessionError> {
        if required > self.available {
            Err(StorageImportSessionError::Space {
                required,
                available: self.available,
            })
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageImportSessionDestination {
    folder: Option<uuid::Uuid>,
    revision: u64,
}

impl StorageImportSessionDestination {
    pub const fn unfiled() -> Self {
        Self {
            folder: None,
            revision: 0,
        }
    }
    pub fn folder(folder: uuid::Uuid, revision: u64) -> Result<Self, StorageImportSessionError> {
        if folder.is_nil() {
            return Err(StorageImportSessionError::State);
        }
        Ok(Self {
            folder: Some(folder),
            revision,
        })
    }
    pub const fn folder_id(self) -> Option<uuid::Uuid> {
        self.folder
    }
    pub const fn revision(self) -> u64 {
        self.revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageImportSessionSnapshot {
    stamp: StorageImportSessionCatalogStamp,
    is_adoption_complete: bool,
    destination: StorageImportSessionDestination,
}

impl StorageImportSessionSnapshot {
    pub const fn new(
        stamp: StorageImportSessionCatalogStamp,
        is_adoption_complete: bool,
        destination: StorageImportSessionDestination,
    ) -> Self {
        Self {
            stamp,
            is_adoption_complete,
            destination,
        }
    }
    pub const fn catalog_stamp(self) -> StorageImportSessionCatalogStamp {
        self.stamp
    }
    pub const fn is_adoption_complete(self) -> bool {
        self.is_adoption_complete
    }
    pub const fn destination(self) -> StorageImportSessionDestination {
        self.destination
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageImportSessionCandidate {
    game: GameId,
    revision: u64,
}

impl StorageImportSessionCandidate {
    pub const fn new(game: GameId, revision: u64) -> Self {
        Self { game, revision }
    }
    pub const fn game(self) -> GameId {
        self.game
    }
    pub const fn revision(self) -> u64 {
        self.revision
    }
}

#[derive(Debug, Clone)]
pub struct StorageImportSessionCandidateBody {
    candidate: StorageImportSessionCandidate,
    body: StorageImportSessionCandidateEnumBody,
}

#[derive(Debug, Clone)]
enum StorageImportSessionCandidateEnumBody {
    Owned(Arc<str>),
    Reserved(Arc<StorageImportSessionCandidateReservedBody>),
}

#[derive(Debug)]
struct StorageImportSessionCandidateReservedBody {
    body: String,
    _reservation: tokio::sync::OwnedSemaphorePermit,
}

impl StorageImportSessionCandidateBody {
    pub fn new(
        candidate: StorageImportSessionCandidate,
        body: Arc<str>,
    ) -> Result<Self, StorageImportSessionRepositoryError> {
        if body.len() > crate::storage::document_store::StorageDocumentStore::MEMORY_BUDGET {
            return Err(StorageImportSessionRepositoryError::InvalidResponse);
        }
        Ok(Self {
            candidate,
            body: StorageImportSessionCandidateEnumBody::Owned(body),
        })
    }
    pub(crate) fn from_reserved(
        candidate: StorageImportSessionCandidate,
        body: String,
        reservation: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<Self, StorageImportSessionRepositoryError> {
        let required = body
            .capacity()
            .checked_add(std::mem::size_of::<StorageImportSessionCandidateReservedBody>())
            .and_then(|bytes| bytes.checked_add(2 * std::mem::size_of::<usize>()))
            .ok_or(StorageImportSessionRepositoryError::InvalidResponse)?;
        if reservation.num_permits() < required {
            return Err(StorageImportSessionRepositoryError::InvalidResponse);
        }
        Ok(Self {
            candidate,
            body: StorageImportSessionCandidateEnumBody::Reserved(Arc::new(
                StorageImportSessionCandidateReservedBody {
                    body,
                    _reservation: reservation,
                },
            )),
        })
    }
    pub const fn candidate(&self) -> StorageImportSessionCandidate {
        self.candidate
    }
    pub fn body(&self) -> &str {
        match &self.body {
            StorageImportSessionCandidateEnumBody::Owned(body) => body,
            StorageImportSessionCandidateEnumBody::Reserved(body) => &body.body,
        }
    }
    pub(crate) const fn reserved_overhead() -> usize {
        std::mem::size_of::<StorageImportSessionCandidateReservedBody>()
            + 2 * std::mem::size_of::<usize>()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageImportSessionCandidateCursor {
    after: Option<GameId>,
}

impl StorageImportSessionCandidateCursor {
    pub const fn start() -> Self {
        Self { after: None }
    }
    pub const fn after(game: GameId) -> Self {
        Self { after: Some(game) }
    }
    pub const fn boundary(self) -> Option<GameId> {
        self.after
    }
}

#[derive(Debug, Clone)]
pub struct StorageImportSessionCandidateQuery {
    stamp: StorageImportSessionCatalogStamp,
    identity: Arc<NotationGameIdentity>,
    structural_digest: [u8; 32],
    cursor: StorageImportSessionCandidateCursor,
}

impl StorageImportSessionCandidateQuery {
    pub(crate) fn new(
        stamp: StorageImportSessionCatalogStamp,
        identity: Arc<NotationGameIdentity>,
        cursor: StorageImportSessionCandidateCursor,
    ) -> Result<Self, StorageImportSessionError> {
        Ok(Self {
            stamp,
            structural_digest: Self::structural_digest_for(identity.structural_key())?,
            identity,
            cursor,
        })
    }
    pub fn structural_digest_for(
        key: &NotationGameIdentityStructuralKey,
    ) -> Result<[u8; 32], StorageImportSessionError> {
        Ok(Sha256::digest(serde_json::to_vec(&(
            key.variant(),
            key.initial_fen(),
            key.mainline_uci(),
        ))?)
        .into())
    }
    pub fn structural_key(&self) -> &NotationGameIdentityStructuralKey {
        self.identity.structural_key()
    }
    pub(crate) fn identity_bytes(&self) -> Result<usize, StorageImportSessionError> {
        [self.identity.white(), self.identity.black()]
            .into_iter()
            .flatten()
            .try_fold(std::mem::size_of::<NotationGameIdentity>(), |sum, text| {
                sum.checked_add(text.len())
                    .ok_or(StorageImportSessionError::Arithmetic)
            })
    }
    pub const fn catalog_stamp(&self) -> StorageImportSessionCatalogStamp {
        self.stamp
    }
    pub const fn structural_digest(&self) -> [u8; 32] {
        self.structural_digest
    }
    pub const fn cursor(&self) -> StorageImportSessionCandidateCursor {
        self.cursor
    }
    pub const fn limit(&self) -> usize {
        StorageImportSessionPageRequest::MAX_ITEMS
    }
}

pub struct StorageImportSessionCandidatePage {
    stamp: StorageImportSessionCatalogStamp,
    candidates: Vec<StorageImportSessionCandidate>,
    next: Option<StorageImportSessionCandidateCursor>,
    _reservation: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl StorageImportSessionCandidatePage {
    pub fn new(
        stamp: StorageImportSessionCatalogStamp,
        candidates: Vec<StorageImportSessionCandidate>,
        next: Option<StorageImportSessionCandidateCursor>,
    ) -> Result<Self, StorageImportSessionRepositoryError> {
        if candidates.len() > StorageImportSessionPageRequest::MAX_ITEMS
            || (next.is_some() && candidates.is_empty())
            || candidates.windows(2).any(|pair| {
                pair[0].game().as_uuid().as_bytes() >= pair[1].game().as_uuid().as_bytes()
            })
        {
            return Err(StorageImportSessionRepositoryError::InvalidResponse);
        }
        Ok(Self {
            stamp,
            candidates,
            next,
            _reservation: None,
        })
    }
    pub(crate) fn from_reserved(
        stamp: StorageImportSessionCatalogStamp,
        candidates: Vec<StorageImportSessionCandidate>,
        next: Option<StorageImportSessionCandidateCursor>,
        reservation: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<Self, StorageImportSessionRepositoryError> {
        let mut page = Self::new(stamp, candidates, next)?;
        page._reservation = Some(reservation);
        Ok(page)
    }
    pub const fn catalog_stamp(&self) -> StorageImportSessionCatalogStamp {
        self.stamp
    }
    pub fn candidates(&self) -> &[StorageImportSessionCandidate] {
        &self.candidates
    }
    pub const fn next_cursor(&self) -> Option<StorageImportSessionCandidateCursor> {
        self.next
    }
    pub(crate) fn validate(
        &self,
        query: &StorageImportSessionCandidateQuery,
    ) -> Result<(), StorageImportSessionError> {
        if self.stamp != query.stamp {
            return Err(StorageImportSessionRepositoryError::StaleCatalog.into());
        }
        if self.candidates.first().is_some_and(|first| {
            query.cursor.boundary().is_some_and(|after| {
                first.game().as_uuid().as_bytes() <= after.as_uuid().as_bytes()
            })
        }) || self.next.is_some_and(|next| {
            next.boundary() != self.candidates.last().map(|candidate| candidate.game())
        }) {
            return Err(StorageImportSessionRepositoryError::InvalidResponse.into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageImportSessionEnumChoice {
    Import,
    Skip,
    KeepCopy,
}

impl StorageImportSessionEnumChoice {
    pub(crate) const fn is_selected(self) -> bool {
        matches!(self, Self::Import | Self::KeepCopy)
    }
    pub(crate) const fn code(self) -> u64 {
        match self {
            Self::Import => 1,
            Self::Skip => 2,
            Self::KeepCopy => 3,
        }
    }
    pub(crate) fn from_code(code: u64) -> Result<Self, StorageImportSessionError> {
        match code {
            1 => Ok(Self::Import),
            2 => Ok(Self::Skip),
            3 => Ok(Self::KeepCopy),
            _ => Err(StorageImportSessionError::Proof),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageImportSessionPageRequest {
    offset: u64,
    limit: usize,
    byte_limit: usize,
}

impl StorageImportSessionPageRequest {
    pub const MAX_ITEMS: usize = 100;
    pub const MAX_BYTES: usize = 1024 * 1024;
    pub fn new(
        offset: u64,
        limit: usize,
        byte_limit: usize,
    ) -> Result<Self, StorageImportSessionError> {
        if limit == 0 || limit > Self::MAX_ITEMS || byte_limit == 0 || byte_limit > Self::MAX_BYTES
        {
            return Err(StorageImportSessionError::PageBudget);
        }
        offset
            .checked_add(
                u64::try_from(limit).map_err(|_error| StorageImportSessionError::Arithmetic)?,
            )
            .ok_or(StorageImportSessionError::Arithmetic)?;
        Ok(Self {
            offset,
            limit,
            byte_limit,
        })
    }
    pub const fn offset(self) -> u64 {
        self.offset
    }
    pub const fn limit(self) -> usize {
        self.limit
    }
    pub const fn byte_limit(self) -> usize {
        self.byte_limit
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageImportSessionExclusion {
    pub(crate) ordinal: u64,
    pub(crate) byte_offset: u64,
    pub(crate) line: u64,
    pub(crate) column: u64,
    pub(crate) reason: String,
}

impl StorageImportSessionExclusion {
    pub const fn ordinal(&self) -> u64 {
        self.ordinal
    }
    pub const fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub const fn line(&self) -> u64 {
        self.line
    }
    pub const fn column(&self) -> u64 {
        self.column
    }
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageImportSessionRow {
    pub(crate) ordinal: u64,
    pub(crate) game: Option<GameId>,
    pub(crate) white: String,
    pub(crate) black: String,
    pub(crate) choice: Option<StorageImportSessionEnumChoice>,
    pub(crate) exact: u64,
    pub(crate) potential: u64,
    pub(crate) exclusion: Option<StorageImportSessionExclusion>,
}

impl StorageImportSessionRow {
    pub const fn ordinal(&self) -> u64 {
        self.ordinal
    }
    pub const fn game(&self) -> Option<GameId> {
        self.game
    }
    pub fn white(&self) -> &str {
        &self.white
    }
    pub fn black(&self) -> &str {
        &self.black
    }
    pub const fn choice(&self) -> Option<StorageImportSessionEnumChoice> {
        self.choice
    }
    pub const fn exact_matches(&self) -> u64 {
        self.exact
    }
    pub const fn potential_matches(&self) -> u64 {
        self.potential
    }
    pub fn exclusion(&self) -> Option<&StorageImportSessionExclusion> {
        self.exclusion.as_ref()
    }
}

#[derive(Debug)]
pub struct StorageImportSessionPage {
    pub(crate) rows: Vec<StorageImportSessionRow>,
    pub(crate) next: Option<u64>,
}

impl StorageImportSessionPage {
    pub fn rows(&self) -> &[StorageImportSessionRow] {
        &self.rows
    }
    pub const fn next_cursor(&self) -> Option<u64> {
        self.next
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageImportSessionSummary {
    pub(crate) valid: u64,
    pub(crate) excluded: u64,
    pub(crate) duplicates: u64,
    pub(crate) selected: u64,
}

impl StorageImportSessionSummary {
    pub const fn valid(self) -> u64 {
        self.valid
    }
    pub const fn excluded(self) -> u64 {
        self.excluded
    }
    pub const fn duplicates(self) -> u64 {
        self.duplicates
    }
    pub const fn selected(self) -> u64 {
        self.selected
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StorageImportSessionCommitFootprint {
    pub(crate) count: u64,
    pub(crate) bodies: u64,
    pub(crate) projections: u64,
    pub(crate) largest: u64,
}

impl StorageImportSessionCommitFootprint {
    pub const fn count(self) -> u64 {
        self.count
    }
    pub const fn body_bytes(self) -> u64 {
        self.bodies
    }
    pub const fn projection_bytes(self) -> u64 {
        self.projections
    }
    pub const fn largest_frame_bytes(self) -> u64 {
        self.largest
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageImportSessionConfirmation {
    pub(crate) session: GameId,
    pub(crate) revision: u64,
    pub(crate) digest: [u8; 32],
    pub(crate) snapshot: StorageImportSessionSnapshot,
    pub(crate) summary: StorageImportSessionSummary,
    pub(crate) source_bytes: u64,
    pub(crate) source_digest: [u8; 32],
    pub(crate) stage_bytes: u64,
    pub(crate) commit_bytes: u64,
    pub(crate) sqlite_bytes: u64,
}

impl StorageImportSessionConfirmation {
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }
    pub const fn catalog_stamp(&self) -> StorageImportSessionCatalogStamp {
        self.snapshot.catalog_stamp()
    }
    pub const fn summary(&self) -> StorageImportSessionSummary {
        self.summary
    }
    pub const fn source_bytes(&self) -> u64 {
        self.source_bytes
    }
    pub const fn source_digest(&self) -> [u8; 32] {
        self.source_digest
    }
    pub const fn stage_bytes(&self) -> u64 {
        self.stage_bytes
    }
    pub const fn commit_bytes(&self) -> u64 {
        self.commit_bytes
    }
    pub const fn sqlite_bytes(&self) -> u64 {
        self.sqlite_bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageImportSessionCommitReceipt {
    commit: GameId,
    count: u64,
}

impl StorageImportSessionCommitReceipt {
    pub const fn new(commit: GameId, count: u64) -> Self {
        Self { commit, count }
    }
    pub const fn commit_id(self) -> GameId {
        self.commit
    }
    pub const fn count(self) -> u64 {
        self.count
    }
}

#[derive(Debug)]
pub enum StorageImportSessionEnumCommitOutcome {
    Committed(StorageImportSessionCommitReceipt),
    CommittedCleanupFailed {
        receipt: StorageImportSessionCommitReceipt,
        cleanup: Arc<StorageImportSessionError>,
    },
}

#[derive(Debug)]
pub enum StorageImportSessionEnumCancelOutcome {
    Cancelled,
    Committed(StorageImportSessionCommitReceipt),
}

pub trait StorageImportSessionIdentityProvider: Send + Sync + RefUnwindSafe + 'static {
    fn next_game(&self) -> Result<GameId, GameIdentityError>;
    fn next_move(&self) -> Result<GameMoveId, GameIdentityError>;
}

pub trait StorageImportSessionFilesystemProvider: Send + Sync + RefUnwindSafe + 'static {
    fn create_spool(&self) -> Result<File, StorageImportSessionError>;
    fn duplicate(&self, file: &File) -> std::io::Result<File>;
    fn observe_space(
        &self,
        volume: StorageImportSessionEnumVolume,
    ) -> Result<StorageImportSessionSpace, StorageImportSessionError>;
    fn read_at(&self, file: &mut File, offset: u64, bytes: &mut [u8]) -> std::io::Result<usize>;
    fn write_at(&self, file: &mut File, offset: u64, bytes: &[u8]) -> std::io::Result<()>;
    fn length(&self, file: &File) -> std::io::Result<u64>;
    fn flush(&self, file: &mut File) -> std::io::Result<()>;
    fn release(&self, file: &mut File) -> std::io::Result<()>;
    fn cleanup_retained(&self) -> Result<(), StorageImportSessionError>;
}

pub trait StorageImportSessionRepository: Send + Sync + RefUnwindSafe + 'static {
    type PreparedProjection: AsRef<[u8]>;
    fn snapshot(&self) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionSnapshot>;
    fn candidates(
        &self,
        query: StorageImportSessionCandidateQuery,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCandidatePage>;
    fn read_candidate(
        &self,
        candidate: StorageImportSessionCandidate,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCandidateBody>;
    fn prepare_projection(
        &self,
        record: &StorageLibraryRecord,
        body: StorageImportSessionBodyProof<'_>,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Self::PreparedProjection, StorageImportSessionRepositoryError>;
    fn estimate_commit_space(
        &self,
        snapshot: StorageImportSessionSnapshot,
        footprint: StorageImportSessionCommitFootprint,
    ) -> StorageImportSessionRepositoryFuture<'_, u64>;
    fn commit_import(
        &self,
        prepared: StorageImportSessionPreparedCommit,
        cancellation: StorageImportSessionCancellation,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCommitReceipt>;
}

pub struct StorageImportSessionResources<R, F, I> {
    pub executor: Arc<CapacityExecutor>,
    pub repository: Arc<R>,
    pub filesystem: Arc<F>,
    pub identities: Arc<I>,
}
