use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::engine::package::platform::{
    EnginePackagePlatformEnumFeature, EnginePackagePlatformEnumOsState,
};
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

    pub fn profile_path(self, profile: EnginePackageEnumProfile) -> PathBuf {
        match profile.file_suffix() {
            None => self.relative_path(),
            Some(suffix) => {
                let extension = match self {
                    Self::WindowsX64 => ".exe",
                    Self::MacosArm | Self::LinuxX64 => "",
                };
                PathBuf::from("bin").join(format!(
                    "reckless-{}-{suffix}{extension}",
                    self.triple()
                ))
            }
        }
    }

    pub const fn supports_profile(self, profile: EnginePackageEnumProfile) -> bool {
        match (self, profile) {
            (_, EnginePackageEnumProfile::Portable) => true,
            (
                Self::WindowsX64 | Self::LinuxX64,
                EnginePackageEnumProfile::X8664V3 | EnginePackageEnumProfile::X8664V4,
            ) => true,
            (Self::LinuxX64, EnginePackageEnumProfile::Numa) => true,
            _ => false,
        }
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
    #[serde(rename = "x86_64_v3")]
    X8664V3,
    #[serde(rename = "x86_64_v4")]
    X8664V4,
    Numa,
}

impl EnginePackageEnumProfile {
    const V3_FEATURES: [EnginePackagePlatformEnumFeature; 6] = [
        EnginePackagePlatformEnumFeature::Avx2,
        EnginePackagePlatformEnumFeature::Bmi1,
        EnginePackagePlatformEnumFeature::Bmi2,
        EnginePackagePlatformEnumFeature::Fma,
        EnginePackagePlatformEnumFeature::Lzcnt,
        EnginePackagePlatformEnumFeature::Movbe,
    ];
    const V4_FEATURES: [EnginePackagePlatformEnumFeature; 11] = [
        EnginePackagePlatformEnumFeature::Avx2,
        EnginePackagePlatformEnumFeature::Bmi1,
        EnginePackagePlatformEnumFeature::Bmi2,
        EnginePackagePlatformEnumFeature::Fma,
        EnginePackagePlatformEnumFeature::Lzcnt,
        EnginePackagePlatformEnumFeature::Movbe,
        EnginePackagePlatformEnumFeature::Avx512f,
        EnginePackagePlatformEnumFeature::Avx512bw,
        EnginePackagePlatformEnumFeature::Avx512cd,
        EnginePackagePlatformEnumFeature::Avx512dq,
        EnginePackagePlatformEnumFeature::Avx512vl,
    ];
    const V3_STATES: [EnginePackagePlatformEnumOsState; 1] =
        [EnginePackagePlatformEnumOsState::YmmEnabled];
    const V4_STATES: [EnginePackagePlatformEnumOsState; 2] = [
        EnginePackagePlatformEnumOsState::YmmEnabled,
        EnginePackagePlatformEnumOsState::ZmmEnabled,
    ];
    pub const ALL: [Self; 4] = [Self::Portable, Self::X8664V3, Self::X8664V4, Self::Numa];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Portable => "Portable baseline",
            Self::X8664V3 => "x86-64-v3",
            Self::X8664V4 => "x86-64-v4",
            Self::Numa => "Linux NUMA",
        }
    }

    pub const fn file_suffix(self) -> Option<&'static str> {
        match self {
            Self::Portable => None,
            Self::X8664V3 => Some("x86-64-v3"),
            Self::X8664V4 => Some("x86-64-v4"),
            Self::Numa => Some("numa"),
        }
    }

    pub const fn rustflags(self) -> Option<&'static str> {
        match self {
            Self::Portable => None,
            Self::X8664V3 => Some("-C\x1ftarget-cpu=x86-64-v3"),
            Self::X8664V4 => Some("-C\x1ftarget-cpu=x86-64-v4"),
            Self::Numa => Some("-C\x1ftarget-cpu=x86-64"),
        }
    }

    pub const fn cargo_features(self) -> &'static [&'static str] {
        match self {
            Self::Portable | Self::X8664V3 | Self::X8664V4 => &["syzygy"],
            Self::Numa => &["syzygy", "numa"],
        }
    }

    pub const fn rank(self) -> u8 {
        match self {
            Self::Portable => 0,
            Self::Numa => 1,
            Self::X8664V3 => 2,
            Self::X8664V4 => 3,
        }
    }

    pub fn required_features(self) -> &'static [EnginePackagePlatformEnumFeature] {
        match self {
            Self::Portable | Self::Numa => &[],
            Self::X8664V3 => &Self::V3_FEATURES,
            Self::X8664V4 => &Self::V4_FEATURES,
        }
    }

    pub fn required_os_states(self) -> &'static [EnginePackagePlatformEnumOsState] {
        match self {
            Self::Portable | Self::Numa => &[],
            Self::X8664V3 => &Self::V3_STATES,
            Self::X8664V4 => &Self::V4_STATES,
        }
    }

    pub const fn requires_numa(self) -> bool {
        matches!(self, Self::Numa)
    }
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
    #[serde(default)]
    cargo_features: Vec<String>,
    #[serde(default)]
    rustflags: String,
}

impl EnginePackageEntry {
    pub fn profiled(
        target: EnginePackageEnumTarget,
        profile: EnginePackageEnumProfile,
        bytes: u64,
        sha256: PackageDigest,
        compiler: String,
    ) -> Result<Self, EnginePackageManifestError> {
        let entry = Self {
            target,
            profile,
            executable: target.profile_path(profile),
            bytes,
            sha256,
            compiler,
            syzygy: true,
            cargo_features: profile
                .cargo_features()
                .iter()
                .map(|feature| (*feature).to_owned())
                .collect(),
            rustflags: profile.rustflags().unwrap_or_default().to_owned(),
        };
        entry.validate()?;
        Ok(entry)
    }

    pub fn cargo_features(&self) -> &[String] {
        &self.cargo_features
    }

    pub fn rustflags(&self) -> &str {
        &self.rustflags
    }

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
            cargo_features: Vec::new(),
            rustflags: String::new(),
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
        if self.executable != self.target.profile_path(self.profile)
            || !self.target.supports_profile(self.profile)
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
    pub const MAX_PACKAGES: usize = 8;

    pub fn digest(bytes: &[u8]) -> PackageDigest {
        PackageDigest::hash_slice(bytes)
    }

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
            || self.packages.len() > Self::MAX_PACKAGES
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
