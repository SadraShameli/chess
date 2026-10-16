use std::io::Read;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::engine::package::digest::{EnginePackageDigest, EnginePackageDigestError};
use crate::engine::package::host::EnginePackageHost;
use crate::engine::package::manifest::{
    EnginePackageEnumProfile, EnginePackageEnumTarget, EnginePackageManifest,
    EnginePackageManifestError,
};

#[derive(Debug, Error)]
pub enum EnginePackageSelectorError {
    #[error(transparent)]
    Manifest(#[from] EnginePackageManifestError),
    #[error(transparent)]
    Digest(#[from] EnginePackageDigestError),
    #[error("The packaged engine is unavailable: {0}")]
    Io(#[from] std::io::Error),
    #[error("No compatible engine package exists for this host")]
    MissingPackage,
    #[error("The packaged executable changed or failed its checksum")]
    ChecksumMismatch,
    #[error("The packaged engine must be a regular executable inside the application resources")]
    InvalidExecutable,
    #[error("The engine manifest exceeds its size bound")]
    OversizedManifest,
}

#[derive(Debug, Clone)]
pub struct EnginePackageSelected {
    executable: PathBuf,
    target: EnginePackageEnumTarget,
    profile: EnginePackageEnumProfile,
    digest: EnginePackageDigest,
}

impl EnginePackageSelected {
    pub fn executable(&self) -> &Path {
        &self.executable
    }
    pub fn target(&self) -> EnginePackageEnumTarget {
        self.target
    }
    pub fn profile(&self) -> EnginePackageEnumProfile {
        self.profile
    }
    pub fn digest(&self) -> EnginePackageDigest {
        self.digest
    }
}

pub struct EnginePackageSelector;

impl EnginePackageSelector {
    const MANIFEST_LIMIT: u64 = 64 * 1024;

    pub fn load(
        directory: &Path,
        host: EnginePackageHost,
    ) -> Result<EnginePackageSelected, EnginePackageSelectorError> {
        let mut bytes = Vec::new();
        std::fs::File::open(directory.join("manifest.json"))?
            .take(Self::MANIFEST_LIMIT + 1)
            .read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len())
            .map_err(|_error| EnginePackageSelectorError::OversizedManifest)?
            > Self::MANIFEST_LIMIT
        {
            return Err(EnginePackageSelectorError::OversizedManifest);
        }
        let manifest = EnginePackageManifest::parse(&bytes)?;
        Self::select(directory, &manifest, host)
    }

    pub fn select(
        directory: &Path,
        manifest: &EnginePackageManifest,
        host: EnginePackageHost,
    ) -> Result<EnginePackageSelected, EnginePackageSelectorError> {
        let entry = manifest
            .entries()
            .iter()
            .find(|entry| entry.target() == host.target())
            .ok_or(EnginePackageSelectorError::MissingPackage)?;
        let root = directory.canonicalize()?;
        let executable = root.join(entry.executable()).canonicalize()?;
        if !executable.starts_with(&root) {
            return Err(EnginePackageSelectorError::InvalidExecutable);
        }
        let file = std::fs::File::open(&executable)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(EnginePackageSelectorError::InvalidExecutable);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                return Err(EnginePackageSelectorError::InvalidExecutable);
            }
        }
        if metadata.len() != entry.bytes() || EnginePackageDigest::read(file)? != entry.sha256() {
            return Err(EnginePackageSelectorError::ChecksumMismatch);
        }
        Ok(EnginePackageSelected {
            executable,
            target: entry.target(),
            profile: entry.profile(),
            digest: entry.sha256(),
        })
    }
}
