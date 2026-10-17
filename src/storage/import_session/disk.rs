use std::fs::File;
use std::io::{ErrorKind, Read};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use strum::IntoEnumIterator;

use crate::game::enums::GameId;
use crate::storage::import_session::model::{
    StorageImportSessionEnumChoice, StorageImportSessionEnumVolume, StorageImportSessionError,
    StorageImportSessionFilesystemProvider,
};

pub(crate) struct StorageImportSessionDisk {
    pub(crate) files: Vec<StorageImportSessionDiskSpool>,
    pub(crate) filesystem: Arc<dyn StorageImportSessionFilesystemProvider>,
}

#[derive(Clone, Copy, strum::EnumIter)]
pub(crate) enum StorageImportSessionDiskEnumFile {
    Source,
    Bodies,
    Projections,
    Directory,
    Metadata,
    StructuralIndex,
    IdentityIndex,
    Sort,
    Commit,
    CompleteIndex,
    ClassIndex,
    ClassMembers,
    IncompleteIndex,
}

impl StorageImportSessionDiskEnumFile {
    const fn index(self) -> usize {
        match self {
            Self::Source => 0,
            Self::Bodies => 1,
            Self::Projections => 2,
            Self::Directory => 3,
            Self::Metadata => 4,
            Self::StructuralIndex => 5,
            Self::IdentityIndex => 6,
            Self::Sort => 7,
            Self::Commit => 8,
            Self::CompleteIndex => 9,
            Self::ClassIndex => 10,
            Self::ClassMembers => 11,
            Self::IncompleteIndex => 12,
        }
    }
}

pub(crate) struct StorageImportSessionDiskSpool {
    pub(crate) file: File,
    pub(crate) length: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StorageImportSessionDiskSpan {
    pub(crate) offset: u64,
    pub(crate) length: u64,
}

impl StorageImportSessionDiskSpan {
    pub(crate) const EMPTY: Self = Self {
        offset: 0,
        length: 0,
    };
    pub(crate) fn end(self) -> Result<u64, StorageImportSessionError> {
        self.offset
            .checked_add(self.length)
            .ok_or(StorageImportSessionError::Arithmetic)
    }
}

pub(crate) struct StorageImportSessionDiskRecord {
    pub(crate) ordinal: u64,
    pub(crate) game: Option<GameId>,
    pub(crate) choice: Option<StorageImportSessionEnumChoice>,
    pub(crate) body: StorageImportSessionDiskSpan,
    pub(crate) projection: StorageImportSessionDiskSpan,
    pub(crate) metadata: StorageImportSessionDiskSpan,
    pub(crate) exact: u64,
    pub(crate) potential: u64,
    pub(crate) body_digest: [u8; 32],
    pub(crate) projection_digest: [u8; 32],
    pub(crate) metadata_digest: [u8; 32],
    pub(crate) structural_digest: [u8; 32],
    pub(crate) candidate_digest: [u8; 32],
}

impl StorageImportSessionDiskRecord {
    pub(crate) const WIDTH: usize = 264;

    pub(crate) fn encoded(&self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        for (target, value) in bytes[..88].as_chunks_mut::<8>().0.iter_mut().zip([
            self.ordinal,
            u64::from(self.game.is_some()),
            self.choice.map_or(0, StorageImportSessionEnumChoice::code),
            self.body.offset,
            self.body.length,
            self.projection.offset,
            self.projection.length,
            self.metadata.offset,
            self.metadata.length,
            self.exact,
            self.potential,
        ]) {
            target.copy_from_slice(&value.to_le_bytes());
        }
        if let Some(game) = self.game {
            bytes[88..104].copy_from_slice(game.as_uuid().as_bytes());
        }
        for (target, digest) in bytes[104..].as_chunks_mut::<32>().0.iter_mut().zip([
            self.body_digest,
            self.projection_digest,
            self.metadata_digest,
            self.structural_digest,
            self.candidate_digest,
        ]) {
            target.copy_from_slice(&digest);
        }
        bytes
    }

