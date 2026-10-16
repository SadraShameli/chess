use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::package::digest::{PackageDigest, PackageDigestError};

#[derive(Debug, Error)]
pub enum EnginePackageManifestError {
    #[error("This operating system or architecture is outside the supported desktop targets")]
    UnsupportedHost,
    #[error("Engine provenance does not match the pinned source and network")]
    PinMismatch,
    #[error("The engine manifest has an unsupported schema or invalid package entry")]
    InvalidManifest,
    #[error("Cannot decode the engine manifest: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Digest(#[from] PackageDigestError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnginePackageEnumTarget {
    #[serde(rename = "aarch64-apple-darwin")]
    MacosArm,
    #[serde(rename = "x86_64-pc-windows-msvc")]
    WindowsX64,
    #[serde(rename = "x86_64-unknown-linux-gnu")]
    LinuxX64,
}

impl EnginePackageEnumTarget {
    pub fn current() -> Result<Self, EnginePackageManifestError> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => Ok(Self::MacosArm),
            ("windows", "x86_64") => Ok(Self::WindowsX64),
            ("linux", "x86_64") => Ok(Self::LinuxX64),
            _ => Err(EnginePackageManifestError::UnsupportedHost),
        }
    }

    pub const fn triple(self) -> &'static str {
        match self {
            Self::MacosArm => "aarch64-apple-darwin",
            Self::WindowsX64 => "x86_64-pc-windows-msvc",
            Self::LinuxX64 => "x86_64-unknown-linux-gnu",
        }
    }

    pub fn filename(self) -> String {
        let extension = match self {
            Self::WindowsX64 => ".exe",
            Self::MacosArm | Self::LinuxX64 => "",
        };
        format!("reckless-{}{extension}", self.triple())
    }

    pub fn relative_path(self) -> PathBuf {
        PathBuf::from("bin").join(self.filename())
    }

    pub const fn portable_flags(self) -> &'static str {
        match self {
            Self::MacosArm => "-C\x1ftarget-cpu=generic\x1f-C\x1ftarget-feature=+neon",
            Self::WindowsX64 | Self::LinuxX64 => "-C\x1ftarget-cpu=x86-64",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnginePackageEnumProfile {
    Portable,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(remote = "Self")]
pub struct EnginePackageEntry {
    target: EnginePackageEnumTarget,
    profile: EnginePackageEnumProfile,
    executable: PathBuf,
    bytes: u64,
    sha256: PackageDigest,
    compiler: String,
    syzygy: bool,
}

impl EnginePackageEntry {
    pub fn portable(
        target: EnginePackageEnumTarget,
        bytes: u64,
        sha256: PackageDigest,
        compiler: String,
    ) -> Result<Self, EnginePackageManifestError> {
        let entry = Self {
            target,
            profile: EnginePackageEnumProfile::Portable,
            executable: target.relative_path(),
            bytes,
            sha256,
            compiler,
            syzygy: true,
        };
        entry.validate()?;
        Ok(entry)
    }

    pub fn target(&self) -> EnginePackageEnumTarget {
        self.target
    }
    pub fn profile(&self) -> EnginePackageEnumProfile {
        self.profile
    }
    pub fn executable(&self) -> &std::path::Path {
        &self.executable
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn sha256(&self) -> PackageDigest {
        self.sha256
    }
    pub fn has_syzygy(&self) -> bool {
        self.syzygy
    }

    fn validate(&self) -> Result<(), EnginePackageManifestError> {
        if self.executable != self.target.relative_path()
            || self.bytes == 0
            || !self.syzygy
            || self.compiler.is_empty()
            || self.compiler.len() > 4096
        {
            return Err(EnginePackageManifestError::InvalidManifest);
        }
        Ok(())
    }
}

impl Serialize for EnginePackageEntry {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Self::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for EnginePackageEntry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entry = Self::deserialize(deserializer)?;
        entry.validate().map_err(serde::de::Error::custom)?;
        Ok(entry)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(remote = "Self")]
pub struct EnginePackageManifest {
    schema_version: u32,
    version: String,
    source_commit: String,
    network_name: String,
    network_bytes: u64,
    network_sha256: PackageDigest,
    packages: Vec<EnginePackageEntry>,
}

impl EnginePackageManifest {
    pub const REPOSITORY: &str = "https://github.com/codedeliveryservice/Reckless.git";
    pub const TAG: &str = "v0.9.0";
    pub const VERSION: &str = "0.9.0";
    pub const COMMIT: &str = "0e92358f5acd66e5ac77b1bf558202e47c515435";
    pub const NETWORK_NAME: &str = "v54-5478683c.nnue";
    pub const NETWORK_BYTES: u64 = 63_266_880;
    pub const NETWORK_SHA256: &str =
        "5478683cb1bababde29ae8f29468a99846726548fc6a0ed54cac40ab6d38efbf";
    const SCHEMA_VERSION: u32 = 1;

    pub fn network_url() -> String {
        format!(
            "https://github.com/codedeliveryservice/RecklessNetworks/releases/download/networks/{}",
            Self::NETWORK_NAME
        )
    }

    pub fn new(entry: EnginePackageEntry) -> Result<Self, EnginePackageManifestError> {
        entry.validate()?;
        Ok(Self {
            schema_version: Self::SCHEMA_VERSION,
            version: Self::VERSION.to_owned(),
            source_commit: Self::COMMIT.to_owned(),
            network_name: Self::NETWORK_NAME.to_owned(),
            network_bytes: Self::NETWORK_BYTES,
            network_sha256: Self::NETWORK_SHA256.parse()?,
            packages: vec![entry],
        })
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, EnginePackageManifestError> {
        Ok(serde_json::from_slice(bytes)?)
    }

    pub fn validate(&self) -> Result<(), EnginePackageManifestError> {
        if self.version != Self::VERSION
            || self.source_commit != Self::COMMIT
            || self.network_name != Self::NETWORK_NAME
            || self.network_bytes != Self::NETWORK_BYTES
            || self.network_sha256.to_string() != Self::NETWORK_SHA256
        {
            return Err(EnginePackageManifestError::PinMismatch);
        }
        if self.schema_version != Self::SCHEMA_VERSION
            || self.packages.is_empty()
            || self.packages.len() > 3
        {
            return Err(EnginePackageManifestError::InvalidManifest);
        }
        for (index, entry) in self.packages.iter().enumerate() {
            entry.validate()?;
            if self.packages[..index].iter().any(|previous| {
                previous.target == entry.target && previous.profile == entry.profile
            }) {
                return Err(EnginePackageManifestError::InvalidManifest);
            }
        }
        Ok(())
    }

    pub fn entries(&self) -> &[EnginePackageEntry] {
        &self.packages
    }

    pub fn replace(&mut self, entry: EnginePackageEntry) {
        self.packages.retain(|previous| {
            previous.target != entry.target || previous.profile != entry.profile
        });
        self.packages.push(entry);
    }
}

impl Serialize for EnginePackageManifest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Self::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for EnginePackageManifest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let manifest = Self::deserialize(deserializer)?;
        manifest.validate().map_err(serde::de::Error::custom)?;
        Ok(manifest)
    }
}
