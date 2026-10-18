use std::path::PathBuf;

use chess::desktop::backup_commands::{
    DesktopBackupCommandsEntry, DesktopBackupCommandsPrerequisites,
};
use chess::package::digest::PackageDigest;
use chess::presentation::settings::backup::PresentationSettingsBackup;
use chess::presentation::settings::restore::PresentationSettingsRestore;
use chess::storage::backup::manifest::{
    BackupDependency, BackupEnumCategory, BackupEnumDependencyKind, BackupEnumPlatform,
    BackupManifest, BackupManifestParts,
};
use chess::storage::backup::preview::BackupPreviewEnumFinding;
use chess::storage::backup::scheduler::{BackupScheduler, BackupState};
use uuid::Uuid;

type PresentationBackupResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn entry(
    category: BackupEnumCategory,
    id: u128,
) -> PresentationBackupResult<DesktopBackupCommandsEntry> {
    let payload = b"payload";
    Ok(DesktopBackupCommandsEntry {
        path: PathBuf::from(format!("backup-{id}")),
        manifest: BackupManifest::new(BackupManifestParts {
            id: Uuid::from_u128(id),
            category,
            created_ms: 1,
            platform: BackupEnumPlatform::current(),
            schema_versions: vec!["1".to_owned()],
            catalog_revision: "0".to_owned(),
            payload_bytes: 3 * 1024 * 1024 + 1,
            payload_sha256: PackageDigest::hash_slice(payload),
            dependencies: Vec::new(),
            portable_paths: Vec::new(),
        })?,
    })
}

#[test]
fn a_failed_automatic_backup_shows_a_persistent_warning_without_blocking_play()
-> PresentationBackupResult {
    let day = BackupScheduler::INTERVAL_MS;
    let failed = BackupState::new(Some(0), None, Some("the disk is full".to_owned()));
    let status = PresentationSettingsBackup::status(&failed, day + 5, &[]);
    let warning = status.warning.ok_or("missing warning")?;
    assert!(warning.contains("the disk is full"));
    assert!(warning.contains("Play is not affected"));
    assert_eq!(status.schedule, "An automatic backup is due");
    let healthy = BackupState::new(Some(0), Some(0), None);
    let status = PresentationSettingsBackup::status(&healthy, day / 2, &[]);
    assert!(status.warning.is_none());
    assert!(status.schedule.contains("12 hours"));
    let fresh = PresentationSettingsBackup::status(&BackupState::new(None, None, None), 0, &[]);
    assert!(fresh.schedule.contains("first saved data"));
    Ok(())
}

#[test]
fn the_backup_list_reports_category_and_rounded_up_megabytes() -> PresentationBackupResult {
    let entries = [
        entry(BackupEnumCategory::Automatic, 1)?,
        entry(BackupEnumCategory::PreRestore, 2)?,
    ];
    let status =
        PresentationSettingsBackup::status(&BackupState::new(None, None, None), 0, &entries);
    assert_eq!(status.rows.len(), 2);
    assert_eq!(status.rows[0].category, BackupEnumCategory::Automatic);
    assert_eq!(status.rows[0].megabytes, 4);
    assert!(status.rows[1].label.starts_with("pre-restore backup"));
    Ok(())
}

#[test]
fn restore_previews_explain_platform_locations_dependencies_and_migration() {
    let dependency = BackupDependency {
        kind: BackupEnumDependencyKind::Dataset,
        name: "openings".to_owned(),
        digest: None,
    };
    let findings = [
        BackupPreviewEnumFinding::CrossPlatform {
            archive: BackupEnumPlatform::Windows,
            destination: BackupEnumPlatform::Linux,
        },
        BackupPreviewEnumFinding::DestinationLocationsKept {
            stored: vec!["C:/Exports".to_owned()],
        },
        BackupPreviewEnumFinding::DependencyUnavailable(dependency),
        BackupPreviewEnumFinding::OlderSchemaWillMigrate {
            pending: vec!["6".to_owned(), "7".to_owned()],
        },
    ];
    let text: Vec<String> = findings
        .iter()
        .map(PresentationSettingsRestore::describe)
        .collect();
    assert!(text[0].contains("Windows") && text[0].contains("Linux"));
    assert!(text[1].contains("1 stored locations"));
    assert!(text[2].contains("openings") && text[2].contains("unavailable"));
    assert!(text[3].contains("2 database changes"));
}

#[test]
fn confirmation_is_blocked_until_games_are_saved_and_edits_are_resolved() {
    let blocked = DesktopBackupCommandsPrerequisites {
        games_paused_and_saved: false,
        editors_have_no_unsaved_edits: true,
    };
    assert_eq!(
        PresentationSettingsRestore::blocked_reason(blocked),
        Some("Pause and save every open game before restoring.")
    );
    let editing = DesktopBackupCommandsPrerequisites {
        games_paused_and_saved: true,
        editors_have_no_unsaved_edits: false,
    };
    assert_eq!(
        PresentationSettingsRestore::blocked_reason(editing),
        Some("Save or discard unsaved edits before restoring.")
    );
    let ready = DesktopBackupCommandsPrerequisites {
        games_paused_and_saved: true,
        editors_have_no_unsaved_edits: true,
    };
    assert_eq!(PresentationSettingsRestore::blocked_reason(ready), None);
}