    pub(crate) fn immutable_bytes(&self) -> [u8; Self::WIDTH] {
        let mut bytes = self.encoded();
        bytes[16..24].fill(0);
        bytes[72..88].fill(0);
        bytes[232..].fill(0);
        bytes
    }

    pub(crate) fn decode(bytes: [u8; Self::WIDTH]) -> Result<Self, StorageImportSessionError> {
        let mut numbers = [0; 11];
        for (target, chunk) in numbers
            .iter_mut()
            .zip(bytes[..88].as_chunks::<8>().0.iter())
        {
            *target = u64::from_le_bytes(*chunk);
        }
        let game = match numbers[1] {
            0 if bytes[88..104].iter().all(|byte| *byte == 0) => None,
            1 => Some(
                uuid::Uuid::from_slice(&bytes[88..104])
                    .map_err(GameIdentityError::from)?
                    .to_string()
                    .parse()?,
            ),
            _ => return Err(StorageImportSessionError::Proof),
        };
        let choice = if game.is_some() {
            Some(StorageImportSessionEnumChoice::from_code(numbers[2])?)
        } else if numbers[2] == 0 {
            None
        } else {
            return Err(StorageImportSessionError::Proof);
        };
        let mut digests = [[0; 32]; 5];
        for (digest, chunk) in digests
            .iter_mut()
            .zip(bytes[104..].as_chunks::<32>().0.iter())
        {
            digest.copy_from_slice(chunk);
        }
        Ok(Self {
            ordinal: numbers[0],
            game,
            choice,
            body: StorageImportSessionDiskSpan {
                offset: numbers[3],
                length: numbers[4],
            },
            projection: StorageImportSessionDiskSpan {
                offset: numbers[5],
                length: numbers[6],
            },
            metadata: StorageImportSessionDiskSpan {
                offset: numbers[7],
                length: numbers[8],
            },
            exact: numbers[9],
            potential: numbers[10],
            body_digest: digests[0],
            projection_digest: digests[1],
            metadata_digest: digests[2],
            structural_digest: digests[3],
            candidate_digest: digests[4],
        })
    }

    pub(crate) fn is_duplicate(&self) -> bool {
        self.exact != 0 || self.potential != 0
    }
}

use crate::game::enums::GameIdentityError;

impl StorageImportSessionDisk {
    pub(crate) fn new(filesystem: Arc<dyn StorageImportSessionFilesystemProvider>) -> Self {
        Self {
            files: Vec::new(),
            filesystem,
        }
    }

    pub(crate) fn initialize(&mut self) -> Result<(), StorageImportSessionError> {
        for _kind in StorageImportSessionDiskEnumFile::iter().skip(self.files.len()) {
            let file = self.filesystem.create_spool()?;
            self.files
                .push(StorageImportSessionDiskSpool { file, length: 0 });
        }
        Ok(())
    }

    pub(crate) fn spool(
        &mut self,
        kind: StorageImportSessionDiskEnumFile,
    ) -> Result<&mut StorageImportSessionDiskSpool, StorageImportSessionError> {
        self.files
            .get_mut(kind.index())
            .ok_or(StorageImportSessionError::State)
    }

    pub(crate) fn take_commit(&mut self) -> Result<File, StorageImportSessionError> {
        if self.files.len() != StorageImportSessionDiskEnumFile::iter().count() {
            return Err(StorageImportSessionError::State);
        }
        Ok(self
            .files
            .swap_remove(StorageImportSessionDiskEnumFile::Commit.index())
            .file)
    }

    pub(crate) fn len(
        &self,
        kind: StorageImportSessionDiskEnumFile,
    ) -> Result<u64, StorageImportSessionError> {
        self.files
            .get(kind.index())
            .map(|spool| spool.length)
            .ok_or(StorageImportSessionError::State)
    }

