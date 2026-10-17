use chess::package::digest::PackageDigest;
use chess::storage::connection::StorageConnection;
use chess::storage::dataset_import::{
    StorageDatasetImport, StorageDatasetImportEnumConfirmation, StorageDatasetImportError,
    StorageDatasetImportRequest,
};
use chess::storage::dataset_package::{
    StorageDatasetPackageEnumFormat, StorageDatasetPackageEnumValidation,
};
use chess::storage::dataset_registry::{
    StorageDatasetRegistry, StorageDatasetRegistryEnumHolder, StorageDatasetRegistryEnumKind,
    StorageDatasetRegistryEnumOrigin, StorageDatasetRegistryError, StorageDatasetRegistryNew,
};
use chess::storage::migration::StorageMigrationRunner;

type StorageDatasetsTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct StorageDatasetsFixture;

impl StorageDatasetsFixture {
    fn open(directory: &tempfile::TempDir) -> StorageDatasetsTestResult<StorageConnection> {
        let mut connection = StorageConnection::open(&directory.path().join("datasets.sqlite3"))?;
        StorageMigrationRunner::run_embedded(connection.diesel())?;
        Ok(connection)
    }

    fn dataset(version: &str) -> StorageDatasetRegistryNew {
        StorageDatasetRegistryNew {
            kind: StorageDatasetRegistryEnumKind::Corpus,
            origin: StorageDatasetRegistryEnumOrigin::Personal,
            name: "Club games".to_owned(),
            version: version.to_owned(),
            format: StorageDatasetPackageEnumFormat::Pgn,
            license: "Personal use".to_owned(),
            checksum: PackageDigest::hash_slice(version.as_bytes()),
            location: format!("/datasets/club-{version}.pgn"),
            validation: StorageDatasetPackageEnumValidation::IntegrityVerified,
            records: Some(10),
        }
    }

    fn book_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x463b_9618_1691_fc9c_u64.to_be_bytes());
        bytes.extend_from_slice(&0x031c_u16.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes
    }

    fn book_request() -> StorageDatasetImportRequest {
        StorageDatasetImportRequest {
            kind: StorageDatasetRegistryEnumKind::Book,
            name: "Personal book".to_owned(),
            version: "1".to_owned(),
            license: "Personal use".to_owned(),
            format: StorageDatasetPackageEnumFormat::Polyglot,
            location: "/datasets/book.bin".to_owned(),
        }
    }
}

#[test]
fn replacement_switches_versions_atomically_and_keeps_a_leased_version_until_release()
-> StorageDatasetsTestResult {
    let directory = tempfile::tempdir()?;
    let mut connection = StorageDatasetsFixture::open(&directory)?;
    let first = StorageDatasetRegistry::register(
        connection.diesel(),
        &StorageDatasetsFixture::dataset("v1"),
    )?;
    let lease = StorageDatasetRegistry::lease(
        connection.diesel(),
        first,
        StorageDatasetRegistryEnumHolder::Run,
        "run-1",
    )?;
    let second = StorageDatasetRegistry::replace(
        connection.diesel(),
        first,
        &StorageDatasetsFixture::dataset("v2"),
    )?;
    let listed = StorageDatasetRegistry::list(connection.diesel())?;
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().any(|entry| entry.id == first && entry.superseded));
    assert!(listed.iter().any(|entry| entry.id == second && !entry.superseded));
    assert_eq!(StorageDatasetRegistry::purge_released(connection.diesel())?, 0);
    StorageDatasetRegistry::release(connection.diesel(), lease)?;
    assert_eq!(StorageDatasetRegistry::purge_released(connection.diesel())?, 1);
    let listed = StorageDatasetRegistry::list(connection.diesel())?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, second);
    Ok(())
}

