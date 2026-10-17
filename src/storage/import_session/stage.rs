use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::capacity::executor::CapacityExecutorCancellation;
use crate::game::enums::GameId;
use crate::notation::game_identity::{NotationGameIdentity, NotationGameIdentityEnumMatch};
use crate::notation::pgn_import::{NotationPgnImport, NotationPgnImportEnumRecovery};
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::game_document::StorageGameDocument;
use crate::storage::import_session::disk::{
    StorageImportSessionDisk, StorageImportSessionDiskEnumFile, StorageImportSessionDiskReader,
    StorageImportSessionDiskRecord, StorageImportSessionDiskSpan,
};
use crate::storage::import_session::index::{
    StorageImportSessionIndex, StorageImportSessionIndexBudget, StorageImportSessionIndexEntry,
};
use crate::storage::import_session::model::{
    StorageImportSessionCancellation, StorageImportSessionCandidateBody,
    StorageImportSessionCandidateCursor, StorageImportSessionCandidateQuery,
    StorageImportSessionCommitFootprint, StorageImportSessionConfirmation,
    StorageImportSessionEnumChoice, StorageImportSessionEnumVolume, StorageImportSessionError,
    StorageImportSessionExclusion, StorageImportSessionIdentityProvider, StorageImportSessionPage,
    StorageImportSessionPageRequest, StorageImportSessionRepository,
    StorageImportSessionRepositoryError, StorageImportSessionRow, StorageImportSessionSnapshot,
    StorageImportSessionSummary,
};
use crate::storage::import_session::prepared::{
    StorageImportSessionPreparedCommit, StorageImportSessionPreparedOptions,
};
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};

pub(crate) struct StorageImportSessionStageParseOptions<'a, R, I> {
    pub(crate) source: &'a mut File,
    pub(crate) repository: &'a R,
    pub(crate) identities: &'a I,
}

pub(crate) struct StorageImportSessionStage {
    pub(crate) disk: StorageImportSessionDisk,
    pub(crate) snapshot: Option<StorageImportSessionSnapshot>,
    pub(crate) records: u64,
    pub(crate) revision: u64,
    pub(crate) summary: StorageImportSessionSummary,
    pub(crate) frozen: Option<StorageImportSessionConfirmation>,
    source_length: u64,
    source_digest: [u8; 32],
    original: Sha256,
    index_roots: Option<([u8; 32], [u8; 32])>,
    preview_digest: Option<[u8; 32]>,
    active_identity: Option<(u64, Arc<NotationGameIdentity>)>,
    candidate_digest: Sha256,
    last_candidate: Option<GameId>,
    stream_digest: Option<[u8; 32]>,
    footprint: Option<StorageImportSessionCommitFootprint>,
}

#[cfg(test)]
#[path = "../../../tests/storage/import_session.rs"]
mod tests;

impl StorageImportSessionStage {
    const SOURCE_LIMIT: u64 = 20 * 1024 * 1024 * 1024;
    const DIRECTORY_LIMIT: u64 = 1_000_000;
    const METADATA_LIMIT: usize = 16 * 1024;
    const DISPLAY_BYTES: usize = 256;
    const EXCLUSION_BYTES: usize = 4096;
    const PROOF_FORMAT: &'static [u8] = b"chess.pgn.import.v1";

    pub(crate) fn new(disk: StorageImportSessionDisk) -> Self {
        Self {
            disk,
            snapshot: None,
            records: 0,
            revision: 0,
            summary: StorageImportSessionSummary {
                valid: 0,
                excluded: 0,
                duplicates: 0,
                selected: 0,
            },
            frozen: None,
            source_length: 0,
            source_digest: [0; 32],
            original: Sha256::new(),
            index_roots: None,
            preview_digest: None,
            active_identity: None,
            candidate_digest: Sha256::new(),
            last_candidate: None,
            stream_digest: None,
            footprint: None,
        }
    }

