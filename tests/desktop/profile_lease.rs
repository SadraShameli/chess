use std::fmt::{Display, Formatter};
use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::sync::Barrier;

use chess::desktop::profile::{
    DesktopProfileAdmission, DesktopProfileEnumAdmission, DesktopProfileError, DesktopProfileId,
    DesktopProfileIdentity, DesktopProfileLease, DesktopProfileOsLock, DesktopProfileSecurity,
};
use tempfile::TempDir;

#[derive(Debug)]
struct DesktopProfileLeaseThreadError {
    payload: Box<dyn std::any::Any + Send>,
}

impl Display for DesktopProfileLeaseThreadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Profile launch thread failed: {:?}",
            self.payload
        )
    }
}

impl std::error::Error for DesktopProfileLeaseThreadError {}

struct DesktopProfileLeaseFixture {
    directory: TempDir,
    admission: DesktopProfileAdmission<DesktopProfileOsLock>,
}

impl DesktopProfileLeaseFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            directory: TempDir::new()?,
            admission: DesktopProfileAdmission::new(
                DesktopProfileSecurity::current()?,
                DesktopProfileOsLock,
            ),
        })
    }

    fn profile_path(&self) -> std::path::PathBuf {
        self.directory.path().join("profile")
    }

    fn acquire(&self) -> Result<DesktopProfileEnumAdmission, DesktopProfileError> {
        self.admission.acquire(&self.profile_path(), Self::id()?)
    }

    fn owned(&self) -> Result<DesktopProfileLease, Box<dyn std::error::Error>> {
        match self.acquire()? {
            DesktopProfileEnumAdmission::Owned(lease) => Ok(*lease),
            DesktopProfileEnumAdmission::Busy => {
                Err(std::io::Error::other("Expected profile ownership").into())
            }
        }
    }

    fn id() -> Result<DesktopProfileId, DesktopProfileError> {
        DesktopProfileId::from_str("d92f840f-e575-4654-92bd-79b3ba8b8e55")
    }
}

#[test]
fn preserves_the_original_application_data_directory_name() {
    let base = Path::new("application-data");
    assert_eq!(
        DesktopProfileIdentity::data_directory_in(base),
        base.join("nl.sadra.chess")
    );
}

#[test]
fn exclusion_retains_the_nontruncated_permanent_anchor_until_drop()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileLeaseFixture::new()?;
    let initial = fixture.owned()?;
    let lock_path = initial.identity().lock_path().to_path_buf();
    let anchor = same_file::Handle::from_path(&lock_path)?;
    let profile_id = initial.identity().profile_id();
    drop(initial);
    fs::write(&lock_path, b"permanent anchor")?;
    let lease = fixture.owned()?;
    assert_eq!(lease.identity().profile_id(), profile_id);
    assert!(matches!(
        fixture.acquire()?,
        DesktopProfileEnumAdmission::Busy
    ));
    assert_eq!(fs::read(&lock_path)?, b"permanent anchor");
    lease.verify_identity()?;
    drop(lease);
    let next = fixture.owned()?;
    assert_eq!(same_file::Handle::from_path(&lock_path)?, anchor);
    assert_eq!(next.identity().profile_id(), profile_id);
    Ok(())
}

#[test]
fn replacement_of_user_data_does_not_replace_the_control_anchor()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileLeaseFixture::new()?;
    let lease = fixture.owned()?;
    let library_path = lease.identity().data_directory().join("library.sqlite3");
    fs::write(&library_path, b"old library")?;
    fs::remove_file(&library_path)?;
    fs::write(&library_path, b"restored library")?;
    assert!(matches!(
        fixture.acquire()?,
        DesktopProfileEnumAdmission::Busy
    ));
    lease.verify_identity()?;
    Ok(())
}

#[test]
fn stale_owner_metadata_and_diagnostic_pid_do_not_grant_ownership()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileLeaseFixture::new()?;
    let first = fixture.owned()?;
    let owner_record = first.identity().control_directory().join("owner.json");
    fs::write(&owner_record, br#"{"pid":1,"generation":"stale"}"#)?;
    assert!(matches!(
        fixture.acquire()?,
        DesktopProfileEnumAdmission::Busy
    ));
    drop(first);
    let next = fixture.owned()?;
    next.verify_identity()?;
    assert_eq!(
        fs::read(owner_record)?,
        br#"{"pid":1,"generation":"stale"}"#
    );
    Ok(())
}