#[test]
fn replacement_must_keep_the_collection_and_change_the_version() -> StorageDatasetsTestResult {
    let directory = tempfile::tempdir()?;
    let mut connection = StorageDatasetsFixture::open(&directory)?;
    let first = StorageDatasetRegistry::register(
        connection.diesel(),
        &StorageDatasetsFixture::dataset("v1"),
    )?;
    assert!(matches!(
        StorageDatasetRegistry::replace(
            connection.diesel(),
            first,
            &StorageDatasetsFixture::dataset("v1")
        ),
        Err(StorageDatasetRegistryError::ReplacementMismatch)
    ));
    assert!(matches!(
        StorageDatasetRegistry::register(
            connection.diesel(),
            &StorageDatasetsFixture::dataset("v1")
        ),
        Err(StorageDatasetRegistryError::Duplicate)
    ));
    assert_eq!(StorageDatasetRegistry::list(connection.diesel())?.len(), 1);
    Ok(())
}

#[test]
fn deletion_is_refused_while_active_work_references_the_version() -> StorageDatasetsTestResult {
    let directory = tempfile::tempdir()?;
    let mut connection = StorageDatasetsFixture::open(&directory)?;
    let id = StorageDatasetRegistry::register(
        connection.diesel(),
        &StorageDatasetsFixture::dataset("v1"),
    )?;
    let lease = StorageDatasetRegistry::lease(
        connection.diesel(),
        id,
        StorageDatasetRegistryEnumHolder::Query,
        "explorer",
    )?;
    assert!(matches!(
        StorageDatasetRegistry::remove(connection.diesel(), id),
        Err(StorageDatasetRegistryError::InUse { leases: 1 })
    ));
    StorageDatasetRegistry::release(connection.diesel(), lease)?;
    StorageDatasetRegistry::remove(connection.diesel(), id)?;
    assert!(StorageDatasetRegistry::list(connection.diesel())?.is_empty());
    Ok(())
}

#[test]
fn import_needs_a_preview_and_explicit_confirmation_and_cancel_has_no_effect()
-> StorageDatasetsTestResult {
    let directory = tempfile::tempdir()?;
    let mut connection = StorageDatasetsFixture::open(&directory)?;
    let bytes = StorageDatasetsFixture::book_bytes();
    let preview = StorageDatasetImport::preview(
        StorageDatasetsFixture::book_request(),
        &bytes,
        1_000,
        None,
        || false,
    )?;
    assert_eq!(preview.accepted(), 1);
    assert_eq!(preview.rejected(), 0);
    assert_eq!(preview.checksum(), PackageDigest::hash_slice(&bytes));
    assert!(matches!(
        StorageDatasetImport::apply(
            connection.diesel(),
            &preview,
            StorageDatasetImportEnumConfirmation::Unconfirmed
        ),
        Err(StorageDatasetImportError::ConfirmationRequired)
    ));
    assert!(StorageDatasetRegistry::list(connection.diesel())?.is_empty());
    let second = preview.clone();
    StorageDatasetImport::cancel(second);
    assert!(StorageDatasetRegistry::list(connection.diesel())?.is_empty());
    let id = StorageDatasetImport::apply(
        connection.diesel(),
        &preview,
        StorageDatasetImportEnumConfirmation::Confirmed,
    )?;
    let listed = StorageDatasetRegistry::list(connection.diesel())?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].origin, StorageDatasetRegistryEnumOrigin::Personal);
    Ok(())
}

#[test]
fn import_refuses_insufficient_space_empty_and_mismatched_files() -> StorageDatasetsTestResult {
    let bytes = StorageDatasetsFixture::book_bytes();
    assert!(matches!(
        StorageDatasetImport::preview(
            StorageDatasetsFixture::book_request(),
            &bytes,
            8,
            None,
            || false
        ),
        Err(StorageDatasetImportError::Space {
            required: 16,
            available: 8
        })
    ));
    assert!(matches!(
        StorageDatasetImport::preview(
            StorageDatasetsFixture::book_request(),
            &[],
            1_000,
            None,
            || false
        ),
        Err(StorageDatasetImportError::Empty)
    ));
    let mut request = StorageDatasetsFixture::book_request();
    request.format = StorageDatasetPackageEnumFormat::Pgn;
    assert!(matches!(
        StorageDatasetImport::preview(request, &bytes, 1_000, None, || false),
        Err(StorageDatasetImportError::FormatMismatch)
    ));
    Ok(())
}
