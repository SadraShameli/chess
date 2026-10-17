use std::fs::File;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::game::enums::GameId;
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::import_session::disk::{
    StorageImportSessionDisk, StorageImportSessionDiskRecord,
};
use crate::storage::import_session::model::{
    StorageImportSessionCancellation, StorageImportSessionCatalogStamp,
    StorageImportSessionCommitFootprint, StorageImportSessionConfirmation,
    StorageImportSessionDestination, StorageImportSessionEnumChoice, StorageImportSessionError,
    StorageImportSessionFilesystemProvider,
};

pub struct StorageImportSessionPreparedFrame<'a> {
    ordinal: u64,
    game: GameId,
    choice: StorageImportSessionEnumChoice,
    body: &'a [u8],
    projection: &'a [u8],
    body_digest: [u8; 32],
    projection_digest: [u8; 32],
}

impl StorageImportSessionPreparedFrame<'_> {
    pub const fn ordinal(&self) -> u64 {
        self.ordinal
    }
    pub const fn game(&self) -> GameId {
        self.game
    }
    pub const fn choice(&self) -> StorageImportSessionEnumChoice {
        self.choice
    }
    pub fn body(&self) -> &[u8] {
        self.body
    }
    pub fn projection(&self) -> &[u8] {
        self.projection
    }
    pub const fn body_digest(&self) -> [u8; 32] {
        self.body_digest
    }
    pub const fn projection_digest(&self) -> [u8; 32] {
        self.projection_digest
    }
}

pub struct StorageImportSessionPreparedCommit {
    file: File,
    filesystem: Arc<dyn StorageImportSessionFilesystemProvider>,
    confirmation: StorageImportSessionConfirmation,
    footprint: StorageImportSessionCommitFootprint,
    expected_digest: [u8; 32],
    cancellation: StorageImportSessionCancellation,
    digest: Sha256,
    offset: u64,
    count: u64,
    previous_ordinal: u64,
    body: Vec<u8>,
    projection: Vec<u8>,
}

impl StorageImportSessionPreparedCommit {
    pub(crate) const HEADER_BYTES: usize = 112;
    pub const MAX_PROJECTION_BYTES: usize = StorageDocumentStore::MEMORY_BUDGET;

    pub(crate) fn header_length() -> Result<u64, StorageImportSessionError> {
        u64::try_from(Self::HEADER_BYTES).map_err(|_error| StorageImportSessionError::Arithmetic)
    }

    pub(super) fn new(
        file: File,
        filesystem: Arc<dyn StorageImportSessionFilesystemProvider>,
        options: StorageImportSessionPreparedOptions,
    ) -> Self {
        Self {
            file,
            filesystem,
            confirmation: options.confirmation,
            footprint: options.footprint,
            expected_digest: options.digest,
            cancellation: options.cancellation,
            digest: Sha256::new(),
            offset: 0,
            count: 0,
            previous_ordinal: 0,
            body: Vec::new(),
            projection: Vec::new(),
        }
    }

    pub const fn commit_id(&self) -> GameId {
        self.confirmation.session
    }
    pub const fn catalog_stamp(&self) -> StorageImportSessionCatalogStamp {
        self.confirmation.snapshot.catalog_stamp()
    }
    pub const fn destination(&self) -> StorageImportSessionDestination {
        self.confirmation.snapshot.destination()
    }
    pub const fn footprint(&self) -> StorageImportSessionCommitFootprint {
        self.footprint
    }
    pub fn confirmation(&self) -> &StorageImportSessionConfirmation {
        &self.confirmation
    }

    pub fn recheck_space(&self) -> Result<(), StorageImportSessionError> {
        self.cancellation.check()?;
        self.filesystem
            .observe_space(
                crate::storage::import_session::model::StorageImportSessionEnumVolume::Library,
            )?
            .require(self.confirmation.sqlite_bytes)
    }

    pub(crate) fn header(
        record: &StorageImportSessionDiskRecord,
    ) -> Result<[u8; Self::HEADER_BYTES], StorageImportSessionError> {
        let mut header = [0; Self::HEADER_BYTES];
        header[..8].copy_from_slice(&record.ordinal.to_le_bytes());
        header[8..24].copy_from_slice(
            record
                .game
                .ok_or(StorageImportSessionError::Proof)?
                .as_uuid()
                .as_bytes(),
        );
        header[24..32].copy_from_slice(
            &record
                .choice
                .ok_or(StorageImportSessionError::Proof)?
                .code()
                .to_le_bytes(),
        );
        header[32..40].copy_from_slice(&record.body.length.to_le_bytes());
        header[40..48].copy_from_slice(&record.projection.length.to_le_bytes());
        header[48..80].copy_from_slice(&record.body_digest);
        header[80..112].copy_from_slice(&record.projection_digest);
        Ok(header)
    }

