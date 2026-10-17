use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Barrier;

use chess::desktop::identity::DesktopRequestId;
use chess::desktop::native::startup::{
    DesktopNativeStartup, DesktopNativeStartupEnumAdmission, DesktopNativeStartupError,
    DesktopNativeStartupOwner,
};
use chess::desktop::profile::activation::{
    DesktopProfileActivationBinding, DesktopProfileActivationExchange,
};
use chess::desktop::profile::installation::{
    DesktopProfileInstallationEnumState, DesktopProfileInstallationIntent,
};
use chess::desktop::profile::owner_record::{
    DesktopProfileEnumPhase, DesktopProfileOwnerRecordBoundEndpoint,
    DesktopProfileOwnerRecordEndpoint, DesktopProfileOwnerRecordError,
    DesktopProfileOwnerRecordPublisher,
};
use chess::desktop::profile::{
    DesktopProfileAdmission, DesktopProfileEnumAdmission, DesktopProfileError, DesktopProfileId,
    DesktopProfileIdentity, DesktopProfileLockProvider, DesktopProfileOsLock,
    DesktopProfileSecurity,
};
use tempfile::TempDir;

struct DesktopNativeStartupFixture {
    directory: TempDir,
    security: DesktopProfileSecurity,
    startup: DesktopNativeStartup<DesktopProfileOsLock>,
}

impl DesktopNativeStartupFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let security = DesktopProfileSecurity::current()?;
        Ok(Self {
            directory: TempDir::new()?,
            startup: DesktopNativeStartup::new(security.clone(), DesktopProfileOsLock),
            security,
        })
    }

    fn data(&self) -> PathBuf {
        self.directory.path().join("profile")
    }

    fn profile() -> Result<DesktopProfileId, DesktopProfileError> {
        DesktopProfileId::from_str("d92f840f-e575-4654-92bd-79b3ba8b8e55")
    }

    fn generation() -> Result<DesktopRequestId, chess::desktop::error::DesktopError> {
        DesktopRequestId::from_str("191b8766-233c-4a15-820a-65f3e042b730")
    }

    fn next_generation() -> Result<DesktopRequestId, chess::desktop::error::DesktopError> {
        DesktopRequestId::from_str("a129d71c-ed98-41aa-a71c-9840659bd468")
    }

    fn admit(&self) -> Result<DesktopNativeStartupEnumAdmission, Box<dyn std::error::Error>> {
        Ok(self
            .startup
            .admit(&self.data(), Self::profile()?, Self::generation()?)?)
    }

    fn owned(&self) -> Result<DesktopNativeStartupOwner, Box<dyn std::error::Error>> {
        let DesktopNativeStartupEnumAdmission::Owned(owner) = self.admit()? else {
            return Err(
                std::io::Error::other("The fixture was not admitted as a writable owner").into(),
            );
        };
        Ok(owner)
    }

    fn publish<'a>(
        &self,
        owner: &'a DesktopNativeStartupOwner,
    ) -> Result<DesktopProfileOwnerRecordPublisher<'a>, Box<dyn std::error::Error>> {
        let proof = DesktopNativeStartupBoundEndpointFixture::new(owner.binding())?;
        let pid = NonZeroU32::new(std::process::id())
            .ok_or_else(|| std::io::Error::other("The test process identifier is zero"))?;
        Ok(DesktopProfileOwnerRecordPublisher::publish(
            owner.lease(),
            owner.binding(),
            &proof,
            pid,
        )?)
    }

    fn owner_path(owner: &DesktopNativeStartupOwner) -> PathBuf {
        owner
            .lease()
            .identity()
            .control_directory()
            .join("owner.json")
    }

    fn identity_path(owner: &DesktopNativeStartupOwner) -> PathBuf {
        owner
            .lease()
            .identity()
            .control_directory()
            .join("profile.id")
    }

    fn write_installation(
        &self,
        owner: &DesktopNativeStartupOwner,
        bytes: &[u8],
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        self.publish(owner)?;
        let destination = owner
            .lease()
            .identity()
            .control_directory()
            .join("installation.json");
        fs::rename(Self::owner_path(owner), &destination)?;
        fs::write(&destination, bytes)?;
        Ok(destination)
    }

    fn installation_bytes(owner: &DesktopNativeStartupOwner, state: &str) -> Vec<u8> {
        format!(
            r#"{{"version":1,"record":{{"profile_id":"{}","owner_generation":"{}","helper_generation":"34fa257f-deec-4988-a5e8-3af16487d77d","package_digest":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","package_version":"1.2.3","safety_backup_id":"4420fb16-b3ed-491e-ab96-80b34890b62e","data_schema":1,"state":{state}}}}}"#,
            owner.binding().profile(),
            String::from(owner.binding().generation()),
        )
        .into_bytes()
    }

    fn assert_services_absent(&self) -> Result<(), Box<dyn std::error::Error>> {
        let entries = fs::read_dir(self.data())?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(entries, vec![std::ffi::OsString::from("control")]);
        assert!(tokio::runtime::Handle::try_current().is_err());
        Ok(())
    }
}

