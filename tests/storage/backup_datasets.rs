use chess::desktop::backup_commands::{
    DesktopBackupCommandsEnumPrerequisite, DesktopBackupCommandsPrerequisites,
};
use chess::package::digest::PackageDigest;
use chess::storage::backup::create::{BackupCreator, BackupSpaceNative};
use chess::storage::backup::manifest::BackupEnumCategory;
use chess::storage::connection::StorageConnection;
use chess::storage::dataset_package::{
    StorageDatasetPackageEnumFormat, StorageDatasetPackageEnumValidation,
};
use chess::storage::dataset_registry::{
    StorageDatasetRegistry, StorageDatasetRegistryEnumKind, StorageDatasetRegistryEnumOrigin,
    StorageDatasetRegistryNew,
};

use crate::storage_backup::{StorageBackupFixture, StorageBackupTestResult};

#[tokio::test]
async fn backups_carry_dataset_references_with_identity_checksum_and_validation_state()
-> StorageBackupTestResult {
    let fixture = StorageBackupFixture::new()?;
    let database = fixture.database();
    StorageBackupFixture::write(&database, 1, r#"{"kind":"committed"}"#).await?;
    let checksum = PackageDigest::hash_slice(b"club");
    {
        let mut connection = StorageConnection::open(&database)?;
        StorageDatasetRegistry::register(
            connection.diesel(),
            &StorageDatasetRegistryNew {
                kind: StorageDatasetRegistryEnumKind::Corpus,
                origin: StorageDatasetRegistryEnumOrigin::Personal,
                name: "Club games".to_owned(),
                version: "v1".to_owned(),
                format: StorageDatasetPackageEnumFormat::Pgn,
                license: "Personal use".to_owned(),
                checksum,
                location: "/datasets/club.pgn".to_owned(),
                validation: StorageDatasetPackageEnumValidation::IntegrityVerified,
                records: Some(12),
            },
        )?;
    }
    let backups = fixture.backups();
    let archive = BackupCreator::create_blocking(
        StorageBackupFixture::request(&database, &backups, BackupEnumCategory::Manual, 20, 2_000),
        &BackupSpaceNative,
    )?;
    let preview = StorageBackupFixture::inspect(archive.path())?;
    let scratch = fixture.directory.path();
    let reference = StorageBackupFixture::payload_scalar(
        &preview,
        scratch,
        "SELECT name || '|' || version || '|' || validation || '|' || checksum AS value FROM datasets LIMIT 1",
    )?;
    assert_eq!(
        reference,
        format!("Club games|v1|integrity_verified|{checksum}")
    );
    Ok(())
}

#[test]
fn restore_prerequisites_report_paused_games_before_unsaved_editors() {
    let both = DesktopBackupCommandsPrerequisites {
        games_paused_and_saved: false,
        editors_have_no_unsaved_edits: false,
    };
    assert_eq!(
        both.first_unmet(),
        Some(DesktopBackupCommandsEnumPrerequisite::GamesPausedAndSaved)
    );
    let editors = DesktopBackupCommandsPrerequisites {
        games_paused_and_saved: true,
        editors_have_no_unsaved_edits: false,
    };
    assert_eq!(
        editors.first_unmet(),
        Some(DesktopBackupCommandsEnumPrerequisite::EditorsHaveNoUnsavedEdits)
    );
    let ready = DesktopBackupCommandsPrerequisites {
        games_paused_and_saved: true,
        editors_have_no_unsaved_edits: true,
    };
    assert_eq!(ready.first_unmet(), None);
}
