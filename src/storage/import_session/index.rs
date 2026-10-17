use std::mem::size_of;

use crate::storage::import_session::disk::{
    StorageImportSessionDisk, StorageImportSessionDiskEnumFile,
};
use crate::storage::import_session::model::StorageImportSessionError;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StorageImportSessionIndexEntry {
    pub(crate) key: [u8; 32],
    pub(crate) ordinal: u64,
}

impl StorageImportSessionIndexEntry {
    pub(crate) const WIDTH: usize = 40;

    pub(crate) fn encoded(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        bytes[..32].copy_from_slice(&self.key);
        bytes[32..].copy_from_slice(&self.ordinal.to_le_bytes());
        bytes
    }

    fn decode(bytes: [u8; Self::WIDTH]) -> Result<Self, StorageImportSessionError> {
        Ok(Self {
            key: bytes[..32]
                .try_into()
                .map_err(|_error| StorageImportSessionError::Proof)?,
            ordinal: u64::from_le_bytes(
                bytes[32..]
                    .try_into()
                    .map_err(|_error| StorageImportSessionError::Proof)?,
            ),
        })
    }
}

pub(crate) struct StorageImportSessionIndexBudget(usize);

impl StorageImportSessionIndexBudget {
    pub(crate) const MAX_BYTES: usize = 128 * 1024 * 1024;

    pub(crate) fn new(bytes: usize) -> Result<Self, StorageImportSessionError> {
        if bytes
            < 2 * (size_of::<StorageImportSessionIndexEntry>()
                + StorageImportSessionIndexEntry::WIDTH)
            || bytes > Self::MAX_BYTES
        {
            return Err(StorageImportSessionError::PageBudget);
        }
        Ok(Self(bytes))
    }
}

pub(crate) struct StorageImportSessionIndex;

impl StorageImportSessionIndex {
    fn offset(index: u64) -> Result<u64, StorageImportSessionError> {
        let width = u64::try_from(StorageImportSessionIndexEntry::WIDTH)
            .map_err(|_error| StorageImportSessionError::Arithmetic)?;
        index
            .checked_mul(width)
            .ok_or(StorageImportSessionError::Arithmetic)
    }

    pub(crate) fn count(
        disk: &StorageImportSessionDisk,
        kind: StorageImportSessionDiskEnumFile,
    ) -> Result<u64, StorageImportSessionError> {
        let length = disk.len(kind)?;
        let width = u64::try_from(StorageImportSessionIndexEntry::WIDTH)
            .map_err(|_error| StorageImportSessionError::Arithmetic)?;
        if length % width != 0 {
            return Err(StorageImportSessionError::Proof);
        }
        Ok(length / width)
    }

    pub(crate) fn entry(
        disk: &mut StorageImportSessionDisk,
        kind: StorageImportSessionDiskEnumFile,
        index: u64,
    ) -> Result<StorageImportSessionIndexEntry, StorageImportSessionError> {
        let mut bytes = [0; StorageImportSessionIndexEntry::WIDTH];
        disk.read_exact(kind, Self::offset(index)?, &mut bytes)?;
        StorageImportSessionIndexEntry::decode(bytes)
    }

    fn write(
        disk: &mut StorageImportSessionDisk,
        kind: StorageImportSessionDiskEnumFile,
        index: u64,
        bytes: &[u8],
    ) -> Result<(), StorageImportSessionError> {
        let offset = Self::offset(index)?;
        let length =
            u64::try_from(bytes.len()).map_err(|_error| StorageImportSessionError::Arithmetic)?;
        let end = offset
            .checked_add(length)
            .ok_or(StorageImportSessionError::Arithmetic)?;
        let previous = disk.len(kind)?;
        if end > previous {
            disk.filesystem
                .observe_space(
                    crate::storage::import_session::model::StorageImportSessionEnumVolume::Stage,
                )?
                .require(end - previous)?;
        }
        let fs = std::sync::Arc::clone(&disk.filesystem);
        let spool = disk.spool(kind)?;
        fs.write_at(&mut spool.file, offset, bytes)?;
        spool.length = spool.length.max(end);
        Ok(())
    }

