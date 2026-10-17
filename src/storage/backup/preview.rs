use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::package::digest::PackageDigest;
use crate::storage::backup::archive::BackupArchiveFormat;
use crate::storage::backup::error::BackupError;
use crate::storage::backup::manifest::{BackupDependency, BackupEnumPlatform, BackupManifest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupPreviewEnumFinding {
    CrossPlatform {
        archive: BackupEnumPlatform,
        destination: BackupEnumPlatform,
    },
    DestinationLocationsKept {
        stored: Vec<String>,
    },
    DependencyUnavailable(BackupDependency),
    OlderSchemaWillMigrate {
        pending: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupPreview {
    path: PathBuf,
    manifest: BackupManifest,
    payload_offset: u64,
    findings: Vec<BackupPreviewEnumFinding>,
}

pub struct BackupPreviewContext<'a> {
    pub known_schema_versions: &'a [String],
    pub destination_platform: BackupEnumPlatform,
    pub available_dependencies: &'a [BackupDependency],
}

impl BackupPreview {
    pub fn inspect_blocking(
        path: &Path,
        context: &BackupPreviewContext<'_>,
    ) -> Result<Self, BackupError> {
        let metadata = path.symlink_metadata()?;
        if !metadata.is_file() {
            return Err(BackupError::InvalidArchive("not a regular file"));
        }
        let mut file = File::open(path)?;
        let (manifest, payload_offset) = BackupArchiveFormat::read_header(&mut file)?;
        let expected = payload_offset
            .checked_add(manifest.payload_bytes())
            .ok_or(BackupError::InvalidArchive("archive size"))?;
        if metadata.len() != expected {
            return Err(BackupError::InvalidArchive("payload length"));
        }
        let digest = PackageDigest::read(file.take(manifest.payload_bytes()))?;
        if digest != manifest.payload_sha256() {
            return Err(BackupError::ChecksumMismatch);
        }
        let findings = Self::findings(&manifest, context)?;
        Ok(Self {
            path: path.to_path_buf(),
            manifest,
            payload_offset,
            findings,
        })
    }

    fn findings(
        manifest: &BackupManifest,
        context: &BackupPreviewContext<'_>,
    ) -> Result<Vec<BackupPreviewEnumFinding>, BackupError> {
        if let Some(unknown) = manifest
            .schema_versions()
            .iter()
            .find(|version| !context.known_schema_versions.contains(version))
        {
            return Err(BackupError::UnsupportedSchema {
                version: unknown.clone(),
            });
        }
        let mut findings = Vec::new();
        if manifest.platform() != context.destination_platform {
            findings.push(BackupPreviewEnumFinding::CrossPlatform {
                archive: manifest.platform(),
                destination: context.destination_platform,
            });
        }
        if !manifest.portable_paths().is_empty() {
            findings.push(BackupPreviewEnumFinding::DestinationLocationsKept {
                stored: manifest.portable_paths().to_vec(),
            });
        }
        for dependency in manifest.dependencies() {
            if !context.available_dependencies.contains(dependency) {
                findings.push(BackupPreviewEnumFinding::DependencyUnavailable(
                    dependency.clone(),
                ));
            }
        }
        let pending: Vec<String> = context
            .known_schema_versions
            .iter()
            .filter(|version| !manifest.schema_versions().contains(version))
            .cloned()
            .collect();
        if !pending.is_empty() {
            findings.push(BackupPreviewEnumFinding::OlderSchemaWillMigrate { pending });
        }
        Ok(findings)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }

    pub const fn payload_offset(&self) -> u64 {
        self.payload_offset
    }

    pub fn findings(&self) -> &[BackupPreviewEnumFinding] {
        &self.findings
    }

    pub const fn id(&self) -> Uuid {
        self.manifest.id()
    }

    pub const fn digest(&self) -> PackageDigest {
        self.manifest.payload_sha256()
    }

    pub fn open_payload(&self) -> Result<impl Read, BackupError> {
        use std::io::{Seek, SeekFrom};
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(self.payload_offset))?;
        Ok(file.take(self.manifest.payload_bytes()))
    }
}
