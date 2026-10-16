use std::fs::File;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::package::digest::{PackageDigest, PackageDigestError};

#[derive(Debug, Error)]
pub enum DesktopNativeResourcesError {
    #[error("The native resource directory must be absolute: {0}")]
    Relative(PathBuf),
    #[error("Native resource access failed at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("The native resource is not a regular file inside its package: {0}")]
    InvalidFile(PathBuf),
    #[error("The native asset does not match its packaged provenance: {0}")]
    Integrity(PathBuf),
    #[error("The bundled resource provenance is invalid: {0}")]
    Provenance(#[from] serde_json::Error),
    #[error(transparent)]
    Digest(#[from] PackageDigestError),
}

#[derive(Deserialize)]
struct DesktopNativeResourcesAsset {
    sha256: PackageDigest,
    bytes: u64,
}

#[derive(Deserialize)]
struct DesktopNativeResourcesPieceManifest {
    asset: DesktopNativeResourcesAsset,
}

#[derive(Debug, Clone)]
pub struct DesktopNativeResources {
    root: PathBuf,
    pieces: PathBuf,
    engine: PathBuf,
}

impl DesktopNativeResources {
    pub const PIECES_ASSET: &str = "pieces/chess.glb";

    pub fn from_directory(directory: &Path) -> Result<Self, DesktopNativeResourcesError> {
        if !directory.is_absolute() {
            return Err(DesktopNativeResourcesError::Relative(
                directory.to_path_buf(),
            ));
        }
        let root = directory
            .canonicalize()
            .map_err(|source| Self::io(directory, source))?;
        let pieces = Self::file(&root, Self::PIECES_ASSET)?;
        let engine = root.join("engine");
        let piece_manifest: DesktopNativeResourcesPieceManifest =
            serde_json::from_str(include_str!("../../../resources/pieces/provenance.json"))?;
        Self::verify(&pieces, &piece_manifest.asset)?;
        Ok(Self {
            root,
            pieces,
            engine,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn pieces(&self) -> &Path {
        &self.pieces
    }

    pub fn engine(&self) -> &Path {
        &self.engine
    }

    fn file(root: &Path, relative: &str) -> Result<PathBuf, DesktopNativeResourcesError> {
        let path = root.join(relative);
        let canonical = path
            .canonicalize()
            .map_err(|source| Self::io(&path, source))?;
        let metadata = canonical
            .metadata()
            .map_err(|source| Self::io(&canonical, source))?;
        if !canonical.starts_with(root) || !metadata.is_file() {
            return Err(DesktopNativeResourcesError::InvalidFile(path));
        }
        Ok(canonical)
    }

    fn verify(
        path: &Path,
        expected: &DesktopNativeResourcesAsset,
    ) -> Result<(), DesktopNativeResourcesError> {
        let file = File::open(path).map_err(|source| Self::io(path, source))?;
        let bytes = file
            .metadata()
            .map_err(|source| Self::io(path, source))?
            .len();
        if bytes != expected.bytes || PackageDigest::read(file)? != expected.sha256 {
            return Err(DesktopNativeResourcesError::Integrity(path.to_path_buf()));
        }
        Ok(())
    }

    fn io(path: &Path, source: std::io::Error) -> DesktopNativeResourcesError {
        DesktopNativeResourcesError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}