    pub(crate) fn check(
        token: &CapacityExecutorCancellation,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<(), StorageImportSessionError> {
        cancellation.check()?;
        if token.is_cancelled() {
            Err(StorageImportSessionError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn clipped(text: &str, bytes: usize) -> String {
        let mut end = text.len().min(bytes);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text[..end].to_owned()
    }

    fn append_record(
        &mut self,
        mut record: StorageImportSessionDiskRecord,
        row: StorageImportSessionRow,
    ) -> Result<(), StorageImportSessionError> {
        let metadata = serde_json::to_vec(&row)?;
        if metadata.len() > Self::METADATA_LIMIT {
            return Err(StorageImportSessionError::PageBudget);
        }
        record.metadata = self
            .disk
            .append(StorageImportSessionDiskEnumFile::Metadata, &metadata)?;
        record.metadata_digest = Sha256::digest(&metadata).into();
        self.original.update(record.immutable_bytes());
        self.disk.append(
            StorageImportSessionDiskEnumFile::Directory,
            &record.encoded(),
        )?;
        if let Some(game) = record.game {
            self.disk.append(
                StorageImportSessionDiskEnumFile::StructuralIndex,
                &StorageImportSessionIndexEntry {
                    key: record.structural_digest,
                    ordinal: record.ordinal,
                }
                .encoded(),
            )?;
            let mut key = [0; 32];
            key[..16].copy_from_slice(game.as_uuid().as_bytes());
            self.disk.append(
                StorageImportSessionDiskEnumFile::IdentityIndex,
                &StorageImportSessionIndexEntry {
                    key,
                    ordinal: record.ordinal,
                }
                .encoded(),
            )?;
        }
        self.records = record.ordinal;
        Ok(())
    }

    pub(crate) fn parse<
        R: StorageImportSessionRepository,
        I: StorageImportSessionIdentityProvider,
    >(
        &mut self,
        options: StorageImportSessionStageParseOptions<'_, R, I>,
        token: &CapacityExecutorCancellation,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<(), StorageImportSessionError> {
        let StorageImportSessionStageParseOptions {
            source,
            repository,
            identities: ids,
        } = options;
        Self::check(token, cancellation)?;
        let fs = Arc::clone(&self.disk.filesystem);
        let expected = fs.length(source)?;
        if expected > Self::SOURCE_LIMIT {
            return Err(StorageImportSessionError::SourceLimit);
        }
        fs.observe_space(StorageImportSessionEnumVolume::Stage)?
            .require(expected)?;
        self.disk.initialize()?;
        let mut buffer = vec![0; StorageImportSessionDisk::COPY_BYTES];
        let mut source_hash = Sha256::new();
        loop {
            Self::check(token, cancellation)?;
            let count = fs.read_at(source, self.source_length, &mut buffer)?;
            if count == 0 {
                break;
            }
            if count > buffer.len() {
                return Err(StorageImportSessionError::Proof);
            }
            let end = self
                .source_length
                .checked_add(
                    u64::try_from(count).map_err(|_error| StorageImportSessionError::Arithmetic)?,
                )
                .ok_or(StorageImportSessionError::Arithmetic)?;
            if end > Self::SOURCE_LIMIT {
                return Err(StorageImportSessionError::SourceLimit);
            }
            self.disk
                .append(StorageImportSessionDiskEnumFile::Source, &buffer[..count])?;
            source_hash.update(&buffer[..count]);
            self.source_length = end;
        }
        self.source_digest = source_hash.finalize().into();
        drop(buffer);
        let mut copied = fs.duplicate(
            &self
                .disk
                .spool(StorageImportSessionDiskEnumFile::Source)?
                .file,
        )?;
        let reader = StorageImportSessionDiskReader {
            file: &mut copied,
            filesystem: fs.as_ref(),
            offset: 0,
        };
        let mut parser = NotationPgnImport::new(
            BufReader::with_capacity(StorageImportSessionDisk::COPY_BYTES, reader),
            || ids.next_move(),
        );
        loop {
            Self::check(token, cancellation)?;
            let next = parser.next_game(&|| cancellation.is_cancelled() || token.is_cancelled());
            if matches!(next, Ok(None)) {
                break;
            }
            let ordinal = self
                .records
                .checked_add(1)
                .filter(|ordinal| *ordinal <= Self::DIRECTORY_LIMIT)
                .ok_or(StorageImportSessionError::DirectoryLimit)?;
            let mut record = StorageImportSessionDiskRecord {
                ordinal,
                game: None,
                choice: None,
                body: StorageImportSessionDiskSpan::EMPTY,
                projection: StorageImportSessionDiskSpan::EMPTY,
                metadata: StorageImportSessionDiskSpan::EMPTY,
                exact: 0,
                potential: 0,
                body_digest: [0; 32],
                projection_digest: [0; 32],
                metadata_digest: [0; 32],
                structural_digest: [0; 32],
                candidate_digest: [0; 32],
            };
            let row = match next {
                Ok(Some(game)) => {
                    let id = ids.next_game()?;
                    let library_record = StorageLibraryRecord::new(
                        id,
                        StorageLibraryEnumContent::Notation(Box::new(game)),
                    )?;
                    let identity = library_record.identity()?;
                    record.structural_digest =
                        StorageImportSessionCandidateQuery::structural_digest_for(
                            identity.structural_key(),
                        )?;
                    drop(identity);
                    let StorageLibraryEnumContent::Notation(game) = library_record.content() else {
                        return Err(StorageImportSessionError::State);
                    };
                    let body = StorageGameDocument::capture_notation_cancellable(id, game, token)?
                        .json_cancellable(token)?;
                    Self::check(token, cancellation)?;
                    let body_proof =
                        crate::storage::import_session::model::StorageImportSessionBodyProof::new(
                            &library_record,
                            &body,
                            || Self::check(token, cancellation),
                        )?;
                    record.body_digest = body_proof.digest();
                    let projection =
                        repository.prepare_projection(&library_record, body_proof, token)?;
                    if projection.as_ref().len()
                        > StorageImportSessionPreparedCommit::MAX_PROJECTION_BYTES
                    {
                        return Err(StorageImportSessionError::PageBudget);
                    }
                    record.projection_digest = Sha256::digest(projection.as_ref()).into();
                    record.body = self
                        .disk
                        .append(StorageImportSessionDiskEnumFile::Bodies, body.as_bytes())?;
                    record.projection = self.disk.append(
                        StorageImportSessionDiskEnumFile::Projections,
                        projection.as_ref(),
                    )?;
                    record.game = Some(id);
                    record.choice = Some(StorageImportSessionEnumChoice::Import);
                    StorageImportSessionRow {
                        ordinal,
                        game: Some(id),
                        white: Self::clipped(
                            game.metadata()
                                .known()
                                .white
                                .as_ref()
                                .map_or("", |name| name.as_str()),
                            Self::DISPLAY_BYTES,
                        ),
                        black: Self::clipped(
                            game.metadata()
                                .known()
                                .black
                                .as_ref()
                                .map_or("", |name| name.as_str()),
                            Self::DISPLAY_BYTES,
                        ),
                        choice: record.choice,
                        exact: 0,
                        potential: 0,
                        exclusion: None,
                    }
                }
                Err(error) if error.recovery() == NotationPgnImportEnumRecovery::NextGame => {
                    let location = error.location();
                    StorageImportSessionRow {
                        ordinal,
                        game: None,
                        white: String::new(),
                        black: String::new(),
                        choice: None,
                        exact: 0,
                        potential: 0,
                        exclusion: Some(StorageImportSessionExclusion {
                            ordinal,
                            byte_offset: location.byte_offset(),
                            line: location.line(),
                            column: location.column(),
                            reason: Self::clipped(&error.to_string(), Self::EXCLUSION_BYTES),
                        }),
                    }
                }
                Err(error) => return Err(error.into()),
                Ok(None) => return Err(StorageImportSessionError::State),
            };
            self.append_record(record, row)?;
        }
        Self::check(token, cancellation)?;
        for kind in [
            StorageImportSessionDiskEnumFile::StructuralIndex,
            StorageImportSessionDiskEnumFile::IdentityIndex,
        ] {
            StorageImportSessionIndex::sort(
                &mut self.disk,
                kind,
                StorageImportSessionIndexBudget::new(StorageImportSessionIndexBudget::MAX_BYTES)?,
                || Self::check(token, cancellation),
            )?;
        }
        let mut last = None;
        for index in 0..StorageImportSessionIndex::count(
            &self.disk,
            StorageImportSessionDiskEnumFile::IdentityIndex,
        )? {
            Self::check(token, cancellation)?;
            let entry = StorageImportSessionIndex::entry(
                &mut self.disk,
                StorageImportSessionDiskEnumFile::IdentityIndex,
                index,
            )?;
            if last == Some(entry.key) {
                return Err(StorageImportSessionError::RepeatedGame);
            }
            last = Some(entry.key);
        }
        self.index_roots = Some((
            self.disk
                .digest_file(StorageImportSessionDiskEnumFile::StructuralIndex, || {
                    Self::check(token, cancellation)
                })?,
            self.disk
                .digest_file(StorageImportSessionDiskEnumFile::IdentityIndex, || {
                    Self::check(token, cancellation)
                })?,
        ));
        self.compare_staged(token, cancellation)
    }

    pub(crate) fn read_record(
        &mut self,
        ordinal: u64,
        token: &CapacityExecutorCancellation,
    ) -> Result<StorageLibraryRecord, StorageImportSessionError> {
        let record = self.disk.record(ordinal)?;
        let bytes = self.disk.bytes(
            StorageImportSessionDiskEnumFile::Bodies,
            record.body,
            StorageDocumentStore::MEMORY_BUDGET,
            record.body_digest,
        )?;
        let restored =
            StorageGameDocument::recover_record_cancellable(std::str::from_utf8(&bytes)?, token)?;
        if Some(restored.id()) != record.game {
            return Err(StorageImportSessionError::Proof);
        }
        Ok(restored)
    }

    fn count_match(
        record: &mut StorageImportSessionDiskRecord,
        relation: NotationGameIdentityEnumMatch,
        weight: u64,
    ) -> Result<(), StorageImportSessionError> {
        let count = match relation {
            NotationGameIdentityEnumMatch::Exact => &mut record.exact,
            NotationGameIdentityEnumMatch::Potential => &mut record.potential,
            NotationGameIdentityEnumMatch::Different => return Ok(()),
        };
        *count = count
            .checked_add(weight)
            .ok_or(StorageImportSessionError::Arithmetic)?;
        Ok(())
    }

    fn compare_staged(
        &mut self,
        token: &CapacityExecutorCancellation,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<(), StorageImportSessionError> {
        self.build_classes(token, cancellation)?;
        self.assign_complete_classes(token, cancellation)?;
        let mut active: Option<(u64, NotationGameIdentity)> = None;
        for ordinal in 1..=self.records {
            Self::check(token, cancellation)?;
            let mut directory = self.disk.record(ordinal)?;
            if directory.game.is_none() {
                continue;
            }
            let (representative, _) = self.class_member(ordinal)?;
            if active
                .as_ref()
                .is_none_or(|(id, _identity)| *id != representative)
            {
                active = Some((
                    representative,
                    self.read_record(representative, token)?.identity()?,
                ));
            }
            let (_, identity) = active.as_ref().ok_or(StorageImportSessionError::State)?;
            let relation_to_self = identity.compare(identity);
            let kind = match relation_to_self {
                NotationGameIdentityEnumMatch::Exact => {
                    StorageImportSessionDiskEnumFile::IncompleteIndex
                }
                NotationGameIdentityEnumMatch::Potential => {
                    StorageImportSessionDiskEnumFile::ClassIndex
                }
                NotationGameIdentityEnumMatch::Different => {
                    return Err(StorageImportSessionError::Proof);
                }
            };
            let (begin, end) = StorageImportSessionIndex::bucket_in(
                &mut self.disk,
                kind,
                directory.structural_digest,
            )?;
            directory.exact = 0;
            directory.potential = 0;
            if relation_to_self == NotationGameIdentityEnumMatch::Exact {
                let (_, prior) = self.class_member(representative)?;
                Self::count_match(&mut directory, relation_to_self, prior)?;
            }
            for index in begin..end {
                Self::check(token, cancellation)?;
                let candidate = StorageImportSessionIndex::entry(&mut self.disk, kind, index)?;
                if candidate.ordinal >= ordinal {
                    break;
                }
                let (_, count) = self.class_member(candidate.ordinal)?;
                if count == 0 {
                    continue;
                }
                let relation = if candidate.ordinal == representative {
                    identity.compare(identity)
                } else {
                    identity.compare(&self.read_record(candidate.ordinal, token)?.identity()?)
                };
                Self::count_match(&mut directory, relation, count)?;
            }
            let (_, count) = self.class_member(representative)?;
            self.write_class_member(
                representative,
                representative,
                count
                    .checked_add(1)
                    .ok_or(StorageImportSessionError::Arithmetic)?,
            )?;
            self.disk.write_record(&directory)?;
        }
        Ok(())
    }

    pub(crate) fn query(
        &mut self,
        ordinal: u64,
        cursor: StorageImportSessionCandidateCursor,
        token: &CapacityExecutorCancellation,
    ) -> Result<Option<StorageImportSessionCandidateQuery>, StorageImportSessionError> {
        let record = self.disk.record(ordinal)?;
        if record.game.is_none() {
            return Ok(None);
        }
        if cursor.boundary().is_none() {
            self.active_identity = Some((
                ordinal,
                Arc::new(self.read_record(ordinal, token)?.identity()?),
            ));
            self.candidate_digest = Sha256::new();
            self.last_candidate = None;
        }
        let (active, identity) = self
            .active_identity
            .as_ref()
            .ok_or(StorageImportSessionError::State)?;
        if *active != ordinal {
            return Err(StorageImportSessionError::State);
        }
        Ok(Some(StorageImportSessionCandidateQuery::new(
            self.snapshot
                .ok_or(StorageImportSessionError::State)?
                .catalog_stamp(),
            Arc::clone(identity),
            cursor,
        )?))
    }

    pub(crate) fn candidate(
        &mut self,
        ordinal: u64,
        body: StorageImportSessionCandidateBody,
        token: &CapacityExecutorCancellation,
    ) -> Result<(), StorageImportSessionError> {
        let reference = body.candidate();
        if self
            .last_candidate
            .is_some_and(|last| last.as_uuid().as_bytes() >= reference.game().as_uuid().as_bytes())
        {
            return Err(StorageImportSessionRepositoryError::InvalidResponse.into());
        }
        let candidate = StorageGameDocument::recover_record_cancellable(body.body(), token)?;
        if candidate.id() != reference.game() {
            return Err(StorageImportSessionRepositoryError::InvalidResponse.into());
        }
        let (active, identity) = self
            .active_identity
            .as_ref()
            .ok_or(StorageImportSessionError::State)?;
        if *active != ordinal {
            return Err(StorageImportSessionError::State);
        }
        let relation = identity.compare(&candidate.identity()?);
        let mut directory = self.disk.record(ordinal)?;
        Self::count_match(&mut directory, relation, 1)?;
        self.disk.write_record(&directory)?;
        self.candidate_digest
            .update(reference.game().as_uuid().as_bytes());
        self.candidate_digest
            .update(reference.revision().to_le_bytes());
        self.candidate_digest
            .update(Sha256::digest(body.body().as_bytes()));
        self.last_candidate = Some(reference.game());
        Ok(())
    }

    pub(crate) fn finish_candidates(
        &mut self,
        ordinal: u64,
    ) -> Result<(), StorageImportSessionError> {
        let mut record = self.disk.record(ordinal)?;
        record.candidate_digest = self.candidate_digest.clone().finalize().into();
        record.choice = Some(if record.is_duplicate() {
            StorageImportSessionEnumChoice::Skip
        } else {
            StorageImportSessionEnumChoice::Import
        });
        self.disk.write_record(&record)?;
        self.active_identity = None;
        Ok(())
    }

    pub(crate) fn finish_preview(
        &mut self,
        token: &CapacityExecutorCancellation,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<StorageImportSessionSummary, StorageImportSessionError> {
        self.summarize(token, cancellation)?;
        self.preview_digest = Some(
            self.disk
                .digest_file(StorageImportSessionDiskEnumFile::Directory, || {
                    Self::check(token, cancellation)
                })?,
        );
        Ok(self.summary)
    }

    fn summarize(
        &mut self,
        token: &CapacityExecutorCancellation,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<(), StorageImportSessionError> {
        let mut summary = StorageImportSessionSummary {
            valid: 0,
            excluded: 0,
            duplicates: 0,
            selected: 0,
        };
        for ordinal in 1..=self.records {
            Self::check(token, cancellation)?;
            let record = self.disk.record(ordinal)?;
            if record.game.is_some() {
                summary.valid = summary
                    .valid
                    .checked_add(1)
                    .ok_or(StorageImportSessionError::Arithmetic)?;
                if record.is_duplicate() {
                    summary.duplicates = summary
                        .duplicates
                        .checked_add(1)
                        .ok_or(StorageImportSessionError::Arithmetic)?;
                }
                if record
                    .choice
                    .is_some_and(StorageImportSessionEnumChoice::is_selected)
                {
                    summary.selected = summary
                        .selected
                        .checked_add(1)
                        .ok_or(StorageImportSessionError::Arithmetic)?;
                }
            } else {
                summary.excluded = summary
                    .excluded
                    .checked_add(1)
                    .ok_or(StorageImportSessionError::Arithmetic)?;
            }
        }
        self.summary = summary;
        Ok(())
    }

    pub(crate) fn page(
        &mut self,
        request: StorageImportSessionPageRequest,
    ) -> Result<StorageImportSessionPage, StorageImportSessionError> {
        let mut rows = Vec::with_capacity(request.limit());
        let mut used = 2_usize;
        let mut offset = request.offset();
        while offset < self.records && rows.len() < request.limit() {
            let ordinal = offset
                .checked_add(1)
                .ok_or(StorageImportSessionError::Arithmetic)?;
            let directory = self.disk.record(ordinal)?;
            let bytes = self.disk.bytes(
                StorageImportSessionDiskEnumFile::Metadata,
                directory.metadata,
                Self::METADATA_LIMIT,
                directory.metadata_digest,
            )?;
            let mut row: StorageImportSessionRow = serde_json::from_slice(&bytes)?;
            if row.ordinal != ordinal || row.game != directory.game {
                return Err(StorageImportSessionError::Proof);
            }
            row.choice = directory.choice;
            row.exact = directory.exact;
            row.potential = directory.potential;
            let size = serde_json::to_vec(&row)?
                .len()
                .checked_add(1)
                .ok_or(StorageImportSessionError::Arithmetic)?;
            let next = used
                .checked_add(size)
                .ok_or(StorageImportSessionError::Arithmetic)?;
            if next > request.byte_limit() {
                if rows.is_empty() {
                    return Err(StorageImportSessionError::PageBudget);
                }
                break;
            }
            rows.push(row);
            used = next;
            offset = ordinal;
        }
        Ok(StorageImportSessionPage {
            rows,
            next: (offset < self.records).then_some(offset),
        })
    }

    pub(crate) fn choose(
        &mut self,
        ordinal: u64,
        choice: StorageImportSessionEnumChoice,
        token: &CapacityExecutorCancellation,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<StorageImportSessionSummary, StorageImportSessionError> {
        if self.preview_digest
            != Some(
                self.disk
                    .digest_file(StorageImportSessionDiskEnumFile::Directory, || {
                        Self::check(token, cancellation)
                    })?,
            )
        {
            return Err(StorageImportSessionError::Proof);
        }
        let mut record = self.disk.record(ordinal)?;
        if record.game.is_none() {
            return Err(StorageImportSessionError::State);
        }
        if record.is_duplicate() && choice == StorageImportSessionEnumChoice::Import {
            return Err(StorageImportSessionError::KeepCopyRequired);
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(StorageImportSessionError::Arithmetic)?;
        self.frozen = None;
        self.stream_digest = None;
        self.footprint = None;
        record.choice = Some(choice);
        self.disk.write_record(&record)?;
        self.revision = revision;
        let fs = Arc::clone(&self.disk.filesystem);
        let commit = self.disk.spool(StorageImportSessionDiskEnumFile::Commit)?;
        fs.release(&mut commit.file)?;
        commit.length = 0;
        self.finish_preview(token, cancellation)
    }

    pub(crate) fn footprint(
        &mut self,
        token: &CapacityExecutorCancellation,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<StorageImportSessionCommitFootprint, StorageImportSessionError> {
        let mut footprint = StorageImportSessionCommitFootprint {
            count: 0,
            bodies: 0,
            projections: 0,
            largest: 0,
        };
        let header_length = StorageImportSessionPreparedCommit::header_length()?;
        for ordinal in 1..=self.records {
            Self::check(token, cancellation)?;
            let record = self.disk.record(ordinal)?;
            if record
                .choice
                .is_some_and(StorageImportSessionEnumChoice::is_selected)
            {
                footprint.count = footprint
                    .count
                    .checked_add(1)
                    .ok_or(StorageImportSessionError::Arithmetic)?;
                footprint.bodies = footprint
                    .bodies
                    .checked_add(record.body.length)
                    .ok_or(StorageImportSessionError::Arithmetic)?;
                footprint.projections = footprint
                    .projections
                    .checked_add(record.projection.length)
                    .ok_or(StorageImportSessionError::Arithmetic)?;
                footprint.largest = footprint.largest.max(
                    record
                        .body
                        .length
                        .checked_add(record.projection.length)
                        .and_then(|value| value.checked_add(header_length))
                        .ok_or(StorageImportSessionError::Arithmetic)?,
                );
            }
        }
        if footprint.count == 0 {
            return Err(StorageImportSessionError::EmptySelection);
        }
        Ok(footprint)
    }

    pub(crate) fn require_space(
        &self,
        additional_stage: u64,
        sqlite: u64,
    ) -> Result<(), StorageImportSessionError> {
        let stage = self
            .disk
            .filesystem
            .observe_space(StorageImportSessionEnumVolume::Stage)?;
        let library = self
            .disk
            .filesystem
            .observe_space(StorageImportSessionEnumVolume::Library)?;
        if stage.volume() == library.volume() {
            let required = additional_stage
                .checked_add(sqlite)
                .ok_or(StorageImportSessionError::Arithmetic)?;
            stage.require(required)?;
            library.require(required)
        } else {
            stage.require(additional_stage)?;
            library.require(sqlite)
        }
    }

    pub(crate) fn freeze(
        &mut self,
        session: GameId,
        sqlite: u64,
        token: &CapacityExecutorCancellation,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<StorageImportSessionConfirmation, StorageImportSessionError> {
        Self::check(token, cancellation)?;
        self.frozen = None;
        self.stream_digest = None;
        self.footprint = None;
        if self.preview_digest
            != Some(
                self.disk
                    .digest_file(StorageImportSessionDiskEnumFile::Directory, || {
                        Self::check(token, cancellation)
                    })?,
            )
        {
            return Err(StorageImportSessionError::Proof);
        }
        if self
            .disk
            .digest_file(StorageImportSessionDiskEnumFile::Source, || {
                Self::check(token, cancellation)
            })?
            != self.source_digest
        {
            return Err(StorageImportSessionError::Proof);
        }
        let roots = self.index_roots.ok_or(StorageImportSessionError::State)?;
        if self
            .disk
            .digest_file(StorageImportSessionDiskEnumFile::StructuralIndex, || {
                Self::check(token, cancellation)
            })?
            != roots.0
            || self
                .disk
                .digest_file(StorageImportSessionDiskEnumFile::IdentityIndex, || {
                    Self::check(token, cancellation)
                })?
                != roots.1
        {
            return Err(StorageImportSessionError::Proof);
        }
        let footprint = self.footprint(token, cancellation)?;
        let commit_bytes = footprint
            .count
            .checked_mul(StorageImportSessionPreparedCommit::header_length()?)
            .and_then(|value| value.checked_add(footprint.bodies))
            .and_then(|value| value.checked_add(footprint.projections))
            .ok_or(StorageImportSessionError::Arithmetic)?;
        let fs = Arc::clone(&self.disk.filesystem);
        let commit = self.disk.spool(StorageImportSessionDiskEnumFile::Commit)?;
        fs.release(&mut commit.file)?;
        commit.length = 0;
        self.require_space(commit_bytes, sqlite)?;
        let mut original = Sha256::new();
        for ordinal in 1..=self.records {
            Self::check(token, cancellation)?;
            let record = self.disk.record(ordinal)?;
            original.update(record.immutable_bytes());
            self.disk.bytes(
                StorageImportSessionDiskEnumFile::Metadata,
                record.metadata,
                Self::METADATA_LIMIT,
                record.metadata_digest,
            )?;
            if record.game.is_some() {
                let body = self.disk.bytes(
                    StorageImportSessionDiskEnumFile::Bodies,
                    record.body,
                    StorageDocumentStore::MEMORY_BUDGET,
                    record.body_digest,
                )?;
                let projection = self.disk.bytes(
                    StorageImportSessionDiskEnumFile::Projections,
                    record.projection,
                    StorageImportSessionPreparedCommit::MAX_PROJECTION_BYTES,
                    record.projection_digest,
                )?;
                if record
                    .choice
                    .is_some_and(StorageImportSessionEnumChoice::is_selected)
                {
                    self.disk.append(
                        StorageImportSessionDiskEnumFile::Commit,
                        &StorageImportSessionPreparedCommit::header(&record)?,
                    )?;
                    self.disk
                        .append(StorageImportSessionDiskEnumFile::Commit, &body)?;
                    self.disk
                        .append(StorageImportSessionDiskEnumFile::Commit, &projection)?;
                }
            }
        }
        let original_digest: [u8; 32] = self.original.clone().finalize().into();
        if <[u8; 32]>::from(original.finalize()) != original_digest {
            return Err(StorageImportSessionError::Proof);
        }
        for spool in &mut self.disk.files {
            fs.flush(&mut spool.file)?;
        }
        let stream_digest = self
            .disk
            .digest_file(StorageImportSessionDiskEnumFile::Commit, || {
                Self::check(token, cancellation)
            })?;
        let snapshot = self.snapshot.ok_or(StorageImportSessionError::State)?;
        let mut confirmation = StorageImportSessionConfirmation {
            session,
            revision: self.revision,
            digest: [0; 32],
            snapshot,
            summary: self.summary,
            source_bytes: self.source_length,
            source_digest: self.source_digest,
            stage_bytes: self.disk.total_len()?,
            commit_bytes,
            sqlite_bytes: sqlite,
        };
        let mut digest = Sha256::new();
        digest.update(Self::PROOF_FORMAT);
        digest.update(session.as_uuid().as_bytes());
        digest.update(original_digest);
        digest.update(
            self.preview_digest
                .ok_or(StorageImportSessionError::State)?,
        );
        digest.update(stream_digest);
        digest.update(self.source_digest);
        for number in [
            self.revision,
            snapshot.catalog_stamp().value(),
            snapshot.destination().revision(),
            self.summary.valid,
            self.summary.excluded,
            self.summary.duplicates,
            self.summary.selected,
            self.source_length,
            confirmation.stage_bytes,
            commit_bytes,
            sqlite,
        ] {
            digest.update(number.to_le_bytes());
        }
        digest.update(
            snapshot
                .destination()
                .folder_id()
                .map_or([0; 16], |folder| *folder.as_bytes()),
        );
        confirmation.digest = digest.finalize().into();
        self.stream_digest = Some(stream_digest);
        self.footprint = Some(footprint);
        self.frozen = Some(confirmation.clone());
        Ok(confirmation)
    }

    pub(crate) fn take_prepared(
        &mut self,
        confirmation: &StorageImportSessionConfirmation,
        cancellation: StorageImportSessionCancellation,
    ) -> Result<StorageImportSessionPreparedCommit, StorageImportSessionError> {
        if self.frozen.as_ref() != Some(confirmation) {
            return Err(StorageImportSessionError::Proof);
        }
        self.require_space(0, confirmation.sqlite_bytes)?;
        let footprint = self.footprint.ok_or(StorageImportSessionError::State)?;
        let digest = self.stream_digest.ok_or(StorageImportSessionError::State)?;
        Ok(StorageImportSessionPreparedCommit::new(
            self.disk.take_commit()?,
            Arc::clone(&self.disk.filesystem),
            StorageImportSessionPreparedOptions {
                confirmation: confirmation.clone(),
                footprint,
                digest,
                cancellation,
            },
        ))
    }
}
