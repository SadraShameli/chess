use std::num::NonZeroU32;
use std::process::Command;
use std::time::Duration;

use semver::Version;

use super::{DesktopNativeUpdateEnvironment, DesktopProfileInstallationSpec};
use crate::desktop::backup_commands::DesktopBackupCommandsPrerequisites;
use crate::desktop::identity::DesktopRequestId;
use crate::package::digest::PackageDigest;
use crate::update::install::{UpdateHelperEnumReadiness, UpdateInstallEnvironment};

#[cfg(unix)]
#[test]
fn a_helper_that_never_acknowledges_is_killed_and_reaped() -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::new("sleep");
    command.arg("30");
    let mut environment = DesktopNativeUpdateEnvironment {
        prerequisites: DesktopBackupCommandsPrerequisites {
            games_paused_and_saved: true,
            editors_have_no_unsaved_edits: true,
        },
        backup: uuid::Uuid::new_v4(),
        command,
        child: None,
        ready_timeout: Duration::from_millis(200),
    };
    let spec = DesktopProfileInstallationSpec {
        owner_generation: DesktopRequestId::new(),
        helper_generation: DesktopRequestId::new(),
        package_digest: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            .parse::<PackageDigest>()?,
        package_version: Version::new(1, 2, 0),
        safety_backup_id: DesktopRequestId::new(),
        data_schema: NonZeroU32::MIN,
    };
    assert_eq!(
        environment.start_helper(&spec),
        UpdateHelperEnumReadiness::Lost
    );
    assert!(environment.confirm_helper_stopped(&spec));
    Ok(())
}
