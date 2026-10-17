use strum::IntoEnumIterator;
use chess::engine::package::host::EnginePackageHost;
use chess::engine::package::manifest::{
    EnginePackageEntry, EnginePackageEnumProfile, EnginePackageEnumTarget, EnginePackageManifest,
};
use chess::engine::package::platform::{
    EnginePackagePlatform, EnginePackagePlatformEnumFeature, EnginePackagePlatformEnumOsState,
    EnginePackagePlatformGlibc,
};
use chess::engine::package::selector::{EnginePackageEnumPreference, EnginePackageSelector};
use chess::package::digest::PackageDigest;

type EnginePackageResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const TARGET: EnginePackageEnumTarget = EnginePackageEnumTarget::LinuxX64;

fn host() -> Result<EnginePackageHost, Box<dyn std::error::Error>> {
    Ok(EnginePackageHost::from_observation(
        TARGET, "22.04", "ubuntu", true,
    )?)
}

fn platform(
    profile: Option<EnginePackageEnumProfile>,
    numa_nodes: u16,
    libnuma: bool,
) -> EnginePackagePlatform {
    let mut features = vec![EnginePackagePlatformEnumFeature::Sse2];
    let mut states = Vec::new();
    if let Some(profile) = profile {
        features.extend(profile.required_features().iter().copied());
        states.extend(profile.required_os_states().iter().copied());
    }
    EnginePackagePlatform::from_observation(features, states, numa_nodes, libnuma)
}

fn package(
    profiles: &[EnginePackageEnumProfile],
) -> Result<(tempfile::TempDir, EnginePackageManifest), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    std::fs::create_dir(directory.path().join("bin"))?;
    let mut manifest: Option<EnginePackageManifest> = None;
    for profile in profiles {
        let bytes = format!("engine {}", profile.label()).into_bytes();
        let path = directory.path().join(TARGET.profile_path(*profile));
        std::fs::write(&path, &bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        }
        let entry = match profile {
            EnginePackageEnumProfile::Portable => EnginePackageEntry::portable(
                TARGET,
                u64::try_from(bytes.len())?,
                PackageDigest::hash_slice(&bytes),
                "fixture compiler".to_owned(),
            )?,
            _ => EnginePackageEntry::profiled(
                TARGET,
                *profile,
                u64::try_from(bytes.len())?,
                PackageDigest::hash_slice(&bytes),
                "fixture compiler".to_owned(),
            )?,
        };
        match &mut manifest {
            Some(manifest) => manifest.replace(entry),
            None => manifest = Some(EnginePackageManifest::new(entry)?),
        }
    }
    Ok((directory, manifest.ok_or("no profiles")?))
}

fn select(
    directory: &tempfile::TempDir,
    manifest: &EnginePackageManifest,
    platform: &EnginePackagePlatform,
    preference: EnginePackageEnumPreference,
) -> Result<
    chess::engine::package::selector::EnginePackageSelected,
    chess::engine::package::selector::EnginePackageSelectorError,
> {
    let host = EnginePackageHost::from_observation(TARGET, "22.04", "ubuntu", true)
        .map_err(|_error| chess::engine::package::selector::EnginePackageSelectorError::MissingPackage)?;
    EnginePackageSelector::select_with(directory.path(), manifest, host, platform, preference)
}

#[test]
fn the_manifest_digest_changes_when_any_byte_changes() {
    let original = br#"{"schema_version":1}"#;
    let tampered = br#"{"schema_version":2}"#;
    assert_eq!(
        EnginePackageManifest::digest(original),
        PackageDigest::hash_slice(original)
    );
    assert_ne!(
        EnginePackageManifest::digest(original),
        EnginePackageManifest::digest(tampered)
    );
}