struct DesktopNativeStartupBoundEndpointFixture {
    binding: DesktopProfileActivationBinding,
    endpoint: DesktopProfileOwnerRecordEndpoint,
}

impl DesktopNativeStartupBoundEndpointFixture {
    fn new(
        binding: DesktopProfileActivationBinding,
    ) -> Result<Self, DesktopProfileOwnerRecordError> {
        #[cfg(unix)]
        let endpoint =
            DesktopProfileOwnerRecordEndpoint::unix_socket(Path::new("/tmp/chess-startup.sock"))?;
        #[cfg(windows)]
        let endpoint = DesktopProfileOwnerRecordEndpoint::windows_pipe(r"\\.\pipe\chess-startup")?;
        Ok(Self { binding, endpoint })
    }
}

impl DesktopProfileOwnerRecordBoundEndpoint for DesktopNativeStartupBoundEndpointFixture {
    fn confirm_bound(
        &self,
        binding: DesktopProfileActivationBinding,
    ) -> Result<DesktopProfileOwnerRecordEndpoint, DesktopProfileOwnerRecordError> {
        if self.binding != binding {
            return Err(DesktopProfileOwnerRecordError::BindingMismatch);
        }
        Ok(self.endpoint.clone())
    }
}

struct DesktopNativeStartupLockFailureFixture {
    kind: std::io::ErrorKind,
}

impl DesktopProfileLockProvider for DesktopNativeStartupLockFailureFixture {
    fn try_lock(&self, _file: &fs::File) -> Result<(), fs::TryLockError> {
        Err(fs::TryLockError::Error(std::io::Error::from(self.kind)))
    }
}

#[test]
fn normal_startup_is_synchronous_and_preserves_the_sole_guard_before_services()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeStartupFixture::new()?;
    let owner = fixture.owned()?;
    assert_eq!(
        owner.binding().profile(),
        DesktopNativeStartupFixture::profile()?
    );
    assert_eq!(
        owner.binding().generation(),
        DesktopNativeStartupFixture::generation()?
    );
    fixture.assert_services_absent()?;
    let anchor = same_file::Handle::from_path(owner.lease().identity().lock_path())?;
    let (lease, binding) = owner.into_parts();
    assert_eq!(binding.profile(), lease.identity().profile_id());
    assert!(matches!(
        fixture.admit()?,
        DesktopNativeStartupEnumAdmission::Busy
    ));
    assert_eq!(
        same_file::Handle::from_path(lease.identity().lock_path())?,
        anchor
    );
    lease.verify_identity()?;
    drop(lease);
    let next = fixture.owned()?;
    assert_eq!(
        same_file::Handle::from_path(next.lease().identity().lock_path())?,
        anchor
    );
    Ok(())
}

#[test]
fn a_busy_owner_without_metadata_never_admits_a_second_writer()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeStartupFixture::new()?;
    let owner = fixture.owned()?;
    for _trial in 0..DesktopProfileActivationExchange::CLIENT_TRIALS {
        assert!(matches!(
            fixture.admit()?,
            DesktopNativeStartupEnumAdmission::Busy
        ));
        owner.lease().verify_identity()?;
    }
    fixture.assert_services_absent()?;
    assert!(!DesktopNativeStartupFixture::owner_path(&owner).exists());
    Ok(())
}