    pub(crate) fn sort(
        disk: &mut StorageImportSessionDisk,
        kind: StorageImportSessionDiskEnumFile,
        budget: StorageImportSessionIndexBudget,
        mut check: impl FnMut() -> Result<(), StorageImportSessionError>,
    ) -> Result<(), StorageImportSessionError> {
        let count = Self::count(disk, kind)?;
        let batch = budget.0
            / (size_of::<StorageImportSessionIndexEntry>() + StorageImportSessionIndexEntry::WIDTH);
        let run_width =
            u64::try_from(batch).map_err(|_error| StorageImportSessionError::Arithmetic)?;
        let mut start = 0_u64;
        while start < count {
            check()?;
            let end = start
                .checked_add(run_width)
                .ok_or(StorageImportSessionError::Arithmetic)?
                .min(count);
            let capacity = usize::try_from(end - start)
                .map_err(|_error| StorageImportSessionError::Arithmetic)?;
            let mut entries = Vec::with_capacity(capacity);
            for index in start..end {
                check()?;
                entries.push(Self::entry(disk, kind, index)?);
            }
            entries.sort_unstable();
            check()?;
            let byte_capacity = capacity
                .checked_mul(StorageImportSessionIndexEntry::WIDTH)
                .ok_or(StorageImportSessionError::Arithmetic)?;
            let mut bytes = Vec::with_capacity(byte_capacity);
            for entry in entries {
                bytes.extend_from_slice(&entry.encoded());
            }
            Self::write(disk, kind, start, &bytes)?;
            start = end;
        }
        let mut width = run_width;
        let mut is_original = true;
        while width < count {
            check()?;
            let (source, destination) = if is_original {
                (kind, StorageImportSessionDiskEnumFile::Sort)
            } else {
                (StorageImportSessionDiskEnumFile::Sort, kind)
            };
            let mut start = 0_u64;
            while start < count {
                let middle = start
                    .checked_add(width)
                    .ok_or(StorageImportSessionError::Arithmetic)?
                    .min(count);
                let end = middle
                    .checked_add(width)
                    .ok_or(StorageImportSessionError::Arithmetic)?
                    .min(count);
                let (mut left, mut right, mut output) = (start, middle, start);
                let mut a = if left < middle {
                    Some(Self::entry(disk, source, left)?)
                } else {
                    None
                };
                let mut b = if right < end {
                    Some(Self::entry(disk, source, right)?)
                } else {
                    None
                };
                while a.is_some() || b.is_some() {
                    check()?;
                    let take_left = match (a, b) {
                        (Some(a), Some(b)) => a <= b,
                        (Some(_), None) => true,
                        (None, Some(_)) => false,
                        (None, None) => return Err(StorageImportSessionError::Proof),
                    };
                    let value = if take_left {
                        let value = a.take().ok_or(StorageImportSessionError::Proof)?;
                        left = left
                            .checked_add(1)
                            .ok_or(StorageImportSessionError::Arithmetic)?;
                        a = if left < middle {
                            Some(Self::entry(disk, source, left)?)
                        } else {
                            None
                        };
                        value
                    } else {
                        let value = b.take().ok_or(StorageImportSessionError::Proof)?;
                        right = right
                            .checked_add(1)
                            .ok_or(StorageImportSessionError::Arithmetic)?;
                        b = if right < end {
                            Some(Self::entry(disk, source, right)?)
                        } else {
                            None
                        };
                        value
                    };
                    Self::write(disk, destination, output, &value.encoded())?;
                    output = output
                        .checked_add(1)
                        .ok_or(StorageImportSessionError::Arithmetic)?;
                }
                start = end;
            }
            is_original = !is_original;
            width = width
                .checked_mul(2)
                .ok_or(StorageImportSessionError::Arithmetic)?;
        }
        if !is_original {
            for index in 0..count {
                check()?;
                let entry = Self::entry(disk, StorageImportSessionDiskEnumFile::Sort, index)?;
                Self::write(disk, kind, index, &entry.encoded())?;
            }
        }
        let fs = std::sync::Arc::clone(&disk.filesystem);
        let scratch = disk.spool(StorageImportSessionDiskEnumFile::Sort)?;
        fs.release(&mut scratch.file)?;
        scratch.length = 0;
        Ok(())
    }

    pub(crate) fn bucket(
        disk: &mut StorageImportSessionDisk,
        key: [u8; 32],
    ) -> Result<(u64, u64), StorageImportSessionError> {
        Self::bucket_in(disk, StorageImportSessionDiskEnumFile::StructuralIndex, key)
    }

    pub(crate) fn bucket_in(
        disk: &mut StorageImportSessionDisk,
        kind: StorageImportSessionDiskEnumFile,
        key: [u8; 32],
    ) -> Result<(u64, u64), StorageImportSessionError> {
        let count = Self::count(disk, kind)?;
        let (mut low, mut high) = (0_u64, count);
        while low < high {
            let middle = low + (high - low) / 2;
            if Self::entry(disk, kind, middle)?.key < key {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let begin = low;
        high = count;
        while low < high {
            let middle = low + (high - low) / 2;
            if Self::entry(disk, kind, middle)?.key <= key {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        Ok((begin, low))
    }
}
