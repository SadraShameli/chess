use std::io::{Read, Write};

use crate::storage::backup::error::BackupError;
use crate::storage::backup::manifest::BackupManifest;

pub struct BackupArchiveFormat;

impl BackupArchiveFormat {
    pub const MAGIC: [u8; 8] = *b"CHESSBK1";
    pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;
    pub const EXTENSION: &'static str = "chessbackup";

    pub fn write_header(
        writer: &mut impl Write,
        manifest: &BackupManifest,
    ) -> Result<u64, BackupError> {
        let encoded = serde_json::to_vec(manifest)?;
        if encoded.len() > Self::MAX_MANIFEST_BYTES {
            return Err(BackupError::InvalidArchive("manifest size"));
        }
        let length = u32::try_from(encoded.len())
            .map_err(|_error| BackupError::InvalidArchive("manifest size"))?;
        writer.write_all(&Self::MAGIC)?;
        writer.write_all(&length.to_le_bytes())?;
        writer.write_all(&encoded)?;
        Ok(Self::MAGIC.len() as u64 + 4 + u64::from(length))
    }

    pub fn read_header(reader: &mut impl Read) -> Result<(BackupManifest, u64), BackupError> {
        let mut magic = [0_u8; 8];
        reader
            .read_exact(&mut magic)
            .map_err(|_error| BackupError::InvalidArchive("missing signature"))?;
        if magic != Self::MAGIC {
            return Err(BackupError::InvalidArchive("signature"));
        }
        let mut length = [0_u8; 4];
        reader
            .read_exact(&mut length)
            .map_err(|_error| BackupError::InvalidArchive("missing manifest length"))?;
        let length = u32::from_le_bytes(length);
        let size = usize::try_from(length)
            .map_err(|_error| BackupError::InvalidArchive("manifest size"))?;
        if size == 0 || size > Self::MAX_MANIFEST_BYTES {
            return Err(BackupError::InvalidArchive("manifest size"));
        }
        let mut encoded = vec![0_u8; size];
        reader
            .read_exact(&mut encoded)
            .map_err(|_error| BackupError::InvalidArchive("truncated manifest"))?;
        let manifest: BackupManifest = serde_json::from_slice(&encoded)?;
        manifest.validate()?;
        Ok((manifest, Self::MAGIC.len() as u64 + 4 + u64::from(length)))
    }
}
