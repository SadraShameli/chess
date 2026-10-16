use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chess::desktop::identity::DesktopRequestId;
use chess::desktop::profile::activation::DesktopProfileActivationBinding;
use chess::desktop::profile::owner_record::{
    DesktopProfileEnumPhase, DesktopProfileOwnerRecord, DesktopProfileOwnerRecordBoundEndpoint,
    DesktopProfileOwnerRecordEndpoint, DesktopProfileOwnerRecordError,
    DesktopProfileOwnerRecordPublisher,
};
use chess::desktop::profile::{
    DesktopProfileAdmission, DesktopProfileEnumAdmission, DesktopProfileError, DesktopProfileId,
    DesktopProfileLease, DesktopProfileOsLock, DesktopProfileSecurity,
};
use tempfile::TempDir;

pub(crate) struct DesktopProfileOwnerRecordFixture {
    directory: TempDir,
    pub(crate) lease: Box<DesktopProfileLease>,
    pub(crate) security: DesktopProfileSecurity,
}

impl DesktopProfileOwnerRecordFixture {
    pub(crate) fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = TempDir::new()?;
        let security = DesktopProfileSecurity::current()?;
        let admission = DesktopProfileAdmission::new(security.clone(), DesktopProfileOsLock);
        let DesktopProfileEnumAdmission::Owned(lease) =
            admission.acquire(&directory.path().join("profile"), Self::profile_id()?)?
        else {
            return Err(std::io::Error::other("Expected profile ownership").into());
        };
        Ok(Self {
            directory,
            lease,
            security,
        })
    }

    fn profile_id() -> Result<DesktopProfileId, DesktopProfileError> {
        DesktopProfileId::from_str("d92f840f-e575-4654-92bd-79b3ba8b8e55")
    }

    fn binding(&self) -> Result<DesktopProfileActivationBinding, Box<dyn std::error::Error>> {
        Ok(DesktopProfileActivationBinding::new(
            self.lease.identity().profile_id(),
            DesktopRequestId::from_str("191b8766-233c-4a15-820a-65f3e042b730")?,
        ))
    }

    fn diagnostic_pid() -> Result<NonZeroU32, Box<dyn std::error::Error>> {
        NonZeroU32::new(std::process::id())
            .ok_or_else(|| std::io::Error::other("Expected a nonzero process identifier").into())
    }

    pub(crate) fn record_path(&self, filename: &str) -> PathBuf {
        self.lease.identity().control_directory().join(filename)
    }

    pub(crate) fn write_private_record(
        &self,
        filename: &str,
        bytes: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let path = self.record_path(filename);
        if !path.exists() {
            let binding = self.binding()?;
            let proof = DesktopProfileOwnerRecordEndpointProof::new(binding)?;
            let pid = Self::diagnostic_pid()?;
            DesktopProfileOwnerRecordPublisher::publish(&self.lease, binding, &proof, pid)?;
            let owner = self.record_path("owner.json");
            if owner != path {
                fs::rename(owner, &path)?;
            }
        }
        fs::write(&path, bytes)?;
        Ok(())
    }

    #[cfg(unix)]
    fn untouched_sentinel(&self) -> PathBuf {
        self.directory.path().join("outside")
    }

    fn missing_profile_path(&self) -> PathBuf {
        self.directory.path().join("absent")
    }
}

struct DesktopProfileOwnerRecordEndpointProof {
    binding: DesktopProfileActivationBinding,
    endpoint: DesktopProfileOwnerRecordEndpoint,
}

impl DesktopProfileOwnerRecordEndpointProof {
    fn new(
        binding: DesktopProfileActivationBinding,
    ) -> Result<Self, DesktopProfileOwnerRecordError> {
        #[cfg(unix)]
        let endpoint =
            DesktopProfileOwnerRecordEndpoint::unix_socket(Path::new("/tmp/chess.sock"))?;
        #[cfg(windows)]
        let endpoint = DesktopProfileOwnerRecordEndpoint::windows_pipe(r"\\.\pipe\chess-test")?;
        Ok(Self { binding, endpoint })
    }
}

impl DesktopProfileOwnerRecordBoundEndpoint for DesktopProfileOwnerRecordEndpointProof {
    fn confirm_bound(
        &self,
        binding: DesktopProfileActivationBinding,
    ) -> Result<DesktopProfileOwnerRecordEndpoint, DesktopProfileOwnerRecordError> {
        if binding != self.binding {
            return Err(DesktopProfileOwnerRecordError::BindingMismatch);
        }
        Ok(self.endpoint.clone())
    }
}

