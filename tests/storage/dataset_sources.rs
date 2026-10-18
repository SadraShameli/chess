use std::io;

use chess::package::digest::PackageDigest;
use chess::storage::dataset_sources::{StorageDatasetSources, StorageDatasetSourcesError};

const SOURCES: &str = include_str!("../../resources/datasets/sources.json");

#[test]
fn the_frozen_manifest_names_the_three_cc0_sources_and_refuses_while_hashes_are_unrecorded()
-> Result<(), Box<dyn std::error::Error>> {
    let sources = StorageDatasetSources::from_json(SOURCES.as_bytes())?;
    assert_eq!(sources.sources().len(), 3);
    assert!(
        sources
            .sources()
            .iter()
            .any(|source| { source.version == "a6189a30dc273ccb21fc2536a9a2fefd5592a67a" })
    );
    assert!(
        sources
            .sources()
            .iter()
            .any(|source| source.version == "2025-12")
    );
    assert!(
        sources
            .sources()
            .iter()
            .any(|source| source.version == "2026-10-02")
    );
    assert!(matches!(
        sources.verify(|_| Ok(None)),
        Err(StorageDatasetSourcesError::Unrecorded(_))
    ));
    Ok(())
}

#[test]
fn recorded_hashes_must_match_and_missing_or_changed_files_fail()
-> Result<(), Box<dyn std::error::Error>> {
    let digest = PackageDigest::hash_slice(b"fixture");
    let mut value: serde_json::Value = serde_json::from_str(SOURCES)?;
    for source in value["sources"].as_array_mut().ok_or("sources")? {
        source["sha256"] = serde_json::Value::String(digest.to_string());
    }
    let sources = StorageDatasetSources::from_json(value.to_string().as_bytes())?;
    sources.verify(|_| Ok(Some(digest)))?;
    assert!(matches!(
        sources.verify(|_| Ok(None)),
        Err(StorageDatasetSourcesError::Missing(_))
    ));
    assert!(matches!(
        sources.verify(|_| Ok(Some(PackageDigest::hash_slice(b"other")))),
        Err(StorageDatasetSourcesError::Changed { .. })
    ));
    assert!(matches!(
        sources.verify(|_| Err(io::Error::from(io::ErrorKind::PermissionDenied))),
        Err(StorageDatasetSourcesError::Io { .. })
    ));
    Ok(())
}

#[test]
fn unsupported_schema_license_and_incomplete_kinds_are_refused() {
    let wrong_schema = SOURCES.replace("\"schema_version\": 1", "\"schema_version\": 2");
    assert!(matches!(
        StorageDatasetSources::from_json(wrong_schema.as_bytes()),
        Err(StorageDatasetSourcesError::Schema(2))
    ));
    let wrong_license = SOURCES.replacen("CC0-1.0", "CC-BY-4.0", 1);
    assert!(matches!(
        StorageDatasetSources::from_json(wrong_license.as_bytes()),
        Err(StorageDatasetSourcesError::License(_))
    ));
}
