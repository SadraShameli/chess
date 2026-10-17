use std::io::Read;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::engine::package::host::EnginePackageHost;
use crate::engine::package::manifest::{
    EnginePackageEntry, EnginePackageEnumProfile, EnginePackageEnumTarget, EnginePackageManifest,
    EnginePackageManifestError,
};
use crate::engine::package::platform::EnginePackagePlatform;
use crate::package::digest::{PackageDigest, PackageDigestError};

#[derive(Debug, Error)]
pub enum EnginePackageSelectorError {
    #[error(transparent)]
    Manifest(#[from] EnginePackageManifestError),
    #[error(transparent)]
    Digest(#[from] PackageDigestError),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnginePackageEnumPreference {
    #[default]
    Automatic,
    Override(EnginePackageEnumProfile),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnginePackageFallback {
    requested: EnginePackageEnumProfile,
    reasons: Vec<String>,
}

impl EnginePackageFallback {
    pub const fn requested(&self) -> EnginePackageEnumProfile {
        self.requested
    }

    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }
}

#[derive(Debug, Clone)]
pub struct EnginePackageSelected {
    executable: PathBuf,
    target: EnginePackageEnumTarget,
    profile: EnginePackageEnumProfile,
    digest: PackageDigest,
    fallback: Option<EnginePackageFallback>,
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
    pub fn digest(&self) -> PackageDigest {
        self.digest
    }
    pub fn fallback(&self) -> Option<&EnginePackageFallback> {
        self.fallback.as_ref()
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
        Self::select_with(
            directory,
            manifest,
            host,
            &EnginePackagePlatform::detect(),
            EnginePackageEnumPreference::Automatic,
        )
    }

    pub fn incompatibilities(
        profile: EnginePackageEnumProfile,
        platform: &EnginePackagePlatform,
    ) -> Vec<String> {
        let mut reasons: Vec<String> = platform
            .missing_features(profile.required_features())
            .into_iter()
            .map(|feature| format!("the processor lacks {}", feature.label()))
            .collect();
        reasons.extend(
            platform
                .missing_os_states(profile.required_os_states())
                .into_iter()
                .map(|state| format!("{} is not enabled", state.label())),
        );
        if profile.requires_numa() {
            if platform.numa_nodes() < 2 {
                reasons.push("the host has fewer than two NUMA nodes".to_owned());
            }
            if !platform.has_libnuma() {
                reasons.push("libnuma is not installed".to_owned());
            }
        }
        reasons
    }

    pub fn select_with(
        directory: &Path,
        manifest: &EnginePackageManifest,
        host: EnginePackageHost,
        platform: &EnginePackagePlatform,
        preference: EnginePackageEnumPreference,
    ) -> Result<EnginePackageSelected, EnginePackageSelectorError> {
        let candidates: Vec<&EnginePackageEntry> = manifest
            .entries()
            .iter()
            .filter(|entry| entry.target() == host.target())
            .collect();
        let is_compatible = |entry: &&EnginePackageEntry| {
            Self::incompatibilities(entry.profile(), platform).is_empty()
        };
        let mut fallback = None;
        let mut chosen = None;
        if let EnginePackageEnumPreference::Override(requested) = preference {
            match candidates.iter().find(|entry| entry.profile() == requested) {
                Some(entry) if is_compatible(entry) => chosen = Some(*entry),
                Some(entry) => {
                    fallback = Some(EnginePackageFallback {
                        requested,
                        reasons: Self::incompatibilities(entry.profile(), platform),
                    });
                }
                None => {
                    fallback = Some(EnginePackageFallback {
                        requested,
                        reasons: vec!["the profile is not packaged for this host".to_owned()],
                    });
                }
            }
        }
        let entry = chosen
            .or_else(|| {
                candidates
                    .iter()
                    .filter(|entry| is_compatible(entry))
                    .max_by_key(|entry| entry.profile().rank())
                    .copied()
            })
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
        if metadata.len() != entry.bytes() || PackageDigest::read(file)? != entry.sha256() {
            return Err(EnginePackageSelectorError::ChecksumMismatch);
        }
        Ok(EnginePackageSelected {
            executable,
            target: entry.target(),
            profile: entry.profile(),
            digest: entry.sha256(),
            fallback,
        })
    }
}
