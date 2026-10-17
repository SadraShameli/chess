use chess::desktop::backup_commands::{
    DesktopBackupCommands, DesktopBackupCommandsEnumPrerequisite, DesktopBackupCommandsError,
    DesktopBackupCommandsPrerequisites,
};
use chess::storage::backup::create::BackupSpaceNative;
use chess::storage::backup::manifest::BackupEnumCategory;
use chess::storage::backup::retention::BackupRetention;
use uuid::Uuid;

use crate::storage_backup::{StorageBackupFixture, StorageBackupTestResult};

fn ready() -> DesktopBackupCommandsPrerequisites {
    DesktopBackupCommandsPrerequisites {
        games_paused_and_saved: true,
        editors_have_no_unsaved_edits: true,
    }
}

fn commands(fixture: &StorageBackupFixture) -> DesktopBackupCommands<BackupSpaceNative> {
    DesktopBackupCommands::new(
        fixture.database(),
        fixture.backups(),
        BackupSpaceNative,
        Vec::new(),
        Vec::new(),
    )
}

#[tokio::test]
async fn unmet_prerequisites_refuse_before_any_archive_is_created() -> StorageBackupTestResult {
    let fixture = StorageBackupFixture::new()?;
    StorageBackupFixture::write(&fixture.database(), 1, r#"{"kind":"committed"}"#).await?;
    let commands = commands(&fixture);
    let unsaved = DesktopBackupCommandsPrerequisites {
        games_paused_and_saved: false,
        editors_have_no_unsaved_edits: true,
    };
    assert!(matches!(
        commands.pre_update_blocking(unsaved, Uuid::from_u128(30), 3_000),
        Err(DesktopBackupCommandsError::Prerequisite(
            DesktopBackupCommandsEnumPrerequisite::GamesPausedAndSaved
        ))
    ));
    assert!(!fixture.backups().exists() || commands.list_blocking()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn a_pre_update_backup_is_created_verified_and_found_again_by_its_identity()
-> StorageBackupTestResult {
    let fixture = StorageBackupFixture::new()?;
    StorageBackupFixture::write(&fixture.database(), 1, r#"{"kind":"committed"}"#).await?;
    let commands = commands(&fixture);
    let id = Uuid::from_u128(31);
    let created = commands.pre_update_blocking(ready(), id, 3_000)?;
    assert_eq!(created, id);
    let path = commands.find_blocking(id)?;
    assert!(path.is_file());
    let entries = BackupRetention::scan_blocking(&fixture.backups())?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].manifest.category(), BackupEnumCategory::PreUpdate);
    let preview = StorageBackupFixture::inspect(&path)?;
    assert_eq!(preview.id(), id);
    assert!(matches!(
        commands.find_blocking(Uuid::from_u128(99)),
        Err(DesktopBackupCommandsError::UnknownPreview(_))
    ));
    Ok(())
}