#[test]
fn the_best_fully_compatible_profile_wins() -> EnginePackageResult {
    host()?;
    let all = [
        EnginePackageEnumProfile::Portable,
        EnginePackageEnumProfile::X8664V3,
        EnginePackageEnumProfile::X8664V4,
    ];
    let (directory, manifest) = package(&all)?;
    for (supported, expected) in [
        (None, EnginePackageEnumProfile::Portable),
        (
            Some(EnginePackageEnumProfile::X8664V3),
            EnginePackageEnumProfile::X8664V3,
        ),
        (
            Some(EnginePackageEnumProfile::X8664V4),
            EnginePackageEnumProfile::X8664V4,
        ),
    ] {
        let selected = select(
            &directory,
            &manifest,
            &platform(supported, 1, false),
            EnginePackageEnumPreference::Automatic,
        )?;
        assert_eq!(selected.profile(), expected);
        assert!(selected.fallback().is_none());
    }
    Ok(())
}

#[test]
fn a_missing_cpu_feature_or_os_register_state_excludes_the_profile() -> EnginePackageResult {
    let v3_features = EnginePackageEnumProfile::X8664V3.required_features().to_vec();
    let without_bmi2: Vec<_> = v3_features
        .iter()
        .copied()
        .filter(|feature| *feature != EnginePackagePlatformEnumFeature::Bmi2)
        .collect();
    let lacking_feature = EnginePackagePlatform::from_observation(
        without_bmi2,
        [EnginePackagePlatformEnumOsState::YmmEnabled],
        1,
        false,
    );
    let reasons =
        EnginePackageSelector::incompatibilities(EnginePackageEnumProfile::X8664V3, &lacking_feature);
    assert_eq!(reasons, vec!["the processor lacks BMI2".to_owned()]);
    let lacking_state = EnginePackagePlatform::from_observation(v3_features, [], 1, false);
    let reasons =
        EnginePackageSelector::incompatibilities(EnginePackageEnumProfile::X8664V3, &lacking_state);
    assert_eq!(reasons.len(), 1);
    assert!(reasons[0].contains("register state"));
    let avx512_without_zmm = EnginePackagePlatform::from_observation(
        EnginePackageEnumProfile::X8664V4.required_features().iter().copied(),
        [EnginePackagePlatformEnumOsState::YmmEnabled],
        1,
        false,
    );
    assert!(
        !EnginePackageSelector::incompatibilities(
            EnginePackageEnumProfile::X8664V4,
            &avx512_without_zmm
        )
        .is_empty()
    );
    Ok(())
}

#[test]
fn an_incompatible_override_falls_back_with_explicit_reasons() -> EnginePackageResult {
    let (directory, manifest) = package(&[
        EnginePackageEnumProfile::Portable,
        EnginePackageEnumProfile::X8664V4,
    ])?;
    let selected = select(
        &directory,
        &manifest,
        &platform(Some(EnginePackageEnumProfile::X8664V3), 1, false),
        EnginePackageEnumPreference::Override(EnginePackageEnumProfile::X8664V4),
    )?;
    assert_eq!(selected.profile(), EnginePackageEnumProfile::Portable);
    let fallback = selected.fallback().ok_or("the fallback is recorded")?;
    assert_eq!(fallback.requested(), EnginePackageEnumProfile::X8664V4);
    assert!(!fallback.reasons().is_empty());
    let unpackaged = select(
        &directory,
        &manifest,
        &platform(Some(EnginePackageEnumProfile::X8664V4), 1, false),
        EnginePackageEnumPreference::Override(EnginePackageEnumProfile::X8664V3),
    )?;
    assert_eq!(unpackaged.profile(), EnginePackageEnumProfile::X8664V4);
    assert_eq!(
        unpackaged.fallback().map(|fallback| fallback.reasons().to_vec()),
        Some(vec!["the profile is not packaged for this host".to_owned()])
    );
    Ok(())
}