#[test]
fn published_owner_metadata_only_produces_a_bound_activation_target()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeStartupFixture::new()?;
    let owner = fixture.owned()?;
    let mut publication = fixture.publish(&owner)?;
    let phases = [
        DesktopProfileEnumPhase::Starting,
        DesktopProfileEnumPhase::Ready,
        DesktopProfileEnumPhase::Recovery,
        DesktopProfileEnumPhase::Ready,
        DesktopProfileEnumPhase::InstallPending,
        DesktopProfileEnumPhase::ShuttingDown,
    ];
    for phase in phases {
        publication.transition(phase)?;
        let DesktopNativeStartupEnumAdmission::ActivationOnly(record) = fixture.admit()? else {
            return Err(
                std::io::Error::other("A busy published owner must only yield activation").into(),
            );
        };
        assert_eq!(record.binding(), owner.binding());
        assert_eq!(record.endpoint(), publication.record().endpoint());
        assert_eq!(record.phase(), phase);
    }
    fixture.assert_services_absent()?;
    owner.lease().verify_identity()?;
    Ok(())
}

#[test]
fn stale_owner_metadata_never_replaces_successful_exclusive_acquisition()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeStartupFixture::new()?;
    let owner = fixture.owned()?;
    fixture.publish(&owner)?;
    let path = DesktopNativeStartupFixture::owner_path(&owner);
    let stale = fs::read(&path)?;
    let expected_profile = owner.binding().profile();
    drop(owner);
    let proposed = DesktopProfileId::from_str("b726b8e1-813c-4190-8be1-b17c07b0aa16")?;
    let DesktopNativeStartupEnumAdmission::Owned(next) = fixture.startup.admit(
        &fixture.data(),
        proposed,
        DesktopNativeStartupFixture::next_generation()?,
    )?
    else {
        return Err(std::io::Error::other("Stale metadata prevented exclusive acquisition").into());
    };
    assert_eq!(next.binding().profile(), expected_profile);
    assert_eq!(
        next.binding().generation(),
        DesktopNativeStartupFixture::next_generation()?
    );
    assert_eq!(fs::read(path)?, stale);
    fixture.assert_services_absent()?;
    Ok(())
}

#[test]
fn activation_records_must_match_the_persisted_profile_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeStartupFixture::new()?;
    let owner = fixture.owned()?;
    fixture.publish(&owner)?;
    let path = DesktopNativeStartupFixture::owner_path(&owner);
    let original = fs::read_to_string(&path)?;
    let foreign = DesktopProfileId::from_str("b726b8e1-813c-4190-8be1-b17c07b0aa16")?;
    let changed = original.replace(&owner.binding().profile().to_string(), &foreign.to_string());
    fs::write(&path, &changed)?;
    assert!(matches!(
        fixture.startup.admit(&fixture.data(), DesktopNativeStartupFixture::profile()?, DesktopNativeStartupFixture::generation()?),
        Err(DesktopNativeStartupError::ProfileBindingMismatch { owner: record, stored })
            if record == foreign && stored == owner.binding().profile()
    ));
    assert_eq!(fs::read_to_string(path)?, changed);
    owner.lease().verify_identity()?;
    fixture.assert_services_absent()?;
    Ok(())
}

#[test]
fn an_owner_record_without_its_destination_identity_fails_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeStartupFixture::new()?;
    let owner = fixture.owned()?;
    fixture.publish(&owner)?;
    let identity = DesktopNativeStartupFixture::identity_path(&owner);
    fs::remove_file(&identity)?;
    assert!(matches!(
        fixture.startup.admit(
            &fixture.data(),
            DesktopNativeStartupFixture::profile()?,
            DesktopNativeStartupFixture::generation()?
        ),
        Err(DesktopNativeStartupError::MissingProfileIdentity)
    ));
    assert!(!identity.exists());
    fixture.assert_services_absent()?;
    Ok(())
}

