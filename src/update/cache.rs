use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::update::manifest::{UpdateManifest, UpdateManifestAsset, UpdateManifestError, UpdateTarget};
use crate::update::state::{UpdateAttemptId, UpdateCacheReceipt, UpdateReleaseIdentity};

#[derive(Debug, Error)]
pub enum UpdateCacheError {
    #[error("The update cache I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("The update cache receipt is not valid: {0}")]
    Receipt(String),
    #[error(transparent)]
    Package(#[from] UpdateManifestError),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateCacheWireReceipt {
    release_id: u64,
    asset_id: u64,
    asset_name: String,
    version: String,
    target: String,
    manifest_sha256: String,
    package_sha256: String,
    length: u64,
}

pub struct UpdateCache {
    root: PathBuf,
}

impl UpdateCache {
    const RECEIPT: &'static str = "receipt.json";
    const MAX_RECEIPT_BYTES: u64 = 16 * 1024;

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn partial_dir(&self) -> PathBuf {
        self.root.join("partial")
    }

    fn completed_dir(&self) -> PathBuf {
        self.root.join("completed")
    }

    pub fn begin_partial(&self, attempt: UpdateAttemptId) -> Result<PathBuf, UpdateCacheError> {
        self.cleanup_partials()?;
        fs::create_dir_all(self.partial_dir())?;
        Ok(self
            .partial_dir()
            .join(format!("{}.part", attempt.as_uuid())))
    }

    pub fn cleanup_partials(&self) -> Result<(), UpdateCacheError> {
        let directory = self.partial_dir();
        match fs::read_dir(&directory) {
            Ok(entries) => {
                for entry in entries {
                    fs::remove_file(entry?.path())?;
                }
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn sync_directory(path: &Path) -> Result<(), UpdateCacheError> {
        #[cfg(unix)]
        {
            File::open(path)?.sync_all()?;
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
        Ok(())
    }

    pub fn complete(
        &self,
        partial: &Path,
        asset: &UpdateManifestAsset,
        receipt: &UpdateCacheReceipt,
    ) -> Result<PathBuf, UpdateCacheError> {
        asset.revalidate(partial)?;
        File::open(partial)?.sync_all()?;
        let completed = self.completed_dir();
        fs::create_dir_all(&completed)?;
        let package = completed.join(asset.name());
        fs::rename(partial, &package)?;
        File::open(&package)?.sync_all()?;
        let wire = UpdateCacheWireReceipt {
            release_id: receipt.release.release_id,
            asset_id: receipt.release.asset_id,
            asset_name: receipt.release.asset_name.clone(),
            version: receipt.release.version.to_string(),
            target: receipt.release.target.to_string(),
            manifest_sha256: UpdateManifest::encode_hex(&receipt.manifest_digest),
            package_sha256: UpdateManifest::encode_hex(&receipt.package_sha256),
            length: receipt.length,
        };
        let temporary = completed.join("receipt.json.tmp");
        let mut file = File::create(&temporary)?;
        file.write_all(
            &serde_json::to_vec(&wire).map_err(|error| UpdateCacheError::Receipt(error.to_string()))?,
        )?;
        file.sync_all()?;
        fs::rename(&temporary, completed.join(Self::RECEIPT))?;
        Self::sync_directory(&completed)?;
        for entry in fs::read_dir(&completed)? {
            let path = entry?.path();
            let keep = path.file_name().is_some_and(|name| {
                name == Self::RECEIPT || name == asset.name()
            });
            if !keep {
                fs::remove_file(path)?;
            }
        }
        Ok(package)
    }

    pub fn completed(&self) -> Result<Option<(UpdateCacheReceipt, PathBuf)>, UpdateCacheError> {
        let path = self.completed_dir().join(Self::RECEIPT);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > Self::MAX_RECEIPT_BYTES {
            return Err(UpdateCacheError::Receipt("the receipt is too large".to_owned()));
        }
        let wire: UpdateCacheWireReceipt = serde_json::from_slice(&bytes)
            .map_err(|error| UpdateCacheError::Receipt(error.to_string()))?;
        let invalid = |what: &str| UpdateCacheError::Receipt(what.to_owned());
        let version = Version::parse(&wire.version).map_err(|_error| invalid("version"))?;
        let target = wire.target.parse::<UpdateTarget>().map_err(|_error| invalid("target"))?;
        let manifest_digest = UpdateManifest::hex32(&wire.manifest_sha256).ok_or_else(|| invalid("manifest digest"))?;
        let package_sha256 = UpdateManifest::hex32(&wire.package_sha256).ok_or_else(|| invalid("package digest"))?;
        if wire.asset_name.contains(['/', '\\']) || wire.asset_name == ".." {
            return Err(invalid("asset name"));
        }
        let package = self.completed_dir().join(&wire.asset_name);
        if fs::metadata(&package)?.len() != wire.length {
            return Err(invalid("package length"));
        }
        let receipt = UpdateCacheReceipt {
            release: UpdateReleaseIdentity {
                release_id: wire.release_id,
                asset_id: wire.asset_id,
                version,
                target,
                asset_name: wire.asset_name,
            },
            manifest_digest,
            package_sha256,
            length: wire.length,
        };
        Ok(Some((receipt, package)))
    }

    pub fn consume_after_verified_startup(
        &self,
        installed: &Version,
    ) -> Result<bool, UpdateCacheError> {
        let Some((receipt, _package)) = self.completed()? else {
            return Ok(false);
        };
        if receipt.release.version > *installed {
            return Ok(false);
        }
        fs::remove_dir_all(self.completed_dir())?;
        Ok(true)
    }
}
