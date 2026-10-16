use std::io::Cursor;
use std::num::NonZeroUsize;

use chess::capacity::budget::{CapacityBudget, CapacityBudgetError};
use chess::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use chess::package::digest::PackageDigest;
use chess::storage::dataset_package::{
    StorageDatasetPackage, StorageDatasetPackageEnumFormat, StorageDatasetPackageEnumValidation,
    StorageDatasetPackageError,
};

type StorageDatasetPackageTestResult = Result<(), Box<dyn std::error::Error>>;

struct StorageDatasetPackageFixture;

impl StorageDatasetPackageFixture {
    const CONTENT: &[u8] = b"integrity does not establish semantic chess validity";

    fn budget() -> Result<CapacityBudget, Box<dyn std::error::Error>> {
        Ok(CapacityBudget::new(
            NonZeroUsize::new(1).ok_or("invalid fixture capacity")?,
        )?)
    }

    fn descriptor(
        path: std::path::PathBuf,
    ) -> Result<StorageDatasetPackage, Box<dyn std::error::Error>> {
        Ok(StorageDatasetPackage::new(
            StorageDatasetPackageEnumFormat::Pgn,
            PackageDigest::read(Cursor::new(Self::CONTENT))?,
            path,
        )?)
    }
}

#[tokio::test]
async fn verified_bytes_retain_format_and_canonical_location_without_claiming_semantic_validation()
-> StorageDatasetPackageTestResult {
    let directory = tempfile::tempdir()?;
    let location = directory.path().join("fixture.pgn");
    tokio::fs::write(&location, StorageDatasetPackageFixture::CONTENT).await?;
    let descriptor = StorageDatasetPackageFixture::descriptor(location.clone())?;
    assert_eq!(
        descriptor.validation(),
        StorageDatasetPackageEnumValidation::Unchecked
    );
    let checksum = descriptor.checksum();
    let executor = CapacityExecutor::new(StorageDatasetPackageFixture::budget()?)?;
    let verified = descriptor.verify_integrity(&executor)?.join().await??;
    assert_eq!(verified.format(), StorageDatasetPackageEnumFormat::Pgn);
    assert_eq!(verified.checksum(), checksum);
    assert_eq!(verified.location(), location.canonicalize()?);
    assert_eq!(
        verified.validation(),
        StorageDatasetPackageEnumValidation::IntegrityVerified
    );
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn checksum_failure_never_produces_a_verified_replacement() -> StorageDatasetPackageTestResult
{
    let directory = tempfile::tempdir()?;
    let location = directory.path().join("changed.pgn");
    tokio::fs::write(&location, b"changed bytes").await?;
    let descriptor = StorageDatasetPackageFixture::descriptor(location)?;
    let executor = CapacityExecutor::new(StorageDatasetPackageFixture::budget()?)?;
    assert!(matches!(
        descriptor.verify_integrity(&executor)?.join().await?,
        Err(StorageDatasetPackageError::Checksum { .. })
    ));
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn relative_missing_and_nonregular_locations_fail_explicitly()
-> StorageDatasetPackageTestResult {
    assert!(matches!(
        StorageDatasetPackage::new(
            StorageDatasetPackageEnumFormat::PuzzleCsv,
            PackageDigest::read(Cursor::new(StorageDatasetPackageFixture::CONTENT))?,
            "relative.csv".into(),
        ),
        Err(StorageDatasetPackageError::RelativeLocation(_))
    ));
    let directory = tempfile::tempdir()?;
    let executor = CapacityExecutor::new(StorageDatasetPackageFixture::budget()?)?;
    let missing = StorageDatasetPackageFixture::descriptor(directory.path().join("missing.pgn"))?;
    assert!(matches!(
        missing.verify_integrity(&executor)?.join().await?,
        Err(StorageDatasetPackageError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound
    ));
    let nonregular = StorageDatasetPackageFixture::descriptor(directory.path().to_path_buf())?;
    assert!(matches!(
        nonregular.verify_integrity(&executor)?.join().await?,
        Err(StorageDatasetPackageError::NotRegularFile(_))
    ));
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn verification_respects_shared_capacity_before_touching_a_missing_file()
-> StorageDatasetPackageTestResult {
    let directory = tempfile::tempdir()?;
    let budget = StorageDatasetPackageFixture::budget()?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let lease = budget.try_lease(1)?;
    let descriptor =
        StorageDatasetPackageFixture::descriptor(directory.path().join("missing.pgn"))?;
    assert!(matches!(
        descriptor.verify_integrity(&executor),
        Err(StorageDatasetPackageError::Executor(
            CapacityExecutorError::Budget(CapacityBudgetError::Unavailable)
        ))
    ));
    drop(lease);
    assert_eq!(budget.available(), budget.workers());
    executor.shutdown().await?;
    Ok(())
}