#[test]
fn malformed_destination_identities_cannot_authorize_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeStartupFixture::new()?;
    let owner = fixture.owned()?;
    fixture.publish(&owner)?;
    let identity = DesktopNativeStartupFixture::identity_path(&owner);
    let oversized = [b'x'; 37];
    let cases: [&[u8]; 5] = [
        b"",
        b"invalid",
        &[0xff],
        b"00000000-0000-0000-0000-000000000000",
        &oversized,
    ];
    for bytes in cases {
        fs::write(&identity, bytes)?;
        assert!(matches!(
            fixture.startup.admit(
                &fixture.data(),
                DesktopNativeStartupFixture::profile()?,
                DesktopNativeStartupFixture::generation()?
            ),
            Err(DesktopNativeStartupError::Profile(_))
        ));
        assert_eq!(fs::read(&identity)?, bytes);
    }
    fixture.assert_services_absent()?;
    Ok(())
}

#[test]
fn every_installation_phase_blocks_services_and_releases_the_guard_before_status()
-> Result<(), Box<dyn std::error::Error>> {
    let states = [
        r#"{"phase":"prepared"}"#,
        r#"{"phase":"committed"}"#,
        r#"{"phase":"outcome","result":"succeeded"}"#,
        r#"{"phase":"outcome","result":"failed"}"#,
        r#"{"phase":"outcome","result":"unknown"}"#,
    ];
    for state in states {
        let fixture = DesktopNativeStartupFixture::new()?;
        let owner = fixture.owned()?;
        let bytes = DesktopNativeStartupFixture::installation_bytes(&owner, state);
        let path = fixture.write_installation(&owner, &bytes)?;
        let profile = owner.binding().profile();
        drop(owner);
        let DesktopNativeStartupEnumAdmission::InstallationBlocked(status) = fixture.admit()?
        else {
            return Err(
                std::io::Error::other("A durable intent admitted ordinary services").into(),
            );
        };
        assert_eq!(status.binding().profile(), profile);
        assert_eq!(status.intent().profile_id(), profile);
        assert_eq!(status.intent().package_version().to_string(), "1.2.3");
        assert_eq!(
            status.intent().state(),
            serde_json::from_str::<DesktopProfileInstallationEnumState>(state)?
        );
        let admission =
            DesktopProfileAdmission::new(fixture.security.clone(), DesktopProfileOsLock);
        let DesktopProfileEnumAdmission::Owned(guard) =
            admission.acquire(&fixture.data(), profile)?
        else {
            return Err(
                std::io::Error::other("The status result retained the installation guard").into(),
            );
        };
        guard.verify_identity()?;
        assert_eq!(fs::read(path)?, bytes);
        fixture.assert_services_absent()?;
        assert_eq!(status.intent().profile_id(), profile);
    }
    Ok(())
}

#[test]
fn invalid_installation_intents_fail_before_services_and_release_the_acquired_guard()
-> Result<(), Box<dyn std::error::Error>> {
    let oversized = [b'x'; 8193];
    let cases: [&[u8]; 4] = [
        b"",
        br#"{"version":2,"record":{}}"#,
        br#"{"version":1,"record":{"state":{"phase":"prepared"}}}"#,
        &oversized,
    ];
    for bytes in cases {
        let fixture = DesktopNativeStartupFixture::new()?;
        let owner = fixture.owned()?;
        let path = fixture.write_installation(&owner, bytes)?;
        let profile = owner.binding().profile();
        drop(owner);
        assert!(matches!(
            fixture.startup.admit(
                &fixture.data(),
                profile,
                DesktopNativeStartupFixture::generation()?
            ),
            Err(DesktopNativeStartupError::Installation(_))
        ));
        let admission =
            DesktopProfileAdmission::new(fixture.security.clone(), DesktopProfileOsLock);
        let DesktopProfileEnumAdmission::Owned(guard) =
            admission.acquire(&fixture.data(), profile)?
        else {
            return Err(
                std::io::Error::other("Failed installation inspection retained ownership").into(),
            );
        };
        guard.verify_identity()?;
        assert_eq!(fs::read(path)?, bytes);
        fixture.assert_services_absent()?;
    }
    Ok(())
}