    pub(crate) fn total_len(&self) -> Result<u64, StorageImportSessionError> {
        self.files.iter().try_fold(0_u64, |total, spool| {
            total
                .checked_add(spool.length)
                .ok_or(StorageImportSessionError::Arithmetic)
        })
    }

    pub(crate) fn append(
        &mut self,
        kind: StorageImportSessionDiskEnumFile,
        bytes: &[u8],
    ) -> Result<StorageImportSessionDiskSpan, StorageImportSessionError> {
        let length =
            u64::try_from(bytes.len()).map_err(|_error| StorageImportSessionError::Arithmetic)?;
        self.filesystem
            .observe_space(StorageImportSessionEnumVolume::Stage)?
            .require(length)?;
        let fs = Arc::clone(&self.filesystem);
        let spool = self.spool(kind)?;
        let offset = spool.length;
        let end = offset
            .checked_add(length)
            .ok_or(StorageImportSessionError::Arithmetic)?;
        fs.write_at(&mut spool.file, offset, bytes)?;
        spool.length = end;
        Ok(StorageImportSessionDiskSpan { offset, length })
    }

    pub(crate) fn read_exact(
        &mut self,
        kind: StorageImportSessionDiskEnumFile,
        offset: u64,
        bytes: &mut [u8],
    ) -> Result<(), StorageImportSessionError> {
        let fs = Arc::clone(&self.filesystem);
        let spool = self.spool(kind)?;
        Self::read_file_exact(fs.as_ref(), &mut spool.file, offset, bytes)
    }

    pub(crate) fn read_file_exact(
        fs: &dyn StorageImportSessionFilesystemProvider,
        file: &mut File,
        mut offset: u64,
        mut bytes: &mut [u8],
    ) -> Result<(), StorageImportSessionError> {
        while !bytes.is_empty() {
            let count = fs.read_at(file, offset, bytes)?;
            if count == 0 || count > bytes.len() {
                return Err(StorageImportSessionError::Proof);
            }
            offset = offset
                .checked_add(
                    u64::try_from(count).map_err(|_error| StorageImportSessionError::Arithmetic)?,
                )
                .ok_or(StorageImportSessionError::Arithmetic)?;
            bytes = bytes
                .get_mut(count..)
                .ok_or(StorageImportSessionError::Proof)?;
        }
        Ok(())
    }

    pub(crate) fn bytes(
        &mut self,
        kind: StorageImportSessionDiskEnumFile,
        span: StorageImportSessionDiskSpan,
        cap: usize,
        digest: [u8; 32],
    ) -> Result<Vec<u8>, StorageImportSessionError> {
        let length =
            usize::try_from(span.length).map_err(|_error| StorageImportSessionError::Arithmetic)?;
        if length > cap || span.end()? > self.len(kind)? {
            return Err(StorageImportSessionError::Proof);
        }
        let mut bytes = vec![0; length];
        self.read_exact(kind, span.offset, &mut bytes)?;
        if <[u8; 32]>::from(Sha256::digest(&bytes)) != digest {
            return Err(StorageImportSessionError::Proof);
        }
        Ok(bytes)
    }

    pub(crate) fn record(
        &mut self,
        ordinal: u64,
    ) -> Result<StorageImportSessionDiskRecord, StorageImportSessionError> {
        let mut bytes = [0; StorageImportSessionDiskRecord::WIDTH];
        let offset = Self::record_offset(ordinal)?;
        self.read_exact(
            StorageImportSessionDiskEnumFile::Directory,
            offset,
            &mut bytes,
        )?;
        let record = StorageImportSessionDiskRecord::decode(bytes)?;
        if record.ordinal != ordinal {
            return Err(StorageImportSessionError::Proof);
        }
        Ok(record)
    }

