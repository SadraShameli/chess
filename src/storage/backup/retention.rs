use std::fs;
use std::path::{Path, PathBuf};

use crate::storage::backup::archive::BackupArchiveFormat;
use crate::storage::backup::error::BackupError;
use strum::IntoEnumIterator;

use crate::storage::backup::manifest::{BackupEnumCategory, BackupManifest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRetentionEntry {
    pub path: PathBuf,
    pub manifest: BackupManifest,
    pub recovery_protected: bool,
}

pub struct BackupRetention;

impl BackupRetention {
    pub const AUTOMATIC_LIMIT: usize = 30;
    pub const SAFETY_LIMIT: usize = 5;

    pub fn scan_blocking(directory: &Path) -> Result<Vec<BackupRetentionEntry>, BackupError> {
        let mut entries = Vec::new();
        if !directory.is_dir() {
            return Ok(entries);
        }
        for item in fs::read_dir(directory)? {
            let path = item?.path();
            if path.extension().and_then(|value| value.to_str())
                != Some(BackupArchiveFormat::EXTENSION)
            {
                continue;
            }
            let mut file = fs::File::open(&path)?;
            if let Ok((manifest, _offset)) = BackupArchiveFormat::read_header(&mut file) {
                entries.push(BackupRetentionEntry {
                    path,
                    manifest,
                    recovery_protected: false,
                });
            }
        }
        entries.sort_by(|left, right| {
            right
                .manifest
                .created_ms()
                .cmp(&left.manifest.created_ms())
                .then_with(|| right.manifest.id().cmp(&left.manifest.id()))
        });
        Ok(entries)
    }

    pub fn expired(entries: &[BackupRetentionEntry]) -> Vec<&BackupRetentionEntry> {
        let mut expired = Vec::new();
        for category in BackupEnumCategory::iter() {
            let limit = match category {
                BackupEnumCategory::Manual => continue,
                BackupEnumCategory::Automatic => Self::AUTOMATIC_LIMIT,
                BackupEnumCategory::PreRestore | BackupEnumCategory::PreUpdate => {
                    Self::SAFETY_LIMIT
                }
            };
            expired.extend(
                entries
                    .iter()
                    .filter(|entry| entry.manifest.category() == category)
                    .skip(limit)
                    .filter(|entry| !entry.recovery_protected),
            );
        }
        expired
    }

    pub fn apply_blocking(
        directory: &Path,
        protected: &[uuid::Uuid],
    ) -> Result<Vec<PathBuf>, BackupError> {
        let mut entries = Self::scan_blocking(directory)?;
        for entry in &mut entries {
            entry.recovery_protected = protected.contains(&entry.manifest.id());
        }
        let mut removed = Vec::new();
        for entry in Self::expired(&entries) {
            fs::remove_file(&entry.path)?;
            removed.push(entry.path.clone());
        }
        Ok(removed)
    }
}