#[test]
fn read_only_identity_inspection_creates_nothing_and_does_not_require_the_lock()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeStartupFixture::new()?;
    assert_eq!(
        DesktopProfileIdentity::read_existing(&fixture.data(), &fixture.security)?,
        None
    );
    assert!(!fixture.data().exists());
    let owner = fixture.owned()?;
    let path = DesktopNativeStartupFixture::identity_path(&owner);
    let before = fs::read(&path)?;
    assert_eq!(
        DesktopProfileIdentity::read_existing(&fixture.data(), &fixture.security)?,
        Some(owner.binding().profile()),
    );
    assert_eq!(fs::read(path)?, before);
    assert!(matches!(
        fixture.admit()?,
        DesktopNativeStartupEnumAdmission::Busy
    ));
    owner.lease().verify_identity()?;
    Ok(())
}

#[test]
fn unsupported_or_failed_locking_never_uses_metadata_as_ownership_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeStartupFixture::new()?;
    let owner = fixture.owned()?;
    fixture.publish(&owner)?;
    let path = DesktopNativeStartupFixture::owner_path(&owner);
    fs::write(&path, b"invalid metadata")?;
    for kind in [
        std::io::ErrorKind::Unsupported,
        std::io::ErrorKind::PermissionDenied,
    ] {
        let startup = DesktopNativeStartup::new(
            fixture.security.clone(),
            DesktopNativeStartupLockFailureFixture { kind },
        );
        let result = startup.admit(
            &fixture.data(),
            DesktopNativeStartupFixture::profile()?,
            DesktopNativeStartupFixture::generation()?,
        );
        if kind == std::io::ErrorKind::Unsupported {
            assert!(matches!(
                result,
                Err(DesktopNativeStartupError::Profile(
                    DesktopProfileError::LockUnsupported { .. }
                ))
            ));
        } else {
            assert!(matches!(
                result,
                Err(DesktopNativeStartupError::Profile(
                    DesktopProfileError::LockUnavailable { .. }
                ))
            ));
        }
    }
    assert_eq!(fs::read(path)?, b"invalid metadata");
    fixture.assert_services_absent()?;
    Ok(())
}

#[test]
fn simultaneous_first_launch_produces_exactly_one_owner_and_no_services()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeStartupFixture::new()?;
    let start = Barrier::new(2);
    let acquired = Barrier::new(2);
    let generations = [
        DesktopNativeStartupFixture::generation()?,
        DesktopNativeStartupFixture::next_generation()?,
    ];
    let profile = DesktopNativeStartupFixture::profile()?;
    let data = fixture.data();
    let outcomes = std::thread::scope(|scope| {
        let handles: Vec<_> = generations
            .into_iter()
            .map(|generation| {
                let fixture = &fixture;
                let data = &data;
                let start = &start;
                let acquired = &acquired;
                scope.spawn(move || {
                    start.wait();
                    let outcome = fixture.startup.admit(data, profile, generation);
                    acquired.wait();
                    outcome
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                let result = handle.join().map_err(|payload| {
                    std::io::Error::other(format!("A startup contender panicked: {payload:?}"))
                })?;
                Ok::<_, Box<dyn std::error::Error>>(result?)
            })
            .collect::<Result<Vec<_>, _>>()
    })?;
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, DesktopNativeStartupEnumAdmission::Owned(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, DesktopNativeStartupEnumAdmission::Busy))
            .count(),
        1
    );
    fixture.assert_services_absent()?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn aliases_share_the_same_owner_and_persisted_activation_binding()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeStartupFixture::new()?;
    let owner = fixture.owned()?;
    fixture.publish(&owner)?;
    let alias = fixture.directory.path().join("alias");
    std::os::unix::fs::symlink(fixture.data(), &alias)?;
    let DesktopNativeStartupEnumAdmission::ActivationOnly(record) = fixture.startup.admit(
        &alias,
        DesktopNativeStartupFixture::profile()?,
        DesktopNativeStartupFixture::next_generation()?,
    )?
    else {
        return Err(
            std::io::Error::other("A profile alias failed activation-only admission").into(),
        );
    };
    assert_eq!(record.binding(), owner.binding());
    let canonical = owner.lease().identity().data_directory().to_path_buf();
    drop(owner);
    let DesktopNativeStartupEnumAdmission::Owned(next) = fixture.startup.admit(
        &alias,
        DesktopNativeStartupFixture::profile()?,
        DesktopNativeStartupFixture::next_generation()?,
    )?
    else {
        return Err(
            std::io::Error::other("A released profile alias failed exclusive admission").into(),
        );
    };
    assert_eq!(next.lease().identity().data_directory(), canonical);
    fixture.assert_services_absent()?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn identity_inspection_refuses_symlinks_and_hardlinks_without_repairing_them()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeStartupFixture::new()?;
    let owner = fixture.owned()?;
    let path = DesktopNativeStartupFixture::identity_path(&owner);
    let retained = fixture.directory.path().join("retained.id");
    fs::rename(&path, &retained)?;
    std::os::unix::fs::symlink(&retained, &path)?;
    assert!(matches!(
        DesktopProfileIdentity::read_existing(&fixture.data(), &fixture.security),
        Err(DesktopProfileError::UnsafeIdentity { .. })
    ));
    assert!(fs::symlink_metadata(&path)?.is_symlink());
    fs::remove_file(&path)?;
    fs::rename(retained, &path)?;
    let alias = fixture.directory.path().join("hardlink.id");
    fs::hard_link(&path, &alias)?;
    assert!(matches!(
        DesktopProfileIdentity::read_existing(&fixture.data(), &fixture.security),
        Err(DesktopProfileError::UnsafeIdentity { .. })
    ));
    assert!(alias.exists());
    fs::remove_file(alias)?;
    assert_eq!(
        DesktopProfileIdentity::read_existing(&fixture.data(), &fixture.security)?,
        Some(owner.binding().profile())
    );
    Ok(())
}