    fn check(
        &self,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<(), StorageImportSessionError> {
        self.cancellation.check()?;
        cancellation.check()
    }

    fn read(
        &mut self,
        bytes: &mut [u8],
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<(), StorageImportSessionError> {
        for chunk in bytes.chunks_mut(StorageImportSessionDisk::COPY_BYTES) {
            self.check(cancellation)?;
            StorageImportSessionDisk::read_file_exact(
                self.filesystem.as_ref(),
                &mut self.file,
                self.offset,
                chunk,
            )?;
            self.digest.update(&*chunk);
            self.offset = self
                .offset
                .checked_add(
                    u64::try_from(chunk.len())
                        .map_err(|_error| StorageImportSessionError::Arithmetic)?,
                )
                .ok_or(StorageImportSessionError::Arithmetic)?;
            if self.offset > self.confirmation.commit_bytes {
                return Err(StorageImportSessionError::Proof);
            }
        }
        Ok(())
    }

    fn number(bytes: &[u8]) -> Result<u64, StorageImportSessionError> {
        Ok(u64::from_le_bytes(
            bytes
                .try_into()
                .map_err(|_error| StorageImportSessionError::Proof)?,
        ))
    }

    pub fn next_frame(
        &mut self,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<Option<StorageImportSessionPreparedFrame<'_>>, StorageImportSessionError> {
        self.check(cancellation)?;
        if self.count == self.footprint.count {
            self.finish(cancellation)?;
            return Ok(None);
        }
        self.body = Vec::new();
        self.projection = Vec::new();
        let mut header = [0; Self::HEADER_BYTES];
        self.read(&mut header, cancellation)?;
        let ordinal = Self::number(&header[..8])?;
        if ordinal <= self.previous_ordinal {
            return Err(StorageImportSessionError::Proof);
        }
        let game: GameId = uuid::Uuid::from_slice(&header[8..24])
            .map_err(crate::game::enums::GameIdentityError::from)?
            .to_string()
            .parse()?;
        let choice = StorageImportSessionEnumChoice::from_code(Self::number(&header[24..32])?)?;
        if !choice.is_selected() {
            return Err(StorageImportSessionError::Proof);
        }
        let body_bytes = usize::try_from(Self::number(&header[32..40])?)
            .map_err(|_error| StorageImportSessionError::Arithmetic)?;
        let projection_bytes = usize::try_from(Self::number(&header[40..48])?)
            .map_err(|_error| StorageImportSessionError::Arithmetic)?;
        if body_bytes == 0
            || body_bytes > StorageDocumentStore::MEMORY_BUDGET
            || projection_bytes > Self::MAX_PROJECTION_BYTES
        {
            return Err(StorageImportSessionError::Proof);
        }
        let payload_bytes = u64::try_from(body_bytes)
            .map_err(|_error| StorageImportSessionError::Arithmetic)?
            .checked_add(
                u64::try_from(projection_bytes)
                    .map_err(|_error| StorageImportSessionError::Arithmetic)?,
            )
            .ok_or(StorageImportSessionError::Arithmetic)?;
        let frame_bytes = payload_bytes
            .checked_add(Self::header_length()?)
            .ok_or(StorageImportSessionError::Arithmetic)?;
        if frame_bytes > self.footprint.largest_frame_bytes()
            || self
                .offset
                .checked_add(payload_bytes)
                .ok_or(StorageImportSessionError::Arithmetic)?
                > self.confirmation.commit_bytes
        {
            return Err(StorageImportSessionError::Proof);
        }
        let mut body = vec![0; body_bytes];
        self.read(&mut body, cancellation)?;
        let body_digest: [u8; 32] = Sha256::digest(&body).into();
        if body_digest.as_slice() != &header[48..80] {
            return Err(StorageImportSessionError::Proof);
        }
        let mut projection = vec![0; projection_bytes];
        self.read(&mut projection, cancellation)?;
        let projection_digest: [u8; 32] = Sha256::digest(&projection).into();
        if projection_digest.as_slice() != &header[80..112] {
            return Err(StorageImportSessionError::Proof);
        }
        self.count = self
            .count
            .checked_add(1)
            .ok_or(StorageImportSessionError::Arithmetic)?;
        self.previous_ordinal = ordinal;
        self.body = body;
        self.projection = projection;
        Ok(Some(StorageImportSessionPreparedFrame {
            ordinal,
            game,
            choice,
            body: &self.body,
            projection: &self.projection,
            body_digest,
            projection_digest,
        }))
    }

    pub fn finish(
        &self,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<(), StorageImportSessionError> {
        self.check(cancellation)?;
        if self.count != self.footprint.count
            || self.offset != self.confirmation.commit_bytes
            || self.filesystem.length(&self.file)? != self.offset
            || <[u8; 32]>::from(self.digest.clone().finalize()) != self.expected_digest
        {
            return Err(StorageImportSessionError::Proof);
        }
        Ok(())
    }
}

pub(super) struct StorageImportSessionPreparedOptions {
    pub(super) confirmation: StorageImportSessionConfirmation,
    pub(super) footprint: StorageImportSessionCommitFootprint,
    pub(super) digest: [u8; 32],
    pub(super) cancellation: StorageImportSessionCancellation,
}
