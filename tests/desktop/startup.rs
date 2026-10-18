use std::fs::{File, TryLockError};
use std::io;
use std::str::FromStr;

use chess::desktop::profile::{
    DesktopProfileAdmission, DesktopProfileEnumAdmission, DesktopProfileError, DesktopProfileId,
    DesktopProfileLockProvider, DesktopProfileOsLock, DesktopProfileSecurity,
};

struct DesktopProfileStartupUnsupportedLock;

impl DesktopProfileLockProvider for DesktopProfileStartupUnsupportedLock {
    fn try_lock(&self, _file: &File) -> Result<(), TryLockError> {
        Err(TryLockError::Error(io::Error::from(
            io::ErrorKind::Unsupported,
        )))
    }
}

#[test]
fn unsupported_exclusion_is_a_typed_failure_before_identity_publication()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::TempDir::new()?;
    let profile = directory.path().join("profile");
    let admission = DesktopProfileAdmission::new(
        DesktopProfileSecurity::current()?,
        DesktopProfileStartupUnsupportedLock,
    );
    let id = DesktopProfileId::from_str("6507b181-6fc4-46df-a8a7-335637c01547")?;
    assert!(matches!(
        admission.acquire(&profile, id),
        Err(DesktopProfileError::LockUnsupported { .. })
    ));
    assert!(!profile.join("control/profile.id").exists());
    assert!(!profile.join("library.sqlite3").exists());
    Ok(())
}

#[test]
fn a_contending_launch_neither_overwrites_identity_nor_creates_application_data()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::TempDir::new()?;
    let profile = directory.path().join("profile");
    let admission =
        DesktopProfileAdmission::new(DesktopProfileSecurity::current()?, DesktopProfileOsLock);
    let first_id = DesktopProfileId::from_str("d2fbaae6-2946-478c-b4bf-00d5cb31c7b4")?;
    let owner = admission.acquire(&profile, first_id)?;
    assert!(matches!(owner, DesktopProfileEnumAdmission::Owned(_)));
    let before = std::fs::read(profile.join("control/profile.id"))?;
    let proposed = DesktopProfileId::from_str("a857318d-66e4-4f00-a1dd-764ea1c38288")?;
    assert!(matches!(
        admission.acquire(&profile, proposed)?,
        DesktopProfileEnumAdmission::Busy
    ));
    assert_eq!(std::fs::read(profile.join("control/profile.id"))?, before);
    assert!(!profile.join("library.sqlite3").exists());
    assert!(!profile.join("engine").exists());
    drop(owner);
    Ok(())
}

#[test]
fn rejects_invalid_or_noncanonical_destination_ids() {
    for invalid in [
        "",
        "00000000-0000-0000-0000-000000000000",
        "D2FBAAE6-2946-478C-B4BF-00D5CB31C7B4",
        "d2fbaae62946478cb4bf00d5cb31c7b4",
    ] {
        assert!(DesktopProfileId::from_str(invalid).is_err(), "{invalid}");
    }
}
