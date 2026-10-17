use std::io::Read;
use std::path::Path;

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::update::verifier::{UpdateSignatureVerifier, UpdateVerifierError};

#[derive(Debug, Error)]
pub enum UpdateManifestError {
    #[error(transparent)]
    Signature(#[from] UpdateVerifierError),
    #[error("The manifest exceeds {0} bytes")]
    TooLarge(usize),
    #[error("The manifest is not valid: {0}")]
    Document(String),
    #[error("The manifest schema {0} is unsupported")]
    Schema(u32),
    #[error("The manifest channel {0:?} is not stable")]
    Channel(String),
    #[error("The manifest version {0:?} is not a canonical stable version")]
    Version(String),
    #[error("The manifest asset list is empty, too long or repeats an identity")]
    Assets,
    #[error("The manifest asset {0:?} is invalid")]
    Asset(String),
    #[error("The package length or digest does not match the signed manifest")]
    PackageMismatch,
    #[error("The package could not be read: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    strum::EnumString,
    strum::AsRefStr,
    strum::Display,
)]
pub enum UpdateTarget {
    #[strum(serialize = "macos-arm64")]
    MacosArm64,
    #[strum(serialize = "windows-x86_64")]
    WindowsX86_64,
    #[strum(serialize = "linux-x86_64-appimage")]
    LinuxAppImage,
    #[strum(serialize = "linux-x86_64-deb")]
    LinuxDeb,
}

impl UpdateTarget {
    pub fn detect() -> Option<Self> {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Some(Self::MacosArm64)
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Some(Self::WindowsX86_64)
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            if std::env::var_os("APPIMAGE").is_some() {
                Some(Self::LinuxAppImage)
            } else {
                Some(Self::LinuxDeb)
            }
        } else {
            None
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateManifestWire {
    schema: u32,
    channel: String,
    version: String,
    assets: Vec<UpdateManifestWireAsset>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateManifestWireAsset {
    id: u64,
    name: String,
    target: String,
    length: u64,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateManifestAsset {
    id: u64,
    name: String,
    target: UpdateTarget,
    length: u64,
    sha256: [u8; 32],
}

impl UpdateManifestAsset {
    pub const fn id(&self) -> u64 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn target(&self) -> UpdateTarget {
        self.target
    }

    pub const fn length(&self) -> u64 {
        self.length
    }

    pub const fn sha256(&self) -> [u8; 32] {
        self.sha256
    }

    pub fn revalidate(&self, path: &Path) -> Result<(), UpdateManifestError> {
        let mut file = std::fs::File::open(path)?;
        if file.metadata()?.len() != self.length {
            return Err(UpdateManifestError::PackageMismatch);
        }
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut total = 0_u64;
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            total = total
                .checked_add(u64::try_from(read).map_err(|_error| UpdateManifestError::PackageMismatch)?)
                .ok_or(UpdateManifestError::PackageMismatch)?;
            if total > self.length {
                return Err(UpdateManifestError::PackageMismatch);
            }
            hasher.update(&buffer[..read]);
        }
        let digest: [u8; 32] = hasher.finalize().into();
        if total != self.length || digest != self.sha256 {
            return Err(UpdateManifestError::PackageMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateManifest {
    version: Version,
    assets: Vec<UpdateManifestAsset>,
    digest: [u8; 32],
}

impl UpdateManifest {
    pub const MAX_BYTES: usize = 1024 * 1024;
    pub const MAX_ASSETS: usize = 16;
    pub const MAX_PACKAGE_BYTES: u64 = 6 * 1024 * 1024 * 1024;
    pub const SCHEMA: u32 = 1;

    pub fn verify_and_parse(
        verifier: &impl UpdateSignatureVerifier,
        manifest: &[u8],
        signature: &[u8],
    ) -> Result<Self, UpdateManifestError> {
        if manifest.len() > Self::MAX_BYTES {
            return Err(UpdateManifestError::TooLarge(Self::MAX_BYTES));
        }
        verifier.verify(manifest, signature)?;
        Self::parse(manifest)
    }

    fn parse(bytes: &[u8]) -> Result<Self, UpdateManifestError> {
        let wire: UpdateManifestWire = serde_json::from_slice(bytes)
            .map_err(|error| UpdateManifestError::Document(error.to_string()))?;
        if wire.schema != Self::SCHEMA {
            return Err(UpdateManifestError::Schema(wire.schema));
        }
        if wire.channel != "stable" {
            return Err(UpdateManifestError::Channel(wire.channel));
        }
        let version = Version::parse(&wire.version)
            .map_err(|_error| UpdateManifestError::Version(wire.version.clone()))?;
        if !version.pre.is_empty()
            || !version.build.is_empty()
            || version.to_string() != wire.version
        {
            return Err(UpdateManifestError::Version(wire.version));
        }
        if wire.assets.is_empty() || wire.assets.len() > Self::MAX_ASSETS {
            return Err(UpdateManifestError::Assets);
        }
        let mut assets: Vec<UpdateManifestAsset> = Vec::with_capacity(wire.assets.len());
        for asset in wire.assets {
            let converted = Self::asset(asset)?;
            if assets
                .iter()
                .any(|seen| seen.id == converted.id || seen.target == converted.target)
            {
                return Err(UpdateManifestError::Assets);
            }
            assets.push(converted);
        }
        Ok(Self {
            version,
            assets,
            digest: Sha256::digest(bytes).into(),
        })
    }

    fn asset(wire: UpdateManifestWireAsset) -> Result<UpdateManifestAsset, UpdateManifestError> {
        let invalid = || UpdateManifestError::Asset(wire.name.clone());
        if wire.name.is_empty()
            || wire.name.len() > 255
            || wire
                .name
                .chars()
                .any(|character| character == '/' || character == '\\' || character.is_control())
            || wire.name == "."
            || wire.name == ".."
        {
            return Err(invalid());
        }
        if wire.length == 0 || wire.length > Self::MAX_PACKAGE_BYTES {
            return Err(invalid());
        }
        let target = wire.target.parse::<UpdateTarget>().map_err(|_error| invalid())?;
        let sha256 = Self::hex32(&wire.sha256).ok_or_else(invalid)?;
        Ok(UpdateManifestAsset {
            id: wire.id,
            name: wire.name.clone(),
            target,
            length: wire.length,
            sha256,
        })
    }

    pub(crate) fn hex32(text: &str) -> Option<[u8; 32]> {
        if text.len() != 64
            || !text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return None;
        }
        let mut out = [0_u8; 32];
        for (slot, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
            let high = char::from(pair[0]).to_digit(16)?;
            let low = char::from(pair[1]).to_digit(16)?;
            *slot = u8::try_from(high * 16 + low).ok()?;
        }
        Some(out)
    }

    pub fn encode_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub const fn version(&self) -> &Version {
        &self.version
    }

    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }

    pub fn assets(&self) -> &[UpdateManifestAsset] {
        &self.assets
    }

    pub fn asset_for(&self, target: UpdateTarget) -> Option<&UpdateManifestAsset> {
        self.assets.iter().find(|asset| asset.target == target)
    }

    pub fn require_newer(&self, installed: &Version) -> Result<(), UpdateManifestError> {
        if self.version > *installed {
            Ok(())
        } else {
            Err(UpdateManifestError::Version(self.version.to_string()))
        }
    }
}