#[test]
fn publishes_only_the_bound_generation_and_retains_the_same_exclusive_lease()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileOwnerRecordFixture::new()?;
    let binding = fixture.binding()?;
    let proof = DesktopProfileOwnerRecordEndpointProof::new(binding)?;
    let pid = DesktopProfileOwnerRecordFixture::diagnostic_pid()?;
    let publisher =
        DesktopProfileOwnerRecordPublisher::publish(&fixture.lease, binding, &proof, pid)?;
    assert_eq!(
        publisher.record().phase(),
        DesktopProfileEnumPhase::Starting
    );
    let record = DesktopProfileOwnerRecord::read(
        fixture.lease.identity().data_directory(),
        &fixture.security,
    )?
    .ok_or("Expected a published owner record")?;
    assert_eq!(record.binding(), binding);
    assert_eq!(record.endpoint(), &proof.endpoint);
    assert_eq!(record.diagnostic_pid(), pid);
    let admission = DesktopProfileAdmission::new(fixture.security.clone(), DesktopProfileOsLock);
    assert!(matches!(
        admission.acquire(fixture.lease.identity().data_directory(), binding.profile())?,
        DesktopProfileEnumAdmission::Busy
    ));
    fixture.lease.verify_identity()?;
    Ok(())
}

#[test]
fn refuses_endpoint_proof_for_another_generation_without_publishing()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileOwnerRecordFixture::new()?;
    let binding = fixture.binding()?;
    let proof = DesktopProfileOwnerRecordEndpointProof::new(DesktopProfileActivationBinding::new(
        binding.profile(),
        DesktopRequestId::from_str("34fa257f-deec-4988-a5e8-3af16487d77d")?,
    ))?;
    let pid = DesktopProfileOwnerRecordFixture::diagnostic_pid()?;
    assert!(matches!(
        DesktopProfileOwnerRecordPublisher::publish(&fixture.lease, binding, &proof, pid),
        Err(DesktopProfileOwnerRecordError::BindingMismatch)
    ));
    assert!(!fixture.record_path("owner.json").exists());
    Ok(())
}

#[test]
fn canceled_shutdown_restores_only_the_exact_previous_owned_phase()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileOwnerRecordFixture::new()?;
    let binding = fixture.binding()?;
    let proof = DesktopProfileOwnerRecordEndpointProof::new(binding)?;
    let pid = DesktopProfileOwnerRecordFixture::diagnostic_pid()?;
    let mut publisher =
        DesktopProfileOwnerRecordPublisher::publish(&fixture.lease, binding, &proof, pid)?;
    publisher.transition(DesktopProfileEnumPhase::Recovery)?;
    publisher.transition(DesktopProfileEnumPhase::ShuttingDown)?;
    assert!(matches!(
        publisher.transition(DesktopProfileEnumPhase::Ready),
        Err(DesktopProfileOwnerRecordError::InvalidPhaseTransition { .. })
    ));
    publisher.transition(DesktopProfileEnumPhase::Recovery)?;
    let bytes = fs::read(fixture.record_path("owner.json"))?;
    assert!(matches!(
        publisher.transition(DesktopProfileEnumPhase::Starting),
        Err(DesktopProfileOwnerRecordError::InvalidPhaseTransition { .. })
    ));
    assert_eq!(fs::read(fixture.record_path("owner.json"))?, bytes);
    assert_eq!(
        publisher.record().phase(),
        DesktopProfileEnumPhase::Recovery
    );
    Ok(())
}

#[test]
fn missing_owner_metadata_read_does_not_create_profile_or_control_files()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileOwnerRecordFixture::new()?;
    let missing = fixture.missing_profile_path();
    assert!(
        DesktopProfileOwnerRecord::read(&missing, &DesktopProfileSecurity::current()?)?.is_none()
    );
    assert!(!missing.exists());
    Ok(())
}

