use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use chess::desktop::profile::DesktopProfileEnumAdmission;
use chess::storage::backup::error::BackupError;
use chess::storage::backup::manifest::BackupEnumCategory;
use chess::storage::backup::restore::BackupRestoreRequest;
use uuid::Uuid;

use crate::storage_backup::{StorageBackupFixture, StorageBackupTestResult};
use crate::storage_restore::StorageRestoreFixture;

fn tree(root: &Path) -> StorageBackupTestResult<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.insert(
                    path.strip_prefix(root)?.display().to_string(),
                    fs::read(&path)?,
                );
            }
        }
    }
    Ok(files)
}

#[tokio::test]
async fn restore_never_touches_control_records_and_keeps_the_root_lease_and_exclusion()
-> StorageBackupTestResult {
    let fixture = StorageRestoreFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, r#"{"v":"original"}"#).await?;
    let archive = fixture.archive(BackupEnumCategory::Manual, 10, 1_000)?;
    StorageBackupFixture::write(&database, 1, r#"{"v":"newer"}"#).await?;
    let control = fixture.lease.identity().control_directory().to_path_buf();
    let control_before = tree(&control)?;
    assert!(!control_before.is_empty());
    assert!(matches!(
        fixture.admit()?,
        DesktopProfileEnumAdmission::Busy
    ));
    let preview = StorageBackupFixture::inspect(archive.path())?;
    let safety = fixture.archive(BackupEnumCategory::PreRestore, 11, 2_000)?;
    fixture.restore(&preview, &safety, 12)?;
    assert!(matches!(
        fixture.admit()?,
        DesktopProfileEnumAdmission::Busy
    ));
    fixture.lease.verify_identity()?;
    assert_eq!(tree(&control)?, control_before);
    let snapshot = StorageBackupFixture::payload_scalar(
        &preview,
        fixture.directory.path(),
        "SELECT count(*) || '' AS value FROM sqlite_schema WHERE name LIKE '%owner%' OR name LIKE '%lease%' OR name LIKE '%activation%'",
    )?;
    assert_eq!(snapshot, "0");
    Ok(())
}

#[tokio::test]
async fn a_refused_restore_keeps_the_lease_the_exclusion_and_every_original_byte()
-> StorageBackupTestResult {
    let fixture = StorageRestoreFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, r#"{"v":"original"}"#).await?;
    let archive = fixture.archive(BackupEnumCategory::Manual, 10, 1_000)?;
    let preview = StorageBackupFixture::inspect(archive.path())?;
    let safety = fixture.archive(BackupEnumCategory::PreRestore, 11, 2_000)?;
    let data_before = tree(&fixture.data())?;
    let outside = fixture.directory.path().join("elsewhere.sqlite3");
    fs::write(&outside, b"other")?;
    let request = BackupRestoreRequest {
        lease: &*fixture.lease,
        database: &outside,
        preview: &preview,
        confirmed_id: preview.id(),
        confirmed_digest: preview.digest(),
        safety: &safety,
        restore_id: Uuid::from_u128(13),
    };
    assert!(matches!(
        chess::storage::backup::restore::BackupRestorer::restore_blocking(request),
        Err(BackupError::Lease(_))
    ));
    assert_eq!(tree(&fixture.data())?, data_before);
    assert_eq!(fs::read(&outside)?, b"other");
    assert!(matches!(
        fixture.admit()?,
        DesktopProfileEnumAdmission::Busy
    ));
    fixture.lease.verify_identity()?;
    Ok(())
}

#[tokio::test]
async fn a_restore_with_a_damaged_staged_copy_rolls_back_before_replacing_anything()
-> StorageBackupTestResult {
    let fixture = StorageRestoreFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, r#"{"v":"original"}"#).await?;
    let archive = fixture.archive(BackupEnumCategory::Manual, 10, 1_000)?;
    let preview = StorageBackupFixture::inspect(archive.path())?;
    let safety = fixture.archive(BackupEnumCategory::PreRestore, 11, 2_000)?;
    let mut bytes = fs::read(archive.path())?;
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(archive.path(), bytes)?;
    let live_before = fs::read(&database)?;
    assert!(fixture.restore(&preview, &safety, 12).is_err());
    assert_eq!(fs::read(&database)?, live_before);
    assert!(
        fs::read_dir(fixture.data())?
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().contains(".staged"))
    );
    assert!(matches!(
        fixture.admit()?,
        DesktopProfileEnumAdmission::Busy
    ));
    Ok(())
}
