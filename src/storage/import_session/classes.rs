use std::hash::{Hash, Hasher};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::capacity::executor::CapacityExecutorCancellation;
use crate::notation::game_identity::{NotationGameIdentity, NotationGameIdentityEnumMatch};
use crate::storage::import_session::disk::StorageImportSessionDiskEnumFile;
use crate::storage::import_session::index::{
    StorageImportSessionIndex, StorageImportSessionIndexBudget, StorageImportSessionIndexEntry,
};
use crate::storage::import_session::model::{
    StorageImportSessionCancellation, StorageImportSessionError,
};
use crate::storage::import_session::stage::StorageImportSessionStage;

struct StorageImportSessionClassHasher(Sha256);

impl Hasher for StorageImportSessionClassHasher {
    fn finish(&self) -> u64 {
        let digest = self.0.clone().finalize();
        let mut bytes = [0; 8];
        bytes.copy_from_slice(&digest[..8]);
        u64::from_le_bytes(bytes)
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

impl StorageImportSessionClassHasher {
    fn key(identity: &NotationGameIdentity) -> [u8; 32] {
        let mut hasher = Self(Sha256::new());
        identity.hash(&mut hasher);
        hasher.0.finalize().into()
    }
}

impl StorageImportSessionStage {
    const CLASS_MEMBER_WIDTH: usize = 16;

    fn class_member_offset(ordinal: u64) -> Result<u64, StorageImportSessionError> {
        let width = u64::try_from(Self::CLASS_MEMBER_WIDTH)
            .map_err(|_error| StorageImportSessionError::Arithmetic)?;
        ordinal
            .checked_sub(1)
            .and_then(|index| index.checked_mul(width))
            .ok_or(StorageImportSessionError::Arithmetic)
    }

    pub(super) fn class_member(
        &mut self,
        ordinal: u64,
    ) -> Result<(u64, u64), StorageImportSessionError> {
        let mut bytes = [0; Self::CLASS_MEMBER_WIDTH];
        self.disk.read_exact(
            StorageImportSessionDiskEnumFile::ClassMembers,
            Self::class_member_offset(ordinal)?,
            &mut bytes,
        )?;
        let representative = u64::from_le_bytes(
            bytes[..8]
                .try_into()
                .map_err(|_error| StorageImportSessionError::Proof)?,
        );
        let count = u64::from_le_bytes(
            bytes[8..]
                .try_into()
                .map_err(|_error| StorageImportSessionError::Proof)?,
        );
        if representative == 0 || representative > ordinal {
            return Err(StorageImportSessionError::Proof);
        }
        Ok((representative, count))
    }

    pub(super) fn write_class_member(
        &mut self,
        ordinal: u64,
        representative: u64,
        count: u64,
    ) -> Result<(), StorageImportSessionError> {
        let mut bytes = [0; Self::CLASS_MEMBER_WIDTH];
        bytes[..8].copy_from_slice(&representative.to_le_bytes());
        bytes[8..].copy_from_slice(&count.to_le_bytes());
        let offset = Self::class_member_offset(ordinal)?;
        let width =
            u64::try_from(bytes.len()).map_err(|_error| StorageImportSessionError::Arithmetic)?;
        if offset
            .checked_add(width)
            .ok_or(StorageImportSessionError::Arithmetic)?
            > self
                .disk
                .len(StorageImportSessionDiskEnumFile::ClassMembers)?
        {
            return Err(StorageImportSessionError::Proof);
        }
        let fs = Arc::clone(&self.disk.filesystem);
        let spool = self
            .disk
            .spool(StorageImportSessionDiskEnumFile::ClassMembers)?;
        fs.write_at(&mut spool.file, offset, &bytes)?;
        Ok(())
    }

    pub(super) fn build_classes(
        &mut self,
        token: &CapacityExecutorCancellation,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<(), StorageImportSessionError> {
        for kind in [
            StorageImportSessionDiskEnumFile::CompleteIndex,
            StorageImportSessionDiskEnumFile::ClassIndex,
            StorageImportSessionDiskEnumFile::ClassMembers,
            StorageImportSessionDiskEnumFile::IncompleteIndex,
        ] {
            let fs = Arc::clone(&self.disk.filesystem);
            let spool = self.disk.spool(kind)?;
            fs.release(&mut spool.file)?;
            spool.length = 0;
        }
        for ordinal in 1..=self.records {
            Self::check(token, cancellation)?;
            self.disk.append(
                StorageImportSessionDiskEnumFile::ClassMembers,
                &[0; Self::CLASS_MEMBER_WIDTH],
            )?;
            let record = self.disk.record(ordinal)?;
            if record.game.is_none() {
                continue;
            }
            let identity = self.read_record(ordinal, token)?.identity()?;
            let complete = identity.compare(&identity) == NotationGameIdentityEnumMatch::Exact;
            let (kind, key) = if complete {
                (
                    StorageImportSessionDiskEnumFile::CompleteIndex,
                    StorageImportSessionClassHasher::key(&identity),
                )
            } else {
                self.write_class_member(ordinal, ordinal, 0)?;
                self.disk.append(
                    StorageImportSessionDiskEnumFile::IncompleteIndex,
                    &StorageImportSessionIndexEntry {
                        key: record.structural_digest,
                        ordinal,
                    }
                    .encoded(),
                )?;
                (
                    StorageImportSessionDiskEnumFile::ClassIndex,
                    record.structural_digest,
                )
            };
            self.disk.append(
                kind,
                &StorageImportSessionIndexEntry { key, ordinal }.encoded(),
            )?;
        }
        StorageImportSessionIndex::sort(
            &mut self.disk,
            StorageImportSessionDiskEnumFile::CompleteIndex,
            StorageImportSessionIndexBudget::new(StorageImportSessionIndexBudget::MAX_BYTES)?,
            || Self::check(token, cancellation),
        )?;
        StorageImportSessionIndex::sort(
            &mut self.disk,
            StorageImportSessionDiskEnumFile::IncompleteIndex,
            StorageImportSessionIndexBudget::new(StorageImportSessionIndexBudget::MAX_BYTES)?,
            || Self::check(token, cancellation),
        )?;
        Ok(())
    }

    pub(super) fn assign_complete_classes(
        &mut self,
        token: &CapacityExecutorCancellation,
        cancellation: &StorageImportSessionCancellation,
    ) -> Result<(), StorageImportSessionError> {
        let mut previous_key = None;
        let mut begin = 0;
        let mut cached: Option<(u64, NotationGameIdentity)> = None;
        for index in 0..StorageImportSessionIndex::count(
            &self.disk,
            StorageImportSessionDiskEnumFile::CompleteIndex,
        )? {
            Self::check(token, cancellation)?;
            let entry = StorageImportSessionIndex::entry(
                &mut self.disk,
                StorageImportSessionDiskEnumFile::CompleteIndex,
                index,
            )?;
            if previous_key != Some(entry.key) {
                previous_key = Some(entry.key);
                begin = StorageImportSessionIndex::count(
                    &self.disk,
                    StorageImportSessionDiskEnumFile::ClassIndex,
                )?;
                cached = None;
            }
            let identity = self.read_record(entry.ordinal, token)?.identity()?;
            if identity.compare(&identity) != NotationGameIdentityEnumMatch::Exact {
                return Err(StorageImportSessionError::Proof);
            }
            let mut representative = cached
                .as_ref()
                .filter(|(_ordinal, other)| *other == identity)
                .map(|(ordinal, _identity)| *ordinal);
            if representative.is_none() {
                for candidate in begin
                    ..StorageImportSessionIndex::count(
                        &self.disk,
                        StorageImportSessionDiskEnumFile::ClassIndex,
                    )?
                {
                    Self::check(token, cancellation)?;
                    let other = StorageImportSessionIndex::entry(
                        &mut self.disk,
                        StorageImportSessionDiskEnumFile::ClassIndex,
                        candidate,
                    )?;
                    let other_identity = self.read_record(other.ordinal, token)?.identity()?;
                    if identity == other_identity {
                        representative = Some(other.ordinal);
                        break;
                    }
                }
            }
            let representative = match representative {
                Some(ordinal) => ordinal,
                None => {
                    let record = self.disk.record(entry.ordinal)?;
                    self.disk.append(
                        StorageImportSessionDiskEnumFile::ClassIndex,
                        &StorageImportSessionIndexEntry {
                            key: record.structural_digest,
                            ordinal: entry.ordinal,
                        }
                        .encoded(),
                    )?;
                    entry.ordinal
                }
            };
            self.write_class_member(entry.ordinal, representative, 0)?;
            cached = Some((representative, identity));
        }
        StorageImportSessionIndex::sort(
            &mut self.disk,
            StorageImportSessionDiskEnumFile::ClassIndex,
            StorageImportSessionIndexBudget::new(StorageImportSessionIndexBudget::MAX_BYTES)?,
            || Self::check(token, cancellation),
        )
    }
}