#[test]
fn a_compatible_override_is_honored_without_a_fallback() -> EnginePackageResult {
    let (directory, manifest) = package(&[
        EnginePackageEnumProfile::Portable,
        EnginePackageEnumProfile::X8664V3,
    ])?;
    let selected = select(
        &directory,
        &manifest,
        &platform(Some(EnginePackageEnumProfile::X8664V3), 1, false),
        EnginePackageEnumPreference::Override(EnginePackageEnumProfile::Portable),
    )?;
    assert_eq!(selected.profile(), EnginePackageEnumProfile::Portable);
    assert!(selected.fallback().is_none());
    Ok(())
}

#[test]
fn the_numa_profile_needs_two_nodes_and_libnuma() -> EnginePackageResult {
    let (directory, manifest) = package(&[
        EnginePackageEnumProfile::Portable,
        EnginePackageEnumProfile::Numa,
    ])?;
    for (nodes, libnuma, expected) in [
        (1, true, EnginePackageEnumProfile::Portable),
        (2, false, EnginePackageEnumProfile::Portable),
        (2, true, EnginePackageEnumProfile::Numa),
    ] {
        let selected = select(
            &directory,
            &manifest,
            &platform(None, nodes, libnuma),
            EnginePackageEnumPreference::Automatic,
        )?;
        assert_eq!(selected.profile(), expected, "{nodes} nodes, libnuma {libnuma}");
    }
    Ok(())
}

