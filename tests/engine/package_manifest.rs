use chess::engine::package::manifest::{
    EnginePackageEntry, EnginePackageEnumTarget, EnginePackageManifest,
};
use chess::package::digest::PackageDigest;

#[test]
fn deserialization_cannot_bypass_engine_provenance_or_package_constraints()
-> Result<(), Box<dyn std::error::Error>> {
    let digest = PackageDigest::read(&b"engine fixture"[..])?;
    let entry = EnginePackageEntry::portable(
        EnginePackageEnumTarget::MacosArm,
        14,
        digest,
        "fixture compiler".to_owned(),
    )?;
    let manifest = EnginePackageManifest::new(entry)?;
    let valid = serde_json::to_value(manifest)?;
    assert!(serde_json::from_value::<EnginePackageManifest>(valid.clone()).is_ok());
    let mut invalid = valid.clone();
    invalid["source_commit"] = "wrong source".into();
    assert!(serde_json::from_value::<EnginePackageManifest>(invalid).is_err());
    let mut invalid = valid.clone();
    invalid["packages"][0]["executable"] = "../../untrusted".into();
    assert!(serde_json::from_value::<EnginePackageManifest>(invalid).is_err());
    let mut invalid = valid.clone();
    invalid["packages"][0]["syzygy"] = false.into();
    assert!(serde_json::from_value::<EnginePackageManifest>(invalid).is_err());
    let mut invalid = valid.clone();
    invalid["network_bytes"] = 1.into();
    assert!(serde_json::from_value::<EnginePackageManifest>(invalid).is_err());
    let mut invalid = valid;
    invalid["packages"][0]["bytes"] = 0.into();
    assert!(serde_json::from_value::<EnginePackageManifest>(invalid).is_err());
    Ok(())
}

#[test]
fn duplicate_packages_and_noncanonical_checksums_are_refused()
-> Result<(), Box<dyn std::error::Error>> {
    let digest = PackageDigest::read(&b"fixture"[..])?;
    let entry = EnginePackageEntry::portable(
        EnginePackageEnumTarget::LinuxX64,
        7,
        digest,
        "fixture compiler".to_owned(),
    )?;
    let manifest = EnginePackageManifest::new(entry)?;
    let mut value = serde_json::to_value(manifest)?;
    let duplicate = value["packages"][0].clone();
    value["packages"]
        .as_array_mut()
        .ok_or("missing package array")?
        .push(duplicate);
    assert!(serde_json::from_value::<EnginePackageManifest>(value).is_err());
    assert!("A".repeat(64).parse::<PackageDigest>().is_err());
    assert!("0".repeat(63).parse::<PackageDigest>().is_err());
    Ok(())
}
