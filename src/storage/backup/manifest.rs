use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::package::digest::PackageDigest;
use crate::storage::backup::error::BackupError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupEnumCategory {
    Manual,
    Automatic,
    PreRestore,
    PreUpdate,
}

impl BackupEnumCategory {
    pub const ALL: [Self; 4] = [
        Self::Manual,
        Self::Automatic,
        Self::PreRestore,
        Self::PreUpdate,
    ];

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Automatic => "automatic",
            Self::PreRestore => "pre-restore",
            Self::PreUpdate => "pre-update",
        }
    }

    pub const fn is_safety(self) -> bool {
        matches!(self, Self::PreRestore | Self::PreUpdate)
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|category| category.slug() == slug)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupEnumPlatform {
    Macos,
    Windows,
    Linux,
}

impl BackupEnumPlatform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupEnumDependencyKind {
    EnginePackage,
    Dataset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupDependency {
    pub kind: BackupEnumDependencyKind,
    pub name: String,
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    format_version: u32,
    id: Uuid,
    category: BackupEnumCategory,
    created_ms: u64,
    platform: BackupEnumPlatform,
    schema_versions: Vec<String>,
    catalog_revision: String,
    payload_bytes: u64,
    payload_sha256: PackageDigest,
    dependencies: Vec<BackupDependency>,
    portable_paths: Vec<String>,
}

pub struct BackupManifestParts {
    pub id: Uuid,
    pub category: BackupEnumCategory,
    pub created_ms: u64,
    pub platform: BackupEnumPlatform,
    pub schema_versions: Vec<String>,
    pub catalog_revision: String,
    pub payload_bytes: u64,
    pub payload_sha256: PackageDigest,
    pub dependencies: Vec<BackupDependency>,
    pub portable_paths: Vec<String>,
}

impl BackupManifest {
    pub const FORMAT_VERSION: u32 = 1;
    pub const MAX_SCHEMA_VERSIONS: usize = 256;
    pub const MAX_DEPENDENCIES: usize = 1024;
    pub const MAX_PATHS: usize = 1024;
    pub const MAX_TEXT_BYTES: usize = 4096;
    pub const MAX_PAYLOAD_BYTES: u64 = 32 * 1024 * 1024 * 1024;

    pub fn new(parts: BackupManifestParts) -> Result<Self, BackupError> {
        let manifest = Self {
            format_version: Self::FORMAT_VERSION,
            id: parts.id,
            category: parts.category,
            created_ms: parts.created_ms,
            platform: parts.platform,
            schema_versions: parts.schema_versions,
            catalog_revision: parts.catalog_revision,
            payload_bytes: parts.payload_bytes,
            payload_sha256: parts.payload_sha256,
            dependencies: parts.dependencies,
            portable_paths: parts.portable_paths,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), BackupError> {
        if self.format_version != Self::FORMAT_VERSION {
            return Err(BackupError::UnsupportedFormat {
                found: self.format_version,
            });
        }
        if self.schema_versions.is_empty() || self.schema_versions.len() > Self::MAX_SCHEMA_VERSIONS
        {
            return Err(BackupError::InvalidArchive("schema version count"));
        }
        if self.dependencies.len() > Self::MAX_DEPENDENCIES
            || self.portable_paths.len() > Self::MAX_PATHS
        {
            return Err(BackupError::InvalidArchive("dependency or path count"));
        }
        let oversized = self
            .schema_versions
            .iter()
            .chain(self.portable_paths.iter())
            .chain(self.dependencies.iter().map(|dependency| &dependency.name))
            .chain(std::iter::once(&self.catalog_revision))
            .any(|text| text.len() > Self::MAX_TEXT_BYTES || text.contains('\0'));
        if oversized {
            return Err(BackupError::InvalidArchive("text field"));
        }
        if self.payload_bytes == 0 || self.payload_bytes > Self::MAX_PAYLOAD_BYTES {
            return Err(BackupError::InvalidArchive("payload size"));
        }
        Ok(())
    }

    pub const fn id(&self) -> Uuid {
        self.id
    }

    pub const fn category(&self) -> BackupEnumCategory {
        self.category
    }

    pub const fn created_ms(&self) -> u64 {
        self.created_ms
    }

    pub const fn platform(&self) -> BackupEnumPlatform {
        self.platform
    }

    pub fn schema_versions(&self) -> &[String] {
        &self.schema_versions
    }

    pub fn catalog_revision(&self) -> &str {
        &self.catalog_revision
    }

    pub const fn payload_bytes(&self) -> u64 {
        self.payload_bytes
    }

    pub const fn payload_sha256(&self) -> PackageDigest {
        self.payload_sha256
    }

    pub fn dependencies(&self) -> &[BackupDependency] {
        &self.dependencies
    }

    pub fn portable_paths(&self) -> &[String] {
        &self.portable_paths
    }
}