fn current_version_bytes(owner: &DesktopNativeStartupOwner, state: &str) -> Vec<u8> {
    String::from_utf8_lossy(&DesktopNativeStartupFixture::installation_bytes(owner, state))
        .replace(
            "\"package_version\":\"1.2.3\"",
            &format!("\"package_version\":\"{}\"", env!("CARGO_PKG_VERSION")),
        )
        .into_bytes()
}

#[test]
fn a_matching_installed_version_with_a_successful_intent_admits_and_clears()
-> Result<(), Box<dyn std::error::Error>> {
    for state in [
        r#"{"phase":"committed"}"#,
        r#"{"phase":"outcome","result":"succeeded"}"#,
    ] {
        let fixture = DesktopNativeStartupFixture::new()?;
        let owner = fixture.owned()?;
        let bytes = current_version_bytes(&owner, state);
        let path = fixture.write_installation(&owner, &bytes)?;
        drop(owner);
        let DesktopNativeStartupEnumAdmission::Owned(owner) = fixture.admit()? else {
            return Err(std::io::Error::other("A completed update stayed blocked").into());
        };
        assert!(path.exists());
        DesktopProfileInstallationIntent::clear(owner.lease())?;
        assert!(!path.exists());
        drop(owner);
        assert!(matches!(
            fixture.admit()?,
            DesktopNativeStartupEnumAdmission::Owned(_)
        ));
    }
    Ok(())
}

#[test]
fn prepared_failed_and_unknown_intents_stay_blocked_even_at_the_installed_version()
-> Result<(), Box<dyn std::error::Error>> {
    for state in [
        r#"{"phase":"prepared"}"#,
        r#"{"phase":"outcome","result":"failed"}"#,
        r#"{"phase":"outcome","result":"unknown"}"#,
    ] {
        let fixture = DesktopNativeStartupFixture::new()?;
        let owner = fixture.owned()?;
        let bytes = current_version_bytes(&owner, state);
        let path = fixture.write_installation(&owner, &bytes)?;
        drop(owner);
        assert!(matches!(
            fixture.admit()?,
            DesktopNativeStartupEnumAdmission::InstallationBlocked(_)
        ));
        assert_eq!(fs::read(path)?, bytes);
    }
    Ok(())
}
