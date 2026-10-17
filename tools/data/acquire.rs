use std::fs::File;
use std::io;
use std::path::Path;

use chess::package::digest::PackageDigest;
use chess::storage::dataset_sources::{StorageDatasetSources, StorageDatasetSourcesError};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ToolsDataAcquireError {
    #[error("Reading the source manifest failed: {0}")]
    Manifest(#[from] io::Error),
    #[error(transparent)]
    Sources(#[from] StorageDatasetSourcesError),
}

pub(crate) struct ToolsDataAcquire;

impl ToolsDataAcquire {
    pub(crate) fn verify(sources: &Path, cache: &Path) -> Result<usize, ToolsDataAcquireError> {
        let manifest = StorageDatasetSources::from_json(&std::fs::read(sources)?)?;
        manifest.verify(|entry| {
            let path = cache.join(&entry.file);
            if !path.is_file() {
                return Ok(None);
            }
            PackageDigest::read(File::open(path)?)
                .map(Some)
                .map_err(|error| io::Error::other(error.to_string()))
        })?;
        Ok(manifest.sources().len())
    }
}
