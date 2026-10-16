use chess::engine::package::digest::EnginePackageDigest;
use chess::engine::package::host::EnginePackageHost;
use chess::engine::package::manifest::{
    EnginePackageEntry, EnginePackageEnumTarget, EnginePackageManifest,
};
use chess::engine::package::selector::{EnginePackageSelector, EnginePackageSelectorError};

#[test]
fn host_policy_rejects_old_unknown_and_incompatible_targets()
-> Result<(), Box<dyn std::error::Error>> {
    for (target, version, distribution) in [
        (EnginePackageEnumTarget::MacosArm, "14.0", ""),
        (EnginePackageEnumTarget::WindowsX64, "11 (26100)", ""),
        (EnginePackageEnumTarget::LinuxX64, "22.04", "ubuntu"),
    ] {
        assert_eq!(
            EnginePackageHost::from_observation(target, version, distribution, true)?.target(),
            target
        );
        assert!(EnginePackageHost::from_observation(target, version, distribution, false).is_err());
    }
    for (target, version, distribution) in [
        (EnginePackageEnumTarget::MacosArm, "13.9", ""),
        (EnginePackageEnumTarget::WindowsX64, "10 (19045)", ""),
        (EnginePackageEnumTarget::LinuxX64, "22.03", "ubuntu"),
        (EnginePackageEnumTarget::LinuxX64, "24.04", "debian"),
        (EnginePackageEnumTarget::MacosArm, "unknown", ""),
    ] {
        assert!(EnginePackageHost::from_observation(target, version, distribution, true).is_err());
    }
    Ok(())
}

pub(super) struct EnginePackageFixture {
    directory: tempfile::TempDir,
    manifest: EnginePackageManifest,
}

impl EnginePackageFixture {
    pub(super) fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let bytes = b"verified engine fixture";
        let target = EnginePackageEnumTarget::MacosArm;
        std::fs::create_dir(directory.path().join("bin"))?;
        let path = directory.path().join(target.relative_path());
        std::fs::write(&path, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        }
        let entry = EnginePackageEntry::portable(
            target,
            u64::try_from(bytes.len())?,
            EnginePackageDigest::read(&bytes[..])?,
            "fixture compiler".to_owned(),
        )?;
        Ok(Self {
            directory,
            manifest: EnginePackageManifest::new(entry)?,
        })
    }

    pub(super) fn select(
        &self,
    ) -> Result<chess::engine::package::selector::EnginePackageSelected, EnginePackageSelectorError>
    {
        let host = EnginePackageHost::from_observation(
            EnginePackageEnumTarget::MacosArm,
            "14.0",
            "",
            true,
        )
        .map_err(|_error| EnginePackageSelectorError::MissingPackage)?;
        EnginePackageSelector::select(self.directory.path(), &self.manifest, host)
    }
}

#[test]
fn selection_verifies_bytes_and_rejects_missing_target() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let selected = fixture.select()?;
    assert_eq!(selected.target(), EnginePackageEnumTarget::MacosArm);
    std::fs::write(selected.executable(), b"tampered engine fixture")?;
    assert!(matches!(
        fixture.select(),
        Err(EnginePackageSelectorError::ChecksumMismatch)
    ));
    let windows =
        EnginePackageHost::from_observation(EnginePackageEnumTarget::WindowsX64, "11", "", true)?;
    assert!(matches!(
        EnginePackageSelector::select(fixture.directory.path(), &fixture.manifest, windows),
        Err(EnginePackageSelectorError::MissingPackage)
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn selection_refuses_symlink_escape_and_nonexecutable_files()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let fixture = EnginePackageFixture::new()?;
    let selected = fixture.select()?;
    std::fs::set_permissions(
        selected.executable(),
        std::fs::Permissions::from_mode(0o600),
    )?;
    assert!(matches!(
        fixture.select(),
        Err(EnginePackageSelectorError::InvalidExecutable)
    ));
    let external = tempfile::NamedTempFile::new()?;
    std::fs::remove_file(selected.executable())?;
    symlink(external.path(), selected.executable())?;
    assert!(matches!(
        fixture.select(),
        Err(EnginePackageSelectorError::InvalidExecutable)
    ));
    Ok(())
}

#[test]
fn oversized_manifest_is_bounded_before_deserialization() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = EnginePackageFixture::new()?;
    std::fs::write(
        fixture.directory.path().join("manifest.json"),
        vec![b' '; 65_537],
    )?;
    let host =
        EnginePackageHost::from_observation(EnginePackageEnumTarget::MacosArm, "14", "", true)?;
    assert!(matches!(
        EnginePackageSelector::load(fixture.directory.path(), host),
        Err(EnginePackageSelectorError::OversizedManifest)
    ));
    Ok(())
}
