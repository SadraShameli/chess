use crate::package::digest::PackageDigest;
use crate::storage::dataset_package::StorageDatasetPackageSourceIdentity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningBookIdentity {
    source: StorageDatasetPackageSourceIdentity,
    checksum: PackageDigest,
}

impl OpeningBookIdentity {
    pub(super) const fn new(
        source: StorageDatasetPackageSourceIdentity,
        checksum: PackageDigest,
    ) -> Self {
        Self { source, checksum }
    }

    pub const fn source(&self) -> &StorageDatasetPackageSourceIdentity {
        &self.source
    }

    pub const fn checksum(&self) -> PackageDigest {
        self.checksum
    }
}