#[test]
fn builds_outside_a_targets_supported_profiles_are_rejected() -> EnginePackageResult {
    let digest = PackageDigest::hash_slice(b"engine");
    for (target, profile) in [
        (EnginePackageEnumTarget::MacosArm, EnginePackageEnumProfile::X8664V3),
        (EnginePackageEnumTarget::MacosArm, EnginePackageEnumProfile::Numa),
        (EnginePackageEnumTarget::WindowsX64, EnginePackageEnumProfile::Numa),
        (EnginePackageEnumTarget::WindowsX64, EnginePackageEnumProfile::AppleNeon),
        (EnginePackageEnumTarget::LinuxX64, EnginePackageEnumProfile::AppleNeon),
    ] {
        assert!(
            EnginePackageEntry::profiled(target, profile, 6, digest, "compiler".to_owned())
                .is_err(),
            "{target:?} must not accept {profile:?}"
        );
    }
    assert!(
        EnginePackageEntry::profiled(
            EnginePackageEnumTarget::WindowsX64,
            EnginePackageEnumProfile::X8664V4,
            6,
            digest,
            "compiler".to_owned()
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn the_apple_neon_profile_needs_neon_and_ranks_above_the_portable_baseline() -> EnginePackageResult {
    let profile = EnginePackageEnumProfile::AppleNeon;
    assert!(EnginePackageEnumTarget::MacosArm.supports_profile(profile));
    assert!(profile.rank() > EnginePackageEnumProfile::Portable.rank());
    assert!(profile.rustflags().is_some_and(|flags| flags.contains("+neon")));
    let without_neon = EnginePackagePlatform::from_observation([], [], 1, false);
    assert_eq!(
        EnginePackageSelector::incompatibilities(profile, &without_neon),
        vec!["the processor lacks NEON".to_owned()]
    );
    let with_neon = EnginePackagePlatform::from_observation(
        [EnginePackagePlatformEnumFeature::Neon],
        [],
        1,
        false,
    );
    assert!(EnginePackageSelector::incompatibilities(profile, &with_neon).is_empty());
    assert!(
        EnginePackageEntry::profiled(
            EnginePackageEnumTarget::MacosArm,
            profile,
            6,
            PackageDigest::hash_slice(b"engine"),
            "compiler".to_owned()
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn a_tampered_profile_binary_fails_its_checksum() -> EnginePackageResult {
    let (directory, manifest) = package(&[EnginePackageEnumProfile::X8664V3])?;
    std::fs::write(
        directory
            .path()
            .join(TARGET.profile_path(EnginePackageEnumProfile::X8664V3)),
        b"swapped",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            directory
                .path()
                .join(TARGET.profile_path(EnginePackageEnumProfile::X8664V3)),
            std::fs::Permissions::from_mode(0o700),
        )?;
    }
    assert!(
        select(
            &directory,
            &manifest,
            &platform(Some(EnginePackageEnumProfile::X8664V3), 1, false),
            EnginePackageEnumPreference::Automatic,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn every_profile_has_a_distinct_path_and_serialization() -> EnginePackageResult {
    let mut paths = std::collections::HashSet::new();
    for profile in EnginePackageEnumProfile::iter() {
        assert!(paths.insert(TARGET.profile_path(profile)));
        let json = serde_json::to_string(&profile)?;
        let restored: EnginePackageEnumProfile = serde_json::from_str(&json)?;
        assert_eq!(restored, profile);
    }
    assert_eq!(paths.len(), EnginePackageEnumProfile::iter().len());
    assert_eq!(
        TARGET.profile_path(EnginePackageEnumProfile::Portable),
        TARGET.relative_path()
    );
    Ok(())
}

#[test]
fn glibc_older_than_the_package_baseline_blocks_every_profile_with_a_reason() -> EnginePackageResult {
    assert_eq!(
        EnginePackagePlatformGlibc::parse("glibc 2.35\n"),
        Some(EnginePackagePlatformGlibc::new(2, 35))
    );
    assert_eq!(EnginePackagePlatformGlibc::parse("getconf: unknown"), None);
    assert!(EnginePackagePlatformGlibc::new(2, 34) < EnginePackagePlatformGlibc::REQUIRED);
    let old = platform(None, 1, false).with_glibc(Some(EnginePackagePlatformGlibc::new(2, 34)));
    let reasons = EnginePackageSelector::incompatibilities(EnginePackageEnumProfile::Portable, &old);
    assert_eq!(
        reasons,
        vec!["glibc 2.34 is older than the required 2.35".to_owned()]
    );
    let current =
        platform(None, 1, false).with_glibc(Some(EnginePackagePlatformGlibc::new(2, 39)));
    assert!(
        EnginePackageSelector::incompatibilities(EnginePackageEnumProfile::Portable, &current)
            .is_empty()
    );
    let unknown = platform(None, 1, false);
    assert!(unknown.glibc().is_none());
    assert!(
        EnginePackageSelector::incompatibilities(EnginePackageEnumProfile::Portable, &unknown)
            .is_empty()
    );
    let (directory, manifest) = package(&[EnginePackageEnumProfile::Portable])?;
    assert!(
        EnginePackageSelector::select_with(
            directory.path(),
            &manifest,
            host()?,
            &old,
            EnginePackageEnumPreference::Automatic
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn profile_and_target_spellings_are_unchanged_and_the_iterator_matches_the_registry()
-> EnginePackageResult {
    let labels = [
        (EnginePackageEnumProfile::Portable, "Portable baseline"),
        (EnginePackageEnumProfile::X8664V3, "x86-64-v3"),
        (EnginePackageEnumProfile::X8664V4, "x86-64-v4"),
        (EnginePackageEnumProfile::Numa, "Linux NUMA"),
        (EnginePackageEnumProfile::AppleNeon, "Apple ARM64 NEON"),
    ];
    for (profile, label) in labels {
        assert_eq!(profile.label(), label);
        assert_eq!(label.parse::<EnginePackageEnumProfile>()?, profile);
    }
    for target in EnginePackageEnumTarget::iter() {
        assert_eq!(target.triple().parse::<EnginePackageEnumTarget>()?, target);
        assert_eq!(serde_json::to_string(&target)?, format!("\"{}\"", target.triple()));
    }
    for feature in EnginePackagePlatformEnumFeature::iter() {
        assert_eq!(feature.label().parse::<EnginePackagePlatformEnumFeature>()?, feature);
    }
    for state in EnginePackagePlatformEnumOsState::iter() {
        assert_eq!(state.label().parse::<EnginePackagePlatformEnumOsState>()?, state);
    }
    assert_eq!(EnginePackagePlatformEnumFeature::Avx512bw.label(), "AVX-512BW");
    Ok(())
}