#[test]
fn owner_reader_rejects_oversized_future_and_unknown_field_records_without_repair()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopProfileOwnerRecordFixture::new()?;
    let oversized = vec![b'x'; 4097];
    fixture.write_private_record("owner.json", &oversized)?;
    assert!(matches!(
        DesktopProfileOwnerRecord::read(
            fixture.lease.identity().data_directory(),
            &fixture.security,
        ),
        Err(DesktopProfileOwnerRecordError::Profile(
            DesktopProfileError::ControlRecordTooLarge { maximum: 4096, .. }
        ))
    ));
    assert_eq!(fs::read(fixture.record_path("owner.json"))?, oversized);
    let future = br#"{"version":2,"record":{}}"#;
    fixture.write_private_record("owner.json", future)?;
    assert!(matches!(
        DesktopProfileOwnerRecord::read(
            fixture.lease.identity().data_directory(),
            &fixture.security,
        ),
        Err(DesktopProfileOwnerRecordError::Profile(
            DesktopProfileError::UnsupportedControlRecordVersion { received: 2, .. }
        ))
    ));
    assert_eq!(fs::read(fixture.record_path("owner.json"))?, future);
    let binding = fixture.binding()?;
    let proof = DesktopProfileOwnerRecordEndpointProof::new(binding)?;
    DesktopProfileOwnerRecordPublisher::publish(
        &fixture.lease,
        binding,
        &proof,
        DesktopProfileOwnerRecordFixture::diagnostic_pid()?,
    )?;
    let mut unknown = fs::read(fixture.record_path("owner.json"))?;
    assert_eq!(unknown.pop(), Some(b'}'));
    unknown.extend_from_slice(br#", "unexpected":true}"#);
    fixture.write_private_record("owner.json", &unknown)?;
    assert!(matches!(
        DesktopProfileOwnerRecord::read(
            fixture.lease.identity().data_directory(),
            &fixture.security,
        ),
        Err(DesktopProfileOwnerRecordError::Profile(
            DesktopProfileError::InvalidControlRecord { .. }
        ))
    ));
    assert_eq!(fs::read(fixture.record_path("owner.json"))?, unknown);
    Ok(())
}

#[cfg(unix)]
#[test]
fn owner_publication_refuses_to_replace_an_existing_symlink()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let fixture = DesktopProfileOwnerRecordFixture::new()?;
    let outside = fixture.untouched_sentinel();
    fs::write(&outside, b"untouched")?;
    symlink(&outside, fixture.record_path("owner.json"))?;
    let binding = fixture.binding()?;
    let proof = DesktopProfileOwnerRecordEndpointProof::new(binding)?;
    let pid = DesktopProfileOwnerRecordFixture::diagnostic_pid()?;
    assert!(
        DesktopProfileOwnerRecordPublisher::publish(&fixture.lease, binding, &proof, pid).is_err()
    );
    assert_eq!(fs::read(outside)?, b"untouched");
    assert!(fs::symlink_metadata(fixture.record_path("owner.json"))?.is_symlink());
    Ok(())
}

#[test]
fn activation_endpoint_constructors_refuse_unbounded_or_nonlocal_identifiers() {
    assert!(DesktopProfileOwnerRecordEndpoint::unix_socket(Path::new("relative.sock")).is_err());
    assert!(DesktopProfileOwnerRecordEndpoint::unix_socket(Path::new("/tmp/../escape")).is_err());
    let maximum_unix = format!("/{}", "x".repeat(102));
    let oversized_unix = format!("{maximum_unix}x");
    assert!(DesktopProfileOwnerRecordEndpoint::unix_socket(Path::new(&maximum_unix)).is_ok());
    assert!(DesktopProfileOwnerRecordEndpoint::unix_socket(Path::new(&oversized_unix)).is_err());
    assert!(DesktopProfileOwnerRecordEndpoint::unix_socket(Path::new("/tmp/name\0extra")).is_err());
    assert!(DesktopProfileOwnerRecordEndpoint::windows_pipe(r"\\remote\pipe\chess").is_err());
    assert!(DesktopProfileOwnerRecordEndpoint::windows_pipe(r"\\.\pipe\").is_err());
    assert!(DesktopProfileOwnerRecordEndpoint::windows_pipe(r"\\.\pipe\name\extra").is_err());
    assert!(DesktopProfileOwnerRecordEndpoint::windows_pipe("\\\\.\\pipe\\name\0extra").is_err());
    let maximum_windows = format!(r"\\.\pipe\{}", "x".repeat(128));
    let oversized_windows = format!("{maximum_windows}x");
    assert!(DesktopProfileOwnerRecordEndpoint::windows_pipe(&maximum_windows).is_ok());
    assert!(DesktopProfileOwnerRecordEndpoint::windows_pipe(&oversized_windows).is_err());
}
