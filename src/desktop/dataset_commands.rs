use diesel::SqliteConnection;
use thiserror::Error;

use crate::package::digest::PackageDigest;
use crate::storage::dataset_import::{
    StorageDatasetImport, StorageDatasetImportEnumConfirmation, StorageDatasetImportError,
    StorageDatasetImportPreview, StorageDatasetImportRequest,
};
use crate::storage::dataset_integrity::{
    StorageDatasetIntegrityEnumFeature, StorageDatasetIntegrityFinding,
    StorageDatasetIntegrityReport,
};
use crate::storage::dataset_package::{
    StorageDatasetPackageEnumLicense, StorageDatasetPackageEnumSourceKind,
    StorageDatasetPackageEnumValidation, StorageDatasetPackageManifest,
};
use crate::storage::dataset_registry::{
    StorageDatasetId, StorageDatasetRegistry, StorageDatasetRegistryEntry,
    StorageDatasetRegistryEnumKind,
    StorageDatasetRegistryEnumOrigin, StorageDatasetRegistryError,
};

#[derive(Debug, Error)]
pub enum DesktopDatasetCommandsError {
    #[error(transparent)]
    Registry(#[from] StorageDatasetRegistryError),
    #[error(transparent)]
    Import(#[from] StorageDatasetImportError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopDatasetStarter {
    pub source: String,
    pub version: String,
    pub kind: StorageDatasetPackageEnumSourceKind,
    pub license: String,
    pub origin: String,
    pub checksum: PackageDigest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopDatasetRow {
    pub id: StorageDatasetId,
    pub kind: StorageDatasetRegistryEnumKind,
    pub origin: StorageDatasetRegistryEnumOrigin,
    pub name: String,
    pub version: String,
    pub license: String,
    pub records: Option<u32>,
    pub validation: StorageDatasetPackageEnumValidation,
    pub superseded: bool,
    pub refusal: Vec<StorageDatasetIntegrityFinding>,
}

pub struct DesktopDatasetCommands;

impl DesktopDatasetCommands {
    pub fn starters(manifest: &StorageDatasetPackageManifest) -> Vec<DesktopDatasetStarter> {
        manifest
            .sources()
            .iter()
            .map(|source| DesktopDatasetStarter {
                source: source.identity().source().to_owned(),
                version: source.identity().version().to_owned(),
                kind: source.kind(),
                license: match source.license().terms() {
                    StorageDatasetPackageEnumLicense::Cc0 => "CC0".to_owned(),
                    StorageDatasetPackageEnumLicense::PersonalUse => "Personal use".to_owned(),
                    StorageDatasetPackageEnumLicense::Other(id) => id.as_str().to_owned(),
                },
                origin: source.origin().to_owned(),
                checksum: source.checksum(),
            })
            .collect()
    }

    pub const fn feature(kind: StorageDatasetRegistryEnumKind) -> StorageDatasetIntegrityEnumFeature {
        match kind {
            StorageDatasetRegistryEnumKind::Corpus => StorageDatasetIntegrityEnumFeature::Explorer,
            StorageDatasetRegistryEnumKind::Book => StorageDatasetIntegrityEnumFeature::Book,
            StorageDatasetRegistryEnumKind::Puzzles => StorageDatasetIntegrityEnumFeature::Puzzles,
        }
    }

    pub fn rows(
        connection: &mut SqliteConnection,
        integrity: &StorageDatasetIntegrityReport,
    ) -> Result<Vec<DesktopDatasetRow>, DesktopDatasetCommandsError> {
        Ok(Self::rows_from_entries(
            StorageDatasetRegistry::list(connection)?,
            integrity,
        ))
    }

    pub fn rows_from_entries(
        entries: Vec<StorageDatasetRegistryEntry>,
        integrity: &StorageDatasetIntegrityReport,
    ) -> Vec<DesktopDatasetRow> {
        entries
            .into_iter()
            .map(|entry| {
                let feature = Self::feature(entry.kind);
                let refusal = if entry.origin == StorageDatasetRegistryEnumOrigin::Starter {
                    integrity
                        .ensure_available(feature)
                        .err()
                        .map(|refusal| refusal.findings)
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                DesktopDatasetRow {
                    id: entry.id,
                    kind: entry.kind,
                    origin: entry.origin,
                    name: entry.name,
                    version: entry.version,
                    license: entry.license,
                    records: entry.records,
                    validation: entry.validation,
                    superseded: entry.superseded,
                    refusal,
                }
            })
            .collect()
    }

    pub fn preview_import(
        request: StorageDatasetImportRequest,
        bytes: &[u8],
        available_bytes: u64,
        replaces: Option<StorageDatasetId>,
        cancelled: impl Fn() -> bool,
    ) -> Result<StorageDatasetImportPreview, DesktopDatasetCommandsError> {
        Ok(StorageDatasetImport::preview(
            request,
            bytes,
            available_bytes,
            replaces,
            cancelled,
        )?)
    }

    pub fn apply_import(
        connection: &mut SqliteConnection,
        preview: &StorageDatasetImportPreview,
        confirmation: StorageDatasetImportEnumConfirmation,
    ) -> Result<StorageDatasetId, DesktopDatasetCommandsError> {
        Ok(StorageDatasetImport::apply(connection, preview, confirmation)?)
    }

    pub fn cancel_import(preview: StorageDatasetImportPreview) {
        StorageDatasetImport::cancel(preview);
    }

    pub fn delete(
        connection: &mut SqliteConnection,
        dataset: StorageDatasetId,
        confirmation: StorageDatasetImportEnumConfirmation,
    ) -> Result<(), DesktopDatasetCommandsError> {
        Ok(StorageDatasetImport::delete(connection, dataset, confirmation)?)
    }
}