#[test]
fn simultaneous_first_launch_admits_exactly_one_owner() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileLeaseFixture::new()?;
    let start = Barrier::new(2);
    let acquired = Barrier::new(2);
    let outcomes = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    let outcome = fixture.acquire();
                    acquired.wait();
                    outcome
                        .map(|admission| matches!(admission, DesktopProfileEnumAdmission::Owned(_)))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                let outcome = handle
                    .join()
                    .map_err(|payload| DesktopProfileLeaseThreadError { payload })?;
                Ok(outcome?)
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()
    })?;
    assert_eq!(outcomes.into_iter().filter(|owned| *owned).count(), 1);
    Ok(())
}

#[test]
fn an_existing_destination_keeps_its_local_id_despite_a_new_proposal()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileLeaseFixture::new()?;
    let first = fixture.owned()?;
    let expected = first.identity().profile_id();
    drop(first);
    let proposed = DesktopProfileId::from_str("3713a2a0-43cc-477a-bc72-cdbd0cf49971")?;
    match fixture
        .admission
        .acquire(&fixture.profile_path(), proposed)?
    {
        DesktopProfileEnumAdmission::Owned(lease) => {
            assert_eq!(lease.identity().profile_id(), expected)
        }
        DesktopProfileEnumAdmission::Busy => {
            return Err(std::io::Error::other("Profile remained busy after drop").into());
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn aliases_share_the_canonical_profile_and_lock_anchor() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileLeaseFixture::new()?;
    let first = fixture.owned()?;
    let alias = fixture.directory.path().join("alias");
    std::os::unix::fs::symlink(fixture.profile_path(), &alias)?;
    assert!(matches!(
        fixture
            .admission
            .acquire(&alias, DesktopProfileLeaseFixture::id()?)?,
        DesktopProfileEnumAdmission::Busy
    ));
    let canonical = first.identity().data_directory().to_path_buf();
    drop(first);
    match fixture
        .admission
        .acquire(&alias, DesktopProfileLeaseFixture::id()?)?
    {
        DesktopProfileEnumAdmission::Owned(lease) => {
            assert_eq!(lease.identity().data_directory(), canonical)
        }
        DesktopProfileEnumAdmission::Busy => {
            return Err(std::io::Error::other("Alias remained busy after drop").into());
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_a_public_control_directory_instead_of_repairing_it()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let fixture = DesktopProfileLeaseFixture::new()?;
    let first = fixture.owned()?;
    let control = first.identity().control_directory().to_path_buf();
    drop(first);
    fs::set_permissions(&control, fs::Permissions::from_mode(0o777))?;
    assert!(matches!(
        fixture.acquire(),
        Err(DesktopProfileError::UnsafePermissions { .. })
    ));
    assert_eq!(fs::metadata(control)?.permissions().mode() & 0o777, 0o777);
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlink_control_and_hardlinked_lock_files() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileLeaseFixture::new()?;
    let first = fixture.owned()?;
    let control = first.identity().control_directory().to_path_buf();
    let lock = first.identity().lock_path().to_path_buf();
    drop(first);
    let extra_link = fixture.directory.path().join("lock-alias");
    fs::hard_link(&lock, &extra_link)?;
    assert!(matches!(
        fixture.acquire(),
        Err(DesktopProfileError::UnsafeIdentity { .. })
    ));
    fs::remove_file(extra_link)?;
    let relocated = fixture.directory.path().join("relocated-control");
    fs::rename(&control, &relocated)?;
    std::os::unix::fs::symlink(&relocated, &control)?;
    assert!(matches!(
        fixture.acquire(),
        Err(DesktopProfileError::UnsafeIdentity { .. })
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn detects_directory_replacement_while_retaining_the_original_lease()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileLeaseFixture::new()?;
    let lease = fixture.owned()?;
    fs::rename(
        fixture.profile_path(),
        fixture.directory.path().join("previous-profile"),
    )?;
    fs::create_dir(fixture.profile_path())?;
    assert!(matches!(
        lease.verify_identity(),
        Err(DesktopProfileError::IdentityChanged { .. })
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_a_replaced_control_path_that_links_to_the_original_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileLeaseFixture::new()?;
    let lease = fixture.owned()?;
    let control = lease.identity().control_directory().to_path_buf();
    let relocated = fixture.directory.path().join("relocated-control");
    fs::rename(&control, &relocated)?;
    std::os::unix::fs::symlink(&relocated, &control)?;
    assert!(matches!(
        lease.verify_identity(),
        Err(DesktopProfileError::UnsafeIdentity { .. })
    ));
    Ok(())
}

#[test]
fn rejects_a_malformed_destination_identity_only_after_exclusive_acquisition()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileLeaseFixture::new()?;
    let owner = fixture.owned()?;
    let identity_path = owner.identity().control_directory().join("profile.id");
    fs::write(&identity_path, b"not a profile identity")?;
    assert!(matches!(
        fixture.acquire()?,
        DesktopProfileEnumAdmission::Busy
    ));
    drop(owner);
    assert!(matches!(
        fixture.acquire(),
        Err(DesktopProfileError::InvalidIdentityRecord { .. })
    ));
    assert_eq!(fs::read(identity_path)?, b"not a profile identity");
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn refuses_a_macos_data_directory_mutation_grant_without_repairing_it()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    use exacl::{AclEntry, Perm};

    let fixture = DesktopProfileLeaseFixture::new()?;
    let owner = fixture.owned()?;
    let data = owner.identity().data_directory().to_path_buf();
    drop(owner);
    let entries = vec![AclEntry::allow_group("everyone", Perm::WRITE, None)];
    exacl::setfacl(&[&data], &entries, None)?;
    let before = exacl::getfacl(&data, None)?;
    let mode = fs::metadata(&data)?.permissions().mode() & 0o777;
    let result = fixture.acquire();
    let after = exacl::getfacl(&data, None)?;
    exacl::setfacl(&[&data], &[], None)?;
    assert_eq!(mode, 0o700);
    assert!(matches!(
        result,
        Err(DesktopProfileError::UnsafePermissions { .. })
    ));
    assert_eq!(before, after);
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn refuses_a_macos_private_control_read_grant_despite_private_mode_bits()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    use exacl::{AclEntry, Perm};

    let fixture = DesktopProfileLeaseFixture::new()?;
    let owner = fixture.owned()?;
    let control = owner.identity().control_directory().to_path_buf();
    drop(owner);
    let entries = vec![AclEntry::allow_group("everyone", Perm::READ, None)];
    exacl::setfacl(&[&control], &entries, None)?;
    let before = exacl::getfacl(&control, None)?;
    let mode = fs::metadata(&control)?.permissions().mode() & 0o777;
    let result = fixture.acquire();
    let after = exacl::getfacl(&control, None)?;
    exacl::setfacl(&[&control], &[], None)?;
    assert_eq!(mode, 0o700);
    assert!(matches!(
        result,
        Err(DesktopProfileError::UnsafePermissions { .. })
    ));
    assert_eq!(before, after);
    Ok(())
}

#[cfg(windows)]
#[test]
fn refuses_an_existing_control_directory_with_a_public_dacl()
-> Result<(), Box<dyn std::error::Error>> {
    use windows_permissions::constants::{SeObjectType, SecurityInformation};
    use windows_permissions::wrappers;

    let fixture = DesktopProfileLeaseFixture::new()?;
    let owner = fixture.owned()?;
    let control_path = owner.identity().control_directory().to_path_buf();
    drop(owner);
    let descriptor =
        wrappers::ConvertStringSecurityDescriptorToSecurityDescriptor("D:P(A;OICI;FA;;;WD)")?;
    let dacl = wrappers::GetSecurityDescriptorDacl(&descriptor)?;
    wrappers::SetNamedSecurityInfo(
        control_path.as_os_str(),
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
        None,
        None,
        dacl,
        None,
    )?;
    assert!(matches!(
        fixture.acquire(),
        Err(DesktopProfileError::UnsafePermissions { .. })
    ));
    Ok(())
}

#[cfg(windows)]
#[test]
fn rejects_a_present_null_control_dacl_without_panicking() -> Result<(), Box<dyn std::error::Error>>
{
    use windows_permissions::constants::{SeObjectType, SecurityInformation};
    use windows_permissions::wrappers;

    let fixture = DesktopProfileLeaseFixture::new()?;
    let owner = fixture.owned()?;
    let control_path = owner.identity().control_directory().to_path_buf();
    drop(owner);
    wrappers::SetNamedSecurityInfo(
        control_path.as_os_str(),
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
        None,
        None,
        None,
        None,
    )?;
    assert!(matches!(
        fixture.acquire(),
        Err(DesktopProfileError::UnsafePermissions { .. })
    ));
    Ok(())
}