    pub(crate) fn write_record(
        &mut self,
        record: &StorageImportSessionDiskRecord,
    ) -> Result<(), StorageImportSessionError> {
        let fs = Arc::clone(&self.filesystem);
        let spool = self.spool(StorageImportSessionDiskEnumFile::Directory)?;
        fs.write_at(
            &mut spool.file,
            Self::record_offset(record.ordinal)?,
            &record.encoded(),
        )?;
        Ok(())
    }

    fn record_offset(ordinal: u64) -> Result<u64, StorageImportSessionError> {
        let width = u64::try_from(StorageImportSessionDiskRecord::WIDTH)
            .map_err(|_error| StorageImportSessionError::Arithmetic)?;
        ordinal
            .checked_sub(1)
            .and_then(|value| value.checked_mul(width))
            .ok_or(StorageImportSessionError::Arithmetic)
    }

    pub(crate) fn digest_file(
        &mut self,
        kind: StorageImportSessionDiskEnumFile,
        mut check: impl FnMut() -> Result<(), StorageImportSessionError>,
    ) -> Result<[u8; 32], StorageImportSessionError> {
        let length = self.len(kind)?;
        let mut offset = 0_u64;
        let mut buffer = vec![0; Self::COPY_BYTES];
        let mut digest = Sha256::new();
        while offset < length {
            check()?;
            let count = usize::try_from(
                (length - offset).min(
                    u64::try_from(buffer.len())
                        .map_err(|_error| StorageImportSessionError::Arithmetic)?,
                ),
            )
            .map_err(|_error| StorageImportSessionError::Arithmetic)?;
            self.read_exact(kind, offset, &mut buffer[..count])?;
            digest.update(&buffer[..count]);
            offset = offset
                .checked_add(
                    u64::try_from(count).map_err(|_error| StorageImportSessionError::Arithmetic)?,
                )
                .ok_or(StorageImportSessionError::Arithmetic)?;
        }
        let fs = Arc::clone(&self.filesystem);
        if fs.length(&self.spool(kind)?.file)? != length {
            return Err(StorageImportSessionError::Proof);
        }
        Ok(digest.finalize().into())
    }

    pub(crate) const COPY_BYTES: usize = 64 * 1024;

    pub(crate) fn cleanup(&mut self) -> Result<(), StorageImportSessionError> {
        let mut failure: Option<Arc<StorageImportSessionError>> = None;
        for spool in &mut self.files {
            match self.filesystem.release(&mut spool.file) {
                Ok(()) => spool.length = 0,
                Err(error) => {
                    let cleanup = Arc::new(StorageImportSessionError::Io(error));
                    failure = Some(match failure {
                        None => cleanup,
                        Some(primary) => {
                            Arc::new(StorageImportSessionError::Cleanup { primary, cleanup })
                        }
                    });
                }
            }
        }
        if let Err(error) = self.filesystem.cleanup_retained() {
            let cleanup = Arc::new(error);
            failure = Some(match failure {
                None => cleanup,
                Some(primary) => Arc::new(StorageImportSessionError::Cleanup { primary, cleanup }),
            });
        }
        if let Some(primary) = failure {
            return Err(StorageImportSessionError::Retained(primary));
        }
        self.files.clear();
        Ok(())
    }
}

pub(crate) struct StorageImportSessionDiskReader<'a> {
    pub(crate) file: &'a mut File,
    pub(crate) filesystem: &'a dyn StorageImportSessionFilesystemProvider,
    pub(crate) offset: u64,
}

impl Read for StorageImportSessionDiskReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let count = self.filesystem.read_at(self.file, self.offset, bytes)?;
        if count > bytes.len() {
            return Err(std::io::Error::from(ErrorKind::InvalidData));
        }
        self.offset = self
            .offset
            .checked_add(u64::try_from(count).map_err(std::io::Error::other)?)
            .ok_or_else(|| std::io::Error::from(ErrorKind::InvalidData))?;
        Ok(count)
    }
}
